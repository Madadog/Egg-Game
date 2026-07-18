//! Native asset hot-reload. A coarse mtime poller re-reads the game's authoring
//! files when they change on disk (an external editor, a `git checkout`) and
//! re-applies them through the **same parse+install seams** the in-game editors
//! use for their own live-reload — so editing `en.eggtext` in vim lands exactly
//! like editing it in the F2 editor. A parse error leaves the running state on
//! the last good version and logs loudly, matching the F2 editor's save
//! semantics (last-good-wins).
//!
//! What's watched (as the host names files, under `assets/`):
//! * `script/en.eggtext` → [`egg_core::data::script::eggtext::parse`] +
//!   `Script::set_base`;
//! * `data/main.eggscene` **and** `data/recorded.eggscene` (the path
//!   recorder's machine-owned counterpart — see the `.eggscene` module doc) →
//!   both re-parsed with [`egg_core::data::scene::parse`], merged with
//!   [`egg_core::data::scene::SceneFile::merge`], and installed via
//!   `EggState::set_scenes`; either file changing re-parses and re-merges
//!   both, so an edit to one can never wipe the other's content out of the
//!   live registry. `recorded.eggscene` not existing yet is not an error — it
//!   contributes an empty source;
//! * `data/data.toml` → `EggState::reload_data` (item/preset/portrait
//!   registries — a portrait reload also re-bakes any already-installed
//!   dialogue via `Script::reresolve_portraits`);
//! * every `.tmj` in the **maps directory** (`assets/maps/`, scanned the same
//!   way [`crate::fantasy_console::map_stems`] discovers the boot set — not
//!   the store, so a brand-new map becomes watched too) →
//!   [`egg_core::data::tiled::from_json`] re-derived into the `MapStore`
//!   (image-layer pixels carried over, and the live map rebuilt when it's the
//!   one the player is standing on). Scanning the directory rather than the
//!   store is what makes a brand-new map hot-*addable* — see the kind-aware
//!   first-sight rule in [`poll_hot_reload`];
//! * `sprites/sheet.png` → decoded with [`egg_game_headless::decode_png`] (the
//!   same Bevy-free `image`-crate call the headless harness boots with) and
//!   re-derived into both sheet forms `DrawState` keeps (RGBA + indexed via
//!   `RgbaImage::to_indexed`). Installed on the main `DrawState`, cloned into
//!   every open extra view's own `DrawState` (each view owns its sheets
//!   independently, see `src/views.rs`), and the live map's sprite-plane
//!   derivations are rebuilt (they're baked from sheet pixels at load time);
//! * every loaded map's image-layer PNGs (`maps/images/*.png`, path as
//!   authored in the `.tmj`) → decoded with the same
//!   [`egg_game_headless::decode_png`] seam and reattached via
//!   [`egg_core::data::tiled::TiledMap::attach_image`] to every map that
//!   references that path (a background/mask PNG can be shared), with the
//!   live map rebuilt if it was one of them. Unlike the `.tmj` set above, this
//!   list is still store-derived — only a *parsed* map knows its image paths;
//! * every `.ogg` in the **sfx directory** (`assets/sfx/`, scanned the same
//!   way [`crate::fantasy_console::sfx_stems`] discovers the boot set, so a
//!   brand-new sound becomes watched too) → an already-loaded stem is
//!   re-read in place via `AssetServer::reload`, so the existing
//!   `Handle<AudioSource>` in [`crate::fantasy_console::SfxAssets`] resolves
//!   to the new bytes on its next cue; a stem not yet in `SfxAssets` is
//!   `assets.load`ed and inserted, so a brand-new sound is playable
//!   immediately — mirroring the `.tmj` set's kind-aware first-sight rule,
//!   see [`poll_hot_reload`].
//!
//! Deliberately NOT watched: the save (`save.json`, user data the running game
//! rewrites constantly) and the one remaining binary art asset — the font.
//! Nothing technical rules it out — the same [`egg_game_headless::decode_png`]
//! seam would decode it too — it's just not wired up yet.
//!
//! A `.tmj` that vanishes (deleted, or mid-write unlinked by an external tool)
//! is never removed from the store by this poller: [`mtime`] reports a missing
//! file as "unchanged", so the poller just stops seeing it change. A live map's
//! removal only ever happens through the editor's own `MapStore::remove`
//! (`delete_map`) or by not being present at the next boot — a transient
//! missing-file stat must never nuke a map the player is standing on. An
//! `.ogg` that vanishes is the same story: its `Handle<AudioSource>` stays in
//! [`crate::fantasy_console::SfxAssets`] and keeps playing whatever bytes the
//! `AssetServer` last had cached until the app restarts — this poller never
//! removes a sound either.
//!
//! Native only: web has no filesystem to poll — its editor-write persistence is
//! `localStorage`, handled in [`crate::fantasy_console`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bevy::prelude::*;
use egg_core::data::tiled::{self, TiledMapLayer};

use crate::EggGame;

/// How often the poller stats the watched files, in seconds. Coarse on purpose:
/// hot-reload is an authoring convenience, not a per-frame concern, and statting
/// a handful of files every second is free.
const POLL_INTERVAL: f32 = 1.0;

/// What reload a changed *engine* path triggers. The variants mirror the four
/// parse+install seams the startup pipeline and in-game editors already use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadKind {
    /// `script/en.eggtext` — re-parse to a `ScriptFile`, install via `set_base`.
    Script,
    /// `data/main.eggscene` — re-parse to a `SceneFile`, install via `set_scenes`.
    Scenes,
    /// `data/data.toml` — re-run `EggState::reload_data` (item/preset/portrait
    /// registries).
    Data,
    /// `maps/<name>.tmj` — re-parse to a `TiledMap`, re-derive into the `MapStore`.
    Map(String),
    /// `sprites/sheet.png` — re-decode, re-derive RGBA + indexed forms, and
    /// reinstall on the main `DrawState`, every open view, and the live map.
    Sheet,
    /// `maps/<rel>.png` where `<rel>` is a map image-layer's authored path
    /// (e.g. `images/bedroom1_mask.png`, carrying subdirectories as authored)
    /// — re-decode and reattach to every map layer referencing it.
    MapImage(String),
    /// `sfx/<stem>.ogg` — re-read via `AssetServer::reload` if already loaded
    /// (the existing handle then resolves to the new bytes), or `load` +
    /// insert into `SfxAssets` if this is a brand-new stem.
    Sfx(String),
}

/// Classify a watched **engine** path (as the host names files: forward-slash,
/// relative to `assets/`) into the reload it triggers, or `None` if the path is
/// outside the hot-reloadable set. Pure, so the poller uses it both to build its
/// watch set and to dispatch a change — the two can't drift.
pub fn classify(engine_path: &str) -> Option<ReloadKind> {
    match engine_path {
        "script/en.eggtext" => Some(ReloadKind::Script),
        "data/main.eggscene" | "data/recorded.eggscene" => Some(ReloadKind::Scenes),
        // Kept in sync with `egg_core::data::eggdata::DATA_PATH` (asserted in tests).
        "data/data.toml" => Some(ReloadKind::Data),
        "sprites/sheet.png" => Some(ReloadKind::Sheet),
        other if other.starts_with("maps/") => other.strip_prefix("maps/").and_then(|rest| {
            if let Some(name) = rest.strip_suffix(".tmj") {
                (!name.is_empty() && !name.contains('/')).then(|| ReloadKind::Map(name.to_string()))
            } else if rest
                .strip_suffix(".png")
                .is_some_and(|name| !name.is_empty())
            {
                // The rel path as authored in the `.tmj`, which may nest under a
                // subdirectory (`images/…`) — unlike `.tmj` names, that's expected.
                Some(ReloadKind::MapImage(rest.to_string()))
            } else {
                None
            }
        }),
        other => other.strip_prefix("sfx/").and_then(|rest| {
            // Same "no subdirectories" rule as `.tmj` names — `sfx_stems` only
            // ever scans the flat `assets/sfx/` directory, so a nested path
            // could never match a loaded stem.
            let name = rest.strip_suffix(".ogg")?;
            (!name.is_empty() && !name.contains('/')).then(|| ReloadKind::Sfx(name.to_string()))
        }),
    }
}

/// Poller state: elapsed accumulator, per-path last-seen mtime, and whether the
/// initial snapshot has been taken.
#[derive(Resource, Default)]
pub struct HotReload {
    /// Seconds accumulated since the last poll; fires at [`POLL_INTERVAL`].
    elapsed: f32,
    /// Last-seen mtime per watched engine path. A path first seen on a poll is
    /// recorded, and — for the kinds with an "add" seam (`.tmj` maps, `.ogg`
    /// sounds) — dispatched too, which is the hot-add path; every other kind is
    /// record-only on first sight (see the match in [`poll_hot_reload`]).
    seen: HashMap<String, SystemTime>,
    /// Whether the initial mtime snapshot has been taken (once assets finish
    /// loading). Until then there's nothing meaningful to compare against.
    primed: bool,
}

/// Resolve an engine path (`maps/house.tmj`) to its on-disk location under
/// `assets/`, mirroring [`crate::fantasy_console`]'s `asset_path`. The engine
/// only ever names relative forward-slash paths, so a plain join is safe here.
fn asset_fs_path(engine_path: &str) -> PathBuf {
    Path::new("assets").join(engine_path)
}

/// The file's last-modified time, or `None` if it doesn't exist / can't be
/// stat'd (treated as "unchanged" — a mid-write unlink shouldn't reload).
fn mtime(engine_path: &str) -> Option<SystemTime> {
    std::fs::metadata(asset_fs_path(engine_path))
        .and_then(|m| m.modified())
        .ok()
}

/// The set of engine paths to watch this poll: the four fixed authoring files,
/// plus the `.tmj` set discovered by scanning the maps directory (mirroring
/// `fantasy_console::map_stems`'s native path — *not* derived from the store,
/// so a brand-new file becomes watched as soon as it lands on disk, before
/// anything has loaded it), plus the image-layer PNGs of every *loaded* map
/// (this list stays store-derived — only a parsed map knows its image paths),
/// plus the `.ogg` set discovered by scanning the sfx directory (mirroring
/// `fantasy_console::sfx_stems`'s native path, same not-store-derived reasoning
/// as the map set — a brand-new sound becomes watched before anything has
/// loaded it). Image paths are deduped — two maps sharing a background/mask
/// PNG should still dispatch one reload, not one per map.
fn watch_paths(game: &EggGame) -> Vec<String> {
    let mut paths = vec![
        "script/en.eggtext".to_string(),
        "data/main.eggscene".to_string(),
        "data/recorded.eggscene".to_string(),
        egg_core::data::eggdata::DATA_PATH.to_string(),
        "sprites/sheet.png".to_string(),
    ];
    for name in crate::fantasy_console::map_stems() {
        paths.push(format!("maps/{name}.tmj"));
    }
    for stem in crate::fantasy_console::sfx_stems() {
        paths.push(format!("sfx/{stem}.ogg"));
    }
    let mut images = std::collections::HashSet::new();
    for name in game.state.maps.names() {
        if let Some(map) = game.state.maps.get(name) {
            for rel in map.image_layer_paths() {
                if images.insert(rel.to_string()) {
                    paths.push(format!("maps/{rel}"));
                }
            }
        }
    }
    paths
}

/// Coarse-timer poll: stat every watched file, and on a changed mtime re-run the
/// matching load path. Runs on `Update` (not the fixed sim step) — reloading is
/// host bookkeeping, independent of the sim clock. Takes `ViewWindows` (always
/// present, `init_resource`'d by `ViewsPlugin`) so a sheet reload can refresh
/// every open view's own sheet clones alongside the main `DrawState`. Takes
/// `AssetServer` and `SfxAssets` (both always present once the app is up —
/// `SfxAssets` is inserted at `Startup` by `setup_assets`) for the sfx arm.
pub fn poll_hot_reload(
    time: Res<Time>,
    mut hot: ResMut<HotReload>,
    mut game: ResMut<EggGame>,
    mut views: ResMut<crate::views::ViewWindows>,
    assets: Res<AssetServer>,
    mut sfx: ResMut<crate::fantasy_console::SfxAssets>,
) {
    if !game.loaded {
        return;
    }
    hot.elapsed += time.delta_secs();
    if hot.elapsed < POLL_INTERVAL {
        return;
    }
    hot.elapsed = 0.0;

    let paths = watch_paths(&game);

    // First poll after load: snapshot current mtimes without reloading — the
    // running state already reflects them.
    if !hot.primed {
        for path in &paths {
            if let Some(mtime) = mtime(path) {
                hot.seen.insert(path.clone(), mtime);
            }
        }
        hot.primed = true;
        return;
    }

    // Detect changes first (recording new mtimes), then apply — so the borrow of
    // `game` for `watch_paths` is released before `apply_reload` takes `&mut`.
    let mut changed = Vec::new();
    for path in paths {
        let Some(mtime) = mtime(&path) else { continue };
        match hot.seen.get(&path) {
            Some(prev) if *prev == mtime => {}
            Some(_) => {
                hot.seen.insert(path.clone(), mtime);
                changed.push(path);
            }
            None => {
                hot.seen.insert(path.clone(), mtime);
                // First sight after priming: a `.tmj` or an `.ogg` is dispatched
                // right away — this is the hot-*add* path, since the maps/sfx
                // directories (not the store/`SfxAssets`) are the watch sets now,
                // a brand-new file must actually reach its registry to become
                // usable in-game. An editor-created map is inserted into the
                // store *before* its file is written (see
                // `MapViewer::create_map`), so the re-parse this triggers is a
                // harmless no-op re-install, not a race; a brand-new sound has no
                // such registration step, so this dispatch *is* what makes it
                // playable. Every other kind keeps the original
                // record-only-on-first-sight rule: the store has nothing to
                // receive them (there's no equivalent "add" for the four fixed
                // files or an image PNG with no map yet referencing it).
                if matches!(
                    classify(&path),
                    Some(ReloadKind::Map(_)) | Some(ReloadKind::Sfx(_))
                ) {
                    changed.push(path);
                }
            }
        }
    }
    for path in changed {
        if let Some(kind) = classify(&path) {
            apply_reload(&mut game, &mut views, &assets, &mut sfx, &path, kind);
        }
    }
}

/// Dispatch a detected change to the matching parse+install seam.
fn apply_reload(
    game: &mut EggGame,
    views: &mut crate::views::ViewWindows,
    assets: &AssetServer,
    sfx: &mut crate::fantasy_console::SfxAssets,
    engine_path: &str,
    kind: ReloadKind,
) {
    match kind {
        ReloadKind::Script => reload_script(game, engine_path),
        ReloadKind::Scenes => reload_scenes(game, engine_path),
        ReloadKind::Data => {
            game.state.reload_data(&mut game.system);
            info!("Hot-reloaded {engine_path}");
        }
        ReloadKind::Map(name) => reload_map(game, engine_path, &name),
        ReloadKind::Sheet => reload_sheet(game, views, engine_path),
        ReloadKind::MapImage(rel) => reload_map_image(game, engine_path, &rel),
        ReloadKind::Sfx(stem) => reload_sfx(assets, sfx, engine_path, &stem),
    }
}

/// Read a watched text file to a `String`; `None` (with a warning) on an I/O
/// error or non-UTF-8 content — the caller keeps the last good version.
fn read_text(engine_path: &str) -> Option<String> {
    match std::fs::read(asset_fs_path(engine_path)) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => Some(s),
            Err(e) => {
                warn!("Hot-reload: {engine_path} is not valid UTF-8: {e}");
                None
            }
        },
        Err(e) => {
            warn!("Hot-reload: failed to read {engine_path}: {e}");
            None
        }
    }
}

/// Re-parse `en.eggtext` and install it as the base script (same seam as the F2
/// editor's `pending_script` drain). Parse error: log and keep the last good.
fn reload_script(game: &mut EggGame, path: &str) {
    let Some(src) = read_text(path) else { return };
    match egg_core::data::script::eggtext::parse(&src) {
        Ok(file) => {
            game.state.script.set_base(file, &game.state.portraits);
            info!("Hot-reloaded {path}");
        }
        Err(e) => warn!("Hot-reload: invalid eggtext in {path}: {e}"),
    }
}

/// Re-parse **both** `main.eggscene` and `recorded.eggscene` and install the
/// merged cutscene registry (same seam as the F2 editor's `pending_scene`
/// drain) — `path` is whichever of the two just changed, but both are always
/// re-read, since a stale copy of the other would otherwise silently drop out
/// of the live registry (see the `.eggscene` module doc). A missing
/// `recorded.eggscene` is not an error (it may not exist yet) and contributes
/// an empty source; an unparseable file, or a merge conflict between the two,
/// logs and keeps the last good registry.
fn reload_scenes(game: &mut EggGame, path: &str) {
    let Some(changed_src) = read_text(path) else {
        return;
    };
    let changed = match egg_core::data::scene::parse(&changed_src) {
        Ok(file) => file,
        Err(e) => {
            warn!("Hot-reload: invalid eggscene in {path}: {e}");
            return;
        }
    };
    let other_path = if path == egg_core::data::scene::MAIN_SCENE_PATH {
        egg_core::data::scene::RECORDED_SCENE_PATH
    } else {
        egg_core::data::scene::MAIN_SCENE_PATH
    };
    let other = match std::fs::read_to_string(asset_fs_path(other_path)) {
        Ok(src) => match egg_core::data::scene::parse(&src) {
            Ok(file) => file,
            Err(e) => {
                warn!("Hot-reload: invalid eggscene in {other_path}: {e}");
                return;
            }
        },
        Err(_) => egg_core::data::scene::SceneFile::default(),
    };
    let (main, recorded) = if path == egg_core::data::scene::MAIN_SCENE_PATH {
        (changed, other)
    } else {
        (other, changed)
    };
    match main.merge(recorded) {
        Ok(merged) => {
            game.state.set_scenes(merged);
            info!("Hot-reloaded {path}");
        }
        Err(e) => warn!("Hot-reload: scene merge conflict ({path} vs its counterpart): {e}"),
    }
}

/// Re-parse a `maps/<name>.tmj` into the `MapStore`. `from_json` yields a map
/// with no image-layer pixels, so the ones already in the store are carried over
/// (the same transplant the in-game editor's `sync_store` does) — an external
/// tile/object edit must not blank a painted background/collision-mask layer. If
/// the reloaded map is the one the player is standing on, the live `MapInfo` is
/// re-derived so the edit shows without a re-warp (mirroring the editor's
/// `pending_reload` rebuild). Parse/IO error: log and keep the last good.
fn reload_map(game: &mut EggGame, path: &str, name: &str) {
    let bytes = match std::fs::read(asset_fs_path(path)) {
        Ok(b) => b,
        Err(e) => {
            warn!("Hot-reload: failed to read {path}: {e}");
            return;
        }
    };
    let mut fresh = match tiled::from_json(&bytes) {
        Ok(m) => m,
        Err(e) => {
            warn!("Hot-reload: invalid map {path}: {e}");
            return;
        }
    };
    if let Some(old) = game.state.maps.get(name) {
        let pixels: Vec<(String, _)> = old
            .layers
            .iter()
            .filter_map(|layer| match layer {
                TiledMapLayer::ImageLayer(image) => Some((image.image.clone(), image.pixels.clone()?)),
                _ => None,
            })
            .collect();
        for (img_path, px) in pixels {
            fresh.attach_image(&img_path, px);
        }
    }
    game.state.maps.insert(name, fresh);
    info!("Hot-reloaded map {name}");

    rebuild_live_map(game, name);
}

/// If `name` is the map the player is currently standing on, re-derive its live
/// `MapInfo` from the current `MapStore`/sprite sheet and splice the refreshed
/// fields into `walkaround.current_map` — so an edit to either the map file or
/// the sprite sheet it draws from shows up without a re-warp. A no-op for any
/// other map. Shared by [`reload_map`] (the map changed) and [`reload_sheet`]
/// (the sheet the map's sprite-plane layers are derived from changed) so the
/// two rebuild paths can't drift.
fn rebuild_live_map(game: &mut EggGame, name: &str) {
    if game.state.walkaround.current_map.source != name {
        return;
    }
    let rebuilt = egg_core::world::map::map_by_name(
        &game.state.draw_state.indexed_sprites,
        name,
        &game.state.maps,
    );
    if let Some(fresh) = rebuilt {
        game.state.walkaround.current_map.bg_colour = fresh.bg_colour;
        game.state.walkaround.current_map.camera_bounds = fresh.camera_bounds;
        game.state.walkaround.current_map.layers = fresh.layers;
        game.state.walkaround.current_map.fg_layers = fresh.fg_layers;
        game.state.walkaround.current_map.sprite_layers = fresh.sprite_layers;
        game.state.walkaround.current_map.sprite_components = fresh.sprite_components;
    }
}

/// Re-decode a map image-layer PNG (`maps/<rel>`, `rel` as authored in the
/// `.tmj`) and reattach it via
/// [`egg_core::data::tiled::TiledMap::attach_image`] to every map in the store
/// that references `rel` — a background/mask PNG shared by several maps gets
/// the fresh pixels on all of them from one file change. If any affected map
/// is the one the player is standing on, its live `MapInfo` is rebuilt via
/// [`rebuild_live_map`] so the repaint or collision-mask edit shows without a
/// re-warp. Decode/IO error: log and keep the last good pixels, matching every
/// other arm.
fn reload_map_image(game: &mut EggGame, path: &str, rel: &str) {
    let bytes = match std::fs::read(asset_fs_path(path)) {
        Ok(b) => b,
        Err(e) => {
            warn!("Hot-reload: failed to read {path}: {e}");
            return;
        }
    };
    let pixels = match egg_game_headless::decode_png(&bytes) {
        Ok(img) => img,
        Err(e) => {
            warn!("Hot-reload: invalid map image {path}: {e}");
            return;
        }
    };

    let affected: Vec<String> = game
        .state
        .maps
        .names()
        .iter()
        .filter(|name| {
            game.state
                .maps
                .get(name)
                .is_some_and(|map| map.image_layer_paths().contains(&rel))
        })
        .map(|name| name.to_string())
        .collect();
    for name in &affected {
        if let Some(map) = game.state.maps.get_mut(name) {
            map.attach_image(rel, pixels.clone());
        }
    }
    info!("Hot-reloaded {path}");

    let current = game.state.walkaround.current_map.source.clone();
    if affected.iter().any(|name| *name == current) {
        rebuild_live_map(game, &current);
    }
}

/// Re-decode `sprites/sheet.png` and re-derive both sheet forms `DrawState`
/// keeps: the RGBA form installs as-is, the indexed form is re-matched against
/// the active palette (same `to_indexed` policy the loader and headless harness
/// use). Installed on the main `DrawState`, then cloned into every open extra
/// view's own `DrawState` (each view holds independent sheet copies, see
/// `src/views.rs`) — a view left on the stale sheet would draw the old art.
/// Finally the live map's sprite-plane derivations are rebuilt via
/// [`rebuild_live_map`], since those are baked from sheet pixels. Decode/IO
/// error: log and keep the last good sheet, matching every other arm.
fn reload_sheet(game: &mut EggGame, views: &mut crate::views::ViewWindows, path: &str) {
    let bytes = match std::fs::read(asset_fs_path(path)) {
        Ok(b) => b,
        Err(e) => {
            warn!("Hot-reload: failed to read {path}: {e}");
            return;
        }
    };
    let sheet = match egg_game_headless::decode_png(&bytes) {
        Ok(img) => img,
        Err(e) => {
            warn!("Hot-reload: invalid sprite sheet {path}: {e}");
            return;
        }
    };
    let palette = game.state.draw_state.palettes[0].clone();
    let indexed = sheet.to_indexed(&palette);

    for view in &mut views.views {
        view.draw_state.rgba_sprites = sheet.clone();
        view.draw_state.indexed_sprites = indexed.clone();
    }
    game.state.draw_state.rgba_sprites = sheet;
    game.state.draw_state.indexed_sprites = indexed;

    let name = game.state.walkaround.current_map.source.clone();
    rebuild_live_map(game, &name);

    info!("Hot-reloaded {path}");
}

/// Re-read or newly load `sfx/<stem>.ogg`. Bevy audio decodes on *play*, not on
/// load — an `AudioSource` asset just holds the encoded bytes, and rodio
/// decodes them fresh each time a cue spawns a player — so there's no
/// despawn/rewire step here, unlike the sheet or a map:
///
/// * a stem already in [`crate::fantasy_console::SfxAssets`] (an edited file)
///   is re-read in place via `AssetServer::reload`. The existing
///   `Handle<AudioSource>` other systems (`play_sounds`) already hold stays
///   valid and just resolves to the new bytes, so the very next `#sound` cue
///   for that stem plays the edit — no further plumbing needed.
/// * a stem not yet in `SfxAssets` (a brand-new file) is `assets.load`ed and
///   inserted, so a `#sound` cue naming it finds a handle immediately instead
///   of warning "no sound named" (see `play_sounds`).
///
/// Both legs are fire-and-forget: the actual disk read and decode happen
/// asynchronously on Bevy's IO task pool, same as any other `assets.load`, so
/// there's no I/O error to handle here the way the other arms do — a bad
/// `.ogg` surfaces later as an `AssetLoadError` event this poller doesn't
/// listen for, matching how any other malformed bundled sfx would fail today.
fn reload_sfx(
    assets: &AssetServer,
    sfx: &mut crate::fantasy_console::SfxAssets,
    path: &str,
    stem: &str,
) {
    // `AssetPath`'s `&str` conversion is `&'static str`-only (an owned path
    // borrowed for `'static`), so a path built at runtime goes through `String`
    // instead — same as every other `assets.load(format!(...))` call site.
    if sfx.sounds.contains_key(stem) {
        assets.reload(path.to_string());
    } else {
        sfx.sounds
            .insert(stem.to_string(), assets.load(path.to_string()));
    }
    info!("Hot-reloaded {path}");
}

/// Native asset hot-reload: registers the poller and its state.
pub struct HotReloadPlugin;

impl Plugin for HotReloadPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HotReload>()
            .add_systems(Update, poll_hot_reload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_known_assets() {
        assert_eq!(classify("script/en.eggtext"), Some(ReloadKind::Script));
        assert_eq!(classify("data/main.eggscene"), Some(ReloadKind::Scenes));
        assert_eq!(classify("data/recorded.eggscene"), Some(ReloadKind::Scenes));
        // Both scene-source arms must stay in step with the engine's constants.
        assert_eq!(
            classify(egg_core::data::scene::MAIN_SCENE_PATH),
            Some(ReloadKind::Scenes)
        );
        assert_eq!(
            classify(egg_core::data::scene::RECORDED_SCENE_PATH),
            Some(ReloadKind::Scenes)
        );
        assert_eq!(classify("data/data.toml"), Some(ReloadKind::Data));
        // The Data arm must stay in step with the engine's constant.
        assert_eq!(
            classify(egg_core::data::eggdata::DATA_PATH),
            Some(ReloadKind::Data)
        );
        assert_eq!(
            classify("maps/house.tmj"),
            Some(ReloadKind::Map("house".to_string()))
        );
        assert_eq!(
            classify("maps/house_kitchen2.tmj"),
            Some(ReloadKind::Map("house_kitchen2".to_string()))
        );
        assert_eq!(classify("sprites/sheet.png"), Some(ReloadKind::Sheet));
        // Map image-layer PNGs, keyed by their rel path as authored in the
        // `.tmj` — unlike `.tmj` names, a subdirectory is expected (`images/`).
        assert_eq!(
            classify("maps/images/bedroom1_mask.png"),
            Some(ReloadKind::MapImage("images/bedroom1_mask.png".to_string()))
        );
        assert_eq!(
            classify("maps/images/nested/dir/odd.name.png"),
            Some(ReloadKind::MapImage(
                "images/nested/dir/odd.name.png".to_string()
            ))
        );
        assert_eq!(
            classify("sfx/13_door.ogg"),
            Some(ReloadKind::Sfx("13_door".to_string()))
        );
    }

    #[test]
    fn rejects_unwatched_and_malformed() {
        // User data is never hot-reloaded; the font isn't wired up yet (unlike
        // the sheet, see `classifies_known_assets`).
        assert_eq!(classify("save.json"), None);
        assert_eq!(classify("fonts/tic80_font.png"), None);
        // A different language isn't the base script.
        assert_eq!(classify("script/fr.eggtext"), None);
        // Malformed map paths.
        assert_eq!(classify("maps/.tmj"), None);
        assert_eq!(classify("maps/sub/dir.tmj"), None);
        assert_eq!(classify("maps/house.tmj.bak"), None);
        // `maps/house.png` (no subdirectory) is *not* malformed — a map image
        // rel path isn't required to live under `images/`, only `.tmj` names
        // are barred from nesting. See `classifies_known_assets`.
        assert_eq!(
            classify("maps/house.png"),
            Some(ReloadKind::MapImage("house.png".to_string()))
        );
        // Malformed map image paths.
        assert_eq!(classify("maps/images/mask.jpg"), None);
        assert_eq!(classify("maps/.png"), None);
        // A PNG outside `maps/` is never a map image (the sheet's own literal
        // arm is what makes `sprites/sheet.png` classify as `Sheet`, not this
        // one — see `classifies_known_assets`).
        assert_eq!(classify("images/mask.png"), None);
        // Malformed sfx paths.
        assert_eq!(classify("sfx/.ogg"), None);
        assert_eq!(classify("sfx/sub/dir.ogg"), None);
        assert_eq!(classify("sfx/13_door.wav"), None);
        // Music isn't watched — only the sfx directory is.
        assert_eq!(classify("music/intro.ogg"), None);
    }
}

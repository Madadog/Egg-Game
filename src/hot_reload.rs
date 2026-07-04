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
//! * `data/main.eggscene` → [`egg_core::data::scene::parse`] +
//!   `EggState::set_scenes`;
//! * `data/data.toml` → `EggState::reload_data`;
//! * every loaded map's `maps/<name>.tmj` → [`egg_core::data::tiled::from_json`]
//!   re-derived into the `MapStore` (image-layer pixels carried over, and the
//!   live map rebuilt when it's the one the player is standing on).
//!
//! Deliberately NOT watched: the save (`save.json`, user data the running game
//! rewrites constantly) and the binary art (sprite sheet / font) — re-decoding a
//! PNG outside Bevy's async loader would mean either a new dependency or enabling
//! the `file_watcher` feature (which pulls `notify`), both out of scope here.
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
    /// `data/data.toml` — re-run `EggState::reload_data` (item/preset registries).
    Data,
    /// `maps/<name>.tmj` — re-parse to a `TiledMap`, re-derive into the `MapStore`.
    Map(String),
}

/// Classify a watched **engine** path (as the host names files: forward-slash,
/// relative to `assets/`) into the reload it triggers, or `None` if the path is
/// outside the hot-reloadable set. Pure, so the poller uses it both to build its
/// watch set and to dispatch a change — the two can't drift.
pub fn classify(engine_path: &str) -> Option<ReloadKind> {
    match engine_path {
        "script/en.eggtext" => Some(ReloadKind::Script),
        "data/main.eggscene" => Some(ReloadKind::Scenes),
        // Kept in sync with `egg_core::data::eggdata::DATA_PATH` (asserted in tests).
        "data/data.toml" => Some(ReloadKind::Data),
        other => other
            .strip_prefix("maps/")
            .and_then(|rest| rest.strip_suffix(".tmj"))
            .filter(|name| !name.is_empty() && !name.contains('/'))
            .map(|name| ReloadKind::Map(name.to_string())),
    }
}

/// Poller state: elapsed accumulator, per-path last-seen mtime, and whether the
/// initial snapshot has been taken.
#[derive(Resource, Default)]
pub struct HotReload {
    /// Seconds accumulated since the last poll; fires at [`POLL_INTERVAL`].
    elapsed: f32,
    /// Last-seen mtime per watched engine path. A path first seen on a poll is
    /// recorded but not reloaded, so a newly-authored map doesn't trigger a
    /// spurious reload of content the store doesn't yet have.
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

/// The set of engine paths to watch this poll: the three fixed authoring files
/// plus one `.tmj` per loaded map (so a map added at runtime becomes watched
/// once it's in the store).
fn watch_paths(game: &EggGame) -> Vec<String> {
    let mut paths = vec![
        "script/en.eggtext".to_string(),
        "data/main.eggscene".to_string(),
        egg_core::data::eggdata::DATA_PATH.to_string(),
    ];
    for name in game.state.maps.names() {
        paths.push(format!("maps/{name}.tmj"));
    }
    paths
}

/// Coarse-timer poll: stat every watched file, and on a changed mtime re-run the
/// matching load path. Runs on `Update` (not the fixed sim step) — reloading is
/// host bookkeeping, independent of the sim clock.
pub fn poll_hot_reload(time: Res<Time>, mut hot: ResMut<HotReload>, mut game: ResMut<EggGame>) {
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
            // First sight of a file: record it, but don't reload — the store may
            // not know this map yet (e.g. one just created in the editor).
            None => {
                hot.seen.insert(path.clone(), mtime);
            }
        }
    }
    for path in changed {
        if let Some(kind) = classify(&path) {
            apply_reload(&mut game, &path, kind);
        }
    }
}

/// Dispatch a detected change to the matching parse+install seam.
fn apply_reload(game: &mut EggGame, engine_path: &str, kind: ReloadKind) {
    match kind {
        ReloadKind::Script => reload_script(game, engine_path),
        ReloadKind::Scenes => reload_scenes(game, engine_path),
        ReloadKind::Data => {
            game.state.reload_data(&mut game.system);
            info!("Hot-reloaded {engine_path}");
        }
        ReloadKind::Map(name) => reload_map(game, engine_path, &name),
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
            game.state.script.set_base(file);
            info!("Hot-reloaded {path}");
        }
        Err(e) => warn!("Hot-reload: invalid eggtext in {path}: {e}"),
    }
}

/// Re-parse `main.eggscene` and install the cutscene registry (same seam as the
/// F2 editor's `pending_scene` drain). Parse error: log and keep the last good.
fn reload_scenes(game: &mut EggGame, path: &str) {
    let Some(src) = read_text(path) else { return };
    match egg_core::data::scene::parse(&src) {
        Ok(file) => {
            game.state.set_scenes(file);
            info!("Hot-reloaded {path}");
        }
        Err(e) => warn!("Hot-reload: invalid eggscene in {path}: {e}"),
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

    if game.state.walkaround.current_map.source == name {
        let g = &mut *game;
        let rebuilt = egg_core::world::map::map_by_name(
            &g.state.draw_state.indexed_sprites,
            name,
            &g.state.maps,
        );
        if let Some(fresh) = rebuilt {
            g.state.walkaround.current_map.bg_colour = fresh.bg_colour;
            g.state.walkaround.current_map.camera_bounds = fresh.camera_bounds;
            g.state.walkaround.current_map.layers = fresh.layers;
            g.state.walkaround.current_map.fg_layers = fresh.fg_layers;
            g.state.walkaround.current_map.sprite_layers = fresh.sprite_layers;
            g.state.walkaround.current_map.sprite_components = fresh.sprite_components;
        }
    }
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
    }

    #[test]
    fn rejects_unwatched_and_malformed() {
        // User data and binary art are never hot-reloaded.
        assert_eq!(classify("save.json"), None);
        assert_eq!(classify("sprites/sheet.png"), None);
        assert_eq!(classify("fonts/tic80_font.png"), None);
        // A different language isn't the base script.
        assert_eq!(classify("script/fr.eggtext"), None);
        // Malformed map paths.
        assert_eq!(classify("maps/.tmj"), None);
        assert_eq!(classify("maps/sub/dir.tmj"), None);
        assert_eq!(classify("maps/house.png"), None);
        assert_eq!(classify("maps/house.tmj.bak"), None);
    }
}

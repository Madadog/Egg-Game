use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use egg_core::EggState;

use egg_core::data::tiled::TiledMap;
use egg_core::editor::text::TextEditor;
use egg_core::platform::ConsoleApi;
use egg_core::platform::{EggInput, HEIGHT, ScanCode, WIDTH};
use fantasy_console::{
    ConsolePlugin, FantasyConsole, SfxAssets, play_music, play_sounds, screen_scale, update_texture,
};
use script_asset::{SceneAsset, ScriptAsset, ScriptPlugin};
use tiled::{ManifestAsset, TiledMapAsset, TiledMapPlugin};

mod base64;
mod fantasy_console;
#[cfg(not(target_arch = "wasm32"))]
mod hot_reload;
mod hotkeys;
mod script_asset;
mod tiled;
mod views;

/// Bevy frontend: Stores console and game state. Plus stuff for loading assets, pausing sim and window management.
#[derive(Resource)]
pub struct EggGame {
    pub state: EggState,
    system: FantasyConsole,
    /// The PRIMARY window's input for this frame. `step_state` refreshes it and —
    /// only while the primary is focused — fills it from the host events; each
    /// extra window keeps its own [`ViewWindow.input`](views::ViewWindow::input).
    /// Threaded into the engine as data (via [`egg_core::Ctx::input`]) rather than
    /// pulled through the console.
    pub input: EggInput,

    pub loaded: bool,
    pub pause: bool,
    pub scale_mode: ScaleMode,
    /// Whether the primary window shows the raw text editor (toggled with `F2`,
    /// `F1` returns) instead of the game/map-editor — the main-window peer of the
    /// per-view [`views::ViewMode::Text`]. The sim freezes while it's open.
    pub text_mode: bool,
    /// The primary window's text editor for the script files (used while
    /// [`text_mode`](Self::text_mode)).
    pub text_editor: TextEditor,
}
impl EggGame {
    pub fn run(&mut self) {
        // Disjoint field borrows: the sim reads its input as data while it still
        // holds `&mut system` for host effects.
        let g = &mut *self;
        g.state.run(&mut g.system, &g.input);
    }
}
impl Default for EggGame {
    fn default() -> Self {
        EggGame {
            state: EggState::default(),

            system: FantasyConsole::new(),
            input: EggInput::new(),

            pause: false,
            loaded: false,

            scale_mode: ScaleMode::Linear,
            text_mode: false,
            text_editor: TextEditor::default(),
        }
    }
}

/// How the framebuffer is scaled into the window: the fixed base resolution is
/// scaled to fit the window (the classic look), either smoothly or at integer
/// steps.
#[derive(Clone, Copy, PartialEq)]
pub enum ScaleMode {
    Linear,
    Integer,
}

fn main() {
    // Route wasm panics to the browser console (and dev tools) instead of the
    // opaque "unreachable executed" trap.
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.102, 0.110, 0.173)))
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        // Bind winit to the <canvas id="game-canvas"> in index.html
                        // rather than letting it append its own. The HTML canvas
                        // carries `tabindex` so it can hold keyboard focus, and JS
                        // focuses it on load/click — without this, key events on
                        // web go to <body> and the game never sees them. Ignored on
                        // native.
                        canvas: Some("#game-canvas".into()),
                        fit_canvas_to_parent: true,
                        title: "Egg Game".to_string(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        // Asset codecs/loaders (engine-agnostic data → Bevy assets).
        .add_plugins(TiledMapPlugin)
        .add_plugins(ScriptPlugin)
        // Domain plugins: each owns its systems/resources in the module that
        // owns the domain. `CorePlugin` additionally assembles the single
        // cross-domain `FixedUpdate` chain (see its docs).
        .add_plugins(AssetsPlugin)
        .add_plugins(ConsolePlugin)
        .add_plugins(views::ViewsPlugin)
        .add_plugins(hotkeys::HotkeysPlugin)
        .add_plugins(CorePlugin);
    // Native asset hot-reload: re-read the authoring files when they change on
    // disk. Web has no filesystem to poll (its persistence is localStorage).
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(hot_reload::HotReloadPlugin);
    app.run();
}

/// Core simulation + app plugin: the [`EggGame`] resource, the fixed-timestep
/// clock, the single ordered `FixedUpdate` chain that drives a frame, and the
/// engine-requested-quit handler.
///
/// Registers:
/// * resource: [`EggGame`] (init) and `Time::<Fixed>` (the 64 FPS sim clock).
/// * `Update`: [`handle_exit_request`].
/// * `FixedUpdate` (one `.chain()`, strict order — its members live across
///   modules but the order is load-bearing, so it is assembled here as a single
///   call): [`step_state`] → [`views::update_views`] →
///   [`fantasy_console::play_sounds`] → [`fantasy_console::play_music`] →
///   [`fantasy_console::update_texture`]. `step_state` advances the sim and maps
///   the focused cursor; `update_views` then renders each extra view from that;
///   the sfx/music systems drain the sim's sound queue; `update_texture` blits
///   the finished framebuffer last.
struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EggGame>()
            .add_systems(Update, handle_exit_request)
            .add_systems(
                FixedUpdate,
                (
                    step_state,
                    views::update_views,
                    play_sounds,
                    play_music,
                    update_texture,
                )
                    .chain(),
            )
            // 64 FPS
            .insert_resource(Time::<Fixed>::default());
    }
}

/// Asset loading plugin. Spans both data domains (Tiled maps via
/// [`crate::tiled`] and language scripts via [`crate::script_asset`]) plus the
/// manifest that names them, so it lives in the host root rather than inside
/// either loader's module. The loader is manifest-driven and runs in **three
/// phases** (all in [`load_assets`], gated on resource presence/load state):
///
/// 1. **expand** — wait for the manifest, then build [`GameAssets`] from the
///    maps it names (so the set of maps is data, not code).
/// 2. **discover images** — once the essentials are loaded and every map has
///    settled, walk the *loaded* maps for their image-layer PNG paths, resolve
///    each under `maps/`, and start loading it (recorded in
///    [`GameAssets::map_images`]). This phase can only run after the maps parse,
///    since only then are their image-layer paths known.
/// 3. **install** — once those image handles have also settled, decode each into
///    the engine's `RgbaImage`, attach it to its map, and insert the maps (plus
///    sheets/flags/script) into the engine. Essentials failure is fatal; a
///    failed map *or* image is logged and skipped, so neither can wedge boot.
///
/// Registers:
/// * resources: [`Manifest`] + [`GameAssets`] + [`SfxAssets`] (inserted by
///   [`setup_assets`]) and [`PendingLanguage`] (init).
/// * `Startup`: [`setup_assets`].
/// * `Update`: [`load_assets`], [`poll_language_change`] (registered unordered).
struct AssetsPlugin;

impl Plugin for AssetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingLanguage>()
            .add_systems(Startup, setup_assets)
            .add_systems(Update, (load_assets, poll_language_change));
    }
}

/// The asset manifest handle, loaded first (in `setup_assets`). `load_assets`
/// waits for it, then builds the real [`GameAssets`] from the maps it names — so
/// the set of maps is data, not code.
#[derive(Resource)]
pub struct Manifest(pub Handle<ManifestAsset>);

/// One image-layer PNG a loaded map references: which map it belongs to, the
/// path as authored (relative to the map, the key the engine attaches it under),
/// and the Bevy handle the host is loading it through. Collected in phase 2 once
/// the maps are parsed (only then are their image-layer paths known), then
/// awaited in phase 3 like maps are — same never-wedge rule.
#[derive(Debug)]
pub struct MapImage {
    /// `MapStore` key of the map this image belongs to.
    pub map_name: String,
    /// Path as authored in the `.tmj` (relative to the map file) — the key
    /// [`egg_core::data::tiled::TiledMap::attach_image`] matches on.
    pub rel_path: String,
    pub handle: Handle<Image>,
}

/// Every handle the game needs to boot, expanded from the [`Manifest`]. Built
/// once the manifest finishes loading (see [`GameAssets::from_manifest`]); the
/// font/sheet/script are fixed, while the maps come from the manifest's name
/// list.
#[derive(Debug, Resource)]
pub struct GameAssets {
    pub font: Handle<Image>,
    pub sheet: Handle<Image>,
    pub maps: Vec<Handle<TiledMapAsset>>,
    /// Base file names for `maps` (same order) — the names they're stored
    /// under in the game's `MapStore`, and where an edited "modern" map is
    /// saved back to (`maps/<name>.tmj`). A map that fails to parse is logged
    /// and skipped at install time, so what actually lands in the store is the
    /// subset of these that loaded cleanly.
    pub map_names: Vec<String>,
    pub script: Handle<ScriptAsset>,
    /// The cutscene registry (`data/main.eggscene`), loaded and installed
    /// alongside the language script — a separate, language-independent file.
    pub scenes: Handle<SceneAsset>,
    /// The image-layer PNGs the loaded maps reference, collected in phase 2 once
    /// the maps have parsed. `None` until that phase runs (so its presence is the
    /// phase-2-done signal); empty `Some` when no map has an image layer.
    pub map_images: Option<Vec<MapImage>>,
}
impl GameAssets {
    /// Expand a loaded [`GameManifest`] into concrete asset handles: each map
    /// stem → `maps/<name>.tmj`, plus the fixed font/sheet/script. Kicks off
    /// loading all of them.
    fn from_manifest(assets: &AssetServer, manifest: &egg_core::data::tiled::GameManifest) -> Self {
        Self {
            font: assets.load("fonts/tic80_font.png"),
            sheet: assets.load("sprites/sheet.png"),
            maps: manifest
                .maps
                .iter()
                .map(|name| assets.load(format!("maps/{name}.tmj")))
                .collect(),
            map_names: manifest.maps.clone(),
            script: assets.load("script/en.eggtext"),
            scenes: assets.load("data/main.eggscene"),
            map_images: None,
        }
    }
    /// The essential assets (font, sheet, script) by name — the ones the game
    /// cannot boot without. Maps are deliberately excluded: boot must not block
    /// on an individual map (see [`Self::maps_settled`]), so they're handled
    /// resiliently at install time instead.
    fn essentials(&self) -> impl Iterator<Item = (String, bevy::asset::UntypedAssetId)> + '_ {
        [
            ("font", self.font.id().untyped()),
            ("sprite sheet", self.sheet.id().untyped()),
            ("script", self.script.id().untyped()),
            ("cutscenes", self.scenes.id().untyped()),
        ]
        .into_iter()
        .map(|(name, id)| (name.to_string(), id))
    }
    /// Whether every essential asset is loaded.
    fn essentials_loaded(&self, assets: &AssetServer) -> bool {
        self.essentials()
            .all(|(_, id)| assets.get_load_state(id).is_some_and(|s| s.is_loaded()))
    }
    /// The first essential asset that *failed* to load, by name. A failure here
    /// is a fatal config error (bad path or corrupt file), distinct from "still
    /// loading" — the caller fails loudly rather than sitting on the loading
    /// screen forever.
    fn essential_failure(&self, assets: &AssetServer) -> Option<String> {
        self.essentials()
            .find(|(_, id)| assets.get_load_state(*id).is_some_and(|s| s.is_failed()))
            .map(|(name, _)| name)
    }
    /// Whether every map handle has *settled* — finished loading, one way or the
    /// other (`Loaded` or `Failed`), with none still in flight. Boot proceeds
    /// once this holds, installing the maps that loaded and skipping those that
    /// failed, so a single bad map can never wedge the game in the loading state.
    fn maps_settled(&self, assets: &AssetServer) -> bool {
        self.maps.iter().all(|m| {
            assets
                .get_load_state(m.id())
                .is_some_and(|s| s.is_loaded() || s.is_failed())
        })
    }
    /// Whether every image-layer PNG handle has *settled* (loaded or failed).
    /// `true` before phase 2 has collected them (nothing to wait on yet); after,
    /// the same never-wedge rule as maps — a missing/failed PNG settles as failed
    /// and its layer just gets no pixels (not drawn, empty collision).
    fn images_settled(&self, assets: &AssetServer) -> bool {
        self.map_images.iter().flatten().all(|img| {
            assets
                .get_load_state(img.handle.id())
                .is_some_and(|s| s.is_loaded() || s.is_failed())
        })
    }
}

fn setup_assets(mut commands: Commands, assets: Res<AssetServer>) {
    // Load the manifest first; `load_assets` expands it into GameAssets once it
    // arrives. The manifest's bespoke `.manifest` extension keeps it off the
    // script loader (which owns `.json`).
    commands.insert_resource(Manifest(assets.load("game.manifest")));
    commands.insert_resource(SfxAssets::new(&assets));
}

/// Prefer a persisted asset override (a web editor write) over the Bevy-loaded
/// `bundled` asset, parsing the override bytes with `parse`. On native
/// [`fantasy_console::asset_override`] is always `None`, so this returns
/// `bundled` unchanged (zero behaviour change); on web a present override is
/// parsed and used, so an in-game edit survives a reload. A malformed override
/// logs and falls back to `bundled`, so a bad persisted edit can't wedge boot.
fn prefer_override<T>(
    path: &str,
    bundled: Option<T>,
    parse: impl FnOnce(&[u8]) -> Result<T, String>,
) -> Option<T> {
    match fantasy_console::asset_override(path) {
        Some(bytes) => match parse(&bytes) {
            Ok(value) => Some(value),
            Err(e) => {
                warn!("Ignoring invalid persisted override for {path}: {e}");
                bundled
            }
        },
        None => bundled,
    }
}

// Bevy system parameters: each `Res`/`ResMut` is a distinct world access, so the
// arity is structural — bundling them into a `SystemParam` would only hide it.
#[allow(clippy::too_many_arguments)]
fn load_assets(
    mut commands: Commands,
    manifest: Option<Res<Manifest>>,
    game_assets: Option<ResMut<GameAssets>>,
    assets: Res<AssetServer>,
    images: Res<Assets<Image>>,
    maps: Res<Assets<TiledMapAsset>>,
    manifests: Res<Assets<ManifestAsset>>,
    scripts: Res<Assets<ScriptAsset>>,
    scenes: Res<Assets<SceneAsset>>,
    mut state: ResMut<EggGame>,
) {
    // Phase 1: wait for the manifest, then expand it into GameAssets from the
    // maps it names. We only do this once — GameAssets existing is the signal
    // that phase 1 is done.
    if game_assets.is_none() {
        let Some(manifest) = manifest else { return };
        match assets.get_load_state(manifest.0.id()) {
            Some(s) if s.is_loaded() => {
                // Prefer a persisted manifest override (web editor writes) over the
                // bundled copy; on native there's no override so the Bevy asset is
                // used unchanged.
                let bundled = manifests.get(&manifest.0).map(|m| m.0.clone());
                let manifest = prefer_override("game.manifest", bundled, |b| {
                    egg_core::data::tiled::manifest_from_json(b).map_err(|e| e.to_string())
                });
                if let Some(manifest) = manifest {
                    info!("Manifest loaded: {} map(s).", manifest.maps.len());
                    commands.insert_resource(GameAssets::from_manifest(&assets, &manifest));
                }
            }
            Some(s) if s.is_failed() => panic!("Could not load game.manifest: {s:?}"),
            _ => {}
        }
        return;
    }
    let mut game_assets = game_assets.unwrap();

    // A *failed* essential can never resolve: fail loudly (as the old loader did)
    // rather than waiting on the loading screen forever. This guard holds across
    // all later phases.
    if let Some(which) = game_assets.essential_failure(&assets) {
        panic!("Essential asset failed to load: {which}");
    }

    // Phase 2: once the essentials are loaded and every map has settled (loaded
    // or failed — never blocking boot on an individual map), discover the
    // image-layer PNGs the loaded maps reference and start loading them. We only
    // know a map's image paths after it parses, so this can't be folded into
    // phase 1. `map_images` going from `None` to `Some` is the phase-2-done flag.
    if game_assets.map_images.is_none() {
        if !(game_assets.essentials_loaded(&assets) && game_assets.maps_settled(&assets)) {
            return;
        }
        let mut map_images = Vec::new();
        for (name, handle) in game_assets.map_names.iter().zip(&game_assets.maps) {
            let Some(map) = maps.get(handle) else {
                continue;
            };
            for rel_path in map.0.image_layer_paths() {
                // Image paths are authored relative to the map file, which lives
                // in `maps/`; resolve under it (e.g. `images/bedroom1_mask.png`
                // → `maps/images/bedroom1_mask.png`).
                let handle = assets.load(format!("maps/{rel_path}"));
                map_images.push(MapImage {
                    map_name: name.clone(),
                    rel_path: rel_path.to_string(),
                    handle,
                });
            }
        }
        info!(
            "Discovered {} image-layer PNG(s) across maps.",
            map_images.len()
        );
        game_assets.map_images = Some(map_images);
        return;
    }

    // Phase 3: install once those image handles have also settled (same
    // never-wedge rule — a missing/failed PNG just leaves its layer pixel-less).
    if !game_assets.images_settled(&assets) {
        return;
    }
    let (Some(font), Some(sheet)) = (
        images.get(&game_assets.font),
        images.get(&game_assets.sheet),
    ) else {
        return;
    };
    state.system.set_font(font);
    // The same built font is game data on `EggState` too (threaded through
    // `Ctx::font`), so install a clone there — text drawing reads it from the
    // engine, not the console.
    let built_font = state.system.font.clone();
    state.state.set_font(built_font);
    // The sprite sheets live on DrawState (their single owner); the host
    // converts the Bevy image into the engine's formats and fills DrawState
    // directly.
    let palette = state.state.draw_state.palettes[0].clone();
    state.state.draw_state.rgba_sprites = FantasyConsole::sprites_from_image(sheet);
    state.state.draw_state.indexed_sprites =
        FantasyConsole::indexed_sprites_from_image(sheet, &palette);
    // Maps live on the engine's MapStore, keyed by file stem — the single copy
    // that drawing, collision and the editor all read. RESILIENCE: each map is
    // installed independently; one that failed to load/parse (these five were
    // authored in Tiled 1.8–1.10 and never been through our parser) is logged
    // and skipped, so it can't block boot or panic this system. Each loaded map
    // also gets its image-layer pixels attached (decoded from the phase-2 PNGs)
    // BEFORE it lands in the store, so the runtime sees its painted bg/mask.
    let map_images = game_assets.map_images.as_deref().unwrap_or_default();
    let mut loaded = Vec::new();
    for (name, handle) in game_assets.map_names.iter().zip(&game_assets.maps) {
        // Prefer a persisted `.tmj` override (web editor writes) over the Bevy copy;
        // this also lets a brand-new map that exists only as an override load even
        // though its bundled fetch failed. On native the override is None, so this
        // is exactly the Bevy-loaded map as before.
        let bundled = maps.get(handle).map(|m| m.0.clone());
        let map = prefer_override(&format!("maps/{name}.tmj"), bundled, |b| {
            egg_core::data::tiled::from_json(b).map_err(|e| e.to_string())
        });
        match map {
            Some(mut map) => {
                attach_map_images(&mut map, name, map_images, &images);
                state.state.maps.insert(name.clone(), map);
                loaded.push(name.clone());
            }
            None => warn!("Skipping map `{name}` (failed to load or parse)."),
        }
    }
    info!(
        "Loaded {}/{} maps: {loaded:?}",
        loaded.len(),
        game_assets.map_names.len()
    );
    // Dialogue + cutscenes each prefer a persisted override (web editor writes)
    // over the bundled copy, re-parsed through the same seams the F2 editor's
    // live-reload uses; on native the override is None, so the Bevy asset stands.
    let script_file = prefer_override(
        "script/en.eggtext",
        scripts.get(&game_assets.script).map(|s| s.0.clone()),
        |b| {
            let text = std::str::from_utf8(b).map_err(|e| e.to_string())?;
            egg_core::data::script::eggtext::parse(text).map_err(|e| e.to_string())
        },
    );
    if let Some(file) = script_file {
        state.state.script.set_base(file);
    }
    // The cutscene registry installs alongside the dialogue (both are essentials,
    // so they've settled by here).
    let scene_file = prefer_override(
        "data/main.eggscene",
        scenes.get(&game_assets.scenes).map(|s| s.0.clone()),
        |b| {
            let text = std::str::from_utf8(b).map_err(|e| e.to_string())?;
            egg_core::data::scene::parse(text).map_err(|e| e.to_string())
        },
    );
    if let Some(file) = scene_file {
        state.state.set_scenes(file);
    }
    state.loaded = true;
    info!("Finished loading assets.");
    commands.remove_resource::<GameAssets>();
    commands.remove_resource::<Manifest>();
}

/// Attach the decoded image-layer pixels to `map` (keyed `name`) before it's
/// inserted into the store: for each [`MapImage`] of this map that loaded, the
/// Bevy `Image` is converted to the engine's `RgbaImage` and handed to
/// [`egg_core::data::tiled::TiledMap::attach_image`]. An image that failed to load
/// (no entry in `images`) is logged and skipped — its layer keeps no pixels, so
/// it simply doesn't draw and contributes empty collision.
fn attach_map_images(
    map: &mut TiledMap,
    name: &str,
    map_images: &[MapImage],
    images: &Assets<Image>,
) {
    for img in map_images.iter().filter(|m| m.map_name == name) {
        match images.get(&img.handle) {
            Some(image) => {
                let pixels = FantasyConsole::sprites_from_image(image);
                map.attach_image(&img.rel_path, pixels);
            }
            None => warn!(
                "Image layer PNG `{}` for map `{name}` failed to load; layer has no pixels.",
                img.rel_path
            ),
        }
    }
}

/// The script handle for a language requested at runtime, while it loads.
#[derive(Resource, Default)]
struct PendingLanguage(Option<Handle<ScriptAsset>>);

/// Honour runtime language switches requested via `ConsoleApi::set_language`:
/// start loading the requested `script/<lang>.eggtext`, then install it as the
/// active language (overlaid on the base) once it finishes loading.
fn poll_language_change(
    assets: Res<AssetServer>,
    scripts: Res<Assets<ScriptAsset>>,
    mut pending: ResMut<PendingLanguage>,
    mut state: ResMut<EggGame>,
) {
    if pending.0.is_none()
        && let Some(language) = state.state.take_pending_language()
    {
        info!("Loading language {language:?}");
        pending.0 = Some(assets.load(format!("script/{language}.eggtext")));
    }
    if let Some(handle) = pending.0.clone()
        && let Some(script) = scripts.get(&handle)
    {
        state.state.script.set_language(script.0.clone());
        pending.0 = None;
        info!("Switched active language.");
    }
}

/// Draw a centred status overlay (Paused / Fast-Forward) onto the screen.
fn draw_overlay(game: &mut EggGame, text: &str) {
    let colour =
        egg_core::render::image::Rgba::from_rgb(game.state.draw_state.palettes[0][12]);
    let system = &mut game.system;
    egg_core::render::print_to_with_font(
        &system.font,
        &mut system.output_screen,
        text,
        100,
        62,
        colour,
        egg_core::render::PrintOptions::default(),
    );
}

/// Per-fixed-step simulation driver: held-key input (controller, panning,
/// fast-forward, the `Digit3` cheats), typed text, mouse mapping, and the sim
/// step itself. Edge-triggered hotkeys live in [`hotkeys::primary_hotkeys`] /
/// [`views::view_hotkeys`] (Update schedule), where `just_pressed` fires
/// exactly once per tap — here it would re-fire on every catch-up step of a
/// lagging frame.
#[allow(clippy::too_many_arguments)] // a Bevy system — each param is an injected resource/query
fn step_state(
    mut game: ResMut<EggGame>,
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard_events: MessageReader<KeyboardInput>,
    windows: Query<(Entity, &Window, Has<bevy::window::PrimaryWindow>)>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut mouse_wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut views: ResMut<views::ViewWindows>,
) {
    if !game.loaded {
        // Still drain wheel events so they don't pile up before the game loads.
        mouse_wheel.clear();
        return;
    }

    // Which window owns input this frame? Only the primary drives the player; an
    // extra window drives its own free camera/editor (in `views::update_views`).
    // All focus/typing/editor-open decisions come from the shared routing brain
    // ([`views::InputRouting`]) so this stays in lock-step with the hotkeys and
    // the view systems. Computed up front — nothing below changes focus.
    let focused_entity = windows.iter().find(|(_, w, _)| w.focused).map(|(e, ..)| e);
    let routing = views::InputRouting::compute(focused_entity, &game, &views);
    let drives_player = routing.drives_player;

    // Each window owns its own `EggInput` (the primary's on `game`, each view's on
    // the view). Advance every window's edge-detection history this frame; we then
    // populate only the *focused* window's input, so the primary reads its own —
    // empty when a view is focused, so it can't act on input aimed at the view —
    // and each view's input is threaded straight into its own step in
    // `update_views` (no swap through a shared console).
    game.input.refresh();
    for v in views.views.iter_mut() {
        v.input.refresh();
    }

    // Sum this frame's mouse wheel (touchpad pixel deltas clamped into i8).
    let (mut wheel_x, mut wheel_y) = (0.0f32, 0.0f32);
    for ev in mouse_wheel.read() {
        wheel_x += ev.x;
        wheel_y += ev.y;
    }

    // Map the focused window's cursor to its framebuffer pixel, computed up front
    // (it reads `game`/`views` immutably) so the `&mut` into the focused window's
    // input below doesn't overlap. The screen sprite is centred and drawn at
    // `scale` device px per framebuffer px; invert that (subtract the centring
    // letterbox, divide by scale).
    let cursor_px = windows
        .iter()
        .find(|(_, w, _)| w.focused)
        .and_then(|(_, window, primary)| window.cursor_position().map(|pos| (window, primary, pos)))
        .map(|(window, primary, pos)| {
            let (fb_w, fb_h, scale) = if primary {
                // The primary renders at the fixed base resolution, scaled to fit.
                (
                    game.system.width() as f32,
                    game.system.height() as f32,
                    screen_scale(window, &game.scale_mode),
                )
            } else if let views::Focus::Extra(i) = routing.focus {
                // Extra views render Mirror-style: framebuffer = window ÷ the
                // view's *effective* pixel ratio (not raw `scale`, or a window past
                // the resolution cap would map off-canvas).
                let v = &views.views[i];
                let scale = views::effective_scale(window.width(), window.height(), v.scale);
                (
                    v.output.width() as f32,
                    v.output.height() as f32,
                    scale.max(1) as f32,
                )
            } else {
                (WIDTH as f32, HEIGHT as f32, 1.0)
            };
            framebuffer_pixel(
                pos,
                Vec2::new(window.width(), window.height()),
                Vec2::new(fb_w, fb_h),
                scale,
            )
        });

    // Merge keyboard + (optional) first gamepad into player one's controller. The
    // directional/action keys only apply while the primary is focused; a gamepad
    // always drives the player (it isn't window-routed).
    let pad = gamepads.iter().next().map(|(_, gamepad)| gamepad);
    let stick = |axis: GamepadAxis| pad.and_then(|g| g.get(axis)).unwrap_or(0.0);
    let held = |kb: &[KeyCode], button: GamepadButton| {
        (drives_player && keys.any_pressed(kb.iter().copied()))
            || pad.is_some_and(|g| g.pressed(button))
    };

    // Everything below writes into the *focused* window's input: `game.input` for
    // the primary, the view's own `EggInput` for a focused extra view.
    let target = match routing.focus {
        views::Focus::Primary => &mut game.input,
        views::Focus::Extra(i) => &mut views.views[i].input,
    };
    target.mouse.scroll_x[0] = wheel_x.clamp(-127.0, 127.0) as i8;
    target.mouse.scroll_y[0] = wheel_y.clamp(-127.0, 127.0) as i8;
    {
        let c = &mut target.controllers[0];
        use KeyCode::*;
        c.up[0] =
            held(&[ArrowUp, KeyW], GamepadButton::DPadUp) || stick(GamepadAxis::LeftStickY) > 0.2;
        c.down[0] = held(&[ArrowDown, KeyS], GamepadButton::DPadDown)
            || stick(GamepadAxis::LeftStickY) < -0.2;
        c.left[0] = held(&[ArrowLeft, KeyA], GamepadButton::DPadLeft)
            || stick(GamepadAxis::LeftStickX) < -0.2;
        c.right[0] = held(&[ArrowRight, KeyD], GamepadButton::DPadRight)
            || stick(GamepadAxis::LeftStickX) > 0.2;
        c.a[0] = held(&[KeyZ, Space, Enter, KeyE], GamepadButton::South);
        c.b[0] = held(&[KeyX, Escape, KeyQ], GamepadButton::East);
        c.x[0] = held(&[KeyC], GamepadButton::West);
        c.y[0] = held(&[KeyV], GamepadButton::North);
    }
    for keycode in keys.get_pressed() {
        if let Some(scancode) = keycode_to_scancode(*keycode) {
            // Every key reaches the focused window's editor (map or text). The
            // primary's input stays empty when a view is focused, so it can't act
            // on keys aimed at the view; the focused view gets the full keyboard.
            target.press_key(scancode);
        }
    }
    for event in keyboard_events.read() {
        if event.state == ButtonState::Pressed
            && let Some(text) = event.text.as_ref()
            && let Some(c) = text.chars().next()
            && !c.is_control()
        {
            target.push_char(c);
        }
    }
    if let Some((mx, my)) = cursor_px {
        target.mouse.x[0] = mx;
        target.mouse.y[0] = my;
        target.mouse.left[0] = mouse_button.pressed(MouseButton::Left);
        target.mouse.right[0] = mouse_button.pressed(MouseButton::Right);
        target.mouse.middle[0] = mouse_button.pressed(MouseButton::Middle);
    }

    // Primary text-editor mode (F2): the main window shows the raw script editor
    // instead of the world. Step + draw it into the main framebuffer and skip the
    // sim (and all gameplay/debug hotkeys) — every key feeds the buffer. The
    // cursor was mapped to the framebuffer above, so clicks land on the editor.
    if game.text_mode {
        // One deref of the `ResMut` so the field borrows below are disjoint.
        let g = &mut *game;
        // Step against the primary window's input — which is empty when a view is
        // focused (we populated the view's own input instead), so this is a
        // harmless no-op then. The editor still draws every frame so the window
        // keeps showing it.
        let (w, h) = (g.system.width(), g.system.height());
        g.text_editor
            .step(&mut g.system, &g.input, &g.state.font, w, h);
        if let Some(source) = g.text_editor.pending_script.take() {
            match egg_core::data::script::eggtext::parse(&source) {
                Ok(file) => g.state.script.set_base(file),
                Err(e) => warn!("text editor: invalid eggtext on save: {e}"),
            }
        }
        if let Some(source) = g.text_editor.pending_scene.take() {
            match egg_core::data::scene::parse(&source) {
                Ok(file) => g.state.set_scenes(file),
                Err(e) => warn!("text editor: invalid eggscene on save: {e}"),
            }
        }
        g.text_editor.draw(&mut g.state.draw_state, &g.state.font);
        egg_core::gamestate::walkaround::WalkaroundState::composite_into(
            &mut g.state.draw_state,
            g.system.output_image(),
        );
        return;
    }

    // Pause (P) and single-step (N) are toggled in `hotkeys::primary_hotkeys`;
    // here the paused sim just keeps showing the overlay and skips stepping.
    if game.pause {
        draw_overlay(&mut game, "Paused\n[P] to unpause\n[N] to step forward");
        return;
    }
    // A view owns input this frame (it's focused, not the primary). The primary
    // still simulates + draws, but its own input (`game.input`) was left empty
    // above — we populated the focused view's input instead — so it can't act on
    // input aimed at the view. The view consumes its own input in `update_views`.
    if !drives_player {
        game.run();
        return;
    }
    // From here the PRIMARY is focused. While its own map editor captures typed
    // text, step the game (so it processes the keystrokes) and skip the global
    // debug/cheat hotkeys, so dialogue keys like "town_lamppost" don't fire the
    // m/n/k/l/p shortcuts.
    if routing.editor_typing {
        game.run();
        return;
    }
    // The primary map editor is open (but not typing): it owns the keyboard
    // (its `L`-off toggle and shortcuts are handled in `primary_hotkeys` /
    // `step_map_viewer`), so the held-key cheats below are suppressed — bare
    // keys (e.g. Digit3) must not fire while editing.
    if routing.primary_editor_open {
        game.run();
        return;
    }
    // Held-key cheats stay in the fixed step deliberately: they repeat per
    // simulation step, so their rate is frame-rate independent.
    if keys.pressed(KeyCode::Digit3) && keys.pressed(KeyCode::ShiftLeft) {
        let pos = game.state.walkaround.player().pos;
        let rand = game.state.rng.rand_u8();
        let id = if rand < 64 {
            egg_core::world::player::PresetId::ellie()
        } else if rand < 128 {
            egg_core::world::player::PresetId::dog()
        } else if rand < 192 {
            egg_core::world::player::PresetId::bro()
        } else {
            egg_core::world::player::PresetId::may()
        };
        if let Some(mut new) = game.state.presets.spawn(&id) {
            new.pos = pos;
            game.state.walkaround.entities.push(new);
            info!("we have {} entities", game.state.walkaround.entities.len());
        }
    } else if keys.pressed(KeyCode::Digit3) && keys.pressed(KeyCode::ControlLeft) {
        let pos = game.state.walkaround.player().pos;
        for e in game.state.walkaround.entities.iter_mut() {
            let normalised = e.pos - pos;
            let (x, y) = (normalised.x as f32 * 0.9, normalised.y as f32 * 0.9);
            e.pos = egg_core::geometry::Vec2::new(x as i16, y as i16) + pos;
        }
    } else if keys.pressed(KeyCode::Digit3) {
        if game.state.walkaround.entities.len() > 1 {
            game.state.walkaround.entities.pop();
        }
        info!("we have {} entities", game.state.walkaround.entities.len());
    }
    game.run();
    if keys.pressed(KeyCode::KeyN) {
        game.run();
        draw_overlay(&mut game, "Fast-Forward");
    }
}

/// Translate an engine-requested quit ([`ConsoleApi::exit`], surfaced as
/// `FantasyConsole::exit_requested`) into a Bevy `AppExit`. Save persistence is
/// handled inside the engine's frame (`EggState::flush_save`), so a clean exit
/// needs no extra save hook here. No engine code requests a quit yet.
fn handle_exit_request(game: Res<EggGame>, mut exit: MessageWriter<AppExit>) {
    if game.system.exit_requested() {
        exit.write(AppExit::Success);
    }
}

// TODO: find a home for Bevy -> console conversions (image types etc)
fn keycode_to_scancode(keycode: KeyCode) -> Option<ScanCode> {
    use KeyCode::*;
    Some(match keycode {
        KeyA => ScanCode::A,
        KeyB => ScanCode::B,
        KeyC => ScanCode::C,
        KeyD => ScanCode::D,
        KeyE => ScanCode::E,
        KeyF => ScanCode::F,
        KeyG => ScanCode::G,
        KeyH => ScanCode::H,
        KeyI => ScanCode::I,
        KeyJ => ScanCode::J,
        KeyK => ScanCode::K,
        KeyL => ScanCode::L,
        KeyM => ScanCode::M,
        KeyN => ScanCode::N,
        KeyO => ScanCode::O,
        KeyP => ScanCode::P,
        KeyQ => ScanCode::Q,
        KeyR => ScanCode::R,
        KeyS => ScanCode::S,
        KeyT => ScanCode::T,
        KeyU => ScanCode::U,
        KeyV => ScanCode::V,
        KeyW => ScanCode::W,
        KeyX => ScanCode::X,
        KeyY => ScanCode::Y,
        KeyZ => ScanCode::Z,
        Digit0 => ScanCode::Digit0,
        Digit1 => ScanCode::Digit1,
        Digit2 => ScanCode::Digit2,
        Digit3 => ScanCode::Digit3,
        Digit4 => ScanCode::Digit4,
        Digit5 => ScanCode::Digit5,
        Digit6 => ScanCode::Digit6,
        Digit7 => ScanCode::Digit7,
        Digit8 => ScanCode::Digit8,
        Digit9 => ScanCode::Digit9,
        Minus => ScanCode::Minus,
        Equal => ScanCode::Equals,
        BracketLeft => ScanCode::LeftBracket,
        BracketRight => ScanCode::RightBracket,
        Backslash => ScanCode::Backslash,
        Semicolon => ScanCode::Semicolon,
        Quote => ScanCode::Apostrophe,
        Backquote => ScanCode::Grave,
        Comma => ScanCode::Comma,
        Period => ScanCode::Period,
        Slash => ScanCode::Slash,
        Space => ScanCode::Space,
        Tab => ScanCode::Tab,
        Enter => ScanCode::Return,
        Backspace => ScanCode::Backspace,
        Delete => ScanCode::Delete,
        Insert => ScanCode::Insert,
        PageUp => ScanCode::PageUp,
        PageDown => ScanCode::PageDown,
        Home => ScanCode::Home,
        End => ScanCode::End,
        ArrowUp => ScanCode::Up,
        ArrowDown => ScanCode::Down,
        ArrowLeft => ScanCode::Left,
        ArrowRight => ScanCode::Right,
        CapsLock => ScanCode::CapsLock,
        ControlLeft | ControlRight => ScanCode::Ctrl,
        ShiftLeft | ShiftRight => ScanCode::Shift,
        AltLeft | AltRight => ScanCode::Alt,
        Escape => ScanCode::Escape,
        F1 => ScanCode::F1,
        F2 => ScanCode::F2,
        F3 => ScanCode::F3,
        F4 => ScanCode::F4,
        F5 => ScanCode::F5,
        F6 => ScanCode::F6,
        F7 => ScanCode::F7,
        F8 => ScanCode::F8,
        F9 => ScanCode::F9,
        F10 => ScanCode::F10,
        F11 => ScanCode::F11,
        F12 => ScanCode::F12,
        _ => return None,
    })
}

/// Map a window-space `cursor` to a framebuffer pixel. The `fb`-sized screen is
/// drawn centred at `scale` device-px per framebuffer-px, so subtract the
/// centring letterbox then divide by the scale. Pure so it can be unit-tested.
fn framebuffer_pixel(cursor: Vec2, window: Vec2, fb: Vec2, scale: f32) -> (i16, i16) {
    let offset = (window - fb * scale) / 2.0;
    (
        ((cursor.x - offset.x) / scale) as i16,
        ((cursor.y - offset.y) / scale) as i16,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framebuffer_pixel_centres_and_unscales() {
        // 960x540 framebuffer drawn at 2x exactly fills a 1920x1080 window (no
        // letterbox): the window centre maps to the framebuffer centre.
        assert_eq!(
            framebuffer_pixel(
                Vec2::new(960.0, 540.0),
                Vec2::new(1920.0, 1080.0),
                Vec2::new(960.0, 540.0),
                2.0,
            ),
            (480, 270),
        );

        // 240x136 framebuffer at 4x = 960x544, centred in a 1000x600 window:
        // x letterbox = (1000-960)/2 = 20, y letterbox = (600-544)/2 = 28. A
        // cursor on the image's top-left corner maps to pixel (0, 0)...
        assert_eq!(
            framebuffer_pixel(
                Vec2::new(20.0, 28.0),
                Vec2::new(1000.0, 600.0),
                Vec2::new(240.0, 136.0),
                4.0,
            ),
            (0, 0),
        );
        // ...and 4 device px further right is exactly one framebuffer px right.
        assert_eq!(
            framebuffer_pixel(
                Vec2::new(24.0, 28.0),
                Vec2::new(1000.0, 600.0),
                Vec2::new(240.0, 136.0),
                4.0,
            ),
            (1, 0),
        );
    }
}

use bevy::asset::RenderAssetUsages;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use egg_core::EggState;

use egg_core::system::ConsoleApi;
use egg_core::system::{HEIGHT, ScanCode, WIDTH};
use fantasy_console::FantasyConsole;
use script_asset::{ScriptAsset, ScriptPlugin};
use tiled::{ManifestAsset, TiledMapAsset, TiledMapPlugin, TilesetAsset};

mod fantasy_console;
mod hotkeys;
mod script_asset;
mod tiled;
mod views;

/// Bevy frontend: Stores console and game state. Plus stuff for loading assets, pausing sim and window management.
#[derive(Resource)]
pub struct EggGame {
    pub state: EggState,
    system: FantasyConsole,

    pub loaded: bool,
    pub pause: bool,
    pub scale_mode: ScaleMode,
    /// Fixed-resolution fit vs. window-mirroring framebuffer (toggle: F2).
    pub screen_mode: ScreenMode,
    /// In `Mirror` mode, the window:framebuffer pixel ratio — 1/2/4/8 (cycle: F3).
    pub mirror_scale: u32,
}
impl EggGame {
    pub fn run(&mut self) {
        self.state.run(&mut self.system);
    }
}
impl Default for EggGame {
    fn default() -> Self {
        EggGame {
            state: EggState::default(),

            system: FantasyConsole::new(),

            pause: false,
            loaded: false,

            scale_mode: ScaleMode::Linear,
            screen_mode: ScreenMode::Fit,
            mirror_scale: 1,
        }
    }
}

/// How the framebuffer is scaled into the window in [`ScreenMode::Fit`].
#[derive(Clone, Copy, PartialEq)]
pub enum ScaleMode {
    Linear,
    Integer,
}

/// Screen-sizing policy. `Fit` renders at the fixed base resolution and scales
/// it to fit the window (the classic look). `Mirror` makes the framebuffer
/// follow the window size (÷ `mirror_scale`) for genuinely more drawing room —
/// each game pixel then covers an N×N block of window pixels.
#[derive(Clone, Copy, PartialEq)]
pub enum ScreenMode {
    Fit,
    Mirror,
}

fn main() {
    // Route wasm panics to the browser console (and dev tools) instead of the
    // opaque "unreachable executed" trap.
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    App::new()
        .init_resource::<EggGame>()
        .insert_resource(ClearColor(Color::srgb(0.102, 0.110, 0.173)))
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
        .add_plugins(TiledMapPlugin)
        .add_plugins(ScriptPlugin)
        .init_resource::<PendingLanguage>()
        .init_resource::<views::ViewWindows>()
        .add_systems(Startup, (setup, setup_assets))
        .add_systems(
            Update,
            (
                load_assets,
                poll_language_change,
                hotkeys::primary_hotkeys,
                views::view_hotkeys,
                resize_screen,
                views::resize_views,
                views::handle_closed_views,
                handle_exit_request,
            ),
        )
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
        .insert_resource(Time::<Fixed>::default())
        .run();
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn(Camera2d);
    let screen_handle = images.add(new_screen_image(WIDTH as u32, HEIGHT as u32));
    commands.spawn((
        Sprite {
            image: screen_handle.clone(),
            ..default()
        },
        Transform::from_xyz(0., 0., 0.),
        GameScreenSprite,
    ));
}

/// The asset manifest handle, loaded first (in `setup_assets`). `load_assets`
/// waits for it, then builds the real [`GameAssets`] from the maps/tilesets it
/// names — so the set of maps is data, not code.
#[derive(Resource)]
pub struct Manifest(pub Handle<ManifestAsset>);

/// Every handle the game needs to boot, expanded from the [`Manifest`]. Built
/// once the manifest finishes loading (see [`GameAssets::from_manifest`]); the
/// font/sheet/script are fixed, while the maps and tilesets come from the
/// manifest's name lists.
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
    /// Tileset handles (`maps/<name>.tsj`) named by the manifest, for their
    /// per-tile collision-flag tables.
    pub tilesets: Vec<Handle<TilesetAsset>>,
    pub script: Handle<ScriptAsset>,
}
impl GameAssets {
    /// Expand a loaded [`GameManifest`] into concrete asset handles: each map
    /// stem → `maps/<name>.tmj`, each tileset stem → `maps/<name>.tsj`, plus the
    /// fixed font/sheet/script. Kicks off loading all of them.
    fn from_manifest(assets: &AssetServer, manifest: &egg_core::data::tmj::GameManifest) -> Self {
        Self {
            font: assets.load("fonts/tic80_font.png"),
            sheet: assets.load("sprites/sheet.png"),
            maps: manifest
                .maps
                .iter()
                .map(|name| assets.load(format!("maps/{name}.tmj")))
                .collect(),
            map_names: manifest.maps.clone(),
            tilesets: manifest
                .tilesets
                .iter()
                .map(|name| assets.load(format!("maps/{name}.tsj")))
                .collect(),
            script: assets.load("script/en.eggtext"),
        }
    }
    /// The essential assets (font, sheet, script, every tileset) by name —
    /// the ones the game cannot boot without. Maps are deliberately excluded:
    /// boot must not block on an individual map (see [`Self::maps_settled`]),
    /// so they're handled resiliently at install time instead.
    fn essentials(&self) -> impl Iterator<Item = (String, bevy::asset::UntypedAssetId)> + '_ {
        [
            ("font", self.font.id().untyped()),
            ("sprite sheet", self.sheet.id().untyped()),
            ("script", self.script.id().untyped()),
        ]
        .into_iter()
        .map(|(name, id)| (name.to_string(), id))
        .chain(
            self.tilesets
                .iter()
                .enumerate()
                .map(|(i, t)| (format!("tileset #{i}"), t.id().untyped())),
        )
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
}

fn setup_assets(mut commands: Commands, assets: Res<AssetServer>) {
    // Load the manifest first; `load_assets` expands it into GameAssets once it
    // arrives. The manifest's bespoke `.manifest` extension keeps it off the
    // script loader (which owns `.json`).
    commands.insert_resource(Manifest(assets.load("game.manifest")));
    commands.insert_resource(SfxAssets::new(&assets));
}

// Bevy system parameters: each `Res`/`ResMut` is a distinct world access, so the
// arity is structural — bundling them into a `SystemParam` would only hide it.
#[allow(clippy::too_many_arguments)]
fn load_assets(
    mut commands: Commands,
    manifest: Option<Res<Manifest>>,
    game_assets: Option<Res<GameAssets>>,
    assets: Res<AssetServer>,
    images: Res<Assets<Image>>,
    maps: Res<Assets<TiledMapAsset>>,
    tilesets: Res<Assets<TilesetAsset>>,
    manifests: Res<Assets<ManifestAsset>>,
    scripts: Res<Assets<ScriptAsset>>,
    mut state: ResMut<EggGame>,
) {
    // Phase 1: wait for the manifest, then expand it into GameAssets. We only do
    // this once — GameAssets existing is the signal that phase 1 is done.
    if game_assets.is_none() {
        let Some(manifest) = manifest else { return };
        match assets.get_load_state(manifest.0.id()) {
            Some(s) if s.is_loaded() => {
                if let Some(loaded) = manifests.get(&manifest.0) {
                    info!(
                        "Manifest loaded: {} map(s), {} tileset(s).",
                        loaded.0.maps.len(),
                        loaded.0.tilesets.len()
                    );
                    commands.insert_resource(GameAssets::from_manifest(&assets, &loaded.0));
                }
            }
            Some(s) if s.is_failed() => panic!("Could not load game.manifest: {s:?}"),
            _ => {}
        }
        return;
    }
    let game_assets = game_assets.unwrap();

    // Phase 2: install once the essentials are loaded and every map has settled
    // (loaded or failed) — never blocking boot on an individual map. A *failed*
    // essential, by contrast, can never resolve: fail loudly like the old
    // loader did rather than waiting on the loading screen forever.
    if let Some(which) = game_assets.essential_failure(&assets) {
        panic!("Essential asset failed to load: {which}");
    }
    if !(game_assets.essentials_loaded(&assets) && game_assets.maps_settled(&assets)) {
        return;
    }
    let (Some(font), Some(sheet)) = (images.get(&game_assets.font), images.get(&game_assets.sheet))
    else {
        return;
    };
    state.system.set_font(font);
    // The sprite sheets live on DrawState (their single owner); the host
    // converts the Bevy image into the engine's formats and fills DrawState
    // directly.
    let palette = state.state.draw_state.palettes[0].clone();
    state.state.draw_state.rgba_sprites = FantasyConsole::sprites_from_image(sheet);
    state.state.draw_state.indexed_sprites =
        FantasyConsole::indexed_sprites_from_image(sheet, &palette);
    // Sprite flags are data now: install the per-tile collision-flag table from
    // the manifest's tileset(s). The last loaded tileset wins (there's one in
    // practice, `tiles`); DrawState::default's blob seed is now overwritten.
    for handle in &game_assets.tilesets {
        if let Some(tileset) = tilesets.get(handle) {
            state.state.draw_state.sprite_flags = tileset.0.flag_table();
        }
    }
    // Maps live on the engine's MapStore, keyed by file stem — the single copy
    // that drawing, collision and the editor all read. RESILIENCE: each map is
    // installed independently; one that failed to load/parse (these five were
    // authored in Tiled 1.8–1.10 and never been through our parser) is logged
    // and skipped, so it can't block boot or panic this system.
    let mut loaded = Vec::new();
    for (name, handle) in game_assets.map_names.iter().zip(&game_assets.maps) {
        match maps.get(handle) {
            Some(map) => {
                state.state.maps.insert(name.clone(), map.0.clone());
                loaded.push(name.clone());
            }
            None => warn!("Skipping map `{name}` (failed to load or parse)."),
        }
    }
    info!("Loaded {}/{} maps: {loaded:?}", loaded.len(), game_assets.map_names.len());
    if let Some(script) = scripts.get(&game_assets.script) {
        state.state.script.set_base(script.0.clone());
    }
    state.loaded = true;
    info!("Finished loading assets.");
    commands.remove_resource::<GameAssets>();
    commands.remove_resource::<Manifest>();
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

#[derive(Debug, Resource)]
pub struct SfxAssets {
    pub sounds: HashMap<String, Handle<AudioSource>>,
}
impl SfxAssets {
    pub fn new(assets: &AssetServer) -> Self {
        let sfx = |name: &str| -> (String, Handle<AudioSource>) {
            (name.to_string(), assets.load(format!("sfx/{}.ogg", name)))
        };
        let sounds = HashMap::from_iter([
            sfx("1_piano"),
            sfx("2_obtained"),
            sfx("3_deny"),
            sfx("4_alert_up"),
            sfx("5_alert_down"),
            sfx("6_save"),
            sfx("7_reject"),
            sfx("8_item_up"),
            sfx("9_item_swap"),
            sfx("10_item_down"),
            sfx("11_interact"),
            sfx("12_bip"),
            sfx("13_door"),
            sfx("14_pop"),
            sfx("15_click_pop"),
            sfx("16_fanfare"),
            sfx("17_gain"),
            sfx("18_loss"),
            sfx("19_stairs_down"),
            sfx("20_stairs_up"),
            sfx("21_footstep_plain"),
        ]);
        Self { sounds }
    }
}

/// Standard audio playback at the game's mixing volume.
fn playback_settings(mode: bevy::audio::PlaybackMode, speed: f32) -> PlaybackSettings {
    PlaybackSettings {
        mode,
        volume: bevy::audio::Volume::Decibels(-6.0),
        speed,
        paused: false,
        ..Default::default()
    }
}

fn play_sounds(mut commands: Commands, game_assets: Res<SfxAssets>, mut state: ResMut<EggGame>) {
    for (name, options) in state.system.sounds() {
        if let Some(sound) = game_assets.sounds.get(&name.to_string()) {
            let speed =
                2.0_f32.powf((options.note as f32 + (options.octave as f32 - 5.0) * 12.0) / 12.0);
            commands.spawn((
                AudioPlayer(sound.clone()),
                playback_settings(bevy::audio::PlaybackMode::Despawn, speed),
            ));
        } else {
            panic!("Unknown sound \"{name:?}\" with {options:?}")
        }
    }
    state.system.sounds().clear();
}

fn play_music(
    mut commands: Commands,
    mut query: Query<(Entity, &mut AudioSink), With<MusicPlayer>>,
    mut state: ResMut<EggGame>,
    assets: Res<AssetServer>,
) {
    if let Some((x, playing)) = state.system.music_track() {
        if query.is_empty() && !*playing {
            let music: Handle<AudioSource> = assets.load(format!("music/{}.ogg", x.id));
            commands.spawn((
                AudioPlayer(music.clone()),
                playback_settings(bevy::audio::PlaybackMode::Loop, 1.0),
                MusicPlayer,
            ));
            *playing = true;
        }
    } else {
        for (entity, sink) in query.iter_mut() {
            commands.entity(entity).despawn();
            sink.stop();
        }
    }
}

#[derive(Component)]
pub struct MusicPlayer;

#[derive(Component)]
pub struct GameScreenSprite;

fn update_texture(
    state: ResMut<EggGame>,
    mut images: ResMut<Assets<Image>>,
    mut border_colour: ResMut<ClearColor>,
    sprite: Query<&Sprite, With<GameScreenSprite>>,
) {
    for sprite in sprite.iter() {
        state.system.blit_to_image(
            images
                .get_mut(&sprite.image)
                .unwrap()
                .data
                .as_mut()
                .expect("Main screen texture uninitialized, can't draw game."),
        );
    }
    // Use the current default palette's first colour for the border surround.
    if let Some(colour) = state.state.draw_state.palettes[0].first() {
        border_colour.0 = Color::srgb_u8(colour[0], colour[1], colour[2]);
    }
}

/// Smallest framebuffer Mirror mode will allocate, so a tiny window or a large
/// scale factor can't produce a degenerate (or zero-sized) screen.
const MIN_FB_W: u32 = 64;
const MIN_FB_H: u32 = 48;

/// A fresh black RGBA screen texture of `width`×`height`. Shared by `setup` and
/// the resize path so the format/usages always match.
fn new_screen_image(width: u32, height: u32) -> Image {
    Image::new_fill(
        Extent3d {
            width,
            height,
            ..default()
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::all(),
    )
}

/// Integer/linear scale factor that fits the framebuffer into `window` (Fit mode).
fn screen_scale(window: &Window, mode: &ScaleMode) -> f32 {
    let fit = (window.width() / WIDTH as f32).min(window.height() / HEIGHT as f32);
    match mode {
        // Never floor to 0 — a window smaller than the base resolution would
        // otherwise scale the screen out of existence.
        ScaleMode::Integer => fit.floor().max(1.0),
        ScaleMode::Linear => fit,
    }
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

/// Reconcile the framebuffer with the active screen mode + window size, then
/// scale the screen sprite so it fills the window. In `Fit` the framebuffer
/// stays at the base resolution and the sprite scales to fit; in `Mirror` the
/// framebuffer follows the window (÷ `mirror_scale`) and the sprite scales by
/// exactly that integer factor (crisp N×N pixels).
fn resize_screen(
    mut sprite: Query<(&Sprite, &mut Transform), With<GameScreenSprite>>,
    mut window: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut game: ResMut<EggGame>,
) {
    // The main framebuffer follows the PRIMARY window only; extra view windows
    // are sized independently by `views::resize_views`.
    let Ok(mut window) = window.single_mut() else {
        return;
    };
    window.resolution.set_scale_factor_override(Some(1.0));
    let Ok((sprite, mut transform)) = sprite.single_mut() else {
        return;
    };

    let (target, scale) = match game.screen_mode {
        ScreenMode::Fit => (
            (WIDTH as u32, HEIGHT as u32),
            screen_scale(&window, &game.scale_mode),
        ),
        ScreenMode::Mirror => {
            let n = game.mirror_scale.max(1);
            let w = (window.width() as u32 / n).max(MIN_FB_W);
            let h = (window.height() as u32 / n).max(MIN_FB_H);
            ((w, h), n as f32)
        }
    };

    // Resize the three lock-step buffers (console screen, draw layers, GPU
    // texture) together, only when the size actually changes — `blit_to_image`
    // copies the screen verbatim, so all three must match.
    if (game.system.width() as u32, game.system.height() as u32) != target {
        let g = &mut *game;
        g.system.resize_screen(target.0, target.1);
        g.state.draw_state.resize(target.0, target.1);
        if let Some(image) = images.get_mut(&sprite.image) {
            *image = new_screen_image(target.0, target.1);
        }
    }

    transform.scale = Vec3::new(scale, scale, 1.0);
}

/// Draw a centred status overlay (Paused / Fast-Forward) onto the screen.
fn draw_overlay(game: &mut EggGame, text: &str) {
    let colour = egg_core::system::drawing::image::Rgba::from_rgb(game.state.draw_state.palettes[0][12]);
    let system = &mut game.system;
    egg_core::system::print_to_with_font(
        &system.font,
        &mut system.output_screen,
        text,
        100,
        62,
        colour,
        egg_core::system::PrintOptions::default(),
    );
}

/// Per-fixed-step simulation driver: held-key input (controller, panning,
/// fast-forward, the `Digit3` cheats), typed text, mouse mapping, and the sim
/// step itself. Edge-triggered hotkeys live in [`hotkeys::primary_hotkeys`] /
/// [`views::view_hotkeys`] (Update schedule), where `just_pressed` fires
/// exactly once per tap — here it would re-fire on every catch-up step of a
/// lagging frame.
fn step_state(
    mut game: ResMut<EggGame>,
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard_events: MessageReader<KeyboardInput>,
    windows: Query<(Entity, &Window, Has<bevy::window::PrimaryWindow>)>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    gamepads: Query<(Entity, &Gamepad)>,
    views: Res<views::ViewWindows>,
) {
    if !game.loaded {
        return;
    }

    game.system.input().refresh();

    // Which window owns keyboard input this frame? Only the primary window
    // drives the player; an extra window drives its own free camera/editor
    // (handled in `views::update_views`). We gate the player controller's dpad
    // on this so arrow keys panning an extra view can't also move the player.
    // All the focus/typing/editor-open decisions come from the shared routing
    // brain ([`views::InputRouting`]) so this stays in lock-step with the
    // hotkeys and the view systems instead of re-deriving them here. Computed up
    // front (nothing below mutates the editor's focus/typing before the guards
    // read it), so the snapshot matches the per-schedule rule documented there.
    let focused_entity = windows.iter().find(|(_, w, _)| w.focused).map(|(e, ..)| e);
    let routing = views::InputRouting::compute(focused_entity, &game, &views);
    let drives_player = routing.drives_player;

    // Merge keyboard + (optional) first gamepad into player one's controller.
    // The directional/action keys only apply while the primary window is
    // focused; a gamepad always drives the player (it isn't window-routed).
    let pad = gamepads.iter().next().map(|(_, gamepad)| gamepad);
    let stick = |axis: GamepadAxis| pad.and_then(|g| g.get(axis)).unwrap_or(0.0);
    let held = |kb: &[KeyCode], button: GamepadButton| {
        (drives_player && keys.any_pressed(kb.iter().copied()))
            || pad.is_some_and(|g| g.pressed(button))
    };
    let c = &mut game.system.input().controllers[0];
    use KeyCode::*;
    c.up[0] = held(&[ArrowUp, KeyW], GamepadButton::DPadUp) || stick(GamepadAxis::LeftStickY) > 0.2;
    c.down[0] =
        held(&[ArrowDown, KeyS], GamepadButton::DPadDown) || stick(GamepadAxis::LeftStickY) < -0.2;
    c.left[0] =
        held(&[ArrowLeft, KeyA], GamepadButton::DPadLeft) || stick(GamepadAxis::LeftStickX) < -0.2;
    c.right[0] = held(&[ArrowRight, KeyD], GamepadButton::DPadRight)
        || stick(GamepadAxis::LeftStickX) > 0.2;
    c.a[0] = held(&[KeyZ, Space, Enter, KeyE], GamepadButton::South);
    c.b[0] = held(&[KeyX, Escape, KeyQ], GamepadButton::East);
    c.x[0] = held(&[KeyC], GamepadButton::West);
    c.y[0] = held(&[KeyV], GamepadButton::North);

    for keycode in keys.get_pressed() {
        if let Some(scancode) = keycode_to_scancode(*keycode) {
            // While an extra window is focused, only the editor's keys reach the
            // shared console — so the primary sim's raw-key shortcuts (palette
            // swaps, load-from-memory…) don't fire from keys meant for an extra
            // view. Typed characters (below) always pass, so text entry works in
            // any focused editor. The editor-key allowlist lives on the engine
            // (`MapViewer::wants_key`) so the host can't drift from what the
            // editor actually reads; the keys are inert unless an editor reads
            // them.
            if drives_player || egg_core::gamestate::mapeditor::MapViewer::wants_key(scancode) {
                game.system.input().press_key(scancode);
            }
        }
    }
    for event in keyboard_events.read() {
        if event.state == ButtonState::Pressed
            && let Some(text) = event.text.as_ref()
            && let Some(c) = text.chars().next()
            && !c.is_control()
        {
            game.system.input().push_char(c);
        }
    }

    // Map the FOCUSED window's cursor to its framebuffer pixel, so the editor in
    // whichever window is focused (primary or an extra view) gets the right
    // cursor. The screen sprite is centred and drawn at `scale` device px per
    // framebuffer px, so invert that: subtract the centring letterbox, then
    // divide by the scale.
    if let Some((_, window, primary)) = windows.iter().find(|(_, w, _)| w.focused)
        && let Some(pos) = window.cursor_position()
    {
        let (fb_w, fb_h, scale) = if primary {
            let fb_w = game.system.width() as f32;
            let fb_h = game.system.height() as f32;
            let scale = match game.screen_mode {
                ScreenMode::Fit => screen_scale(window, &game.scale_mode),
                ScreenMode::Mirror => game.mirror_scale.max(1) as f32,
            };
            (fb_w, fb_h, scale)
        } else if let views::Focus::Extra(i) = routing.focus {
            // Extra views render Mirror-style: the framebuffer is the window ÷
            // the view's pixel ratio and the sprite is scaled by exactly that
            // ratio (see `views::update_views`/`resize_views`).
            let v = &views.views[i];
            (
                v.output.width() as f32,
                v.output.height() as f32,
                v.scale.max(1) as f32,
            )
        } else {
            // Focused window is neither the primary nor a known view (shouldn't
            // happen); fall back to the base resolution unscaled.
            (WIDTH as f32, HEIGHT as f32, 1.0)
        };
        let (mx, my) = framebuffer_pixel(
            pos,
            Vec2::new(window.width(), window.height()),
            Vec2::new(fb_w, fb_h),
            scale,
        );
        game.system.input().mouse.x[0] = mx;
        game.system.input().mouse.y[0] = my;
        game.system.input().mouse.left[0] = mouse_button.pressed(MouseButton::Left);
        game.system.input().mouse.right[0] = mouse_button.pressed(MouseButton::Right);
        game.system.input().mouse.middle[0] = mouse_button.pressed(MouseButton::Middle);
    }

    // Pause (P) and single-step (N) are toggled in `hotkeys::primary_hotkeys`;
    // here the paused sim just keeps showing the overlay and skips stepping.
    if game.pause {
        draw_overlay(&mut game, "Paused\n[P] to unpause\n[N] to step forward");
        return;
    }
    // While the map editor is capturing typed text — in the primary window OR
    // any extra view — step the game (so the primary processes its keystrokes)
    // and skip all global debug/cheat hotkeys, so dialogue keys like
    // "town_lamppost" don't fire the m/n/k/l/p shortcuts. (Typed characters go
    // into the shared console and are consumed by whichever editor is focused.)
    if routing.editor_typing {
        game.run();
        return;
    }
    // When an extra view is focused, its arrow keys / `L` / editor are handled in
    // `views::update_views`; here we just keep the primary simulating + drawing
    // (with a zeroed player controller, set above) and skip the primary window's
    // global gameplay/debug hotkeys so they don't fire from keys aimed elsewhere.
    if !drives_player {
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
        let mut new = if rand < 64 {
            egg_core::player::Shell::ellie()
        } else if rand < 128 {
            egg_core::player::Shell::dog()
        } else if rand < 192 {
            egg_core::player::Shell::bro()
        } else {
            egg_core::player::Shell::may()
        };
        new.pos = pos;
        game.state.walkaround.entities.push(new);
        info!("we have {} entities", game.state.walkaround.entities.len());
    } else if keys.pressed(KeyCode::Digit3) && keys.pressed(KeyCode::ControlLeft) {
        let pos = game.state.walkaround.player().pos;
        for e in game.state.walkaround.entities.iter_mut() {
            let normalised = e.pos - pos;
            let (x, y) = (normalised.x as f32 * 0.9, normalised.y as f32 * 0.9);
            e.pos = egg_core::position::Vec2::new(x as i16, y as i16) + pos;
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

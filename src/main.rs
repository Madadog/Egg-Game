use bevy::asset::LoadState;
use bevy::asset::RenderAssetUsages;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use egg_core::EggState;

use egg_core::gamestate::GameMode;
use egg_core::system::ConsoleApi;
use egg_core::system::{HEIGHT, ScanCode, WIDTH};
use fantasy_console::FantasyConsole;
use script_asset::{ScriptAsset, ScriptPlugin};
use tiled::{TiledMap, TiledMapPlugin};

mod fantasy_console;
mod save;
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
        .add_systems(Startup, (setup, setup_assets, load_save_game))
        .add_systems(
            Update,
            (
                load_assets,
                poll_language_change,
                resize_screen,
                views::resize_views,
                views::handle_closed_views,
            ),
        )
        .add_systems(
            FixedUpdate,
            (
                step_state,
                views::update_views,
                autosave,
                play_sounds,
                play_music,
                update_texture,
            )
                .chain(),
        )
        .add_systems(Last, save_on_exit)
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

#[derive(Debug, Resource)]
pub struct GameAssets {
    pub font: Handle<Image>,
    pub sheet: Handle<Image>,
    pub maps: Vec<Handle<TiledMap>>,
    /// Base file names for `maps` (same order). Threaded to the console so an
    /// edited "modern" map can be saved back to `maps/<name>.tmj`.
    pub map_names: Vec<String>,
    pub script: Handle<ScriptAsset>,
}
impl GameAssets {
    pub fn new(assets: &AssetServer) -> Self {
        Self {
            font: assets.load("fonts/tic80_font.png"),
            sheet: assets.load("sprites/sheet.png"),
            maps: vec![
                assets.load("maps/bank1.tmj"),
                assets.load("maps/bank2.tmj"),
                assets.load("maps/office.tmj"),
            ],
            map_names: vec!["bank1".into(), "bank2".into(), "office".into()],
            script: assets.load("script/en.eggtext"),
        }
    }
    pub fn load_state(&self, assets: &AssetServer) -> LoadState {
        [
            self.font.id().untyped(),
            self.sheet.id().untyped(),
            self.script.id().untyped(),
        ]
        .into_iter()
        .chain(self.maps.iter().map(|m| m.id().untyped()))
        .map(|id| assets.get_load_state(id).unwrap())
        .find(|state| !matches!(state, LoadState::Loaded))
        .unwrap_or(LoadState::Loaded)
    }
}

fn setup_assets(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(GameAssets::new(&assets));
    commands.insert_resource(SfxAssets::new(&assets));
}

fn load_assets(
    mut commands: Commands,
    game_assets: Option<Res<GameAssets>>,
    assets: Res<AssetServer>,
    images: Res<Assets<Image>>,
    maps: Res<Assets<TiledMap>>,
    scripts: Res<Assets<ScriptAsset>>,
    mut state: ResMut<EggGame>,
) {
    if let Some(game_assets) = game_assets {
        match game_assets.load_state(&assets) {
            LoadState::Loaded => {
                let font = images.get(&game_assets.font);
                let sheet = images.get(&game_assets.sheet);
                if let (Some(font), Some(sheet)) = (font, sheet) {
                    let maps: Vec<(String, TiledMap)> = game_assets
                        .map_names
                        .iter()
                        .cloned()
                        .zip(
                            game_assets
                                .maps
                                .iter()
                                .map(|x| maps.get(x).cloned().expect("Map missing!")),
                        )
                        .collect();
                    state.system.set_font(font);
                    state.system.set_sprites(sheet);
                    let palette = state.state.draw_state.palettes[0].clone();
                    state.system.set_indexed_sprites(sheet, &palette);
                    state.system.set_maps(maps);
                    if let Some(script) = scripts.get(&game_assets.script) {
                        state.system.script_mut().set_base(script.0.clone());
                    }
                    // Mirror sprites + flags into DrawState (the authoritative
                    // copies for the new draw paths). Maps stay on the console
                    // and are read during drawing via `maps()`. The console also
                    // keeps copies for asset-side queries (e.g.
                    // Collider::from_sprite reads via get_bitmap_indexed).
                    state.state.draw_state.rgba_sprites = state.system.sprites.clone();
                    state.state.draw_state.indexed_sprites = state.system.indexed_sprites.clone();
                    state.state.draw_state.sprite_flags = state.system.sprite_flags.clone();
                    state.loaded = true;
                    info!("Finished loading assets.");
                    commands.remove_resource::<GameAssets>();
                }
            }
            LoadState::Loading | LoadState::NotLoaded => {}
            x => panic!("Could not load assets: {x:?}"),
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
    if pending.0.is_none() {
        if let Some(language) = state.system.take_pending_language() {
            info!("Loading language {language:?}");
            pending.0 = Some(assets.load(format!("script/{language}.eggtext")));
        }
    }
    if let Some(handle) = pending.0.clone() {
        if let Some(script) = scripts.get(&handle) {
            state.system.script_mut().set_language(script.0.clone());
            pending.0 = None;
            info!("Switched active language.");
        }
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
        ScaleMode::Integer => fit.floor(),
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
    let colour = egg_core::system::image::Rgba::from_rgb(game.state.draw_state.palettes[0][12]);
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

#[allow(clippy::too_many_arguments)]
fn step_state(
    mut game: ResMut<EggGame>,
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard_events: MessageReader<KeyboardInput>,
    mut windows: Query<(Entity, &mut Window, Has<bevy::window::PrimaryWindow>)>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut views: ResMut<views::ViewWindows>,
) {
    if !game.loaded {
        return;
    }

    game.system.input().refresh();

    // Which window owns keyboard input this frame? Only the primary window
    // drives the player; an extra window drives its own free camera/editor
    // (handled in `views::update_views`). We gate the player controller's dpad
    // on this so arrow keys panning an extra view can't also move the player.
    let focused_entity = windows.iter().find(|(_, w, _)| w.focused).map(|(e, ..)| e);
    let view_entities: Vec<Entity> = views.views.iter().map(|v| v.window).collect();
    let focus = views::resolve_focus(focused_entity, &view_entities);
    let drives_player = views::drives_player(focus);

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
            // While an extra window is focused, only the editor's text-entry
            // scancodes reach the shared console — so the primary sim's raw-key
            // shortcuts (palette swaps, load-from-memory…) don't fire from keys
            // meant for an extra view. Typed characters (below) always pass, so
            // text entry works in any focused editor.
            let editor_key = matches!(
                scancode,
                ScanCode::Backspace | ScanCode::Escape | ScanCode::Return
            );
            if drives_player || editor_key {
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

    // Screen-mode hotkeys + the scale-factor override apply to the PRIMARY
    // window only — they drive the main framebuffer. F8 opens an extra view.
    if let Some((_, mut window, _)) = windows.iter_mut().find(|(.., primary)| *primary) {
        window.resolution.set_scale_factor_override(Some(1.0));
        if keys.just_pressed(KeyCode::F11) {
            use bevy::window::WindowMode;
            window.mode = match window.mode {
                WindowMode::Windowed => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                _ => WindowMode::Windowed,
            };
        }
        if keys.just_pressed(KeyCode::F5) {
            game.scale_mode = match game.scale_mode {
                ScaleMode::Linear => ScaleMode::Integer,
                _ => ScaleMode::Linear,
            };
        }
        // F2 toggles fixed-resolution Fit vs. window-mirroring. F3 cycles the
        // Mirror pixel ratio 1→2→4→8 — and since that ratio only means anything
        // in Mirror, F3 also flips Fit→Mirror so it's never a silent no-op (the
        // "F3 does nothing in Fit" surprise). Both log so it's clear the key
        // registered and what state you're now in.
        if keys.just_pressed(KeyCode::F2) {
            game.screen_mode = match game.screen_mode {
                ScreenMode::Fit => ScreenMode::Mirror,
                ScreenMode::Mirror => ScreenMode::Fit,
            };
            let mode = if matches!(game.screen_mode, ScreenMode::Fit) { "Fit" } else { "Mirror" };
            info!("Screen mode: {mode} ({}x)", game.mirror_scale);
        }
        if keys.just_pressed(KeyCode::F3) {
            if matches!(game.screen_mode, ScreenMode::Fit) {
                // Enter Mirror first (at the current ratio) so F3 always shows
                // a visible change rather than silently doing nothing in Fit.
                game.screen_mode = ScreenMode::Mirror;
            } else {
                game.mirror_scale = match game.mirror_scale {
                    1 => 2,
                    2 => 4,
                    4 => 8,
                    _ => 1,
                };
            }
            info!("Screen mode: Mirror ({}x)", game.mirror_scale);
        }
    }
    // F8 spawns an extra walkaround window with its own free camera, starting at
    // the main camera's current position. Only in walkaround (where the camera
    // is meaningful and the editor lives).
    if keys.just_pressed(KeyCode::F8)
        && matches!(game.state.gamestate, GameMode::Walkaround)
    {
        let start = game.state.walkaround.camera.pos;
        views::spawn_view(
            &mut commands,
            &mut images,
            &mut views,
            start,
            &game.state.draw_state,
        );
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
        } else {
            // Extra views render at the fixed base resolution and fit-scale
            // their sprite (see `views::resize_views`).
            let fb_w = WIDTH as f32;
            let fb_h = HEIGHT as f32;
            let scale = (window.width() / fb_w).min(window.height() / fb_h);
            (fb_w, fb_h, scale)
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

    if keys.just_pressed(KeyCode::KeyP) {
        game.pause = !game.pause;
    }
    if game.pause {
        if keys.just_pressed(KeyCode::KeyN) {
            game.run();
        }
        draw_overlay(&mut game, "Paused\n[P] to unpause\n[N] to step forward");
        return;
    }
    // While the map editor is capturing typed text — in the primary window OR
    // any extra view — step the game (so the primary processes its keystrokes)
    // and skip all global debug/cheat hotkeys, so dialogue keys like
    // "town_lamppost" don't fire the m/n/k/l/p shortcuts. (Typed characters go
    // into the shared console and are consumed by whichever editor is focused.)
    if game.state.walkaround.map_viewer.is_typing() || views.any_editor_typing() {
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
    if keys.just_pressed(KeyCode::KeyD) && keys.pressed(KeyCode::ShiftLeft) {
        let d = &game.state.debug_info;
        d.set_player_info(!d.player_info());
    }
    if keys.just_pressed(KeyCode::KeyM) {
        let d = &game.state.debug_info;
        d.set_map_info(!d.map_info());
    }
    if keys.just_pressed(KeyCode::KeyN) {
        let d = &game.state.debug_info;
        d.set_memory_info(!d.memory_info());
    }
    // Shift+digit: swap player one for a preset shell.
    if keys.pressed(KeyCode::ShiftLeft) {
        use egg_core::player::Shell;
        let swap = if keys.just_pressed(KeyCode::Digit1) {
            Some(Shell::ellie())
        } else if keys.just_pressed(KeyCode::Digit2) {
            Some(Shell::may())
        } else if keys.just_pressed(KeyCode::Digit4) {
            Some(Shell::dog())
        } else if keys.just_pressed(KeyCode::Digit5) {
            Some(Shell::bro())
        } else {
            None
        };
        if let Some(shell) = swap {
            game.state.walkaround.player().replace(shell);
        }
    }
    if keys.pressed(KeyCode::Digit3) && keys.pressed(KeyCode::ShiftLeft) {
        let pos = game.state.walkaround.player().pos;
        let rand = game.system.rng().rand_u8();
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
    if keys.just_pressed(KeyCode::Digit6) && keys.pressed(KeyCode::ShiftLeft) {
        let player = game.state.walkaround.player().clone();
        if let Some(shell) = game.state.walkaround.entities.get_mut(0) {
            let temp = shell.clone();
            *shell = player;
            *game.state.walkaround.player() = temp;
        }
    }

    if keys.just_pressed(KeyCode::KeyL) && keys.pressed(KeyCode::ShiftLeft) {
        info!("------------------------");
        info!("START CURRENT MAP");
        info!("------------------------");
        info!("{:#?}", game.state.walkaround.current_map);
        info!("------------------------");
        info!("END CURRENT MAP");
        info!("------------------------");
    } else if keys.just_pressed(KeyCode::KeyL) {
        game.state.walkaround.map_viewer.focused = !game.state.walkaround.map_viewer.focused;
        game.state.walkaround.map_viewer.layer_index = 0;
    }
    if keys.just_pressed(KeyCode::Semicolon) && !game.state.walkaround.current_map.layers.is_empty()
    {
        info!("------------------------");
        info!("REMOVED BG LAYER");
        info!("{:#?}", game.state.walkaround.current_map.layers.remove(0));
        info!("------------------------");
    }
    if keys.just_pressed(KeyCode::Quote) && !game.state.walkaround.current_map.fg_layers.is_empty()
    {
        info!("------------------------");
        info!("REMOVED FG LAYER");
        info!(
            "{:#?}",
            game.state.walkaround.current_map.fg_layers.remove(0)
        );
        info!("------------------------");
    }
    if keys.just_pressed(KeyCode::KeyK) {
        game.state.gamestate = GameMode::SpriteTest(0);
    }

    game.run();
    if keys.pressed(KeyCode::KeyN) {
        game.run();
        draw_overlay(&mut game, "Fast-Forward");
    }
}

/// Load `save.json` into the console at startup, and seed the autosave tracker
/// so the first frame doesn't rewrite an unchanged save. A missing or unreadable
/// save falls back to a fresh `SaveData::default()`.
fn load_save_game(mut game: ResMut<EggGame>, mut commands: Commands) {
    let loaded = save::load().unwrap_or_default();
    *game.system.memory() = loaded;
    commands.insert_resource(save::SaveTracker { last: loaded });
}

/// Flush save data to disk whenever it differs from the last value written.
/// Runs after `step_state`, so it captures the frame's changes.
fn autosave(game: Res<EggGame>, mut tracker: ResMut<save::SaveTracker>) {
    if !game.loaded {
        return;
    }
    let current = game.system.save_data();
    if current != tracker.last {
        save::write(&current);
        tracker.last = current;
    }
}

/// Flush the latest save data when the app is closing, in case it changed on
/// the same frame as the exit (before the next `autosave` could run).
fn save_on_exit(
    mut exit: MessageReader<AppExit>,
    game: Res<EggGame>,
    tracker: Option<ResMut<save::SaveTracker>>,
) {
    if exit.is_empty() {
        return;
    }
    exit.clear();
    let Some(mut tracker) = tracker else { return };
    let current = game.system.save_data();
    if current != tracker.last {
        save::write(&current);
        tracker.last = current;
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

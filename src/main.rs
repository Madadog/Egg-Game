use bevy::asset::LoadState;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use egg_core::EggState;

use egg_core::gamestate::GameMode;
use egg_core::system::ConsoleApi;
use egg_core::system::{HEIGHT, ScanCode, WIDTH};
use fantasy_console::FantasyConsole;
use tiled::{TiledMap, TiledMapPlugin};

mod fantasy_console;
mod tiled;

/// Bevy frontend: Stores console and game state. Plus stuff for loading assets, pausing sim and window management.
#[derive(Resource)]
pub struct EggGame {
    pub state: EggState,
    system: FantasyConsole,

    pub loaded: bool,
    pub pause: bool,
    pub scale_mode: ScaleMode,
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
        }
    }
}

pub enum ScaleMode {
    Linear,
    Integer,
}

fn main() {
    App::new()
        .init_resource::<EggGame>()
        .insert_resource(ClearColor(Color::srgb(0.102, 0.110, 0.173)))
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        .add_plugins(TiledMapPlugin)
        .add_systems(Startup, (setup, setup_assets))
        .add_systems(Update, (load_assets, resize_screen))
        .add_systems(
            FixedUpdate,
            (step_state, play_sounds, play_music, update_texture).chain(),
        )
        // 64 FPS
        .insert_resource(Time::<Fixed>::default())
        .run();
}

fn setup(mut commands: Commands, _assets: Res<AssetServer>, mut images: ResMut<Assets<Image>>) {
    commands.spawn(Camera2d);
    let screen = Image::new_fill(
        Extent3d {
            width: WIDTH as u32,
            height: HEIGHT as u32,
            ..default()
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::all(),
    );
    let screen_handle = images.add(screen);
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
        }
    }
    pub fn load_state(&self, assets: &AssetServer) -> LoadState {
        let mut ids = vec![];
        ids.push(self.font.id().untyped());
        ids.push(self.sheet.id().untyped());
        for map in &self.maps {
            ids.push(map.id().untyped());
        }
        for id in ids {
            let load_state = assets.get_load_state(id).unwrap();
            match load_state {
                LoadState::NotLoaded | LoadState::Loading | LoadState::Failed(_) => {
                    return load_state;
                }
                LoadState::Loaded => (),
            };
        }
        LoadState::Loaded
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
    mut state: ResMut<EggGame>,
) {
    if let Some(game_assets) = game_assets {
        match game_assets.load_state(&assets) {
            LoadState::Loaded => {
                let font = images.get(&game_assets.font);
                let sheet = images.get(&game_assets.sheet);
                if let (Some(font), Some(sheet)) = (font, sheet) {
                    println!("Okay I got the fonts and stuff!");
                    let maps: Vec<Option<TiledMap>> = game_assets
                        .maps
                        .iter()
                        .map(|x| maps.get(x).cloned())
                        .collect();
                    let maps: Vec<TiledMap> = maps
                        .into_iter()
                        .map(|map| map.expect("Map missing!"))
                        .collect();
                    println!("Got maps!");
                    state.system.set_font(font);
                    println!("Set fonts!");
                    state.system.set_sprites(sheet);
                    println!("And sprites!");
                    state.system.set_indexed_sprites(sheet);
                    println!("And more sprites!");
                    info!("Loaded {} maps", maps.len());
                    state.system.set_maps(maps);
                    println!("Just set the maps!!");
                    // Phase 2 scaffold: mirror loaded assets into DrawState so
                    // the migrated draw paths can read from there. Removed in
                    // Phase 4 once loaders write directly to DrawState.
                    state.state.draw_state.rgba_sprites = state.system.sprites.clone();
                    state.state.draw_state.indexed_sprites = state.system.indexed_sprites.clone();
                    state.state.draw_state.maps = state.system.maps.clone();
                    state.state.draw_state.sprite_flags = state.system.sprite_flags.clone();
                    state.state.draw_state.palettes[0] = state.system.palette.clone();
                    state.loaded = true;
                    info!("Finished loading assets.");
                    commands.remove_resource::<GameAssets>();
                }
            }
            LoadState::Loading => info!("Loading assets..."),
            LoadState::NotLoaded => info!("Not yet loaded..."),
            x => panic!("Could not load assets: {x:?}"),
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

fn play_sounds(mut commands: Commands, game_assets: Res<SfxAssets>, mut state: ResMut<EggGame>) {
    for (name, options) in state.system.sounds() {
        if let Some(sound) = game_assets.sounds.get(&name.to_string()) {
            let speed =
                2.0_f32.powf((options.note as f32 + (options.octave as f32 - 5.0) * 12.0) / 12.0);
            commands.spawn((
                AudioPlayer(sound.clone()),
                PlaybackSettings {
                    mode: bevy::audio::PlaybackMode::Despawn,
                    volume: bevy::audio::Volume::Decibels(-6.0),
                    speed,
                    paused: false,
                    ..Default::default()
                },
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
        if query.iter().len() == 0 && !*playing {
            let music: Handle<AudioSource> = assets.load(format!("music/{}.ogg", x.id));
            commands.spawn((
                AudioPlayer(music.clone()),
                PlaybackSettings {
                    mode: bevy::audio::PlaybackMode::Loop,
                    volume: bevy::audio::Volume::Decibels(-6.0),
                    speed: 1.0,
                    paused: false,
                    ..Default::default()
                },
                MusicPlayer,
            ));
            *playing = true;
        }
    } else {
        for (entity, sink) in query.iter_mut() {
            info!("Stoppin mussic");
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
    mut state: ResMut<EggGame>,
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
    let colour = state.system.get_border_colour();
    border_colour.0 = Color::srgb_u8(colour[0], colour[1], colour[2]);
}

fn resize_screen(
    mut sprite: Query<&mut Transform, With<GameScreenSprite>>,
    mut window: Query<&mut Window>,
    state: Res<EggGame>,
) {
    if let Ok(mut window) = window.single_mut() {
        let w = window.width() / WIDTH as f32;
        let h = window.height() / HEIGHT as f32;
        window.resolution.set_scale_factor_override(Some(1.0));
        window.title = "Egg Game".to_string();
        let size = match state.scale_mode {
            ScaleMode::Integer => w.min(h).floor(),
            ScaleMode::Linear => w.min(h),
        };
        for mut transform in sprite.iter_mut() {
            transform.scale = Vec3::new(size, size, 1.0);
        }
    }
}

fn step_state(
    mut game: ResMut<EggGame>,
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard_events: EventReader<KeyboardInput>,
    mut window: Query<&mut Window>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    gamepads: Query<(Entity, &Gamepad)>,
) {
    if !game.loaded {
        return;
    }

    game.system.input().refresh();
    game.system.sync_helper().step();

    let (
        mut up,
        mut down,
        mut left,
        mut right,
        mut a_button,
        mut b_button,
        mut x_button,
        mut y_button,
    ) = (
        keys.any_pressed([KeyCode::ArrowUp, KeyCode::KeyW]),
        keys.any_pressed([KeyCode::ArrowDown, KeyCode::KeyS]),
        keys.any_pressed([KeyCode::ArrowLeft, KeyCode::KeyA]),
        keys.any_pressed([KeyCode::ArrowRight, KeyCode::KeyD]),
        keys.any_pressed([KeyCode::KeyZ, KeyCode::Space, KeyCode::Enter, KeyCode::KeyE]),
        keys.any_pressed([KeyCode::KeyX, KeyCode::Escape, KeyCode::KeyQ]),
        keys.any_pressed([KeyCode::KeyC]),
        keys.any_pressed([KeyCode::KeyV]),
    );
    if let Some((_, gamepad)) = gamepads.iter().next() {
        up |= gamepad.pressed(GamepadButton::DPadUp)
            || gamepad.get(GamepadAxis::LeftStickY).unwrap() > 0.2;
        down |= gamepad.pressed(GamepadButton::DPadDown)
            || gamepad.get(GamepadAxis::LeftStickY).unwrap() < -0.2;
        left |= gamepad.pressed(GamepadButton::DPadLeft)
            || gamepad.get(GamepadAxis::LeftStickX).unwrap() < -0.2;
        right |= gamepad.pressed(GamepadButton::DPadRight)
            || gamepad.get(GamepadAxis::LeftStickX).unwrap() > 0.2;
        a_button |= gamepad.pressed(GamepadButton::South);
        b_button |= gamepad.pressed(GamepadButton::East);
        x_button |= gamepad.pressed(GamepadButton::West);
        y_button |= gamepad.pressed(GamepadButton::North);
    }

    if up {
        game.system.input().press(0);
    }
    if down {
        game.system.input().press(1);
    }
    if left {
        game.system.input().press(2);
    }
    if right {
        game.system.input().press(3);
    }
    if a_button {
        game.system.input().press(4);
    }
    if b_button {
        game.system.input().press(5);
    }
    if x_button {
        game.system.input().press(6);
    }
    if y_button {
        game.system.input().press(7);
    }
    for keycode in keys.get_pressed() {
        if let Some(scancode) = keycode_to_scancode(*keycode) {
            game.system.input().press_key(scancode);
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

    if let Ok(mut window) = window.single_mut() {
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
        if let Some(pos) = window.cursor_position() {
            let w = window.width() / WIDTH as f32;
            let h = window.height() / HEIGHT as f32;
            let size = if matches!(game.scale_mode, ScaleMode::Integer) {
                w.min(h).floor()
            } else {
                w.min(h)
            };
            let (x_offset, y_offset) = if w > h {
                ((window.width() - WIDTH as f32 * size) / 2.0, 0.0)
            } else {
                (0.0, (window.height() - HEIGHT as f32 * size) / 2.0)
            };
            game.system.input().mouse.x = ((pos.x - x_offset) / size) as i16;
            game.system.input().mouse.y = ((pos.y - y_offset) / size) as i16;
            game.system.input().mouse.left = mouse_button.pressed(MouseButton::Left);
            game.system.input().mouse.right = mouse_button.pressed(MouseButton::Right);
            game.system.input().mouse.middle = mouse_button.pressed(MouseButton::Middle);
        }
    }

    if keys.just_pressed(KeyCode::KeyP) {
        game.pause = !game.pause;
    }
    if game.pause {
        if keys.just_pressed(KeyCode::KeyN) {
            game.run();
        }
        game.system.print_raw(
            "Paused\n[P] to unpause\n[N] to step forward",
            100,
            62,
            egg_core::system::PrintOptions {
                color: 12,
                ..Default::default()
            },
        );
        return;
    }
    if keys.just_pressed(KeyCode::KeyD) && keys.pressed(KeyCode::ShiftLeft) {
        let x = !game.state.debug_info.player_info();
        game.state.debug_info.set_player_info(x);
    }
    if keys.just_pressed(KeyCode::KeyM) {
        let x = !game.state.debug_info.map_info();
        game.state.debug_info.set_map_info(x);
    }
    if keys.just_pressed(KeyCode::KeyN) {
        let x = !game.state.debug_info.memory_info();
        game.state.debug_info.set_memory_info(x);
    }
    if keys.just_pressed(KeyCode::Digit1) && keys.pressed(KeyCode::ShiftLeft) {
        game.state
            .walkaround
            .player()
            .replace(egg_core::player::Shell::ellie());
    }
    if keys.just_pressed(KeyCode::Digit2) && keys.pressed(KeyCode::ShiftLeft) {
        game.state
            .walkaround
            .player()
            .replace(egg_core::player::Shell::may());
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
    if keys.just_pressed(KeyCode::Digit4) && keys.pressed(KeyCode::ShiftLeft) {
        game.state
            .walkaround
            .player()
            .replace(egg_core::player::Shell::dog());
    }
    if keys.just_pressed(KeyCode::Digit5) && keys.pressed(KeyCode::ShiftLeft) {
        game.state
            .walkaround
            .player()
            .replace(egg_core::player::Shell::bro());
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
        game.system.print_raw(
            "Fast-Forward",
            100,
            62,
            egg_core::system::PrintOptions {
                color: 12,
                ..Default::default()
            },
        );
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

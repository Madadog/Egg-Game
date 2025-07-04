use bevy::asset::LoadState;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use egg_core::gamestate::inventory::InventoryUi;

use egg_core::gamestate::{GameState, walkaround::WalkaroundState};
use egg_core::system::ConsoleApi;
use egg_core::{debug::DebugInfo, rand::Pcg32};
use fantasy_console::FantasyConsole;
use tiled::{TiledMap, TiledMapPlugin};

mod fantasy_console;
mod tiled;

#[derive(Resource)]
pub struct EggState {
    pub walkaround: WalkaroundState,
    pub inventory_ui: InventoryUi,
    pub gamestate: GameState,

    pub system: FantasyConsole,

    pub time: i32,
    pub pause: bool,
    pub rng: Pcg32,
    pub debug_info: DebugInfo,
    pub bg_colour: u8,
    pub loaded: bool,

    pub scale_mode: ScaleMode,
}
impl EggState {
    pub fn run(&mut self) {
        self.gamestate.run(
            &mut self.walkaround,
            &mut self.debug_info,
            self.time,
            &mut self.inventory_ui,
            &mut self.system,
        );
    }
}
impl Default for EggState {
    fn default() -> Self {
        EggState {
            walkaround: WalkaroundState::new(),
            inventory_ui: InventoryUi::new(),
            gamestate: GameState::Animation(0),

            system: FantasyConsole::new(),

            time: 0,
            pause: false,
            rng: Pcg32::default(),
            debug_info: DebugInfo::default(),
            bg_colour: 0,
            loaded: false,

            scale_mode: ScaleMode::Linear,
        }
    }
}

pub enum ScaleMode {
    Linear,
    Integer,
}

#[derive(Component)]
pub struct TicSpriteLayer {
    pub colour: usize,
    pub sprite_index: usize,
}
impl TicSpriteLayer {
    pub fn new(colour: usize, sprite_index: usize) -> Self {
        Self {
            colour,
            sprite_index,
        }
    }
}

fn main() {
    App::new()
        .init_resource::<EggState>()
        .insert_resource(ClearColor(Color::srgb(0.102, 0.110, 0.173)))
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        .add_plugins(TiledMapPlugin)
        .add_systems(Startup, (setup, setup_assets))
        .add_systems(Update, (load_assets, resize_screen))
        .add_systems(
            FixedUpdate,
            (step_state, play_sounds, play_music, update_texture).chain(),
        )
        // 60 FPS
        .insert_resource(Time::<Fixed>::default())
        .run();
}

fn setup(mut commands: Commands, _assets: Res<AssetServer>, mut images: ResMut<Assets<Image>>) {
    commands.spawn(Camera2d::default());
    let screen = Image::new_fill(
        Extent3d {
            width: 240,
            height: 136,
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
    pub fn load_state(&self, _assets: &AssetServer) -> LoadState {
        // assets.get_group_load_state(
        //     [self.font.id(), self.sheet.id()]
        //         .iter()
        //         .cloned()
        //         .chain(self.maps.iter().map(|map| map.id())),
        // )
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
    mut state: ResMut<EggState>,
) {
    if let Some(game_assets) = game_assets {
        match game_assets.load_state(&assets) {
            LoadState::Loaded => {
                let font = images.get(&game_assets.font);
                let sheet = images.get(&game_assets.sheet);
                if let (Some(font), Some(sheet)) = (font, sheet) {
                    println!("Okay I got the fonts and stuff!");
                    // let maps = maps.get(&game_assets.maps).unwrap();
                    let maps: Vec<Option<TiledMap>> = game_assets
                        .maps
                        .iter()
                        .map(|x| maps.get(x).cloned())
                        .collect();
                    // if maps.iter().any(|x| x.is_none()) {
                    //     return;
                    // }
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

fn play_sounds(mut commands: Commands, game_assets: Res<SfxAssets>, mut state: ResMut<EggState>) {
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
    mut state: ResMut<EggState>,
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
            commands.entity(entity).despawn_recursive();
            sink.stop();
        }
    }
}

#[derive(Component)]
pub struct MusicPlayer;

#[derive(Component)]
pub struct GameScreenSprite;

// #[derive(Resource)]
// pub struct GameScreen(pub Image);

fn update_texture(
    mut state: ResMut<EggState>,
    mut images: ResMut<Assets<Image>>,
    sprite: Query<&Sprite, With<GameScreenSprite>>,
) {
    for sprite in sprite.iter() {
        state
            .system
            .to_texture(images.get_mut(&sprite.image).unwrap().data.as_mut().expect("Main screen texture uninitialized, can't draw game."));
    }
}

fn resize_screen(
    mut sprite: Query<&mut Transform, With<GameScreenSprite>>,
    mut window: Query<&mut Window>,
    state: Res<EggState>,
) {
    if let Ok(mut window) = window.single_mut() {
        let w = window.width() as f32 / 240.0;
        let h = window.height() as f32 / 136.0;
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
    mut state: ResMut<EggState>,
    keys: Res<ButtonInput<KeyCode>>,
    mut window: Query<&mut Window>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    gamepads: Query<(Entity, &Gamepad)>,
    // mut window: Query<&mut Mouse>,
) {
    if !state.loaded {
        return;
    }
    state.system.sync_helper().step();
    state.time += 1;

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
        state.system.input().press(0);
    }
    if down {
        state.system.input().press(1);
    }
    if left {
        state.system.input().press(2);
    }
    if right {
        state.system.input().press(3);
    }
    if a_button {
        state.system.input().press(4);
    }
    if b_button {
        state.system.input().press(5);
    }
    if x_button {
        state.system.input().press(6);
    }
    if y_button {
        state.system.input().press(7);
    }
    if keys.pressed(KeyCode::ControlLeft) {
        state.system.input().press_key(63);
    }
    if keys.pressed(KeyCode::ShiftLeft) {
        state.system.input().press_key(64);
    }
    if keys.pressed(KeyCode::AltLeft) {
        state.system.input().press_key(65);
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
            state.scale_mode = match state.scale_mode {
                ScaleMode::Linear => ScaleMode::Integer,
                _ => ScaleMode::Linear,
            };
        }
        if let Some(pos) = window.cursor_position() {
            let w = window.width() / 240.0;
            let h = window.height() / 136.0;
            let size = if matches!(state.scale_mode, ScaleMode::Integer) {
                w.min(h).floor()
            } else {
                w.min(h)
            };
            let (x_offset, y_offset) = if w > h {
                ((window.width() - 240.0 * size) / 2.0, 0.0)
            } else {
                (0.0, (window.height() - 136.0 * size) / 2.0)
            };
            state.system.input().mouse.x = ((pos.x - x_offset) / size) as i16;
            state.system.input().mouse.y = ((pos.y - y_offset) / size) as i16;
            state.system.input().mouse.left = mouse_button.pressed(MouseButton::Left);
            state.system.input().mouse.right = mouse_button.pressed(MouseButton::Right);
            state.system.input().mouse.middle = mouse_button.pressed(MouseButton::Middle);
        }
    }

    if keys.just_pressed(KeyCode::KeyP) {
        state.pause = !state.pause;
    }
    if state.pause {
        if keys.just_pressed(KeyCode::KeyN) {
            state.run();
            state.system.input().refresh();
        }
        state.system.print_raw(
            "Paused\n[P] to unpause\n[N] to step forward",
            100,
            62,
            egg_core::tic80_api::core::PrintOptions {
                color: 12,
                ..Default::default()
            },
        );
        return;
    }
    if keys.just_pressed(KeyCode::KeyD) && keys.pressed(KeyCode::ShiftLeft) {
        let x = !state.debug_info.player_info();
        state.debug_info.set_player_info(x);
    }
    if keys.just_pressed(KeyCode::KeyM) {
        let x = !state.debug_info.map_info();
        state.debug_info.set_map_info(x);
    }
    if keys.just_pressed(KeyCode::KeyN) {
        let x = !state.debug_info.memory_info();
        state.debug_info.set_memory_info(x);
    }

    if keys.just_pressed(KeyCode::KeyL) && keys.pressed(KeyCode::ShiftLeft) {
        info!("------------------------");
        info!("START CURRENT MAP");
        info!("------------------------");
        info!("{:#?}", state.walkaround.current_map);
        info!("------------------------");
        info!("END CURRENT MAP");
        info!("------------------------");
    } else if keys.just_pressed(KeyCode::KeyL) {
        state.walkaround.map_viewer.focused = !state.walkaround.map_viewer.focused;
        state.walkaround.map_viewer.layer_index = 0;
    }
    if keys.just_pressed(KeyCode::Semicolon) && state.walkaround.current_map.layers.len() > 0 {
        info!("------------------------");
        info!("REMOVED BG LAYER");
        info!("{:#?}", state.walkaround.current_map.layers.remove(0));
        info!("------------------------");
    }
    if keys.just_pressed(KeyCode::Quote) && state.walkaround.current_map.fg_layers.len() > 0 {
        info!("------------------------");
        info!("REMOVED FG LAYER");
        info!("{:#?}", state.walkaround.current_map.fg_layers.remove(0));
        info!("------------------------");
    }
    if keys.just_pressed(KeyCode::KeyK) {
        state.gamestate = GameState::SpriteTest(0);
    }

    state.run();
    if keys.pressed(KeyCode::KeyN) {
        state.run();
        state.system.print_raw(
            "Fast-Forward",
            100,
            62,
            egg_core::tic80_api::core::PrintOptions {
                color: 12,
                ..Default::default()
            },
        );
    }
    state.system.input().refresh();
}

use bevy::asset::LoadState;
use bevy::audio::VolumeLevel;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::utils::HashMap;
use egg_core::gamestate::inventory::InventoryUi;

use egg_core::gamestate::{walkaround::WalkaroundState, GameState};
use egg_core::system::{ConsoleApi, DrawParams};
use egg_core::{debug::DebugInfo, rand::Pcg32};
use fantasy_console::FantasyConsole;
use tiled::{TiledMap, TiledMapPlugin};

// static WALKAROUND_STATE: RwLock<WalkaroundState> = RwLock::new(WalkaroundState::new());
// static TIME: AtomicI32 = AtomicI32::new(0);
// static PAUSE: AtomicBool = AtomicBool::new(false);
// static RNG: RwLock<Lazy<Pcg32>> = RwLock::new(Lazy::new(Pcg32::default));
// static DEBUG_INFO: DebugInfo = DebugInfo::const_default();
// static GAMESTATE: RwLock<GameState> = RwLock::new(GameState::Animation(0));
// static BG_COLOUR: AtomicU8 = AtomicU8::new(0);
// static SYNC_HELPER: SyncHelper = SyncHelper::new();
mod fantasy_console;
mod tiled;

#[derive(Resource)]
pub struct EggState {
    pub walkaround: WalkaroundState<'static>,
    pub inventory_ui: InventoryUi,
    pub time: i32,
    pub pause: bool,
    pub rng: Pcg32,
    pub debug_info: DebugInfo,
    pub gamestate: GameState,
    pub bg_colour: u8,
    pub system: FantasyConsole,
    pub loaded: bool,
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
            time: 0,
            pause: false,
            rng: Pcg32::default(),
            debug_info: DebugInfo::const_default(),
            gamestate: GameState::Animation(0),
            bg_colour: 0,
            system: FantasyConsole::new(),
            loaded: false,
        }
    }
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
        .insert_resource(ClearColor(Color::rgb(0.102, 0.110, 0.173)))
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        .add_plugins(TiledMapPlugin)
        .add_systems(Startup, (setup, setup_assets))
        .add_systems(Update, (load_assets, resize_screen))
        .add_systems(
            Update,
            (
                read_state,
                step_state,
                play_sounds,
                play_music,
                update_texture,
            )
                .chain(),
        )
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>, mut images: ResMut<Assets<Image>>) {
    commands.spawn(Camera2dBundle::default());
    let screen = Image::new_fill(
        Extent3d {
            width: 240,
            height: 136,
            ..default()
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
    );
    commands.spawn((
        SpriteBundle {
            texture: images.add(screen),
            transform: Transform::from_xyz(0., 0., 0.),
            ..default()
        },
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
            maps: vec![assets.load("maps/bank1.tmj"), assets.load("maps/bank2.tmj")],
        }
    }
    pub fn load_state(&self, assets: &AssetServer) -> LoadState {
        assets.get_group_load_state([
            self.font.id(),
            self.sheet.id(),
            self.maps[0].id(),
            self.maps[1].id(),
        ])
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
                let font = images.get(&game_assets.font).unwrap();
                let sheet = images.get(&game_assets.sheet).unwrap();
                // let maps = maps.get(&game_assets.maps).unwrap();
                let maps = game_assets
                    .maps
                    .iter()
                    .map(|x| maps.get(x).cloned().unwrap())
                    .collect();
                state.system.set_font(font);
                state.system.set_sprites(sheet);
                state.system.set_maps(maps);
                state.loaded = true;
                info!("Finished loading assets.");
                commands.remove_resource::<GameAssets>();
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
        ]);
        Self { sounds }
    }
}

fn play_sounds(mut commands: Commands, game_assets: Res<SfxAssets>, mut state: ResMut<EggState>) {
    for (name, options) in state.system.sounds() {
        if let Some(sound) = game_assets.sounds.get(&name.to_string()) {
            let speed =
                2.0_f32.powf((options.note as f32 + (options.octave as f32 - 5.0) * 12.0) / 12.0);
            commands.spawn(AudioBundle {
                source: sound.clone(),
                settings: PlaybackSettings {
                    mode: bevy::audio::PlaybackMode::Despawn,
                    volume: bevy::audio::Volume::Relative(VolumeLevel::new(0.5)),
                    speed,
                    paused: false,
                },
            });
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
            commands.spawn((
                AudioBundle {
                    source: assets.load(format!("music/{}.ogg", x.id)),
                    settings: PlaybackSettings {
                        mode: bevy::audio::PlaybackMode::Loop,
                        volume: bevy::audio::Volume::Relative(VolumeLevel::new(1.0)),
                        speed: 1.0,
                        paused: false,
                    },
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
    sprite: Query<&Handle<Image>, With<GameScreenSprite>>,
) {
    for sprite in sprite.iter() {
        state.system.to_texture(images.get_mut(sprite).unwrap());
    }
}

fn resize_screen(
    mut sprite: Query<&mut Transform, With<GameScreenSprite>>,
    window: Query<&Window>,
) {
    let x = window.get_single().unwrap().width() as f32 / 240.0;
    let y = window.get_single().unwrap().height() as f32 / 136.0;
    let size = x.min(y);
    for mut transform in sprite.iter_mut() {
        transform.scale = Vec3::new(size, size, 1.0);
    }
}

fn read_state(_state: Res<EggState>) {
    // info!("Time: {}", state.time);
}

// fn zoom_camera()

fn step_state(mut state: ResMut<EggState>, keys: Res<Input<KeyCode>>, mut window: Query<&mut Window>) {
    state.system.sync_helper().step();
    state.time += 1;

    if keys.pressed(KeyCode::Up) {
        state.system.input().press(0);
    }
    if keys.pressed(KeyCode::Down) {
        state.system.input().press(1);
    }
    if keys.pressed(KeyCode::Left) {
        state.system.input().press(2);
    }
    if keys.pressed(KeyCode::Right) {
        state.system.input().press(3);
    }
    if keys.pressed(KeyCode::Z) {
        state.system.input().press(4);
    }
    if keys.pressed(KeyCode::X) {
        state.system.input().press(5);
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
    if keys.pressed(KeyCode::F11) {
        use bevy::window::WindowMode;
        let mode = window.get_single_mut().unwrap().mode;
        window.get_single_mut().unwrap().mode = match mode {
            WindowMode::Windowed => WindowMode::Fullscreen,
            _ => WindowMode::Windowed,
        };
    }

    if keys.just_pressed(KeyCode::P) {
        state.pause = !state.pause;
        //     print!(
        //         "Paused",
        //         100,
        //         62,
        //         PrintOptions {
        //             color: 12,
        //             ..Default::default()
        //         }
        //     );
    }
    if state.pause {
        return;
    }
    if keys.just_pressed(KeyCode::D) {
        let x = !state.debug_info.player_info();
        state.debug_info.set_player_info(x);
    }
    if keys.just_pressed(KeyCode::M) {
        let x = !state.debug_info.map_info();
        state.debug_info.set_map_info(x);
    }
    if keys.just_pressed(KeyCode::N) {
        let x = !state.debug_info.memory_info();
        state.debug_info.set_memory_info(x);
    }

    // state.gamestate.run(&mut state.walkaround);
    state.run();
    if keys.pressed(KeyCode::N) {
        state.run();
        //     print_raw(
        //         "Fast-Forward\0",
        //         100,
        //         62,
        //         PrintOptions {
        //             color: 12,
        //             ..Default::default()
        //         },
        //     );
        // }
    }
    state.system.input().refresh();
    // input_manager::step_gamepad_helper();
    // input_manager::step_mouse_helper();
}

#[derive(Clone, Debug, Component)]
pub struct ImmediateMode;

fn draw_state(
    state: Res<EggState>,
    mut commands: Commands,
    sprites: Query<Entity, With<ImmediateMode>>,
) {
    // // Draw BG
    // palette_map_reset();
    // cls(self.bg_colour.load(Ordering::SeqCst));
    // self.current_map.draw_bg(self.camera.pos, false);

    for entity in sprites.iter() {
        commands.entity(entity).despawn_recursive();
    }

    // self.particles.draw_tic80(-self.cam_x(), -self.cam_y());
    // blit_segment(4);
    // // Collect sprites for drawing
    let mut sprites: Vec<DrawParams> = Vec::new();

    let walk = state.walkaround.clone();

    sprites.push(walk.player.draw_params(walk.camera.pos));

    for (anim, hitbox) in walk.map_animations.iter().zip(
        walk.current_map
            .interactables
            .iter()
            .filter(|x| x.sprite.is_some())
            .map(|x| x.hitbox),
    ) {
        sprites.push(DrawParams::new(
            anim.current_frame().spr_id.into(),
            anim.current_frame().pos.x as i32 + hitbox.x as i32 - walk.cam_x(),
            anim.current_frame().pos.y as i32 + hitbox.y as i32 - walk.cam_y(),
            anim.current_frame().options.clone(),
            anim.current_frame().outline_colour,
            anim.current_frame().palette_rotate,
        ));
    }

    sprites.extend(
        walk.creatures
            .iter()
            .map(|x| x.draw_params(walk.camera.pos)),
    );

    for (i, companion) in walk.companion_list.companions.iter().enumerate() {
        if let Some(companion) = companion {
            let (position, direction) = if i == 0 {
                walk.companion_trail.oldest()
            } else {
                walk.companion_trail.mid()
            };
            let walktime = walk.companion_trail.walktime();
            let params = companion.spr_params(position, direction, walktime, &walk.camera);
            sprites.push(params);
        }
    }

    // Sort sprites in order of Y index
    sprites.sort_by(|a, b| a.bottom().partial_cmp(&b.bottom()).unwrap());

    // // Draw sprites
    for (i, options) in sprites.into_iter().enumerate() {
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::RED,
                    custom_size: Some(Vec2::splat(8.0)),
                    ..default()
                },
                transform: Transform::from_translation(Vec3::new(
                    options.x as f32,
                    options.y as f32,
                    i as f32 / 10.0,
                )),
                ..default()
            },
            ImmediateMode,
        ));
    }

    // // Draw FG
    // palette_map_reset();
    // self.current_map.draw_fg(self.camera.pos, false);

    // if let Some(string) = &self.dialogue.current_text {
    //     self.dialogue.draw_dialogue_box(string, true);
    // }
    // if debug_info.map_info() {
    //     for warp in self.current_map.warps.iter() {
    //         warp.hitbox()
    //             .offset_xy(-self.camera.pos.x, -self.camera.pos.y)
    //             .draw(12);
    //     }
    //     self.player
    //         .hitbox()
    //         .offset_xy(-self.camera.pos.x, -self.camera.pos.y)
    //         .draw(12);
    //     for item in self.current_map.interactables.iter() {
    //         item.hitbox
    //             .offset_xy(-self.camera.pos.x, -self.camera.pos.y)
    //             .draw(14);
    //     }
    // }
    // if debug_info.player_info() {
    //     print_raw(
    //         &format!("Player: {:#?}\0", self.player),
    //         0,
    //         0,
    //         PrintOptions {
    //             small_text: true,
    //             color: 11,
    //             ..Default::default()
    //         },
    //     );
    //     print_raw(
    //         &format!("Camera: {:#?}\0", self.camera),
    //         74,
    //         0,
    //         PrintOptions {
    //             small_text: true,
    //             color: 11,
    //             ..Default::default()
    //         },
    //     );
    // }
}

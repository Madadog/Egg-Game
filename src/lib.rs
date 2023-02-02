pub mod alloc;
mod tic80;
mod tic_helpers;
mod rand;
mod map_data;
mod camera;
mod position;
mod player;
mod interact;
mod animation;
mod dialogue;
mod dialogue_data;
mod gamestate;

use tic80::*;
use crate::rand::Pcg32;
use crate::position::{Vec2, Hitbox, touches_tile};
use crate::camera::Camera;
use crate::map_data::*;
use crate::player::*;
use crate::dialogue::Dialogue;
use crate::gamestate::GameState;
use once_cell::sync::Lazy;
use std::sync::{RwLock, RwLockWriteGuard, RwLockReadGuard};
use std::sync::atomic::{AtomicBool, Ordering};

pub struct DebugInfo {
    player_info: bool,
    map_info: bool
}
impl DebugInfo {
    pub const fn const_default() -> Self {
        DebugInfo { player_info: false, map_info: false }
    }
}

static TIME: RwLock<i32> = RwLock::new(0);
static PLAYER: RwLock<Player> = RwLock::new(Player::const_default());
static ANIMATIONS: RwLock<Vec<(u16, usize)>> = RwLock::new(Vec::new());
static RNG: RwLock<Lazy<Pcg32>> = RwLock::new(Lazy::new(|| {Pcg32::default()}));
static PAUSE: AtomicBool = AtomicBool::new(false);
static CAMERA: RwLock<Camera> = RwLock::new(Camera::const_default());
static DEBUG_INFO: RwLock<DebugInfo> = RwLock::new(DebugInfo::const_default());
static CURRENT_MAP: RwLock<&MapSet> = RwLock::new(&SUPERMARKET);
static DIALOGUE: RwLock<Dialogue> = RwLock::new(Dialogue::const_default());
static GAMESTATE: RwLock<GameState> = RwLock::new(GameState::Animation(0));
static GAMEPAD_HELPER: RwLock<[u8; 4]> = RwLock::new([0; 4]);
static MAINMENU: RwLock<usize> = RwLock::new(0);
static RESET_PROTECTOR: RwLock<usize> = RwLock::new(0);

// REMINDER: Heap maxes at 8192 u32.

pub fn frames() -> i32 {
    *TIME.read().unwrap()
}
pub fn player_mut<'a>() -> RwLockWriteGuard<'a, Player> {
    PLAYER.write().unwrap()
}
pub fn player<'a>() -> RwLockReadGuard<'a, Player> {
    PLAYER.read().unwrap()
}
pub fn debug_info_mut<'a>() -> RwLockWriteGuard<'a, DebugInfo> {
    DEBUG_INFO.write().unwrap()
}
pub fn debug_info<'a>() -> RwLockReadGuard<'a, DebugInfo> {
    DEBUG_INFO.read().unwrap()
}
pub fn camera_mut<'a>() -> RwLockWriteGuard<'a, Camera> {
    CAMERA.write().unwrap()
}
pub fn camera<'a>() -> RwLockReadGuard<'a, Camera> {
    CAMERA.read().unwrap()
}
pub fn cam_x() -> i32 { i32::from(camera().pos.x)}
pub fn cam_y() -> i32 { i32::from(camera().pos.y)}
pub fn rand() -> u32 {
    RNG.write().unwrap().next_u32()
}
pub fn rand_u8() -> u8 {
    (rand() % 256).try_into().unwrap()
}
pub fn is_paused() -> bool {
    PAUSE.load(Ordering::Relaxed)
}
pub fn set_pause(pause: bool) {
    PAUSE.store(pause, Ordering::Relaxed);
}
pub fn current_map<'a>() -> RwLockReadGuard<'a, &'a MapSet<'a>> {
    CURRENT_MAP.read().unwrap()
}
pub fn load_map(map: &'static MapSet<'static>) {
    let map1 = &map.maps[0];
    *camera_mut() = Camera::from_map_size(map1.w as u8, map1.h as u8, map1.sx as i16, map1.sy as i16);
    *CURRENT_MAP.write().unwrap() = map;
    
    ANIMATIONS.write().unwrap().clear();
    for _ in map.interactables {
        ANIMATIONS.write().unwrap().push((0, 0));
    }
}
pub fn run_gamestate() {
    GAMESTATE.write().unwrap().run();
}
pub fn mem_btn(id: u8) -> bool {
    let controller: usize = (id/8).min(3).into();
    let id = id % 8;
    let buttons = unsafe {(*GAMEPADS)[controller]};
    (1 << id) & buttons != 0
}
pub fn mem_btnp(id: u8, hold: i8, repeat: i8) -> bool {
    let controller: usize = (id/8).min(3).into();
    let id = id % 8;
    let buttons = unsafe {(*GAMEPADS)[controller]};
    let previous = GAMEPAD_HELPER.read().unwrap()[controller];
    (1 << id) & buttons != (1 << id) & previous && (1 << id) & buttons != 0
}
pub fn step_gamepad_helper() {
    let buttons = unsafe {*GAMEPADS};
    *GAMEPAD_HELPER.write().unwrap() = buttons;
}

#[export_name = "BOOT"]
pub fn boot() {
    load_map(&SUPERMARKET);
}

#[export_name = "TIC"]
pub fn tic() {
    *TIME.write().unwrap() += 1;
    
    if keyp(16, -1, -1) {
        set_pause(!is_paused());
        print!("Paused", 100, 62, PrintOptions {color: 12, ..Default::default()});
    }
    if is_paused() { return }
    if keyp(4, -1, -1) {
        let p = debug_info().player_info;
        debug_info_mut().player_info = !p;
    }
    if keyp(13, -1, -1) {
        let p = debug_info().map_info;
        debug_info_mut().map_info = !p;
    }
    
    run_gamestate();
    step_gamepad_helper();
}

// Copyright (c) 2023 Adam Godwin <evilspamalt/at/gmail.com>
//
// This file is part of Egg Game - https://github.com/Madadog/Egg-Game/
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later
// version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// this program. If not, see <https://www.gnu.org/licenses/>.

pub mod alloc;
mod animation;
mod camera;
mod dialogue;
mod dialogue_data;
mod gamestate;
mod interact;
mod map_data;
mod player;
mod position;
mod rand;
mod tic80;
mod tic_helpers;

use crate::camera::Camera;
use crate::dialogue::Dialogue;
use crate::gamestate::GameState;
use crate::map_data::*;
use crate::player::*;
use crate::position::{Hitbox, Vec2};
use crate::rand::Pcg32;
use crate::tic_helpers::MOUSE_INPUT_DEFAULT;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use tic80::*;

pub struct DebugInfo {
    player_info: bool,
    map_info: bool,
}
impl DebugInfo {
    pub const fn const_default() -> Self {
        DebugInfo {
            player_info: false,
            map_info: false,
        }
    }
}

static TIME: RwLock<i32> = RwLock::new(0);
static PLAYER: RwLock<Player> = RwLock::new(Player::const_default());
static ANIMATIONS: RwLock<Vec<(u16, usize)>> = RwLock::new(Vec::new());
static RNG: RwLock<Lazy<Pcg32>> = RwLock::new(Lazy::new(|| Pcg32::default()));
static PAUSE: AtomicBool = AtomicBool::new(false);
static CAMERA: RwLock<Camera> = RwLock::new(Camera::const_default());
static DEBUG_INFO: RwLock<DebugInfo> = RwLock::new(DebugInfo::const_default());
static CURRENT_MAP: RwLock<&MapSet> = RwLock::new(&SUPERMARKET);
static DIALOGUE: RwLock<Dialogue> = RwLock::new(Dialogue::const_default());
static GAMESTATE: RwLock<GameState> = RwLock::new(GameState::Animation(0));
static GAMEPAD_HELPER: RwLock<[u8; 4]> = RwLock::new([0; 4]);
static MOUSE_HELPER: RwLock<MouseInput> = RwLock::new(MOUSE_INPUT_DEFAULT);
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
pub fn cam_x() -> i32 {
    i32::from(camera().pos.x)
}
pub fn cam_y() -> i32 {
    i32::from(camera().pos.y)
}
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
    *camera_mut() =
        Camera::from_map_size(map1.w as u8, map1.h as u8, map1.sx as i16, map1.sy as i16);
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
    let controller: usize = (id / 8).min(3).into();
    let id = id % 8;
    let buttons = unsafe { (*GAMEPADS)[controller] };
    (1 << id) & buttons != 0
}
pub fn mem_btnp(id: u8, hold: i8, repeat: i8) -> bool {
    let controller: usize = (id / 8).min(3).into();
    let id = id % 8;
    let buttons = unsafe { (*GAMEPADS)[controller] };
    let previous = GAMEPAD_HELPER.read().unwrap()[controller];
    (1 << id) & buttons != (1 << id) & previous && (1 << id) & buttons != 0
}
pub fn any_btnp() -> bool {
    let buttons = unsafe { *GAMEPADS };
    let previous = *GAMEPAD_HELPER.read().unwrap();
    buttons != previous
}
pub fn step_gamepad_helper() {
    let buttons = unsafe { *GAMEPADS };
    *GAMEPAD_HELPER.write().unwrap() = buttons;
}
pub fn step_mouse_helper() {
    let input = mouse();
    *MOUSE_HELPER.write().unwrap() = input;
}
pub fn mouse_delta() -> MouseInput {
    let old = MOUSE_HELPER.read().unwrap().clone();
    let new = mouse();
    MouseInput {
        x: new.x - old.x,
        y: new.y - old.y,
        left: new.left && !old.left,
        middle: new.middle && !old.middle,
        right: new.right && !old.right,
        ..new
    }
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
        print!(
            "Paused",
            100,
            62,
            PrintOptions {
                color: 12,
                ..Default::default()
            }
        );
    }
    if is_paused() {
        return;
    }
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
    step_mouse_helper();
}

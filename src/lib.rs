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
mod map;
mod map_data;
mod player;
mod inventory;
mod position;
mod rand;
mod tic80;
mod tic_helpers;
mod input_manager;
mod walkaround;
mod save;
mod particles;

use crate::gamestate::GameState;
use crate::map_data::*;
use crate::position::{Hitbox, Vec2};
use crate::rand::Pcg32;
use crate::tic_helpers::{SyncHelper};
use once_cell::sync::Lazy;
use walkaround::WalkaroundState;
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

static WALKAROUND_STATE: RwLock<WalkaroundState> = RwLock::new(WalkaroundState::new());
static TIME: RwLock<i32> = RwLock::new(0);
static PAUSE: AtomicBool = AtomicBool::new(false);
static RNG: RwLock<Lazy<Pcg32>> = RwLock::new(Lazy::new(Pcg32::default));
static DEBUG_INFO: RwLock<DebugInfo> = RwLock::new(DebugInfo::const_default());
static GAMESTATE: RwLock<GameState> = RwLock::new(GameState::Animation(0));
static BG_COLOUR: RwLock<u8> = RwLock::new(0);
static SYNC_HELPER: RwLock<SyncHelper> = RwLock::new(SyncHelper::new());

// REMINDER: Heap maxes at 8192 u32.

pub fn frames() -> i32 {
    *TIME.read().unwrap()
}
pub fn debug_info_mut<'a>() -> RwLockWriteGuard<'a, DebugInfo> {
    DEBUG_INFO.write().unwrap()
}
pub fn debug_info<'a>() -> RwLockReadGuard<'a, DebugInfo> {
    DEBUG_INFO.read().unwrap()
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

pub fn run_gamestate() {
    GAMESTATE.write().unwrap().run();
}

#[export_name = "BOOT"]
pub fn boot() {
    std::panic::set_hook(Box::new(|x| {
        trace!(format!("{x}"),12);
    }));
    WALKAROUND_STATE.write().unwrap().load_map(&BEDROOM);
}

#[export_name = "TIC"]
pub fn tic() {
    SYNC_HELPER.write().unwrap().step();
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
    
    input_manager::step_gamepad_helper();
    input_manager::step_mouse_helper();
}

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

mod animation;
mod camera;
mod dialogue;
mod dialogue_data;
mod gamestate;
mod interact;
mod map;
mod map_data;
mod particles;
mod player;
mod portraits;
mod position;
mod rand;
mod save;
mod sound;
mod tic80_core;
mod tic80_helpers;
mod packed;

use crate::gamestate::walkaround::WalkaroundState;
use crate::gamestate::GameState;
use crate::map_data::*;
use crate::position::{Hitbox, Vec2};
use crate::rand::Pcg32;
use crate::tic80_helpers::SyncHelper;
use once_cell::sync::Lazy;
use packed::{PackedI16, PackedU8};
use std::fmt::format;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use tic80_core::*;
use tic80_helpers::input_manager;

pub struct DebugInfo {
    player_info: bool,
    map_info: bool,
    memory_info: bool,
    memory_index: usize,
}
impl DebugInfo {
    pub const fn const_default() -> Self {
        DebugInfo {
            player_info: false,
            map_info: false,
            memory_info: false,
            memory_index: 0,
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
        trace!(format!("{x}"), 2);
    }));
    WALKAROUND_STATE.write().unwrap().load_map(BEDROOM);
    PackedU8::test();
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
    if keyp(14, -1, -1) {
        let p = debug_info().memory_info;
        debug_info_mut().memory_info = !p;
    }

    run_gamestate();
    if key(6) {
        run_gamestate();
        print_raw(
            "Fast-Forward\0",
            100,
            62,
            PrintOptions {
                color: 12,
                ..Default::default()
            },
        );
    }

    if debug_info().memory_info {
        for i in 0i32..((163840 / 2).min(240 * 136)) {
            let j = (i as usize + debug_info().memory_index).min(163839) as i32;
            let x = unsafe { *((0x18000 + j) as *mut u8) };
            let (l, u) = (x % 16, x >> 4);
            pix((i * 2) % 240, i / 240, l);
            pix((i * 2 + 1) % 240, i / 240, u);
        }
        let acc = MEM_USAGE.load(Ordering::SeqCst);
        print_raw(
            &format!(
                "{acc}/160kB used (heap). [n] to close.\nDisplaying address offset = {}\0",
                debug_info().memory_index
            ),
            1,
            1,
            PrintOptions::default().with_color(12),
        );
        if input_manager::mem_btn(0) {
            let x = (debug_info().memory_index + 240 * 8).min(163840);
            debug_info_mut().memory_index = x;
        }
        if input_manager::mem_btn(1) {
            let x = debug_info().memory_index.saturating_sub(240 * 8);
            debug_info_mut().memory_index = x;
        }
        if debug_info().memory_index == 163840 {
            print_raw("End.\0", 1, 120, PrintOptions::default().with_color(12));
        }
    }
    input_manager::step_gamepad_helper();
    input_manager::step_mouse_helper();
}

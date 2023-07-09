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

use self::inventory::{InventoryUiState, InventoryUi};
use self::walkaround::WalkaroundState;
use crate::data::save;
use crate::debug::DebugInfo;
use crate::dialogue::{print_width, DIALOGUE_OPTIONS};
use crate::rand::Lcg64Xsh32;
use tic80_api::core::*;

use self::menu::MenuState;
use tic80_api::helpers::input_manager::{any_btnp, mem_btn};
use tic80_api::helpers::*;

mod intro;
pub mod inventory;
mod menu;
pub mod walkaround;

#[derive(Clone, Debug)]
pub struct EggInput {
    pub gamepads: [u8; 4],
    pub previous_gamepads: [u8; 4],
    pub keyboard: [bool; 65],
    pub previous_keyboard: [bool; 65],
    pub mouse: MouseInput,
    pub previous_mouse: MouseInput,
}
impl EggInput {
    pub fn new() -> Self {
        Self {
            gamepads: [0; 4],
            previous_gamepads: [0; 4],
            keyboard: [false; 65],
            previous_keyboard: [false; 65],
            mouse: MouseInput::default(),
            previous_mouse: MouseInput::default(),
        }
    }
    pub fn mem_btn(&self, id: u8) -> bool {
        let controller: usize = (id / 8).min(3).into();
        let id = id % 8;
        let buttons = self.gamepads[controller];
        (1 << id) & buttons != 0
    }
    pub fn mem_btnp(&self, id: u8) -> bool {
        let controller: usize = (id / 8).min(3).into();
        let id = id % 8;
        let buttons = self.gamepads[controller];
        let previous = self.previous_gamepads[controller];
        (1 << id) & buttons != (1 << id) & previous && (1 << id) & buttons != 0
    }
    pub fn any_btnp(&self) -> bool {
        let mut flag = false;
        for (b0, b1) in self.previous_gamepads.iter().zip(self.gamepads.iter()) {
            flag |= b0.count_ones() < b1.count_ones();
        }
        flag
    }
    pub fn any_btnpr(&self) -> bool {
        self.previous_gamepads != self.gamepads
    }
    pub fn keyp(&self, index: usize, _: i32, _: i32) -> bool {
        self.keyboard[index] && !self.previous_keyboard[index]
    }
    pub fn key(&self, index: usize) -> bool {
        self.keyboard[index]
    }
    pub fn mouse(&self) -> MouseInput {
        self.mouse.clone()
    }
    pub fn mouse_delta(&self) -> MouseInput {
        let new = self.mouse.clone();
        let old = self.previous_mouse.clone();
        MouseInput {
            x: new.x - old.x,
            y: new.y - old.y,
            left: new.left && !old.left,
            middle: new.middle && !old.middle,
            right: new.right && !old.right,
            ..new
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct EggMemory {
    pub memory: [u8; 1024],
}
impl EggMemory {
    pub fn new() -> Self {
        Self {
            memory: [0; 1024],
        }
    }
    pub fn from_array(array: [u8; 1024]) -> Self {
        Self { memory: array }
    }
    pub fn is(&self, bit: save::PmemBit) -> bool {
        bit.is_true_with(&self.memory)
    }
    pub fn set(&mut self, bit: save::PmemBit) {
        bit.set_true_with(&mut self.memory);
    }
    pub fn clear(&mut self, bit: save::PmemBit) {
        bit.set_false_with(&mut self.memory);
    }
    pub fn get_byte(&self, byte: save::PmemU8) -> u8 {
        self.memory[byte.index()]
    }
    pub fn set_byte(&mut self, byte: save::PmemU8, value: u8) {
        self.memory[byte.index()] = value;
    }
    
}

#[derive(Debug)]
pub enum GameState {
    Instructions(u16),
    Walkaround,
    Animation(u16),
    MainMenu(MenuState),
    Inventory,
}
impl GameState {
    pub fn run(
        &mut self,
        walkaround_state: &mut WalkaroundState,
        sync_helper: &mut SyncHelper,
        debug_info: &mut DebugInfo,
        map_flags: &[u8],
        elapsed_frames: i32,
        inventory_ui: &mut InventoryUi,
        rng: &mut Lcg64Xsh32,
        input: &EggInput,
        memory: &mut EggMemory,
    ) {
        println!("Game state: {self:?}");
        match self {
            Self::Instructions(i) => {
                *i += 1;
                if (*i > 60 || memory.is(save::INSTRUCTIONS_READ)) && input.any_btnp() {
                    memory.set(save::INSTRUCTIONS_READ);
                    walkaround_state.load_pmem(sync_helper, memory);
                    *self = Self::Walkaround;
                }
                draw_instructions();
            }
            Self::Walkaround => {
                let next = walkaround_state.step((sync_helper, map_flags, inventory_ui, memory, input));
                walkaround_state.draw(debug_info);
                if let Some(state) = next {
                    *self = state;
                }
            }
            Self::Animation(x) => {
                println!("Intro frame {x}");
                if memory.is(save::INTRO_ANIM_SEEN) {
                    *self = Self::MainMenu(MenuState::new());
                    return;
                };
                if input.mem_btn(5) {
                    *x += 1000;
                }
                println!("Drawing frame...");
                // if intro::draw_animation(*x, rng) {
                if false {
                    println!("Drew frame...");
                    *x += 1;
                } else {
                    println!("Animation done...");
                    *self = Self::MainMenu(MenuState::new());
                }
            }
            Self::MainMenu(state) => {
                match state.step_main_menu(walkaround_state, inventory_ui, memory, input) {
                    Some(x) => *self = x,
                    None => state.draw_main_menu(elapsed_frames),
                };
            }
            Self::Inventory => {
                inventory_ui.step();
                match inventory_ui.state {
                    InventoryUiState::Close => *self = Self::Walkaround,
                    InventoryUiState::Options => {
                        *self = Self::MainMenu(MenuState::inventory_options())
                    }
                    _ => inventory_ui.draw(),
                }
            }
        }
    }
}

pub trait Game<T, U> {
    fn step(&mut self, state: T) -> Option<GameState> {
        None
    }
    fn draw(&self, state: U);
}

pub fn draw_instructions() {
    cls(0);
    use crate::data::dialogue_data::{INSTRUCTIONS, INSTRUCTIONS_TITLE};
    let small_text = DIALOGUE_OPTIONS.small_text();
    rect_outline(6, 15, 228, 100, 0, 1);
    rect(8, 17, 224, 96, 1);
    print_raw_shadow(
        &format!("{}\0", INSTRUCTIONS_TITLE),
        11,
        20,
        PrintOptions {
            color: 12,
            small_text,
            ..Default::default()
        },
        0,
    );
    print_raw_shadow(
        INSTRUCTIONS,
        11,
        36,
        PrintOptions {
            color: 12,
            small_text,
            ..Default::default()
        },
        0,
    );
    let origin = 11.0;
    let width = (print_width(INSTRUCTIONS_TITLE, false, small_text) - 1) as f32;
    line(origin, 27.0, origin + width, 27.0, 12);
    line(origin + 1.0, 28.0, origin + width + 1.0, 28.0, 0);
}

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

use std::process;

use crate::dialogue::{print_width, DIALOGUE_OPTIONS};
use self::inventory::{InventoryUiState, INVENTORY};
use crate::data::save;
use crate::{tic80_core::*, WALKAROUND_STATE};

use self::menu::MenuState;
use crate::input_manager::{any_btnp, mem_btn};
use crate::tic80_helpers::*;

mod intro;
mod menu;
mod inventory;
pub mod walkaround;

#[derive(Debug)]
pub enum GameState {
    Instructions(u16),
    Walkaround,
    Animation(u16),
    MainMenu(MenuState),
    Inventory,
}
impl GameState {
    pub fn run(&mut self) {
        match self {
            Self::Instructions(i) => {
                *i += 1;
                if (*i > 60 || save::INSTRUCTIONS_READ.is_true()) && any_btnp() {
                    save::INSTRUCTIONS_READ.set_true();
                    if let Ok(mut walkaround) = WALKAROUND_STATE.write() {
                        walkaround.load_pmem();
                    }
                    *self = Self::Walkaround;
                }
                draw_instructions();
            }
            Self::Walkaround => {
                let next = WALKAROUND_STATE.write().unwrap_or_else(|_| process::abort()).step();
                WALKAROUND_STATE.read().unwrap_or_else(|_| process::abort()).draw();
                if let Some(state) = next {
                    *self = state;
                }
            }
            Self::Animation(x) => {
                if save::INTRO_ANIM_SEEN.is_true() {
                    *self = Self::MainMenu(MenuState::new());
                    return;
                };
                if mem_btn(5) {
                    *x += 1000;
                }
                if intro::draw_animation(*x) {
                    *x += 1;
                } else {
                    *self = Self::MainMenu(MenuState::new());
                }
            }
            Self::MainMenu(state) => {
                match state.step_main_menu() {
                    Some(x) => *self = x,
                    None => state.draw_main_menu(),
                };
            }
            Self::Inventory => {
                INVENTORY.write().unwrap_or_else(|_| process::abort()).step();
                match INVENTORY.read().unwrap_or_else(|_| process::abort()).state {
                    InventoryUiState::Close => {*self = Self::Walkaround},
                    InventoryUiState::Options => {*self = Self::MainMenu(MenuState::inventory_options())},
                    _ => {INVENTORY.read().unwrap_or_else(|_| process::abort()).draw()}
                }
            }
        }
    }
}

pub trait Game {
    fn step(&mut self) -> Option<GameState> {
        None
    }
    fn draw(&self);
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

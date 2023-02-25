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

use crate::dialogue::DIALOGUE_OPTIONS;
use crate::inventory::{InventoryUiState, INVENTORY};
use crate::save;
use crate::{tic80_core::*, WALKAROUND_STATE};

use self::menu::{MainMenuOption, MenuState};
use crate::input_manager::{any_btnp, mem_btn};
use crate::tic80_helpers::*;

mod intro;
mod menu;
pub mod walkaround;

pub enum GameState {
    Instructions(u16),
    Walkaround,
    Animation(u16),
    MainMenu(MenuState),
    Options(MenuState),
    Inventory,
}
impl GameState {
    pub fn run(&mut self) {
        match self {
            Self::Instructions(i) => {
                *i += 1;
                if (*i > 60 || save::INSTRUCTIONS_READ.is_true()) && any_btnp() {
                    save::INSTRUCTIONS_READ.set_true();
                    *self = Self::Walkaround;
                }
                draw_instructions();
            }
            Self::Walkaround => {
                let next = WALKAROUND_STATE.write().unwrap().step();
                WALKAROUND_STATE.read().unwrap().draw();
                if let Some(state) = next {
                    *self = state;
                }
            }
            Self::Animation(x) => {
                if save::INTRO_ANIM_SEEN.is_true() {
                    *self = Self::MainMenu(MenuState::new());
                    return;
                };
                if mem_btn(4) {
                    *x += 1;
                }
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
                    Some(MainMenuOption::Play) => *self = Self::Instructions(0),
                    Some(MainMenuOption::Options) => *self = Self::Options(MenuState::new()),
                    None => state.draw_main_menu(),
                };
            }
            Self::Options(state) => {
                if state.step_options() {
                    state.draw_options();
                } else {
                    *self = Self::MainMenu(MenuState::new());
                }
            }
            Self::Inventory => {
                INVENTORY.write().unwrap().step();
                if matches!(INVENTORY.read().unwrap().state, InventoryUiState::Close) {
                    *self = Self::Walkaround;
                } else {
                    INVENTORY.read().unwrap().draw();
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
    let string = crate::dialogue_data::INSTRUCTIONS;
    let small_text = DIALOGUE_OPTIONS.small_text();
    rect_outline(6, 15, 228, 100, 0, 1);
    rect(8, 17, 224, 96, 1);
    print_raw(
        string,
        12,
        21,
        PrintOptions {
            color: 0,
            small_text,
            ..Default::default()
        },
    );
    print_raw(
        string,
        11,
        20,
        PrintOptions {
            color: 12,
            small_text,
            ..Default::default()
        },
    );
    let origin = 11.0;
    let width = 66.0;
    line(origin, 27.0, origin + width, 27.0, 12);
    line(origin + 1.0, 28.0, origin + width + 1.0, 28.0, 0);
}

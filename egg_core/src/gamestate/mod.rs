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

use crate::system::{pressed, Controller, MouseInput, PrintOptions, ScanCode, SCANCODE_COUNT};
use log::trace;

use self::inventory::{InventoryUi, InventoryUiState};
use self::walkaround::WalkaroundState;
use crate::debug::DebugInfo;
use crate::dialogue::DIALOGUE_OPTIONS;
use crate::system::{ConsoleApi, ConsoleHelper};

use self::menu::MenuState;

mod debug;
mod intro;
pub mod inventory;
mod menu;
pub mod walkaround;

#[derive(Clone, Debug)]
pub struct EggInput {
    pub controllers: [Controller; 4],
    pub keyboard: [bool; SCANCODE_COUNT],
    pub previous_keyboard: [bool; SCANCODE_COUNT],
    pub mouse: MouseInput,
    pub typed_chars: Vec<char>,
}
impl Default for EggInput {
    fn default() -> Self {
        Self::new()
    }
}

impl EggInput {
    pub fn new() -> Self {
        Self {
            controllers: [Controller::default(); 4],
            keyboard: [false; SCANCODE_COUNT],
            previous_keyboard: [false; SCANCODE_COUNT],
            mouse: MouseInput::default(),
            typed_chars: Vec::with_capacity(8),
        }
    }
    pub fn press_key(&mut self, key: ScanCode) {
        self.keyboard[key.index()] = true;
    }
    pub fn push_char(&mut self, c: char) {
        self.typed_chars.push(c);
    }
    pub fn refresh(&mut self) {
        self.previous_keyboard = self.keyboard;
        self.mouse.step();
        for controller in &mut self.controllers {
            controller.step();
        }
        self.keyboard = [false; SCANCODE_COUNT];
        self.typed_chars.clear();
    }
    pub fn key_chars(&self) -> &[char] {
        &self.typed_chars
    }
    pub fn keyp(&self, key: ScanCode) -> bool {
        let i = key.index();
        self.keyboard[i] && !self.previous_keyboard[i]
    }
    pub fn key(&self, key: ScanCode) -> bool {
        self.keyboard[key.index()]
    }
}

#[derive(Debug)]
pub enum GameMode {
    Instructions(u16),
    Walkaround,
    Animation(u16),
    MainMenu(MenuState),
    Inventory,
    SpriteTest(u32),
}
impl GameMode {
    pub fn run(
        &mut self,
        walkaround_state: &mut WalkaroundState,
        debug_info: &mut DebugInfo,
        elapsed_frames: i32,
        inventory_ui: &mut InventoryUi,
        draw_state: &mut crate::drawstate::DrawState,
        system: &mut impl ConsoleApi,
    ) {
        trace!("Game state: {self:?}");
        match self {
            Self::Instructions(i) => {
                *i += 1;
                if (*i > 60 || system.memory().instructions_read) && system.any_btnp() {
                    if system.memory().instructions_read {
                        walkaround_state.load_pmem(system);
                    } else {
                        walkaround_state.new_game(system);
                    }
                    system.memory().instructions_read = true;
                    *self = Self::Walkaround;
                }
                draw_instructions(draw_state, system);
            }
            Self::Walkaround => {
                let next = walkaround_state.step((draw_state, system, inventory_ui));
                walkaround_state.draw((draw_state, system, debug_info));
                if let Some(state) = next {
                    *self = state;
                }
            }
            Self::Animation(x) => {
                if system.memory().intro_anim_seen {
                    *self = Self::MainMenu(MenuState::new());
                    return;
                };
                // Press X to skip cutscene
                if pressed(system.controller().b) {
                    *x += 1000;
                }
                if intro::draw_animation(*x, draw_state, system) {
                    *x += 1;
                } else {
                    *self = Self::MainMenu(MenuState::new());
                }
            }
            Self::MainMenu(state) => {
                let next = state.step_main_menu(draw_state, system, walkaround_state, inventory_ui);
                state.draw_main_menu(draw_state, system, elapsed_frames);
                match next {
                    Some(x) => *self = x,
                    None => (),
                };
            }
            Self::Inventory => {
                inventory_ui.step(system);
                match inventory_ui.state {
                    InventoryUiState::Close => *self = Self::Walkaround,
                    InventoryUiState::Options => {
                        *self = Self::MainMenu(MenuState::inventory_options())
                    }
                    _ => inventory_ui.draw(draw_state, system),
                }
            }
            Self::SpriteTest(x) => {
                debug::step_sprite_test(system, x);
                debug::draw_sprite_test(draw_state, system, *x);
            }
        }
    }
}

pub trait Game<T, U> {
    fn step(&mut self, _state: T) -> Option<GameMode> {
        None
    }
    fn draw(&self, state: U);
}

pub fn draw_instructions(
    draw_state: &mut crate::drawstate::DrawState,
    system: &mut impl ConsoleApi,
) {
    use crate::data::dialogue_data::{INSTRUCTIONS, INSTRUCTIONS_TITLE};
    use crate::drawstate::LayerId;
    use crate::system::drawing::{Canvas, EdgePolicy, Transform};
    use crate::system::image::RgbaImage;
    let small_text = DIALOGUE_OPTIONS.small_text(system);
    let title = format!("{INSTRUCTIONS_TITLE}\0");
    let colour_12 = draw_state.colour(12);
    let colour_1 = draw_state.colour(1);
    let colour_0 = draw_state.colour(0);
    let opts = PrintOptions {
        color: 12,
        small_text,
        ..Default::default()
    };
    {
        let canvas = draw_state.rgba(LayerId::BG);
        canvas.fill(colour_0);
        canvas.outlined_rect(6, 15, 228, 100, colour_0, colour_1);
        canvas.fill_rect(8, 17, 224, 96, colour_1);
        system.print_to_shadow(canvas, &title, 11, 20, colour_12, colour_0, opts.clone());
        system.print_to_shadow(canvas, INSTRUCTIONS, 11, 36, colour_12, colour_0, opts.clone());
        let width = system.print_to(canvas, INSTRUCTIONS_TITLE, 999, 999, colour_12, opts) - 1;
        let origin = 11;
        canvas.line(origin, 27, origin + width, 27, colour_12);
        canvas.line(origin + 1, 28, origin + width + 1, 28, colour_0);
    }
    let output = system.output_image();
    output.blit::<RgbaImage>(
        0,
        0,
        &draw_state.rgba_canvas[LayerId::BG as usize],
        EdgePolicy::Transparent,
        Transform::IDENTITY,
        |p| p.a() == 0,
    );
}

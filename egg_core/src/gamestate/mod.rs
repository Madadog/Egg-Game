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
use crate::Ctx;
use crate::debug::DebugInfo;
use crate::system::{ConsoleApi, ConsoleHelper};

use self::menu::MenuState;

mod debug;
mod intro;
pub mod inventory;
// Public so a host can give an extra walkaround window its own `MapViewer`
// (the in-game map editor) instance — see the frontend's multi-window views.
pub mod mapeditor;
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
        ctx: &mut Ctx<impl ConsoleApi>,
        walkaround_state: &mut WalkaroundState,
        inventory_ui: &mut InventoryUi,
        debug_info: &mut DebugInfo,
        elapsed_frames: i32,
    ) {
        trace!("Game state: {self:?}");
        match self {
            Self::Instructions(i) => {
                *i += 1;
                if (*i > 60 || ctx.system.memory().instructions_read) && ctx.system.any_btnp() {
                    if ctx.system.memory().instructions_read {
                        walkaround_state.load_pmem(ctx.system, &ctx.draw.indexed_sprites, ctx.maps);
                    } else {
                        walkaround_state.new_game(ctx.system, &ctx.draw.indexed_sprites, ctx.maps);
                    }
                    ctx.system.memory().instructions_read = true;
                    *self = Self::Walkaround;
                }
                draw_instructions(ctx);
            }
            Self::Walkaround => {
                let next = walkaround_state.step(ctx, inventory_ui);
                walkaround_state.draw(ctx, debug_info);
                if let Some(state) = next {
                    *self = state;
                }
            }
            Self::Animation(x) => {
                if ctx.system.memory().intro_anim_seen {
                    *self = Self::MainMenu(MenuState::new());
                    return;
                };
                // Press X to skip cutscene
                if pressed(ctx.system.controller().b) {
                    *x += 1000;
                }
                if intro::draw_animation(*x, ctx) {
                    *x += 1;
                } else {
                    *self = Self::MainMenu(MenuState::new());
                }
            }
            Self::MainMenu(state) => {
                let next = state.step_main_menu(ctx, walkaround_state, inventory_ui);
                state.draw_main_menu(ctx, elapsed_frames);
                if let Some(x) = next {
                    *self = x;
                }
            }
            Self::Inventory => {
                inventory_ui.step(ctx);
                match inventory_ui.state {
                    InventoryUiState::Close => *self = Self::Walkaround,
                    InventoryUiState::Options => {
                        *self = Self::MainMenu(MenuState::inventory_options())
                    }
                    _ => inventory_ui.draw(ctx),
                }
            }
            Self::SpriteTest(x) => {
                debug::step_sprite_test(ctx, x);
                debug::draw_sprite_test(ctx, *x);
            }
        }
    }
}

pub fn draw_instructions(ctx: &mut Ctx<impl ConsoleApi>) {
    use crate::drawstate::LayerId;
    use crate::system::drawing::{Canvas, EdgePolicy, Transform};
    use crate::system::drawing::image::RgbaImage;
    let small_text = ctx.system.memory().small_text_on;
    let title = ctx.system.label("instructions_title");
    let instructions = ctx.system.label("instructions");
    let colour_12 = ctx.draw.colour(12);
    let colour_1 = ctx.draw.colour(1);
    let colour_0 = ctx.draw.colour(0);
    let opts = PrintOptions {
        color: 12,
        small_text,
        ..Default::default()
    };
    {
        let canvas = ctx.draw.rgba(LayerId::BG);
        canvas.fill(colour_0);
        canvas.outlined_rect(6, 15, 228, 100, colour_0, colour_1);
        canvas.fill_rect(8, 17, 224, 96, colour_1);
        ctx.system.print_to_shadow(canvas, &title, 11, 20, colour_12, colour_0, opts.clone());
        ctx.system.print_to_shadow(canvas, &instructions, 11, 36, colour_12, colour_0, opts.clone());
        let width = ctx.system.print_to(canvas, &title, 999, 999, colour_12, opts) - 1;
        let origin = 11;
        canvas.line(origin, 27, origin + width, 27, colour_12);
        canvas.line(origin + 1, 28, origin + width + 1, 28, colour_0);
    }
    let output = ctx.system.output_image();
    output.blit::<RgbaImage>(
        0,
        0,
        &ctx.draw.rgba_canvas[LayerId::BG as usize],
        EdgePolicy::Transparent,
        Transform::IDENTITY,
        |p| p.a() == 0,
    );
}

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

use crate::system::{Controller, MouseInput, PrintOptions, SCANCODE_COUNT, ScanCode, pressed};
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
/// The reusable line-editing buffer ([`text_field::TextField`]) shared by the map
/// editor's property fields and the multi-line [`texteditor`].
mod text_field;
/// A full-window raw text editor for the `.eggtext`/`.eggscene` script files,
/// hosted per extra view — see the frontend's multi-window views.
pub mod texteditor;
pub mod walkaround;

#[derive(Clone, Debug)]
pub struct EggInput {
    pub controllers: [Controller; 4],
    pub keyboard: [bool; SCANCODE_COUNT],
    pub previous_keyboard: [bool; SCANCODE_COUNT],
    /// Consecutive fixed steps each scancode has been held (0 while up), advanced
    /// in [`refresh`](Self::refresh) — drives [`key_repeat`](Self::key_repeat).
    pub held: [u16; SCANCODE_COUNT],
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
            held: [0; SCANCODE_COUNT],
            mouse: MouseInput::default(),
            typed_chars: Vec::with_capacity(8),
        }
    }
    pub fn press_key(&mut self, key: ScanCode) {
        if let Some(down) = self.keyboard.get_mut(key.index()) {
            *down = true;
        }
    }
    pub fn push_char(&mut self, c: char) {
        self.typed_chars.push(c);
    }
    pub fn refresh(&mut self) {
        // Advance the per-key hold counters from the frame that just ended — the
        // `keyboard` array still holds it here, before the clear below.
        for (held, &down) in self.held.iter_mut().zip(&self.keyboard) {
            *held = if down { held.saturating_add(1) } else { 0 };
        }
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
        let down = self.keyboard.get(i).copied().unwrap_or(false);
        let prev = self.previous_keyboard.get(i).copied().unwrap_or(false);
        down && !prev
    }
    pub fn key(&self, key: ScanCode) -> bool {
        self.keyboard.get(key.index()).copied().unwrap_or(false)
    }
    /// Edge-or-repeat: true on the initial press, then — while still held — again
    /// every `rate` fixed steps after an initial `delay` (both in fixed steps).
    /// `delay`/`rate` are per-call so different consumers can tune their cadence.
    pub fn key_repeat(&self, key: ScanCode, delay: u16, rate: u16) -> bool {
        let i = key.index();
        let down = self.keyboard.get(i).copied().unwrap_or(false);
        let held = self.held.get(i).copied().unwrap_or(0);
        if !down {
            return false;
        }
        if held == 0 {
            return true;
        }
        held >= delay && (held - delay).is_multiple_of(rate.max(1))
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;

    /// `key_repeat` fires on the press frame, then — once held past `delay` — every
    /// `rate` fixed steps, and never while the key is up. One frame = `refresh()`
    /// (advances the hold counter from last frame, clears `keyboard`) then a press.
    #[test]
    fn key_repeat_fires_on_press_then_after_delay_at_rate() {
        let mut input = EggInput::new();
        let k = ScanCode::Backspace;
        let (delay, rate) = (3u16, 2u16);

        let mut fired = Vec::new();
        for frame in 0..10 {
            input.refresh();
            input.press_key(k);
            if input.key_repeat(k, delay, rate) {
                fired.push(frame);
            }
        }
        // Initial press at 0, then held reaches `delay` (3) and repeats every `rate`.
        assert_eq!(fired, vec![0, 3, 5, 7, 9]);

        // A held key that's no longer pressed this frame never repeats…
        input.refresh();
        assert!(!input.key_repeat(k, delay, rate));
        // …and after release the counter resets, so a fresh press fires again.
        input.refresh();
        input.press_key(k);
        assert!(input.key_repeat(k, delay, rate));
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
                if (*i > 60 || ctx.save.instructions_read) && ctx.system.any_btnp() {
                    if ctx.save.instructions_read {
                        walkaround_state.load_pmem(ctx);
                    } else {
                        walkaround_state.new_game(ctx);
                    }
                    ctx.save.instructions_read = true;
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
                if ctx.save.intro_anim_seen {
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
    use crate::system::drawing::image::RgbaImage;
    use crate::system::drawing::{Canvas, EdgePolicy, Transform};
    let small_text = ctx.save.small_text_on;
    let title = ctx.label("instructions_title");
    let instructions = ctx.label("instructions");
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
        // The box spans the framebuffer width with fixed 6/8px margins (228 = 240-12
        // at the base width). `d` vertically centres the 136-tall design (0 at the
        // base height), so the box rides the middle of a taller window.
        let cw = canvas.width() as i32;
        let d = (canvas.height() as i32 - crate::system::HEIGHT) / 2;
        canvas.outlined_rect(6, 15 + d, cw - 12, 100, colour_0, colour_1);
        canvas.fill_rect(8, 17 + d, cw - 16, 96, colour_1);
        ctx.system.print_to_shadow(
            canvas,
            &title,
            11,
            20 + d,
            colour_12,
            colour_0,
            opts.clone(),
        );
        ctx.system.print_to_shadow(
            canvas,
            &instructions,
            11,
            36 + d,
            colour_12,
            colour_0,
            opts.clone(),
        );
        let width = ctx.system.text_width(&title, opts) - 1;
        let origin = 11;
        canvas.line(origin, 27 + d, origin + width, 27 + d, colour_12);
        canvas.line(origin + 1, 28 + d, origin + width + 1, 28 + d, colour_0);
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

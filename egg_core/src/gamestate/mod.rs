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

use crate::system::{Controller, MouseInput, PrintOptions, SCANCODE_COUNT, ScanCode};

use self::walkaround::WalkaroundState;
use crate::Ctx;
use crate::system::{ConsoleApi, ConsoleHelper};

pub use self::debug::SpriteTest;
pub use self::intro::IntroAnimation;
pub use self::menu::MenuState;

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
    /// Index a per-scancode array by `key`, yielding the type's default (`false` /
    /// `0`) for an out-of-range scancode. `ScanCode::index()` is always in range,
    /// so this just keeps every lookup panic-free behind one helper.
    fn at<T: Copy + Default>(array: &[T], key: ScanCode) -> T {
        array.get(key.index()).copied().unwrap_or_default()
    }
    /// Whether `key` is down this frame.
    pub fn key(&self, key: ScanCode) -> bool {
        Self::at(&self.keyboard, key)
    }
    /// Whether `key` was down on the previous frame.
    fn was_down(&self, key: ScanCode) -> bool {
        Self::at(&self.previous_keyboard, key)
    }
    /// Fixed steps `key` has been held (0 while up).
    fn held_steps(&self, key: ScanCode) -> u16 {
        Self::at(&self.held, key)
    }
    /// True only on the frame `key` goes down (down now, up last frame).
    pub fn keyp(&self, key: ScanCode) -> bool {
        self.key(key) && !self.was_down(key)
    }
    /// Edge-or-repeat: true on the initial press, then — while still held — again
    /// every `rate` fixed steps after an initial `delay` (both in fixed steps).
    /// `delay`/`rate` are per-call so different consumers can tune their cadence.
    pub fn key_repeat(&self, key: ScanCode, delay: u16, rate: u16) -> bool {
        if !self.key(key) {
            return false;
        }
        let held = self.held_steps(key);
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

/// The current game mode — a pure tag. Each mode's state lives in its own field
/// on [`EggState`](crate::EggState) (e.g. [`IntroAnimation`], [`Instructions`],
/// [`MenuState`], [`SpriteTest`], plus the already-external walkaround/inventory);
/// dispatch and on-entry setup are [`EggState::step_mode`](crate::EggState) and
/// [`EggState::enter`](crate::EggState). The four `…Menu`/`…Options` variants all
/// drive the shared [`MenuState`], differing only in which menu `enter` builds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GameMode {
    Instructions,
    Walkaround,
    Animation,
    Inventory,
    SpriteTest,

    // menus
    MainMenu,
    InventoryOptions,
    DebugMenu,
    MapSelect,
}

/// The startup instructions screen: a brief timer gates the "press any button to
/// start" prompt, then it loads the world and hands off to
/// [`GameMode::Walkaround`].
#[derive(Debug, Default)]
pub struct Instructions {
    timer: u16,
}
impl Instructions {
    pub fn step(
        &mut self,
        ctx: &mut Ctx<impl ConsoleApi>,
        walkaround: &mut WalkaroundState,
    ) -> Option<GameMode> {
        self.timer += 1;
        let mut next = None;
        if (self.timer > 60 || ctx.save.instructions_read) && ctx.system.any_btnp() {
            if ctx.save.instructions_read {
                walkaround.load_pmem(ctx);
            } else {
                walkaround.new_game(ctx);
            }
            ctx.save.instructions_read = true;
            next = Some(GameMode::Walkaround);
        }
        draw_instructions(ctx);
        next
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

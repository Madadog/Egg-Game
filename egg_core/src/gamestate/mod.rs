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

use crate::render::PrintOptions;

use self::walkaround::WalkaroundState;
use crate::Ctx;
use crate::platform::{ConsoleApi, ConsoleHelper};

pub use self::intro::IntroAnimation;
pub use self::menu::MenuState;
pub use self::sprite_test::SpriteTest;

mod intro;
mod menu;
mod sprite_test;
pub mod walkaround;

/// The current game mode — a pure tag. Each mode's state lives in its own field
/// on [`EggState`](crate::EggState) (e.g. [`IntroAnimation`], [`Instructions`],
/// [`MenuState`], [`SpriteTest`], plus the external walkaround — which owns the
/// inventory as an overlay rather than it being its own mode);
/// dispatch and on-entry setup are [`EggState::step_mode`](crate::EggState) and
/// [`EggState::enter`](crate::EggState). The four `…Menu`/`…Options` variants all
/// drive the shared [`MenuState`], differing only in which menu `enter` builds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GameMode {
    Instructions,
    Walkaround,
    Animation,
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
    use crate::draw_state::LayerId;
    use crate::render::image::RgbaImage;
    use crate::render::{Canvas, EdgePolicy, Transform};
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
        let d = (canvas.height() as i32 - crate::platform::HEIGHT) / 2;
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

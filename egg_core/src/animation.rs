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

use serde::{Deserialize, Serialize};

use crate::position::Vec2;
use crate::system::SpriteOptions;

/// One frame of an object's animated sprite. Serde-serialisable so an object's
/// full sprite (multi-frame, per-frame offsets/durations, palette rotation,
/// outline, multi-tile [`SpriteOptions`]) can round-trip through a map file's
/// `anim` object property — some maps carry sprites richer than a single static
/// tile id, and the `.tmj` codec must preserve every one. The
/// `#[serde(default)]`s let a partial frame (just a `spr_id`, say) still parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimFrame {
    #[serde(default = "Vec2::zero")]
    pub pos: Vec2,
    pub spr_id: u16,
    #[serde(default = "one_u16")]
    pub duration: u16,
    #[serde(default)]
    pub options: SpriteOptions,
    #[serde(default = "default_outline")]
    pub outline_colour: Option<u8>,
    #[serde(default)]
    pub palette_rotate: u8,
}

/// Serde default for [`AnimFrame::duration`]: one tick, never zero.
fn one_u16() -> u16 {
    1
}

/// Serde default for [`AnimFrame::outline_colour`]: the `Some(1)` the
/// [`AnimFrame::new`] constructor picks (so a frame authored without an explicit
/// outline keeps the historical outlined look).
fn default_outline() -> Option<u8> {
    Some(1)
}
impl AnimFrame {
    pub const fn new(pos: Vec2, spr_id: u16, duration: u16, options: SpriteOptions) -> Self {
        Self {
            pos,
            spr_id,
            duration,
            options,
            outline_colour: Some(1),
            palette_rotate: 0,
        }
    }
    pub const fn with_outline(self, outline: Option<u8>) -> Self {
        Self {
            outline_colour: outline,
            ..self
        }
    }
    pub const fn default() -> Self {
        Self {
            pos: Vec2::new(0, 0),
            spr_id: 0,
            duration: 1,
            options: SpriteOptions::transparent_zero(),
            outline_colour: Some(1),
            palette_rotate: 0,
        }
    }

    pub const fn with_palette_rotate(self, palette_rotate: u8) -> Self {
        Self {
            palette_rotate,
            ..self
        }
    }
}

#[derive(Debug, Clone)]
pub struct Animation {
    /// Timer used to switch frames
    pub tick: u16,
    /// Current frame being displayed
    pub index: usize,
    pub frames: Vec<AnimFrame>,
}
impl Animation {
    pub fn new(frames: &[AnimFrame]) -> Self {
        Self {
            frames: frames.into(),
            ..Self::default()
        }
    }
    pub const fn default() -> Self {
        Self {
            tick: 0,
            index: 0,
            frames: Vec::new(),
        }
    }
    pub fn current_frame(&self) -> &AnimFrame {
        self.frames
            .get(self.index)
            .expect("Couldn't find animation frame!")
    }
    pub fn advance(&mut self) {
        if self.tick >= self.current_frame().duration {
            self.index = (self.index + 1) % self.frames.len();
            self.tick = 0;
        } else {
            self.tick += 1;
        }
    }
}

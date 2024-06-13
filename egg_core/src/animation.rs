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

use tic80_api::core::{SpriteOptions, StaticSpriteOptions};
use crate::position::Vec2;

// TODO: Allocate game dialogue so it can be loaded from files

#[derive(Debug, Clone)]
pub struct StaticAnimFrame<'a> {
    pub pos: Vec2,
    pub spr_id: u16,
    pub duration: u16,
    pub options: StaticSpriteOptions<'a>,
    pub outline_colour: Option<u8>,
    pub palette_rotate: u8,
}
impl<'a> StaticAnimFrame<'a> {
    pub const fn new(pos: Vec2, spr_id: u16, duration: u16, options: StaticSpriteOptions<'a>) -> Self {
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
        Self { outline_colour: outline, ..self }
    }
    pub const fn default() -> Self {
        Self {
            pos: Vec2::new(0, 0),
            spr_id: 0,
            duration: 1,
            options: StaticSpriteOptions::transparent_zero(),
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
pub struct StaticAnimation<'a> {
    pub tick: u16,
    pub index: usize,
    pub frames: &'a [StaticAnimFrame<'a>],
}
impl<'a> StaticAnimation<'a> {
    pub const fn new(frames: &'a [StaticAnimFrame<'a>]) -> Self {
        Self { frames, ..Self::default() }
    }
    pub const fn default() -> Self {
        Self {
            tick: 0,
            index: 0,
            frames: &[],
        }
    }
    pub fn current_frame(&self) -> &StaticAnimFrame<'a> {
        &self.frames.get(self.index).expect("Couldn't find animation frame!")
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

#[derive(Debug, Clone)]
pub struct AnimFrame {
    pub pos: Vec2,
    pub spr_id: u16,
    pub duration: u16,
    pub options: SpriteOptions,
    pub outline_colour: Option<u8>,
    pub palette_rotate: u8,
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
        Self { outline_colour: outline, ..self }
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

impl<'a> From<StaticAnimFrame<'a>> for AnimFrame {
    fn from(other: StaticAnimFrame) -> Self {
        Self {
            pos: other.pos,
            spr_id: other.spr_id,
            duration: other.duration,
            options: other.options.into(),
            outline_colour: other.outline_colour,
            palette_rotate: other.palette_rotate,
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
        Self { frames: frames.into(), ..Self::default() }
    }
    pub const fn default() -> Self {
        Self {
            tick: 0,
            index: 0,
            frames: Vec::new(),
        }
    }
    pub fn current_frame(&self) -> &AnimFrame {
        &self.frames.get(self.index).expect("Couldn't find animation frame!")
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

impl<'a> From<StaticAnimation<'a>> for Animation {
    fn from(other: StaticAnimation) -> Self {
        Self {
            tick: other.tick,
            index: other.index,
            frames: other.frames.iter().map(|x| x.clone().into()).collect(),
        }
    }
}
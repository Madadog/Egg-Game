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

use crate::{SpriteOptions, Vec2};

#[derive(Debug)]
pub struct AnimFrame<'a> {
    pub pos: Vec2,
    pub spr_id: u16,
    pub duration: u16,
    pub options: SpriteOptions<'a>,
    pub outline_colour: Option<u8>,
    pub palette_rotate: u8,
}
impl<'a> AnimFrame<'a> {
    pub const fn new(pos: Vec2, spr_id: u16, duration: u16, options: SpriteOptions<'a>) -> Self {
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
    pub fn const_default() -> Self {
        Self {
            pos: Vec2::new(0, 0),
            spr_id: 0,
            duration: 1,
            options: SpriteOptions::transparent_zero(),
            outline_colour: Some(1),
            palette_rotate: 0,
        }
    }
}

#[derive(Debug)]
pub struct Animation<'a> {
    pub tick: u16,
    pub index: usize,
    pub frames: &'a [AnimFrame<'a>],
}
impl<'a> Animation<'a> {
    pub const fn const_default() -> Self {
        Self {
            tick: 0,
            index: 0,
            frames: &[],
        }
    }
    pub fn current_frame(&self) -> &AnimFrame<'a> {
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

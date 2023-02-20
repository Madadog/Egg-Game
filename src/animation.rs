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
    pub id: u16,
    pub length: u16,
    pub options: SpriteOptions<'a>,
    pub outline: Option<u8>,
    pub palette_rotate: u8,
}
impl<'a> AnimFrame<'a> {
    pub const fn new(pos: Vec2, id: u16, length: u16, options: SpriteOptions<'a>) -> Self {
        Self {
            pos,
            id,
            length,
            options,
            outline: Some(1),
            palette_rotate: 0,
        }
    }
    pub const fn with_outline(self, outline: Option<u8>) -> Self {
        Self {
            outline,
            ..self
        }
    }
    pub fn const_default() -> Self {
        Self {
            pos: Vec2::new(0, 0),
            id: 0,
            length: 1,
            options: SpriteOptions::transparent_zero(),
            outline: Some(1),
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
        &self.frames[self.index]
    }
    pub fn advance(&mut self) {
        if self.tick >= self.current_frame().length {
            self.index += 1;
            if self.index == self.frames.len() {
                self.index = 0;
            }
            self.tick = 0;
        } else {
            self.tick += 1;
        }
    }
}

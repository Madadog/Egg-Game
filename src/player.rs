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

use crate::{Flip, Hitbox, Vec2};

#[derive(Debug)]
pub struct Player {
    /// coords are (x, y)
    pub dir: (i8, i8),
    pub hp: u8,
    pub local_hitbox: Hitbox,
    pub pos: Vec2,
    pub walking: bool,
    pub walktime: u16,
}
impl Player {
    pub const fn const_default() -> Self {
        Self {
            pos: Vec2::new(62, 23),
            local_hitbox: Hitbox::new(0, 10, 7, 5),
            hp: 3,
            dir: (0, 1),
            walktime: 0,
            walking: false,
        }
    }
    pub fn sprite_index(&self) -> (i32, Flip, i32) {
        let timer = (self.walktime + 19) / 20;
        let y_offset = (timer % 2) as i32;
        let sprite_offset = if self.walktime > 0 { y_offset + 1 } else { 0 };
        if self.dir.1 > 0 {
            (768 + sprite_offset, Flip::None, y_offset) // Up
        } else if self.dir.1 < 0 {
            (771 + sprite_offset, Flip::None, y_offset) // Down
        } else {
            let flip = if self.dir.0 > 0 {
                Flip::None
            } else {
                Flip::Horizontal
            };
            let index = match timer % 4 {
                0 | 2 => 832,
                1 => 833,
                _ => 834,
            };
            (index, flip, y_offset) // Left
        }
    }
    pub fn hitbox(&self) -> Hitbox {
        self.local_hitbox.offset(self.pos)
    }
}
impl Default for Player {
    fn default() -> Self {
        Self::const_default()
    }
}

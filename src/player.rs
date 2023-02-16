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

use crate::{Flip, Hitbox, Vec2, tic80::SpriteOptions, cam_x, cam_y};

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

#[derive(Clone, Copy, PartialEq)]
pub enum Companion {
    Dog,
}
impl Companion {
    pub fn spr_params(&self, position: Vec2, direction: (i8, i8), walktime: u8) -> (i32, i32, i32, SpriteOptions, u8, u8) {
        match &self {
            Self::Dog => {
                let t = (walktime / 10) % 2;
                let (w, i, flip) = match direction {
                    (1, 0) => (2, 706 + t as i32 * 2, Flip::Horizontal),
                    (-1, 0) => (2, 706 + t as i32 * 2, Flip::None),
                    (_, 1) => (1, 710 + t as i32, Flip::None),
                    (_, _) => (1, 712 + t as i32, Flip::None),
                };
                let x_offset = if let Flip::Horizontal = flip {
                    -8
                } else {
                    0
                };
                (i, position.x as i32 - cam_x() + x_offset,
                position.y as i32 - cam_y() - 2,
                SpriteOptions {
                    w,
                    h: 2,
                    flip,
                    ..SpriteOptions::transparent_zero()
                }, 1, 1)
            },
            _ => (0, 0, 0, SpriteOptions::default(), 0, 0),
        }
    }
}

pub struct CompanionTrail {
    positions: [Vec2; 16],
    directions: [(i8, i8); 16],
    walktime: u8,
}
impl CompanionTrail {
    pub const fn new() -> Self {
        Self {
            positions: [Vec2::new(0, 0); 16],
            directions: [(0, 0); 16],
            walktime: 0,
        }
    }
    /// When player moves, rotate all positions towards start of buffer, add new position end of buffer.
    pub fn push(&mut self, position: Vec2, direction: (i8, i8)) {
        assert_eq!(self.positions.len(), self.directions.len());
        for i in 0..self.positions.len()-1 {
            self.positions[i] = self.positions[i+1];
            self.directions[i] = self.directions[i+1];
        }
        self.positions[self.positions.len()-1] = position;
        self.directions[self.directions.len()-1] = direction;
        self.walktime = self.walktime.wrapping_add(1);
    }
    /// When player stops moving, tell animations to switch to idle pose.
    pub fn stop(&mut self) {
        self.walktime = 0;
    }
    /// Moves all companions to the same point.
    pub fn fill(&mut self, position: Vec2, direction: (i8, i8)) {
        self.positions.fill(position);
        self.directions.fill(direction);
    }
    pub fn mid(&self) -> (Vec2, (i8, i8)) {
        (
            self.positions[self.positions.len()/2],
            self.directions[self.directions.len()/2]
        )
    }
    pub fn oldest(&self) -> (Vec2, (i8, i8)) {
        (self.positions[0], self.directions[0])
    }
    pub fn walktime(&self) -> u8 {self.walktime}
}

pub struct CompanionList {
    pub companions: [Option<Companion>; 2]
}
impl CompanionList {
    pub const fn new() -> Self {
        Self { companions: [None; 2] }
    }
    pub fn add(&mut self, companion: Companion) {
        if let Some(x) = self.companions.iter_mut().find(|x| x.is_none()) {
            *x = Some(companion);
        } else {
            *self.companions.iter_mut().last().unwrap() = Some(companion);
        }
    }
    pub fn has(&self, companion: Companion) -> bool {
        self.companions.contains(&Some(companion))
    }
    pub fn remove(&mut self, target: Companion) -> bool {
        if let Some(x) = self.companions.iter_mut().find(|x| if let Some(x) = x {*x==target} else {false}) {
            *x = None;
            true
        } else {
            false
        }
    }
}
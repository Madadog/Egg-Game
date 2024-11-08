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

use std::ops::{Add, Div, Mul, Sub};

use crate::system::ConsoleApi;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vec2 {
    pub x: i16,
    pub y: i16,
}
impl Vec2 {
    pub const fn new(x: i16, y: i16) -> Self {
        Vec2 { x, y }
    }
    pub const fn splat(value: i16) -> Self {
        Vec2::new(value, value)
    }
    pub fn draw_tic80(&self, system: &mut impl ConsoleApi, colour: u8) {
        system.pix(self.x.into(), self.y.into(), colour);
    }
    pub fn towards(&self, other: &Vec2) -> Vec2 {
        let diff = *other - *self;
        Vec2::new(diff.x.clamp(-1, 1), diff.y.clamp(-1, 1))
    }
}

// Math operations on Vec2
impl Add for Vec2 {
    type Output = Vec2;

    fn add(self, rhs: Self) -> Self::Output {
        Vec2::new(self.x + rhs.x, self.y + rhs.y)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;

    fn sub(self, rhs: Self) -> Self::Output {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}
impl Mul for Vec2 {
    type Output = Vec2;

    fn mul(self, rhs: Self) -> Self::Output {
        Vec2::new(self.x * rhs.x, self.y * rhs.y)
    }
}
impl Div for Vec2 {
    type Output = Vec2;

    fn div(self, rhs: Self) -> Self::Output {
        Vec2::new(self.x / rhs.x, self.y / rhs.y)
    }
}

// Math operations with singular numbers...
impl Mul<i16> for Vec2 {
    type Output = Vec2;

    fn mul(self, rhs: i16) -> Self::Output {
        Vec2::new(self.x * rhs, self.y * rhs)
    }
}
impl Div<i16> for Vec2 {
    type Output = Vec2;

    fn div(self, rhs: i16) -> Self::Output {
        Vec2::new(self.x / rhs, self.y / rhs)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Hitbox {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
}
impl Hitbox {
    pub const fn new(x: i16, y: i16, w: i16, h: i16) -> Self {
        assert!(w.is_positive() && h.is_positive());
        Hitbox { x, y, w, h }
    }
    pub fn ex(&self) -> i16 {
        self.x + self.w - 1
    }
    pub fn ey(&self) -> i16 {
        self.y + self.h - 1
    }
    pub fn area(&self) -> i16 {
        self.w * self.h
    }
    pub fn x_intersects(&self, other: Hitbox) -> bool {
        self.x <= other.ex() && self.ex() >= other.x
    }
    pub fn y_intersects(&self, other: Hitbox) -> bool {
        self.y <= other.ey() && self.ey() >= other.y
    }
    pub fn xy_intersects(&self, other: Hitbox) -> bool {
        self.x_intersects(other) || self.y_intersects(other)
    }
    pub fn x_intersects_point(&self, point: Vec2) -> bool {
        self.x <= point.x && self.ex() >= point.x
    }
    pub fn y_intersects_point(&self, point: Vec2) -> bool {
        self.y <= point.y && self.ey() >= point.y
    }
    pub fn touches_point(&self, other: Vec2) -> bool {
        self.x_intersects_point(other) && self.y_intersects_point(other)
    }
    pub fn touches(&self, other: Hitbox) -> bool {
        self.x_intersects(other) && self.y_intersects(other)
    }
    pub fn offset_xy(&self, x: i16, y: i16) -> Self {
        Self {
            x: self.x + x,
            y: self.y + y,
            ..*self
        }
    }
    pub fn offset(&self, delta: Vec2) -> Self {
        self.offset_xy(delta.x, delta.y)
    }
    pub fn grow(&self, w: i16, h: i16) -> Self {
        Self {
            w: self.w + w,
            h: self.h + h,
            ..*self
        }
    }
    /// Returns corner points in the order `[Top Left, Top Right, Bottom Left, Bottom Right]`
    pub fn corners(&self) -> [Vec2; 4] {
        [
            Vec2::new(self.x, self.y),
            Vec2::new(self.ex(), self.y),
            Vec2::new(self.x, self.ey()),
            Vec2::new(self.ex(), self.ey()),
        ]
    }
    pub fn top_corners(&self) -> [Vec2; 2] {
        [Vec2::new(self.x, self.y), Vec2::new(self.ex(), self.y)]
    }
    pub fn bottom_corners(&self) -> [Vec2; 2] {
        [
            Vec2::new(self.x, self.ey()),
            Vec2::new(self.ex(), self.ey()),
        ]
    }
    pub fn left_corners(&self) -> [Vec2; 2] {
        [Vec2::new(self.x, self.y), Vec2::new(self.x, self.ey())]
    }
    pub fn right_corners(&self) -> [Vec2; 2] {
        [
            Vec2::new(self.ex(), self.y),
            Vec2::new(self.ex(), self.ey()),
        ]
    }
    pub fn dx_corners(&self, dx: i16) -> Option<[Vec2; 2]> {
        if dx != 0 {
            if dx.is_positive() {
                Some(self.offset_xy(dx, 0).right_corners())
            } else {
                Some(self.offset_xy(dx, 0).left_corners())
            }
        } else {
            None
        }
    }
    pub fn dy_corners(&self, dy: i16) -> Option<[Vec2; 2]> {
        if dy != 0 {
            if dy.is_positive() {
                Some(self.offset_xy(0, dy).bottom_corners())
            } else {
                Some(self.offset_xy(0, dy).top_corners())
            }
        } else {
            None
        }
    }
    pub fn dd_corner(&self, d: Vec2) -> Option<Vec2> {
        if d.x != 0 && d.y != 0 {
            let offset = self.offset(d);
            if d.y.is_positive() {
                if d.x.is_positive() {
                    Some(offset.corners()[3])
                } else {
                    Some(offset.corners()[2])
                }
            } else if d.x.is_positive() {
                Some(offset.corners()[1])
            } else {
                Some(offset.corners()[0])
            }
        } else {
            None
        }
    }
    pub fn draw(&self, system: &mut impl ConsoleApi, colour: u8) {
        system.rectb(
            self.x.into(),
            self.y.into(),
            self.w.into(),
            self.h.into(),
            colour,
        );
    }
}

pub fn touches_tile(flags: u8, point: Vec2) -> bool {
    let point = Vec2::new(point.x % 8, point.y % 8);
    // Tile flag corresponds to collision type
    match flags {
        0b0 => false,                           // Walkable
        0b1 => true,                            // Solid
        0b10 => point.x + point.y <= 7,         // Top-left ramp
        0b11 => point.x >= point.y,             // Top-right ramp
        0b100 => point.x + point.y >= 7,        // Bottom-right ramp
        0b101 => point.x <= point.y,            // Bottom-left ramp
        0b110 => point.y <= 3,                  // Top half
        0b111 => point.y >= 3,                  // Bottom half
        0b1000 => point.x >= 3,                 // Right half
        0b1001 => point.x <= 3,                 // Left half
        0b1010 => point.y <= 3 && point.x >= 3, // Top-right corner
        0b1011 => point.y <= 3 && point.x <= 3, // Top-left corner
        0b1100 => point.y >= 3 && point.x >= 3, // Bottom-right corner
        0b1101 => point.y >= 3 && point.x <= 3, // Bottom-left corner
        _ => false,
    }
}

/// An 8x8 custom bitmap collider
#[derive(Clone, Debug, Default)]
pub struct Collider {
    pub data: [[bool; 8]; 8],
}
impl Collider {
    pub fn get(&self, x: usize, y: usize) -> bool {
        let (x, y) = (x % 8, y % 8);
        self.data[y][x]
    }
    pub fn set(&mut self, x: usize, y: usize, value: bool) {
        let (x, y) = (x % 8, y % 8);
        self.data[y][x] = value;
    }
    pub fn from_sprite(system: &impl ConsoleApi, index: usize) -> Collider {
        let bitmap = system.get_bitmap_indexed(2);
        let mut collider = Collider::default();
        for i in 0..8 {
            for j in 0..8 {
                let sprite_offset = (index % 32) * 8 + (index / 32) * 2048;
                let pixel = bitmap[sprite_offset + i + j * 256];
                if pixel != 0 && pixel != 255 {
                    collider.set(i, j, true);
                }
            }
        }
        collider
    }
}

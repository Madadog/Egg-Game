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

use crate::{
    camera::Camera,
    map::{Axis, MapSet},
    tic80::SpriteOptions,
    Flip, Hitbox, Vec2,
};

#[derive(Debug)]
pub struct Player {
    /// coords are (x, y)
    pub dir: (i8, i8),
    pub hp: u8,
    pub local_hitbox: Hitbox,
    pub pos: Vec2,
    pub walking: bool,
    pub walktime: u16,
    pub flip_controls: Axis,
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
            flip_controls: Axis::None,
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
    pub fn walk(
        &mut self,
        mut dx: i16,
        mut dy: i16,
        noclip: bool,
        current_map: &MapSet,
    ) -> (i16, i16) {
        use crate::map::layer_collides;

        if dx == 0 && dy == 0 {
            return (dx, dy);
        };

        match self.flip_controls {
            Axis::None => {}
            Axis::X => dx *= -1,
            Axis::Y => dy *= -1,
            Axis::Both => {
                dx *= -1;
                dy *= -1
            }
        }

        // Face direction
        self.dir.1 = dy as i8;
        self.dir.0 = dx as i8;

        if noclip {
            return (dx, dy);
        };

        // Player position + intended movement
        let player_hitbox = self.hitbox();
        let delta_hitbox = player_hitbox.offset_xy(-1, -1).grow(2, 2);

        // Collide
        let points_dx = player_hitbox.dx_corners(dx);
        let points_dx_up = player_hitbox.offset_xy(0, -1).dx_corners(dx);
        let points_dx_down = player_hitbox.offset_xy(0, 1).dx_corners(dx);
        let (mut dx_collision_x, mut dx_collision_up, mut dx_collision_down) =
            (false, false, false);
        let points_dy = player_hitbox.dy_corners(dy);
        let points_dy_left = player_hitbox.offset_xy(-1, 0).dy_corners(dy);
        let points_dy_right = player_hitbox.offset_xy(1, 0).dy_corners(dy);
        let (mut dy_collision_y, mut dy_collision_left, mut dy_collision_right) =
            (false, false, false);
        let point_diag = player_hitbox.dd_corner(Vec2::new(dx, dy));
        let mut diagonal_collision = false;
        for layer in current_map.maps.iter() {
            let layer_hitbox = Hitbox::new(
                layer.sx as i16,
                layer.sy as i16,
                layer.w as i16 * 8,
                layer.h as i16 * 8,
            );
            if !layer_hitbox.touches(delta_hitbox) {
                continue;
            }
            [dx_collision_x, dx_collision_up, dx_collision_down] = test_many_points(
                [points_dx, points_dx_up, points_dx_down],
                layer_hitbox,
                layer.x,
                layer.y,
                [dx_collision_x, dx_collision_up, dx_collision_down],
            );
            [dy_collision_y, dy_collision_left, dy_collision_right] = test_many_points(
                [points_dy, points_dy_left, points_dy_right],
                layer_hitbox,
                layer.x,
                layer.y,
                [dy_collision_y, dy_collision_left, dy_collision_right],
            );
            if let Some(point_diag) = point_diag {
                if layer_collides(point_diag, layer_hitbox, layer.x, layer.y) {
                    diagonal_collision = true;
                }
            }
        }
        alt_dir(
            dx_collision_x,
            dx_collision_down,
            dx_collision_up,
            &mut dx,
            &mut dy,
        );
        alt_dir(
            dy_collision_y,
            dy_collision_right,
            dy_collision_left,
            &mut dy,
            &mut dx,
        );
        if diagonal_collision && dx != 0 && dy != 0 {
            dx = 0;
            dy = 0;
        }

        (dx, dy)
    }
    pub fn apply_motion(&mut self, dx: i16, dy: i16, trail: &mut CompanionTrail) {
        // Apply motion
        if dx == 0 && dy == 0 {
            trail.stop();
            self.walktime = 0;
            self.walking = false;
            return;
        }

        trail.push(Vec2::new(self.pos.x, self.pos.y), (self.dir.0, self.dir.1));
        self.pos.x += dx;
        self.pos.y += dy;
        self.walktime = self.walktime.wrapping_add(1);
        self.walking = true;
    }
}
impl Default for Player {
    fn default() -> Self {
        Self::const_default()
    }
}
fn test_many_points(
    p: [Option<[Vec2; 2]>; 3],
    layer_hitbox: Hitbox,
    layer_x: i32,
    layer_y: i32,
    mut flags: [bool; 3],
) -> [bool; 3] {
    use crate::map::layer_collides;
    for (i, points) in p.iter().enumerate() {
        if let Some(points) = points {
            points.iter().for_each(|point| {
                if layer_collides(*point, layer_hitbox, layer_x, layer_y) {
                    flags[i] = true;
                }
            });
        };
    }
    flags
}
fn alt_dir(main: bool, plus: bool, minus: bool, main_axis: &mut i16, sec_axis: &mut i16) {
    if *sec_axis == 0 && main {
        if !plus {
            *sec_axis = 1;
        } else if !minus {
            *sec_axis = -1;
        } else {
            *main_axis = 0;
        }
    } else if main {
        *main_axis = 0;
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Companion {
    Dog,
}
impl Companion {
    pub fn spr_params(
        &self,
        position: Vec2,
        direction: (i8, i8),
        walktime: u8,
        camera: &Camera,
    ) -> (i32, i32, i32, SpriteOptions, Option<u8>, u8) {
        match &self {
            Self::Dog => {
                let t = (walktime / 10) % 2;
                let (w, i, flip) = match direction {
                    (1, 0) => (2, 706 + t as i32 * 2, Flip::Horizontal),
                    (-1, 0) => (2, 706 + t as i32 * 2, Flip::None),
                    (_, 1) => (1, 710 + t as i32, Flip::None),
                    (_, _) => (1, 712 + t as i32, Flip::None),
                };
                let x_offset = if let Flip::Horizontal = flip { -8 } else { 0 };
                (
                    i,
                    position.x as i32 - camera.x() + x_offset,
                    position.y as i32 - camera.y() - 2,
                    SpriteOptions {
                        w,
                        h: 2,
                        flip,
                        ..SpriteOptions::transparent_zero()
                    },
                    Some(1),
                    1,
                )
            }
            _ => (0, 0, 0, SpriteOptions::default(), None, 0),
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
        for i in 0..self.positions.len() - 1 {
            self.positions[i] = self.positions[i + 1];
            self.directions[i] = self.directions[i + 1];
        }
        self.positions[self.positions.len() - 1] = position;
        self.directions[self.directions.len() - 1] = direction;
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
            self.positions[self.positions.len() / 2],
            self.directions[self.directions.len() / 2],
        )
    }
    pub fn oldest(&self) -> (Vec2, (i8, i8)) {
        (self.positions[0], self.directions[0])
    }
    pub fn walktime(&self) -> u8 {
        self.walktime
    }
}

pub struct CompanionList {
    pub companions: [Option<Companion>; 2],
}
impl CompanionList {
    pub const fn new() -> Self {
        Self {
            companions: [None; 2],
        }
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
        if let Some(x) =
            self.companions
                .iter_mut()
                .find(|x| if let Some(x) = x { *x == target } else { false })
        {
            *x = None;
            true
        } else {
            false
        }
    }
}

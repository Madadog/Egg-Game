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
    interact::Interactable,
    map::{Axis, MapSet},
    tic80_core::SpriteOptions,
    tic80_helpers::DrawParams,
    Flip, Hitbox, Vec2, position,
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
    pub pet_timer: Option<u8>,
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
            pet_timer: None,
        }
    }
    pub fn sprite_index(&self) -> (i32, Flip, i32) {
        let timer = (self.walktime + 19) / 20;
        let y_offset = (timer % 2) as i32;
        let sprite_offset = if self.walktime > 0 { y_offset + 1 } else { 0 };
        let flip = if self.dir.0 > 0 {
            Flip::None
        } else {
            Flip::Horizontal
        };
        if let Some(t) = self.pet_timer {
            return (774 + (t / 20 % 2) as i32, flip, 0);
        }
        if self.dir.1 > 0 {
            (768 + sprite_offset, Flip::None, y_offset) // Up
        } else if self.dir.1 < 0 {
            (771 + sprite_offset, Flip::None, y_offset) // Down
        } else {
            let index = match timer % 4 {
                0 | 2 => 832,
                1 => 833,
                _ => 834,
            };
            (index, flip, y_offset) // Left
        }
    }
    pub fn draw_params(&self, offset: Vec2) -> DrawParams {
        let player_sprite = self.sprite_index();
        DrawParams::new(
            player_sprite.0,
            i32::from(self.pos.x - offset.x),
            i32::from(self.pos.y - offset.y) - player_sprite.2,
            SpriteOptions {
                w: 1,
                h: 2,
                transparent: &[0],
                scale: 1,
                flip: player_sprite.1,
                ..Default::default()
            },
            Some(1),
            1,
        )
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
                layer.offset().x,
                layer.offset().y,
                layer.size().x * 8,
                layer.size().y * 8,
            );
            if !layer_hitbox.touches(delta_hitbox) {
                continue;
            }
            [dx_collision_x, dx_collision_up, dx_collision_down] = test_many_points(
                [points_dx, points_dx_up, points_dx_down],
                layer_hitbox,
                layer.origin.x().into(),
                layer.origin.y().into(),
                layer.shift_sprite_flags(),
                [dx_collision_x, dx_collision_up, dx_collision_down],
            );
            [dy_collision_y, dy_collision_left, dy_collision_right] = test_many_points(
                [points_dy, points_dy_left, points_dy_right],
                layer_hitbox,
                layer.origin.x().into(),
                layer.origin.y().into(),
                layer.shift_sprite_flags(),
                [dy_collision_y, dy_collision_left, dy_collision_right],
            );
            if let Some(point_diag) = point_diag {
                if layer_collides(
                    point_diag,
                    layer_hitbox,
                    layer.origin.x().into(),
                    layer.origin.y().into(),
                    layer.shift_sprite_flags(),
                ) {
                    diagonal_collision = true;
                }
            }
        }
        slide_ramp(
            dx_collision_x,
            dx_collision_down,
            dx_collision_up,
            &mut dx,
            &mut dy,
        );
        slide_ramp(
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
            self.animate_stop();
        } else {
            trail.push(Vec2::new(self.pos.x, self.pos.y), (self.dir.0, self.dir.1));
            self.pos.x += dx;
            self.pos.y += dy;
            self.animate_walk();
        }
    }
    pub fn animate_walk(&mut self) {
        self.walktime = self.walktime.wrapping_add(1);
        self.walking = true;
    }
    pub fn animate_stop(&mut self) {
        self.walktime = 0;
        self.walking = false;
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
    spr_flag_offset: bool,
    mut flags: [bool; 3],
) -> [bool; 3] {
    use crate::map::layer_collides;
    for (i, points) in p.iter().enumerate() {
        if let Some(points) = points {
            points.iter().for_each(|point| {
                if layer_collides(*point, layer_hitbox, layer_x, layer_y, spr_flag_offset) {
                    flags[i] = true;
                }
            });
        };
    }
    flags
}

/// Logic for sliding on 1 pixel ramps.
///
/// If there is a forwards collision but no diagonal one,
/// this function will move in the first available
/// diagonal direction.
fn slide_ramp(
    main_axis_collides: bool,
    plus_side_collides: bool,
    minus_side_collides: bool,
    main_axis_delta: &mut i16,
    side_axis_delta: &mut i16,
) {
    if !main_axis_collides {
        return;
    }
    if *side_axis_delta == 0 {
        if !plus_side_collides {
            *side_axis_delta = 1;
        } else if !minus_side_collides {
            *side_axis_delta = -1;
        } else {
            *main_axis_delta = 0;
        }
    } else {
        *main_axis_delta = 0;
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
    ) -> DrawParams {
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
                DrawParams::new(
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
            _ => DrawParams::new(0, 0, 0, SpriteOptions::default(), None, 0),
        }
    }
    pub fn interact(
        self,
        position: Vec2,
        direction: (i8, i8),
        player_position: Vec2,
    ) -> Interactable<'static> {
        use crate::interact::{InteractFn, Interaction};
        match self {
            Companion::Dog => {
                let mut pixel = 0;
                let offset = if direction.1 == 0 {
                    direction.0 > 0
                } else {
                    let x = player_position.x > position.x;
                    if x {
                        pixel -= 1;
                    }
                    x
                };
                let position = position + Vec2::new(pixel, 0);
                Interactable::new(
                    Hitbox::new(position.x, position.y, 16, 16),
                    Interaction::Func(InteractFn::Pet(position, Some(offset))),
                    None,
                )
            }
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
        *self.positions.last_mut().unwrap() = position;
        *self.directions.last_mut().unwrap() = direction;
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
    pub fn latest(&self) -> (Vec2, (i8, i8)) {
        (
            *self.positions.last().unwrap(),
            *self.directions.last().unwrap(),
        )
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
    pub fn count(&self) -> usize {
        self.companions
            .iter()
            .filter(|companion| companion.is_some())
            .count()
    }
    pub fn interact(&self, positions: &CompanionTrail) -> Vec<Interactable<'static>> {
        match self.companions {
            [Some(x), Some(y)] => vec![
                x.interact(positions.mid().0, positions.mid().1, positions.latest().0),
                y.interact(
                    positions.oldest().0,
                    positions.oldest().1,
                    positions.latest().0,
                ),
            ],
            [Some(x), None] => vec![x.interact(
                positions.oldest().0,
                positions.oldest().1,
                positions.latest().0,
            )],
            [None, None] => vec![],
            [None, Some(_)] => todo!(),
        }
    }
}

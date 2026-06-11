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

use std::mem;

use crate::{
    camera::Camera,
    data::{sound, tmj::TiledMap},
    interact::{Interactable, Interaction},
    map::{Axis, LayerInfo, MapInfo},
    position::{Hitbox, Vec2},
    system::{ConsoleApi, ConsoleHelper, DrawParams, Flip, SpriteOptions},
};

#[derive(Debug, Clone, Default)]
pub enum LoopMode {
    #[default]
    Loop,
    LoopRange(usize, usize),
    Hold,
}
impl LoopMode {
    pub fn loop_index(&self, index: usize, len: usize) -> usize {
        debug_assert!(len > 0);
        match self {
            LoopMode::Loop => index % len,
            &LoopMode::LoopRange(mut start, mut end) => {
                if index > end {
                    if start > end {
                        mem::swap(&mut start, &mut end);
                    }
                    let len = end - start + 1;
                    if len == 1 {
                        end
                    } else {
                        let zeroed_index = index - (end + 1);
                        start + (zeroed_index % len)
                    }
                } else {
                    index
                }
            }
            LoopMode::Hold => index.min(len - 1),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpriteAnimation {
    frames: Vec<SpriteOptions>,
    loopmode: LoopMode,
}

impl SpriteAnimation {
    pub fn new(frames: Vec<SpriteOptions>, loopmode: LoopMode) -> Self {
        Self { frames, loopmode }
    }
    pub fn from_sprite_frames(frames: &[SpriteOptions]) -> Self {
        Self::new(frames.to_vec(), LoopMode::default())
    }
    pub fn from_sprite_ids(ids: &[i32], w: i32, h: i32) -> Self {
        let frames: Vec<SpriteOptions> = ids
            .iter()
            .map(|id| SpriteOptions {
                id: *id,
                w,
                h,
                transparent: Some(0),
                ..SpriteOptions::default()
            })
            .collect();
        Self::from_sprite_frames(&frames)
    }
    pub fn from_base_sprite_id(id: i32, len: i32, w: i32, h: i32) -> Self {
        let frames: Vec<SpriteOptions> = (id..(id + len * w))
            .step_by(w as usize)
            .map(|id| SpriteOptions {
                id,
                w,
                h,
                transparent: Some(0),
                ..SpriteOptions::default()
            })
            .collect();
        Self::from_sprite_frames(&frames)
    }
    pub fn with_flip(mut self, flip: Flip) -> Self {
        self.frames
            .iter_mut()
            .for_each(|frame| frame.flip = flip.clone());
        self
    }
    pub fn with_x_offset(mut self, x_offset: i32) -> Self {
        self.frames
            .iter_mut()
            .for_each(|frame| frame.x_offset = x_offset);
        self
    }
    pub fn with_loopmode(self, loopmode: LoopMode) -> Self {
        Self { loopmode, ..self }
    }
    pub fn frames(&self) -> &[SpriteOptions] {
        &self.frames
    }
    pub fn get_frame(&self, i: usize) -> &SpriteOptions {
        &self.frames()[self.loopmode.loop_index(i, self.frames().len())]
    }
}

#[derive(Debug, Clone)]
pub enum WalkSprites {
    /// Unique sprites for all four directions.
    Compass {
        north: SpriteAnimation,
        south: SpriteAnimation,
        west: SpriteAnimation,
        east: SpriteAnimation,
    },
    /// North & south sprites only, mirrored for side directions.
    /// Default: East unmirrored, west mirrored.
    FrontBack {
        north: SpriteAnimation,
        south: SpriteAnimation,
    },
}
impl WalkSprites {
    pub fn dir_to_sprite(&self, dir: (i8, i8)) -> &SpriteAnimation {
        match self {
            WalkSprites::Compass {
                north,
                south,
                west,
                east,
            } => match dir {
                (1, 0) => east,
                (-1, 0) => west,
                (_, 1) => south,
                (_, _) => north,
            },
            WalkSprites::FrontBack { north, south } => match dir {
                (_, 1) => south,
                (_, _) => north,
            },
        }
    }
    /// Humanoid 4-direction walk. North/south are 3-frame strips (idle + 2 walk
    /// frames, looping the walk pair); the north strip sits 3 tiles after
    /// `south`. The side-on walk cycles `[s, s+1, s, s+2]` from `side`, west
    /// mirrored from east.
    fn humanoid(south: i32, side: i32) -> Self {
        let strip = |base| {
            SpriteAnimation::from_base_sprite_id(base, 3, 1, 2)
                .with_loopmode(LoopMode::LoopRange(1, 2))
        };
        let walk = || SpriteAnimation::from_sprite_ids(&[side, side + 1, side, side + 2], 1, 2);
        Self::Compass {
            north: strip(south + 3),
            south: strip(south),
            west: walk().with_flip(Flip::Horizontal),
            east: walk(),
        }
    }
    pub fn ellie() -> Self {
        Self::humanoid(768, 832)
    }
    pub fn may() -> Self {
        Self::humanoid(2184, 2248)
    }
    pub fn dog() -> Self {
        Self::Compass {
            north: SpriteAnimation::from_base_sprite_id(966, 2, 1, 2),
            south: SpriteAnimation::from_base_sprite_id(964, 2, 1, 2),
            west: SpriteAnimation::from_base_sprite_id(960, 2, 2, 2).with_flip(Flip::Horizontal),
            east: SpriteAnimation::from_base_sprite_id(960, 2, 2, 2).with_x_offset(8),
        }
    }
    pub fn bro() -> Self {
        Self::humanoid(896, 902)
    }
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum MoveMode {
    Player,
    Wander,
}

#[derive(Debug, Clone)]
pub struct ShellSprites {
    pub walk: WalkSprites,
    pub others: Vec<SpriteAnimation>,
}
impl ShellSprites {
    fn new(walk: WalkSprites, other_ids: &[i32], w: i32, h: i32) -> Self {
        Self {
            walk,
            others: vec![SpriteAnimation::from_sprite_ids(other_ids, w, h)],
        }
    }
    pub fn ellie() -> Self {
        Self::new(WalkSprites::ellie(), &[774, 775], 1, 2)
    }
    pub fn may() -> Self {
        Self::new(WalkSprites::may(), &[2251, 2252], 1, 2)
    }
    pub fn dog() -> Self {
        Self::new(WalkSprites::dog(), &[968, 970], 2, 2)
    }
    pub fn bro() -> Self {
        Self::new(WalkSprites::bro(), &[905, 906], 1, 2)
    }
}

/// A controllable game entity.
#[derive(Debug, Clone)]
pub struct Shell {
    /// coords are (x, y)
    pub dir: (i8, i8),
    pub hp: u8,
    pub local_hitbox: Hitbox,
    pub pos: Vec2,
    pub walking: bool,
    pub walktime: u16,
    pub flip_controls: Axis,
    pub pet_timer: Option<u8>,
    pub sprites: ShellSprites,
    pub move_mode: MoveMode,
}
impl Default for Shell {
    fn default() -> Self {
        Self::ellie()
    }
}
impl Shell {
    pub fn sprite_options(&self) -> (SpriteOptions, i32) {
        let timer = if self.dir.1 == 0 {
            // sideways anim 4fps
            self.walktime.div_ceil(15)
        } else {
            // up/down anim at 3fps
            self.walktime.div_ceil(20)
        };
        let y_offset = (timer % 2) as i32;
        // petting animation
        if let Some(t) = self.pet_timer {
            let t = (t / 20 % 2) as usize;
            let mut sprite = self.sprites.others[0].get_frame(t).clone();
            sprite.flip = if self.dir.0 > 0 {
                Flip::None
            } else {
                Flip::Horizontal
            };
            return (sprite, 0);
        }
        let sprite = self
            .sprites
            .walk
            .dir_to_sprite(self.dir)
            .get_frame(timer as usize)
            .clone();
        (sprite, y_offset)
    }
    pub fn draw_params(&self, offset: Vec2) -> DrawParams {
        let (sprite, y_offset) = self.sprite_options();
        DrawParams::new(
            sprite.id,
            i32::from(self.pos.x - offset.x - sprite.x_offset as i16),
            i32::from(self.pos.y - offset.y - sprite.y_offset as i16) - y_offset,
            sprite,
            Some(1),
            0,
        )
    }
    pub fn with_pos(self, pos: Vec2) -> Self {
        Self { pos, ..self }
    }
    pub fn with_move_mode(self, move_mode: MoveMode) -> Self {
        Self { move_mode, ..self }
    }
    pub fn replace(&mut self, shell: Shell) {
        *self = shell.with_pos(self.pos).with_move_mode(self.move_mode);
    }
    pub fn hitbox(&self) -> Hitbox {
        self.local_hitbox.offset(self.pos)
    }
    pub fn apply_walk_direction(&mut self, mut dx: i16, mut dy: i16) -> (i16, i16) {
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

        (dx, dy)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn walk(
        &mut self,
        system: &mut impl ConsoleApi,
        sprite_flags: &[u8],
        mut dx: i16,
        mut dy: i16,
        noclip: bool,
        current_map: &MapInfo,
        tiles: Option<&TiledMap>,
    ) -> (i16, i16) {
        use crate::map::layer_collides_flags;

        if dx == 0 && dy == 0 {
            return (dx, dy);
        };

        (dx, dy) = self.apply_walk_direction(dx, dy);

        if noclip {
            return (dx, dy);
        };

        if (self.walktime + 15).is_multiple_of(20) {
            system.play_sound(sound::FOOTSTEP_PLAIN.with_note(17));
        }

        // No tile source loaded for this map (e.g. the empty default map):
        // nothing to collide with, so walk freely.
        let Some(tiles) = tiles else {
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
        for layer in current_map.layers.iter() {
            let layer_hitbox = layer.hitbox();
            if !layer_hitbox.touches(delta_hitbox) {
                continue;
            }
            [dx_collision_x, dx_collision_up, dx_collision_down] = test_many_points(
                sprite_flags,
                layer,
                tiles,
                [points_dx, points_dx_up, points_dx_down],
                [dx_collision_x, dx_collision_up, dx_collision_down],
            );
            [dy_collision_y, dy_collision_left, dy_collision_right] = test_many_points(
                sprite_flags,
                layer,
                tiles,
                [points_dy, points_dy_left, points_dy_right],
                [dy_collision_y, dy_collision_left, dy_collision_right],
            );
            if let Some(point_diag) = point_diag
                && layer_collides_flags(sprite_flags, point_diag, layer, tiles)
            {
                diagonal_collision = true;
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
    pub fn apply_motion<const N: usize>(
        &mut self,
        dx: i16,
        dy: i16,
        trail: Option<&mut CompanionTrail<N>>,
    ) {
        // Apply motion
        if dx == 0 && dy == 0 {
            if let Some(x) = trail {
                x.stop()
            }
            self.animate_stop();
        } else {
            if let Some(x) = trail {
                x.push(Vec2::new(self.pos.x, self.pos.y), (self.dir.0, self.dir.1))
            }
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

// presets
impl Shell {
    fn preset(local_hitbox: Hitbox, sprites: ShellSprites) -> Self {
        Self {
            pos: Vec2::new(62, 23),
            local_hitbox,
            hp: 3,
            dir: (0, 1),
            walktime: 0,
            walking: false,
            flip_controls: Axis::None,
            pet_timer: None,
            sprites,
            move_mode: MoveMode::Wander,
        }
    }
    pub fn ellie() -> Self {
        Self::preset(Hitbox::new(0, 10, 7, 5), ShellSprites::ellie())
    }
    pub fn may() -> Self {
        Self::preset(Hitbox::new(0, 12, 7, 5), ShellSprites::may())
    }
    pub fn dog() -> Self {
        Self::preset(Hitbox::new(0, 11, 7, 6), ShellSprites::dog())
    }
    pub fn bro() -> Self {
        Self::preset(Hitbox::new(0, 8, 7, 4), ShellSprites::bro())
    }
}

fn test_many_points(
    sprite_flags: &[u8],
    layer: &LayerInfo,
    tiles: &TiledMap,
    points: [Option<[Vec2; 2]>; 3],
    mut side_flags: [bool; 3],
) -> [bool; 3] {
    use crate::map::layer_collides_flags;
    for (i, points) in points.iter().enumerate() {
        if let Some(points) = points {
            points.iter().for_each(|point| {
                if layer_collides_flags(sprite_flags, *point, layer, tiles) {
                    side_flags[i] = true;
                }
            });
        };
    }
    side_flags
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

#[derive(Clone, Copy, PartialEq, Debug)]
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
        }
    }
    pub fn interact(
        self,
        position: Vec2,
        direction: (i8, i8),
        player_position: Vec2,
    ) -> Interactable {
        use crate::interact::InteractFn;
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

#[derive(Clone, Debug)]
pub struct CompanionTrail<const N: usize> {
    positions: [Vec2; N],
    directions: [(i8, i8); N],
    walktime: u8,
}
impl<const N: usize> Default for CompanionTrail<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> CompanionTrail<N> {
    pub const fn new() -> Self {
        Self {
            positions: [Vec2::new(0, 0); N],
            directions: [(0, 0); N],
            walktime: 0,
        }
    }
    /// When player moves, rotate all positions towards start of buffer, add new position end of buffer.
    pub fn push(&mut self, position: Vec2, direction: (i8, i8)) {
        self.positions.rotate_left(1);
        self.directions.rotate_left(1);

        // Array always has at least one element (N >= 1)
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
        // Array always has at least one element (N >= 1)
        (
            *self.positions.last().unwrap(),
            *self.directions.last().unwrap(),
        )
    }
    pub fn walktime(&self) -> u8 {
        self.walktime
    }
}

#[derive(Clone, Debug, Default)]
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
        if let Some(x) = self
            .companions
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
    pub fn interact<const N: usize>(&self, positions: &CompanionTrail<N>) -> Vec<Interactable> {
        // Trail points go to companions by presence, not slot: with two, the
        // first walks at the trail's midpoint and the second at its tail; a
        // lone companion (whichever slot it occupies) takes the tail.
        let present: Vec<Companion> = self.companions.iter().flatten().copied().collect();
        let count = present.len();
        present
            .into_iter()
            .enumerate()
            .map(|(i, companion)| {
                let (position, direction) = if count == 2 && i == 0 {
                    positions.mid()
                } else {
                    positions.oldest()
                };
                companion.interact(position, direction, positions.latest().0)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn companion_in_second_slot_interacts_without_panic() {
        let list = CompanionList {
            companions: [None, Some(Companion::Dog)],
        };
        let trail: CompanionTrail<16> = CompanionTrail::new();
        assert_eq!(list.interact(&trail).len(), 1);
    }
}

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

use crate::animation::*;
use crate::dialogue_data::*;
use crate::interact::InteractFn;
use crate::interact::{Interactable, Interaction};
use crate::position::{Hitbox, Vec2};
use crate::{MapOptions, SpriteOptions};

pub(crate) const DEFAULT_MAP: MapOptions = MapOptions {
    x: 60,
    y: 17,
    w: 30,
    h: 17,
    transparent: &[],
    sx: 0,
    sy: 0,
    scale: 1,
};
pub(crate) const DEFAULT_MAP_SET: MapSet = MapSet {
    maps: &[],
    fg_maps: &[],
    warps: &[],
    interactables: &[],
    bg_colour: 0,
    palette_rotation: &[],
    music_track: None,
    bank: 0,
};

#[derive(Clone)]
pub struct MapSet<'a> {
    pub maps: &'a [MapOptions<'a>],
    pub fg_maps: &'a [MapOptions<'a>],
    pub warps: &'a [Warp<'a>],
    pub interactables: &'a [Interactable<'a>],
    pub bg_colour: u8,
    pub palette_rotation: &'a [u8],
    pub music_track: Option<u8>,
    pub bank: u8,
}

#[derive(Clone)]
pub struct Warp<'a> {
    pub from: Hitbox,
    pub map: Option<&'a MapSet<'a>>,
    pub to: Vec2,
}

impl<'a> Warp<'a> {
    pub const fn new(from: Hitbox, map: Option<&'a MapSet<'a>>, to: Vec2) -> Self {
        Self { from, map, to }
    }
    /// Defaults to 8x8 tile, start and end destinations are in 8x8 tile coordinates (i.e. tx1=2 becomes x=16)
    pub const fn new_tile(
        tx1: i16,
        ty1: i16,
        map: Option<&'a MapSet<'a>>,
        tx2: i16,
        ty2: i16,
    ) -> Self {
        Self::new(
            Hitbox::new(tx1 * 8, ty1 * 8, 8, 8),
            map,
            Vec2::new(tx2 * 8, ty2 * 8),
        )
    }
}

pub static SUPERMARKET: MapSet<'static> = MapSet {
    maps: &[
        MapOptions {
            //bg
            x: 60,
            y: 17,
            w: 26,
            h: 12,
            transparent: &[0],
            ..DEFAULT_MAP
        },
        MapOptions {
            //fruit stand
            x: 61,
            y: 29,
            w: 3,
            h: 2,
            transparent: &[0],
            sx: 2 * 8,
            sy: 8 * 8,
            scale: 1,
        },
        MapOptions {
            //vending machines
            x: 70,
            y: 29,
            w: 4,
            h: 5,
            transparent: &[0],
            sx: 19 * 8,
            sy: 4 * 8,
            scale: 1,
        },
        MapOptions {
            //counter
            x: 60,
            y: 31,
            w: 8,
            h: 3,
            transparent: &[0],
            sx: 5 * 8,
            sy: 4 * 8,
            scale: 1,
        },
        MapOptions {
            //top vending machine
            x: 68,
            y: 29,
            w: 2,
            h: 3,
            transparent: &[0],
            sx: 13 * 8,
            sy: 5 * 4,
            scale: 1,
        },
    ],
    warps: &[
        Warp::new_tile(17, 4, Some(&SUPERMARKET_HALL), 9, 4),
        Warp::new_tile(8, 4, Some(&SUPERMARKET_HALL), 3, 4),
        Warp::new(Hitbox::new(11*8,11*8,3*8,8),Some(&HOUSE_LIVING_ROOM),Vec2::new(4*8,9*8)),
    ],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(13 * 8, 5 * 4, 8 * 2, 8 * 3),
            interaction: Interaction::Text(SM_COIN_RETURN),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(2 * 8, 8 * 8, 8 * 3, 8 * 2),
            interaction: Interaction::Text(SM_FRUIT_BASKET),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(4 * 8, 5 * 8, 8, 20),
            interaction: Interaction::Text(SM_MAIN_WINDOW),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(19 * 8, 5 * 8, 8, 15),
            interaction: Interaction::Text(SM_FRIDGE_1),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(20 * 8, 6 * 8, 8, 15),
            interaction: Interaction::Text(SM_FRIDGE_2),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(21 * 8, 7 * 8, 8, 16),
            interaction: Interaction::Text(SM_VENDING_MACHINE),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(11 * 8, 10 * 8, 3 * 8, 8),
            interaction: Interaction::Text(CONSTRUCTION_1),
            sprite: None,
        },
    ],
    palette_rotation: &[1],
    bg_colour: 1,
    ..DEFAULT_MAP_SET
};

pub static SUPERMARKET_HALL: MapSet<'static> = MapSet {
    maps: &[
        MapOptions {
            //bg
            x: 86,
            y: 17,
            w: 13,
            h: 7,
            transparent: &[0],
            ..DEFAULT_MAP
        },
        MapOptions {
            //closet
            x: 87,
            y: 24,
            w: 3,
            h: 4,
            transparent: &[0],
            sx: 5 * 8,
            sy: 0,
            scale: 1,
        },
        MapOptions {
            //diagonal door
            x: 86,
            y: 24,
            w: 1,
            h: 3,
            transparent: &[0],
            sx: 11 * 8,
            sy: 2 * 8,
            scale: 1,
        },
    ],
    warps: &[
        Warp::new_tile(9, 6, Some(&SUPERMARKET), 17, 4),
        Warp::new_tile(3, 6, Some(&SUPERMARKET), 8, 4),
        Warp::new_tile(4, 2, Some(&SUPERMARKET_STOREROOM), 2, 3),
    ],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(11 * 8, 4 * 8, 8, 8),
            interaction: Interaction::Text(EMERGENCY_EXIT),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(8 * 8, 3 * 8, 8, 8),
            interaction: Interaction::Text(CONSTRUCTION_2),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(11 * 4, 0, 2 * 8, 7 * 4),
            interaction: Interaction::Text(SM_HALL_SHELF),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(8, 3 * 8, 12, 16),
            interaction: Interaction::Text(SM_HALL_WINDOW),
            sprite: None,
        },
    ],
    palette_rotation: &[1],
    bg_colour: 1,
    ..DEFAULT_MAP_SET
};

pub static SUPERMARKET_STOREROOM: MapSet<'static> = MapSet {
    maps: &[
        MapOptions {
            x: 86,
            y: 28,
            w: 9,
            h: 6,
            transparent: &[0],
            ..DEFAULT_MAP
        },
        MapOptions {
            x: 93,
            y: 24,
            w: 5,
            h: 4,
            transparent: &[0],
            sx: 2 * 8,
            ..DEFAULT_MAP
        },
    ],
    warps: &[Warp::new_tile(2, 5, Some(&SUPERMARKET_HALL), 4, 2)],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(53, 28, 8, 10),
            interaction: Interaction::Text(EGG_1),
            sprite: Some(Animation {
                frames: &[
                    AnimFrame::new(Vec2::new(0, 0), 524, 30, SpriteOptions::transparent_zero()),
                    AnimFrame::new(Vec2::new(0, -1), 524, 30, SpriteOptions::transparent_zero()),
                ],
                ..Animation::const_default()
            }),
        },
        Interactable {
            hitbox: Hitbox::new(16, 0, 5 * 8, 4 * 7),
            interaction: Interaction::Text(SM_STOREROOM_SHELF),
            sprite: None,
        },
    ],
    palette_rotation: &[1],
    bg_colour: 1,
    ..DEFAULT_MAP_SET
};

pub static TEST_PEN: MapSet<'static> = MapSet {
    maps: &[
        MapOptions {
            x: 53,
            y: 17,
            w: 7,
            h: 9,
            ..DEFAULT_MAP
        },
    ],
    warps: &[Warp::new_tile(3, 8, Some(&SUPERMARKET), 10, 4)],
    interactables: &[Interactable {
        hitbox: Hitbox::new(5 * 8, 8, 8, 10),
        interaction: Interaction::Text(EGG_1),
        sprite: Some(Animation {
            frames: &[
                AnimFrame::new(Vec2::new(0, 0), 524, 30, SpriteOptions::transparent_zero()),
                AnimFrame::new(Vec2::new(0, -1), 524, 30, SpriteOptions::transparent_zero()),
            ],
            ..Animation::const_default()
        }),
    }],
    palette_rotation: &[1],
    bg_colour: 1,
    ..DEFAULT_MAP_SET
};

pub static BEDROOM: MapSet<'static> = MapSet {
    maps: &[
        MapOptions { //room
            x: 30,
            y: 0,
            w: 21,
            h: 10,
            ..DEFAULT_MAP
        },
        MapOptions { //trolley
            x: 30,
            y: 10,
            w: 3,
            h: 2,
            transparent: &[0],
            sx: 101,
            sy: 22,
            ..DEFAULT_MAP
        },
        MapOptions { //mattress
            x: 37,
            y: 10,
            w: 3,
            h: 2,
            transparent: &[0],
            sx: 38,
            sy: 27,
            ..DEFAULT_MAP
        },
    ],
    warps: &[Warp::new_tile(17, 6, Some(&HOUSE_STAIRWELL), 1, 2)],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(38, 27, 3*8, 2*8),
            interaction: Interaction::Text(BEDROOM_MATTRESS),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(2*8, 4*8, 2*8, 4*8),
            interaction: Interaction::Text(BEDROOM_CLOSET),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(101, 22, 3*8, 2*8),
            interaction: Interaction::Text(BEDROOM_TROLLEY),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(10*8, 3*8, 8, 8),
            interaction: Interaction::Text(BEDROOM_WINDOW),
            sprite: None,
        },
    ],
    ..DEFAULT_MAP_SET
};

pub static HOUSE_STAIRWELL: MapSet<'static> = MapSet {
    maps: &[
        MapOptions { //room
            x: 51,
            y: 0,
            w: 16,
            h: 9,
            ..DEFAULT_MAP
        },
        MapOptions { //left door
            x: 41,
            y: 10,
            w: 1,
            h: 3,
            transparent: &[0],
            sx: 0,
            sy: 7,
            ..DEFAULT_MAP
        },
        MapOptions { //right door
            x: 40,
            y: 10,
            w: 1,
            h: 3,
            transparent: &[0],
            sx: 120,
            sy: 7,
            ..DEFAULT_MAP
        },
    ],
    warps: &[Warp::new(Hitbox::new(1,3*8,8,8),Some(&BEDROOM),Vec2::new(16*8,5*8)),
             Warp::new(Hitbox::new(7*8,9*8,2*8,8),Some(&HOUSE_LIVING_ROOM),Vec2::new(21*4,4*8))],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(2*8, 2*8, 8, 8),
            interaction: Interaction::Text(HOUSE_STAIRWELL_WINDOW),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(7*8, 4*8, 2*8, 8),
            interaction: Interaction::Text(HOUSE_STAIRWELL_PAINTING),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(13*8, 2*8, 8, 8),
            interaction: Interaction::Text(HOUSE_STAIRWELL_WINDOW2),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(15*8, 3*8, 8, 8),
            interaction: Interaction::Text(HOUSE_STAIRWELL_DOOR),
            sprite: None,
        },
    ],
    ..DEFAULT_MAP_SET
};

pub static HOUSE_LIVING_ROOM: MapSet<'static> = MapSet {
    maps: &[
        MapOptions { //room
            x: 67,
            y: 0,
            w: 23,
            h: 13,
            ..DEFAULT_MAP
        },
        MapOptions { //couch
            x: 37,
            y: 13,
            w: 4,
            h: 3,
            transparent: &[0],
            sx: 12*8+2,
            sy: 7*8,
            ..DEFAULT_MAP
        },
        MapOptions { //tv
            x: 41,
            y: 13,
            w: 2,
            h: 3,
            transparent: &[0],
            sx: 15*8+2,
            sy: 9*8-1,
            ..DEFAULT_MAP
        },
    ],
    warps: &[Warp::new(Hitbox::new(10*8,4*8,2*8,8),Some(&HOUSE_STAIRWELL),Vec2::new(15*4,7*8)),
    Warp::new(Hitbox::new(3*8,9*8,8,8),Some(&SUPERMARKET),Vec2::new(14*8,5*8)),
    Warp::new(Hitbox::new(14*8,5*8,8,8),Some(&HOUSE_KITCHEN),Vec2::new(7*4,7*8)),
    ],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(12*8+2, 7*8, 3*8, 3*8),
            interaction: Interaction::Text(HOUSE_LIVING_ROOM_COUCH),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(15*8+2, 9*8-1, 2*8, 3*8),
            interaction: Interaction::Text(HOUSE_LIVING_ROOM_TV_1),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(5*8, 6*8, 2*8, 2*8),
            interaction: Interaction::Text(HOUSE_LIVING_ROOM_WINDOW),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(8*8, 6*8, 8, 8),
            interaction: Interaction::Text(CONSTRUCTION_2),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(12*8+9, 7*8, 8, 8),
            interaction: Interaction::Text(HOUSE_LIVING_ROOM_COUCH),
            sprite: Some(Animation {
                frames: &[
                    AnimFrame::new(Vec2::new(0, 0), 576, 30, SpriteOptions {w: 2, h: 3,
                        ..SpriteOptions::transparent_zero()}),
                    AnimFrame::new(Vec2::new(0, 0), 578, 30, SpriteOptions {w: 2, h: 3,
                        ..SpriteOptions::transparent_zero()}),
                ],
                ..Animation::const_default()
            }),
        },
    ],
    ..DEFAULT_MAP_SET
};
pub static HOUSE_KITCHEN: MapSet<'static> = MapSet {
    maps: &[
        MapOptions { //room
            x: 90,
            y: 0,
            w: 13,
            h: 10,
            ..DEFAULT_MAP
        },
        MapOptions { //microwave
            x: 37,
            y: 12,
            w: 2,
            h: 1,
            sx: 7*8+6,
            sy: 4*8-3,
            transparent: &[0],
            ..DEFAULT_MAP
        },
    ],
    warps: &[Warp::new(Hitbox::new(2*8,8*8+7,4*8,8),Some(&HOUSE_LIVING_ROOM),Vec2::new(14*8,5*8)),
    Warp::new(Hitbox::new(11*8,4*8,8,3*8),Some(&BACKYARD),Vec2::new(15*8,5*8))],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(2*8, 4*8, 2*8, 2*8),
            interaction: Interaction::Text(HOUSE_KITCHEN_CUPBOARD),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(5*8, 4*8, 5*4, 2*8),
            interaction: Interaction::Text(HOUSE_KITCHEN_SINK),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(16*4, 4*8, 2*8, 2*8),
            interaction: Interaction::Text(HOUSE_KITCHEN_MICROWAVE),
            sprite: None,
        },
    ],
    ..DEFAULT_MAP_SET
};

pub static BACKYARD: MapSet<'static> = MapSet {
    maps: &[
        MapOptions { //room
            x: 120,
            y: 0,
            ..DEFAULT_MAP
        },
    ],
    warps: &[Warp::new(Hitbox::new(15*8,5*8,8,8),Some(&HOUSE_KITCHEN),Vec2::new(10*8,5*8)),
             Warp::new(Hitbox::new(12*8,16*8+7,4*8,8),Some(&WILDERNESS),Vec2::new(7*8+3,60*8))],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(9*8, 5*8, 2*8, 2*8),
            interaction: Interaction::Text(HOUSE_BACKYARD_BASEMENT),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(20*8, 8*8, 1*8, 2*8),
            interaction: Interaction::Text(HOUSE_BACKYARD_SHED),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(21*8, 13*8, 1*8, 1*8),
            interaction: Interaction::Func(InteractFn::ToggleDog),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(5*8, 0, 1*8, 16*8),
            interaction: Interaction::Text(HOUSE_BACKYARD_STORMDRAIN),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(3, 2*8, 8, 8),
            interaction: Interaction::Text(DEFAULT),
            sprite: Some(Animation {
                frames: &[
                    AnimFrame::new(Vec2::new(0, 0), 646, 30, SpriteOptions::transparent_zero()),
                    AnimFrame::new(Vec2::new(0, 0), 647, 30, SpriteOptions::transparent_zero()),
                ],
                ..Animation::const_default()
            }),
        },
    ],
    ..DEFAULT_MAP_SET
};

pub static WILDERNESS: MapSet<'static> = MapSet {
    maps: &[
        MapOptions { //ground
            x: 120,
            y: 68,
            w: 30*4,
            h: 17*4,
            transparent: &[0],
            ..DEFAULT_MAP
        },
        MapOptions { //left barrier
            x: 120,
            y: 78,
            w: 1,
            h: 22,
            transparent: &[0],
            sx: -8,
            sy: 37*8,
            ..DEFAULT_MAP
        },
        MapOptions { //bottom barrier
            x: 120,
            y: 72,
            w: 23,
            h: 1,
            transparent: &[0],
            sx: 17*8,
            sy: 68*8,
            ..DEFAULT_MAP
        },
    ],
    fg_maps: &[
        MapOptions { //foreground
            x: 120,
            y: 0,
            w: 30*4,
            h: 17*4,
            transparent: &[0],
            ..DEFAULT_MAP
        }
    ],
    bg_colour: 3,
    warps: &[Warp::new(Hitbox::new(7*8,63*8+4,2*8,8),Some(&BACKYARD),Vec2::new(14*8,13*8))],
    interactables: &[],
    bank: 1,
    ..DEFAULT_MAP_SET
};

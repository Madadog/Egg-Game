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
use crate::camera::CameraBounds;
use crate::data::dialogue_data::*;
use crate::interact::InteractFn;
use crate::interact::{StaticInteractable, StaticInteraction};
use crate::map::LayerInfo;
use crate::map::StaticMapInfo;
use crate::map::Warp;
use crate::map::{Axis, WarpMode};
use crate::position::{Hitbox, Vec2};
use tic80_api::core::StaticSpriteOptions;

use super::sound;

pub(crate) const DEFAULT_MAP_SET: StaticMapInfo = StaticMapInfo {
    layers: &[],
    fg_layers: &[],
    warps: &[],
    interactables: &[],
    bg_colour: 0,
    music_track: None,
    bank: 0,
    camera_bounds: None,
};

#[derive(Debug, Clone, Copy)]
pub struct MapIndex(pub usize);
impl MapIndex {
    pub fn map(&self) -> StaticMapInfo<'static> {
        match self.0 {
            0 => SUPERMARKET,
            1 => SUPERMARKET_HALL,
            2 => SUPERMARKET_STOREROOM,
            3 => TEST_PEN,
            4 => BEDROOM,
            5 => HOUSE_STAIRWELL,
            6 => HOUSE_LIVING_ROOM,
            7 => HOUSE_KITCHEN,
            8 => BACKYARD,
            9 => WILDERNESS,
            10 => TOWN,
            11 => PIANO_ROOM,
            _ => SUPERMARKET,
        }
    }
    pub const SUPERMARKET: Self = MapIndex(0);
    pub const SUPERMARKET_HALL: Self = MapIndex(1);
    pub const SUPERMARKET_STOREROOM: Self = MapIndex(2);
    pub const TEST_PEN: Self = MapIndex(3);
    pub const BEDROOM: Self = MapIndex(4);
    pub const HOUSE_STAIRWELL: Self = MapIndex(5);
    pub const HOUSE_LIVING_ROOM: Self = MapIndex(6);
    pub const HOUSE_KITCHEN: Self = MapIndex(7);
    pub const BACKYARD: Self = MapIndex(8);
    pub const WILDERNESS: Self = MapIndex(9);
    pub const TOWN: Self = MapIndex(10);
    pub const PIANO_ROOM: Self = MapIndex(11);
}

pub const SUPERMARKET: StaticMapInfo<'static> = StaticMapInfo {
    layers: &[
        //bg
        LayerInfo::new(60, 17, 26, 12)
            .with_trans(&[0])
            .with_blit_rot_flags(4, 1, 0),
        //fruit stand
        LayerInfo::new(61, 29, 3, 2)
            .with_trans(&[0])
            .with_offset(2 * 8, 8 * 8),
        //vending machines
        LayerInfo::new(70, 29, 4, 5)
            .with_trans(&[0])
            .with_offset(19 * 8, 4 * 8),
        //counter
        LayerInfo::new(60, 31, 8, 3)
            .with_trans(&[0])
            .with_offset(5 * 8, 4 * 8),
        //top vending machine
        LayerInfo::new(68, 29, 2, 3)
            .with_trans(&[0])
            .with_offset(13 * 8, 5 * 4),
    ],
    warps: &[
        Warp::new_tile(17, 4, Some(MapIndex::SUPERMARKET_HALL), 9, 4).with_sound(sound::DOOR),
        Warp::new_tile(8, 4, Some(MapIndex::SUPERMARKET_HALL), 3, 4).with_sound(sound::DOOR),
        Warp::new(
            Hitbox::new(11 * 8, 11 * 8, 3 * 8, 8),
            Some(MapIndex::TOWN),
            Vec2::new(51 * 4, 15 * 8),
        )
        .with_sound(sound::DOOR)
        .with_mode(WarpMode::Auto),
    ],
    interactables: &[
        StaticInteractable {
            hitbox: Hitbox::new(13 * 8, 5 * 4, 8 * 2, 8 * 3),
            interaction: StaticInteraction::Text(SM_COIN_RETURN),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(2 * 8, 8 * 8, 8 * 3, 8 * 2),
            interaction: StaticInteraction::Text(SM_FRUIT_BASKET),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(4 * 8, 5 * 8, 8, 20),
            interaction: StaticInteraction::Text(SM_MAIN_WINDOW),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(19 * 8, 5 * 8, 8, 15),
            interaction: StaticInteraction::Text(SM_FRIDGE_1),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(20 * 8, 6 * 8, 8, 15),
            interaction: StaticInteraction::Text(SM_FRIDGE_2),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(21 * 8, 7 * 8, 8, 16),
            interaction: StaticInteraction::Text(SM_VENDING_MACHINE),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(11 * 8, 10 * 8, 3 * 8, 8),
            interaction: StaticInteraction::Text(CONSTRUCTION_1),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(80, 24, 16, 20),
            interaction: StaticInteraction::EnumText(THING),
            sprite: Some(&[
                StaticAnimFrame::new(
                    Vec2::splat(0),
                    661,
                    30,
                    StaticSpriteOptions {
                        w: 2,
                        h: 2,
                        ..StaticSpriteOptions::transparent_zero()
                    },
                )
                .with_palette_rotate(1),
                StaticAnimFrame::new(
                    Vec2::new(0, 1),
                    661,
                    30,
                    StaticSpriteOptions {
                        w: 2,
                        h: 2,
                        ..StaticSpriteOptions::transparent_zero()
                    },
                )
                .with_palette_rotate(1),
            ]),
        },
    ],
    bg_colour: 1,
    ..DEFAULT_MAP_SET
};

pub const SUPERMARKET_HALL: StaticMapInfo<'static> = StaticMapInfo {
    layers: &[
        //bg
        LayerInfo::new(86, 17, 13, 7)
            .with_trans(&[0])
            .with_blit_rot_flags(4, 1, 0),
        //closet
        LayerInfo::new(87, 24, 3, 4)
            .with_trans(&[0])
            .with_offset(5 * 8, 0),
        //diagonal door
        LayerInfo::new(86, 24, 1, 3)
            .with_trans(&[0])
            .with_offset(11 * 8, 2 * 8),
    ],
    warps: &[
        Warp::new_tile(9, 6, Some(MapIndex::SUPERMARKET), 17, 4)
            .with_mode(WarpMode::Auto)
            .with_sound(sound::DOOR),
        Warp::new_tile(3, 6, Some(MapIndex::SUPERMARKET), 8, 4)
            .with_mode(WarpMode::Auto)
            .with_sound(sound::DOOR),
        Warp::new_tile(4, 2, Some(MapIndex::SUPERMARKET_STOREROOM), 2, 3).with_sound(sound::DOOR),
    ],
    interactables: &[
        StaticInteractable {
            hitbox: Hitbox::new(11 * 8, 4 * 8, 8, 8),
            interaction: StaticInteraction::Text(EMERGENCY_EXIT),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(8 * 8, 3 * 8, 8, 8),
            interaction: StaticInteraction::Text(CONSTRUCTION_2),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(11 * 4, 0, 2 * 8, 7 * 4),
            interaction: StaticInteraction::Text(SM_HALL_SHELF),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(8, 3 * 8, 12, 16),
            interaction: StaticInteraction::Text(SM_HALL_WINDOW),
            sprite: None,
        },
    ],
    bg_colour: 1,
    ..DEFAULT_MAP_SET
};
pub const SUPERMARKET_STOREROOM: StaticMapInfo<'static> = StaticMapInfo {
    layers: &[
        LayerInfo::new(86, 28, 9, 6)
            .with_trans(&[0])
            .with_blit_rot_flags(4, 1, 0),
        LayerInfo::new(93, 24, 5, 4)
            .with_trans(&[0])
            .with_offset(2 * 8, 0),
    ],
    warps: &[Warp::new_tile(2, 5, Some(MapIndex::SUPERMARKET_HALL), 4, 2)
        .with_mode(WarpMode::Auto)
        .with_sound(sound::DOOR)],
    interactables: &[
        StaticInteractable {
            hitbox: Hitbox::new(53, 28, 8, 10),
            interaction: StaticInteraction::Text(EGG_1),
            sprite: Some(&[
                StaticAnimFrame::new(
                    Vec2::new(0, 0),
                    524,
                    30,
                    StaticSpriteOptions::transparent_zero(),
                ),
                StaticAnimFrame::new(
                    Vec2::new(0, -1),
                    524,
                    30,
                    StaticSpriteOptions::transparent_zero(),
                ),
            ]),
        },
        StaticInteractable {
            hitbox: Hitbox::new(16, 0, 5 * 8, 4 * 7),
            interaction: StaticInteraction::Text(SM_STOREROOM_SHELF),
            sprite: None,
        },
    ],
    bg_colour: 1,
    ..DEFAULT_MAP_SET
};

pub const TEST_PEN: StaticMapInfo<'static> = StaticMapInfo {
    layers: &[LayerInfo::new(53, 17, 7, 9).with_blit_rot_flags(0, 1, 0)],
    warps: &[Warp::new_tile(3, 8, Some(MapIndex::SUPERMARKET), 10, 4)],
    interactables: &[StaticInteractable {
        hitbox: Hitbox::new(5 * 8, 8, 8, 10),
        interaction: StaticInteraction::Text(EGG_1),
        sprite: Some(&[
            StaticAnimFrame::new(
                Vec2::new(0, 0),
                524,
                30,
                StaticSpriteOptions::transparent_zero(),
            ),
            StaticAnimFrame::new(
                Vec2::new(0, -1),
                524,
                30,
                StaticSpriteOptions::transparent_zero(),
            ),
        ]),
    }],
    bg_colour: 1,
    ..DEFAULT_MAP_SET
};

pub const BEDROOM: StaticMapInfo<'static> = StaticMapInfo {
    layers: &[
        //room
        LayerInfo::new(30, 0, 21, 10),
        //trolley
        LayerInfo::new(30, 10, 3, 2)
            .with_trans(&[0])
            .with_offset(101 - 16, 22),
        //mattress
        LayerInfo::new(37, 10, 3, 2)
            .with_trans(&[0])
            .with_offset(38, 27),
    ],
    warps: &[Warp::new(
        Hitbox::new(15 * 8, 6 * 8, 8, 8),
        Some(MapIndex::HOUSE_STAIRWELL),
        Vec2::new(1 * 8 + 1, 2 * 8),
    )
    .with_sound(sound::DOOR)],
    interactables: &[
        StaticInteractable {
            hitbox: Hitbox::new(38, 27, 3 * 8, 2 * 8),
            interaction: StaticInteraction::Text(BEDROOM_MATTRESS),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(2 * 8, 4 * 8, 2 * 8, 4 * 8),
            interaction: StaticInteraction::Text(BEDROOM_CLOSET),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(101 - 16, 22, 3 * 8, 2 * 8),
            interaction: StaticInteraction::Text(BEDROOM_TROLLEY),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(9 * 8, 3 * 8, 8, 8),
            interaction: StaticInteraction::EnumText(BEDROOM_WINDOW),
            sprite: None,
        },
    ],
    ..DEFAULT_MAP_SET
};

pub const HOUSE_STAIRWELL: StaticMapInfo<'static> = StaticMapInfo {
    layers: &[
        //room
        LayerInfo::new(51, 0, 16, 9),
        //left door
        LayerInfo::new(41, 10, 1, 3)
            .with_trans(&[0])
            .with_offset(0, 6),
        //right door
        LayerInfo::new(40, 10, 1, 3)
            .with_trans(&[0])
            .with_offset(120, 6),
    ],
    warps: &[
        Warp::new(
            Hitbox::new(1, 3 * 8, 8, 8),
            Some(MapIndex::BEDROOM),
            Vec2::new(14 * 8, 5 * 8),
        )
        .with_sound(sound::DOOR),
        Warp::new(
            Hitbox::new(7 * 8, 9 * 8, 2 * 8, 8),
            Some(MapIndex::HOUSE_LIVING_ROOM),
            Vec2::new(21 * 4, 4 * 8),
        )
        .with_sound(sound::STAIRS_DOWN)
        .with_mode(WarpMode::Auto),
    ],
    interactables: &[
        StaticInteractable {
            hitbox: Hitbox::new(2 * 8, 2 * 8, 8, 8),
            interaction: StaticInteraction::Func(InteractFn::StairwellWindow),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(7 * 8, 4 * 8, 2 * 8, 8),
            interaction: StaticInteraction::Func(InteractFn::StairwellPainting),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(13 * 8, 2 * 8, 8, 8),
            interaction: StaticInteraction::Text(HOUSE_STAIRWELL_WINDOW2),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(15 * 8, 3 * 8, 8, 8),
            interaction: StaticInteraction::Text(HOUSE_STAIRWELL_DOOR),
            sprite: None,
        },
    ],
    ..DEFAULT_MAP_SET
};

pub const HOUSE_LIVING_ROOM: StaticMapInfo<'static> = StaticMapInfo {
    layers: &[
        //room
        LayerInfo::new(67, 0, 23, 13),
        //couch
        LayerInfo::new(37, 14, 4, 2)
            .with_trans(&[0])
            .with_offset(12 * 8 + 2, 8 * 8),
        //tv
        LayerInfo::new(41, 15, 2, 1)
            .with_trans(&[0])
            .with_offset(15 * 8 + 2, 11 * 8 - 1),
    ],
    fg_layers: &[
        //tv
        LayerInfo::new(41, 13, 2, 3)
            .with_trans(&[0])
            .with_offset(15 * 8 + 2, 9 * 8 - 1),
    ],
    warps: &[
        Warp::new(
            Hitbox::new(10 * 8, 4 * 8, 2 * 8, 8),
            Some(MapIndex::HOUSE_STAIRWELL),
            Vec2::new(15 * 4, 7 * 8),
        )
        .with_sound(sound::STAIRS_UP)
        .with_mode(WarpMode::Auto),
        Warp::new(
            Hitbox::new(3 * 8, 9 * 8, 8, 8),
            Some(MapIndex::TOWN),
            Vec2::new(17 * 8, 13 * 8),
        )
        .with_sound(sound::DOOR)
        .with_flip(Axis::Y),
        Warp::new(
            Hitbox::new(14 * 8, 5 * 8, 8, 8),
            Some(MapIndex::HOUSE_KITCHEN),
            Vec2::new(7 * 4, 7 * 8),
        )
        .with_sound(sound::DOOR),
        Warp::new(
            Hitbox::new(8 * 8, 5 * 8, 8, 8),
            Some(MapIndex::PIANO_ROOM),
            Vec2::new(19 * 4, 6 * 8),
        )
        .with_sound(sound::DOOR),
    ],
    interactables: &[
        StaticInteractable {
            hitbox: Hitbox::new(12 * 8 + 2, 7 * 8, 3 * 8, 3 * 8),
            interaction: StaticInteraction::Text(HOUSE_LIVING_ROOM_COUCH),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(15 * 8 + 2, 11 * 8 - 1, 2 * 8, 2 * 8),
            interaction: StaticInteraction::Text(HOUSE_LIVING_ROOM_TV_1),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(5 * 8, 6 * 8, 2 * 8, 2 * 8),
            interaction: StaticInteraction::EnumText(HOUSE_LIVING_ROOM_WINDOW),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(12 * 8 + 2, 7 * 8, 1, 1),
            interaction: StaticInteraction::None,
            sprite: Some(&[StaticAnimFrame::new(
                Vec2::new(0, 0),
                35,
                30,
                StaticSpriteOptions {
                    w: 3,
                    h: 2,
                    ..StaticSpriteOptions::transparent_zero()
                },
            )
            .with_outline(None)]),
        },
        StaticInteractable {
            hitbox: Hitbox::new(12 * 8 + 9, 7 * 8, 8, 8),
            interaction: StaticInteraction::None,
            sprite: Some(&[
                StaticAnimFrame::new(
                    Vec2::new(0, 0),
                    576,
                    30,
                    StaticSpriteOptions {
                        w: 2,
                        h: 3,
                        ..StaticSpriteOptions::transparent_zero()
                    },
                ),
                StaticAnimFrame::new(
                    Vec2::new(0, 0),
                    578,
                    30,
                    StaticSpriteOptions {
                        w: 2,
                        h: 3,
                        ..StaticSpriteOptions::transparent_zero()
                    },
                ),
            ]),
        },
    ],
    ..DEFAULT_MAP_SET
};
pub const HOUSE_KITCHEN: StaticMapInfo<'static> = StaticMapInfo {
    layers: &[
        //room
        LayerInfo::new(90, 0, 13, 10),
        //microwave
        LayerInfo::new(37, 12, 2, 1)
            .with_offset(7 * 8 + 6, 4 * 8 - 3)
            .with_trans(&[0]),
    ],
    warps: &[
        Warp::new(
            Hitbox::new(2 * 8, 8 * 8 + 7, 4 * 8, 8),
            Some(MapIndex::HOUSE_LIVING_ROOM),
            Vec2::new(14 * 8, 5 * 8),
        )
        .with_sound(sound::DOOR)
        .with_mode(WarpMode::Auto),
        Warp::new(
            Hitbox::new(11 * 8, 4 * 8, 8, 3 * 8),
            Some(MapIndex::BACKYARD),
            Vec2::new(15 * 8, 5 * 8),
        )
        .with_sound(sound::DOOR),
    ],
    interactables: &[
        StaticInteractable {
            hitbox: Hitbox::new(2 * 8, 4 * 8, 2 * 8, 2 * 8),
            interaction: StaticInteraction::Text(HOUSE_KITCHEN_CUPBOARD),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(5 * 8, 4 * 8, 4 * 3 - 2, 2 * 8),
            interaction: StaticInteraction::EnumText(HOUSE_KITCHEN_SINK),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(16 * 4 - 2, 4 * 8, 2 * 8 + 2, 2 * 8),
            interaction: StaticInteraction::Text(HOUSE_KITCHEN_MICROWAVE),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(7 * 8, 4 * 8, 8, 2 * 8),
            interaction: StaticInteraction::Text(HOUSE_KITCHEN_WINDOW),
            sprite: None,
        },
    ],
    ..DEFAULT_MAP_SET
};

pub const BACKYARD: StaticMapInfo<'static> = StaticMapInfo {
    layers: &[
        //room
        LayerInfo::new(120, 0, 30, 17),
    ],
    warps: &[
        Warp::new(
            Hitbox::new(15 * 8, 5 * 8, 8, 8),
            Some(MapIndex::HOUSE_KITCHEN),
            Vec2::new(10 * 8 - 3, 5 * 8 + 3),
        )
        .with_sound(sound::DOOR)
        .with_flip(Axis::Y),
        Warp::new(
            Hitbox::new(12 * 8, 16 * 8 + 7, 4 * 8, 8),
            Some(MapIndex::WILDERNESS),
            Vec2::new(8 * 8, 61 * 8),
        )
        .with_mode(WarpMode::Auto)
        .with_flip(Axis::Y),
    ],
    interactables: &[
        StaticInteractable {
            hitbox: Hitbox::new(9 * 8, 5 * 8, 2 * 8, 2 * 8),
            interaction: StaticInteraction::Text(HOUSE_BACKYARD_BASEMENT),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(20 * 8, 8 * 8, 1 * 8, 2 * 8),
            interaction: StaticInteraction::Text(HOUSE_BACKYARD_SHED),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(22 * 8, 8 * 8, 1 * 8, 2 * 8),
            interaction: StaticInteraction::Text(HOUSE_BACKYARD_SHED_WINDOW),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(24 * 8, 10 * 8, 1 * 8, 6 * 8),
            interaction: StaticInteraction::Dialogue(HOUSE_BACKYARD_NEIGHBOURS),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(21 * 8, 13 * 8, 1 * 8, 1 * 8),
            interaction: StaticInteraction::Func(InteractFn::ToggleDog),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(5 * 8, 0, 1 * 8, 16 * 8),
            interaction: StaticInteraction::Text(HOUSE_BACKYARD_STORMDRAIN),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(3, 2 * 8, 8, 8),
            interaction: StaticInteraction::Text(DEFAULT),
            sprite: Some(&[
                StaticAnimFrame::new(
                    Vec2::new(0, 0),
                    646,
                    30,
                    StaticSpriteOptions::transparent_zero(),
                ),
                StaticAnimFrame::new(
                    Vec2::new(0, 0),
                    647,
                    30,
                    StaticSpriteOptions::transparent_zero(),
                ),
            ]),
        },
    ],
    ..DEFAULT_MAP_SET
};
// Somehow reduce code size...
// Reduce necessary tracked state
// Functionify
//TODO: Array2D images?
//TODO: Tiled map collisions
//TODO: Better mouse support
//TODO: Save support
//TODO: Add increment/decrement to menu UI
//TODO: Fix keyboard support
//TODO: Add test cases for game
//TODO: Make ellipses draw properly
//TODO: dialogue files
//TODO: Intro cutscene
//TODO: Remove unsafe code
//TODO: Egg lab
//TODO: Conditional dialogue
//TODO: Platformer, turn-based RPG, geometric puzzle, danmaku
//TODO: non-uniform pixels - 16 bit graphics - 3d
//TODO: Egg OS

//TODO: Creatures collide
//TODO: Chicken <-> egg loop
//TODO: Plot out game middles
//TODO: Soundtrack where relevent
//TODO: Finale

pub const WILDERNESS: StaticMapInfo<'static> = StaticMapInfo {
    layers: &[
        //ground
        LayerInfo::new(120, 68, 30 * 4, 17 * 4).with_trans(&[0]),
        //left barrier
        LayerInfo::new(120, 78, 1, 22)
            .with_trans(&[0])
            .with_offset(-8, 37 * 8),
        //bottom barrier
        LayerInfo::new(120, 72, 23, 1)
            .with_trans(&[0])
            .with_offset(17 * 8, 68 * 8),
    ],
    fg_layers: &[
        //foreground
        LayerInfo::new(120, 0, 30 * 4, 17 * 4).with_trans(&[0]),
    ],
    bg_colour: 3,
    warps: &[Warp::new(
        Hitbox::new(7 * 8, 63 * 8 + 4, 2 * 8, 8),
        Some(MapIndex::BACKYARD),
        Vec2::new(14 * 8 - 4, 15 * 8),
    )
    .with_mode(WarpMode::Auto)
    .with_flip(Axis::Y)],
    interactables: &[],
    bank: 1,
    ..DEFAULT_MAP_SET
};

pub const TOWN: StaticMapInfo<'static> = StaticMapInfo {
    layers: &[
        //ground
        LayerInfo::new(0, 0, 30 * 4, 17 * 4)
            .with_trans(&[0])
            .with_blit_rot_flags(5, 0, 0),
    ],
    fg_layers: &[
        //foreground
        LayerInfo::new(0, 68, 30 * 4, 17 * 4)
            .with_trans(&[0])
            .with_blit_rot_flags(5, 0, 0),
    ],
    bg_colour: 0,
    warps: &[
        Warp::new(
            Hitbox::new(17 * 8, 13 * 8, 8, 8),
            Some(MapIndex::HOUSE_LIVING_ROOM),
            Vec2::new(4 * 9, 8 * 8),
        )
        .with_sound(sound::DOOR),
        Warp::new(
            Hitbox::new(25 * 8, 15 * 8, 2 * 8, 8),
            Some(MapIndex::SUPERMARKET),
            Vec2::new(97, 73),
        )
        .with_sound(sound::DOOR),
    ],
    interactables: &[
        StaticInteractable {
            hitbox: Hitbox::new(8 * 6, 17 * 8, 1 * 8, 6 * 8),
            interaction: StaticInteraction::Text(TOWN_TRAFFIC),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(8 * 8, 17 * 8, 1 * 8, 1 * 8),
            interaction: StaticInteraction::Text(TOWN_LAMPPOST),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(14 * 8, 13 * 8, 8, 8),
            interaction: StaticInteraction::Text(TOWN_HOME_WINDOW),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(224, 142, 8 * 2, 8),
            interaction: StaticInteraction::EnumText(TOWN_WIDE),
            sprite: None,
        },
    ],
    bank: 1,
    ..DEFAULT_MAP_SET
};

pub const PIANO_ROOM: StaticMapInfo<'static> = StaticMapInfo {
    layers: &[LayerInfo::new(99, 15, 21, 10)],
    bg_colour: 0,
    warps: &[Warp::new(
        Hitbox::new(9 * 8, 9 * 8, 8 * 2, 8),
        Some(MapIndex::HOUSE_LIVING_ROOM),
        Vec2::new(8 * 8, 5 * 8),
    )
    .with_sound(sound::DOOR)
    .with_mode(WarpMode::Auto)],
    interactables: &[
        StaticInteractable {
            hitbox: Hitbox::new(4 * 8, 1 * 8, 4 * 25, 4 * 9),
            interaction: StaticInteraction::Func(InteractFn::Piano(Vec2::new(4 * 8, 1 * 8))),
            sprite: None,
        },
        StaticInteractable {
            hitbox: Hitbox::new(0, 6 * 8, 8 * 2, 8 * 1),
            interaction: StaticInteraction::Text(UNKNOWN_3),
            sprite: None,
        },
    ],
    camera_bounds: Some(CameraBounds::stick(21 * 8 / 2 - 120, -64)),
    ..DEFAULT_MAP_SET
};

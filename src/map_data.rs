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
use crate::dialogue_data::*;
use crate::interact::InteractFn;
use crate::interact::{Interactable, Interaction};
use crate::map::MapLayer;
use crate::map::MapSet;
use crate::map::Warp;
use crate::map::{Axis, WarpMode};
use crate::position::{Hitbox, Vec2};
use crate::SpriteOptions;

pub(crate) const DEFAULT_MAP_SET: MapSet = MapSet {
    maps: &[],
    fg_maps: &[],
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
    pub fn map(&self) -> MapSet<'static> {
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

pub const SUPERMARKET: MapSet<'static> = MapSet {
    maps: &[
        //bg
        MapLayer::new(60, 17, 26, 12)
            .with_trans(&[0])
            .with_blit_rot_flags(4, 1, 0),
        //fruit stand
        MapLayer::new(61, 29, 3, 2)
            .with_trans(&[0])
            .with_offset(2 * 8, 8 * 8),
        //vending machines
        MapLayer::new(70, 29, 4, 5)
            .with_trans(&[0])
            .with_offset(19 * 8, 4 * 8),
        //counter
        MapLayer::new(60, 31, 8, 3)
            .with_trans(&[0])
            .with_offset(5 * 8, 4 * 8),
        //top vending machine
        MapLayer::new(68, 29, 2, 3)
            .with_trans(&[0])
            .with_offset(13 * 8, 5 * 4),
    ],
    warps: &[
        Warp::new_tile(17, 4, Some(MapIndex::SUPERMARKET_HALL), 9, 4),
        Warp::new_tile(8, 4, Some(MapIndex::SUPERMARKET_HALL), 3, 4),
        Warp::new(
            Hitbox::new(11 * 8, 11 * 8, 3 * 8, 8),
            Some(MapIndex::TOWN),
            Vec2::new(51 * 4, 15 * 8),
        )
        .with_mode(WarpMode::Auto),
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
        Interactable {
            hitbox: Hitbox::new(80, 24, 16, 20),
            interaction: Interaction::EnumText(THING),
            sprite: Some(&[AnimFrame::new(
                Vec2::splat(0),
                661,
                30,
                SpriteOptions {
                    w: 2,
                    h: 2,
                    ..SpriteOptions::transparent_zero()
                },
            ).with_palette_rotate(1),
            AnimFrame::new(
                Vec2::new(0, 1),
                661,
                30,
                SpriteOptions {
                    w: 2,
                    h: 2,
                    ..SpriteOptions::transparent_zero()
                },
            ).with_palette_rotate(1)]),
        },
    ],
    bg_colour: 1,
    ..DEFAULT_MAP_SET
};

pub const SUPERMARKET_HALL: MapSet<'static> = MapSet {
    maps: &[
        //bg
        MapLayer::new(86, 17, 13, 7)
            .with_trans(&[0])
            .with_blit_rot_flags(4, 1, 0),
        //closet
        MapLayer::new(87, 24, 3, 4)
            .with_trans(&[0])
            .with_offset(5 * 8, 0),
        //diagonal door
        MapLayer::new(86, 24, 1, 3)
            .with_trans(&[0])
            .with_offset(11 * 8, 2 * 8),
    ],
    warps: &[
        Warp::new_tile(9, 6, Some(MapIndex::SUPERMARKET), 17, 4).with_mode(WarpMode::Auto),
        Warp::new_tile(3, 6, Some(MapIndex::SUPERMARKET), 8, 4).with_mode(WarpMode::Auto),
        Warp::new_tile(4, 2, Some(MapIndex::SUPERMARKET_STOREROOM), 2, 3),
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
    bg_colour: 1,
    ..DEFAULT_MAP_SET
};
pub const SUPERMARKET_STOREROOM: MapSet<'static> = MapSet {
    maps: &[
        MapLayer::new(86, 28, 9, 6)
            .with_trans(&[0])
            .with_blit_rot_flags(4, 1, 0),
        MapLayer::new(93, 24, 5, 4)
            .with_trans(&[0])
            .with_offset(2 * 8, 0),
    ],
    warps: &[Warp::new_tile(2, 5, Some(MapIndex::SUPERMARKET_HALL), 4, 2).with_mode(WarpMode::Auto)],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(53, 28, 8, 10),
            interaction: Interaction::Text(EGG_1),
            sprite: Some(&[
                AnimFrame::new(Vec2::new(0, 0), 524, 30, SpriteOptions::transparent_zero()),
                AnimFrame::new(Vec2::new(0, -1), 524, 30, SpriteOptions::transparent_zero()),
            ]),
        },
        Interactable {
            hitbox: Hitbox::new(16, 0, 5 * 8, 4 * 7),
            interaction: Interaction::Text(SM_STOREROOM_SHELF),
            sprite: None,
        },
    ],
    bg_colour: 1,
    ..DEFAULT_MAP_SET
};

pub const TEST_PEN: MapSet<'static> = MapSet {
    maps: &[MapLayer::new(53, 17, 7, 9).with_blit_rot_flags(0, 1, 0)],
    warps: &[Warp::new_tile(3, 8, Some(MapIndex::SUPERMARKET), 10, 4)],
    interactables: &[Interactable {
        hitbox: Hitbox::new(5 * 8, 8, 8, 10),
        interaction: Interaction::Text(EGG_1),
        sprite: Some(&[
            AnimFrame::new(Vec2::new(0, 0), 524, 30, SpriteOptions::transparent_zero()),
            AnimFrame::new(Vec2::new(0, -1), 524, 30, SpriteOptions::transparent_zero()),
        ]),
    }],
    bg_colour: 1,
    ..DEFAULT_MAP_SET
};

pub const BEDROOM: MapSet<'static> = MapSet {
    maps: &[
        //room
        MapLayer::new(30, 0, 21, 10),
        //trolley
        MapLayer::new(30, 10, 3, 2)
            .with_trans(&[0])
            .with_offset(101 - 16, 22),
        //mattress
        MapLayer::new(37, 10, 3, 2)
            .with_trans(&[0])
            .with_offset(38, 27),
    ],
    warps: &[Warp::new(
        Hitbox::new(15 * 8, 6 * 8, 8, 8),
        Some(MapIndex::HOUSE_STAIRWELL),
        Vec2::new(1 * 8 + 1, 2 * 8),
    )],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(38, 27, 3 * 8, 2 * 8),
            interaction: Interaction::Text(BEDROOM_MATTRESS),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(2 * 8, 4 * 8, 2 * 8, 4 * 8),
            interaction: Interaction::Text(BEDROOM_CLOSET),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(101 - 16, 22, 3 * 8, 2 * 8),
            interaction: Interaction::Text(BEDROOM_TROLLEY),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(9 * 8, 3 * 8, 8, 8),
            interaction: Interaction::EnumText(BEDROOM_WINDOW),
            sprite: None,
        },
    ],
    ..DEFAULT_MAP_SET
};

pub const HOUSE_STAIRWELL: MapSet<'static> = MapSet {
    maps: &[
        //room
        MapLayer::new(51, 0, 16, 9),
        //left door
        MapLayer::new(41, 10, 1, 3)
            .with_trans(&[0])
            .with_offset(0, 6),
        //right door
        MapLayer::new(40, 10, 1, 3)
            .with_trans(&[0])
            .with_offset(120, 6),
    ],
    warps: &[
        Warp::new(
            Hitbox::new(1, 3 * 8, 8, 8),
            Some(MapIndex::BEDROOM),
            Vec2::new(14 * 8, 5 * 8),
        ),
        Warp::new(
            Hitbox::new(7 * 8, 9 * 8, 2 * 8, 8),
            Some(MapIndex::HOUSE_LIVING_ROOM),
            Vec2::new(21 * 4, 4 * 8),
        )
        .with_mode(WarpMode::Auto),
    ],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(2 * 8, 2 * 8, 8, 8),
            interaction: Interaction::Func(InteractFn::StairwellWindow),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(7 * 8, 4 * 8, 2 * 8, 8),
            interaction: Interaction::Func(InteractFn::StairwellPainting),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(13 * 8, 2 * 8, 8, 8),
            interaction: Interaction::Text(HOUSE_STAIRWELL_WINDOW2),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(15 * 8, 3 * 8, 8, 8),
            interaction: Interaction::Text(HOUSE_STAIRWELL_DOOR),
            sprite: None,
        },
    ],
    ..DEFAULT_MAP_SET
};

pub const HOUSE_LIVING_ROOM: MapSet<'static> = MapSet {
    maps: &[
        //room
        MapLayer::new(67, 0, 23, 13),
        //couch
        MapLayer::new(37, 14, 4, 2)
            .with_trans(&[0])
            .with_offset(12 * 8 + 2, 8 * 8),
        //tv
        MapLayer::new(41, 15, 2, 1)
            .with_trans(&[0])
            .with_offset(15 * 8 + 2, 11 * 8 - 1),
    ],
    fg_maps: &[
        //tv
        MapLayer::new(41, 13, 2, 3)
            .with_trans(&[0])
            .with_offset(15 * 8 + 2, 9 * 8 - 1),
    ],
    warps: &[
        Warp::new(
            Hitbox::new(10 * 8, 4 * 8, 2 * 8, 8),
            Some(MapIndex::HOUSE_STAIRWELL),
            Vec2::new(15 * 4, 7 * 8),
        )
        .with_mode(WarpMode::Auto),
        Warp::new(
            Hitbox::new(3 * 8, 9 * 8, 8, 8),
            Some(MapIndex::TOWN),
            Vec2::new(17 * 8, 13 * 8),
        )
        .with_flip(Axis::Y),
        Warp::new(
            Hitbox::new(14 * 8, 5 * 8, 8, 8),
            Some(MapIndex::HOUSE_KITCHEN),
            Vec2::new(7 * 4, 7 * 8),
        ),
        Warp::new(
            Hitbox::new(8 * 8, 5 * 8, 8, 8),
            Some(MapIndex::PIANO_ROOM),
            Vec2::new(19 * 4, 6 * 8),
        ),
    ],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(12 * 8 + 2, 7 * 8, 3 * 8, 3 * 8),
            interaction: Interaction::Text(HOUSE_LIVING_ROOM_COUCH),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(15 * 8 + 2, 11 * 8 - 1, 2 * 8, 2 * 8),
            interaction: Interaction::Text(HOUSE_LIVING_ROOM_TV_1),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(5 * 8, 6 * 8, 2 * 8, 2 * 8),
            interaction: Interaction::EnumText(HOUSE_LIVING_ROOM_WINDOW),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(12 * 8 + 2, 7 * 8, 1, 1),
            interaction: Interaction::None,
            sprite: Some(&[AnimFrame::new(
                Vec2::new(0, 0),
                35,
                30,
                SpriteOptions {
                    w: 3,
                    h: 2,
                    ..SpriteOptions::transparent_zero()
                },
            )
            .with_outline(None)]),
        },
        Interactable {
            hitbox: Hitbox::new(12 * 8 + 9, 7 * 8, 8, 8),
            interaction: Interaction::None,
            sprite: Some(&[
                AnimFrame::new(
                    Vec2::new(0, 0),
                    576,
                    30,
                    SpriteOptions {
                        w: 2,
                        h: 3,
                        ..SpriteOptions::transparent_zero()
                    },
                ),
                AnimFrame::new(
                    Vec2::new(0, 0),
                    578,
                    30,
                    SpriteOptions {
                        w: 2,
                        h: 3,
                        ..SpriteOptions::transparent_zero()
                    },
                ),
            ]),
        },
    ],
    ..DEFAULT_MAP_SET
};
pub const HOUSE_KITCHEN: MapSet<'static> = MapSet {
    maps: &[
        //room
        MapLayer::new(90, 0, 13, 10),
        //microwave
        MapLayer::new(37, 12, 2, 1)
            .with_offset(7 * 8 + 6, 4 * 8 - 3)
            .with_trans(&[0]),
    ],
    warps: &[
        Warp::new(
            Hitbox::new(2 * 8, 8 * 8 + 7, 4 * 8, 8),
            Some(MapIndex::HOUSE_LIVING_ROOM),
            Vec2::new(14 * 8, 5 * 8),
        )
        .with_mode(WarpMode::Auto),
        Warp::new(
            Hitbox::new(11 * 8, 4 * 8, 8, 3 * 8),
            Some(MapIndex::BACKYARD),
            Vec2::new(15 * 8, 5 * 8),
        ),
    ],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(2 * 8, 4 * 8, 2 * 8, 2 * 8),
            interaction: Interaction::Text(HOUSE_KITCHEN_CUPBOARD),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(5 * 8, 4 * 8, 4 * 3 - 2, 2 * 8),
            interaction: Interaction::EnumText(HOUSE_KITCHEN_SINK),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(16 * 4 - 2, 4 * 8, 2 * 8 + 2, 2 * 8),
            interaction: Interaction::Text(HOUSE_KITCHEN_MICROWAVE),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(7 * 8, 4 * 8, 8, 2 * 8),
            interaction: Interaction::Text(HOUSE_KITCHEN_WINDOW),
            sprite: None,
        },
    ],
    ..DEFAULT_MAP_SET
};

pub const BACKYARD: MapSet<'static> = MapSet {
    maps: &[
        //room
        MapLayer::new(120, 0, 30, 17),
    ],
    warps: &[
        Warp::new(
            Hitbox::new(15 * 8, 5 * 8, 8, 8),
            Some(MapIndex::HOUSE_KITCHEN),
            Vec2::new(10 * 8 - 3, 5 * 8 + 3),
        )
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
        Interactable {
            hitbox: Hitbox::new(9 * 8, 5 * 8, 2 * 8, 2 * 8),
            interaction: Interaction::Text(HOUSE_BACKYARD_BASEMENT),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(20 * 8, 8 * 8, 1 * 8, 2 * 8),
            interaction: Interaction::Text(HOUSE_BACKYARD_SHED),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(22 * 8, 8 * 8, 1 * 8, 2 * 8),
            interaction: Interaction::Text(HOUSE_BACKYARD_SHED_WINDOW),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(24 * 8, 10 * 8, 1 * 8, 6 * 8),
            interaction: Interaction::Dialogue(HOUSE_BACKYARD_NEIGHBOURS),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(21 * 8, 13 * 8, 1 * 8, 1 * 8),
            interaction: Interaction::Func(InteractFn::ToggleDog),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(5 * 8, 0, 1 * 8, 16 * 8),
            interaction: Interaction::Text(HOUSE_BACKYARD_STORMDRAIN),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(3, 2 * 8, 8, 8),
            interaction: Interaction::Text(DEFAULT),
            sprite: Some(&[
                AnimFrame::new(Vec2::new(0, 0), 646, 30, SpriteOptions::transparent_zero()),
                AnimFrame::new(Vec2::new(0, 0), 647, 30, SpriteOptions::transparent_zero()),
            ]),
        },
    ],
    ..DEFAULT_MAP_SET
};
//TODO: Pet the dog.
//TODO: Somehow reduce code size...
// Reduce necessary tracked state
// Functionify
// Zip strings if necessary...
// serialize, zip, embed, unzip, deserialize...
//TODO: Make intro animation
//TODO: Creatures walk
//TODO: Chicken <-> egg loop

pub const WILDERNESS: MapSet<'static> = MapSet {
    maps: &[
        //ground
        MapLayer::new(120, 68, 30 * 4, 17 * 4).with_trans(&[0]),
        //left barrier
        MapLayer::new(120, 78, 1, 22)
            .with_trans(&[0])
            .with_offset(-8, 37 * 8),
        //bottom barrier
        MapLayer::new(120, 72, 23, 1)
            .with_trans(&[0])
            .with_offset(17 * 8, 68 * 8),
    ],
    fg_maps: &[
        //foreground
        MapLayer::new(120, 0, 30 * 4, 17 * 4).with_trans(&[0]),
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

pub const TOWN: MapSet<'static> = MapSet {
    maps: &[
        //ground
        MapLayer::new(0, 0, 30 * 4, 17 * 4)
            .with_trans(&[0])
            .with_blit_rot_flags(5, 0, 1),
    ],
    fg_maps: &[
        //foreground
        MapLayer::new(0, 68, 30 * 4, 17 * 4)
            .with_trans(&[0])
            .with_blit_rot_flags(5, 0, 0),
    ],
    bg_colour: 0,
    warps: &[
        Warp::new(
            Hitbox::new(17 * 8, 13 * 8, 8, 8),
            Some(MapIndex::HOUSE_LIVING_ROOM),
            Vec2::new(4 * 9, 8 * 8),
        ),
        Warp::new(
            Hitbox::new(25 * 8, 15 * 8, 2 * 8, 8),
            Some(MapIndex::SUPERMARKET),
            Vec2::new(97, 73),
        ),
    ],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(8 * 6, 17 * 8, 1 * 8, 6 * 8),
            interaction: Interaction::Text(TOWN_TRAFFIC),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(8 * 8, 17 * 8, 1 * 8, 1 * 8),
            interaction: Interaction::Text(TOWN_LAMPPOST),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(14 * 8, 13 * 8, 8, 8),
            interaction: Interaction::Text(TOWN_HOME_WINDOW),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(224, 142, 8 * 2, 8),
            interaction: Interaction::EnumText(TOWN_WIDE),
            sprite: None,
        },
    ],
    bank: 1,
    ..DEFAULT_MAP_SET
};

pub const PIANO_ROOM: MapSet<'static> = MapSet {
    maps: &[MapLayer::new(99, 15, 21, 10)],
    bg_colour: 0,
    warps: &[Warp::new(
        Hitbox::new(9 * 8, 9 * 8, 8 * 2, 8),
        Some(MapIndex::HOUSE_LIVING_ROOM),
        Vec2::new(8 * 8, 5 * 8),
    )
    .with_mode(WarpMode::Auto)],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(4 * 8, 1 * 8, 4 * 25, 4 * 9),
            interaction: Interaction::Func(InteractFn::Piano(Vec2::new(4 * 8, 1 * 8))),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(0, 6 * 8, 8 * 2, 8 * 1),
            interaction: Interaction::Text(UNKNOWN_3),
            sprite: None,
        },
    ],
    camera_bounds: Some(CameraBounds::stick(21 * 8 / 2 - 120, -64)),
    ..DEFAULT_MAP_SET
};

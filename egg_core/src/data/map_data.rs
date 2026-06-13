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

use crate::animation::AnimFrame;
use crate::camera::CameraBounds;
use crate::interact::{InteractFn, Interaction};
use crate::map::{Axis, LayerInfo, MapInfo, MapObject, ObjectEffect, Warp, WarpMode};
use crate::position::{Hitbox, Vec2};
use crate::system::SpriteOptions;

use super::sound;

/// The 12 legacy maps' names, indexed by their historical [`MapIndex`].
/// Snake_case, matching the builder fns below — these are the canonical map
/// identities now; the numbers survive only for migration.
const LEGACY_NAMES: [&str; 12] = [
    "supermarket",
    "supermarket_hall",
    "supermarket_storeroom",
    "test_pen",
    "bedroom",
    "house_stairwell",
    "house_living_room",
    "house_kitchen",
    "backyard",
    "wilderness",
    "town",
    "piano_room",
];

/// Migration shim: the numeric id maps used to be addressed by. Kept only so
/// old numeric saves and numeric `to_map` properties in existing `.tmj` files
/// can be translated to names (see `map_by_name`); new code addresses maps by
/// name.
#[derive(Debug, Clone, Copy)]
pub struct MapIndex(pub usize);
impl MapIndex {
    /// Migration shim alongside [`MapIndex`] itself; prefer [`legacy_map`].
    pub fn map(&self) -> MapInfo {
        legacy_map(self.name()).unwrap_or_else(supermarket)
    }
    /// The legacy map's name. Out-of-range indices fall back to the
    /// supermarket, mirroring what [`map`](Self::map) always loaded for them.
    pub fn name(self) -> &'static str {
        LEGACY_NAMES.get(self.0).copied().unwrap_or(LEGACY_NAMES[0])
    }
}

/// Migration shim alongside [`MapIndex::name`]: the numeric id for a legacy
/// map name, so saves keep populating the numeric `current_map` field old
/// binaries read. `None` for modern (named-only) maps.
pub fn legacy_index(name: &str) -> Option<MapIndex> {
    LEGACY_NAMES.iter().position(|n| *n == name).map(MapIndex)
}

/// Load metadata for one of the 12 hardcoded legacy maps, or `None` if `name`
/// isn't one of them.
pub fn legacy_map(name: &str) -> Option<MapInfo> {
    let builder = match name {
        "supermarket" => supermarket,
        "supermarket_hall" => supermarket_hall,
        "supermarket_storeroom" => supermarket_storeroom,
        "test_pen" => test_pen,
        "bedroom" => bedroom,
        "house_stairwell" => house_stairwell,
        "house_living_room" => house_living_room,
        "house_kitchen" => house_kitchen,
        "backyard" => backyard,
        "wilderness" => wilderness,
        "town" => town,
        "piano_room" => piano_room,
        _ => return None,
    };
    Some(builder())
}

/// A two-frame bobbing sprite (`spr_id` at `y` and `y + 1`), as used by several
/// egg / object interactables. `transparent_zero` matches the originals.
fn bob(spr_id: u16) -> Vec<AnimFrame> {
    vec![
        AnimFrame::new(Vec2::new(0, 0), spr_id, 30, SpriteOptions::transparent_zero()),
        AnimFrame::new(Vec2::new(0, -1), spr_id, 30, SpriteOptions::transparent_zero()),
    ]
}

fn supermarket() -> MapInfo {
    MapInfo {
        layers: vec![
            //bg
            LayerInfo::new(60, 17, 26, 12)
                .with_trans(&[0])
                .with_rot_and_shift_flags(1, 0),
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
        objects: vec![
            MapObject::warp_tile(17, 4, Some("supermarket_hall"), 9, 4).with_warp_sound(sound::DOOR),
            MapObject::warp_tile(8, 4, Some("supermarket_hall"), 3, 4).with_warp_sound(sound::DOOR),
            MapObject::warp(
                Hitbox::new(11 * 8, 11 * 8, 3 * 8, 8),
                Warp::new(Some("town"), Vec2::new(51 * 4, 15 * 8))
                    .with_sound(sound::DOOR)
                    .with_mode(WarpMode::Auto),
            ),
            MapObject::dialogue(Hitbox::new(13 * 8, 5 * 4, 8 * 2, 8 * 3), "sm_coin_return"),
            MapObject::dialogue(Hitbox::new(2 * 8, 8 * 8, 8 * 3, 8 * 2), "sm_fruit_basket"),
            MapObject::dialogue(Hitbox::new(4 * 8, 5 * 8, 8, 20), "sm_main_window"),
            MapObject::dialogue(Hitbox::new(19 * 8, 5 * 8, 8, 15), "sm_fridge_1"),
            MapObject::dialogue(Hitbox::new(20 * 8, 6 * 8, 8, 15), "sm_fridge_2"),
            MapObject::dialogue(Hitbox::new(21 * 8, 7 * 8, 8, 16), "sm_vending_machine"),
            MapObject::dialogue(Hitbox::new(11 * 8, 10 * 8, 3 * 8, 8), "construction_1"),
            MapObject::dialogue(Hitbox::new(80, 24, 16, 20), "thing").with_sprite(vec![
                AnimFrame::new(
                    Vec2::splat(0),
                    661,
                    30,
                    SpriteOptions {
                        w: 2,
                        h: 2,
                        ..SpriteOptions::transparent_zero()
                    },
                )
                .with_palette_rotate(1),
                AnimFrame::new(
                    Vec2::new(0, 1),
                    661,
                    30,
                    SpriteOptions {
                        w: 2,
                        h: 2,
                        ..SpriteOptions::transparent_zero()
                    },
                )
                .with_palette_rotate(1),
            ]),
        ],
        bg_colour: 1,
        source: "bank1".to_string(),
        ..MapInfo::default()
    }
}

fn supermarket_hall() -> MapInfo {
    MapInfo {
        layers: vec![
            //bg
            LayerInfo::new(86, 17, 13, 7)
                .with_trans(&[0])
                .with_rot_and_shift_flags(1, 0),
            //closet
            LayerInfo::new(87, 24, 3, 4)
                .with_trans(&[0])
                .with_offset(5 * 8, 0),
            //diagonal door
            LayerInfo::new(86, 24, 1, 3)
                .with_trans(&[0])
                .with_offset(11 * 8, 2 * 8),
        ],
        objects: vec![
            MapObject::warp_tile(9, 6, Some("supermarket"), 17, 4)
                .with_warp_mode(WarpMode::Auto)
                .with_warp_sound(sound::DOOR),
            MapObject::warp_tile(3, 6, Some("supermarket"), 8, 4)
                .with_warp_mode(WarpMode::Auto)
                .with_warp_sound(sound::DOOR),
            MapObject::warp_tile(4, 2, Some("supermarket_storeroom"), 2, 3).with_warp_sound(sound::DOOR),
            MapObject::dialogue(Hitbox::new(11 * 8, 4 * 8, 8, 8), "emergency_exit"),
            MapObject::dialogue(Hitbox::new(8 * 8, 3 * 8, 8, 8), "construction_2"),
            MapObject::dialogue(Hitbox::new(11 * 4, 0, 2 * 8, 7 * 4), "sm_hall_shelf"),
            MapObject::dialogue(Hitbox::new(8, 3 * 8, 12, 16), "sm_hall_window"),
        ],
        bg_colour: 1,
        source: "bank1".to_string(),
        ..MapInfo::default()
    }
}

fn supermarket_storeroom() -> MapInfo {
    MapInfo {
        layers: vec![
            LayerInfo::new(86, 28, 9, 6)
                .with_trans(&[0])
                .with_rot_and_shift_flags(1, 0),
            LayerInfo::new(93, 24, 5, 4)
                .with_trans(&[0])
                .with_offset(2 * 8, 0),
        ],
        objects: vec![
            MapObject::warp_tile(2, 5, Some("supermarket_hall"), 4, 2)
                .with_warp_mode(WarpMode::Auto)
                .with_warp_sound(sound::DOOR),
            MapObject::dialogue(Hitbox::new(53, 28, 8, 10), "egg_1").with_sprite(bob(524)),
            MapObject::dialogue(Hitbox::new(16, 0, 5 * 8, 4 * 7), "sm_storeroom_shelf"),
        ],
        bg_colour: 1,
        source: "bank1".to_string(),
        ..MapInfo::default()
    }
}

fn test_pen() -> MapInfo {
    MapInfo {
        layers: vec![LayerInfo::new(53, 17, 7, 9).with_rot_and_shift_flags(1, 0)],
        objects: vec![
            MapObject::warp_tile(3, 8, Some("supermarket"), 10, 4),
            MapObject::dialogue(Hitbox::new(5 * 8, 8, 8, 10), "egg_1").with_sprite(bob(524)),
        ],
        bg_colour: 1,
        source: "bank1".to_string(),
        ..MapInfo::default()
    }
}

fn bedroom() -> MapInfo {
    MapInfo {
        layers: vec![
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
        objects: vec![
            MapObject::warp(
                Hitbox::new(15 * 8, 6 * 8, 8, 8),
                Warp::new(Some("house_stairwell"), Vec2::new(8 + 1, 2 * 8)).with_sound(sound::DOOR),
            ),
            MapObject::dialogue(Hitbox::new(38, 27, 3 * 8, 2 * 8), "bedroom_mattress"),
            MapObject::dialogue(Hitbox::new(2 * 8, 4 * 8, 2 * 8, 4 * 8), "bedroom_closet"),
            MapObject::dialogue(Hitbox::new(101 - 16, 22, 3 * 8, 2 * 8), "bedroom_trolley"),
            MapObject::dialogue(Hitbox::new(9 * 8, 3 * 8, 8, 8), "bedroom_window"),
        ],
        source: "bank1".to_string(),
        ..MapInfo::default()
    }
}

fn house_stairwell() -> MapInfo {
    MapInfo {
        layers: vec![
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
        objects: vec![
            MapObject::warp(
                Hitbox::new(1, 3 * 8, 8, 8),
                Warp::new(Some("bedroom"), Vec2::new(14 * 8, 5 * 8)).with_sound(sound::DOOR),
            ),
            MapObject::warp(
                Hitbox::new(7 * 8, 9 * 8, 2 * 8, 8),
                Warp::new(Some("house_living_room"), Vec2::new(21 * 4, 4 * 8))
                    .with_sound(sound::STAIRS_DOWN)
                    .with_mode(WarpMode::Auto),
            ),
            // The window sets the `house_stairwell_window_interacted` flag via a
            // `#set` in its dialogue; the painting branches on it with `#if`
            // (see assets/script/en.eggtext) — no bespoke InteractFn needed.
            MapObject::dialogue(Hitbox::new(2 * 8, 2 * 8, 8, 8), "house_stairwell_window"),
            MapObject::dialogue(Hitbox::new(7 * 8, 4 * 8, 2 * 8, 8), "house_stairwell_painting"),
            MapObject::dialogue(Hitbox::new(13 * 8, 2 * 8, 8, 8), "house_stairwell_window2"),
            MapObject::dialogue(Hitbox::new(15 * 8, 3 * 8, 8, 8), "house_stairwell_door"),
        ],
        source: "bank1".to_string(),
        ..MapInfo::default()
    }
}

fn house_living_room() -> MapInfo {
    MapInfo {
        layers: vec![
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
        fg_layers: vec![
            //tv
            LayerInfo::new(41, 13, 2, 3)
                .with_trans(&[0])
                .with_offset(15 * 8 + 2, 9 * 8 - 1),
        ],
        objects: vec![
            MapObject::warp(
                Hitbox::new(10 * 8, 4 * 8, 2 * 8, 8),
                Warp::new(Some("house_stairwell"), Vec2::new(15 * 4, 7 * 8))
                    .with_sound(sound::STAIRS_UP)
                    .with_mode(WarpMode::Auto),
            ),
            MapObject::warp(
                Hitbox::new(3 * 8, 9 * 8, 8, 8),
                Warp::new(Some("town"), Vec2::new(17 * 8, 13 * 8))
                    .with_sound(sound::DOOR)
                    .with_flip(Axis::Y),
            ),
            MapObject::warp(
                Hitbox::new(14 * 8, 5 * 8, 8, 8),
                Warp::new(Some("house_kitchen"), Vec2::new(7 * 4, 7 * 8)).with_sound(sound::DOOR),
            ),
            MapObject::warp(
                Hitbox::new(8 * 8, 5 * 8, 8, 8),
                Warp::new(Some("piano_room"), Vec2::new(19 * 4, 6 * 8)).with_sound(sound::DOOR),
            ),
            MapObject::dialogue(
                Hitbox::new(12 * 8 + 2, 7 * 8, 3 * 8, 3 * 8),
                "house_living_room_couch",
            ),
            MapObject::dialogue(
                Hitbox::new(15 * 8 + 2, 11 * 8 - 1, 2 * 8, 2 * 8),
                "house_living_room_tv_1",
            ),
            MapObject::dialogue(
                Hitbox::new(5 * 8, 6 * 8, 2 * 8, 2 * 8),
                "house_living_room_window",
            ),
            MapObject::new(
                Hitbox::new(12 * 8 + 2, 7 * 8, 1, 1),
                ObjectEffect::Interact(Interaction::None),
                Some(vec![AnimFrame::new(
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
            ),
            MapObject::new(
                Hitbox::new(12 * 8 + 9, 7 * 8, 8, 8),
                ObjectEffect::Interact(Interaction::None),
                Some(vec![
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
            ),
        ],
        source: "bank1".to_string(),
        ..MapInfo::default()
    }
}

fn house_kitchen() -> MapInfo {
    MapInfo {
        layers: vec![
            //room
            LayerInfo::new(90, 0, 13, 10),
            //microwave
            LayerInfo::new(37, 12, 2, 1)
                .with_offset(7 * 8 + 6, 4 * 8 - 3)
                .with_trans(&[0]),
        ],
        objects: vec![
            MapObject::warp(
                Hitbox::new(2 * 8, 8 * 8 + 7, 4 * 8, 8),
                Warp::new(Some("house_living_room"), Vec2::new(14 * 8, 5 * 8))
                    .with_sound(sound::DOOR)
                    .with_mode(WarpMode::Auto),
            ),
            MapObject::warp(
                Hitbox::new(11 * 8, 4 * 8, 8, 3 * 8),
                Warp::new(Some("backyard"), Vec2::new(15 * 8, 5 * 8)).with_sound(sound::DOOR),
            ),
            MapObject::dialogue(Hitbox::new(2 * 8, 4 * 8, 2 * 8, 2 * 8), "house_kitchen_cupboard"),
            MapObject::dialogue(Hitbox::new(5 * 8, 4 * 8, 4 * 3 - 2, 2 * 8), "house_kitchen_sink"),
            MapObject::dialogue(
                Hitbox::new(16 * 4 - 2, 4 * 8, 2 * 8 + 2, 2 * 8),
                "house_kitchen_microwave",
            ),
            MapObject::dialogue(Hitbox::new(7 * 8, 4 * 8, 8, 2 * 8), "house_kitchen_window"),
        ],
        source: "bank1".to_string(),
        ..MapInfo::default()
    }
}

fn backyard() -> MapInfo {
    MapInfo {
        layers: vec![
            //room
            LayerInfo::new(120, 0, 30, 17),
        ],
        objects: vec![
            MapObject::warp(
                Hitbox::new(15 * 8, 5 * 8, 8, 8),
                Warp::new(Some("house_kitchen"), Vec2::new(10 * 8 - 3, 5 * 8 + 3))
                    .with_sound(sound::DOOR)
                    .with_flip(Axis::Y),
            ),
            MapObject::warp(
                Hitbox::new(12 * 8, 16 * 8 + 7, 4 * 8, 8),
                Warp::new(Some("wilderness"), Vec2::new(8 * 8, 61 * 8))
                    .with_mode(WarpMode::Auto)
                    .with_flip(Axis::Y),
            ),
            MapObject::dialogue(Hitbox::new(9 * 8, 5 * 8, 2 * 8, 2 * 8), "house_backyard_basement"),
            MapObject::dialogue(Hitbox::new(20 * 8, 8 * 8, 8, 2 * 8), "house_backyard_shed"),
            MapObject::dialogue(
                Hitbox::new(22 * 8, 8 * 8, 8, 2 * 8),
                "house_backyard_shed_window",
            ),
            MapObject::dialogue(
                Hitbox::new(24 * 8, 10 * 8, 8, 6 * 8),
                "house_backyard_neighbours",
            ),
            MapObject::func(Hitbox::new(21 * 8, 13 * 8, 8, 8), InteractFn::ToggleDog),
            MapObject::dialogue(Hitbox::new(5 * 8, 0, 8, 16 * 8), "house_backyard_stormdrain"),
            MapObject::dialogue(Hitbox::new(3, 2 * 8, 8, 8), "default").with_sprite(vec![
                AnimFrame::new(Vec2::new(0, 0), 646, 30, SpriteOptions::transparent_zero()),
                AnimFrame::new(Vec2::new(0, 0), 647, 30, SpriteOptions::transparent_zero()),
            ]),
        ],
        source: "bank1".to_string(),
        ..MapInfo::default()
    }
}

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

fn wilderness() -> MapInfo {
    MapInfo {
        layers: vec![
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
        fg_layers: vec![
            //foreground
            LayerInfo::new(120, 0, 30 * 4, 17 * 4).with_trans(&[0]),
        ],
        bg_colour: 3,
        objects: vec![
            MapObject::warp(
                Hitbox::new(7 * 8, 63 * 8 + 4, 2 * 8, 8),
                Warp::new(Some("backyard"), Vec2::new(14 * 8 - 4, 15 * 8))
                    .with_mode(WarpMode::Auto)
                    .with_flip(Axis::Y),
            ),
        ],
        source: "bank2".to_string(),
        ..MapInfo::default()
    }
}

fn town() -> MapInfo {
    MapInfo {
        layers: vec![
            //ground
            LayerInfo::new(0, 0, 30 * 4, 17 * 4)
                .with_trans(&[0])
                .with_rot_and_shift_flags(0, 0),
        ],
        fg_layers: vec![
            //foreground
            LayerInfo::new(0, 68, 30 * 4, 17 * 4)
                .with_trans(&[0])
                .with_rot_and_shift_flags(0, 0),
        ],
        bg_colour: 0,
        objects: vec![
            MapObject::warp(
                Hitbox::new(17 * 8, 13 * 8, 8, 8),
                Warp::new(Some("house_living_room"), Vec2::new(4 * 9, 8 * 8)).with_sound(sound::DOOR),
            ),
            MapObject::warp(
                Hitbox::new(25 * 8, 15 * 8, 2 * 8, 8),
                Warp::new(Some("supermarket"), Vec2::new(97, 73)).with_sound(sound::DOOR),
            ),
            MapObject::dialogue(Hitbox::new(8 * 6, 17 * 8, 8, 6 * 8), "town_traffic"),
            MapObject::dialogue(Hitbox::new(8 * 8, 17 * 8, 8, 8), "town_lamppost"),
            MapObject::dialogue(Hitbox::new(14 * 8, 13 * 8, 8, 8), "town_home_window"),
            MapObject::dialogue(Hitbox::new(224, 142, 8 * 2, 8), "town_wide"),
        ],
        source: "bank2".to_string(),
        ..MapInfo::default()
    }
}

fn piano_room() -> MapInfo {
    MapInfo {
        layers: vec![LayerInfo::new(99, 15, 21, 10)],
        bg_colour: 0,
        objects: vec![
            MapObject::warp(
                Hitbox::new(9 * 8, 9 * 8, 8 * 2, 8),
                Warp::new(Some("house_living_room"), Vec2::new(8 * 8, 5 * 8))
                    .with_sound(sound::DOOR)
                    .with_mode(WarpMode::Auto),
            ),
            MapObject::func(
                Hitbox::new(4 * 8, 8, 4 * 25, 4 * 9),
                InteractFn::Piano(Vec2::new(4 * 8, 8)),
            ),
            MapObject::dialogue(Hitbox::new(0, 6 * 8, 8 * 2, 8), "unknown_3"),
        ],
        camera_bounds: Some(CameraBounds::stick(21 * 8 / 2 - 120, -64)),
        source: "bank1".to_string(),
        ..MapInfo::default()
    }
}

/// Map-name → its bank source, in the historical [`LEGACY_NAMES`] order. The
/// exporter and the collision-parity test both need each legacy map's bank to
/// sample tiles from; this keeps that single source of truth.
#[cfg(test)]
const LEGACY_BANKS: [&str; 12] = [
    "bank1", "bank1", "bank1", "bank1", "bank1", "bank1", "bank1", "bank1", "bank1", "bank2",
    "bank2", "bank1",
];

/// Tooling that converts the 12 hardcoded legacy maps above into modern Tiled
/// `.tmj` files (the second-to-last step of the all-maps-modern plan), plus the
/// permanent parity tests that pin the committed exports to these builders until
/// the final legacy sweep deletes the builders, the bank windows and the flag
/// table for good.
///
/// The exporter is one `#[ignore]`d test run by hand
/// (`cargo test -p egg_core export_legacy_maps -- --ignored`); the parity tests
/// run every build. Both share [`collision_grid`] (the legacy per-cell flag
/// union) and [`flag_to_gid`] (flag → collision-sprite GID), so the export and
/// its guard can never drift in how they read the old data.
#[cfg(test)]
mod export {
    use super::*;
    use crate::data::tmj::{
        ObjectLayer, Property, TileLayer, TiledMap, TiledMapLayer, Tileset, TilesetFile,
    };

    /// The collision-sprite **sheet id** a collision flag paints to (`flag` 1
    /// solid → id 2560), or 0 for the walkable flag 0 (an empty cell). The
    /// hand-painted collision vocabulary runs one sprite per flag from id 2560,
    /// so the mapping is `2559 + flag` for every flag 1..=13.
    /// office.tmj / bedroom1.tmj use the same ids (their solid is sheet id 2560,
    /// GID 2561 — [`anchored_to_office_convention`] pins this absolutely, since
    /// the exporter and the parity test share this fn and a shared off-by-one
    /// would otherwise self-validate). The exported collision layer stores these
    /// sheet ids (`to_tmj` re-adds the `firstgid` on the way out, exactly as for
    /// the art layers); the 1px half/corner discrepancies between the legacy
    /// `>= 3` predicates and the true 4px sprites are accepted (see the task's
    /// background notes).
    fn flag_to_tile(flag: u8) -> usize {
        if (1..=13).contains(&flag) {
            2559 + flag as usize
        } else {
            0
        }
    }

    /// The one absolute anchor for [`flag_to_tile`]: solid (flag 1) must land on
    /// the full-block collision sprite — sheet id 2560, GID 2561 — the id the
    /// hand-authored office.tmj and bedroom1.tmj collision layers use for their
    /// plain walls. Every other shape follows linearly (the sprite row is painted
    /// in flag order: solid, the four ramps, the four halves, the four corners).
    #[test]
    fn anchored_to_office_convention() {
        assert_eq!(flag_to_tile(1), 2560, "solid = the full-block sprite");
        assert_eq!(flag_to_gid(1), 2561, "solid GID matches office/bedroom1");
        assert_eq!(flag_to_gid(0), 0, "walkable stays an empty cell");
        assert_eq!(flag_to_gid(13), 2573, "corner_bl ends the linear run");
        assert_eq!(flag_to_gid(14), 0, "unknown flags export as empty");
    }

    /// The collision-layer **GID** a flag lands at in the written file: the sheet
    /// id ([`flag_to_tile`]) plus the single tileset's `firstgid` (1), or 0 for an
    /// empty cell. The form the collision-parity test compares the committed file
    /// against (`flag 1 → GID 2561`, matching the task's flag→GID table).
    fn flag_to_gid(flag: u8) -> usize {
        match flag_to_tile(flag) {
            0 => 0,
            id => id + 1,
        }
    }

    /// Flatten a raw bank GID to a sheet-local tile id, the way
    /// [`crate::data::tmj::from_json`] would: the bank's real tiles all live in
    /// the first tileset (`firstgid` 1), and every window cell is asserted below
    /// 2049 (no GID from bank1's stale second tileset), so the flatten is the
    /// firstgid-1 subtraction, with raw 0 and raw 1 both collapsing to 0 (the
    /// documented lossy edge the engine already lives with).
    fn flatten(raw_gid: usize) -> usize {
        raw_gid.saturating_sub(1)
    }

    /// Read a bank map (`bank1`/`bank2`) with **raw** GIDs (no flatten), so the
    /// "no GID ≥ 2049" assertion can see the real file ids and the art copy can
    /// flatten them itself ([`flatten`]).
    fn read_bank_raw(name: &str) -> TiledMap {
        let bytes = std::fs::read(format!("../assets/maps/{name}.tmj")).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// The flag table (`tiles.tsj`), indexed by plain sheet id.
    fn flag_table() -> Vec<u8> {
        let bytes = std::fs::read("../assets/maps/tiles.tsj").unwrap();
        let tileset: TilesetFile = serde_json::from_slice(&bytes).unwrap();
        tileset.flag_table()
    }

    /// A raw bank cell's GID at window-local `(ox + i, oy + j)`, or 0 off the bank.
    fn bank_gid(bank: &TiledMap, ox: i16, oy: i16, i: i16, j: i16) -> usize {
        let (x, y) = ((ox + i) as usize, (oy + j) as usize);
        bank.get(0, x, y).unwrap_or(0)
    }

    /// The collision flag a single bg [`LayerInfo`] contributes at world tile
    /// `(cx, cy)`, or `None` if the layer doesn't cover that cell. Mirrors
    /// [`crate::map::layer_collides_flags`]'s tile path exactly: the layer's pixel
    /// rect is the window placed at its offset; a covered cell samples the bank
    /// tile under it and reads its flag. Tile-aligned offsets map cleanly; the
    /// four non-aligned flagged decor layers are snapped to the nearest cell
    /// (`round(offset/8)`), matching how their few solid tiles sit in-game.
    fn layer_flag_at(layer: &LayerInfo, bank: &TiledMap, flags: &[u8], cx: i16, cy: i16) -> Option<u8> {
        let base_x = (layer.offset.x as f32 / 8.0).round() as i16;
        let base_y = (layer.offset.y as f32 / 8.0).round() as i16;
        let (i, j) = (cx - base_x, cy - base_y);
        if i < 0 || j < 0 || i >= layer.size.x || j >= layer.size.y {
            return None;
        }
        let gid = bank_gid(bank, layer.origin.x, layer.origin.y, i, j);
        Some(flags.get(flatten(gid)).copied().unwrap_or(0))
    }

    /// Reduce the flags every covering bg layer contributes at one cell to the
    /// single collision flag the exported cell carries. Solid (1) wins outright;
    /// a lone partial keeps its shape; two *different* partials over one cell
    /// can't be expressed as one flag, so they conservatively promote to solid
    /// (and log, so the export run surfaces any such cell — expected never).
    fn combine_flags(found: &[u8], cx: i16, cy: i16, map: &str) -> u8 {
        let mut result = 0u8;
        for &flag in found {
            if flag == 0 {
                continue;
            }
            if flag == 1 {
                return 1;
            }
            if result == 0 {
                result = flag;
            } else if result != flag {
                eprintln!(
                    "{map}: cell ({cx},{cy}) overlaps differing partials {result} and {flag}; promoting to solid"
                );
                return 1;
            }
        }
        result
    }

    /// The legacy collision grid for a map: `width`×`height` (the first bg
    /// layer's tile size) of per-cell flags, each the [`combine_flags`] union of
    /// every bg layer covering that cell. Shared by the exporter (to paint the
    /// collision layer) and the collision-parity test (to check the committed
    /// one), so neither can drift. fg layers never collide (the walk loop scans
    /// `layers` only), so they're excluded.
    fn collision_grid(info: &MapInfo, bank: &TiledMap, flags: &[u8], map: &str) -> (usize, usize, Vec<u8>) {
        let size = info.layers[0].size;
        let (w, h) = (size.x, size.y);
        let mut grid = vec![0u8; (w as usize) * (h as usize)];
        for cy in 0..h {
            for cx in 0..w {
                let found: Vec<u8> = info
                    .layers
                    .iter()
                    .filter_map(|layer| layer_flag_at(layer, bank, flags, cx, cy))
                    .collect();
                grid[(cx + cy * w) as usize] = combine_flags(&found, cx, cy, map);
            }
        }
        (w as usize, h as usize, grid)
    }

    /// Whether a bg art layer is an **off-grid** barrier (the wilderness left/
    /// bottom barriers): its base cell is negative, or its window runs past the
    /// map edge, so it can't live in a full-map-sized layer and is exported as a
    /// window-sized offset tile layer instead (matching the modern draw model,
    /// which blits a tile layer from its own origin at the layer offset).
    fn is_offgrid(layer: &LayerInfo, width: i16, height: i16) -> bool {
        let base_x = layer.offset.x.div_euclid(8);
        let base_y = layer.offset.y.div_euclid(8);
        base_x < 0
            || base_y < 0
            || base_x + layer.size.x > width
            || base_y + layer.size.y > height
    }

    /// Build a draw tile layer (named `name`) from a legacy art [`LayerInfo`].
    ///
    /// An on-grid layer becomes a **full-map-sized** layer (matching office /
    /// bedroom1), its window cells placed at `floor(offset/8)` with the sub-tile
    /// remainder in `offsetx`/`offsety`. An off-grid barrier becomes a
    /// **window-sized** layer placed at its full pixel offset. Cells carry
    /// flattened sheet ids (re-gid'd by `to_tmj`); a `palette_rotate` 1 layer
    /// carries the matching property.
    fn art_layer(layer: &LayerInfo, bank: &TiledMap, name: &str, width: i16, height: i16) -> TileLayer {
        let (lw, lh) = (layer.size.x, layer.size.y);
        let offgrid = is_offgrid(layer, width, height);
        let (cols, rows, base_x, base_y, offsetx, offsety) = if offgrid {
            (lw, lh, 0, 0, layer.offset.x as f64, layer.offset.y as f64)
        } else {
            let bx = layer.offset.x.div_euclid(8);
            let by = layer.offset.y.div_euclid(8);
            (
                width,
                height,
                bx,
                by,
                layer.offset.x.rem_euclid(8) as f64,
                layer.offset.y.rem_euclid(8) as f64,
            )
        };
        let mut data = vec![0usize; (cols as usize) * (rows as usize)];
        for j in 0..lh {
            for i in 0..lw {
                let (dx, dy) = (base_x + i, base_y + j);
                if dx < 0 || dy < 0 || dx >= cols || dy >= rows {
                    continue;
                }
                let gid = bank_gid(bank, layer.origin.x, layer.origin.y, i, j);
                data[(dx + dy * cols) as usize] = flatten(gid);
            }
        }
        let properties = if layer.palette_rotate() != 0 {
            vec![Property::int("palette_rotate", layer.palette_rotate() as i64)]
        } else {
            Vec::new()
        };
        TileLayer {
            width: cols as usize,
            height: rows as usize,
            data,
            name: name.to_string(),
            offsetx,
            offsety,
            properties,
        }
    }

    /// The bg/fg art-layer display names for a map, matching the `//` comments on
    /// its builder's `LayerInfo`s (uncommented layers get a sensible stand-in).
    /// Returned as `(bg names, fg names)` aligned with `MapInfo.layers` /
    /// `fg_layers`. The bg name list is the *art* layers only — the collision
    /// layer the exporter prepends is named separately.
    fn layer_names(map: &str) -> (Vec<&'static str>, Vec<&'static str>) {
        match map {
            "supermarket" => (
                vec!["bg", "fruit stand", "vending machines", "counter", "top vending machine"],
                vec![],
            ),
            "supermarket_hall" => (vec!["bg", "closet", "diagonal door"], vec![]),
            "supermarket_storeroom" => (vec!["bg", "shelf"], vec![]),
            "test_pen" => (vec!["bg"], vec![]),
            "bedroom" => (vec!["room", "trolley", "mattress"], vec![]),
            "house_stairwell" => (vec!["room", "left door", "right door"], vec![]),
            "house_living_room" => (vec!["room", "couch", "tv"], vec!["fg tv"]),
            "house_kitchen" => (vec!["room", "microwave"], vec![]),
            "backyard" => (vec!["room"], vec![]),
            "wilderness" => (
                vec!["ground", "left barrier", "bottom barrier"],
                vec!["fg foreground"],
            ),
            "town" => (vec!["ground"], vec!["fg foreground"]),
            "piano_room" => (vec!["bg"], vec![]),
            _ => (vec![], vec![]),
        }
    }

    /// The painted-collision image layers a map needs for collision its tile
    /// grid can't hold: the two off-grid wilderness barriers, as invisible
    /// `collision`-named image layers over the pre-existing opaque barrier PNGs.
    /// Empty for every other map.
    fn barrier_image_layers(map: &str) -> Vec<TiledMapLayer> {
        if map != "wilderness" {
            return Vec::new();
        }
        vec![
            image_layer("collision left barrier", "images/wilderness_barrier_left.png", -8.0, 296.0),
            image_layer("collision bottom barrier", "images/wilderness_barrier_bottom.png", 136.0, 544.0),
        ]
    }

    /// One invisible image layer at a pixel offset, the codec form used both for
    /// the barriers' painted-collision masks (names starting `collision`, so the
    /// model treats them as collision) and the preserved stairwell tracing mask
    /// (a non-`collision` name, so it stays a — hidden — drawn layer).
    fn image_layer(name: &str, image: &str, offsetx: f64, offsety: f64) -> TiledMapLayer {
        TiledMapLayer::ImageLayer(crate::data::tmj::ImageLayer {
            name: name.to_string(),
            image: image.to_string(),
            offsetx,
            offsety,
            visible: false,
            opacity: 1.0,
            properties: Vec::new(),
            pixels: None,
        })
    }

    /// The map-level properties a map carries: a `bg_colour` when nonzero and a
    /// `camera_stick` `"x,y"` when the map pins its camera.
    fn map_properties(info: &MapInfo) -> Vec<Property> {
        let mut properties = Vec::new();
        if info.bg_colour != 0 {
            properties.push(Property::int("bg_colour", info.bg_colour as i64));
        }
        if let Some(bounds) = &info.camera_bounds
            && let Some((x, y)) = stick_values(bounds)
        {
            properties.push(Property::string("camera_stick", &format!("{x},{y}")));
        }
        properties
    }

    /// The `(x, y)` of a stick camera, recovered by probing the bounds (its
    /// fields are private): a `Stick` axis clamps every focus to its pinned value,
    /// so bounding any point reads it straight back. Only `piano_room` uses one.
    fn stick_values(bounds: &CameraBounds) -> Option<(i16, i16)> {
        let probe = bounds.bound(Vec2::new(0, 0));
        let other = bounds.bound(Vec2::new(1000, 1000));
        (probe == other).then_some((probe.x, probe.y))
    }

    /// The mask image layer to append to the `house_stairwell` export: the user's
    /// tracing reference, kept (but hidden) so the export doesn't discard it. Its
    /// name deliberately doesn't start with `collision`, so it stays a drawn (but
    /// invisible) layer rather than becoming a collision mask.
    fn stairwell_mask() -> TiledMapLayer {
        image_layer("Image Layer 1", "images/house_stairwell_mask.png", 74.0, 33.0)
    }

    /// Build the full modern [`TiledMap`] for a legacy `map`: a collision tile
    /// layer (flag→GID over [`collision_grid`]), the bg then fg art layers, the
    /// wilderness barriers' painted-collision image layers, an objects layer, the
    /// map properties, and (for `house_stairwell`) the preserved tracing mask.
    fn build_tiled_map(map: &str, info: &MapInfo, bank: &TiledMap, flags: &[u8]) -> (TiledMap, Vec<MapObject>) {
        // Assert the windows never touch bank1's stale second tileset.
        assert_window_gids_in_range(info, bank);

        let (width, height, grid) = collision_grid(info, bank, flags, map);
        let (w, h) = (width as i16, height as i16);
        let (bg_names, fg_names) = layer_names(map);

        let mut layers = Vec::new();
        // Collision layer first (modern model: first tile layer = collision).
        layers.push(TiledMapLayer::TileLayer(TileLayer {
            width,
            height,
            data: grid.iter().map(|&f| flag_to_tile(f)).collect(),
            name: "collision".to_string(),
            ..Default::default()
        }));
        // bg art layers, then the barriers' collision masks, then fg art layers.
        for (layer, name) in info.layers.iter().zip(&bg_names) {
            layers.push(TiledMapLayer::TileLayer(art_layer(layer, bank, name, w, h)));
        }
        layers.extend(barrier_image_layers(map));
        for (layer, name) in info.fg_layers.iter().zip(&fg_names) {
            layers.push(TiledMapLayer::TileLayer(art_layer(layer, bank, name, w, h)));
        }
        // Objects (one group, serialised from the builder's object list).
        layers.push(TiledMapLayer::ObjectLayer(ObjectLayer {
            name: "objects".to_string(),
            objects: Vec::new(),
        }));
        if map == "house_stairwell" {
            layers.push(stairwell_mask());
        }

        let tiled = TiledMap {
            width,
            height,
            layers,
            tilesets: vec![Tileset {
                firstgid: 1,
                source: "tiles.tsj".to_string(),
            }],
            properties: map_properties(info),
        };
        (tiled, info.objects.clone())
    }

    /// Assert every bg/fg window cell is below GID 2049 (no tile from bank1's
    /// stale second tileset), so the flatten is the simple firstgid-1 subtraction.
    fn assert_window_gids_in_range(info: &MapInfo, bank: &TiledMap) {
        for layer in info.layers.iter().chain(&info.fg_layers) {
            for j in 0..layer.size.y {
                for i in 0..layer.size.x {
                    let gid = bank_gid(bank, layer.origin.x, layer.origin.y, i, j);
                    assert!(gid < 2049, "window GID {gid} >= 2049 (stale tileset)");
                }
            }
        }
    }

    /// Export all 12 legacy maps to `assets/maps/<name>.tmj`. Run by hand:
    /// `cargo test -p egg_core export_legacy_maps -- --ignored`.
    #[test]
    #[ignore = "regenerates committed map assets; run manually"]
    fn export_legacy_maps() {
        let flags = flag_table();
        for (idx, name) in LEGACY_NAMES.iter().enumerate() {
            let info = legacy_map(name).unwrap();
            let bank = read_bank_raw(LEGACY_BANKS[idx]);
            let (tiled, objects) = build_tiled_map(name, &info, &bank, &flags);
            let json = tiled.to_tmj(&objects);
            std::fs::write(format!("../assets/maps/{name}.tmj"), json).unwrap();
            eprintln!("exported {name}.tmj");
        }
    }

    /// **Collision parity**: the committed `<name>.tmj`'s collision layer (its
    /// first tile layer) is exactly `flag_to_gid` over the legacy
    /// [`collision_grid`], for all 12 maps — so any hand-edit of an exported
    /// collision layer that drifts from the builder + flag table is caught until
    /// the legacy sweep removes flags entirely.
    #[test]
    fn collision_parity() {
        let flags = flag_table();
        for (idx, name) in LEGACY_NAMES.iter().enumerate() {
            let info = legacy_map(name).unwrap();
            let bank = read_bank_raw(LEGACY_BANKS[idx]);
            let (w, h, grid) = collision_grid(&info, &bank, &flags, name);
            let expected: Vec<usize> = grid.iter().map(|&f| flag_to_gid(f)).collect();

            let bytes = std::fs::read(format!("../assets/maps/{name}.tmj")).unwrap();
            let exported = crate::data::tmj::from_json(&bytes).unwrap();
            let collision = exported
                .layers
                .iter()
                .find_map(|l| match l {
                    TiledMapLayer::TileLayer(t) => Some(t),
                    _ => None,
                })
                .expect("exported map has a collision tile layer");
            assert_eq!(
                (collision.width, collision.height),
                (w, h),
                "{name}: collision layer size"
            );
            // `from_json` flattened the GIDs; re-gid for comparison with the
            // expected GID list (the flatten maps the firstgid back to 0/empty).
            let actual: Vec<usize> = collision
                .data
                .iter()
                .map(|&t| if t == 0 { 0 } else { t + 1 })
                .collect();
            assert_eq!(actual, expected, "{name}: collision layer GIDs");
        }
    }

    /// **Object parity**: the committed `<name>.tmj`'s parsed objects re-serialise
    /// (through `object_to_tmj`) to byte-identical JSON to the legacy builder's
    /// objects, for all 12 maps — so every warp/interaction/func/sprite (incl. the
    /// living-room's multi-frame `anim` sprites and sprite-only objects) survives
    /// the round-trip the export relies on.
    #[test]
    fn object_parity() {
        for name in LEGACY_NAMES.iter() {
            let info = legacy_map(name).unwrap();
            let bytes = std::fs::read(format!("../assets/maps/{name}.tmj")).unwrap();
            let exported = crate::data::tmj::from_json(&bytes).unwrap();
            let parsed = exported.parse_objects();
            assert_eq!(
                objects_json(&info.objects),
                objects_json(&parsed),
                "{name}: object round-trip"
            );
        }
    }

    /// Serialise an object list to the JSON `object_to_tmj` emits, for comparison.
    /// Any object the codec can't spell (there should be none) surfaces as a
    /// `null`, so a dropped object fails the parity assert loudly.
    fn objects_json(objects: &[MapObject]) -> serde_json::Value {
        // `object_to_tmj` is private; round-trip through `to_tmj` on a throwaway
        // one-object-layer map and read the objects array back.
        let map = TiledMap {
            width: 1,
            height: 1,
            layers: vec![TiledMapLayer::ObjectLayer(ObjectLayer {
                name: "objects".to_string(),
                objects: Vec::new(),
            })],
            tilesets: vec![Tileset {
                firstgid: 1,
                source: "tiles.tsj".to_string(),
            }],
            properties: Vec::new(),
        };
        let json = map.to_tmj(objects);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["layers"][0]["objects"].clone()
    }

    /// **Art parity**: every exported art tile layer's nonzero cells equal its
    /// bank window (flattened), placed at the layer's recorded base cell + offset.
    /// Spot-checks the full layer for one map per bank so the art copy is pinned
    /// without re-deriving the whole sheet.
    #[test]
    fn art_parity() {
        for name in ["supermarket", "wilderness"] {
            let info = legacy_map(name).unwrap();
            let bank = read_bank_raw(if name == "wilderness" { "bank2" } else { "bank1" });
            let bytes = std::fs::read(format!("../assets/maps/{name}.tmj")).unwrap();
            let exported = crate::data::tmj::from_json(&bytes).unwrap();
            let (w, h, _) = collision_grid(&info, &bank, &flag_table(), name);
            let (bg_names, fg_names) = layer_names(name);

            // Rebuild each art layer and find its committed twin by name.
            for (layer, lname) in info
                .layers
                .iter()
                .zip(&bg_names)
                .chain(info.fg_layers.iter().zip(&fg_names))
            {
                let want = art_layer(layer, &bank, lname, w as i16, h as i16);
                let got = exported
                    .layers
                    .iter()
                    .find_map(|l| match l {
                        TiledMapLayer::TileLayer(t) if t.name == *lname => Some(t),
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("{name}: exported layer {lname:?} missing"));
                assert_eq!(got.data, want.data, "{name}: layer {lname:?} cells");
                assert_eq!(
                    (got.offsetx, got.offsety),
                    (want.offsetx, want.offsety),
                    "{name}: layer {lname:?} offset"
                );
            }
        }
    }

    /// The objects layer must be present and named `objects`, and every map's
    /// objects round-trip without a single dropped object (a dropped object would
    /// be a hole in the `anim`/sprite vocabulary). Belt-and-braces over
    /// [`object_parity`] for the count.
    #[test]
    fn no_objects_dropped() {
        for name in LEGACY_NAMES.iter() {
            let info = legacy_map(name).unwrap();
            let bytes = std::fs::read(format!("../assets/maps/{name}.tmj")).unwrap();
            let exported = crate::data::tmj::from_json(&bytes).unwrap();
            assert_eq!(
                exported.parse_objects().len(),
                info.objects.len(),
                "{name}: every legacy object round-trips"
            );
        }
    }

    /// **Map-properties parity**: the exported maps carry exactly the `bg_colour`
    /// / `camera_stick` the builders imply (supermarket trio + test_pen + the
    /// wilderness bg colour; piano_room's stuck camera), and `modern_map_info`
    /// reads them back into the right `MapInfo` fields.
    #[test]
    fn map_properties_parity() {
        use crate::map::map_by_name;
        use crate::map::MapStore;
        use crate::system::test_console::TestConsole;

        let console = TestConsole::new();
        for name in LEGACY_NAMES.iter() {
            let legacy = legacy_map(name).unwrap();
            let bytes = std::fs::read(format!("../assets/maps/{name}.tmj")).unwrap();
            let mut store = MapStore::default();
            store.insert(*name, crate::data::tmj::from_json(&bytes).unwrap());
            let modern = map_by_name(&console.indexed_sprites, name, &store).unwrap();
            assert_eq!(modern.bg_colour, legacy.bg_colour, "{name}: bg_colour");
            assert_eq!(
                stick_values_opt(&modern.camera_bounds),
                stick_values_opt(&legacy.camera_bounds),
                "{name}: camera stick"
            );
        }
    }

    /// `stick_values` over an optional bounds (None camera → None).
    fn stick_values_opt(bounds: &Option<CameraBounds>) -> Option<(i16, i16)> {
        bounds.as_ref().and_then(stick_values)
    }

    /// **Round-trip safety**: each exported map survives `from_json → to_tmj →
    /// from_json` stable for the new constructs — tile-layer offsets/properties,
    /// map-level properties, and any `anim` object sprites — so an in-game save
    /// of an exported map never corrupts them.
    #[test]
    fn export_round_trips() {
        for name in LEGACY_NAMES.iter() {
            let bytes = std::fs::read(format!("../assets/maps/{name}.tmj")).unwrap();
            let map = crate::data::tmj::from_json(&bytes).unwrap();
            let once = map.to_tmj(&map.parse_objects());
            let reloaded = crate::data::tmj::from_json(once.as_bytes()).unwrap();
            let twice = reloaded.to_tmj(&reloaded.parse_objects());
            assert_eq!(once, twice, "{name}: to_tmj is idempotent after a reload");

            // The new constructs survive: layer offsets/properties and map props.
            assert_eq!(
                tile_layer_shapes(&map),
                tile_layer_shapes(&reloaded),
                "{name}: tile-layer offsets/properties stable"
            );
            assert_eq!(
                map.properties.len(),
                reloaded.properties.len(),
                "{name}: map properties stable"
            );
            // Objects (incl. anim sprites) stay byte-identical too.
            assert_eq!(
                objects_json(&map.parse_objects()),
                objects_json(&reloaded.parse_objects()),
                "{name}: objects stable"
            );
        }
    }

    /// Each tile layer's `(name, offsetx, offsety, property count)` — the
    /// round-trip-sensitive shape the new tile-layer fields add.
    fn tile_layer_shapes(map: &TiledMap) -> Vec<(String, f64, f64, usize)> {
        map.layers
            .iter()
            .filter_map(|l| match l {
                TiledMapLayer::TileLayer(t) => {
                    Some((t.name.clone(), t.offsetx, t.offsety, t.properties.len()))
                }
                _ => None,
            })
            .collect()
    }
}

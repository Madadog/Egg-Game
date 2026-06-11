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
use crate::interact::{InteractFn, Interactable, Interaction};
use crate::map::{Axis, LayerInfo, MapInfo, Warp, WarpMode};
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
        warps: vec![
            Warp::new_tile(17, 4, Some("supermarket_hall"), 9, 4).with_sound(sound::DOOR),
            Warp::new_tile(8, 4, Some("supermarket_hall"), 3, 4).with_sound(sound::DOOR),
            Warp::new(
                Hitbox::new(11 * 8, 11 * 8, 3 * 8, 8),
                Some("town"),
                Vec2::new(51 * 4, 15 * 8),
            )
            .with_sound(sound::DOOR)
            .with_mode(WarpMode::Auto),
        ],
        interactables: vec![
            Interactable::dialogue(Hitbox::new(13 * 8, 5 * 4, 8 * 2, 8 * 3), "sm_coin_return"),
            Interactable::dialogue(Hitbox::new(2 * 8, 8 * 8, 8 * 3, 8 * 2), "sm_fruit_basket"),
            Interactable::dialogue(Hitbox::new(4 * 8, 5 * 8, 8, 20), "sm_main_window"),
            Interactable::dialogue(Hitbox::new(19 * 8, 5 * 8, 8, 15), "sm_fridge_1"),
            Interactable::dialogue(Hitbox::new(20 * 8, 6 * 8, 8, 15), "sm_fridge_2"),
            Interactable::dialogue(Hitbox::new(21 * 8, 7 * 8, 8, 16), "sm_vending_machine"),
            Interactable::dialogue(Hitbox::new(11 * 8, 10 * 8, 3 * 8, 8), "construction_1"),
            Interactable::dialogue(Hitbox::new(80, 24, 16, 20), "thing").with_sprite(vec![
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
        warps: vec![
            Warp::new_tile(9, 6, Some("supermarket"), 17, 4)
                .with_mode(WarpMode::Auto)
                .with_sound(sound::DOOR),
            Warp::new_tile(3, 6, Some("supermarket"), 8, 4)
                .with_mode(WarpMode::Auto)
                .with_sound(sound::DOOR),
            Warp::new_tile(4, 2, Some("supermarket_storeroom"), 2, 3).with_sound(sound::DOOR),
        ],
        interactables: vec![
            Interactable::dialogue(Hitbox::new(11 * 8, 4 * 8, 8, 8), "emergency_exit"),
            Interactable::dialogue(Hitbox::new(8 * 8, 3 * 8, 8, 8), "construction_2"),
            Interactable::dialogue(Hitbox::new(11 * 4, 0, 2 * 8, 7 * 4), "sm_hall_shelf"),
            Interactable::dialogue(Hitbox::new(8, 3 * 8, 12, 16), "sm_hall_window"),
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
        warps: vec![Warp::new_tile(2, 5, Some("supermarket_hall"), 4, 2)
            .with_mode(WarpMode::Auto)
            .with_sound(sound::DOOR)],
        interactables: vec![
            Interactable::dialogue(Hitbox::new(53, 28, 8, 10), "egg_1").with_sprite(bob(524)),
            Interactable::dialogue(Hitbox::new(16, 0, 5 * 8, 4 * 7), "sm_storeroom_shelf"),
        ],
        bg_colour: 1,
        source: "bank1".to_string(),
        ..MapInfo::default()
    }
}

fn test_pen() -> MapInfo {
    MapInfo {
        layers: vec![LayerInfo::new(53, 17, 7, 9).with_rot_and_shift_flags(1, 0)],
        warps: vec![Warp::new_tile(3, 8, Some("supermarket"), 10, 4)],
        interactables: vec![
            Interactable::dialogue(Hitbox::new(5 * 8, 8, 8, 10), "egg_1").with_sprite(bob(524)),
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
        warps: vec![Warp::new(
            Hitbox::new(15 * 8, 6 * 8, 8, 8),
            Some("house_stairwell"),
            Vec2::new(8 + 1, 2 * 8),
        )
        .with_sound(sound::DOOR)],
        interactables: vec![
            Interactable::dialogue(Hitbox::new(38, 27, 3 * 8, 2 * 8), "bedroom_mattress"),
            Interactable::dialogue(Hitbox::new(2 * 8, 4 * 8, 2 * 8, 4 * 8), "bedroom_closet"),
            Interactable::dialogue(Hitbox::new(101 - 16, 22, 3 * 8, 2 * 8), "bedroom_trolley"),
            Interactable::dialogue(Hitbox::new(9 * 8, 3 * 8, 8, 8), "bedroom_window"),
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
        warps: vec![
            Warp::new(
                Hitbox::new(1, 3 * 8, 8, 8),
                Some("bedroom"),
                Vec2::new(14 * 8, 5 * 8),
            )
            .with_sound(sound::DOOR),
            Warp::new(
                Hitbox::new(7 * 8, 9 * 8, 2 * 8, 8),
                Some("house_living_room"),
                Vec2::new(21 * 4, 4 * 8),
            )
            .with_sound(sound::STAIRS_DOWN)
            .with_mode(WarpMode::Auto),
        ],
        interactables: vec![
            Interactable::func(Hitbox::new(2 * 8, 2 * 8, 8, 8), InteractFn::StairwellWindow),
            Interactable::func(Hitbox::new(7 * 8, 4 * 8, 2 * 8, 8), InteractFn::StairwellPainting),
            Interactable::dialogue(Hitbox::new(13 * 8, 2 * 8, 8, 8), "house_stairwell_window2"),
            Interactable::dialogue(Hitbox::new(15 * 8, 3 * 8, 8, 8), "house_stairwell_door"),
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
        warps: vec![
            Warp::new(
                Hitbox::new(10 * 8, 4 * 8, 2 * 8, 8),
                Some("house_stairwell"),
                Vec2::new(15 * 4, 7 * 8),
            )
            .with_sound(sound::STAIRS_UP)
            .with_mode(WarpMode::Auto),
            Warp::new(
                Hitbox::new(3 * 8, 9 * 8, 8, 8),
                Some("town"),
                Vec2::new(17 * 8, 13 * 8),
            )
            .with_sound(sound::DOOR)
            .with_flip(Axis::Y),
            Warp::new(
                Hitbox::new(14 * 8, 5 * 8, 8, 8),
                Some("house_kitchen"),
                Vec2::new(7 * 4, 7 * 8),
            )
            .with_sound(sound::DOOR),
            Warp::new(
                Hitbox::new(8 * 8, 5 * 8, 8, 8),
                Some("piano_room"),
                Vec2::new(19 * 4, 6 * 8),
            )
            .with_sound(sound::DOOR),
        ],
        interactables: vec![
            Interactable::dialogue(
                Hitbox::new(12 * 8 + 2, 7 * 8, 3 * 8, 3 * 8),
                "house_living_room_couch",
            ),
            Interactable::dialogue(
                Hitbox::new(15 * 8 + 2, 11 * 8 - 1, 2 * 8, 2 * 8),
                "house_living_room_tv_1",
            ),
            Interactable::dialogue(
                Hitbox::new(5 * 8, 6 * 8, 2 * 8, 2 * 8),
                "house_living_room_window",
            ),
            Interactable::new(
                Hitbox::new(12 * 8 + 2, 7 * 8, 1, 1),
                Interaction::None,
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
            Interactable::new(
                Hitbox::new(12 * 8 + 9, 7 * 8, 8, 8),
                Interaction::None,
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
        warps: vec![
            Warp::new(
                Hitbox::new(2 * 8, 8 * 8 + 7, 4 * 8, 8),
                Some("house_living_room"),
                Vec2::new(14 * 8, 5 * 8),
            )
            .with_sound(sound::DOOR)
            .with_mode(WarpMode::Auto),
            Warp::new(
                Hitbox::new(11 * 8, 4 * 8, 8, 3 * 8),
                Some("backyard"),
                Vec2::new(15 * 8, 5 * 8),
            )
            .with_sound(sound::DOOR),
        ],
        interactables: vec![
            Interactable::dialogue(Hitbox::new(2 * 8, 4 * 8, 2 * 8, 2 * 8), "house_kitchen_cupboard"),
            Interactable::dialogue(Hitbox::new(5 * 8, 4 * 8, 4 * 3 - 2, 2 * 8), "house_kitchen_sink"),
            Interactable::dialogue(
                Hitbox::new(16 * 4 - 2, 4 * 8, 2 * 8 + 2, 2 * 8),
                "house_kitchen_microwave",
            ),
            Interactable::dialogue(Hitbox::new(7 * 8, 4 * 8, 8, 2 * 8), "house_kitchen_window"),
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
        warps: vec![
            Warp::new(
                Hitbox::new(15 * 8, 5 * 8, 8, 8),
                Some("house_kitchen"),
                Vec2::new(10 * 8 - 3, 5 * 8 + 3),
            )
            .with_sound(sound::DOOR)
            .with_flip(Axis::Y),
            Warp::new(
                Hitbox::new(12 * 8, 16 * 8 + 7, 4 * 8, 8),
                Some("wilderness"),
                Vec2::new(8 * 8, 61 * 8),
            )
            .with_mode(WarpMode::Auto)
            .with_flip(Axis::Y),
        ],
        interactables: vec![
            Interactable::dialogue(Hitbox::new(9 * 8, 5 * 8, 2 * 8, 2 * 8), "house_backyard_basement"),
            Interactable::dialogue(Hitbox::new(20 * 8, 8 * 8, 8, 2 * 8), "house_backyard_shed"),
            Interactable::dialogue(
                Hitbox::new(22 * 8, 8 * 8, 8, 2 * 8),
                "house_backyard_shed_window",
            ),
            Interactable::dialogue(
                Hitbox::new(24 * 8, 10 * 8, 8, 6 * 8),
                "house_backyard_neighbours",
            ),
            Interactable::func(Hitbox::new(21 * 8, 13 * 8, 8, 8), InteractFn::ToggleDog),
            Interactable::dialogue(Hitbox::new(5 * 8, 0, 8, 16 * 8), "house_backyard_stormdrain"),
            Interactable::dialogue(Hitbox::new(3, 2 * 8, 8, 8), "default").with_sprite(vec![
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
        warps: vec![Warp::new(
            Hitbox::new(7 * 8, 63 * 8 + 4, 2 * 8, 8),
            Some("backyard"),
            Vec2::new(14 * 8 - 4, 15 * 8),
        )
        .with_mode(WarpMode::Auto)
        .with_flip(Axis::Y)],
        interactables: vec![],
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
        warps: vec![
            Warp::new(
                Hitbox::new(17 * 8, 13 * 8, 8, 8),
                Some("house_living_room"),
                Vec2::new(4 * 9, 8 * 8),
            )
            .with_sound(sound::DOOR),
            Warp::new(
                Hitbox::new(25 * 8, 15 * 8, 2 * 8, 8),
                Some("supermarket"),
                Vec2::new(97, 73),
            )
            .with_sound(sound::DOOR),
        ],
        interactables: vec![
            Interactable::dialogue(Hitbox::new(8 * 6, 17 * 8, 8, 6 * 8), "town_traffic"),
            Interactable::dialogue(Hitbox::new(8 * 8, 17 * 8, 8, 8), "town_lamppost"),
            Interactable::dialogue(Hitbox::new(14 * 8, 13 * 8, 8, 8), "town_home_window"),
            Interactable::dialogue(Hitbox::new(224, 142, 8 * 2, 8), "town_wide"),
        ],
        source: "bank2".to_string(),
        ..MapInfo::default()
    }
}

fn piano_room() -> MapInfo {
    MapInfo {
        layers: vec![LayerInfo::new(99, 15, 21, 10)],
        bg_colour: 0,
        warps: vec![Warp::new(
            Hitbox::new(9 * 8, 9 * 8, 8 * 2, 8),
            Some("house_living_room"),
            Vec2::new(8 * 8, 5 * 8),
        )
        .with_sound(sound::DOOR)
        .with_mode(WarpMode::Auto)],
        interactables: vec![
            Interactable::func(
                Hitbox::new(4 * 8, 8, 4 * 25, 4 * 9),
                InteractFn::Piano(Vec2::new(4 * 8, 8)),
            ),
            Interactable::dialogue(Hitbox::new(0, 6 * 8, 8 * 2, 8), "unknown_3"),
        ],
        camera_bounds: Some(CameraBounds::stick(21 * 8 / 2 - 120, -64)),
        source: "bank1".to_string(),
        ..MapInfo::default()
    }
}

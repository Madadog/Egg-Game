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
use crate::position::Hitbox;
use crate::dialogue::TextContent;
use crate::position::Vec2;

#[derive(Debug, Clone)]
pub enum Interaction<'a> {
    Text(&'static str),
    EnumText(&'a [TextContent]),
    Dialogue(&'a [&'static str]),
    Func(InteractFn),
    None,
}

#[derive(Debug, Clone)]
pub struct Interactable<'a> {
    pub hitbox: Hitbox,
    pub interaction: Interaction<'a>,
    pub sprite: Option<&'a [AnimFrame<'a>]>,
}

impl<'a> Interactable<'a> {
    pub const fn new(
        hitbox: Hitbox,
        interaction: Interaction<'a>,
        sprite: Option<&'a [AnimFrame<'a>]>,
    ) -> Self {
        Self {
            hitbox,
            interaction,
            sprite,
        }
    }
}

/// A 'scripting' API for the walkaround section of the game. Various interactables
/// do one-off things, so they are all put inside this enum.
/// 
/// This probably doesn't scale well.
#[derive(Debug, Clone)]
pub enum InteractFn {
    ToggleDog,
    StairwellWindow,
    StairwellPainting,
    /// i32: Pitch of note
    Note(i32),
    /// Vec2: Size of piano
    Piano(Vec2),
    /// usize: number of creatures to add
    AddCreatures(usize),
    /// Vec2: Dog position. bool: Dog direction, false=left, true=right
    Pet(Vec2, Option<bool>),
}

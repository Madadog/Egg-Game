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

use crate::position::Vec2;

/// The effect payload of an interaction [`MapObject`](crate::map::MapObject):
/// what running it does. Resolved against the dialogue registry / dispatched
/// when the player triggers the object.
#[derive(Debug, Clone)]
pub enum Interaction {
    /// A dialogue-registry key. Resolved to a `Vec<Message>` when it fires.
    Dialogue(String),
    Func(InteractFn),
    None,
}

/// A 'scripting' API for the walkaround section of the game. Various interactions
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

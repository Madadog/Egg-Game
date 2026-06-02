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
use crate::position::Vec2;

/// Compile-time interaction. Dialogue is referenced by a `&'static str` key
/// into the registry built by [`crate::data::script`]; the actual text
/// is resolved at runtime when the interaction fires.
#[derive(Debug, Clone)]
pub enum StaticInteraction {
    /// A dialogue-registry key. Resolves to a `Vec<Message>` at runtime.
    Dialogue(&'static str),
    Func(InteractFn),
    None,
}

#[derive(Debug, Clone)]
pub struct StaticInteractable<'a> {
    pub hitbox: Hitbox,
    pub interaction: StaticInteraction,
    pub sprite: Option<&'a [StaticAnimFrame<'a>]>,
}

impl<'a> StaticInteractable<'a> {
    pub const fn new(
        hitbox: Hitbox,
        interaction: StaticInteraction,
        sprite: Option<&'a [StaticAnimFrame<'a>]>,
    ) -> Self {
        Self {
            hitbox,
            interaction,
            sprite,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Interaction {
    /// A dialogue-registry key. Resolved to a `Vec<Message>` when it fires.
    Dialogue(String),
    Func(InteractFn),
    None,
}

impl From<StaticInteraction> for Interaction {
    fn from(other: StaticInteraction) -> Self {
        match other {
            StaticInteraction::Dialogue(key) => Self::Dialogue(key.to_string()),
            StaticInteraction::Func(x) => Self::Func(x),
            StaticInteraction::None => Self::None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Interactable {
    pub hitbox: Hitbox,
    pub interaction: Interaction,
    pub sprite: Option<Vec<AnimFrame>>,
}

impl Interactable {
    pub const fn new(
        hitbox: Hitbox,
        interaction: Interaction,
        sprite: Option<Vec<AnimFrame>>,
    ) -> Self {
        Self {
            hitbox,
            interaction,
            sprite,
        }
    }
}

impl<'a> From<StaticInteractable<'a>> for Interactable {
    fn from(other: StaticInteractable) -> Self {
        Self {
            hitbox: other.hitbox,
            interaction: other.interaction.into(),
            sprite: other
                .sprite
                .map(|x| x.iter().map(|x| x.clone().into()).collect()),
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

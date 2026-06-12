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

use crate::position::{Hitbox, Vec2};

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

/// A small 'scripting' verb for the walkaround section: the one-off behaviours a
/// map object can run when triggered (toggling a companion, sounding a piano
/// key, spawning creatures…), dispatched by
/// [`execute_interact_fn`](crate::gamestate::walkaround::WalkaroundState::execute_interact_fn).
///
/// The cases that a map authors declaratively are *named* — a `.tmj` object
/// with a `func` property round-trips through [`from_name`](Self::from_name) /
/// [`name`](Self::name) (the `sound::by_name` / `portraits::by_name` precedent),
/// so the in-game editor can place and re-save them. State-only conditionals
/// that used to live here (the stairwell window/painting pair) are gone: they
/// are now plain dialogue objects driven by named save flags + `#set`/`#if` in
/// the script (see [`crate::data::eggtext`]), which keeps the enum to genuine
/// *behaviour* rather than per-object boolean wiring.
///
/// [`Pet`](Self::Pet) is the one case with no name: it is constructed only by
/// [`Companion::interact`](crate::player::Companion::interact) from live
/// companion/player positions, never from a map object, so there is nothing for
/// an editor to place and it has no `func` spelling.
#[derive(Debug, Clone)]
pub enum InteractFn {
    /// Add or remove the dog companion. `func = "toggle_dog"` (no properties).
    ToggleDog,
    /// Sound one piano key from the note under the player. `func = "piano"`; its
    /// origin is the owning object's hitbox origin, so it carries no properties.
    Piano(Vec2),
    /// Play a single note. `func = "note"`, `pitch` int property.
    Note(i32),
    /// Spawn `count + 1` wandering creatures at the player. `func =
    /// "add_creatures"`, `count` int property.
    AddCreatures(usize),
    /// Pet the dog. Companion-internal (see the type doc): no `func` name.
    /// `Vec2`: dog position. `bool`: facing, `false` = left, `true` = right.
    Pet(Vec2, Option<bool>),
}

impl InteractFn {
    /// Build the [`InteractFn`] a `.tmj` object names through its `func`
    /// property, reading any scalar properties it needs (`pitch`, `count`) and
    /// taking positional data from the object's `hitbox` (the piano's origin).
    /// `None` for an unknown name, so the caller can fall through to other
    /// object kinds. Inverse of [`name`](Self::name) (plus the scalar props
    /// [`tmj`](crate::data::tmj) writes back out).
    pub fn from_name(
        name: &str,
        pitch: Option<i32>,
        count: Option<usize>,
        hitbox: Hitbox,
    ) -> Option<Self> {
        Some(match name {
            "toggle_dog" => InteractFn::ToggleDog,
            "piano" => InteractFn::Piano(Vec2::new(hitbox.x, hitbox.y)),
            "note" => InteractFn::Note(pitch.unwrap_or(0)),
            "add_creatures" => InteractFn::AddCreatures(count.unwrap_or(0)),
            _ => return None,
        })
    }

    /// The `func` name a `.tmj` object serialises this as, or `None` for the
    /// cases that don't round-trip through a map object ([`Pet`](Self::Pet)).
    /// The scalar properties a name needs (`pitch`/`count`) are written by
    /// [`tmj`](crate::data::tmj); the piano needs none (its origin is the
    /// hitbox). Inverse of [`from_name`](Self::from_name).
    pub fn name(&self) -> Option<&'static str> {
        Some(match self {
            InteractFn::ToggleDog => "toggle_dog",
            InteractFn::Piano(_) => "piano",
            InteractFn::Note(_) => "note",
            InteractFn::AddCreatures(_) => "add_creatures",
            InteractFn::Pet(..) => return None,
        })
    }
}

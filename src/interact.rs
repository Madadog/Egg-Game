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

use crate::COMPANIONS;
use crate::COMPANION_TRAIL;
use crate::dialogue_data::{DOG_OBTAINED, DOG_RELINQUISHED, HOUSE_BACKYARD_DOGHOUSE};
use crate::player::Companion;
use crate::animation::*;
use crate::Hitbox;

#[derive(Debug)]
pub enum Interaction<'a> {
    Text(&'a str),
    Func(InteractFn),
}

#[derive(Debug)]
pub struct Interactable<'a> {
    pub hitbox: Hitbox,
    pub interaction: Interaction<'a>,
    pub sprite: Option<Animation<'a>>,
}

impl<'a> Interactable<'a> {
    pub const fn new(
        hitbox: Hitbox,
        interaction: Interaction<'a>,
        sprite: Option<Animation<'a>>,
    ) -> Self {
        Self {
            hitbox,
            interaction,
            sprite,
        }
    }
}


#[derive(Debug)]
pub enum InteractFn {
    ToggleDog,
}
impl InteractFn {
    pub fn execute(&self) -> Option<&'static str> {
        match self {
            Self::ToggleDog => {
                COMPANION_TRAIL.write().unwrap().fill(crate::player().pos, crate::player().dir);
                let mut x = COMPANIONS.write().unwrap();
                if x.has(Companion::Dog) {
                    x.remove(Companion::Dog);
                    Some(DOG_RELINQUISHED)
                } else {
                    x.add(Companion::Dog);
                    Some(DOG_OBTAINED)
                }
            },
            _ => {Some(HOUSE_BACKYARD_DOGHOUSE)}
        }
    }
}
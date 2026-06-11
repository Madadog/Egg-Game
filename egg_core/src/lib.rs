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

pub mod animation;
pub mod camera;
pub mod drawstate;
pub mod data;
pub mod debug;
pub mod dialogue;
pub mod gamestate;
pub mod interact;
pub mod map;
pub mod particles;
pub mod player;
pub mod position;
pub mod rand;
pub mod system;
pub mod ui;

use crate::debug::DebugInfo;
use crate::drawstate::DrawState;
use crate::gamestate::GameMode;
use crate::gamestate::inventory::InventoryUi;
use crate::gamestate::walkaround::WalkaroundState;
use crate::map::MapStore;

pub struct EggState {
    pub draw_state: DrawState,
    pub gamestate: GameMode,
    pub walkaround: WalkaroundState,
    pub debug_info: DebugInfo,
    pub time: i32,
    pub inventory_ui: InventoryUi,
    /// Every loaded Tiled map by name — the tile data the game draws,
    /// collides against and edits. The host fills it at asset-load time.
    pub maps: MapStore,
}
impl EggState {
    pub fn run(&mut self, system: &mut impl system::ConsoleApi) {
        self.time += 1;
        self.gamestate.run(
            &mut self.walkaround,
            &mut self.debug_info,
            self.time,
            &mut self.inventory_ui,
            &mut self.draw_state,
            &mut self.maps,
            system,
        );
    }
}
impl Default for EggState {
    fn default() -> Self {
        EggState {
            draw_state: DrawState::default(),
            walkaround: WalkaroundState::new(),
            inventory_ui: InventoryUi::new(),
            gamestate: GameMode::Animation(0),
            time: 0,
            debug_info: DebugInfo::default(),
            maps: MapStore::default(),
        }
    }
}

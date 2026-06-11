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

use crate::data::script::Script;
use crate::debug::DebugInfo;
use crate::dialogue::Message;
use crate::drawstate::DrawState;
use crate::gamestate::GameMode;
use crate::gamestate::inventory::InventoryUi;
use crate::gamestate::walkaround::WalkaroundState;
use crate::map::MapStore;
use crate::rand::Lcg64Xsh32;
use crate::system::ConsoleApi;

/// The shared world every game state steps and draws against — the layer
/// canvases, the console, the loaded maps, and the loaded text — passed as one
/// parameter so gamestate signatures stop growing element-by-element. Game-data
/// helpers (labels, dialogue lookups) accumulate here as the console shrinks to
/// a hardware-only surface. Deliberately lean: per-state things (`InventoryUi`,
/// `DebugInfo`, `elapsed_frames`) stay explicit parameters of the few methods
/// that need them.
pub struct Ctx<'a, S: ConsoleApi> {
    pub draw: &'a mut DrawState,
    pub system: &'a mut S,
    pub maps: &'a mut MapStore,
    /// The game's pseudo-random generator. Lives on [`EggState`] (not the
    /// console) so randomness is a piece of game state, not a hardware service.
    pub rng: &'a mut Lcg64Xsh32,
    /// The loaded UI labels + dialogue. Read-only here: gameplay only reads
    /// text, while the host installs the base script and swaps languages by
    /// mutating [`EggState::script`] directly (see [`EggState::set_language`]).
    pub script: &'a Script,
}

impl<S: ConsoleApi> Ctx<'_, S> {
    /// A UI label by key (see [`Script::label`]).
    pub fn label(&self, key: &str) -> String {
        self.script.label(key)
    }

    /// An ordered string list by key (see [`Script::list`]).
    pub fn list(&self, key: &str) -> Vec<String> {
        self.script.list(key)
    }

    /// A dialogue conversation by key (see [`Script::get_dialogue`]).
    pub fn get_dialogue(&self, key: &str) -> Vec<Message> {
        self.script.get_dialogue(key)
    }
}

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
    /// The game's RNG, threaded into every state through [`Ctx::rng`].
    pub rng: Lcg64Xsh32,
    /// The loaded UI labels + dialogue, threaded into every state through
    /// [`Ctx::script`]. The host installs the base language at asset-load time
    /// (`set_base`) and applies runtime language switches (`set_language`) by
    /// mutating this directly; gameplay only ever reads it.
    pub script: Script,
    /// A language requested at runtime via [`EggState::set_language`], awaiting
    /// load by the host's asset loop (see [`EggState::take_pending_language`]).
    pending_language: Option<String>,
}
impl EggState {
    pub fn run(&mut self, system: &mut impl system::ConsoleApi) {
        self.time += 1;
        let mut ctx = Ctx {
            draw: &mut self.draw_state,
            system,
            maps: &mut self.maps,
            rng: &mut self.rng,
            script: &self.script,
        };
        self.gamestate.run(
            &mut ctx,
            &mut self.walkaround,
            &mut self.inventory_ui,
            &mut self.debug_info,
            self.time,
        );
    }
    /// Request switching the active language at runtime. The host's asset loop
    /// drains the request via [`take_pending_language`](Self::take_pending_language),
    /// loads the matching script file, and applies it to [`script`](Self::script).
    /// (No game-code callers yet — plumbing for a future language menu.)
    pub fn set_language(&mut self, language: &str) {
        self.pending_language = Some(language.to_string());
    }
    /// Take any language requested at runtime via [`set_language`](Self::set_language),
    /// for the host's asset loop to load and apply.
    pub fn take_pending_language(&mut self) -> Option<String> {
        self.pending_language.take()
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
            rng: Lcg64Xsh32::default(),
            script: Script::new(),
            pending_language: None,
        }
    }
}

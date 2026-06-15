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

use crate::data::eggscene::{CutsceneDef, SceneFile};
use crate::data::save::{SAVE_PATH, SaveData};
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
    /// The loaded cutscene registry (a SEPARATE, language-independent file —
    /// `assets/script/main.eggscene`). Held apart from [`script`](Self::script)
    /// because choreography is not translated; a `dialogue` *step* still refers
    /// to a script key resolved at play time. Read-only here.
    pub scenes: &'a SceneFile,
    /// Persistent progress. Gameplay reads and writes it freely; the engine
    /// flushes it to the host's file store at the end of each frame (see
    /// [`EggState::flush_save`]), so save persistence is a piece of game state,
    /// not a hardware service.
    pub save: &'a mut SaveData,
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

    /// A dialogue conversation by key, resolved against the live save so its
    /// `#if` branches pick by the player's flags (see [`Script::get_dialogue`]).
    pub fn get_dialogue(&self, key: &str) -> Vec<Message> {
        self.script.get_dialogue(key, self.save)
    }

    /// A cutscene definition by name from the loaded registry, or `None` if
    /// undefined (see [`SceneFile::get_cutscene`]). The walkaround builds a
    /// playable cutscene from this when a `cutscene` map object fires.
    pub fn get_cutscene(&self, name: &str) -> Option<&CutsceneDef> {
        self.scenes.get_cutscene(name)
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
    /// The loaded cutscene registry, threaded into every state through
    /// [`Ctx::scenes`]. The host installs it at asset-load time from
    /// `assets/script/main.eggscene` (see [`EggState::set_scenes`]); it is a
    /// single, language-independent file, so unlike [`script`](Self::script) it
    /// has no per-language overlay. Gameplay only reads it.
    pub scenes: SceneFile,
    /// A language requested at runtime via [`EggState::set_language`], awaiting
    /// load by the host's asset loop (see [`EggState::take_pending_language`]).
    pending_language: Option<String>,
    /// Persistent progress, threaded into every state through [`Ctx::save`].
    /// Loaded once from the host's file store (see [`load_save`](Self::load_save))
    /// and autosaved at the end of each frame (see [`flush_save`](Self::flush_save)).
    pub save: SaveData,
    /// False until [`load_save`](Self::load_save) has read the persisted save
    /// once; it guards that read so a frame's edits aren't clobbered by a
    /// reload on the next frame.
    save_loaded: bool,
    /// The last [`SaveData`] flushed to storage. [`flush_save`](Self::flush_save)
    /// diffs the live save against this so it only writes when something changed.
    last_flushed_save: SaveData,
}
impl EggState {
    pub fn run(&mut self, system: &mut impl system::ConsoleApi) {
        // Pull the persisted save in before any state reads it, and flush it out
        // after every state has had a chance to mutate it — so the same frame
        // that changes progress also writes it, and exit-time saving needs no
        // special host hook.
        self.load_save(system);
        self.time += 1;
        let mut ctx = Ctx {
            draw: &mut self.draw_state,
            system,
            maps: &mut self.maps,
            rng: &mut self.rng,
            script: &self.script,
            scenes: &self.scenes,
            save: &mut self.save,
        };
        self.gamestate.run(
            &mut ctx,
            &mut self.walkaround,
            &mut self.inventory_ui,
            &mut self.debug_info,
            self.time,
        );
        self.flush_save(system);
    }

    /// Load the persisted save from the host's file store, once. Mirrors the
    /// old host loader's tone: a missing/unreadable/garbage save logs and falls
    /// back to the existing (default) `save`. Either way the last-flushed copy
    /// is seeded so the first [`flush_save`](Self::flush_save) doesn't rewrite
    /// an unchanged file.
    pub fn load_save(&mut self, system: &mut impl system::ConsoleApi) {
        if self.save_loaded {
            return;
        }
        self.save_loaded = true;
        if let Some(bytes) = system.read_file(SAVE_PATH) {
            match serde_json::from_slice(&bytes) {
                Ok(data) => self.save = data,
                Err(e) => log::error!("Failed to parse save ({SAVE_PATH}): {e}"),
            }
        }
        self.last_flushed_save = self.save.clone();
    }

    /// Flush the save to the host's file store when it differs from the last
    /// value written. A serialisation failure logs and skips — a failed save
    /// never crashes the game.
    pub fn flush_save(&mut self, system: &mut impl system::ConsoleApi) {
        if self.save == self.last_flushed_save {
            return;
        }
        match serde_json::to_string_pretty(&self.save) {
            Ok(json) => {
                system.write_file(SAVE_PATH, json.as_bytes());
                self.last_flushed_save = self.save.clone();
            }
            Err(e) => log::error!("Failed to serialise save data: {e}"),
        }
    }
    /// Install the loaded cutscene registry (parsed from
    /// `assets/script/main.eggscene`). Called once at startup by the host's asset
    /// loop, and again when the file is re-saved in-editor — mirroring
    /// [`Script::set_base`] for dialogue.
    pub fn set_scenes(&mut self, scenes: SceneFile) {
        self.scenes = scenes;
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
            scenes: SceneFile::default(),
            pending_language: None,
            save: SaveData::default(),
            save_loaded: false,
            last_flushed_save: SaveData::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::save::SAVE_PATH;
    use crate::system::test_console::TestConsole;

    /// `flush_save` writes the save (as pretty JSON, under [`SAVE_PATH`]) only
    /// when it differs from the last flush — an unchanged save is a no-op so the
    /// per-frame flush doesn't rewrite the file constantly.
    #[test]
    fn flush_save_writes_on_change_skips_when_unchanged() {
        let mut console = TestConsole::new();
        let mut state = EggState::default();

        // A fresh, unchanged save (matching last_flushed) writes nothing.
        state.flush_save(&mut console);
        assert!(!console.files.contains_key(SAVE_PATH));

        // After a change, the next flush writes parseable JSON.
        state.save.egg_count = 7;
        state.flush_save(&mut console);
        let bytes = console.files.get(SAVE_PATH).expect("flush wrote the save");
        let written: SaveData = serde_json::from_slice(bytes).expect("valid json");
        assert_eq!(written.egg_count, 7);

        // No further change -> no rewrite (clear the file, flush, stays absent).
        console.files.remove(SAVE_PATH);
        state.flush_save(&mut console);
        assert!(!console.files.contains_key(SAVE_PATH));
    }

    /// `load_save` installs a valid pre-existing file and runs once; garbage in
    /// the store logs and leaves the default save in place.
    #[test]
    fn load_save_installs_valid_file_and_falls_back_on_garbage() {
        // Valid file -> installed into `save`.
        let mut console = TestConsole::new();
        let stored = SaveData { egg_count: 42, ..SaveData::default() };
        console.files.insert(
            SAVE_PATH.to_string(),
            serde_json::to_vec(&stored).unwrap(),
        );
        let mut state = EggState::default();
        state.load_save(&mut console);
        assert_eq!(state.save.egg_count, 42);

        // The guard makes it run once: a later store change isn't re-read.
        console
            .files
            .insert(SAVE_PATH.to_string(), serde_json::to_vec(&SaveData::default()).unwrap());
        state.load_save(&mut console);
        assert_eq!(state.save.egg_count, 42);

        // Garbage bytes -> fall back to the existing default save.
        let mut console = TestConsole::new();
        console
            .files
            .insert(SAVE_PATH.to_string(), b"not json".to_vec());
        let mut state = EggState::default();
        state.load_save(&mut console);
        assert_eq!(state.save, SaveData::default());
    }
}

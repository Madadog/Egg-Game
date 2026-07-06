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

pub mod debug;
pub mod gamestate;
/// The primitives crate ([`egg_render`]) re-exported under its historical
/// in-crate paths, so `crate::geometry::…` / `crate::render::…` (and the
/// host's `egg_core::geometry`/`egg_core::render`) keep resolving after the
/// extraction. `geometry` is a submodule of that crate; `render` is the crate
/// root itself.
pub use egg_render as render;
pub use egg_render::geometry;
/// The console-abstraction crate ([`egg_platform`]) re-exported under its
/// historical in-crate path, so `crate::platform::…` (and the host's
/// `egg_core::platform::…`) keep resolving after the extraction. The host-facing
/// sound value types it now owns are re-exported by [`data::sound`] under their
/// old paths too.
pub use egg_platform as platform;
/// The game-domain crate ([`egg_world`]) re-exported under its historical
/// in-crate module paths, so `crate::data::…`, `crate::world::…`,
/// `crate::draw_state::…` and `crate::rand::…` (and the host's
/// `egg_core::data`/`world`/`draw_state`/`rand`) keep resolving after the
/// extraction. Parsing stays fenced in [`data`]; `draw_state` lives down there
/// because `ui` + `editor` both consume it while `render` stays primitives-only.
pub use egg_world::{data, draw_state, rand, world};
/// The retained-layout UI crate ([`egg_ui`]) re-exported under its historical
/// in-crate module path, so `crate::ui::…` (and the host's `egg_core::ui::…`)
/// keep resolving after the extraction. The toolkit — the Taffy-backed
/// [`layout`](egg_ui::layout), the shared [`text_field`](egg_ui::text_field),
/// and the [`dialogue`](egg_ui::dialogue) box + its [`portrait`](egg_ui::portrait)
/// renderer — sits above `render`/`platform` and reads the game's `draw_state` +
/// text data; the `GameMode` screens that drive it stay up here and the
/// [`editor`] that also drives it sits alongside it.
pub use egg_ui as ui;
/// The in-game dev-tooling crate ([`egg_editor`]) re-exported under its
/// historical in-crate module path, so `crate::editor::…` (and the host's
/// `egg_core::editor::…`) keep resolving after the extraction. Top of the stack —
/// the [`map`](egg_editor::map) editor (the `MapViewer` + its dock UI) and the
/// raw [`text`](egg_editor::text) editor for the `.eggtext`/`.eggscene` script
/// files — it reads every lower crate but nothing in the engine depends on it;
/// `WalkaroundState` still owns the `MapViewer` it drives (ownership inversion
/// deferred until the server/client design).
pub use egg_editor as editor;

use crate::data::eggdata::{GameItems, Presets};
use crate::data::save::{SAVE_PATH, SaveData};
use crate::data::scene::{CutsceneDef, SceneFile};
use crate::data::script::Script;
use crate::data::script::message::Message;
use crate::debug::DebugInfo;
use crate::draw_state::DrawState;
use crate::data::scene::ScrubRequest;
use crate::gamestate::walkaround::WalkaroundState;
use crate::gamestate::{CutsceneScrubber, GameMode, Instructions, IntroAnimation, MenuState, SpriteTest};
use crate::platform::{ConsoleApi, EggInput};
use crate::rand::Lcg64Xsh32;
use crate::render::{
    Canvas, Font, PrintOptions, print_to_centered_with_font, print_to_shadow_with_font,
    print_to_with_font, text_width,
};
use crate::world::map::MapStore;

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
    /// A whole frame's input, threaded in as data rather than pulled through the
    /// console — the host decides which window's input a step sees (see
    /// [`EggInput`](crate::platform::EggInput)). Read-only: a step observes the
    /// frame's edges/held state, it never advances them (the host owns the
    /// per-window `refresh` cadence).
    pub input: &'a EggInput,
    pub maps: &'a mut MapStore,
    /// The game's pseudo-random generator. Lives on [`EggState`] (not the
    /// console) so randomness is a piece of game state, not a hardware service.
    pub rng: &'a mut Lcg64Xsh32,
    /// The loaded UI labels + dialogue. Read-only here: gameplay only reads
    /// text, while the host installs the base script and swaps languages by
    /// mutating [`EggState::script`] directly (see [`EggState::set_language`]).
    pub script: &'a Script,
    /// The loaded cutscene registry (a SEPARATE, language-independent file —
    /// `assets/data/main.eggscene`). Held apart from [`script`](Self::script)
    /// because choreography is not translated; a `dialogue` *step* still refers
    /// to a script key resolved at play time. Read-only here.
    pub scenes: &'a SceneFile,
    /// Persistent progress. Gameplay reads and writes it freely; the engine
    /// flushes it to the host's file store at the end of each frame (see
    /// [`EggState::flush_save`]), so save persistence is a piece of game state,
    /// not a hardware service.
    pub save: &'a mut SaveData,
    /// The loaded item registry (sprite per item key). Loaded game data like
    /// [`maps`](Self::maps)/[`script`](Self::script); read-only here.
    pub items: &'a GameItems,
    /// The loaded creature registry (preset defs by [`PresetId`](crate::world::player::PresetId)).
    /// Loaded game data like [`items`](Self::items); read-only here.
    pub presets: &'a Presets,
    /// The loaded bitmap [`Font`], threaded in as game data rather than a console
    /// service. The text-drawing convenience methods on `Ctx` ([`print_to`] &c.)
    /// render with it; a headless console needs no font at all.
    ///
    /// [`print_to`]: Ctx::print_to
    pub font: &'a Font,
}

impl<S: ConsoleApi> Ctx<'_, S> {
    /// Render `text` onto `target` with the loaded [`font`](Self::font). `colour`
    /// is the pixel value for non-transparent font pixels; the glyph atlas is read
    /// alpha-only. To measure without drawing, use [`text_width`](Self::text_width).
    pub fn print_to<C: Canvas>(
        &self,
        target: &mut C,
        text: &str,
        x: i32,
        y: i32,
        colour: C::Pixel,
        opts: PrintOptions,
    ) {
        print_to_with_font(self.font, target, text, x, y, colour, opts);
    }

    /// Render `text` horizontally centred on `x` (measured with the loaded font).
    pub fn print_to_centered<C: Canvas>(
        &self,
        target: &mut C,
        text: &str,
        x: i32,
        y: i32,
        colour: C::Pixel,
        opts: PrintOptions,
    ) {
        print_to_centered_with_font(self.font, target, text, x, y, colour, opts);
    }

    /// Render `text` with a one-pixel drop shadow (`shadow` at `+1/+1`, then
    /// `colour`).
    #[allow(clippy::too_many_arguments)]
    pub fn print_to_shadow<C: Canvas>(
        &self,
        target: &mut C,
        text: &str,
        x: i32,
        y: i32,
        colour: C::Pixel,
        shadow: C::Pixel,
        opts: PrintOptions,
    ) {
        print_to_shadow_with_font(self.font, target, text, x, y, colour, shadow, opts);
    }

    /// Measure `text`'s pixel width in the loaded font without drawing it.
    pub fn text_width(&self, text: &str, opts: PrintOptions) -> i32 {
        text_width(self.font, text, opts)
    }

    /// A UI label by key (see [`Script::label`]).
    pub fn label(&self, key: &str) -> String {
        self.script.label(key)
    }

    /// An ordered string list by key (see [`Script::list`]).
    pub fn list(&self, key: &str) -> Vec<String> {
        self.script.list(key)
    }

    /// An inventory item's display name — element 0 of its `item_<key>` list.
    pub fn item_name(&self, key: &str) -> String {
        self.list(&format!("item_{key}"))
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    /// An inventory item's description — element 1 of its `item_<key>` list.
    pub fn item_desc(&self, key: &str) -> String {
        self.list(&format!("item_{key}"))
            .into_iter()
            .nth(1)
            .unwrap_or_default()
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
    /// The startup intro animation's state (mode [`GameMode::Animation`]).
    pub intro: IntroAnimation,
    /// The startup instructions screen's state (mode [`GameMode::Instructions`]).
    pub instructions: Instructions,
    /// The sprite-test debug screen's state (mode [`GameMode::SpriteTest`]).
    pub sprite_test: SpriteTest,
    /// The shared menu, driven by the four menu modes ([`GameMode::MainMenu`] &c.);
    /// [`enter`](Self::enter) rebuilds it to the right flavor on entry.
    pub menu: MenuState,
    /// Every loaded Tiled map by name — the tile data the game draws,
    /// collides against and edits. The host fills it at asset-load time.
    pub maps: MapStore,
    /// The loaded item registry (sprite per item key), threaded into every state
    /// through [`Ctx::items`]. Loaded from `data.toml` at boot (see
    /// [`load_data`](Self::load_data)); [`GameItems::default`] is the fallback.
    pub items: GameItems,
    /// The loaded creature registry (preset defs), threaded into every state via
    /// [`Ctx::presets`]. Defaults to the embedded built-ins ([`Presets::builtin`]);
    /// [`load_data`](Self::load_data) re-derives it from the runtime `data.toml`.
    pub presets: Presets,
    /// The loaded bitmap [`Font`], threaded into every state through [`Ctx::font`].
    /// Starts blank ([`Font::blank`]); the host installs the real glyph atlas at
    /// asset-load time via [`set_font`](Self::set_font). Game data, not a console
    /// service — so a headless console can carry no font of its own.
    pub font: Font,
    /// The game's RNG, threaded into every state through [`Ctx::rng`].
    pub rng: Lcg64Xsh32,
    /// The loaded UI labels + dialogue, threaded into every state through
    /// [`Ctx::script`]. The host installs the base language at asset-load time
    /// (`set_base`) and applies runtime language switches (`set_language`) by
    /// mutating this directly; gameplay only ever reads it.
    pub script: Script,
    /// The loaded cutscene registry, threaded into every state through
    /// [`Ctx::scenes`]. The host installs it at asset-load time from
    /// `assets/data/main.eggscene` (see [`EggState::set_scenes`]); it is a
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
    /// False until [`load_data`](Self::load_data) has installed the game-data
    /// file (the item registry) once; guards that one-time read like `save_loaded`.
    data_loaded: bool,
    /// The last [`SaveData`] flushed to storage. [`flush_save`](Self::flush_save)
    /// diffs the live save against this so it only writes when something changed.
    last_flushed_save: SaveData,
    /// The open cutscene scrubber, if any (see [`CutsceneScrubber`]). A fullscreen
    /// editor modal: while it's `Some`, [`step_mode`](Self::step_mode) drives + draws
    /// it and skips the normal sim. Opened via the editor's `pending_scrub` request.
    pub scrubber: Option<CutsceneScrubber>,
}
impl EggState {
    pub fn run(&mut self, system: &mut impl platform::ConsoleApi, input: &EggInput) {
        // Pull the persisted save in before any state reads it, and flush it out
        // after every state has had a chance to mutate it — so the same frame
        // that changes progress also writes it, and exit-time saving needs no
        // special host hook.
        // Install the game-data file (item registry) before the save's inventory
        // is rehydrated against it below, so saved keys validate against it.
        self.load_data(system);
        let loaded = self.load_save(system);
        // On the one frame the save is read from storage, rebuild the live
        // inventory from its persisted item keys (the inverse of the sync before
        // `flush_save` below), so what the player picked up last run is restored
        // before any state draws the inventory.
        if loaded {
            self.walkaround
                .load_inventory(&self.save.inventory, &self.items);
        }
        self.time += 1;
        if let Some(mode) = self.step_mode(system, input) {
            self.enter(mode);
        }
        // Serialise the live inventory into the save before it is flushed, so an
        // item the player gained, dropped or reordered this frame persists (the
        // inverse of `load_from_save` after `load_save` above).
        self.save.inventory = self.walkaround.snapshot_inventory();
        self.flush_save(system);
    }

    /// Step the active [`GameMode`], dispatching to that mode's state (its `step`,
    /// and draw where the mode owns one) and returning any requested transition.
    /// Each mode's state is a field on `self`; the four menu variants all drive
    /// the shared [`menu`](Self::menu).
    fn step_mode(
        &mut self,
        system: &mut impl platform::ConsoleApi,
        input: &EggInput,
    ) -> Option<GameMode> {
        // The cutscene scrubber is a fullscreen modal over everything: drive +
        // draw it and skip the normal sim while a session is open.
        if self.scrubber.is_some() {
            self.drive_scrubber(system, input);
            return None;
        }
        let transition = {
            let mut ctx = Ctx {
                draw: &mut self.draw_state,
                system,
                input,
                maps: &mut self.maps,
                rng: &mut self.rng,
                script: &self.script,
                scenes: &self.scenes,
                save: &mut self.save,
                items: &self.items,
                presets: &self.presets,
                font: &self.font,
            };
            match self.gamestate {
                GameMode::Instructions => self.instructions.step(&mut ctx, &mut self.walkaround),
                GameMode::Walkaround => {
                    let next = self.walkaround.step(&mut ctx);
                    self.walkaround.draw(&mut ctx, &self.debug_info);
                    next
                }
                GameMode::Animation => self.intro.step(&mut ctx),
                // Every menu flavour shares this one dispatch — `enter` is what
                // makes them differ. `InventoryOptions` is the bag's Options page
                // (a menu, reached from the overlay), NOT a second inventory
                // route: the bag itself is stepped/drawn only under `Walkaround`,
                // as an overlay the walkaround owns (see `step_inventory`).
                GameMode::MainMenu
                | GameMode::InventoryOptions
                | GameMode::DebugMenu
                | GameMode::MapSelect => {
                    let next = self.menu.step_main_menu(&mut ctx, &mut self.walkaround);
                    self.menu.draw_main_menu(&mut ctx, self.time);
                    next
                }
                GameMode::SpriteTest => self.sprite_test.step(&mut ctx),
            }
        };
        // The map editor can request a scrubber (the `P` shortcut, or save-and-
        // play in the recorder); open it here, where the full Ctx + registry are
        // in reach. A recorded def opens directly — no registry round-trip.
        if let Some(req) = self.walkaround.map_viewer.pending_scrub.take() {
            match req {
                ScrubRequest::ByName(name) => self.open_scrubber(&name),
                ScrubRequest::Recorded(name, def) => self.open_scrubber_def(name, def),
            }
        }
        // A walk-sprite editor save rewrote `data.toml`: re-install the live
        // item/preset registries from the store so the next spawn uses the edit
        // (works on web too, where no mtime watcher will notice the write).
        if self.walkaround.map_viewer.pending_data_reload {
            self.walkaround.map_viewer.pending_data_reload = false;
            self.reload_data(system);
        }
        transition
    }

    /// Switch to `mode` and (re)initialise its state. Transient modes reset on
    /// entry — the menu rebuilds to the flavor the variant names; the intro,
    /// instructions and sprite-test counters restart — while the persistent world
    /// and inventory are left untouched. The canonical way to change mode from
    /// outside (e.g. a host debug hotkey), so the target's state is set up.
    pub fn enter(&mut self, mode: GameMode) {
        self.gamestate = mode;
        match mode {
            GameMode::Animation => self.intro = IntroAnimation::default(),
            GameMode::Instructions => self.instructions = Instructions::default(),
            GameMode::SpriteTest => self.sprite_test = SpriteTest::default(),
            GameMode::MainMenu => self.menu = MenuState::new(),
            GameMode::InventoryOptions => self.menu = MenuState::inventory_options(),
            GameMode::DebugMenu => self.menu = MenuState::debug_options(&self.script),
            GameMode::MapSelect => self.menu = MenuState::map_select(&self.maps),
            GameMode::Walkaround => {}
        }
    }

    /// Load the persisted save from the host's file store, once. Mirrors the
    /// old host loader's tone: a missing/unreadable/garbage save logs and falls
    /// back to the existing (default) `save`. Either way the last-flushed copy
    /// is seeded so the first [`flush_save`](Self::flush_save) doesn't rewrite
    /// an unchanged file. Returns `true` on the one call that actually performs
    /// the read, so [`run`](Self::run) can rebuild the live inventory from the
    /// freshly loaded save exactly once.
    pub fn load_save(&mut self, system: &mut impl platform::ConsoleApi) -> bool {
        if self.save_loaded {
            return false;
        }
        self.save_loaded = true;
        if let Some(bytes) = system.read_file(SAVE_PATH) {
            // `from_json` (not a bare `from_slice`) so a save written with the old
            // `is_night` bool migrates that state onto the modern flag on load.
            match SaveData::from_json(&bytes) {
                Ok(data) => self.save = data,
                Err(e) => log::error!("Failed to parse save ({SAVE_PATH}): {e}"),
            }
        }
        self.last_flushed_save = self.save.clone();
        true
    }

    /// Load the game-data file (`assets/data/data.toml`) from the host's file
    /// store, once, installing the item registry it defines (a full replace, so
    /// the file is the source of truth). Mirrors [`load_save`](Self::load_save):
    /// a missing file is ignored silently and a malformed one logs, either way
    /// leaving the built-in [`GameItems::default`] in place. Called at the top of
    /// [`run`](Self::run) so the registry is ready before the save's inventory is
    /// rehydrated against it.
    ///
    /// Cross-platform note: on native the host serves asset paths through the
    /// file store, so this reads the real file; on web the file store is
    /// user-data only (`localStorage`), so the read returns nothing and the
    /// built-in default stands. Reading this asset on web means routing it
    /// through the engine's async asset pipeline (as the script/maps do) — a
    /// follow-up, harmless while the shipped file matches the default.
    pub fn load_data(&mut self, system: &mut impl platform::ConsoleApi) {
        use crate::data::eggdata;
        if self.data_loaded {
            return;
        }
        self.data_loaded = true;
        let Some(bytes) = system.read_file(eggdata::DATA_PATH) else {
            return;
        };
        match std::str::from_utf8(&bytes)
            .map_err(|e| e.to_string())
            .and_then(|s| eggdata::parse(s).map_err(|e| e.to_string()))
        {
            Ok(data) => {
                self.items = GameItems::from_data(&data.items);
                self.presets = eggdata::Presets::from_data(&data);
            }
            Err(e) => log::error!("Failed to parse game data ({}): {e}", eggdata::DATA_PATH),
        }
    }

    /// Re-load `data.toml` from the host's file store, *bypassing* the once-guard
    /// [`load_data`](Self::load_data) honours, so an external edit picked up by the
    /// host's native hot-reload re-installs the item/preset registries. Same
    /// parse+install and last-good-wins error semantics as `load_data` (a
    /// missing/malformed file leaves the current registries untouched).
    pub fn reload_data(&mut self, system: &mut impl platform::ConsoleApi) {
        self.data_loaded = false;
        self.load_data(system);
    }

    /// Flush the save to the host's file store when it differs from the last
    /// value written. A serialisation failure logs and skips — a failed save
    /// never crashes the game.
    pub fn flush_save(&mut self, system: &mut impl platform::ConsoleApi) {
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
    /// `assets/data/main.eggscene`). Called once at startup by the host's asset
    /// loop, and again when the file is re-saved in-editor — mirroring
    /// [`Script::set_base`] for dialogue.
    pub fn set_scenes(&mut self, scenes: SceneFile) {
        self.scenes = scenes;
    }
    /// Install the loaded bitmap [`Font`] (the glyph atlas the host built from its
    /// font image). Called once at asset-load time; text drawing reads it through
    /// [`Ctx::font`].
    pub fn set_font(&mut self, font: Font) {
        self.font = font;
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
            intro: IntroAnimation::default(),
            instructions: Instructions::default(),
            sprite_test: SpriteTest::default(),
            menu: MenuState::new(),
            gamestate: GameMode::Animation,
            time: 0,
            debug_info: DebugInfo::default(),
            maps: MapStore::default(),
            items: GameItems::default(),
            presets: Presets::builtin(),
            font: Font::blank(),
            rng: Lcg64Xsh32::default(),
            script: Script::new(),
            scenes: SceneFile::default(),
            pending_language: None,
            save: SaveData::default(),
            save_loaded: false,
            data_loaded: false,
            last_flushed_save: SaveData::default(),
            scrubber: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::save::SAVE_PATH;
    use crate::platform::test_console::TestConsole;

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
        let stored = SaveData {
            egg_count: 42,
            ..SaveData::default()
        };
        console
            .files
            .insert(SAVE_PATH.to_string(), serde_json::to_vec(&stored).unwrap());
        let mut state = EggState::default();
        state.load_save(&mut console);
        assert_eq!(state.save.egg_count, 42);

        // The guard makes it run once: a later store change isn't re-read.
        console.files.insert(
            SAVE_PATH.to_string(),
            serde_json::to_vec(&SaveData::default()).unwrap(),
        );
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

    /// `load_data` reads the game-data file from the host store and installs its
    /// item registry over the built-in default (a full replace), runs once, and
    /// leaves the default in place when the file is absent or malformed.
    #[test]
    fn load_data_installs_items_and_runs_once() {
        use crate::data::eggdata::DATA_PATH;

        // No file: the built-in default (ff/lm/chegg) stays.
        let mut console = TestConsole::new();
        let mut state = EggState::default();
        state.load_data(&mut console);
        assert!(state.items.contains("ff"));
        assert!(!state.items.contains("widget"));

        // A valid file replaces the registry with exactly what it defines.
        let mut console = TestConsole::new();
        console.files.insert(
            DATA_PATH.to_string(),
            b"[items.widget]\nsprite = 700\n".to_vec(),
        );
        let mut state = EggState::default();
        state.load_data(&mut console);
        assert_eq!(state.items.get("widget").map(|d| d.sprite), Some(700));
        // The file is the source of truth: a default it omits is gone.
        assert!(!state.items.contains("ff"));

        // The guard makes it run once: a later store change isn't re-read.
        console
            .files
            .insert(DATA_PATH.to_string(), b"[items.other]\nsprite = 1\n".to_vec());
        state.load_data(&mut console);
        assert!(!state.items.contains("other"));

        // Malformed bytes -> fall back to the default registry (logs, no panic).
        let mut console = TestConsole::new();
        console
            .files
            .insert(DATA_PATH.to_string(), b"items = [not a table]".to_vec());
        let mut state = EggState::default();
        state.load_data(&mut console);
        assert!(state.items.contains("ff"));
    }

    /// The inventory survives a full save→load cycle through the real sync
    /// points (`run`'s serialise-before-flush and repopulate-after-load): fill
    /// the live inventory, flush it to the store, clear it on a fresh state, load
    /// it back, and the slot contents (by item key) are identical. Exercises the
    /// `Inventory::to_save`/`load_from_save` round trip through
    /// [`SaveData::inventory`] and the host's file store via [`TestConsole`].
    #[test]
    fn inventory_round_trips_through_save_and_load() {
        use crate::gamestate::walkaround::inventory::Inventory;

        let mut console = TestConsole::new();

        // A populated inventory (the default three starting items) is the thing
        // we expect to find again after a save→clear→load cycle.
        let mut source = EggState::default();
        source.load_save(&mut console); // no file: leaves the default save.
        // Grant a fourth item so the live inventory differs from the default
        // save (whose `inventory` default is the three starting items) — this is
        // what makes the diff-gated `flush_save` below actually write a file.
        assert!(source.walkaround.inventory_ui.inventory.add("ff".into()));
        let filled = source.walkaround.inventory_ui.inventory.to_save();
        assert_ne!(
            filled,
            [const { None }; 8],
            "the populated inventory has items to persist"
        );

        // Serialise-before-flush (the line `run` performs) then write to storage.
        source.save.inventory = source.walkaround.inventory_ui.inventory.to_save();
        source.flush_save(&mut console);
        assert!(console.files.contains_key(SAVE_PATH), "save was written");

        // A fresh state with a *cleared* inventory loads the stored save and
        // repopulates from it (the lines `run` performs on the load frame).
        let mut loaded = EggState::default();
        loaded.walkaround.inventory_ui.inventory = Inventory {
            items: [const { None }; 8],
        };
        let did_load = loaded.load_save(&mut console);
        assert!(did_load, "the stored save is read once");
        loaded
            .walkaround
            .inventory_ui
            .inventory
            .load_from_save(&loaded.save.inventory, &loaded.items);

        // The reloaded slots match the originals item-for-item.
        assert_eq!(loaded.walkaround.inventory_ui.inventory.to_save(), filled);
    }

    /// Erasing the save (the Options menu's "lose data" action) resets the LIVE
    /// inventory back to the default starting items — not just `SaveData`. This is
    /// the regression: the live inventory lives on `WalkaroundState.inventory_ui`,
    /// which the erase used to leave alone, and `run` re-syncs it into the save at
    /// the end of every frame, so a stale inventory would be written straight back
    /// over the freshly-erased default. Drives the real erase through the public
    /// [`MenuState::click`] path (Options → Reset → confirm), then runs `run`'s own
    /// serialise-before-flush + `flush_save`, and asserts the live inventory, the
    /// in-memory save and the on-disk save are all the defaults.
    #[test]
    fn erase_resets_live_inventory_to_defaults() {
        // `MenuState`/`GameMode` are already imported at module scope (the `use
        // super::*;` above pulls in the parent's `gamestate::{MenuState, …}`).
        let mut console = TestConsole::new();
        let mut state = EggState::default();
        state.load_save(&mut console); // no file: leaves the default save.

        // Dirty the live inventory and some other progress, so the erase has
        // something to actually undo and the post-erase flush has a diff to write.
        assert!(state.walkaround.inventory_ui.inventory.add("ff".into()));
        assert!(state.walkaround.inventory_ui.inventory.add("lm".into()));
        state.save.egg_count = 99;
        assert_ne!(
            state.walkaround.inventory_ui.inventory.to_save(),
            SaveData::default().inventory,
            "inventory dirtied for the test"
        );

        // Persist that dirtied state to disk first (as ordinary play would, via
        // `run`'s serialise-before-flush), so a real save file with the old items
        // exists — and so the post-erase flush below has a genuine diff to write
        // (`flush_save` is diff-gated against the last flush).
        state.save.inventory = state.walkaround.inventory_ui.inventory.to_save();
        state.flush_save(&mut console);
        assert!(console.files.contains_key(SAVE_PATH), "dirty save written");

        // Drive the menu through its public `click` API, exactly as `step_mode`
        // does: open Options (installs the sub-screen whose entries end in Reset),
        // then click Reset twice — the first arms the confirm, the second erases.
        // `Options` sits at index 1 of the main menu; `Reset` is the last of the
        // four Options entries (index 3). Each click builds a fresh `Ctx`
        // split-borrowing the same `EggState` fields (it borrows mutably, so it
        // can't outlive the call) and hands the walkaround in alongside (the bag
        // now lives on the walkaround), the way `step_mode` does.
        state.menu = MenuState::new();
        let input = EggInput::new();
        let mut returned = None;
        for index in [1, 3, 3] {
            let mut walk = std::mem::take(&mut state.walkaround);
            let mut menu = std::mem::take(&mut state.menu);
            {
                let mut ctx = Ctx {
                    draw: &mut state.draw_state,
                    system: &mut console,
                    input: &input,
                    maps: &mut state.maps,
                    rng: &mut state.rng,
                    script: &state.script,
                    scenes: &state.scenes,
                    save: &mut state.save,
                    items: &state.items,
                    presets: &state.presets,
                    font: &state.font,
                };
                returned = menu.click(Some(index), &mut ctx, &mut walk);
            }
            state.walkaround = walk;
            state.menu = menu;
        }
        // The erase requests the intro (a fresh game), confirming it fired.
        assert_eq!(returned, Some(GameMode::Animation), "erase fired");

        // The live inventory is back to the default starting items immediately…
        assert_eq!(
            state.walkaround.inventory_ui.inventory.to_save(),
            SaveData::default().inventory,
            "live inventory reset to the starting items"
        );
        // The in-memory save is the default (egg_count cleared as well).
        assert_eq!(state.save, SaveData::default(), "save zeroed");

        // Now run the two lines `run` performs after `step_mode`: serialise the
        // live inventory back into the save, then flush. This is the exact path the
        // bug exploited — with the fix the synced-back inventory is the default, so
        // the persisted save stays erased rather than re-acquiring the old items.
        state.save.inventory = state.walkaround.inventory_ui.inventory.to_save();
        state.flush_save(&mut console);

        assert_eq!(
            state.save.inventory,
            SaveData::default().inventory,
            "post-sync save inventory is the starting items, not the stale ones"
        );
        let bytes = console.files.get(SAVE_PATH).expect("erase flushed to disk");
        let on_disk: SaveData = serde_json::from_slice(bytes).expect("valid json");
        assert_eq!(
            on_disk,
            SaveData::default(),
            "the erased default is what persists across a restart"
        );
    }
}

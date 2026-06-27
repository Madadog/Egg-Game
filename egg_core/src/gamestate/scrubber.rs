//! The cutscene scrubber: an editor-owned, steppable replay of one cutscene.
//!
//! It snapshots the live world + save the moment a scene is opened, arms the
//! scene on that snapshot, and measures its length once. Any playhead frame is
//! then a fresh deterministic re-simulation of the snapshot through a headless
//! [`ScrubConsole`] — the AI/RNG loop is skipped while a cutscene plays, so
//! nothing diverges between runs. The live world and save are never touched.
//!
//! It lives on [`EggState`] (not the map editor) because re-simming needs the
//! full [`Ctx`] — `draw`/`rng`/`scenes`/`presets`/… — that only the run loop
//! assembles; the editor merely *requests* one via `MapViewer::pending_scrub`.

use crate::data::save::SaveData;
use crate::debug::DebugInfo;
use crate::draw_state::LayerId;
use crate::editor::map::MapViewer;
use crate::gamestate::walkaround::WalkaroundState;
use crate::platform::{ConsoleApi, ScanCode, ScrubConsole};
use crate::render::{PrintOptions, print_to_shadow_with_font};
use crate::{Ctx, EggState};

/// Hold ←/→ to scrub: a tap steps one frame; holding past `SCRUB_REPEAT_DELAY`
/// fixed steps then pages every `SCRUB_REPEAT_RATE` steps (so you don't have to
/// tap once per frame). A touch snappier than the text-field repeat — scrubbing
/// wants to start paging sooner.
const SCRUB_REPEAT_DELAY: u16 = 12;
const SCRUB_REPEAT_RATE: u16 = 2;

/// A steppable replay session for one cutscene. Built by
/// [`EggState::open_scrubber`] and driven by [`EggState::drive_scrubber`].
pub struct CutsceneScrubber {
    /// The scene's registry name (shown in the chrome).
    pub name: String,
    /// The world snapshot right after the scene was armed on its stack; every
    /// seek re-sims forward from here.
    base_world: WalkaroundState,
    /// The save snapshot, cloned per re-sim so a replayed `set_flag` can never
    /// reach real progress.
    base_save: SaveData,
    /// Headless console driving the re-sim (neutral input, muted audio).
    console: ScrubConsole,
    /// Current playhead frame and the measured total length.
    frame: usize,
    total: usize,
}

impl EggState {
    /// Open the scrubber on the cutscene named `name`: snapshot the live world +
    /// save, arm the scene on the snapshot, measure its length, and park the
    /// playhead at frame 0. An unknown name logs and no-ops (like a dangling
    /// trigger), leaving any current session untouched.
    pub fn open_scrubber(&mut self, name: &str) {
        let Some(def) = self.scenes.get_cutscene(name).cloned() else {
            log::info!("scrubber: unknown cutscene {name:?}");
            return;
        };
        // Snapshot the live world without the editor overlay or any live scene,
        // so the replay shows a clean world and starts from a known stack.
        let mut base_world = self.walkaround.clone();
        base_world.map_viewer = MapViewer::default();
        base_world.cutscene.clear();
        let base_save = self.save.clone();
        let mut console = ScrubConsole::new();

        // Arm the scene on the snapshot, then measure its length — both on the
        // headless console + a throwaway save so nothing leaks into live state.
        let total = {
            let mut scratch = base_save.clone();
            let mut ctx = Ctx {
                draw: &mut self.draw_state,
                system: &mut console,
                maps: &mut self.maps,
                rng: &mut self.rng,
                script: &self.script,
                scenes: &self.scenes,
                save: &mut scratch,
                items: &self.items,
                presets: &self.presets,
                font: &self.font,
            };
            base_world.arm_cutscene(&def, &mut ctx);
            base_world.measure_cutscene(&mut ctx)
        };

        log::info!("scrubber: opened {name:?} ({total} frames)");
        self.scrubber = Some(CutsceneScrubber {
            name: name.to_string(),
            base_world,
            base_save,
            console,
            frame: 0,
            total,
        });
    }

    /// Drive the open scrubber one host frame: read step/close input from the
    /// real console, re-sim the snapshot to the playhead on the headless one,
    /// and render that frame fullscreen. A no-op if no session is open.
    pub fn drive_scrubber(&mut self, system: &mut impl ConsoleApi) {
        // Take the session out so the re-sim can borrow `self`'s other fields
        // freely; put it back at the end (drop = close).
        let Some(mut scrubber) = self.scrubber.take() else {
            return;
        };

        // Input from the REAL console: Esc/X closes; arrows step the playhead.
        if system.keyp(ScanCode::Escape) || system.keyp(ScanCode::X) {
            return;
        }
        if system.key_repeat(ScanCode::Left, SCRUB_REPEAT_DELAY, SCRUB_REPEAT_RATE) {
            scrubber.frame = scrubber.frame.saturating_sub(1);
        }
        if system.key_repeat(ScanCode::Right, SCRUB_REPEAT_DELAY, SCRUB_REPEAT_RATE) {
            scrubber.frame = (scrubber.frame + 1).min(scrubber.total);
        }

        // Re-sim from the snapshot to the playhead on the headless console + a
        // fresh save clone (so a replayed `set_flag` can't touch real progress).
        let current = {
            let mut scratch = scrubber.base_save.clone();
            let mut ctx = Ctx {
                draw: &mut self.draw_state,
                system: &mut scrubber.console,
                maps: &mut self.maps,
                rng: &mut self.rng,
                script: &self.script,
                scenes: &self.scenes,
                save: &mut scratch,
                items: &self.items,
                presets: &self.presets,
                font: &self.font,
            };
            scrubber.base_world.sim_cutscene_to(scrubber.frame, &mut ctx)
        };

        // Render the ghost fullscreen through the REAL console (sprite assets),
        // with a default unfocused editor (so no editor chrome draws), then the
        // scrubber's own banner over the top.
        let overlay = MapViewer::default();
        {
            let mut ctx = Ctx {
                draw: &mut self.draw_state,
                system,
                maps: &mut self.maps,
                rng: &mut self.rng,
                script: &self.script,
                scenes: &self.scenes,
                save: &mut self.save,
                items: &self.items,
                presets: &self.presets,
                font: &self.font,
            };
            current.draw_world(&mut ctx, current.camera.pos, &overlay, &DebugInfo::default());
            let banner = format!(
                "SCRUBBER  {}   frame {}/{}   [<- ->] step   [X] close",
                scrubber.name, scrubber.frame, scrubber.total
            );
            let fg = ctx.draw.colour(11);
            let shadow = ctx.draw.colour(0);
            print_to_shadow_with_font(
                ctx.font,
                ctx.draw.rgba(LayerId::BG),
                &banner,
                2,
                2,
                fg,
                shadow,
                PrintOptions::default(),
            );
            WalkaroundState::composite_into(ctx.draw, ctx.system.output_image());
        }

        self.scrubber = Some(scrubber);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::scene;

    /// Opening on a known scene measures its length and parks the playhead at 0,
    /// without disturbing the live world.
    #[test]
    fn open_scrubber_measures_and_parks_at_zero() {
        let mut state = EggState::default();
        state.scenes =
            scene::parse("#cutscene town_path\n    move\n        player: walk 30 0 in 10").unwrap();
        let live_pos = state.walkaround.player_ref().pos;

        state.open_scrubber("town_path");

        let s = state.scrubber.as_ref().expect("session opened");
        assert_eq!(s.name, "town_path");
        assert_eq!(s.frame, 0, "playhead parked at the start");
        assert_eq!(s.total, 10, "measured the 10-frame move");
        assert_eq!(
            state.walkaround.player_ref().pos,
            live_pos,
            "the live world is untouched (it replays a snapshot)"
        );
    }

    /// An unknown scene name opens nothing (like a dangling trigger).
    #[test]
    fn open_scrubber_ignores_unknown_scene() {
        let mut state = EggState::default();
        state.open_scrubber("does_not_exist");
        assert!(state.scrubber.is_none(), "no session for an unknown scene");
    }
}

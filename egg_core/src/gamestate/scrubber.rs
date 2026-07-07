//! The cutscene scrubber: an editor-owned, steppable replay of one cutscene.
//!
//! It snapshots the live world + save the moment a scene is opened, arms the
//! scene on that snapshot, and re-simulates it once — capturing its length, its
//! beat offsets, and a [snapshot ladder](CutsceneReplay). Any playhead frame is
//! then a fresh deterministic re-simulation from the nearest ladder rung through
//! a headless [`NullConsole`] — the AI/RNG loop is skipped while a cutscene
//! plays, so nothing diverges between runs. The live world and save are never
//! touched.
//!
//! It lives on [`EggState`] (not the map editor) because re-simming needs the
//! full [`Ctx`] — `draw`/`rng`/`scenes`/`presets`/… — that only the run loop
//! assembles; the editor merely *requests* one via `MapViewer::pending_scrub`.

use crate::data::save::SaveData;
use crate::data::scene::CutsceneDef;
use crate::debug::DebugInfo;
use crate::draw_state::LayerId;
use crate::gamestate::walkaround::{CutsceneReplay, WalkaroundState};
use crate::platform::{ConsoleApi, EggInput, MouseInput, NullConsole, ScanCode, just_pressed, pressed};
use crate::render::{Canvas, PrintOptions, print_to_shadow_with_font};
use crate::ui::layout::{Rect, Ui, UiBuilder};
use crate::{Ctx, EggState};

/// Hold ←/→ to scrub: a tap steps one frame; holding past `SCRUB_REPEAT_DELAY`
/// fixed steps then pages every `SCRUB_REPEAT_RATE` steps (so you don't have to
/// tap once per frame). A touch snappier than the text-field repeat — scrubbing
/// wants to start paging sooner.
const SCRUB_REPEAT_DELAY: u16 = 12;
const SCRUB_REPEAT_RATE: u16 = 2;

/// Frames between snapshot-ladder rungs. Chosen in the 30–60 range: a seek
/// re-sims at most this many cheap cutscene steps (comfortably within a 60fps
/// host frame), while a rung every ~0.75s of playback keeps even a multi-minute
/// scene's clone count in the low hundreds. Each rung excludes the tile bitmap
/// (shared in the map store), so the per-rung memory is modest — 45 balances
/// seek latency against ladder size.
const SNAPSHOT_STRIDE: usize = 45;

/// A keyed node in the scrubber's timeline widget, so a mouse hit resolves to
/// the scrub track (the only interactive element — press/drag it to seek).
#[derive(Clone, Copy, PartialEq)]
enum TimelineKey {
    /// The scrub track: press anywhere on it, or drag along it, to seek.
    Track,
}

/// A steppable replay session for one cutscene. Built by
/// [`EggState::open_scrubber`] and driven by [`EggState::drive_scrubber`].
pub struct CutsceneScrubber {
    /// The scene's registry name (shown in the chrome).
    pub name: String,
    /// The precomputed replay: total length, beat offsets, and the snapshot
    /// ladder each seek re-sims from (its first rung is the armed frame-0 world).
    replay: CutsceneReplay,
    /// The save snapshot, cloned per re-sim so a replayed `set_flag` can never
    /// reach real progress.
    base_save: SaveData,
    /// Headless console driving the re-sim (muted audio, no file IO).
    console: NullConsole,
    /// The input the re-sim frames are stepped against: neutral except a
    /// permanent held-`A` rising edge, threaded through the re-sim `Ctx`s. See
    /// [`open_scrubber_def`](EggState::open_scrubber_def) for why.
    sim_input: EggInput,
    /// Current playhead frame (0..=`replay.total`).
    frame: usize,
    /// Whether a timeline drag is in progress: set when the mouse presses on the
    /// track, cleared on release, so the playhead follows the cursor between.
    scrubbing: bool,
    /// The playhead's re-simmed world, refreshed by [`drive_scrubber`](EggState::drive_scrubber)
    /// each host frame (and seeded at the armed frame 0 on open). The primary
    /// window draws it from the ghost's own camera; the extra F8 views read it via
    /// [`world`](Self::world) to draw the same ghost from their own free cameras.
    /// It carries its own `current_map` — a scene's `init_map` may differ from the
    /// live map — which is why each view draws through this world's own `draw_world`.
    current: WalkaroundState,
}

/// The frame a cursor at screen-`x` selects on `track`, mapping the track's left
/// edge to frame 0 and its right edge to `total`. Pure geometry, split out so the
/// press/drag path and a unit test share one mapping.
fn frame_at_x(x: i16, track: Rect, total: usize) -> usize {
    if track.w <= 0 || total == 0 {
        return 0;
    }
    let t = (f32::from(x - track.x) / f32::from(track.w)).clamp(0.0, 1.0);
    (t * total as f32).round() as usize
}

/// The screen-`x` of `frame`'s playhead on `track` — the inverse of
/// [`frame_at_x`], so frame 0 sits at the left edge and `total` at the last
/// pixel inside the track. Used to draw the playhead and beat markers.
fn x_at_frame(frame: usize, track: Rect, total: usize) -> i16 {
    if total == 0 {
        return track.x;
    }
    let t = frame.min(total) as f32 / total as f32;
    track.x + (t * f32::from((track.w - 1).max(0))).round() as i16
}

impl EggState {
    /// Open the scrubber on the cutscene named `name`, looked up in the loaded
    /// registry. An unknown name logs and no-ops (like a dangling trigger),
    /// leaving any current session untouched. See [`open_scrubber_def`](Self::open_scrubber_def)
    /// for replaying a definition that isn't (yet) in the registry.
    pub fn open_scrubber(&mut self, name: &str) {
        let Some(def) = self.scenes.get_cutscene(name).cloned() else {
            log::info!("scrubber: unknown cutscene {name:?}");
            return;
        };
        self.open_scrubber_def(name.to_string(), def);
    }

    /// Open the scrubber directly on a cutscene `def` — bypassing the registry,
    /// so play-right-after-recording needs no round-trip through the on-disk
    /// scene file + host live-reload (which lands a frame later). Snapshots the
    /// live world + save, arms the scene, replays it once to build the snapshot
    /// ladder + beat offsets, parks at frame 0.
    pub fn open_scrubber_def(&mut self, name: String, def: CutsceneDef) {
        // Snapshot the live world without any live scene, so the replay shows a
        // clean world and starts from a known stack. The editor overlay is no
        // longer part of `WalkaroundState` (the host owns the primary `MapViewer`
        // now), so a re-sim world carries none — nothing to reset here.
        let mut base_world = self.walkaround.clone();
        base_world.cutscene.clear();
        let base_save = self.save.clone();
        let mut console = NullConsole::new();
        // Hold `A` as a permanent rising edge (`[down, up]`): the re-sim never
        // advances this input's edge state, so `just_pressed(a)` reads true every
        // frame, auto-advancing any `dialogue` beat instead of stalling on it
        // (those wait for an `A` press). The dpad stays neutral, so movement /
        // interrupt / skip are untouched.
        let mut sim_input = EggInput::new();
        sim_input.controllers[0].a = [true, false];

        // Arm the scene on the snapshot, then replay it once to capture its
        // length, beat offsets, and the snapshot ladder — all on the headless
        // console + a throwaway save so nothing leaks into live state.
        let replay = {
            let mut scratch = base_save.clone();
            let mut ctx = Ctx {
                draw: &mut self.draw_state,
                system: &mut console,
                input: &sim_input,
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
            base_world.replay_cutscene(SNAPSHOT_STRIDE, &mut ctx)
        };

        log::info!(
            "scrubber: opened {name:?} ({} frames, {} beats)",
            replay.total,
            replay.beats.len()
        );
        // Seed the cached ghost at frame 0: `base_world` is still the armed
        // frame-0 world (`replay_cutscene` re-sims a clone, leaving it untouched),
        // so it *is* the ladder's first rung — moving it in matches `seek(0)`
        // without a redundant re-sim. `drive_scrubber` refreshes it each frame.
        self.scrubber = Some(CutsceneScrubber {
            name,
            replay,
            base_save,
            console,
            sim_input,
            frame: 0,
            scrubbing: false,
            current: base_world,
        });
    }

    /// Drive the open scrubber one host frame: read step/close/scrub input from
    /// the real `input`, re-sim to the playhead from the nearest ladder rung on
    /// the headless console, and render that frame fullscreen under the timeline.
    /// A no-op if no session is open.
    pub fn drive_scrubber(&mut self, system: &mut impl ConsoleApi, input: &EggInput) {
        // Take the session out so the re-sim can borrow `self`'s other fields
        // freely; put it back at the end (drop = close).
        let Some(mut scrubber) = self.scrubber.take() else {
            return;
        };

        // The REAL input drives the playhead: Esc/X closes.
        if input.keyp(ScanCode::Escape) || input.keyp(ScanCode::X) {
            return;
        }

        // Lay the timeline out against the live framebuffer, so the hit test that
        // starts a drag and the draw pass agree on the track rect. The counter is
        // fixed-width, so the track geometry is frame-independent — this rect is
        // reused to draw the playhead below.
        let screen = (system.width() as f32, system.height() as f32);
        let ui = scrubber.build_timeline(screen);
        let track = ui.rect(TimelineKey::Track);

        // Mouse scrub: a press on the track grabs the playhead; while held, the
        // playhead follows the cursor along the track; release drops it.
        if let Some(track) = track {
            let on_track = ui.hit_at(0, 0, input.mouse.pos()) == Some(TimelineKey::Track);
            scrubber.scrub_pointer(&input.mouse, track, on_track);
        }

        // Arrows step one frame (tap) or page (held) — a nudge next to the drag.
        if input.key_repeat(ScanCode::Left, SCRUB_REPEAT_DELAY, SCRUB_REPEAT_RATE) {
            scrubber.frame = scrubber.frame.saturating_sub(1);
        }
        if input.key_repeat(ScanCode::Right, SCRUB_REPEAT_DELAY, SCRUB_REPEAT_RATE) {
            scrubber.frame = (scrubber.frame + 1).min(scrubber.replay.total);
        }

        // Re-sim to the playhead from the nearest ladder rung and cache it on the
        // session, so the extra views can draw the same ghost from their own
        // cameras without re-simming themselves.
        self.seek_scrubber_current(&mut scrubber);

        // Render the ghost fullscreen through the REAL console (sprite assets) —
        // world only, no editor overlay — then the scrubber's banner + timeline
        // over the top.
        {
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
            scrubber
                .current
                .draw_world(&mut ctx, scrubber.current.camera.pos, &DebugInfo::default());
            let banner = format!(
                "SCRUBBER  {}   drag timeline / [<- ->] step   [X] close",
                scrubber.name
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
            scrubber.draw_timeline(ctx.draw, ctx.font, track);
            WalkaroundState::composite_into(ctx.draw, ctx.system.output_image());
        }

        self.scrubber = Some(scrubber);
    }

    /// Re-sim the scrubber's ghost to its playhead and cache it on the session
    /// ([`CutsceneScrubber::current`]), without drawing. The seek runs on the
    /// scrubber's own headless [`NullConsole`], so this needs no external console —
    /// [`drive_scrubber`](Self::drive_scrubber) calls it before the draw, and the
    /// tests exercise it to observe `current` at a frame with no draw pass. Takes
    /// the session by `&mut` so its fields (`console`/`sim_input`/`replay`) borrow
    /// disjointly from `self`'s (`draw`/`maps`/`rng`/…).
    fn seek_scrubber_current(&mut self, scrubber: &mut CutsceneScrubber) {
        let mut scratch = scrubber.base_save.clone();
        let mut ctx = Ctx {
            draw: &mut self.draw_state,
            system: &mut scrubber.console,
            input: &scrubber.sim_input,
            maps: &mut self.maps,
            rng: &mut self.rng,
            script: &self.script,
            scenes: &self.scenes,
            save: &mut scratch,
            items: &self.items,
            presets: &self.presets,
            font: &self.font,
        };
        scrubber.current = scrubber.replay.seek(scrubber.frame, &mut ctx);
    }
}

impl CutsceneScrubber {
    /// The playhead's re-simmed ghost world (refreshed each host frame by
    /// [`drive_scrubber`](EggState::drive_scrubber)). The extra F8 views read it to
    /// draw the ghost from their own free cameras; it carries its own `current_map`,
    /// so a scene that loads a different map still renders correctly in a view.
    pub fn world(&self) -> &WalkaroundState {
        &self.current
    }

    /// Apply one frame of timeline pointer input: a press on the track (`on_track`
    /// from the layout hit test) grabs the playhead; while the button stays held
    /// the playhead follows the cursor's x along `track`; release lets go. Split
    /// out from [`drive_scrubber`](EggState::drive_scrubber) so the drag glue is
    /// testable without the draw pass.
    fn scrub_pointer(&mut self, mouse: &MouseInput, track: Rect, on_track: bool) {
        if just_pressed(mouse.left) && on_track {
            self.scrubbing = true;
        }
        if self.scrubbing {
            if pressed(mouse.left) {
                self.frame = frame_at_x(mouse.pos().x, track, self.replay.total);
            } else {
                self.scrubbing = false;
            }
        }
    }

    /// Lay out the timeline chrome fullscreen: a bottom bar holding the scrub
    /// track (stretched to fill) and a `frame/total` counter. Rebuilt each host
    /// frame (immediate mode) and shared by the hit test and the draw pass, so
    /// they can't disagree on where the track sits.
    fn build_timeline(&self, screen: (f32, f32)) -> Ui<TimelineKey> {
        let mut b = UiBuilder::new();
        // An empty fixed-height box that grows to fill the bar's width — the track.
        let track = b
            .spacer(9.0)
            .grow(1.0)
            .outlined(1, 13)
            .key(TimelineKey::Track)
            .id();
        // Right-pad the current frame to the total's digit count so the counter's
        // width — and thus the grown track's — stays fixed as the playhead moves;
        // otherwise the track would jitter by a digit and the drawn markers would
        // drift off the rect the hit test used.
        let digits = self.replay.total.max(1).to_string().len();
        let counter = b
            .text(format!("{:>digits$}/{}", self.frame, self.replay.total))
            .small(true)
            .color(12)
            .id();
        let bar = b.row(3.0, [track, counter]).pad(2.0).fill(0).id();
        let spacer = b.spacer(0.0).grow(1.0).id();
        let root = b.column(0.0, [spacer, bar]).size(screen.0, screen.1).id();
        b.finish(root, screen)
    }

    /// Draw the timeline over the ghost: the bar + track (via the laid-out UI),
    /// then the beat markers and the playhead on the track (dynamic geometry the
    /// layout tree doesn't carry). `track` is the laid-out track rect from
    /// [`build_timeline`](Self::build_timeline).
    fn draw_timeline(&self, draw: &mut crate::draw_state::DrawState, font: &crate::render::Font, track: Option<Rect>) {
        let (sw, sh) = draw.size();
        self.build_timeline((sw as f32, sh as f32))
            .draw_at(0, 0, draw, font, LayerId::BG);
        let Some(track) = track else { return };
        let total = self.replay.total;
        // Resolve colours before borrowing the layer mutably (they read `draw`).
        let marker = draw.colour(6);
        let playhead = draw.colour(8);
        let layer = draw.rgba(LayerId::BG);
        // Beat markers: a tick at each authored step's start frame.
        for &beat in &self.replay.beats {
            let mx = x_at_frame(beat, track, total);
            layer.vline(i32::from(mx), i32::from(track.y), i32::from(track.h), marker);
        }
        // Playhead: a full-height line, extended a pixel past the track's edges.
        let px = x_at_frame(self.frame, track, total);
        layer.vline(i32::from(px), i32::from(track.y - 1), i32::from(track.h + 2), playhead);
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
        assert_eq!(s.replay.total, 10, "measured the 10-frame move");
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

    /// Play-after-record: a freshly recorded def opens directly, with no entry in
    /// the registry — so it can't race the on-disk scene's live-reload.
    #[test]
    fn open_scrubber_def_replays_without_a_registry_entry() {
        let mut state = EggState::default();
        // `state.scenes` is empty: the definition comes straight in.
        let def = scene::parse("#cutscene x\n    move\n        player: walk 30 0 in 8")
            .unwrap()
            .get_cutscene("x")
            .unwrap()
            .clone();

        state.open_scrubber_def("backyard_path".to_string(), def);

        let s = state.scrubber.as_ref().expect("opened straight from the def");
        assert_eq!(s.name, "backyard_path");
        assert_eq!(s.replay.total, 8, "measured the 8-frame move");
        assert!(
            state.scenes.get_cutscene("backyard_path").is_none(),
            "never went through the registry",
        );
    }

    /// On open the cached ghost ([`CutsceneScrubber::world`]) is the frame-0
    /// snapshot: the scene has no `map`, so the ghost plays in place — same map
    /// source and same player position as the live world it snapshotted.
    #[test]
    fn scrubber_world_is_the_frame_zero_ghost_on_open() {
        let mut state = EggState::default();
        state.walkaround.player().pos = crate::geometry::Vec2::new(0, 0);
        state.scenes =
            scene::parse("#cutscene t\n    move\n        player: walk 30 0 in 10").unwrap();
        let live_map = state.walkaround.current_map.source.clone();
        let live_pos = state.walkaround.player_ref().pos;

        state.open_scrubber("t");

        let s = state.scrubber.as_ref().expect("session opened");
        assert_eq!(
            s.world().current_map.source,
            live_map,
            "frame-0 ghost plays on the live map (the def has no init_map)"
        );
        assert_eq!(
            s.world().player_ref().pos,
            live_pos,
            "frame-0 ghost is the snapshot, unmoved"
        );
    }

    /// Driving the playhead to a later frame and re-simming through the seam
    /// (`seek_scrubber_current`, no draw pass) updates the cached ghost — the
    /// player has walked toward the move's target.
    #[test]
    fn seeking_a_later_frame_updates_the_cached_ghost() {
        let mut state = EggState::default();
        state.walkaround.player().pos = crate::geometry::Vec2::new(0, 0);
        state.scenes =
            scene::parse("#cutscene t\n    move\n        player: walk 30 0 in 10").unwrap();
        state.open_scrubber("t");

        let mut scrubber = state.scrubber.take().expect("session opened");
        assert_eq!(scrubber.world().player_ref().pos.x, 0, "parked at frame 0");

        // Seek to the end through the same seam `drive_scrubber` uses (minus draw).
        scrubber.frame = scrubber.replay.total;
        state.seek_scrubber_current(&mut scrubber);

        assert_eq!(
            scrubber.world().player_ref().pos.x,
            30,
            "seeking to the end re-simmed the ghost to the walk target"
        );
    }

    /// The timeline's cursor→frame mapping: the track's left edge is frame 0, its
    /// right edge the total, the midpoint halfway, and clicks past either end
    /// clamp into range.
    #[test]
    fn timeline_maps_cursor_to_frame() {
        let track = Rect { x: 20, y: 0, w: 100, h: 9 };
        let total = 200;

        assert_eq!(frame_at_x(20, track, total), 0, "left edge is frame 0");
        assert_eq!(frame_at_x(120, track, total), total, "right edge is the total");
        assert_eq!(frame_at_x(70, track, total), 100, "midpoint is halfway");
        assert_eq!(frame_at_x(5, track, total), 0, "left of the track clamps to 0");
        assert_eq!(frame_at_x(999, track, total), total, "right of the track clamps to total");

        // A degenerate (empty / zero-width) track never divides by zero.
        assert_eq!(frame_at_x(50, track, 0), 0, "no frames ⇒ frame 0");
        assert_eq!(frame_at_x(50, Rect { w: 0, ..track }, total), 0, "zero-width ⇒ frame 0");
    }

    /// A playhead sits inside the track, advances monotonically, and — when the
    /// track is wider than the frame count (one pixel per frame or more) —
    /// round-trips back to its exact frame through [`frame_at_x`].
    #[test]
    fn timeline_playhead_round_trips_on_a_wide_track() {
        let track = Rect { x: 20, y: 0, w: 100, h: 9 };
        let total = 20; // wider track than frames ⇒ the inverse is exact.

        let mut last_x = i16::MIN;
        for frame in 0..=total {
            let x = x_at_frame(frame, track, total);
            assert!(
                (track.x..track.x + track.w).contains(&x),
                "playhead for frame {frame} is inside the track ({x})",
            );
            assert!(x >= last_x, "playhead advances monotonically at frame {frame}");
            last_x = x;
            assert_eq!(
                frame_at_x(x, track, total),
                frame,
                "frame {frame} round-trips through its playhead x",
            );
        }
        assert_eq!(x_at_frame(3, track, 0), track.x, "no frames ⇒ left edge");
    }

    /// The drag glue on a real opened scrubber: pressing the track's right edge
    /// grabs the playhead and jumps it to the end; holding and dragging to the
    /// left edge walks it back to 0; releasing lets go, so later motion with the
    /// button up no longer moves the playhead. This drives the same
    /// [`scrub_pointer`](CutsceneScrubber::scrub_pointer) path
    /// [`drive_scrubber`](EggState::drive_scrubber) does, minus the draw pass.
    #[test]
    fn dragging_the_timeline_moves_the_playhead() {
        let mut state = EggState::default();
        state.scenes =
            scene::parse("#cutscene t\n    move\n        player: walk 30 0 in 20").unwrap();
        state.open_scrubber("t");
        let mut scrubber = state.scrubber.take().expect("session opened");
        assert_eq!(scrubber.replay.total, 20);

        // Lay out the timeline as `drive_scrubber` does, and locate the track.
        let screen = (240.0, 136.0);
        let track = scrubber
            .build_timeline(screen)
            .rect(TimelineKey::Track)
            .expect("track laid out");
        let cy = track.center_y();

        // A helper: one pointer frame at `(x, cy)` with the given button edge.
        let pointer = |scrubber: &mut CutsceneScrubber, x: i16, left: [bool; 2]| {
            let mut mouse = MouseInput::default();
            mouse.x = [x, x];
            mouse.y = [cy, cy];
            mouse.left = left;
            let on_track =
                scrubber.build_timeline(screen).hit_at(0, 0, mouse.pos()) == Some(TimelineKey::Track);
            scrubber.scrub_pointer(&mouse, track, on_track);
        };

        // Press the right edge: grabs the playhead, jumps it to the end.
        pointer(&mut scrubber, track.x + track.w - 1, [true, false]);
        assert!(scrubber.scrubbing, "a press on the track starts a drag");
        assert_eq!(scrubber.frame, 20, "pressing the right edge seeks to the end");

        // Drag to the left edge with the button still held: walks back to 0.
        pointer(&mut scrubber, track.x, [true, true]);
        assert_eq!(scrubber.frame, 0, "dragging to the left edge seeks to the start");

        // Release: the drag ends.
        pointer(&mut scrubber, track.x, [false, true]);
        assert!(!scrubber.scrubbing, "releasing ends the drag");

        // A later move with the button up doesn't move the playhead.
        pointer(&mut scrubber, track.x + track.w - 1, [false, false]);
        assert_eq!(scrubber.frame, 0, "no drag ⇒ the playhead holds");
    }
}

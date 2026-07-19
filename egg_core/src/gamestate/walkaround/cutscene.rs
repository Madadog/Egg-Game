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

//! The runtime for [`CutsceneDef`](crate::data::scene::CutsceneDef): a live,
//! playing [`Cutscene`] built by requisitioning its actors, then stepping its
//! content one step at a time. Cutscenes form a **stack** on
//! [`WalkaroundState`] — a `load` step pushes a sub-cutscene, popped on finish —
//! so map changes (a sub-cutscene's `init_map`) happen at cutscene boundaries
//! with fresh requisition.

use std::collections::{HashMap, HashSet};

use crate::Ctx;
use crate::data::scene::{
    CameraTarget, Chain, CueHandler, CutsceneContent, CutsceneDef, EntityRef, GetEntity, Motion,
};
use crate::data::sound::music::MusicTrack;
use crate::data::sound::{self};
use crate::geometry::Vec2;
use crate::platform::{ConsoleApi, ConsoleHelper, dpad_delta, just_pressed, pressed};
use crate::world::camera::Shake;
use crate::world::player::{EntityId, MoveMode, Shell};

use super::{EntityPath, WalkaroundState};

/// Natural-speed move cap (frames): a `time == 0` move that can't reach its
/// target (a point behind a wall) gives up after this so a step can't hang.
const NATURAL_CAP: u16 = 600;

/// A live, playing cutscene. Built from a [`CutsceneDef`] via [`launch`], which
/// requisitions its actors into a name→entity table, then steps its content in
/// order ([`step`]). Lives on a stack ([`WalkaroundState`]); a `load` step
/// pushes a sub-cutscene, popped on finish.
///
/// [`launch`]: Self::launch
/// [`step`]: Self::step
#[derive(Clone, Debug)]
pub struct Cutscene {
    /// Actor name → the entity it resolves to (aliases + bound ids). Names absent
    /// here fall back to the reserved `player`/`companion N`, then a bare id.
    table: HashMap<String, EntityId>,
    /// Names of transient `spawn`ed actors, removed from the world on finish.
    spawned: Vec<String>,
    content: Vec<CutsceneContent>,
    /// Index of the content step currently playing.
    step: usize,
    /// Live state of that step.
    state: StepState,
    /// `#cutscene NAME interruptible` — player movement cancels the scene.
    interruptible: bool,
    /// Set when a required (`?`) motion is blocked; [`step`](Self::step) then
    /// reports [`Outcome::Cancelled`].
    aborted: bool,
    /// Where this scene points the camera, set by a `camera` step. `None` follows
    /// the player (the default, and where the scene lands back when it ends). Read
    /// live each frame by [`camera_focus`](Self::camera_focus).
    camera: Option<CameraTarget>,
    /// An in-flight `camera … over N` glide, easing the focus from where the
    /// camera was toward the (live) target. Ticked once per frame in
    /// [`step`](Self::step); `None` once it lands (or after a cut).
    glide: Option<Glide>,
    /// An in-flight `shake`, offsetting the focus in a fixed decaying pattern.
    /// Ticked once per frame in [`step`](Self::step); transient — the camera is
    /// back on its focus when it runs out.
    shake: Option<Shake>,
    /// Cue handlers currently running for the in-progress `dialogue` step (see
    /// [`CutsceneContent::Dialogue`]'s `on NAME [wait]` handlers) — populated
    /// and ticked by [`advance_dialogue`](Self::advance_dialogue), cleared the
    /// moment that step completes. Empty outside a `dialogue` step.
    handler_runs: Vec<HandlerRun>,
    /// Actors a `pose` motion (step or snap, top-level or in an `on` handler —
    /// see [`crate::data::scene::Motion::Pose`]) has been applied to since this
    /// scene launched. A pose is scene-scoped choreography, so [`cleanup`]
    /// clears it from every actor recorded here — the same "undo what I did"
    /// contract [`spawned`] gives the actors this scene created outright.
    ///
    /// [`cleanup`]: Self::cleanup
    /// [`spawned`]: Self::spawned
    posed: HashSet<EntityId>,
}

/// Progress of a `camera … over N` glide: the focus eases from `from` (the
/// actual on-screen focus when the step ran, so a mid-pan retarget has no snap)
/// to the live target focus, smoothstepped over `total` frames. Integer fixed
/// point throughout — the scrubber re-sims cutscenes, so this must be exact.
#[derive(Clone, Debug)]
struct Glide {
    from: Vec2,
    total: u32,
    left: u32,
}

impl Glide {
    /// The eased focus `total - left` frames in: `from` at the start, exactly
    /// `to` at the end, smoothstep (t²(3−2t)) between, in 1/1024 fixed point.
    fn at(&self, to: Vec2) -> Vec2 {
        let t = ((self.total - self.left) as i64 * 1024) / self.total as i64;
        let s = (t * t * (3 * 1024 - 2 * t)) >> 20; // 0..=1024
        let ease = |a: i16, b: i16| (a as i64 + ((b as i64 - a as i64) * s) / 1024) as i16;
        Vec2::new(ease(self.from.x, to.x), ease(self.from.y, to.y))
    }
}

/// The live state of the content step a [`Cutscene`] is on.
#[derive(Clone, Debug)]
enum StepState {
    /// Not started — the step's start hook runs next [`step`](Cutscene::step).
    Pending,
    /// Parallel chains (cloned from the step) + one progress record each.
    Move {
        chains: Vec<Chain>,
        progress: Vec<ChainProgress>,
    },
    /// A dialogue box; `opened` latches the one-time open. `close_pending` is
    /// set when the widget phase (see
    /// [`advance_dialogue`](Cutscene::advance_dialogue)) decided to close the
    /// box but a running `wait` handler is holding it frozen — applied the
    /// frame the freeze lifts, so the player never needs a second press for a
    /// close they already entered.
    Dialogue { opened: bool, close_pending: bool },
    /// Frames left to wait.
    Wait(u32),
    /// Finished — advance to the next step.
    Done,
}

/// One chain's progress within a `move` step.
#[derive(Clone, Debug)]
struct ChainProgress {
    /// The current instruction index in the chain (`>= len` ⇒ this chain done).
    instr: usize,
    /// Frames elapsed in the current instruction.
    elapsed: u16,
    /// Consecutive frames the current instruction made no progress (blocked).
    /// Trips [`STUCK_LIMIT`] for a required motion → the scene cancels.
    stuck: u16,
}

/// One running `on NAME [wait]` handler for the current `dialogue` step: which
/// handler (by index into that step's [`CueHandler`] list), how far through
/// its own content it's gotten, and its live [`StepState`] — never
/// `Dialogue`: [`Cutscene::enter_content`]'s `Interact` arm treats a
/// handler's `interact` as always-instant rather than tracking a box it might
/// open (see that method's doc), and a literal `dialogue`/`load` step is
/// rejected inside an `on` body at parse time. A run is created at most once
/// per handler per dialogue step — its mere existence in
/// [`Cutscene::handler_runs`] IS the fired-once dedupe, so there's no
/// separate "already fired" set to keep in sync — and is never removed until
/// the whole step completes (when [`Cutscene::handler_runs`] is cleared).
#[derive(Clone, Debug)]
struct HandlerRun {
    /// Index into the current dialogue step's `handlers` list.
    handler_index: usize,
    /// The next content-step index in `handlers[handler_index].content` to
    /// enter. `>= content.len()` means this run has finished.
    step: usize,
    state: StepState,
}

/// What one [`Cutscene::step`] asks the stack driver to do next.
pub enum Outcome {
    /// Still playing — keep it on the stack.
    Running,
    /// Reached the end — pop it and clean up its spawned actors.
    Finished,
    /// A required (`?`) motion could not make progress — cancel the scene (same
    /// cleanup as [`Finished`](Self::Finished), just early).
    Cancelled,
    /// Hit a `load NAME` (already advanced past it) — push that sub-cutscene.
    Load(String),
}

/// Frames a required move may make no progress before it counts as blocked and
/// cancels the scene. A couple of frames of slide-against-a-wall is fine; a wall
/// it can't get past trips this fast.
const STUCK_LIMIT: u16 = 6;

impl Cutscene {
    /// Build + requisition a live cutscene: load `init_map` (if set), then bind
    /// each actor name (spawn a transient shell / get-or-spawn / find / alias to
    /// a well-known entity).
    pub fn launch<S: ConsoleApi>(
        def: &CutsceneDef,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
    ) -> Self {
        if let Some(map) = &def.init_map {
            walkaround.load_map_by_name(ctx, map);
        }
        let mut table = HashMap::new();
        let mut spawned = Vec::new();
        for entity in &def.init {
            match entity {
                GetEntity::Spawn { name, preset, pos } => {
                    // Stamp the shell with a minted, collision-proof id (not the
                    // author name), so a `spawn` whose NAME collides with a live
                    // map creature neither steals its resolution nor deletes it on
                    // cleanup. The author name resolves to the minted id here.
                    let id = walkaround.mint_spawn_id();
                    spawn_actor(walkaround, ctx, &id, &preset.to_string(), *pos);
                    spawned.push(id.clone());
                    table.insert(name.clone(), EntityId::Id(id));
                }
                GetEntity::GetOrSpawn { name, preset, pos } => {
                    if walkaround.resolve(&EntityId::Id(name.clone())).is_none() {
                        spawn_actor(walkaround, ctx, name, &preset.to_string(), *pos);
                    }
                    table.insert(name.clone(), EntityId::Id(name.clone()));
                }
                GetEntity::GetOrIgnore { name } => {
                    if walkaround.resolve(&EntityId::Id(name.clone())).is_some() {
                        table.insert(name.clone(), EntityId::Id(name.clone()));
                    }
                    // miss: leave unbound → the name resolves to nothing.
                }
                GetEntity::Alias { name, target } => {
                    let id = match target {
                        EntityRef::Player => EntityId::Player,
                        EntityRef::Companion(slot) => EntityId::PlayerCompanion(*slot),
                    };
                    table.insert(name.clone(), id);
                }
            }
        }
        Self {
            table,
            spawned,
            content: def.content.clone(),
            step: 0,
            state: StepState::Pending,
            interruptible: def.interruptible,
            aborted: false,
            camera: None,
            glide: None,
            shake: None,
            handler_runs: Vec::new(),
            posed: HashSet::new(),
        }
    }

    /// Whether this scene cancels on player movement (`#cutscene … interruptible`).
    pub fn is_interruptible(&self) -> bool {
        self.interruptible
    }

    /// Index of the content step currently playing — the scrubber samples this
    /// each re-sim frame to map frames onto authored beats (see
    /// [`WalkaroundState::replay_cutscene`](super::WalkaroundState::replay_cutscene)).
    pub(super) fn active_step(&self) -> usize {
        self.step
    }

    /// The map-pixel point this scene currently centres the camera on, or `None`
    /// to follow the player — the default focus, what the scene resets to when it
    /// ends, and the fallback when a `camera ACTOR` target can't be resolved.
    /// [`play_cutscene`](super::WalkaroundState::play_cutscene) calls this on the
    /// top-of-stack scene each frame. An actor focus is read live (a followed
    /// actor that walks pulls the camera along); player/actor foci carry the same
    /// +4/-2 hitbox-centre offset the follow camera uses, while a fixed point is
    /// centred exactly as authored. An in-flight glide eases toward that point.
    pub(super) fn camera_focus(&self, walkaround: &WalkaroundState) -> Option<Vec2> {
        let to = match self.camera.as_ref()? {
            CameraTarget::Point(p) => *p,
            CameraTarget::Actor(name) => walkaround
                .resolve(&resolve_name(name, &self.table))
                .map(|s| Vec2::new(s.pos.x + 4, s.pos.y - 2))?,
        };
        Some(match &self.glide {
            Some(glide) => glide.at(to),
            None => to,
        })
    }

    /// This frame's `shake` focus offset, or zero when no shake is running.
    /// Applied by [`play_cutscene`](super::WalkaroundState::play_cutscene) on top
    /// of whatever the focus is — a shake jiggles the player-follow default too.
    pub(super) fn shake_offset(&self) -> Vec2 {
        self.shake
            .as_ref()
            .map_or(Vec2::new(0, 0), |shake| shake.offset())
    }

    /// Drive one frame. Chains the instant steps (sound/flag/…) into the same
    /// frame; the first frame-consuming step (`move`/`dialogue`/`wait`) returns
    /// [`Outcome::Running`]. A `load` returns [`Outcome::Load`]; the end returns
    /// [`Outcome::Finished`].
    pub fn step<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
    ) -> Outcome {
        // Advance the background camera effects exactly once per frame (this runs
        // once per frame on the top-of-stack scene; a parent's effects pause while
        // a sub-scene drives). A landed glide drops to `None`, pinning the focus
        // on the target from then on (`Glide::at` is exact at t=1).
        if let Some(glide) = &mut self.glide {
            glide.left -= 1;
            if glide.left == 0 {
                self.glide = None;
            }
        }
        Shake::tick(&mut self.shake);
        loop {
            if matches!(self.state, StepState::Pending) {
                // Cloned so `enter_content` can freely take `&mut self`
                // alongside it — see that method's doc.
                let Some(content) = self.content.get(self.step).cloned() else {
                    return Outcome::Finished;
                };
                match &content {
                    // Kept here rather than in `enter_content`: only the
                    // top-level driver owns opening the box / pushing a
                    // sub-cutscene (a handler can never contain either —
                    // parse-rejected, see the module doc).
                    CutsceneContent::Dialogue { .. } => {
                        self.state = StepState::Dialogue { opened: false, close_pending: false };
                    }
                    CutsceneContent::Load(name) => {
                        let name = name.clone();
                        self.step += 1;
                        self.state = StepState::Pending;
                        return Outcome::Load(name);
                    }
                    _ => {
                        self.state = self.enter_content(ctx, walkaround, &content, false);
                    }
                }
            }

            let done = if matches!(self.state, StepState::Move { .. }) {
                self.advance_move(ctx, walkaround)
            } else if matches!(self.state, StepState::Dialogue { .. }) {
                self.advance_dialogue(ctx, walkaround)
            } else if let StepState::Wait(frames) = &mut self.state {
                *frames = frames.saturating_sub(1);
                *frames == 0
            } else {
                matches!(self.state, StepState::Done)
            };

            // A required motion blocked this frame — bail out of the whole scene.
            if self.aborted {
                return Outcome::Cancelled;
            }
            if done {
                self.step += 1;
                self.state = StepState::Pending;
                continue;
            }
            return Outcome::Running;
        }
    }

    /// Enter a content step that isn't `Dialogue`/`Load` — the instant verbs
    /// (`sound`/`music`/`set`/`camera`/`shake`), `interact`, `move`, and
    /// `wait` — building its initial [`StepState`]. Shared by the main step
    /// loop's Pending handling (which keeps `Dialogue`/`Load` for itself — a
    /// `load` pushes a sub-cutscene, which only the top-level driver owns)
    /// and [`tick_handler_run`](Self::tick_handler_run), where a raw
    /// `Dialogue`/`Load` content step can never appear at all (rejected at
    /// parse time — see the `.eggscene` module doc), so reaching either arm
    /// here is a bug, not bad input.
    ///
    /// `in_handler` changes only the `Interact` arm: at the top level an
    /// `interact` step only ever runs once any prior dialogue box has
    /// already closed (steps are sequential), so "a box is showing
    /// afterward" reliably means this interact just opened one — worth
    /// tracking so the step waits for it. A handler runs *concurrently* with
    /// its own dialogue step's already-open box, so that signal means
    /// nothing there (the box is virtually always showing); a handler's
    /// interact is instant instead, no matter what it fired. (A handler
    /// `interact` that happens to target something whose interaction is
    /// itself `Dialogue`/`Func`-returning-a-key silently overwrites the
    /// parent box's content when it fires — an authoring hazard sharing one
    /// widget between concurrent steps creates, which this wave doesn't try
    /// to detect or prevent.)
    fn enter_content<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
        content: &CutsceneContent,
        in_handler: bool,
    ) -> StepState {
        match content {
            CutsceneContent::Move(chains) => {
                let progress = chains
                    .iter()
                    .map(|_| ChainProgress {
                        instr: 0,
                        elapsed: 0,
                        stuck: 0,
                    })
                    .collect();
                StepState::Move {
                    chains: chains.clone(),
                    progress,
                }
            }
            CutsceneContent::Wait(frames) => StepState::Wait(*frames),
            CutsceneContent::Interact { actor, target } => {
                self.fire_interact(ctx, walkaround, actor, target);
                if !in_handler && walkaround.dialogue.current_text.is_some() {
                    StepState::Dialogue { opened: true, close_pending: false }
                } else {
                    StepState::Done
                }
            }
            CutsceneContent::Sound(name) => {
                if let Some(sfx) = sound::by_name(name) {
                    ctx.system.play_sound(sfx);
                }
                StepState::Done
            }
            CutsceneContent::Music(track) => {
                let track = track.as_deref().map(MusicTrack::named);
                ctx.system.music(track.as_ref());
                StepState::Done
            }
            CutsceneContent::SetFlag(name, value) => {
                ctx.save.set_flag(name, *value);
                StepState::Done
            }
            CutsceneContent::Camera(target, over) => {
                // Retarget the scene camera; the per-frame centring in
                // `play_cutscene` reads it back via `camera_focus`. A
                // glide starts from the focus that's actually on screen
                // (the camera's clamped position, uncentred), so a cut,
                // an earlier glide, or the player-follow default all
                // hand over without a snap. The instant retarget below
                // doesn't move the camera this frame: at t=0 the glide
                // holds `from` exactly.
                self.glide = over.filter(|frames| *frames > 0).map(|total| Glide {
                    from: walkaround.camera.pos
                        + Vec2::new(
                            ctx.system.width() as i16 / 2,
                            ctx.system.height() as i16 / 2,
                        ),
                    total,
                    left: total,
                });
                self.camera = Some(target.clone());
                StepState::Done
            }
            CutsceneContent::Shake { frames, amplitude } => {
                self.shake = Shake::begin(*frames, *amplitude);
                StepState::Done
            }
            CutsceneContent::Dialogue { .. } => unreachable!(
                "Dialogue is handled by the caller: the main loop keeps it for \
                 itself, and a handler body can never contain one (parse-rejected)"
            ),
            CutsceneContent::Load(_) => unreachable!(
                "Load is handled by the caller: the main loop keeps it for \
                 itself, and a handler body can never contain one (parse-rejected)"
            ),
        }
    }

    /// Advance one set of parallel chains (+ their progress) one frame — the
    /// shared core [`advance_move`](Self::advance_move) (the main `Move`
    /// state) and [`advance_handler_move`](Self::advance_handler_move) (a
    /// handler's `Move` state) both drive, since both use the identical
    /// lift-out/tick/put-back pattern their callers wrap. Returns whether
    /// every chain has run out of instructions. Sets `self.aborted` when a
    /// required (`?`) motion sticks — cancelling the whole scene, whether the
    /// chains belong to the main step or a handler (handler side effects
    /// apply to the parent scene exactly as top-level steps do).
    fn advance_move_chains<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
        chains: &[Chain],
        progress: &mut [ChainProgress],
    ) -> bool {
        for (chain, prog) in chains.iter().zip(progress.iter_mut()) {
            if prog.instr >= chain.instructions.len() {
                continue;
            }
            let ins = &chain.instructions[prog.instr];
            // Record every actor a `pose` motion touches (top-level or inside a
            // handler — both paths call this method) so `cleanup` can undo it;
            // see the `posed` field doc.
            if let Motion::Pose(_) = &ins.motion {
                self.posed.insert(resolve_name(&chain.actor, &self.table));
            }
            let (finished, progressed) = step_motion(
                &chain.actor,
                &ins.motion,
                ins.time,
                prog.elapsed,
                &self.table,
                ctx,
                walkaround,
            );
            // A required (`?`) motion that stays blocked cancels the whole scene.
            if ins.required && !progressed {
                prog.stuck = prog.stuck.saturating_add(1);
                if prog.stuck >= STUCK_LIMIT {
                    self.aborted = true;
                }
            } else {
                prog.stuck = 0;
            }
            prog.elapsed = prog.elapsed.saturating_add(1);
            if finished {
                prog.instr += 1;
                prog.elapsed = 0;
                prog.stuck = 0;
            }
        }
        chains
            .iter()
            .zip(progress.iter())
            .all(|(c, p)| p.instr >= c.instructions.len())
    }

    /// Advance every chain of the current `move` step one frame; returns whether
    /// all chains have run out of instructions.
    fn advance_move<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
    ) -> bool {
        // Lift the chains + progress out so motion code can borrow `self.table`
        // (for resolution) alongside them.
        let StepState::Move {
            chains,
            mut progress,
        } = std::mem::replace(&mut self.state, StepState::Done)
        else {
            unreachable!("advance_move only runs on a Move state");
        };
        let all_done = self.advance_move_chains(ctx, walkaround, &chains, &mut progress);
        if !all_done {
            self.state = StepState::Move { chains, progress };
        }
        all_done
    }

    /// Advance handler run `i`'s `Move` state one frame — the handler-side
    /// twin of [`advance_move`](Self::advance_move), same lift-out pattern,
    /// against `self.handler_runs[i].state` instead of `self.state`.
    fn advance_handler_move<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
        i: usize,
    ) -> bool {
        let StepState::Move {
            chains,
            mut progress,
        } = std::mem::replace(&mut self.handler_runs[i].state, StepState::Done)
        else {
            unreachable!("advance_handler_move only runs on a Move state");
        };
        let all_done = self.advance_move_chains(ctx, walkaround, &chains, &mut progress);
        if !all_done {
            self.handler_runs[i].state = StepState::Move { chains, progress };
        }
        all_done
    }

    /// The current dialogue step's `on` handlers, or `&[]` if the step isn't
    /// `Dialogue` (defensive — only ever called while it is).
    fn current_handlers(&self) -> &[CueHandler] {
        match &self.content[self.step] {
            CutsceneContent::Dialogue { handlers, .. } => handlers,
            _ => &[],
        }
    }

    /// Whether any running handler declared `wait` is unfinished — while
    /// true, [`advance_dialogue`](Self::advance_dialogue) freezes the box.
    fn any_wait_handler_running(&self) -> bool {
        let handlers = self.current_handlers();
        self.handler_runs
            .iter()
            .any(|run| handlers[run.handler_index].wait && run.step < handlers[run.handler_index].content.len())
    }

    /// Whether every fired handler for the current dialogue step has
    /// finished (walked its `step` off the end of its own content list).
    /// Vacuously true when nothing has fired.
    fn handlers_finished(&self) -> bool {
        let handlers = self.current_handlers();
        self.handler_runs
            .iter()
            .all(|run| run.step >= handlers[run.handler_index].content.len())
    }

    /// Drain every `#cue` the box has banked since the last drain (see
    /// [`Dialogue::take_cues`](egg_ui::dialogue::Dialogue::take_cues)),
    /// starting a [`HandlerRun`] for each one that names an `on` handler on
    /// the current dialogue step and hasn't already fired this step (a run
    /// already existing for that handler IS the fired-once dedupe — a repeat
    /// firing is logged and ignored; a cue naming no handler here is fine —
    /// a stage direction, or a beat for another scene), then ticks every
    /// running handler one frame. A no-op — not even a `take_cues` call —
    /// when this dialogue step has no handlers at all (the common case):
    /// nothing could ever be listening, so there's nothing to drain for, and
    /// `self.handler_runs` is (and stays) empty.
    fn drain_cues_and_tick_handlers<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
    ) {
        let handlers = self.current_handlers();
        if handlers.is_empty() {
            return;
        }
        // Cloned so the tick loop below can freely call `&mut self` methods
        // alongside it without borrowing `self.content`.
        let handlers = handlers.to_vec();
        for cue in walkaround.dialogue.take_cues() {
            let Some(handler_index) = handlers.iter().position(|h| h.cue == cue) else {
                continue;
            };
            if self.handler_runs.iter().any(|r| r.handler_index == handler_index) {
                log::info!("cutscene: cue `{cue}` fired again — its handler already ran this step");
                continue;
            }
            self.handler_runs.push(HandlerRun {
                handler_index,
                step: 0,
                state: StepState::Pending,
            });
        }
        for i in 0..self.handler_runs.len() {
            self.tick_handler_run(ctx, walkaround, i, &handlers);
        }
    }

    /// Tick handler run `i` one frame, through the same enter → advance →
    /// maybe-finish sequence [`step`](Self::step)'s main loop drives,
    /// against `handlers[run.handler_index]`'s own content list (`handlers`
    /// is the caller's clone — see
    /// [`drain_cues_and_tick_handlers`](Self::drain_cues_and_tick_handlers)
    /// — so this can freely call `&mut self` methods alongside it). Chains
    /// through as many instant steps as complete in one frame, same as the
    /// main loop; stops at the first step that doesn't finish this frame, or
    /// at the handler's end.
    fn tick_handler_run<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
        i: usize,
        handlers: &[CueHandler],
    ) {
        let handler = &handlers[self.handler_runs[i].handler_index];
        loop {
            if self.handler_runs[i].step >= handler.content.len() {
                return;
            }
            if matches!(self.handler_runs[i].state, StepState::Pending) {
                let content = handler.content[self.handler_runs[i].step].clone();
                self.handler_runs[i].state = self.enter_content(ctx, walkaround, &content, true);
            }
            let done = match &self.handler_runs[i].state {
                StepState::Move { .. } => self.advance_handler_move(ctx, walkaround, i),
                StepState::Wait(_) => {
                    let StepState::Wait(frames) = &mut self.handler_runs[i].state else {
                        unreachable!("just matched Wait above");
                    };
                    *frames = frames.saturating_sub(1);
                    *frames == 0
                }
                StepState::Done => true,
                StepState::Dialogue { .. } => unreachable!(
                    "a handler run never enters Dialogue state — see `enter_content`'s doc"
                ),
                StepState::Pending => unreachable!("just entered above"),
            };
            if self.aborted {
                return;
            }
            if done {
                self.handler_runs[i].step += 1;
                self.handler_runs[i].state = StepState::Pending;
                continue;
            }
            return;
        }
    }

    /// Drive the dialogue box for the current `dialogue` step (the walk
    /// loop's dialogue input is short-circuited while a cutscene plays).
    /// Opens the box once. After that, each frame: runs the widget phase
    /// (choice input / typewriter tick / A-advance / B-skip) *unless* a
    /// `wait` handler is running, in which case the box freezes — no tick,
    /// no input, no close; drains any `#cue`s the widget phase banked and
    /// ticks every running handler
    /// ([`drain_cues_and_tick_handlers`](Self::drain_cues_and_tick_handlers));
    /// then applies a close the widget phase decided it wanted, unless a
    /// `wait` handler is (still) running, in which case the close is
    /// remembered (`close_pending`) and applied the frame the freeze lifts
    /// — so the player never needs a second press for a close they already
    /// entered.
    ///
    /// Order matters: [`Dialogue::close`](egg_ui::dialogue::Dialogue::close)
    /// resets the whole widget, wiping `pending_cues` — so the widget phase
    /// only *decides* whether it wants to close, never calls `close`
    /// itself; the drain always runs before any close does. This is what
    /// lets a `#cue` on the very last content item still fire its handler
    /// even though the same advance that surfaces it also ends the
    /// conversation.
    ///
    /// The step is done only once the box has closed *and* every fired
    /// handler has finished (a non-`wait` handler may keep running after the
    /// box closes — the step just waits for it).
    fn advance_dialogue<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
    ) -> bool {
        let (opened, close_pending) = match &self.state {
            StepState::Dialogue { opened, close_pending } => (*opened, *close_pending),
            _ => unreachable!("advance_dialogue only runs on a Dialogue state"),
        };
        if !opened {
            let CutsceneContent::Dialogue { key, .. } = &self.content[self.step] else {
                unreachable!("Dialogue state ⇒ Dialogue content");
            };
            let key = key.clone();
            let convo = ctx.get_dialogue(&key);
            walkaround
                .dialogue
                .set_messages(ctx.system, ctx.font, ctx.save, &convo);
            self.state = StepState::Dialogue { opened: true, close_pending: false };
            self.drain_cues_and_tick_handlers(ctx, walkaround);
            return false;
        }

        let mut want_close = close_pending;
        if !self.any_wait_handler_running() {
            let pad = ctx.input.controller();
            // A choice menu takes over input: up/down moves the highlight, A
            // picks. Under the scrubber the dpad is neutral and A reads as a
            // permanent rising edge, so this deterministically auto-picks the
            // first option (the same auto-advance the scrubber relies on for
            // plain dialogue).
            if walkaround.dialogue.is_choosing() {
                let (_, ddy) = dpad_delta(&pad, just_pressed);
                if ddy != 0 {
                    walkaround.dialogue.move_choice(ddy as i32);
                }
                if just_pressed(pad.a) {
                    let advanced = walkaround.dialogue.confirm_choice(ctx.system, ctx.font, ctx.save);
                    if !advanced
                        && !walkaround.dialogue.is_choosing()
                        && walkaround.dialogue.current_text.is_some()
                    {
                        want_close = true;
                    }
                }
            } else {
                walkaround.dialogue.tick(ctx.system, ctx.font, ctx.save, 1);
                if pressed(pad.a) {
                    walkaround.dialogue.tick(ctx.system, ctx.font, ctx.save, 2);
                }
                if just_pressed(pad.b) {
                    walkaround.dialogue.skip(ctx.system, ctx.font, ctx.save);
                }
                if just_pressed(pad.a) && walkaround.dialogue.is_line_done() {
                    let advanced =
                        walkaround.dialogue.next_text(ctx.system, ctx.font, ctx.save, false);
                    if !advanced && walkaround.dialogue.current_text.is_some() {
                        want_close = true;
                    }
                }
            }
        }

        self.drain_cues_and_tick_handlers(ctx, walkaround);

        if want_close && !self.any_wait_handler_running() {
            walkaround.dialogue.close();
            want_close = false;
        }
        self.state = StepState::Dialogue { opened: true, close_pending: want_close };

        let finished = !walkaround.dialogue.is_active() && self.handlers_finished();
        if finished {
            self.handler_runs.clear();
        }
        finished
    }

    /// Fire the `target` actor's intrinsic [`Shell::interaction`], with `actor`
    /// as the initiator. An unresolvable target or one with no interaction logs
    /// and is skipped.
    fn fire_interact<S: ConsoleApi>(
        &self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
        _actor: &str,
        target: &str,
    ) {
        let target_id = resolve_name(target, &self.table);
        let Some(interaction) = walkaround
            .resolve(&target_id)
            .and_then(|s| s.interaction.clone())
        else {
            log::info!("cutscene interact: `{target}` is absent or not interactive");
            return;
        };
        let mut inventory = std::mem::take(&mut walkaround.inventory_ui.inventory);
        walkaround.fire_interaction(ctx, &interaction, &mut inventory);
        walkaround.inventory_ui.inventory = inventory;
    }

    /// Remove this cutscene's transient `spawn`ed actors from the world and
    /// clear every `pose` it applied — run once, when it finishes or is
    /// skipped, so neither outlives the scene that created it.
    pub fn cleanup(&self, walkaround: &mut WalkaroundState) {
        if !self.spawned.is_empty() {
            walkaround
                .entities
                .retain(|e| match &e.id {
                    Some(id) => !self.spawned.contains(id),
                    None => true,
                });
        }
        for id in &self.posed {
            if let Some(path) = walkaround.resolve_path(id) {
                path.shell_mut(walkaround).pose = None;
            }
        }
    }

    /// Fast-forward one content step to its end state — the per-step body
    /// shared between the top-level [`skip`](Self::skip) loop and snapping a
    /// handler's content (see
    /// [`fire_and_snap_handlers`](Self::fire_and_snap_handlers)). Never
    /// called with `Dialogue`/`Load`: [`skip`](Self::skip) handles both
    /// itself (`Dialogue` needs the manual drain documented there; `Load`
    /// needs to chase a whole sub-cutscene), and a handler body can't
    /// contain either (parse-rejected — see the `.eggscene` module doc).
    fn skip_content<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
        content: &CutsceneContent,
    ) {
        match content {
            CutsceneContent::Move(chains) => {
                for chain in chains {
                    for ins in &chain.instructions {
                        // Same bookkeeping as `advance_move_chains` — see the
                        // `posed` field doc.
                        if let Motion::Pose(_) = &ins.motion {
                            self.posed.insert(resolve_name(&chain.actor, &self.table));
                        }
                        snap_motion(&chain.actor, &ins.motion, &self.table, walkaround);
                    }
                }
            }
            CutsceneContent::SetFlag(name, value) => ctx.save.set_flag(name, *value),
            CutsceneContent::Sound(name) => {
                if let Some(sfx) = sound::by_name(name) {
                    ctx.system.play_sound(sfx);
                }
            }
            CutsceneContent::Music(track) => {
                let track = track.as_deref().map(MusicTrack::named);
                ctx.system.music(track.as_ref());
            }
            CutsceneContent::Interact { actor, target } => {
                self.fire_interact(ctx, walkaround, actor, target)
            }
            // Apply camera steps in order so the scene's final camera target
            // matches full playback — though when a whole scene is skipped it
            // finishes and pops immediately, so `play_cutscene` re-derives the
            // camera from what's left on the stack (the parent, or the player).
            // A glide fast-forwards to landed (its end state), a shake to
            // spent — both transients gone, exactly as full playback ends.
            CutsceneContent::Camera(target, _) => {
                self.camera = Some(target.clone());
                self.glide = None;
            }
            CutsceneContent::Shake { .. } => self.shake = None,
            // A wait has no lasting effect, so fast-forwarding past it is a no-op.
            CutsceneContent::Wait(_) => {}
            CutsceneContent::Dialogue { .. } | CutsceneContent::Load(_) => {
                unreachable!("Dialogue/Load never reach skip_content — see this method's doc")
            }
        }
    }

    /// Snap wave-3 cue handlers during a scene skip. `handlers` is the
    /// current dialogue step's `on` list; `cues` is every cue the manual
    /// drain in [`skip`](Self::skip) surfaced (the *whole* conversation,
    /// replayed from the start).
    ///
    /// Any handler already in flight from live play before the B press (an
    /// existing [`HandlerRun`] in `self.handler_runs`) is snapped only from
    /// its current `step` onward — the steps before that already ran live
    /// and must not replay. Every other cue this drain surfaced that names a
    /// handler and hasn't already fired live is snapped in full, from the
    /// start (it never got to run at all). Either way each handler fires at
    /// most once, matching live play's dedupe.
    fn fire_and_snap_handlers<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
        handlers: &[CueHandler],
        cues: &[String],
    ) {
        let in_flight = std::mem::take(&mut self.handler_runs);
        let mut fired: std::collections::HashSet<usize> =
            in_flight.iter().map(|r| r.handler_index).collect();
        for run in in_flight {
            for content in &handlers[run.handler_index].content[run.step..] {
                self.skip_content(ctx, walkaround, content);
            }
        }
        for cue in cues {
            let Some(handler_index) = handlers.iter().position(|h| &h.cue == cue) else {
                continue;
            };
            if !fired.insert(handler_index) {
                continue;
            }
            for content in &handlers[handler_index].content {
                self.skip_content(ctx, walkaround, content);
            }
        }
    }

    /// Fast-forward to the end (the B-button abort): snap every remaining move to
    /// its end state and fire every remaining instant effect, so lasting side
    /// effects still land, then mark the cutscene finished. Each chain is snapped
    /// instruction-by-instruction (so a trailing `face` doesn't strand the actor
    /// mid-walk), including entity-relative and `record` moves. A `load` is
    /// chased — its sub-scene is launched, skipped, and cleaned up in place (never
    /// left on the stack) — so a skipped story scene can't silently drop a
    /// sub-scene's flags, sound/music, interacts, or map change.
    ///
    /// A `dialogue` step gets a full manual drain — `set_messages`, then
    /// `next_text` repeated (auto-picking any `#choice`'s first option, the
    /// same deterministic pick the scrubber's neutral input already relies
    /// on) until nothing's left — rather than just displaying the first
    /// page: previously this dropped every side effect (`#set`, `#cue`)
    /// after the first item, which contradicted this very doc's promise that
    /// lasting side effects still land on a skip. `#cue`s the drain surfaces
    /// fire (and snap) their `on` handlers exactly like a live-fired one —
    /// see [`fire_and_snap_handlers`](Self::fire_and_snap_handlers). A
    /// dialogue step whose box is already open mid-play is drained from
    /// where it stands, not restarted — its consumed effects (including an
    /// answered `#choice`) already happened live, exactly once.
    pub fn skip<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, walkaround: &mut WalkaroundState) {
        // Whether the step the skip lands on already has its box open
        // mid-play. Everything that box consumed so far — `#set`s, sounds, a
        // `#choice` the player actually answered — happened live, exactly
        // once; the drain below must pick up from the box as it stands, not
        // `set_messages`-restart the conversation, or those effects would
        // replay and an already-answered choice would be answered a *second*
        // time, with its first option, on top of the player's real pick.
        let mut live_box = matches!(self.state, StepState::Dialogue { opened: true, .. });
        while let Some(content) = self.content.get(self.step).cloned() {
            match &content {
                CutsceneContent::Dialogue { key, handlers } => {
                    if !live_box {
                        let convo = ctx.get_dialogue(key);
                        walkaround
                            .dialogue
                            .set_messages(ctx.system, ctx.font, ctx.save, &convo);
                    }
                    loop {
                        if walkaround.dialogue.is_choosing() {
                            walkaround.dialogue.confirm_choice(ctx.system, ctx.font, ctx.save);
                            continue;
                        }
                        if !walkaround.dialogue.next_text(ctx.system, ctx.font, ctx.save, true) {
                            break;
                        }
                    }
                    let cues = walkaround.dialogue.take_cues();
                    walkaround.dialogue.close();
                    self.fire_and_snap_handlers(ctx, walkaround, handlers, &cues);
                }
                CutsceneContent::Load(name) => {
                    // Chase the sub-scene so its lasting side effects still land,
                    // then drop its transient actors — all in place, so nothing is
                    // left running on the stack after the skip.
                    if let Some(def) = ctx.scenes.get_cutscene_resolved(name) {
                        let mut sub = Self::launch(&def, ctx, walkaround);
                        sub.skip(ctx, walkaround);
                        sub.cleanup(walkaround);
                    }
                }
                other => self.skip_content(ctx, walkaround, other),
            }
            self.step += 1;
            // Only the step that was mid-play when the skip began can own a
            // live box; every later step's conversation starts from nothing.
            live_box = false;
        }
        self.state = StepState::Done;
        self.handler_runs.clear();
    }

    /// Whether the cutscene has played every content step.
    pub fn is_finished(&self) -> bool {
        self.step >= self.content.len()
    }

    /// Resolve an actor `name` the way this scene's chains do: through the bound
    /// actor table (so a `spawn`ed actor resolves to its minted id), then the
    /// reserved `player`/`companion N`, then a bare id. A test seam for inspecting
    /// a spawned actor whose real id is a minted, opaque string.
    #[cfg(test)]
    pub(crate) fn resolve_actor(&self, name: &str) -> EntityId {
        resolve_name(name, &self.table)
    }
}

/// Resolve an actor name to an entity id: a bound/aliased name, else the
/// reserved `player`/`companion N`, else a bare id (a map creature by `Shell::id`).
fn resolve_name(name: &str, table: &HashMap<String, EntityId>) -> EntityId {
    if let Some(id) = table.get(name) {
        return id.clone();
    }
    if name == "player" {
        return EntityId::Player;
    }
    if let Some(slot) = name
        .strip_prefix("companion")
        .and_then(|s| s.trim().parse().ok())
    {
        return EntityId::PlayerCompanion(slot);
    }
    EntityId::Id(name.to_string())
}

/// Spawn a fresh shell of `preset` at `pos`, stamped with `id`, as a puppet
/// (`Wander` so it behaves as a creature after the scene; the AI loop is skipped
/// while a cutscene plays, so it doesn't self-move mid-scene). Pushed as a
/// top-level entity. For a transient `spawn` the caller passes a minted,
/// collision-proof id (see [`WalkaroundState::mint_spawn_id`]).
fn spawn_actor<S: ConsoleApi>(
    walkaround: &mut WalkaroundState,
    ctx: &mut Ctx<S>,
    id: &str,
    preset: &str,
    pos: Vec2,
) {
    let mut shell = ctx
        .presets
        .spawn(&crate::world::player::PresetId::new(preset))
        .unwrap_or_default();
    shell.pos = pos;
    shell.id = Some(id.to_string());
    shell.move_mode = MoveMode::Wander;
    walkaround.spawn_shell(shell);
}

/// Integer ceiling division for positive `a`, `b` (`b >= 1`).
fn div_ceil(a: i32, b: i32) -> i32 {
    (a + b - 1) / b
}

/// The cardinal facing from `from` toward `to` (the dominant axis).
fn facing_toward(from: Vec2, to: Vec2) -> (i8, i8) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    if dx.abs() >= dy.abs() {
        (dx.signum() as i8, 0)
    } else {
        (0, dy.signum() as i8)
    }
}

/// Whether a hold-style motion (face/teleport) is done: instant when `time == 0`,
/// else after holding for `time` frames.
fn time_done(time: u16, elapsed: u16) -> bool {
    time == 0 || elapsed + 1 >= time
}

/// Apply a `pose` motion (step or snap — the shared core both
/// [`step_motion`] and [`snap_motion`] call into): set `shell.pose` to
/// `name`, or clear it for `pose none`. A `name` the shell's preset has no
/// strip for is still set (`Shell::sprite_options` falls back to the walk
/// sprite when it draws), but logs once here, at the moment it's applied,
/// rather than every frame it's subsequently drawn.
fn apply_pose(shell: &mut Shell, actor: &str, name: &Option<String>) {
    if let Some(name) = name
        && !shell.sprites.poses.contains_key(name)
    {
        log::warn!("cutscene: actor `{actor}` has no pose named `{name}`; drawing its walk sprite instead");
    }
    shell.pose = name.clone();
}

/// Execute one frame of `motion` for the named `actor`. Returns `(finished,
/// progressed)` — `progressed` is `false` only when a move is blocked, which
/// drives the required-motion abort. An unresolvable actor logs and is skipped.
fn step_motion<S: ConsoleApi>(
    actor: &str,
    motion: &Motion,
    time: u16,
    elapsed: u16,
    table: &HashMap<String, EntityId>,
    ctx: &mut Ctx<S>,
    walkaround: &mut WalkaroundState,
) -> (bool, bool) {
    let actor_id = resolve_name(actor, table);
    let Some(path) = walkaround.resolve_path(&actor_id) else {
        log::info!("cutscene: actor `{actor}` not found, skipping motion");
        return (true, true);
    };
    // The live position of a named target, read before the actor is borrowed mut.
    let target_pos = |name: &str, w: &WalkaroundState| -> Option<Vec2> {
        w.resolve(&resolve_name(name, table)).map(|s| s.pos)
    };

    match motion {
        Motion::FaceDir(dx, dy) => {
            path.shell_mut(walkaround).face((*dx, *dy));
            (time_done(time, elapsed), true)
        }
        Motion::Pose(name) => {
            // Apply once, on the instruction's first frame — an `in N` budget
            // just holds the chain here afterward (like `FaceDir`); reapplying
            // (and re-warning) every held frame would be redundant.
            if elapsed == 0 {
                apply_pose(path.shell_mut(walkaround), actor, name);
            }
            (time_done(time, elapsed), true)
        }
        Motion::FaceEntity(name) => {
            if let Some(tp) = target_pos(name, walkaround) {
                let from = path.shell_ref(walkaround).pos;
                path.shell_mut(walkaround).face(facing_toward(from, tp));
            }
            (time_done(time, elapsed), true)
        }
        Motion::Teleport(p) => {
            path.shell_mut(walkaround).pos = *p;
            (time_done(time, elapsed), true)
        }
        Motion::MoveToPoint(p) => move_toward(path, *p, time, elapsed, false, ctx, walkaround),
        Motion::MoveToPointNoclip(p) => move_toward(path, *p, time, elapsed, true, ctx, walkaround),
        Motion::MoveToEntity(name) => match target_pos(name, walkaround) {
            Some(tp) => move_toward(path, tp, time, elapsed, false, ctx, walkaround),
            None => (true, true),
        },
        Motion::MoveBesideHorizontal { target, gap } => {
            let actor_w = path.shell_ref(walkaround).local_hitbox.w;
            let Some((spot, facing)) = beside_spot(target, *gap, actor_w, table, walkaround) else {
                return (true, true);
            };
            let (done, progressed) = move_toward(path, spot, time, elapsed, false, ctx, walkaround);
            // Look at the target once arrived — the per-frame walk overwrites
            // facing with the travel direction, so set the look-at on the last
            // frame (when the actor has reached the spot).
            if done {
                path.shell_mut(walkaround).face(facing);
            }
            (done, progressed)
        }
        Motion::Record { runs, noclip } => {
            (replay_record(path, runs, *noclip, elapsed, ctx, walkaround), true)
        }
        // Resolved into a `Motion::Record` at the registry boundary (see
        // `SceneFile::inline_paths`) before a def is ever launched — this arm
        // exists only so the match stays exhaustive; a no-op fallback if it's
        // somehow reached beats a panic mid-scene.
        Motion::Path { name, .. } => {
            log::warn!("cutscene: unresolved `path {name}` reached playback");
            (true, true)
        }
    }
}

/// Move `path`'s shell toward `target` one frame. Fixed-duration when `time > 0`
/// (speed inflated to arrive in exactly `time` frames); natural (1 px/frame) when
/// `time == 0`, capped by [`NATURAL_CAP`]. Returns whether the move is finished.
fn move_toward<S: ConsoleApi>(
    path: EntityPath,
    target: Vec2,
    time: u16,
    elapsed: u16,
    noclip: bool,
    ctx: &mut Ctx<S>,
    walkaround: &mut WalkaroundState,
) -> (bool, bool) {
    let pos = path.shell_ref(walkaround).pos;
    let remaining = Vec2::new(target.x - pos.x, target.y - pos.y);
    let cheby = remaining.x.abs().max(remaining.y.abs());
    // Already at the target counts as progress (no failure); a step that moves
    // the shell counts; a blocked step (pos unchanged) does not.
    let mut progressed = cheby == 0;
    if cheby != 0 {
        // Inflate speed so the remaining distance is covered in the remaining
        // budget (computed in i32 — a huge `time` would overflow i16).
        let speed = if time > 0 {
            let budget = time.saturating_sub(elapsed).max(1);
            div_ceil(i32::from(cheby), i32::from(budget)) as i16
        } else {
            1
        };
        let dx = remaining.x.signum() * remaining.x.abs().min(speed);
        let dy = remaining.y.signum() * remaining.y.abs().min(speed);
        // Index the disjoint fields directly so `current_map` can be borrowed
        // alongside the mutable shell (a `&mut Shell` from a method would not).
        let tiles = ctx.maps.get(&walkaround.current_map.source);
        let map = &walkaround.current_map;
        let shell = match path {
            EntityPath::Entity(i) => &mut walkaround.entities[i],
            EntityPath::Companion(i, j) => &mut walkaround.entities[i].companions[j],
        };
        let before = shell.pos;
        if noclip {
            shell.face((dx.signum() as i8, dy.signum() as i8));
            shell.pos.x += dx;
            shell.pos.y += dy;
            shell.animate_walk();
        } else {
            let (mdx, mdy) = shell.walk(ctx.system, dx, dy, false, map, tiles);
            shell.apply_motion(mdx, mdy);
        }
        // Blocked = the collision step left the shell exactly where it was.
        progressed = shell.pos != before;
        // A cutscene-driven actor does NOT drag its companions: during a scene
        // every actor is an explicit puppet (the author moves the dog with its
        // own chain if it should move), so e.g. `player: beside dog` walks the
        // player to the *stationary* dog instead of the dog sliding to the player.
    }
    let arrived = path.shell_ref(walkaround).pos == target;
    let done = if time > 0 {
        elapsed + 1 >= time
    } else {
        arrived || elapsed + 1 >= NATURAL_CAP
    };
    (done, progressed)
}

/// The point an actor (`actor_w` px wide) stands to be *beside* `target`, plus
/// the facing to look at it: `gap` px off the target's head-side (its horizontal
/// facing; a vertically-facing target → the actor's nearest side), feet aligned.
/// `None` if the target is absent. On the left side the actor's own width sets
/// the offset, so a wide actor lands flush rather than overlapping.
fn beside_spot(
    target: &str,
    gap: i16,
    actor_w: i16,
    table: &HashMap<String, EntityId>,
    walkaround: &WalkaroundState,
) -> Option<(Vec2, (i8, i8))> {
    let t = walkaround.resolve(&resolve_name(target, table))?;
    let t_box = t.hitbox();
    // Head-side: the target's horizontal facing; if it faces vertically, fall
    // back to whichever side the player is already on (no crossing over).
    let right_side = if t.dir.0 != 0 {
        t.dir.0 > 0
    } else {
        walkaround.player_ref().pos.x >= t.pos.x
    };
    let x = if right_side {
        t_box.x + t_box.w + gap
    } else {
        t_box.x - gap - actor_w
    };
    // Feet aligned: share the target's hitbox-top (both are flush boxes).
    let spot = Vec2::new(x, t.pos.y);
    let facing = if right_side { (-1, 0) } else { (1, 0) };
    Some((spot, facing))
}

/// Replay a recorded RLE path one frame: hold the run that `elapsed` falls in.
/// Finished once `elapsed` passes the total recorded frames.
fn replay_record<S: ConsoleApi>(
    path: EntityPath,
    runs: &[((i8, i8), u16)],
    noclip: bool,
    elapsed: u16,
    ctx: &mut Ctx<S>,
    walkaround: &mut WalkaroundState,
) -> bool {
    let total: u16 = runs.iter().map(|(_, n)| n).sum();
    if elapsed >= total {
        return true;
    }
    // Find the run this frame falls in.
    let mut acc = 0u16;
    let mut dir = (0i8, 0i8);
    for (d, n) in runs {
        acc += n;
        if elapsed < acc {
            dir = *d;
            break;
        }
    }
    let (dx, dy) = (dir.0 as i16, dir.1 as i16);
    let tiles = ctx.maps.get(&walkaround.current_map.source);
    let map = &walkaround.current_map;
    let shell = match path {
        EntityPath::Entity(i) => &mut walkaround.entities[i],
        EntityPath::Companion(i, j) => &mut walkaround.entities[i].companions[j],
    };
    if noclip {
        shell.face(dir);
        shell.pos.x += dx;
        shell.pos.y += dy;
        shell.animate_walk();
    } else {
        shell.apply_walk_direction(dx, dy);
        let (mdx, mdy) = shell.walk(ctx.system, dx, dy, false, map, tiles);
        shell.apply_motion(mdx, mdy);
    }
    // No companion drag during a cutscene (see `move_toward`).
    elapsed + 1 >= total
}

/// Snap a motion to its end state for the skip path: point moves and teleports
/// jump to their point; a `record` jumps by its cumulative RLE displacement;
/// entity-relative motions (`to`/`beside`/`face`) resolve against the target's
/// live (skip-time) position; faces apply. An entity-relative motion whose target
/// is unresolvable at skip time leaves the actor in place (best-effort).
fn snap_motion(
    actor: &str,
    motion: &Motion,
    table: &HashMap<String, EntityId>,
    walkaround: &mut WalkaroundState,
) {
    let Some(path) = walkaround.resolve_path(&resolve_name(actor, table)) else {
        return;
    };
    match motion {
        Motion::MoveToPoint(p) | Motion::MoveToPointNoclip(p) | Motion::Teleport(p) => {
            let shell = path.shell_mut(walkaround);
            shell.pos = *p;
            shell.animate_stop();
            shell.update_companions();
        }
        Motion::FaceDir(dx, dy) => {
            path.shell_mut(walkaround).face((*dx, *dy));
        }
        Motion::Pose(name) => apply_pose(path.shell_mut(walkaround), actor, name),
        Motion::MoveToEntity(name) => {
            // Resolve the target's position before borrowing the actor mutably.
            if let Some(tp) = walkaround.resolve(&resolve_name(name, table)).map(|s| s.pos) {
                let shell = path.shell_mut(walkaround);
                shell.pos = tp;
                shell.animate_stop();
                shell.update_companions();
            }
        }
        Motion::MoveBesideHorizontal { target, gap } => {
            let actor_w = path.shell_ref(walkaround).local_hitbox.w;
            if let Some((spot, facing)) = beside_spot(target, *gap, actor_w, table, walkaround) {
                let shell = path.shell_mut(walkaround);
                shell.pos = spot;
                shell.face(facing);
                shell.animate_stop();
                shell.update_companions();
            }
        }
        Motion::FaceEntity(name) => {
            if let Some(tp) = walkaround.resolve(&resolve_name(name, table)).map(|s| s.pos) {
                let from = path.shell_ref(walkaround).pos;
                path.shell_mut(walkaround).face(facing_toward(from, tp));
            }
        }
        Motion::Record { runs, .. } => {
            // The path's net displacement is the sum of each run's (heading ×
            // frames); best-effort (collision isn't re-walked at snap time).
            let dx: i16 = runs.iter().map(|(d, n)| d.0 as i16 * *n as i16).sum();
            let dy: i16 = runs.iter().map(|(d, n)| d.1 as i16 * *n as i16).sum();
            let last_dir = runs.iter().rev().map(|(d, _)| *d).find(|d| *d != (0, 0));
            let shell = path.shell_mut(walkaround);
            shell.pos.x += dx;
            shell.pos.y += dy;
            if let Some(dir) = last_dir {
                shell.face(dir);
            }
            shell.animate_stop();
            shell.update_companions();
        }
        // Resolved into a `Motion::Record` before a def is ever launched (see
        // `step_motion`'s matching arm) — a no-op fallback so the match stays
        // exhaustive without special-casing the skip path.
        Motion::Path { name, .. } => {
            log::warn!("cutscene: unresolved `path {name}` reached the skip path");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::portraits::Portraits;
    use crate::data::save::SaveData;
    use crate::data::scene;
    use crate::data::script::Script;
    use crate::draw_state::DrawState;
    use crate::platform::EggInput;
    use crate::platform::test_console::TestConsole;
    use crate::rand::Lcg64Xsh32;
    use crate::world::map::MapStore;

    /// Everything a [`Ctx`] borrows, owned in one place so a test can hand out a
    /// fresh `Ctx` each frame.
    struct Harness {
        system: TestConsole,
        /// This frame's input, threaded into the `Ctx` — a test injects presses
        /// here (e.g. a rising-edge direction to cancel an interruptible scene).
        input: EggInput,
        draw: DrawState,
        maps: MapStore,
        rng: Lcg64Xsh32,
        script: Script,
        scenes: scene::SceneFile,
        save: SaveData,
        items: crate::data::eggdata::GameItems,
        presets: crate::data::eggdata::Presets,
        font: crate::render::Font,
        walk: WalkaroundState,
    }
    impl Harness {
        fn new() -> Self {
            Self {
                system: TestConsole::new(),
                input: EggInput::new(),
                draw: DrawState::default(),
                maps: MapStore::default(),
                rng: Lcg64Xsh32::default(),
                script: Script::new(),
                scenes: scene::SceneFile::default(),
                save: SaveData::default(),
                items: crate::data::eggdata::GameItems::default(),
                presets: crate::data::eggdata::Presets::builtin(),
                font: crate::render::Font::blank(),
                walk: WalkaroundState::new(),
            }
        }
        /// Run `f` against a fresh `Ctx` + the held-apart walkaround (the split
        /// `play_cutscene` does with `self`).
        fn frame<R>(&mut self, f: impl FnOnce(&mut Ctx<TestConsole>, &mut WalkaroundState) -> R) -> R {
            let mut walk = std::mem::take(&mut self.walk);
            let r = {
                let mut ctx = Ctx {
                    draw: &mut self.draw,
                    system: &mut self.system,
                    input: &self.input,
                    maps: &mut self.maps,
                    rng: &mut self.rng,
                    script: &self.script,
                    scenes: &self.scenes,
                    save: &mut self.save,
                    items: &self.items,
                    presets: &self.presets,
                    font: &self.font,
                };
                f(&mut ctx, &mut walk)
            };
            self.walk = walk;
            r
        }
    }

    /// A spawned actor walks to a fixed point with a frame budget, arriving in
    /// exactly that many frames (speed inflated to suit).
    #[test]
    fn fixed_duration_move_arrives_on_time() {
        let def = scene::parse(
            "#cutscene t\n    spawn a critter 0 0\n    move\n        a: walk 30 0 in 10",
        )
        .unwrap();
        let def = def.get_cutscene("t").unwrap().clone();
        let mut h = Harness::new();
        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        for _ in 0..9 {
            h.frame(|ctx, w| cs.step(ctx, w));
        }
        let actor = cs.resolve_actor("a");
        // Not yet arrived one frame before the budget.
        let early = h.walk.resolve(&actor).unwrap().pos;
        assert!(early.x < 30, "still travelling at frame 9 ({})", early.x);
        h.frame(|ctx, w| cs.step(ctx, w));
        let arrived = h.walk.resolve(&actor).unwrap().pos;
        assert_eq!(arrived.x, 30, "arrived exactly at the budget");
    }

    /// The scrubber's re-sim core: a scene launched onto the world's own stack
    /// measures to its exact frame length, seeks to any frame, and does so
    /// deterministically — all on clones, leaving the live world untouched.
    #[test]
    fn scrubber_resim_measures_and_seeks_deterministically() {
        let def = scene::parse("#cutscene t\n    move\n        player: walk 30 0 in 10")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        let mut h = Harness::new();
        h.walk.player().pos = Vec2::new(0, 0);
        // What `open_scrubber` does: launch onto the world's own cutscene stack.
        let cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.walk.cutscene.push(cs);

        // `walk … in 10` runs exactly 10 frames then finishes.
        let total = h.frame(|ctx, w| w.replay_cutscene(50, ctx).total);
        assert_eq!(total, 10, "10-frame move measured");

        // Seek midway and to the end (a fresh re-sim from the snapshot each time).
        let mid = h.frame(|ctx, w| w.sim_cutscene_to(5, ctx));
        let end = h.frame(|ctx, w| w.sim_cutscene_to(total, ctx));
        assert!(
            (1..30).contains(&mid.player_ref().pos.x),
            "partway at frame 5: {}",
            mid.player_ref().pos.x
        );
        assert_eq!(end.player_ref().pos.x, 30, "arrived at the end");

        // Determinism: re-seeking the same frame yields the same world.
        let mid2 = h.frame(|ctx, w| w.sim_cutscene_to(5, ctx));
        assert_eq!(
            mid.player_ref().pos,
            mid2.player_ref().pos,
            "re-sim is deterministic"
        );
        // Re-sim never mutated the live world — its stack is still armed.
        assert_eq!(h.walk.cutscene.len(), 1, "live stack untouched by re-sim");
    }

    /// `spawn` adds a transient actor; finishing the cutscene removes it.
    #[test]
    fn spawn_is_transient() {
        let def = scene::parse("#cutscene t\n    spawn ghost critter 5 5\n    wait 1")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        let mut h = Harness::new();
        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        let ghost = cs.resolve_actor("ghost");
        assert!(
            h.walk.resolve(&ghost).is_some(),
            "spawned for the scene",
        );
        // wait 1 → one frame to finish.
        let outcome = h.frame(|ctx, w| cs.step(ctx, w));
        assert!(matches!(outcome, Outcome::Finished));
        cs.cleanup(&mut h.walk);
        assert!(
            h.walk.resolve(&ghost).is_none(),
            "removed on finish",
        );
    }

    /// A cutscene-driven player move does NOT drag its companion — the dog is
    /// suspended in place for the scene (it would otherwise slide onto the
    /// player's trail and warp). Guards the petting bug.
    #[test]
    fn cutscene_move_suspends_companions() {
        let def = scene::parse("#cutscene t\n    move\n        player: walk 80 50 in 4")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        let mut h = Harness::new();
        h.walk.player().pos = Vec2::new(50, 50);
        let mut dog = crate::world::player::Shell::default();
        dog.pos = Vec2::new(50, 66);
        dog.dir = (1, 0);
        dog.move_mode = MoveMode::Companion { slot: 0 };
        h.walk.player().companions.push(dog);

        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        for _ in 0..8 {
            h.frame(|ctx, w| {
                cs.step(ctx, w);
            });
        }
        assert!(h.walk.player_ref().pos.x > 50, "player moved by the cutscene");
        let dog = &h.walk.player_ref().companions[0];
        assert_eq!(
            dog.pos,
            Vec2::new(50, 66),
            "dog stayed put — not dragged onto the player's trail",
        );
        assert_eq!(dog.dir, (1, 0), "dog kept its facing — not turned by the scene");
    }

    /// An `interruptible` scene cancels (clears the stack) the frame the player
    /// presses a movement direction; without input it keeps playing.
    #[test]
    fn interruptible_scene_cancels_on_movement() {
        let def = scene::parse("#cutscene t interruptible\n    wait 100")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        let mut h = Harness::new();
        let cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.walk.cutscene.push(cs);

        // No input → still playing.
        h.frame(|ctx, w| {
            w.play_cutscene(ctx);
        });
        assert!(!h.walk.cutscene.is_empty(), "plays on with no input");

        // A just-pressed direction cancels it.
        h.input.controllers[0].up = [true, false];
        h.frame(|ctx, w| {
            w.play_cutscene(ctx);
        });
        assert!(h.walk.cutscene.is_empty(), "movement cancelled the scene");
    }

    /// Determinism: the same cutscene from the same start produces byte-identical
    /// results (no RNG during a scene — the entity-AI loop is skipped). This is
    /// the property the scrubber's re-sim (Phase C) relies on.
    #[test]
    fn replay_is_deterministic() {
        let def = scene::parse(
            "#cutscene t\n    spawn a critter 0 0\n    move\n        a: walk 20 10 in 8; face 1 0\n    wait 3",
        )
        .unwrap()
        .get_cutscene("t")
        .unwrap()
        .clone();
        let run = |def: &CutsceneDef| {
            let mut h = Harness::new();
            let mut cs = h.frame(|ctx, w| Cutscene::launch(def, ctx, w));
            for _ in 0..20 {
                h.frame(|ctx, w| {
                    cs.step(ctx, w);
                });
            }
            let actor = cs.resolve_actor("a");
            h.walk.resolve(&actor).map(|s| (s.pos, s.dir, s.walktime))
        };
        assert_eq!(run(&def), run(&def), "two identical runs match exactly");
    }

    /// The shipped `pet_dog` builds (requisitions) without panicking.
    #[test]
    fn shipped_pet_dog_launches() {
        let scenes = scene::parse(include_str!("../../../../assets/data/main.eggscene"))
            .expect("parse main.eggscene");
        let def = scenes.get_cutscene("pet_dog").expect("pet_dog defined").clone();
        let mut h = Harness::new();
        let cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        assert_eq!(cs.content.len(), 2, "move + interact");
    }

    /// A `spawn` whose author name collides with a live map creature's id gets a
    /// minted, collision-proof id: the scene resolves its actor to the spawned
    /// shell (not the creature), and cleanup removes only the spawned shell —
    /// the creature survives.
    #[test]
    fn spawn_cleanup_spares_a_name_colliding_creature() {
        let def = scene::parse("#cutscene t\n    spawn dog critter 5 5\n    wait 1")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        let mut h = Harness::new();
        // A live creature already carries the id the scene `spawn`s under.
        let mut creature = crate::world::player::Shell::default();
        creature.id = Some("dog".to_string());
        creature.pos = Vec2::new(99, 99);
        h.walk.spawn_shell(creature);

        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        let spawned = cs.resolve_actor("dog");
        assert!(h.walk.resolve(&spawned).is_some(), "the spawned actor exists");
        assert_ne!(
            spawned,
            EntityId::Id("dog".into()),
            "the spawned actor has a minted id, not the author name",
        );

        let outcome = h.frame(|ctx, w| cs.step(ctx, w));
        assert!(matches!(outcome, Outcome::Finished));
        cs.cleanup(&mut h.walk);

        let survivor = h.walk.resolve(&EntityId::Id("dog".into()));
        assert!(survivor.is_some(), "the name-colliding creature survived cleanup");
        assert_eq!(
            survivor.unwrap().pos,
            Vec2::new(99, 99),
            "it's the original creature, untouched",
        );
        assert!(h.walk.resolve(&spawned).is_none(), "the spawned actor was removed");
    }

    /// Skipping (B) a scene that only `load`s a child still lands the child's
    /// lasting side effects — the sub-scene is launched, skipped, and cleaned up
    /// in place rather than dropped.
    #[test]
    fn skip_chases_a_loaded_subscene() {
        let file = scene::parse(
            "#cutscene parent\n    load child\n#cutscene child\n    set seen true\n    wait 100",
        )
        .unwrap();
        let def = file.get_cutscene("parent").unwrap().clone();
        let mut h = Harness::new();
        h.scenes = file;
        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.frame(|ctx, w| cs.skip(ctx, w));
        assert!(h.save.flag("seen"), "the loaded sub-scene's flag was set by the skip");
        assert!(cs.is_finished(), "the parent skipped to the end");
    }

    /// Skipping a recorded-path scene snaps the actor by the path's cumulative
    /// RLE displacement (was a no-op — the actor stayed at the start).
    #[test]
    fn skip_snaps_a_recorded_path_to_its_end() {
        let def = scene::parse("#cutscene t\n    move\n        player: record noclip 1 0 10 0 1 5")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        let mut h = Harness::new();
        h.walk.player().pos = Vec2::new(0, 0);
        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.frame(|ctx, w| cs.skip(ctx, w));
        assert_eq!(
            h.walk.player_ref().pos,
            Vec2::new(10, 5),
            "player snapped to the recorded path's cumulative end",
        );
    }

    /// Skipping snaps an entity-relative move (`to NAME`) to its target's live
    /// (skip-time) position (was left un-snapped at the start).
    #[test]
    fn skip_snaps_a_relative_move_to_its_target() {
        let def = scene::parse(
            "#cutscene t\n    spawn goal critter 40 20\n    move\n        player: to goal",
        )
        .unwrap()
        .get_cutscene("t")
        .unwrap()
        .clone();
        let mut h = Harness::new();
        h.walk.player().pos = Vec2::new(0, 0);
        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.frame(|ctx, w| cs.skip(ctx, w));
        assert_eq!(
            h.walk.player_ref().pos,
            Vec2::new(40, 20),
            "player snapped onto its live target",
        );
    }

    /// A wide actor walking `beside` a target lands by its OWN width, not the
    /// player's — flush against the target rather than offset.
    #[test]
    fn beside_uses_the_moving_actors_own_width() {
        let def = scene::parse(
            "#cutscene t\n\
             \x20   spawn mover critter 0 50\n\
             \x20   spawn target critter 100 50\n\
             \x20   move\n\
             \x20       mover: beside target in 8",
        )
        .unwrap()
        .get_cutscene("t")
        .unwrap()
        .clone();
        let mut h = Harness::new();
        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        let mover_id = cs.resolve_actor("mover");
        let target_id = cs.resolve_actor("target");
        // The target faces left, so the mover stands on its left — the side whose
        // offset is the mover's own width. Give the mover a distinctly wide hitbox.
        let player_w = h.walk.player_ref().local_hitbox.w;
        let wide = player_w + 13;
        {
            let tp = h.walk.resolve_path(&target_id).unwrap();
            tp.shell_mut(&mut h.walk).dir = (-1, 0);
            let mp = h.walk.resolve_path(&mover_id).unwrap();
            mp.shell_mut(&mut h.walk).local_hitbox.w = wide;
        }
        for _ in 0..12 {
            h.frame(|ctx, w| {
                cs.step(ctx, w);
            });
        }
        let target_box_x = h.walk.resolve(&target_id).unwrap().hitbox().x;
        let mover = h.walk.resolve(&mover_id).unwrap();
        assert_eq!(
            mover.pos.x,
            target_box_x - wide,
            "landed by its own width ({wide}), not the player's ({player_w})",
        );
    }

    /// A `#choice` inside a cutscene resolves deterministically under the
    /// scrubber's neutral input: A held as a permanent rising edge + a neutral
    /// dpad auto-picks the FIRST option (mirroring plain-dialogue auto-advance),
    /// so a re-sim is reproducible. Here that means the first option's flag is
    /// set and the cutscene finishes.
    #[test]
    fn cutscene_choice_auto_picks_the_first_option() {
        let mut h = Harness::new();
        h.script.set_base(
            crate::data::script::eggtext::parse(
                "#flag picked_a\n#flag picked_b\n#dialogue ask\n\
                 \x20   Pick one:\n\
                 \x20   #choice\n\
                 \x20   #option A\n\
                 \x20   #set picked_a true\n\
                 \x20   #option B\n\
                 \x20   #set picked_b true",
            )
            .unwrap(),
            &Portraits::builtin(),
        );
        let def = scene::parse("#cutscene t\n    dialogue ask")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        // The scrubber's sim_input: A a permanent rising edge, dpad neutral.
        h.input.controllers[0].a = [true, false];

        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        let mut finished = false;
        for _ in 0..60 {
            if matches!(h.frame(|ctx, w| cs.step(ctx, w)), Outcome::Finished) {
                finished = true;
                break;
            }
        }
        assert!(finished, "the dialogue choice cutscene ran to completion");
        assert!(h.save.flag("picked_a"), "auto-picked the first option");
        assert!(!h.save.flag("picked_b"));
    }

    // --- wave 3: `dialogue` `on NAME [wait]` handlers ---

    /// Install `src` as the base script (against the built-in portraits) — the
    /// wave-3 tests' shorthand for `h.script.set_base(...)`.
    fn install_script(h: &mut Harness, src: &str) {
        h.script.set_base(
            crate::data::script::eggtext::parse(src).expect("parse eggtext"),
            &Portraits::builtin(),
        );
    }

    /// A `#cue` fires its `on` handler, and the handler's `move` completes
    /// while the dialogue box is still open (never pressed A) — proving cues
    /// and handlers tick independently of, and concurrently with, the box.
    #[test]
    fn cue_fires_handler_and_it_completes_while_box_stays_open() {
        let mut h = Harness::new();
        install_script(
            &mut h,
            "#dialogue talk\n    #cue arrive\n    Hi there, this stays open.",
        );
        let def = scene::parse(
            "#cutscene t\n\
             \x20   spawn guy critter 0 0\n\
             \x20   dialogue talk\n\
             \x20       on arrive\n\
             \x20           move\n\
             \x20               guy: walk 10 0 in 5",
        )
        .unwrap()
        .get_cutscene("t")
        .unwrap()
        .clone();

        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        let guy = cs.resolve_actor("guy");
        // No A press at all — the box has nothing forcing it to advance.
        for _ in 0..6 {
            h.frame(|ctx, w| cs.step(ctx, w));
        }
        assert_eq!(
            h.walk.resolve(&guy).unwrap().pos,
            Vec2::new(10, 0),
            "the handler's move completed",
        );
        assert!(
            h.walk.dialogue.is_active(),
            "the box is still open — nothing ever asked it to close",
        );
    }

    /// While a `wait` handler is running, the box freezes: it doesn't close
    /// even though the player holds A and the line is already fully shown
    /// (a single-character line is line-done the instant it's set); once the
    /// handler finishes, the box closes on its own, still without a fresh
    /// press.
    #[test]
    fn wait_handler_freezes_the_box_then_releases_it() {
        let mut h = Harness::new();
        install_script(&mut h, "#dialogue talk\n    #cue meltdown\n    !");
        let def = scene::parse(
            "#cutscene t\n    dialogue talk\n        on meltdown wait\n            wait 3",
        )
        .unwrap()
        .get_cutscene("t")
        .unwrap()
        .clone();
        // A permanent rising edge (held from frame one) — if the box were
        // ever unfrozen with its line already done, it would close the very
        // next frame.
        h.input.controllers[0].a = [true, false];

        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.frame(|ctx, w| cs.step(ctx, w)); // frame 1: opens, cue fires, wait 3→2
        assert!(h.walk.dialogue.is_active(), "just opened");

        h.frame(|ctx, w| cs.step(ctx, w)); // frame 2: frozen, wait 2→1
        assert!(h.walk.dialogue.is_active(), "still frozen: held despite A + line-done");
        h.frame(|ctx, w| cs.step(ctx, w)); // frame 3: frozen, wait 1→0 (finishes this tick)
        assert!(
            h.walk.dialogue.is_active(),
            "still frozen this frame — the freeze check reflects the PREVIOUS frame's state",
        );

        let outcome = h.frame(|ctx, w| cs.step(ctx, w)); // frame 4: freeze lifted
        assert!(
            !h.walk.dialogue.is_active(),
            "released the instant the handler finished, no extra press needed",
        );
        assert!(matches!(outcome, Outcome::Finished), "and the step (and scene) completed");
    }

    /// A non-`wait` handler doesn't freeze the box — it can close on schedule
    /// — but the whole `dialogue` step only completes once the handler
    /// itself finishes, even though the box closed several frames earlier.
    #[test]
    fn non_wait_handler_outlives_the_box_close() {
        let mut h = Harness::new();
        install_script(&mut h, "#dialogue talk\n    #cue go\n    !");
        let def = scene::parse("#cutscene t\n    dialogue talk\n        on go\n            wait 5")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        h.input.controllers[0].a = [true, false]; // permanent rising edge

        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.frame(|ctx, w| cs.step(ctx, w)); // frame 1: opens, cue fires, handler wait 5→4

        let outcome = h.frame(|ctx, w| cs.step(ctx, w)); // frame 2: not frozen (non-wait) — box closes
        assert!(matches!(outcome, Outcome::Running), "handler still running");
        assert!(!h.walk.dialogue.is_active(), "the box closed on schedule");

        for _ in 0..2 {
            let outcome = h.frame(|ctx, w| cs.step(ctx, w));
            assert!(
                matches!(outcome, Outcome::Running),
                "step not done — the handler is still going after the box closed",
            );
        }
        let outcome = h.frame(|ctx, w| cs.step(ctx, w)); // handler's wait 5 finally runs out
        assert!(matches!(outcome, Outcome::Finished), "step completes once the handler finishes");
    }

    /// The endgame case: a `#cue` on the very LAST content item, consumed by
    /// the same advance that would otherwise close the box, must still fire
    /// its handler — and if that handler is `wait`, the close is suppressed
    /// and remembered (not dropped), landing the moment the handler finishes
    /// without the player pressing A again.
    #[test]
    fn final_item_cue_still_fires_and_suppresses_the_close() {
        let mut h = Harness::new();
        // A single-character line is line-done the instant it's shown (no
        // typewriter pacing to account for), so the frame the trailing cue
        // is reached is pinned exactly: the very first non-opening frame.
        install_script(&mut h, "#dialogue talk\n    !\n    #cue final");
        let def = scene::parse(
            "#cutscene t\n    dialogue talk\n        on final wait\n            wait 3",
        )
        .unwrap()
        .get_cutscene("t")
        .unwrap()
        .clone();
        h.input.controllers[0].a = [true, false]; // permanent rising edge

        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.frame(|ctx, w| cs.step(ctx, w)); // opens, shows "!"
        assert!(h.walk.dialogue.is_active());

        // Next frame: the line is already done, so `tick`'s own auto-advance
        // consumes the trailing `#cue final` (nothing left after it) and the
        // widget phase decides to close — but the same drain fires `on
        // final`, which freezes it before the close applies.
        let outcome = h.frame(|ctx, w| cs.step(ctx, w));
        assert!(matches!(outcome, Outcome::Running));
        assert!(
            h.walk.dialogue.is_active(),
            "the close was suppressed by the handler its own cue just started",
        );
        // Release A — the remembered close must not need it held or pressed
        // again.
        h.input.controllers[0].a = [false, false];

        let outcome = h.frame(|ctx, w| cs.step(ctx, w)); // handler's wait 3→2→1
        assert!(matches!(outcome, Outcome::Running), "still frozen");
        assert!(h.walk.dialogue.is_active(), "still frozen");

        let outcome = h.frame(|ctx, w| cs.step(ctx, w)); // wait 1→0: handler finishes
        assert!(
            !h.walk.dialogue.is_active(),
            "the remembered close finally landed, with no further input",
        );
        assert!(matches!(outcome, Outcome::Finished));
    }

    /// The same cue name fires twice in one `dialogue` step (two `#cue go`
    /// beats in the same conversation) — the second firing is ignored: only
    /// one [`HandlerRun`] is ever created for `on go`.
    #[test]
    fn duplicate_cue_firing_is_deduped() {
        let mut h = Harness::new();
        // Single-character messages are line-done the instant they're shown,
        // so one permanently-held A press reliably advances one message per
        // frame — no typewriter pacing to account for.
        install_script(&mut h, "#dialogue talk\n    #cue go\n    .\n\n    #cue go\n    !");
        let def = scene::parse("#cutscene t\n    dialogue talk\n        on go\n            wait 20")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        h.input.controllers[0].a = [true, false]; // permanent rising edge

        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.frame(|ctx, w| cs.step(ctx, w)); // opens on ".", first `go` fires
        assert_eq!(cs.handler_runs.len(), 1, "one run after the first firing");

        // One frame consumes the `Pause` between messages; the next actually
        // crosses into "!" — the second `#cue go` bank + drain happens here;
        // it must not add a second run for the same handler.
        for _ in 0..2 {
            h.frame(|ctx, w| cs.step(ctx, w));
        }
        assert_eq!(
            h.walk.dialogue.current_text.as_deref(),
            Some("!"),
            "sanity check: message 2 (where the repeat `go` lives) was actually reached",
        );
        assert_eq!(cs.handler_runs.len(), 1, "the repeat firing was deduped, not a second run");
    }

    /// `skip()` (the B-button abort) mid-conversation: a `#set` after the
    /// FIRST content item still lands (the bug this wave fixes — previously
    /// only the first item's effects survived a skip), and a handler cue
    /// mid-conversation is snapped — an already-in-flight run (some of it
    /// already played live) only from where it left off, never replaying the
    /// steps that already ran.
    #[test]
    fn skip_lands_mid_conversation_set_and_snaps_an_in_flight_handler() {
        let mut h = Harness::new();
        // A single-character first message is line-done at once — no
        // typewriter pacing to account for before the advance into message 2
        // (a page break still costs its own frame, to consume the `Pause`
        // item between messages, before the next press actually crosses
        // into it).
        install_script(
            &mut h,
            "#flag landed\n#dialogue talk\n    .\n\n    #set landed true\n    #cue go\n    Second.",
        );
        let def = scene::parse(
            "#cutscene t\n\
             \x20   spawn guy critter 0 0\n\
             \x20   dialogue talk\n\
             \x20       on go\n\
             \x20           move\n\
             \x20               guy: record 1 0 3\n\
             \x20           move\n\
             \x20               guy: record 1 0 5",
        )
        .unwrap()
        .get_cutscene("t")
        .unwrap()
        .clone();
        h.input.controllers[0].a = [true, false]; // permanent rising edge

        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        let guy = cs.resolve_actor("guy");
        h.frame(|ctx, w| cs.step(ctx, w)); // opens on "."
        // One frame consumes the `Pause` between messages; the next crosses
        // into message 2 — `landed` sets, `go` fires and gets its first live
        // tick (the handler's first `record` step starts) — then a couple
        // more frames let that first `record` step finish live and the
        // second one get partway through, live.
        for _ in 0..4 {
            h.frame(|ctx, w| cs.step(ctx, w));
        }
        let mid = h.walk.resolve(&guy).unwrap().pos.x;
        assert!(mid > 0, "the handler made live progress before the skip: x={mid}");

        h.frame(|ctx, w| cs.skip(ctx, w));

        assert!(h.save.flag("landed"), "the mid-conversation #set landed on skip");
        assert_eq!(
            h.walk.resolve(&guy).unwrap().pos.x,
            9,
            "first record (+3) ran once live, second (+5) snapped once from wherever \
             it was — not replayed from the start and not double-fired by the manual \
             drain rediscovering the same `go` cue",
        );
    }

    /// A skip that lands on a mid-play dialogue step drains the box from
    /// where it stands — it must not `set_messages`-restart the
    /// conversation. The observable stake: a `#choice` the player already
    /// answered live would be answered a second time by the restarted
    /// drain's deterministic first-option pick, firing the *first* option's
    /// flags on top of the player's real pick.
    #[test]
    fn skip_keeps_a_live_answered_choice_and_drains_whats_left() {
        let mut h = Harness::new();
        install_script(
            &mut h,
            "#flag picked_a\n#flag picked_b\n#flag epilogue\n#dialogue ask\n\
             \x20   .\n\
             \x20   #choice\n\
             \x20   #option A\n\
             \x20   #set picked_a true\n\
             \x20   #option B\n\
             \x20   #set picked_b true\n\
             \n\
             \x20   #set epilogue true\n\
             \x20   Done.",
        );
        let def = scene::parse("#cutscene t\n    dialogue ask")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();

        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.frame(|ctx, w| cs.step(ctx, w)); // opens on "." (line-done at once)
        h.input.controllers[0].a = [true, false];
        h.frame(|ctx, w| cs.step(ctx, w)); // A: advance into the choice menu
        assert!(h.walk.dialogue.is_choosing(), "the menu is open");
        h.input.controllers[0].a = [false, true];
        h.input.controllers[0].down = [true, false];
        h.frame(|ctx, w| cs.step(ctx, w)); // highlight option B
        h.input.controllers[0].down = [false, true];
        h.input.controllers[0].a = [true, false];
        h.frame(|ctx, w| cs.step(ctx, w)); // pick it, live
        assert!(h.save.flag("picked_b"), "the player's live pick landed");
        assert!(!h.save.flag("epilogue"), "message 2 not yet reached");

        h.frame(|ctx, w| cs.skip(ctx, w));

        assert!(
            !h.save.flag("picked_a"),
            "the answered choice was not re-answered with option A by a restarted drain"
        );
        assert!(h.save.flag("picked_b"), "the real pick still stands");
        assert!(h.save.flag("epilogue"), "the not-yet-reached #set still landed");
        assert!(!h.walk.dialogue.is_active(), "the box closed");
    }

    /// A dialogue box opened by a top-level `interact` step (not a
    /// `dialogue KEY` step) engages no handlers, even if the dialogue it
    /// happens to show contains a `#cue` — there's no `on` list to check
    /// cues against (the content is `Interact`, not `Dialogue`), so nothing
    /// panics and no `HandlerRun` is ever created.
    #[test]
    fn interact_opened_dialogue_engages_no_handlers() {
        let mut h = Harness::new();
        install_script(&mut h, "#dialogue greet\n    #cue hello\n    Hi there.");
        let def = scene::parse("#cutscene t\n    spawn npc critter 0 0\n    interact player npc")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();

        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        let npc = cs.resolve_actor("npc");
        h.frame(|_, w| {
            let path = w.resolve_path(&npc).unwrap();
            path.shell_mut(w).interaction =
                Some(crate::world::interact::Interaction::Dialogue("greet".into()));
        });

        h.input.controllers[0].a = [true, false]; // permanent rising edge
        let mut finished = false;
        for _ in 0..10 {
            assert!(cs.handler_runs.is_empty(), "no handlers to engage from an Interact step");
            if matches!(h.frame(|ctx, w| cs.step(ctx, w)), Outcome::Finished) {
                finished = true;
                break;
            }
        }
        assert!(finished, "the interact-opened dialogue played to completion without panicking");
    }

    // --- camera verb ---

    use crate::world::camera::{Camera, CameraBounds};

    /// The camera position `Camera::center_on` produces for a focus point under
    /// unbounded (Free) framing — the reference a camera-verb test compares
    /// against, so it tracks the real centring/screen size rather than hardcoding.
    fn center_ref(focus: Vec2, system: &TestConsole) -> Vec2 {
        let mut cam = Camera::new(Vec2::new(0, 0), CameraBounds::free());
        cam.center_on(focus.x, focus.y, system.width() as i16, system.height() as i16);
        cam.pos
    }

    /// Launch `src`'s single scene and arm it on the world's own stack (Free
    /// camera bounds, so tests read the raw centring), with the player at `player`.
    fn arm(src: &str, player: Vec2) -> Harness {
        let def = scene::parse(src)
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        let mut h = Harness::new();
        h.walk.player().pos = player;
        h.walk.camera.bounds = CameraBounds::free();
        let cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.walk.cutscene.push(cs);
        h
    }

    /// `camera ACTOR` centres the camera on that actor's live position (with the
    /// same +4/-2 framing the follow camera uses), not the player.
    #[test]
    fn camera_verb_follows_an_actor() {
        // The actor is stationary at (100, 80); no chain moves it.
        let mut h = arm(
            "#cutscene t\n    spawn a critter 100 80\n    camera a\n    wait 30",
            Vec2::new(0, 0),
        );
        h.frame(|ctx, w| {
            w.play_cutscene(ctx);
        });
        assert_eq!(
            h.walk.camera.pos,
            center_ref(Vec2::new(104, 78), &h.system),
            "camera centres on the followed actor (+4/-2)",
        );
        assert_ne!(
            h.walk.camera.pos,
            center_ref(Vec2::new(4, -2), &h.system),
            "it left the player",
        );
    }

    /// `camera X Y` centres the camera exactly on that fixed map point.
    #[test]
    fn camera_verb_holds_a_fixed_point() {
        let mut h = arm(
            "#cutscene t\n    camera 150 90\n    wait 30",
            Vec2::new(0, 0),
        );
        h.frame(|ctx, w| {
            w.play_cutscene(ctx);
        });
        assert_eq!(
            h.walk.camera.pos,
            center_ref(Vec2::new(150, 90), &h.system),
            "camera centres exactly on the authored point (no hitbox offset)",
        );
    }

    /// When the scene ends the camera lands back on the player, with no per-scene
    /// target left stranding it.
    #[test]
    fn camera_returns_to_player_when_the_scene_ends() {
        let mut h = arm(
            "#cutscene t\n    camera 150 90\n    wait 3",
            Vec2::new(30, 20),
        );
        while h.frame(|ctx, w| w.play_cutscene(ctx)) {}
        assert!(h.walk.cutscene.is_empty(), "scene finished");
        assert_eq!(
            h.walk.camera.pos,
            center_ref(Vec2::new(34, 18), &h.system),
            "camera restored to the player at scene end",
        );
    }

    /// Skipping (B) mid-scene leaves the camera exactly where full playback would:
    /// the scene finishes, so the camera is back on the player rather than stranded
    /// on the mid-scene target.
    #[test]
    fn camera_skip_matches_full_playback() {
        let src = "#cutscene t\n    camera 150 90\n    wait 100";
        let player = Vec2::new(30, 20);

        // Full playback to the end.
        let mut full = arm(src, player);
        while full.frame(|ctx, w| w.play_cutscene(ctx)) {}
        let full_end = full.walk.camera.pos;

        // Skip: one frame arms the camera on the target, the next presses B.
        let mut skip = arm(src, player);
        skip.frame(|ctx, w| {
            w.play_cutscene(ctx);
        });
        assert_eq!(
            skip.walk.camera.pos,
            center_ref(Vec2::new(150, 90), &skip.system),
            "camera is on the target mid-scene, before the skip",
        );
        skip.input.controllers[0].b = [true, false];
        skip.frame(|ctx, w| {
            w.play_cutscene(ctx);
        });

        assert!(skip.walk.cutscene.is_empty(), "B skipped the scene to the end");
        assert_eq!(
            skip.walk.camera.pos, full_end,
            "skipped camera end-state matches full playback",
        );
        assert_eq!(
            full_end,
            center_ref(Vec2::new(34, 18), &skip.system),
            "and both are back on the player",
        );
    }

    /// A sub-scene sets its own camera focus; when it pops, the parent's focus is
    /// restored, and the player once the whole stack drains.
    #[test]
    fn camera_restores_parent_focus_after_a_subscene() {
        let file = scene::parse(
            "#cutscene parent\n    camera 150 90\n    load child\n    wait 5\n\
             #cutscene child\n    camera 40 40\n    wait 5",
        )
        .unwrap();
        let def = file.get_cutscene("parent").unwrap().clone();
        let mut h = Harness::new();
        h.scenes = file;
        h.walk.player().pos = Vec2::new(30, 20);
        h.walk.camera.bounds = CameraBounds::free();
        let cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.walk.cutscene.push(cs);

        // Two frames in, the child is driving: its fixed-point focus is live.
        for _ in 0..2 {
            h.frame(|ctx, w| {
                w.play_cutscene(ctx);
            });
        }
        assert_eq!(
            h.walk.camera.pos,
            center_ref(Vec2::new(40, 40), &h.system),
            "child's focus while it plays",
        );

        // The child (a 5-frame wait) pops; the parent — its focus restored — is now
        // the top of the stack, still on its own wait.
        for _ in 0..6 {
            h.frame(|ctx, w| {
                w.play_cutscene(ctx);
            });
        }
        assert_eq!(
            h.walk.camera.pos,
            center_ref(Vec2::new(150, 90), &h.system),
            "parent's focus restored after the sub-scene ended",
        );

        while h.frame(|ctx, w| w.play_cutscene(ctx)) {}
        assert_eq!(
            h.walk.camera.pos,
            center_ref(Vec2::new(34, 18), &h.system),
            "back on the player once the whole stack drained",
        );
    }

    /// A fixed-point target still clamps to the map's `camera_bounds` (the centring
    /// routes through `Camera::center_on`, so bounds are honoured).
    #[test]
    fn camera_target_respects_map_bounds() {
        let def = scene::parse("#cutscene t\n    camera 9000 9000\n    wait 30")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        let mut h = Harness::new();
        // Default walkaround bounds are the tight `Camera::default()` range
        // (x∈[0,60], y∈[0,64]); a far-off target must clamp into it.
        let cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        h.walk.cutscene.push(cs);
        h.frame(|ctx, w| {
            w.play_cutscene(ctx);
        });
        assert_eq!(
            h.walk.camera.pos,
            Vec2::new(60, 64),
            "far target clamped to the map's camera bounds",
        );
    }

    /// The camera is pure engine state, so the scrubber's re-sim reproduces it: two
    /// seeks to the same frame land the camera identically, and it has left the
    /// player to track the moving actor it follows.
    #[test]
    fn camera_is_deterministic_under_resim() {
        let mut h = arm(
            "#cutscene t\n    spawn a critter 60 40\n    camera a\n    move\n        a: walk 90 40 in 8\n    wait 3",
            Vec2::new(0, 0),
        );
        let a = h.frame(|ctx, w| w.sim_cutscene_to(6, ctx));
        let b = h.frame(|ctx, w| w.sim_cutscene_to(6, ctx));
        assert_eq!(a.camera.pos, b.camera.pos, "re-sim reproduces the camera");
        assert_ne!(
            a.camera.pos,
            center_ref(Vec2::new(4, -2), &h.system),
            "camera followed the moving actor, not the player",
        );
    }

    /// `camera X Y over N` glides instead of cutting: the retarget frame holds
    /// the old framing (no snap), the pan advances monotonically toward the
    /// target, and after N frames it sits exactly where the instant cut would.
    #[test]
    fn camera_glide_eases_to_the_point() {
        let mut h = arm(
            "#cutscene t\n    camera 150 90 over 20\n    wait 60",
            Vec2::new(0, 0),
        );
        let start = h.walk.camera.pos;
        let target = center_ref(Vec2::new(150, 90), &h.system);
        assert!(
            target.x > start.x && target.y > start.y,
            "test geometry: the glide heads down-right",
        );
        h.frame(|ctx, w| {
            w.play_cutscene(ctx);
        });
        assert_eq!(h.walk.camera.pos, start, "no snap on the retarget frame");
        let mut prev = start;
        for _ in 0..10 {
            h.frame(|ctx, w| {
                w.play_cutscene(ctx);
            });
            let pos = h.walk.camera.pos;
            assert!(pos.x >= prev.x && pos.y >= prev.y, "the pan is monotonic");
            prev = pos;
        }
        // Halfway in it's genuinely en route — strictly past the start, short of
        // the target.
        assert!(prev.x > start.x && prev.x < target.x, "mid-glide, en route");
        for _ in 0..10 {
            h.frame(|ctx, w| {
                w.play_cutscene(ctx);
            });
        }
        assert_eq!(h.walk.camera.pos, target, "landed exactly on the target");
        h.frame(|ctx, w| {
            w.play_cutscene(ctx);
        });
        assert_eq!(h.walk.camera.pos, target, "and stays there");
    }

    /// `camera ACTOR over N` lands on the actor's follow framing (+4/-2), same
    /// as the instant form.
    #[test]
    fn camera_glide_lands_on_an_actor() {
        let mut h = arm(
            "#cutscene t\n    spawn a critter 100 80\n    camera a over 10\n    wait 30",
            Vec2::new(0, 0),
        );
        for _ in 0..12 {
            h.frame(|ctx, w| {
                w.play_cutscene(ctx);
            });
        }
        assert_eq!(
            h.walk.camera.pos,
            center_ref(Vec2::new(104, 78), &h.system),
            "glide landed on the actor's follow framing",
        );
    }

    /// Skipping (B) mid-glide and mid-shake leaves the camera exactly where full
    /// playback would — the transients fast-forward to spent, the scene pops, and
    /// the camera is back on the player.
    #[test]
    fn camera_glide_and_shake_skip_match_full_playback() {
        let src = "#cutscene t\n    camera 150 90 over 40\n    shake 20 3\n    wait 100";
        let player = Vec2::new(30, 20);

        let mut full = arm(src, player);
        while full.frame(|ctx, w| w.play_cutscene(ctx)) {}
        let full_end = full.walk.camera.pos;

        // Two frames in — glide and shake both mid-flight — press B.
        let mut skip = arm(src, player);
        for _ in 0..2 {
            skip.frame(|ctx, w| {
                w.play_cutscene(ctx);
            });
        }
        skip.input.controllers[0].b = [true, false];
        skip.frame(|ctx, w| {
            w.play_cutscene(ctx);
        });

        assert!(skip.walk.cutscene.is_empty(), "B skipped the scene to the end");
        assert_eq!(
            skip.walk.camera.pos, full_end,
            "skipped camera end-state matches full playback",
        );
        assert_eq!(
            full_end,
            center_ref(Vec2::new(34, 18), &skip.system),
            "and both are back on the player",
        );
    }

    /// `shake N AMP` displaces the camera off its focus while it runs and puts it
    /// back exactly when the frames run out.
    #[test]
    fn shake_jiggles_then_restores() {
        let mut h = arm(
            "#cutscene t\n    camera 150 90\n    shake 8 3\n    wait 30",
            Vec2::new(0, 0),
        );
        let rest = center_ref(Vec2::new(150, 90), &h.system);
        let mut displaced = 0;
        for _ in 0..8 {
            h.frame(|ctx, w| {
                w.play_cutscene(ctx);
            });
            displaced += (h.walk.camera.pos != rest) as u32;
        }
        assert!(displaced > 0, "the shake visibly moved the camera");
        for _ in 0..2 {
            h.frame(|ctx, w| {
                w.play_cutscene(ctx);
            });
            assert_eq!(h.walk.camera.pos, rest, "spent shake leaves no offset");
        }
    }

    /// A dialogue `#shake` banked on the box (`pending_shake`) arms the
    /// walkaround-level shake at the centring choke point: the camera jiggles
    /// around whatever focus is being centred — here a cutscene's fixed point,
    /// proving the dialogue shake composes with the scene's own camera state —
    /// then restores exact centring when spent.
    #[test]
    fn dialogue_shake_jiggles_through_the_centring() {
        let mut h = arm(
            "#cutscene t\n    camera 150 90\n    wait 30",
            Vec2::new(0, 0),
        );
        let rest = center_ref(Vec2::new(150, 90), &h.system);
        // Settle on the cutscene's fixed focus first (instant cut).
        h.frame(|ctx, w| {
            w.play_cutscene(ctx);
        });
        assert_eq!(h.walk.camera.pos, rest);

        // The box banks a shake mid-conversation; the next centrings pick it up.
        h.walk.dialogue.pending_shake = Some((6, 3));
        let mut displaced = 0;
        for _ in 0..6 {
            h.frame(|ctx, w| {
                w.play_cutscene(ctx);
            });
            displaced += (h.walk.camera.pos != rest) as u32;
            assert!(h.walk.dialogue.pending_shake.is_none(), "banked shake taken");
        }
        assert!(displaced > 0, "the dialogue shake visibly moved the camera");
        for _ in 0..2 {
            h.frame(|ctx, w| {
                w.play_cutscene(ctx);
            });
            assert_eq!(h.walk.camera.pos, rest, "spent shake leaves no offset");
        }
    }

    // --- wave 4: `pose` chain motion ---

    /// A `pose` motion applies during its `move` step and stays on the actor
    /// well past that step — standing choreography, not a one-off action like
    /// every other motion.
    #[test]
    fn pose_applies_and_persists_after_its_step() {
        let def = scene::parse("#cutscene t\n    spawn a critter 0 0\n    move\n        a: pose slump\n    wait 5")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        let mut h = Harness::new();
        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        let actor = cs.resolve_actor("a");
        // One call chains the instantly-finished `pose` move straight into the
        // `wait` that follows (see `step`'s doc) — the pose is already applied.
        h.frame(|ctx, w| cs.step(ctx, w));
        assert_eq!(h.walk.resolve(&actor).unwrap().pose.as_deref(), Some("slump"));
        // Still there partway through the unrelated `wait` — it outlives the
        // instruction, and the whole step, that set it.
        for _ in 0..3 {
            h.frame(|ctx, w| cs.step(ctx, w));
        }
        assert_eq!(
            h.walk.resolve(&actor).unwrap().pose.as_deref(),
            Some("slump"),
            "the pose persists well past its own step",
        );
    }

    /// `cleanup` — run once a scene finishes or is skipped — clears every
    /// pose it applied, the same "undo what I did" contract it already gives
    /// `spawn`ed actors: scene-scoped choreography can't outlive the scene.
    #[test]
    fn cleanup_clears_every_pose_the_scene_applied() {
        let def = scene::parse("#cutscene t\n    move\n        player: pose slump")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        let mut h = Harness::new();
        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        let outcome = h.frame(|ctx, w| cs.step(ctx, w));
        assert!(matches!(outcome, Outcome::Finished));
        assert_eq!(h.walk.player_ref().pose.as_deref(), Some("slump"), "pose landed");
        cs.cleanup(&mut h.walk);
        assert_eq!(h.walk.player_ref().pose, None, "cleanup cleared it");
    }

    /// Skipping (B) a scene that hasn't reached its `pose` step yet still
    /// applies it — snapped, like every other lasting effect a skip fast-
    /// forwards through.
    #[test]
    fn skip_snaps_a_not_yet_reached_pose() {
        let def = scene::parse("#cutscene t\n    wait 50\n    move\n        player: pose slump")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        let mut h = Harness::new();
        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        // Still on the `wait` — the `pose` step hasn't run at all yet.
        h.frame(|ctx, w| cs.step(ctx, w));
        assert_eq!(h.walk.player_ref().pose, None, "not reached yet");
        h.frame(|ctx, w| cs.skip(ctx, w));
        assert_eq!(
            h.walk.player_ref().pose.as_deref(),
            Some("slump"),
            "skip snapped the not-yet-reached pose",
        );
    }

    /// A `pose` inside an `on CUE` handler's `move` lands exactly like a
    /// top-level one — handler bodies flow through the same
    /// `advance_move_chains`/`skip_content` paths, so this is free, but it's
    /// worth proving rather than assuming.
    #[test]
    fn pose_inside_a_handler_lands_when_its_cue_fires() {
        let mut h = Harness::new();
        install_script(&mut h, "#dialogue talk\n    #cue arrive\n    Hi there, this stays open.");
        let def = scene::parse(
            "#cutscene t\n\
             \x20   dialogue talk\n\
             \x20       on arrive\n\
             \x20           move\n\
             \x20               player: pose slump",
        )
        .unwrap()
        .get_cutscene("t")
        .unwrap()
        .clone();

        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        for _ in 0..6 {
            h.frame(|ctx, w| cs.step(ctx, w));
        }
        assert_eq!(
            h.walk.player_ref().pose.as_deref(),
            Some("slump"),
            "a pose inside an `on` handler's move lands like a top-level one",
        );
    }

    /// `pose none` mid-scene clears an earlier pose on the same actor.
    #[test]
    fn pose_none_clears_an_earlier_pose_mid_scene() {
        let def = scene::parse(
            "#cutscene t\n    move\n        player: pose slump\n    wait 3\n    move\n        player: pose none",
        )
        .unwrap()
        .get_cutscene("t")
        .unwrap()
        .clone();
        let mut h = Harness::new();
        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        // The first `pose` chains straight into the `wait`, which still has
        // frames left — so this call lands squarely mid-`wait`, pose standing.
        h.frame(|ctx, w| cs.step(ctx, w));
        assert_eq!(h.walk.player_ref().pose.as_deref(), Some("slump"));
        // The `wait` runs out and chains into `pose none`.
        for _ in 0..2 {
            h.frame(|ctx, w| cs.step(ctx, w));
        }
        assert_eq!(h.walk.player_ref().pose, None, "`pose none` cleared it mid-scene");
    }

    /// A pose naming a strip the actor's preset doesn't have (every built-in
    /// preset, today — none ships one yet) still sets `Shell::pose`: the
    /// fallback lives at draw time (`Shell::sprite_options`), not here. The
    /// `log::warn!` this takes must not panic.
    #[test]
    fn missing_pose_name_warns_but_does_not_panic() {
        let def = scene::parse("#cutscene t\n    move\n        player: pose nonexistent")
            .unwrap()
            .get_cutscene("t")
            .unwrap()
            .clone();
        let mut h = Harness::new();
        let mut cs = h.frame(|ctx, w| Cutscene::launch(&def, ctx, w));
        let outcome = h.frame(|ctx, w| cs.step(ctx, w));
        assert!(matches!(outcome, Outcome::Finished));
        assert_eq!(
            h.walk.player_ref().pose.as_deref(),
            Some("nonexistent"),
            "still set — the fallback is `Shell::sprite_options`, not motion application",
        );
    }
}

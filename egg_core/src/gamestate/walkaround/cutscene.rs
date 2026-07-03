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

use std::collections::HashMap;

use crate::Ctx;
use crate::data::scene::{Chain, CutsceneContent, CutsceneDef, EntityRef, GetEntity, Motion};
use crate::data::sound::music::MusicTrack;
use crate::data::sound::{self};
use crate::geometry::Vec2;
use crate::platform::{ConsoleApi, ConsoleHelper, just_pressed, pressed};
use crate::world::player::{EntityId, MoveMode};

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
    /// A dialogue box; `opened` latches the one-time open.
    Dialogue { opened: bool },
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

    /// Drive one frame. Chains the instant steps (sound/flag/…) into the same
    /// frame; the first frame-consuming step (`move`/`dialogue`/`wait`) returns
    /// [`Outcome::Running`]. A `load` returns [`Outcome::Load`]; the end returns
    /// [`Outcome::Finished`].
    pub fn step<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
    ) -> Outcome {
        loop {
            if matches!(self.state, StepState::Pending) {
                let Some(content) = self.content.get(self.step) else {
                    return Outcome::Finished;
                };
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
                        self.state = StepState::Move {
                            chains: chains.clone(),
                            progress,
                        };
                    }
                    CutsceneContent::Dialogue(_) => {
                        self.state = StepState::Dialogue { opened: false };
                    }
                    CutsceneContent::Wait(frames) => self.state = StepState::Wait(*frames),
                    CutsceneContent::Interact { actor, target } => {
                        self.fire_interact(ctx, walkaround, actor, target);
                        // If the fired interaction opened a dialogue box, drive it
                        // to completion (the walk loop's box handler is bypassed
                        // while a cutscene plays); otherwise the step is done (an
                        // instant effect like the dog's pet beat). `opened: true`
                        // makes `advance_dialogue` skip its key-read, which would
                        // mismatch the Interact content.
                        self.state = if walkaround.dialogue.current_text.is_some() {
                            StepState::Dialogue { opened: true }
                        } else {
                            StepState::Done
                        };
                    }
                    CutsceneContent::Sound(name) => {
                        if let Some(sfx) = sound::by_name(name) {
                            ctx.system.play_sound(sfx);
                        }
                        self.state = StepState::Done;
                    }
                    CutsceneContent::Music(track) => {
                        let track = track.as_deref().map(MusicTrack::named);
                        ctx.system.music(track.as_ref());
                        self.state = StepState::Done;
                    }
                    CutsceneContent::SetFlag(name, value) => {
                        ctx.save.set_flag(name, *value);
                        self.state = StepState::Done;
                    }
                    CutsceneContent::Load(name) => {
                        let name = name.clone();
                        self.step += 1;
                        self.state = StepState::Pending;
                        return Outcome::Load(name);
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
        for (chain, prog) in chains.iter().zip(progress.iter_mut()) {
            if prog.instr >= chain.instructions.len() {
                continue;
            }
            let ins = &chain.instructions[prog.instr];
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
        let all_done = chains
            .iter()
            .zip(progress.iter())
            .all(|(c, p)| p.instr >= c.instructions.len());
        if !all_done {
            self.state = StepState::Move { chains, progress };
        }
        all_done
    }

    /// Drive the dialogue box for the current `dialogue` step (the walk loop's
    /// dialogue input is short-circuited while a cutscene plays). Opens the box
    /// once, then ticks the typewriter and reads A/B; returns whether the box has
    /// fully closed.
    fn advance_dialogue<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        walkaround: &mut WalkaroundState,
    ) -> bool {
        let StepState::Dialogue { opened } = &mut self.state else {
            unreachable!("advance_dialogue only runs on a Dialogue state");
        };
        if !*opened {
            let CutsceneContent::Dialogue(key) = &self.content[self.step] else {
                unreachable!("Dialogue state ⇒ Dialogue content");
            };
            let convo = ctx.get_dialogue(key);
            walkaround
                .dialogue
                .set_messages(ctx.system, ctx.font, ctx.save, &convo);
            *opened = true;
            return false;
        }
        let pad = ctx.input.controller();
        walkaround.dialogue.tick(ctx.system, ctx.font, ctx.save, 1);
        if pressed(pad.a) {
            walkaround.dialogue.tick(ctx.system, ctx.font, ctx.save, 2);
        }
        if just_pressed(pad.b) {
            walkaround.dialogue.skip(ctx.system, ctx.font, ctx.save);
        }
        if just_pressed(pad.a)
            && walkaround.dialogue.is_line_done()
            && !walkaround.dialogue.next_text(ctx.system, ctx.font, ctx.save, false)
            && walkaround.dialogue.current_text.is_some()
        {
            walkaround.dialogue.close();
        }
        walkaround.dialogue.current_text.is_none() && walkaround.dialogue.next_text.is_empty()
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

    /// Remove this cutscene's transient `spawn`ed actors from the world — run
    /// once, when it finishes or is skipped.
    pub fn cleanup(&self, walkaround: &mut WalkaroundState) {
        if self.spawned.is_empty() {
            return;
        }
        walkaround
            .entities
            .retain(|e| match &e.id {
                Some(id) => !self.spawned.contains(id),
                None => true,
            });
    }

    /// Fast-forward to the end (the B-button abort): snap every remaining move to
    /// its end state and fire every remaining instant effect, so lasting side
    /// effects still land, then mark the cutscene finished. Each chain is snapped
    /// instruction-by-instruction (so a trailing `face` doesn't strand the actor
    /// mid-walk), including entity-relative and `record` moves. A `load` is
    /// chased — its sub-scene is launched, skipped, and cleaned up in place (never
    /// left on the stack) — so a skipped story scene can't silently drop a
    /// sub-scene's flags, sound/music, interacts, or map change.
    pub fn skip<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, walkaround: &mut WalkaroundState) {
        // Close any live dialogue box first.
        if matches!(self.state, StepState::Dialogue { opened: true }) {
            walkaround.dialogue.close();
        }
        while let Some(content) = self.content.get(self.step) {
            match content {
                CutsceneContent::Move(chains) => {
                    for chain in chains {
                        for ins in &chain.instructions {
                            snap_motion(&chain.actor, &ins.motion, &self.table, walkaround);
                        }
                    }
                }
                CutsceneContent::Dialogue(key) => {
                    let convo = ctx.get_dialogue(key);
                    walkaround
                        .dialogue
                        .set_messages(ctx.system, ctx.font, ctx.save, &convo);
                    walkaround.dialogue.close();
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
                CutsceneContent::Load(name) => {
                    // Chase the sub-scene so its lasting side effects still land,
                    // then drop its transient actors — all in place, so nothing is
                    // left running on the stack after the skip.
                    if let Some(def) = ctx.scenes.get_cutscene(name).cloned() {
                        let mut sub = Self::launch(&def, ctx, walkaround);
                        sub.skip(ctx, walkaround);
                        sub.cleanup(walkaround);
                    }
                }
                // A wait has no lasting effect, so fast-forwarding past it is a no-op.
                CutsceneContent::Wait(_) => {}
            }
            self.step += 1;
        }
        self.state = StepState::Done;
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}

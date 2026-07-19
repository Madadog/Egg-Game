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

//! `.eggscene` — a small line-oriented DSL for authoring **multi-actor
//! cutscenes**: requisition some entities by name, then drive them along
//! parallel timed paths, talk, and trigger each other's interactions. It is a
//! *separate* registry from the dialogue ([`crate::data::script::eggtext`]):
//! choreography is language-independent (authored once), and keeping it out of
//! `en.eggtext` stops staging from being noise to a translator. A `dialogue`
//! step refers to a `#dialogue` block **by key**, resolved at play time against
//! the active language.
//!
//! The parsed result is a [`SceneFile`]: a registry of named [`CutsceneDef`]s,
//! held by the host (see `EggState`) and looked up when a
//! `cutscene`-typed map object fires.
//!
//! # The format
//!
//! A `#cutscene NAME` header at column 0 opens a block. Its indented body is:
//! **init** verbs first (bind actor names), then **content** steps (one per
//! line, run in sequence). Blank lines and `//` comments are ignored — content
//! order is the line order, not blank-line stages. A `move` step wraps a deeper-
//! indented set of `actor: …` chains that run in *parallel*.
//!
//! ```text
//! #cutscene meet_dog
//!     spawn fido dog 120 64   // id preset x y  (transient)
//!     bind ellie player       // alias a name to a well-known entity
//!
//!     move
//!         ellie: walk 100 60 in 24; face fido
//!         fido:  walk 108 60 in 24
//!     dialogue dog_woof
//!     interact ellie fido
//! ```
//!
//! A `#path NAME` header (same column-0 convention) holds a standalone
//! recorded path: whitespace-separated `DX DY FRAMES` run triples, wrapped
//! across as many indented lines as needed (blank lines and `//` comments
//! tolerated the same as a `#cutscene` body). A `path [noclip] NAME` motion
//! (see below) plays it — the named counterpart to an inline `record`. Keeping
//! it out of the scene body is what keeps a hand-authored cutscene legible: a
//! recorded path is thousands of runs, machine-owned (the path recorder writes
//! it to a *separate* source file, merged into the same registry — see
//! [`SceneFile::merge`]), and inlining that much noise into a cutscene would
//! bury the actual choreography.
//!
//! ```text
//! #path house_entry_route
//!     1 0 4  0 1 12  -1 0 4
//! ```
//!
//! ## Init verbs (bind actor names; must precede content)
//!
//! | verb | meaning |
//! |------|---------|
//! | `map NAME`              | load this map before the scene starts |
//! | `spawn NAME PRESET X Y` | spawn a fresh transient shell (removed on finish) |
//! | `bind NAME PRESET X Y`  | bind an existing shell with id==NAME, else spawn one |
//! | `bind NAME player`      | alias NAME to the player |
//! | `bind NAME companion N` | alias NAME to the player's companion in slot N |
//! | `find NAME`             | bind id==NAME if present, else it resolves to nothing |
//!
//! `player` and `companion N` are also usable directly in chains without binding.
//!
//! ## Content steps (sequential)
//!
//! | verb | meaning |
//! |------|---------|
//! | `move` + indented chains | run the chains in parallel until all finish |
//! | `dialogue KEY` (+ `on` handlers) | play a dialogue block, done when its box closes and every fired handler has finished — see below |
//! | `interact ACTOR TARGET`  | fire TARGET's intrinsic interaction |
//! | `load NAME`              | push a sub-cutscene (popped on its finish) |
//! | `wait N`                 | hold for N frames |
//! | `camera ACTOR` / `camera X Y` | point the scene camera at an actor / a fixed map point |
//! | `camera … over N`        | same, but glide there over N frames (non-blocking — pair with `wait`) |
//! | `shake N [AMP]`          | shake the camera for N frames, ±AMP px (default 2; non-blocking) |
//! | `sound NAME` / `music [NAME]` / `set FLAG BOOL` | effects (carried over) |
//!
//! ### `dialogue` handlers: `on NAME [wait]`
//!
//! A `dialogue KEY` step may carry indented `on NAME [wait]` blocks, each
//! holding its own indented step-list — ordinary content steps, `move`
//! included, except `dialogue`, `load`, and a nested `on` are parse errors
//! (a handler can't open another dialogue, push a sub-cutscene, or nest
//! further; it also can't spawn — init verbs are scene-level only). While
//! the dialogue plays, the cutscene engine drains the box's `#cue NAME`
//! beats ([`.eggtext`](crate::data::script::eggtext)'s directive) and runs
//! the matching handler's steps concurrently with the box: a handler fires
//! at most once per `dialogue` step (a repeat firing of the same cue is
//! ignored), and a cue with no matching handler is fine — it may be a stage
//! direction, or a beat for a different scene. `wait` freezes the box (it
//! can't advance or close) until every running `wait` handler has finished;
//! a non-`wait` handler may keep running after the box closes. The whole
//! step is done only once the box has closed *and* every fired handler has
//! finished — a handler that never fires (its cue sat in an untaken `#if`
//! branch, say) simply never runs and never blocks completion.
//!
//! ```text
//! #cutscene confrontation
//!     spawn guy guy_preset 40 5
//!     dialogue marathon_speech
//!         on arrive
//!             guy path route1
//!         on meltdown wait
//!             guy face player
//!             shake 30
//!     move
//!         player: walk 10 0
//! ```
//!
//! ## Chains & motions
//!
//! A chain is `ACTOR: motion args [in N]; motion args [in N]; …` — a sequence of
//! timed motions. `in N` is the frame budget (the motion takes exactly N frames,
//! speed inflated to suit); omitted ⇒ natural speed until done. Motions:
//! `walk X Y`, `noclip X Y`, `to NAME`, `beside NAME [gap]`, `face NAME`,
//! `face DX DY`, `teleport X Y`, `record [noclip] DX DY N …`,
//! `path [noclip] NAME` (a `#path` block by name — see above), `pose NAME` /
//! `pose none`. `pose`, like `face`, is instant (`in N` just holds the chain
//! there for N frames) — but unlike every other motion, its effect *persists*
//! on the actor past the instruction, past the whole step, even past a scene
//! that's playing something else on top: it's standing choreography (a guy
//! slumped against a wall) rather than a one-off action. It stays until
//! another `pose` overwrites it, `pose none` clears it, or the owning scene's
//! cleanup clears every pose it applied, same as it does spawned actors — so
//! a pose can't outlive the scene that set it. A posed actor draws a named
//! sprite strip off its preset instead of its walk sprite; a name the preset
//! doesn't have logs a warning and falls back to the walk sprite.
//!
//! # What belongs where
//!
//! `.eggscene` owns the *world*: entities (spawning/binding/moving them),
//! the camera, map changes, inventory. It is language-independent —
//! choreography is staged once, not per language — and reaches dialogue only
//! by key (the `dialogue KEY` step above), resolved against whichever
//! language is active when the scene actually plays. A cutscene must never
//! embed text or other per-language behaviour of its own; if it needs to say
//! something, that something is a `#dialogue` block in
//! [`.eggtext`](crate::data::script::eggtext), referenced by key. A
//! `dialogue` step's `on NAME [wait]` handlers are how choreography
//! *subscribes* to that dialogue's `#cue` beats without embedding anything
//! language-specific: the cue name is the only thing shared across the
//! boundary, and `wait` pacing lives scene-side (not as a per-cue flag in
//! `.eggtext`) so a translation can reword or reflow a conversation without
//! ever being able to change how long the box holds.
//!
//! [`.eggtext`](crate::data::script::eggtext) owns *presentation* and the
//! save flags presentation reads/writes: text, portraits, sounds, pacing,
//! choices, `#set`/`#if`. It is authored **per language** — every key is
//! translated as a unit, and a translation is linted against the base
//! script's skeleton (see
//! [`crate::data::validate::check_overlay`]) rather than trusted by eye.

use std::collections::HashMap;

use egg_render::geometry::Vec2;
use crate::world::player::PresetId;

pub use super::script::eggtext::ParseError;
use super::script::eggtext::{collect_block, is_comment, split_first_word};

/// A parsed cutscene: an optional initial map load, an `init` list that binds
/// actor names, then sequential `content` steps. The language- and host-
/// independent *definition*; the runtime
/// `Cutscene` is built from
/// it at launch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CutsceneDef {
    /// A map to load before the scene runs (`map NAME`); `None` plays in place.
    pub init_map: Option<String>,
    /// Requisition: bind each actor name to a live entity before content runs.
    pub init: Vec<GetEntity>,
    /// The steps, played in order; each runs to completion before the next.
    pub content: Vec<CutsceneContent>,
    /// `#cutscene NAME interruptible` — while this scene plays, the player
    /// pressing a movement direction cancels it (cleanup + companions re-seated).
    /// For scenes the player should be able to bail out of.
    pub interruptible: bool,
}

/// One init step: how an actor name is bound before content runs.
#[derive(Clone, Debug, PartialEq)]
pub enum GetEntity {
    /// `spawn NAME PRESET X Y` — always spawn a fresh transient shell (id=NAME),
    /// removed when the cutscene finishes.
    Spawn {
        name: String,
        preset: PresetId,
        pos: Vec2,
    },
    /// `bind NAME PRESET X Y` — bind an existing shell whose id is NAME, else
    /// spawn one there (it persists).
    GetOrSpawn {
        name: String,
        preset: PresetId,
        pos: Vec2,
    },
    /// `find NAME` — bind id==NAME if present, else NAME resolves to nothing
    /// (chains targeting it log + skip).
    GetOrIgnore { name: String },
    /// `bind NAME player` / `bind NAME companion N` — alias a name to a
    /// well-known live entity.
    Alias { name: String, target: EntityRef },
}

/// A reference to a well-known live entity — the resolved form of a reserved
/// name token (`player`, `companion N`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityRef {
    Player,
    Companion(usize),
}

/// One content step, played to completion before the next.
#[derive(Clone, Debug, PartialEq)]
pub enum CutsceneContent {
    /// Parallel actor chains; done when every chain finishes.
    Move(Vec<Chain>),
    /// Play the `#dialogue` block named by this key, with its `on NAME
    /// [wait]` handlers (empty for a plain `dialogue KEY` line — the common
    /// case). Done when the box closes AND every fired handler has finished
    /// — see the module doc's `dialogue` handlers section.
    Dialogue { key: String, handlers: Vec<CueHandler> },
    /// Fire TARGET's intrinsic interaction, with ACTOR as initiator.
    Interact { actor: String, target: String },
    /// Push a sub-cutscene by name onto the stack (popped on its finish).
    Load(String),
    /// Hold for `N` frames.
    Wait(u32),
    /// Play a sound effect by name (resolved at build time).
    Sound(String),
    /// Switch music to a named track, or stop it (`None`).
    Music(Option<String>),
    /// Set a named save flag.
    SetFlag(String, bool),
    /// Retarget the scene camera (`camera ACTOR` / `camera X Y`); it follows the
    /// target until retargeted again, resetting to the player when the scene ends.
    /// `Some(n)` (`… over N`) glides there over `n` frames instead of cutting;
    /// the glide runs in the background (pair with `wait` to let it play out).
    Camera(CameraTarget, Option<u32>),
    /// `shake N [AMP]` — shake the camera for `frames`, offsetting the focus by
    /// up to ±`amplitude` px in a fixed pattern that tapers out. Non-blocking and
    /// transient: the camera is back on its focus when the frames run out.
    Shake { frames: u32, amplitude: i16 },
}

/// The `shake` amplitude used when the author gives only a duration, in pixels.
/// The serializer omits the amplitude at this value, keeping `shake N` canonical.
pub const DEFAULT_SHAKE_AMPLITUDE: i16 = 2;

/// One `on NAME [wait]` handler under a [`CutsceneContent::Dialogue`] step: a
/// step-list run concurrently with the dialogue box once the engine drains a
/// matching `#cue NAME` from it. See the module doc's `dialogue` handlers
/// section for the full semantics (fired-once, `wait` freezes the box,
/// what's allowed in the body).
#[derive(Clone, Debug, PartialEq)]
pub struct CueHandler {
    /// The `#cue` name this handler wakes up on.
    pub cue: String,
    /// The `wait` flag — while this handler is running, the dialogue box
    /// can't advance or close.
    pub wait: bool,
    /// The handler's own step-list, in order — ordinary content steps, but
    /// never `Dialogue`/`Load` (parse errors in an `on` body) or another
    /// handler (there is no nested `on`).
    pub content: Vec<CutsceneContent>,
}

/// A request to open the cutscene scrubber on a scene. The map editor *sets*
/// one (parked on its `pending_scrub`), the engine — which owns the cutscene
/// registry — drains and fulfils it. Scene-domain data rather than an editor
/// type, so the engine's drain doesn't reach into the editor's module tree
/// (a crate-extraction seam: the editor can one day move out of `egg_core`
/// without taking this type with it).
#[derive(Debug, Clone)]
pub enum ScrubRequest {
    /// Replay a scene looked up by name in the registry (the picker's choice).
    ByName(String),
    /// Replay a freshly recorded definition directly, no registry lookup — so
    /// play-right-after-recording doesn't race the on-disk save's live-reload.
    Recorded(String, CutsceneDef),
}

/// Where a `camera` step points the scene camera.
#[derive(Clone, Debug, PartialEq)]
pub enum CameraTarget {
    /// `camera ACTOR` — follow an actor's live position (re-read each frame).
    Actor(String),
    /// `camera X Y` — hold a fixed map point.
    Point(Vec2),
}

/// One actor's timed motion sequence within a [`CutsceneContent::Move`].
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    /// The actor name (resolved against the init bindings + reserved names).
    pub actor: String,
    /// The motions, in order.
    pub instructions: Vec<Instruction>,
}

/// One motion in a [`Chain`], with its timing and a fail-fast flag.
#[derive(Clone, Debug, PartialEq)]
pub struct Instruction {
    pub motion: Motion,
    /// Frame budget (`in N`); `0` = natural speed until done (no fixed budget).
    pub time: u16,
    /// `?` suffix — a *required* motion: if the actor can't make progress
    /// (blocked by collision), the whole cutscene cancels rather than holding out
    /// the budget. Only meaningful for the collision-aware moves.
    pub required: bool,
}

impl Instruction {
    /// A best-effort instruction (no `?`).
    pub fn new(motion: Motion, time: u16) -> Self {
        Self {
            motion,
            time,
            required: false,
        }
    }
    /// Mark this instruction required (the `?` suffix).
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// An RLE motion path: `(heading, frames-held)` runs, direction held for the
/// given frame count each. What `record`/`#path` bodies parse to and
/// [`Motion::Record`] carries — named so it doesn't read as "very complex" at
/// every call site that threads it around ([`SceneFile::paths`],
/// [`parse_path_body`]).
pub type PathRuns = Vec<((i8, i8), u16)>;

/// A single motion an actor performs. Entity-relative motions name another actor
/// (re-read live each frame at play time).
#[derive(Clone, Debug, PartialEq)]
pub enum Motion {
    /// `walk X Y` — toward a point, with collision.
    MoveToPoint(Vec2),
    /// `noclip X Y` — toward a point, ignoring collision.
    MoveToPointNoclip(Vec2),
    /// `to NAME` — toward another actor's live position.
    MoveToEntity(String),
    /// `beside NAME [gap]` — to NAME's head-side, `gap` px away, facing it.
    MoveBesideHorizontal { target: String, gap: i16 },
    /// `face NAME` — turn to face another actor.
    FaceEntity(String),
    /// `face DX DY` — face a fixed direction.
    FaceDir(i8, i8),
    /// `teleport X Y` — jump instantly.
    Teleport(Vec2),
    /// `record [noclip] DX DY N …` — an RLE path (direction held for N frames),
    /// authored by the path recorder. Replayed step-for-step at its recorded
    /// frame counts, so it takes no `in N` budget: one is a parse-time error
    /// (rescaling playback to a budget is future work), and the emitter never
    /// writes one, so a recorded path round-trips.
    Record { runs: PathRuns, noclip: bool },
    /// `path [noclip] NAME` — a named RLE path, authored separately as a
    /// `#path` block (see the module doc) instead of inline. Resolved against
    /// the path registry at the runtime boundary
    /// ([`SceneFile::inline_paths`]) into a [`Motion::Record`] before
    /// execution or drawing code ever sees it, so — like `record` — it takes
    /// no `in N` budget (rejected at parse time) and the emitter never writes
    /// one.
    Path { name: String, noclip: bool },
    /// `pose NAME` / `pose none` — put (or clear) a named standing pose on
    /// the actor: instant like [`FaceDir`](Self::FaceDir) (an `in N` budget
    /// just holds the chain there for N frames), but unlike every other
    /// motion its effect outlives the instruction — it persists on the actor
    /// until another `pose`, a `pose none`, or the owning scene's cleanup
    /// clears it (see the module doc). `None` is `pose none`. The named strip
    /// is resolved against the actor's preset at draw time, not here — this
    /// is just the choreography instruction.
    Pose(Option<String>),
}

/// The hand-owned `.eggscene` source — cutscenes and paths an author writes
/// directly (see the module doc's "What belongs where"). Named here so the
/// handful of load/merge/install sites across the host and editor can't drift
/// on the literal path.
pub const MAIN_SCENE_PATH: &str = "data/main.eggscene";
/// The machine-owned `.eggscene` source the live path recorder writes to (see
/// [`ScrubRequest`] and the module doc's `#path` section) — merged into the
/// same registry as [`MAIN_SCENE_PATH`] at every load site. Missing entirely
/// until the first recording, and never shipped as a bundled asset.
pub const RECORDED_SCENE_PATH: &str = "data/recorded.eggscene";

/// The parsed cutscene registry: every `#cutscene NAME` block by name, plus
/// every `#path NAME` block by name (a separate namespace — see the module
/// doc).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneFile {
    pub cutscenes: HashMap<String, CutsceneDef>,
    pub paths: HashMap<String, PathRuns>,
}

impl SceneFile {
    /// A cutscene definition by name, or `None` if undefined. The RAW def —
    /// any `Motion::Path` reference inside it is left unresolved; a caller
    /// about to execute or draw it wants [`Self::get_cutscene_resolved`]
    /// instead. This accessor stays raw because it's also what the emitter and
    /// checker walk, and both need to see `Motion::Path` as authored.
    pub fn get_cutscene(&self, name: &str) -> Option<&CutsceneDef> {
        self.cutscenes.get(name)
    }

    /// A cutscene definition by name, with every `Motion::Path` reference
    /// resolved against [`Self::paths`] (see [`Self::inline_paths`]) — what
    /// every runtime execution site (`Cutscene::launch`, the scrubber) and the
    /// editor's path-drawing overlay want, so `step_motion`/`snap_motion`/the
    /// polyline drawer never have to know `Motion::Path` exists.
    pub fn get_cutscene_resolved(&self, name: &str) -> Option<CutsceneDef> {
        self.get_cutscene(name).map(|def| self.inline_paths(def))
    }

    /// Replace every `Motion::Path { name, noclip }` in a (cloned) `def` with
    /// the [`Motion::Record`] it names, looked up in [`Self::paths`]. The
    /// runtime boundary a named path reference is resolved at — see the
    /// module doc. An unknown name logs and resolves to an empty (no-op)
    /// record, so a dangling reference degrades gracefully rather than
    /// panicking, matching an unknown dialogue key / sound / portrait
    /// elsewhere in the data web. Recurses into every `on` handler's own
    /// content too — a handler body can carry its own `move` steps (see the
    /// module doc), which can reference a `#path` block exactly like a
    /// top-level one.
    pub fn inline_paths(&self, def: &CutsceneDef) -> CutsceneDef {
        let mut def = def.clone();
        for step in &mut def.content {
            self.inline_paths_step(step);
        }
        def
    }

    /// One content step's half of [`Self::inline_paths`], recursive over
    /// `Dialogue`'s `on` handlers.
    fn inline_paths_step(&self, step: &mut CutsceneContent) {
        match step {
            CutsceneContent::Move(chains) => {
                for chain in chains {
                    for ins in &mut chain.instructions {
                        let Motion::Path { name, noclip } = &ins.motion else {
                            continue;
                        };
                        let runs = self.paths.get(name).cloned().unwrap_or_else(|| {
                            log::warn!("cutscene: unknown path `{name}` — playing as a no-op");
                            Vec::new()
                        });
                        ins.motion = Motion::Record { runs, noclip: *noclip };
                    }
                }
            }
            CutsceneContent::Dialogue { handlers, .. } => {
                for handler in handlers {
                    for sub in &mut handler.content {
                        self.inline_paths_step(sub);
                    }
                }
            }
            CutsceneContent::Interact { .. }
            | CutsceneContent::Load(_)
            | CutsceneContent::Wait(_)
            | CutsceneContent::Sound(_)
            | CutsceneContent::Music(_)
            | CutsceneContent::SetFlag(..)
            | CutsceneContent::Camera(..)
            | CutsceneContent::Shake { .. } => {}
        }
    }

    /// Every cutscene name, sorted — for pickers and listings (the registry is
    /// an unordered map, so a stable order needs sorting).
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.cutscenes.keys().cloned().collect();
        names.sort();
        names
    }

    /// Every cutscene as `(name, def)`, name-sorted, each def path-resolved
    /// (see [`Self::get_cutscene_resolved`]) — pushed into the map editor each
    /// focused frame so its Scenes panel can both list the scenes *and* draw
    /// their movement paths (which walk the whole [`CutsceneDef`]'s motions,
    /// so they must never see a bare `Motion::Path`). Mirrors
    /// `Presets::named_defs`; the registry is an unordered map, so a stable
    /// order needs sorting.
    pub fn named_defs(&self) -> Vec<(String, CutsceneDef)> {
        let mut defs: Vec<(String, CutsceneDef)> = self
            .cutscenes
            .iter()
            .map(|(name, def)| (name.clone(), self.inline_paths(def)))
            .collect();
        defs.sort_by(|a, b| a.0.cmp(&b.0));
        defs
    }

    /// Merge `other` into `self`, erroring if a name is defined in both
    /// **within the same kind** (two cutscenes, or two paths, sharing a
    /// name) — an authoring conflict between the two `.eggscene` sources
    /// (see [`MAIN_SCENE_PATH`] / [`RECORDED_SCENE_PATH`]) that must not
    /// silently drop one side's content. Cutscenes and paths are separate
    /// namespaces: a cutscene and a path may share a name without conflict.
    pub fn merge(mut self, other: SceneFile) -> Result<SceneFile, String> {
        for name in other.cutscenes.keys() {
            if self.cutscenes.contains_key(name) {
                return Err(format!("cutscene `{name}` is defined in more than one source"));
            }
        }
        for name in other.paths.keys() {
            if self.paths.contains_key(name) {
                return Err(format!("path `{name}` is defined in more than one source"));
            }
        }
        self.cutscenes.extend(other.cutscenes);
        self.paths.extend(other.paths);
        Ok(self)
    }
}

/// Leading-whitespace width of a raw line (its indentation).
fn indent(raw: &str) -> usize {
    raw.len() - raw.trim_start().len()
}

/// Parse a whole `.eggscene` source into a [`SceneFile`]. Errors carry the
/// 1-based source line, like [`crate::data::script::eggtext::parse`].
pub fn parse(src: &str) -> Result<SceneFile, ParseError> {
    let mut file = SceneFile::default();
    let mut lines = src.lines().enumerate().peekable();

    while let Some((idx, raw)) = lines.next() {
        let line_no = idx + 1;
        let logical = raw.trim_start();
        if logical.is_empty() || is_comment(logical) {
            continue;
        }
        if raw.starts_with([' ', '\t']) {
            return Err(ParseError::new(line_no, "indented line is not inside a block"));
        }
        let Some(header) = logical.strip_prefix('#') else {
            return Err(ParseError::new(
                line_no,
                "expected a block (`#cutscene name` or `#path name`)",
            ));
        };
        let (kind, rest) = split_first_word(header);
        match kind {
            "cutscene" => {
                let (name, flags) = split_first_word(rest);
                if name.is_empty() {
                    return Err(ParseError::new(line_no, "`#cutscene` needs a name"));
                }
                let interruptible = match flags.trim() {
                    "" => false,
                    "interruptible" => true,
                    other => {
                        return Err(ParseError::new(
                            line_no,
                            format!("unknown cutscene flag `{other}` (only `interruptible`)"),
                        ));
                    }
                };
                let body = collect_block(&mut lines);
                let mut def = parse_cutscene(&body)?;
                def.interruptible = interruptible;
                file.cutscenes.insert(name.to_string(), def);
            }
            "path" => {
                let (name, extra) = split_first_word(rest);
                if name.is_empty() {
                    return Err(ParseError::new(line_no, "`#path` needs a name"));
                }
                if !extra.trim().is_empty() {
                    return Err(ParseError::new(line_no, "`#path` takes no flags"));
                }
                let body = collect_block(&mut lines);
                let runs = parse_path_body(&body, line_no)?;
                file.paths.insert(name.to_string(), runs);
            }
            _ => {
                return Err(ParseError::new(
                    line_no,
                    format!("unknown block `#{kind}` (expected `#cutscene` or `#path`)"),
                ));
            }
        }
    }

    Ok(file)
}

/// Parse a `#path` body: whitespace-separated `DX DY FRAMES` triples, tolerant
/// of blank lines / `//` comments and wrapped across as many lines as needed
/// (all lines concatenated into one flat token stream before chunking) — see
/// the module doc. Each error points at the line the offending token actually
/// sits on, not the block header. `header_line` is used only when the body
/// carries no tokens at all.
fn parse_path_body(body: &[(usize, &str)], header_line: usize) -> Result<PathRuns, ParseError> {
    let mut tokens: Vec<(usize, &str)> = Vec::new();
    for &(line_no, raw) in body {
        let logical = raw.trim_start();
        if logical.is_empty() || is_comment(logical) {
            continue;
        }
        tokens.extend(logical.split_whitespace().map(|tok| (line_no, tok)));
    }
    if tokens.is_empty() || !tokens.len().is_multiple_of(3) {
        let line = tokens.last().map_or(header_line, |&(l, _)| l);
        return Err(ParseError::new(line, "`#path` needs `DX DY FRAMES` triples"));
    }
    let mut runs = Vec::with_capacity(tokens.len() / 3);
    for triple in tokens.chunks_exact(3) {
        let line_no = triple[0].0;
        let err = || ParseError::new(line_no, "`#path` triples are `DX DY FRAMES` integers");
        let dx = triple[0].1.parse().map_err(|_| err())?;
        let dy = triple[1].1.parse().map_err(|_| err())?;
        let frames = triple[2].1.parse().map_err(|_| err())?;
        runs.push(((dx, dy), frames));
    }
    Ok(runs)
}

/// Parse a `#cutscene` body: init verbs first (until the first content verb),
/// then content steps. A `move` step consumes the deeper-indented chain lines
/// that follow it.
fn parse_cutscene(body: &[(usize, &str)]) -> Result<CutsceneDef, ParseError> {
    let mut def = CutsceneDef::default();
    let mut seen_content = false;
    let mut i = 0;
    while i < body.len() {
        let (line_no, raw) = body[i];
        let logical = raw.trim_start();
        if logical.is_empty() || is_comment(logical) {
            i += 1;
            continue;
        }
        let (verb, args) = split_first_word(logical);
        match verb {
            "map" | "spawn" | "bind" | "find" if !seen_content => {
                if verb == "map" {
                    def.init_map = Some(require_name(args, line_no, "`map` needs a name")?);
                } else {
                    def.init.push(parse_init(verb, args, line_no)?);
                }
                i += 1;
            }
            "move" | "dialogue" | "interact" | "load" | "wait" | "sound" | "music" | "set"
            | "camera" | "shake" => {
                seen_content = true;
                let (step, next_i) = parse_content_step(body, i, verb, args, line_no, false)?;
                def.content.push(step);
                i = next_i;
            }
            "map" | "spawn" | "bind" | "find" => {
                return Err(ParseError::new(
                    line_no,
                    format!("init verb `{verb}` must come before content"),
                ));
            }
            other => return Err(ParseError::new(line_no, format!("unknown verb `{other}`"))),
        }
    }
    Ok(def)
}

/// Parse one content step at `body[i]`: `move` consumes its own deeper-
/// indented chain lines (as at the top level — see [`parse_cutscene`]);
/// `dialogue` consumes its own deeper-indented `on` handler blocks (see
/// [`parse_handlers`]); everything else is a single line, delegated to
/// [`parse_content`]. Returns the step and the index of the first line after
/// it — the caller (the top-level loop, or [`parse_content_block`] for a
/// handler body) resumes from there. `in_handler` is true while parsing an
/// `on` handler's own body: `dialogue` and `load` are parse errors there (a
/// handler can't open another dialogue or push a sub-cutscene — see the
/// module doc).
fn parse_content_step(
    body: &[(usize, &str)],
    i: usize,
    verb: &str,
    args: &str,
    line_no: usize,
    in_handler: bool,
) -> Result<(CutsceneContent, usize), ParseError> {
    match verb {
        "move" => {
            let block_indent = indent(body[i].1);
            let mut chains = Vec::new();
            let mut j = i + 1;
            while j < body.len() {
                let (cl_no, craw) = body[j];
                let clog = craw.trim_start();
                if clog.is_empty() || is_comment(clog) {
                    j += 1;
                    continue;
                }
                if indent(craw) <= block_indent {
                    break;
                }
                chains.push(parse_chain(clog, cl_no)?);
                j += 1;
            }
            if chains.is_empty() {
                return Err(ParseError::new(line_no, "`move` needs at least one chain"));
            }
            Ok((CutsceneContent::Move(chains), j))
        }
        "dialogue" if in_handler => {
            Err(ParseError::new(line_no, "`dialogue` cannot nest inside an `on` handler"))
        }
        "dialogue" => {
            let key = require_name(args, line_no, "`dialogue` needs a key")?;
            let block_indent = indent(body[i].1);
            let (handlers, next_i) = parse_handlers(body, i + 1, block_indent)?;
            Ok((CutsceneContent::Dialogue { key, handlers }, next_i))
        }
        "load" if in_handler => {
            Err(ParseError::new(line_no, "`load` cannot nest inside an `on` handler"))
        }
        _ => Ok((parse_content(verb, args, line_no)?, i + 1)),
    }
}

/// Parse the `on NAME [wait]` handler blocks under a `dialogue` step,
/// starting at `body[i]`, each more deeply indented than `dialogue_indent`
/// (the `dialogue` line's own indent). Stops at the first line indented at or
/// shallower than `dialogue_indent`, or at the end of `body`. Each header's
/// body is parsed as ordinary content steps ([`parse_content_block`], nested
/// — no `dialogue`/`load`/further `on`). Returns the handlers — empty for a
/// plain `dialogue KEY` line with no `on` blocks, the common case — and the
/// index of the first line not consumed.
fn parse_handlers(
    body: &[(usize, &str)],
    mut i: usize,
    dialogue_indent: usize,
) -> Result<(Vec<CueHandler>, usize), ParseError> {
    let mut handlers: Vec<CueHandler> = Vec::new();
    while i < body.len() {
        let (line_no, raw) = body[i];
        let logical = raw.trim_start();
        if logical.is_empty() || is_comment(logical) {
            i += 1;
            continue;
        }
        if indent(raw) <= dialogue_indent {
            break;
        }
        let (head, rest) = split_first_word(logical);
        if head != "on" {
            return Err(ParseError::new(line_no, "expected an `on NAME [wait]` handler"));
        }
        let (cue, flags) = split_first_word(rest);
        if cue.is_empty() {
            return Err(ParseError::new(line_no, "`on` needs a cue name"));
        }
        let wait = match flags.trim() {
            "" => false,
            "wait" => true,
            other => {
                return Err(ParseError::new(
                    line_no,
                    format!("unknown `on` flag `{other}` (only `wait`)"),
                ));
            }
        };
        if handlers.iter().any(|h| h.cue == cue) {
            return Err(ParseError::new(
                line_no,
                format!("duplicate `on {cue}` handler in this `dialogue` step"),
            ));
        }
        let handler_indent = indent(raw);
        i += 1;
        let (content, next_i) = parse_content_block(body, i, handler_indent, true)?;
        if content.is_empty() {
            return Err(ParseError::new(line_no, format!("`on {cue}` needs at least one step")));
        }
        handlers.push(CueHandler {
            cue: cue.to_string(),
            wait,
            content,
        });
        i = next_i;
    }
    Ok((handlers, i))
}

/// Parse a sequence of content steps (see [`parse_content_step`]) starting at
/// `body[i]`, for as long as each is indented deeper than `min_indent` — an
/// `on` handler's body. `in_handler` is threaded to each step.
fn parse_content_block(
    body: &[(usize, &str)],
    mut i: usize,
    min_indent: usize,
    in_handler: bool,
) -> Result<(Vec<CutsceneContent>, usize), ParseError> {
    let mut steps = Vec::new();
    while i < body.len() {
        let (line_no, raw) = body[i];
        let logical = raw.trim_start();
        if logical.is_empty() || is_comment(logical) {
            i += 1;
            continue;
        }
        if indent(raw) <= min_indent {
            break;
        }
        let (verb, args) = split_first_word(logical);
        let known = matches!(
            verb,
            "move" | "dialogue" | "interact" | "load" | "wait" | "sound" | "music" | "set"
                | "camera" | "shake"
        );
        if !known {
            return Err(ParseError::new(line_no, format!("unknown verb `{verb}`")));
        }
        let (step, next_i) = parse_content_step(body, i, verb, args, line_no, in_handler)?;
        steps.push(step);
        i = next_i;
    }
    Ok((steps, i))
}

/// Parse a `spawn`/`bind`/`find` init verb into a [`GetEntity`].
fn parse_init(verb: &str, args: &str, line_no: usize) -> Result<GetEntity, ParseError> {
    let (name, rest) = split_first_word(args);
    if name.is_empty() {
        return Err(ParseError::new(line_no, format!("`{verb}` needs a name")));
    }
    let name = name.to_string();
    match verb {
        "find" => Ok(GetEntity::GetOrIgnore { name }),
        "spawn" | "bind" => {
            let (head, tail) = split_first_word(rest);
            // `bind NAME player|companion N` is an alias; otherwise it's a
            // preset + position (spawn / get-or-spawn).
            if verb == "bind" {
                if head == "player" {
                    return Ok(GetEntity::Alias {
                        name,
                        target: EntityRef::Player,
                    });
                }
                if head == "companion" {
                    let slot = parse_u32(tail, line_no, "`companion` needs a slot")? as usize;
                    return Ok(GetEntity::Alias {
                        name,
                        target: EntityRef::Companion(slot),
                    });
                }
            }
            if head.is_empty() {
                return Err(ParseError::new(
                    line_no,
                    format!("`{verb}` needs `NAME PRESET X Y`"),
                ));
            }
            let preset = PresetId::new(head);
            let pos = parse_vec2(tail, line_no, verb)?;
            if verb == "spawn" {
                Ok(GetEntity::Spawn { name, preset, pos })
            } else {
                Ok(GetEntity::GetOrSpawn { name, preset, pos })
            }
        }
        _ => unreachable!("parse_init only called for spawn/bind/find"),
    }
}

/// Parse a non-`move`, non-`dialogue` content verb into a [`CutsceneContent`]
/// (`dialogue` is [`parse_content_step`]'s own arm, since it needs the raw
/// `body`/`i` to parse its `on` handlers).
fn parse_content(verb: &str, args: &str, line_no: usize) -> Result<CutsceneContent, ParseError> {
    Ok(match verb {
        "load" => CutsceneContent::Load(require_name(args, line_no, "`load` needs a name")?),
        "wait" => CutsceneContent::Wait(parse_u32(args, line_no, "`wait` needs a frame count")?),
        "sound" => CutsceneContent::Sound(require_name(args, line_no, "`sound` needs a name")?),
        "music" => CutsceneContent::Music((!args.trim().is_empty()).then(|| args.trim().to_string())),
        "set" => {
            let (name, value) = split_first_word(args);
            if name.is_empty() {
                return Err(ParseError::new(line_no, "`set` needs `FLAG BOOL`"));
            }
            CutsceneContent::SetFlag(name.to_string(), parse_bool(value, line_no)?)
        }
        "interact" => {
            let (actor, target) = split_first_word(args);
            let target = target.trim();
            if actor.is_empty() || target.is_empty() {
                return Err(ParseError::new(line_no, "`interact` needs `ACTOR TARGET`"));
            }
            CutsceneContent::Interact {
                actor: actor.to_string(),
                target: target.to_string(),
            }
        }
        "camera" => {
            let (target, over) = parse_camera(args, line_no)?;
            CutsceneContent::Camera(target, over)
        }
        "shake" => {
            let mut parts = args.split_whitespace();
            let frames = parse_u32(
                parts.next().unwrap_or(""),
                line_no,
                "`shake` needs a frame count",
            )?;
            let amplitude = match parts.next() {
                Some(amp) => amp
                    .parse()
                    .map_err(|_| ParseError::new(line_no, "`shake N AMP` needs integers"))?,
                None => DEFAULT_SHAKE_AMPLITUDE,
            };
            if parts.next().is_some() {
                return Err(ParseError::new(line_no, "`shake` takes `N [AMP]`"));
            }
            CutsceneContent::Shake { frames, amplitude }
        }
        _ => unreachable!("parse_content only called for known content verbs"),
    })
}

/// Parse a `camera` argument: two integer tokens are a fixed `X Y` point, a
/// single token is an actor name to follow (the same one-vs-two-token split
/// `face NAME` / `face DX DY` uses). A trailing `over N` makes it a glide of
/// `N` frames instead of a cut.
fn parse_camera(args: &str, line_no: usize) -> Result<(CameraTarget, Option<u32>), ParseError> {
    let mut tokens: Vec<&str> = args.split_whitespace().collect();
    let over = if tokens.len() >= 2 && tokens[tokens.len() - 2] == "over" {
        let frames = parse_u32(
            tokens[tokens.len() - 1],
            line_no,
            "`over` needs a frame count",
        )?;
        if frames == 0 {
            return Err(ParseError::new(line_no, "`over 0` — glide needs ≥1 frame"));
        }
        tokens.truncate(tokens.len() - 2);
        Some(frames)
    } else if tokens.last() == Some(&"over") {
        return Err(ParseError::new(line_no, "`over` needs a frame count"));
    } else {
        None
    };
    let target = match tokens[..] {
        [] => return Err(ParseError::new(line_no, "`camera` needs `ACTOR` or `X Y`")),
        [actor] => CameraTarget::Actor(actor.to_string()),
        [x, y] => {
            let err = || ParseError::new(line_no, "`camera X Y` needs integers");
            CameraTarget::Point(Vec2::new(
                x.parse().map_err(|_| err())?,
                y.parse().map_err(|_| err())?,
            ))
        }
        _ => return Err(ParseError::new(line_no, "`camera` takes `ACTOR` or `X Y`")),
    };
    Ok((target, over))
}

/// Parse one `actor: motion; motion; …` chain line.
fn parse_chain(logical: &str, line_no: usize) -> Result<Chain, ParseError> {
    let (actor, rest) = logical
        .split_once(':')
        .ok_or_else(|| ParseError::new(line_no, "a chain reads `actor: motion; …`"))?;
    let actor = actor.trim();
    if actor.is_empty() {
        return Err(ParseError::new(line_no, "a chain needs an actor name"));
    }
    let mut instructions = Vec::new();
    for segment in rest.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        instructions.push(parse_motion(segment, line_no)?);
    }
    if instructions.is_empty() {
        return Err(ParseError::new(line_no, "a chain needs at least one motion"));
    }
    Ok(Chain {
        actor: actor.to_string(),
        instructions,
    })
}

/// Parse one motion segment, peeling an optional `in N` frame-budget suffix and a
/// trailing `?` (the fail-fast / required marker).
fn parse_motion(segment: &str, line_no: usize) -> Result<Instruction, ParseError> {
    let mut tokens: Vec<&str> = segment.split_whitespace().collect();
    // Peel `in N` (the last two tokens).
    let time = if tokens.len() >= 2 && tokens[tokens.len() - 2] == "in" {
        let n = tokens[tokens.len() - 1]
            .parse()
            .map_err(|_| ParseError::new(line_no, "`in` needs a frame count"))?;
        tokens.truncate(tokens.len() - 2);
        n
    } else {
        0u16
    };
    // Peel a trailing `?` (attached to the last token, e.g. `dog?`, or standalone).
    let mut required = false;
    if let Some(last) = tokens.last().copied()
        && let Some(stripped) = last.strip_suffix('?')
    {
        required = true;
        tokens.pop();
        if !stripped.is_empty() {
            tokens.push(stripped);
        }
    }
    let (verb, args) = tokens
        .split_first()
        .ok_or_else(|| ParseError::new(line_no, "empty motion"))?;
    let motion = match *verb {
        "walk" => Motion::MoveToPoint(args_vec2(args, line_no, "walk")?),
        "noclip" => Motion::MoveToPointNoclip(args_vec2(args, line_no, "noclip")?),
        "to" => Motion::MoveToEntity(args_name(args, line_no, "to")?),
        "beside" => {
            let target = args_name(&args[..args.len().min(1)], line_no, "beside")?;
            let gap = match args.get(1) {
                Some(s) => s
                    .parse()
                    .map_err(|_| ParseError::new(line_no, "`beside` gap must be an integer"))?,
                None => 0,
            };
            Motion::MoveBesideHorizontal { target, gap }
        }
        "face" => {
            if args.len() == 2
                && let (Ok(dx), Ok(dy)) = (args[0].parse::<i8>(), args[1].parse::<i8>())
            {
                Motion::FaceDir(dx, dy)
            } else {
                Motion::FaceEntity(args_name(args, line_no, "face")?)
            }
        }
        "teleport" => Motion::Teleport(args_vec2(args, line_no, "teleport")?),
        "record" => parse_record(args, line_no)?,
        "path" => parse_path_ref(args, line_no)?,
        "pose" => {
            let name = args_name(args, line_no, "pose")?;
            Motion::Pose(if name == "none" { None } else { Some(name) })
        }
        other => return Err(ParseError::new(line_no, format!("unknown motion `{other}`"))),
    };
    // `record`/`path` both replay at their own recorded frame counts; an `in N`
    // budget on either would be a silent no-op, so reject it at parse time
    // (fail loud beats a dropped budget).
    let recorded_verb = match &motion {
        Motion::Record { .. } => Some("record"),
        Motion::Path { .. } => Some("path"),
        _ => None,
    };
    if time != 0
        && let Some(verb) = recorded_verb
    {
        return Err(ParseError::new(
            line_no,
            format!("`{verb}` takes no `in N` budget (it replays at its recorded frame counts)"),
        ));
    }
    Ok(Instruction {
        motion,
        time,
        required,
    })
}

/// Parse a `path [noclip] NAME` motion: a reference to a `#path` block by
/// name, resolved at the runtime boundary (see [`SceneFile::inline_paths`]).
/// Mirrors [`parse_record`]'s `noclip` peel.
fn parse_path_ref(args: &[&str], line_no: usize) -> Result<Motion, ParseError> {
    let (noclip, rest) = match args.split_first() {
        Some((&"noclip", rest)) => (true, rest),
        _ => (false, args),
    };
    let name = args_name(rest, line_no, "path")?;
    Ok(Motion::Path { name, noclip })
}

/// Parse a `record [noclip] DX DY N …` motion: a flat run of `(dx, dy, frames)`
/// triples, optionally prefixed with `noclip`.
fn parse_record(args: &[&str], line_no: usize) -> Result<Motion, ParseError> {
    let (noclip, rest) = match args.split_first() {
        Some((&"noclip", rest)) => (true, rest),
        _ => (false, args),
    };
    if rest.len() % 3 != 0 || rest.is_empty() {
        return Err(ParseError::new(
            line_no,
            "`record` needs `[noclip]` then `DX DY FRAMES` triples",
        ));
    }
    let mut runs = Vec::new();
    for triple in rest.chunks_exact(3) {
        let err = || ParseError::new(line_no, "`record` triples are `DX DY FRAMES` integers");
        let dx = triple[0].parse().map_err(|_| err())?;
        let dy = triple[1].parse().map_err(|_| err())?;
        let frames = triple[2].parse().map_err(|_| err())?;
        runs.push(((dx, dy), frames));
    }
    Ok(Motion::Record { runs, noclip })
}

fn args_vec2(args: &[&str], line_no: usize, verb: &str) -> Result<Vec2, ParseError> {
    if args.len() != 2 {
        return Err(ParseError::new(line_no, format!("`{verb}` needs `X Y`")));
    }
    let err = || ParseError::new(line_no, format!("`{verb}` needs `X Y` integers"));
    Ok(Vec2::new(
        args[0].parse().map_err(|_| err())?,
        args[1].parse().map_err(|_| err())?,
    ))
}

fn args_name(args: &[&str], line_no: usize, verb: &str) -> Result<String, ParseError> {
    match args.first() {
        Some(name) if args.len() == 1 => Ok((*name).to_string()),
        _ => Err(ParseError::new(line_no, format!("`{verb}` needs one name"))),
    }
}

/// Require a non-empty single argument (a name/key), trimmed.
fn require_name(args: &str, line_no: usize, message: &str) -> Result<String, ParseError> {
    let args = args.trim();
    if args.is_empty() {
        Err(ParseError::new(line_no, message))
    } else {
        Ok(args.to_string())
    }
}

fn parse_u32(args: &str, line_no: usize, message: &str) -> Result<u32, ParseError> {
    args.trim()
        .parse()
        .map_err(|_| ParseError::new(line_no, message))
}

fn parse_bool(arg: &str, line_no: usize) -> Result<bool, ParseError> {
    match arg.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ParseError::new(line_no, "expected `true` or `false`")),
    }
}

fn parse_vec2(args: &str, line_no: usize, verb: &str) -> Result<Vec2, ParseError> {
    let mut parts = args.split_whitespace();
    let err = || ParseError::new(line_no, format!("`{verb}` needs `X Y` integers"));
    let x = parts.next().and_then(|s| s.parse().ok()).ok_or_else(err)?;
    let y = parts.next().and_then(|s| s.parse().ok()).ok_or_else(err)?;
    if parts.next().is_some() {
        return Err(ParseError::new(line_no, format!("`{verb}` takes `X Y`")));
    }
    Ok(Vec2::new(x, y))
}

// --- emitting: the inverse of `parse` ---

/// Emit a whole [`SceneFile`] back to `.eggscene` text that re-parses to the
/// same registry. Cutscenes are emitted first (name-sorted), then `#path`
/// blocks (also name-sorted) — see the module doc. The inverse of [`parse`].
pub fn emit_scene(file: &SceneFile) -> String {
    let mut names: Vec<&String> = file.cutscenes.keys().collect();
    names.sort();
    let mut path_names: Vec<&String> = file.paths.keys().collect();
    path_names.sort();

    let mut out = String::new();
    let mut first = true;
    for name in &names {
        if !first {
            out.push('\n');
        }
        first = false;
        out.push_str(&emit_cutscene(name, &file.cutscenes[*name]));
        out.push('\n');
    }
    for name in &path_names {
        if !first {
            out.push('\n');
        }
        first = false;
        out.push_str(&emit_path(name, &file.paths[*name]));
        out.push('\n');
    }
    out
}

/// Emit one `#path <name>` block, its run triples wrapped at roughly 80
/// columns (a recorded path can run to thousands of runs — see the module
/// doc). The returned string ends without a trailing newline, matching
/// [`emit_cutscene`]. Shared by the emitter and the path recorder (which
/// splices this same block into `recorded.eggscene`). The inverse of
/// [`parse_path_body`].
pub fn emit_path(name: &str, runs: &[((i8, i8), u16)]) -> String {
    const WRAP_COL: usize = 80;
    const INDENT: &str = "    ";
    let mut out = format!("#path {name}");
    let mut line = String::new();
    for ((dx, dy), frames) in runs {
        let triple = format!("{dx} {dy} {frames}");
        if !line.is_empty() && INDENT.len() + line.len() + 1 + triple.len() > WRAP_COL {
            out.push('\n');
            out.push_str(INDENT);
            out.push_str(&line);
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&triple);
    }
    if !line.is_empty() {
        out.push('\n');
        out.push_str(INDENT);
        out.push_str(&line);
    }
    out
}

/// Emit one `#cutscene <name>` block. The returned string ends without a
/// trailing newline.
pub fn emit_cutscene(name: &str, def: &CutsceneDef) -> String {
    let header = if def.interruptible {
        format!("#cutscene {name} interruptible\n")
    } else {
        format!("#cutscene {name}\n")
    };
    let mut out = header;
    if let Some(map) = &def.init_map {
        out.push_str(&format!("    map {map}\n"));
    }
    for entity in &def.init {
        out.push_str("    ");
        out.push_str(&emit_init(entity));
        out.push('\n');
    }
    for step in &def.content {
        out.push_str(&emit_content(step, 1));
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

fn emit_init(entity: &GetEntity) -> String {
    match entity {
        GetEntity::Spawn { name, preset, pos } => {
            format!("spawn {name} {preset} {} {}", pos.x, pos.y)
        }
        GetEntity::GetOrSpawn { name, preset, pos } => {
            format!("bind {name} {preset} {} {}", pos.x, pos.y)
        }
        GetEntity::GetOrIgnore { name } => format!("find {name}"),
        GetEntity::Alias {
            name,
            target: EntityRef::Player,
        } => format!("bind {name} player"),
        GetEntity::Alias {
            name,
            target: EntityRef::Companion(slot),
        } => format!("bind {name} companion {slot}"),
    }
}

/// Emit one content step, including its trailing newline(s), indented
/// `depth` levels deep (4 spaces per level — 1 at the top of a `#cutscene`
/// body, one more for each nesting: a `move`'s chains, an `on` handler
/// header, and that handler's own steps, recursively through this same
/// function). The inverse of [`parse_content_step`].
fn emit_content(step: &CutsceneContent, depth: usize) -> String {
    let ind = "    ".repeat(depth);
    match step {
        CutsceneContent::Move(chains) => {
            let mut out = format!("{ind}move\n");
            let chain_ind = "    ".repeat(depth + 1);
            for chain in chains {
                out.push_str(&chain_ind);
                out.push_str(&emit_chain(chain));
                out.push('\n');
            }
            out
        }
        CutsceneContent::Dialogue { key, handlers } => {
            let mut out = format!("{ind}dialogue {key}\n");
            let handler_ind = "    ".repeat(depth + 1);
            for handler in handlers {
                out.push_str(&handler_ind);
                out.push_str("on ");
                out.push_str(&handler.cue);
                if handler.wait {
                    out.push_str(" wait");
                }
                out.push('\n');
                for sub in &handler.content {
                    out.push_str(&emit_content(sub, depth + 2));
                }
            }
            out
        }
        CutsceneContent::Interact { actor, target } => {
            format!("{ind}interact {actor} {target}\n")
        }
        CutsceneContent::Load(name) => format!("{ind}load {name}\n"),
        CutsceneContent::Wait(frames) => format!("{ind}wait {frames}\n"),
        CutsceneContent::Sound(name) => format!("{ind}sound {name}\n"),
        CutsceneContent::Music(Some(track)) => format!("{ind}music {track}\n"),
        CutsceneContent::Music(None) => format!("{ind}music\n"),
        CutsceneContent::SetFlag(name, value) => format!("{ind}set {name} {value}\n"),
        CutsceneContent::Camera(target, over) => {
            let target = match target {
                CameraTarget::Actor(name) => name.clone(),
                CameraTarget::Point(p) => format!("{} {}", p.x, p.y),
            };
            match over {
                Some(frames) => format!("{ind}camera {target} over {frames}\n"),
                None => format!("{ind}camera {target}\n"),
            }
        }
        CutsceneContent::Shake { frames, amplitude } => {
            if *amplitude == DEFAULT_SHAKE_AMPLITUDE {
                format!("{ind}shake {frames}\n")
            } else {
                format!("{ind}shake {frames} {amplitude}\n")
            }
        }
    }
}

fn emit_chain(chain: &Chain) -> String {
    let motions: Vec<String> = chain
        .instructions
        .iter()
        .map(|ins| {
            let mut m = emit_motion(&ins.motion);
            if ins.required {
                m.push('?');
            }
            // `record`/`path` never carry an `in N` budget (parse rejects one), so
            // never emit one either — otherwise a re-parse of the output would fail.
            if ins.time != 0 && !matches!(ins.motion, Motion::Record { .. } | Motion::Path { .. }) {
                m.push_str(&format!(" in {}", ins.time));
            }
            m
        })
        .collect();
    format!("{}: {}", chain.actor, motions.join("; "))
}

fn emit_motion(motion: &Motion) -> String {
    match motion {
        Motion::MoveToPoint(p) => format!("walk {} {}", p.x, p.y),
        Motion::MoveToPointNoclip(p) => format!("noclip {} {}", p.x, p.y),
        Motion::MoveToEntity(name) => format!("to {name}"),
        Motion::MoveBesideHorizontal { target, gap } => {
            if *gap == 0 {
                format!("beside {target}")
            } else {
                format!("beside {target} {gap}")
            }
        }
        Motion::FaceEntity(name) => format!("face {name}"),
        Motion::FaceDir(dx, dy) => format!("face {dx} {dy}"),
        Motion::Teleport(p) => format!("teleport {} {}", p.x, p.y),
        Motion::Record { runs, noclip } => {
            let mut out = String::from("record");
            if *noclip {
                out.push_str(" noclip");
            }
            for ((dx, dy), frames) in runs {
                out.push_str(&format!(" {dx} {dy} {frames}"));
            }
            out
        }
        Motion::Path { name, noclip } => {
            if *noclip {
                format!("path noclip {name}")
            } else {
                format!("path {name}")
            }
        }
        Motion::Pose(name) => match name {
            Some(name) => format!("pose {name}"),
            None => "pose none".to_string(),
        },
    }
}

/// Whether `name` is acceptable as a `#cutscene` scene name: a non-empty run of
/// ASCII identifier characters (letters, digits, `_`). The header grammar only
/// requires "one whitespace-free word", but the shipped names are all snake_case
/// identifiers, so the recorder holds new names to that same shape — it round-
/// trips through the header (no whitespace to eat, no `#`/`//` to be mistaken for
/// a block/comment) and reads as a name rather than punctuation.
pub fn is_identifier_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether `name` is a reserved actor token — `player` or `companion N` — that a
/// chain resolves without any init binding (see the runtime's `resolve_name`). A
/// non-reserved actor is a map creature referenced by its `Shell::id`, which the
/// recorder pairs with a `find NAME` init so the binding is explicit + fail-safe.
pub fn is_reserved_actor(name: &str) -> bool {
    name == "player"
        || name
            .strip_prefix("companion")
            .is_some_and(|slot| slot.trim().parse::<usize>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The single cutscene `src` defines.
    fn one(src: &str) -> CutsceneDef {
        let file = parse(src).expect("parse");
        file.cutscenes.into_values().next().expect("one cutscene")
    }

    #[test]
    fn init_then_content_parses() {
        let def = one(
            "#cutscene meet\n\
             \x20   spawn fido dog 120 64\n\
             \x20   bind ellie player\n\
             \x20   find cat\n\n\
             \x20   move\n\
             \x20       ellie: walk 100 60 in 24; face fido\n\
             \x20       fido: walk 108 60 in 24\n\
             \x20   dialogue dog_woof\n\
             \x20   interact ellie fido",
        );
        assert_eq!(def.init.len(), 3);
        assert_eq!(
            def.init[0],
            GetEntity::Spawn {
                name: "fido".into(),
                preset: PresetId::new("dog"),
                pos: Vec2::new(120, 64),
            }
        );
        assert_eq!(
            def.init[1],
            GetEntity::Alias {
                name: "ellie".into(),
                target: EntityRef::Player,
            }
        );
        assert_eq!(def.init[2], GetEntity::GetOrIgnore { name: "cat".into() });
        assert_eq!(def.content.len(), 3);
        let CutsceneContent::Move(chains) = &def.content[0] else {
            panic!("first content is a move");
        };
        assert_eq!(chains.len(), 2);
        assert_eq!(chains[0].actor, "ellie");
        assert_eq!(
            chains[0].instructions,
            vec![
                Instruction::new(Motion::MoveToPoint(Vec2::new(100, 60)), 24),
                Instruction::new(Motion::FaceEntity("fido".into()), 0),
            ]
        );
        assert_eq!(
            def.content[2],
            CutsceneContent::Interact {
                actor: "ellie".into(),
                target: "fido".into(),
            }
        );
    }

    #[test]
    fn every_motion_parses() {
        let def = one(
            "#cutscene c\n\
             \x20   move\n\
             \x20       a: walk 1 2; noclip 3 4; to b; beside b 5; face b; face 1 -1; teleport 7 8; record noclip 1 0 10; pose slump; pose none",
        );
        let CutsceneContent::Move(chains) = &def.content[0] else {
            panic!("move");
        };
        assert_eq!(
            chains[0].instructions,
            vec![
                Instruction::new(Motion::MoveToPoint(Vec2::new(1, 2)), 0),
                Instruction::new(Motion::MoveToPointNoclip(Vec2::new(3, 4)), 0),
                Instruction::new(Motion::MoveToEntity("b".into()), 0),
                Instruction::new(Motion::MoveBesideHorizontal { target: "b".into(), gap: 5 }, 0),
                Instruction::new(Motion::FaceEntity("b".into()), 0),
                Instruction::new(Motion::FaceDir(1, -1), 0),
                Instruction::new(Motion::Teleport(Vec2::new(7, 8)), 0),
                Instruction::new(Motion::Record { runs: vec![((1, 0), 10)], noclip: true }, 0),
                Instruction::new(Motion::Pose(Some("slump".into())), 0),
                Instruction::new(Motion::Pose(None), 0),
            ]
        );
    }

    /// `pose` takes the same `in N` budget as `face` (holds the chain there
    /// for N frames rather than being rejected like `record`/`path`), and a
    /// missing pose name is a parse-time error pointed at the line.
    #[test]
    fn pose_takes_an_in_budget_and_requires_a_name() {
        let def = one("#cutscene c\n    move\n        a: pose slump in 30");
        let CutsceneContent::Move(chains) = &def.content[0] else {
            panic!("move");
        };
        assert_eq!(
            chains[0].instructions,
            vec![Instruction::new(Motion::Pose(Some("slump".into())), 30)]
        );
        assert_eq!(
            parse("#cutscene c\n    move\n        a: pose")
                .unwrap_err()
                .line,
            3
        );
    }

    /// The `?` suffix marks a motion required, before or after an `in N` budget;
    /// the `interruptible` header flag parses. Both round-trip.
    #[test]
    fn required_motions_and_interruptible_flag() {
        let def = one(
            "#cutscene c interruptible\n\
             \x20   move\n\
             \x20       a: beside b? ; walk 4 5? in 12",
        );
        assert!(def.interruptible);
        let CutsceneContent::Move(chains) = &def.content[0] else {
            panic!("move");
        };
        assert_eq!(
            chains[0].instructions,
            vec![
                Instruction::new(Motion::MoveBesideHorizontal { target: "b".into(), gap: 0 }, 0)
                    .required(),
                Instruction::new(Motion::MoveToPoint(Vec2::new(4, 5)), 12).required(),
            ]
        );
        let (file, reparsed) = round_trip("#cutscene c interruptible\n    move\n        a: beside b? in 3");
        assert_eq!(file, reparsed);
    }

    #[test]
    fn carryover_effects_parse() {
        let def = one(
            "#cutscene c\n\
             \x20   wait 30\n\
             \x20   sound pop\n\
             \x20   music theme\n\
             \x20   set seen true\n\
             \x20   load next",
        );
        assert_eq!(
            def.content,
            vec![
                CutsceneContent::Wait(30),
                CutsceneContent::Sound("pop".into()),
                CutsceneContent::Music(Some("theme".into())),
                CutsceneContent::SetFlag("seen".into(), true),
                CutsceneContent::Load("next".into()),
            ]
        );
    }

    /// `camera ACTOR` follows a named actor; `camera X Y` holds a fixed point.
    /// A single token is always an actor (even numeric); two tokens are a point.
    /// A trailing `over N` turns either form into an N-frame glide.
    #[test]
    fn camera_targets_parse() {
        let def = one(
            "#cutscene c\n\
             \x20   camera dog\n\
             \x20   camera 120 64\n\
             \x20   camera player\n\
             \x20   camera dog over 30\n\
             \x20   camera 120 64 over 45",
        );
        assert_eq!(
            def.content,
            vec![
                CutsceneContent::Camera(CameraTarget::Actor("dog".into()), None),
                CutsceneContent::Camera(CameraTarget::Point(Vec2::new(120, 64)), None),
                CutsceneContent::Camera(CameraTarget::Actor("player".into()), None),
                CutsceneContent::Camera(CameraTarget::Actor("dog".into()), Some(30)),
                CutsceneContent::Camera(CameraTarget::Point(Vec2::new(120, 64)), Some(45)),
            ]
        );
    }

    /// A `camera` with no target, or a two-token point that isn't integers, or
    /// three tokens, or a malformed `over` clause, is a parse error pointed at
    /// its line.
    #[test]
    fn camera_errors_point_at_the_line() {
        assert_eq!(parse("#cutscene c\n    camera").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    camera 1 x").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    camera 1 2 3").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    camera dog over").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    camera dog over x").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    camera dog over 0").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    camera 1 2 3 over 9").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    camera over 9").unwrap_err().line, 2);
    }

    /// `shake N` uses the default ±2px amplitude; `shake N AMP` overrides it.
    /// Missing/garbled/extra arguments are line-pointed parse errors.
    #[test]
    fn shake_parses_with_optional_amplitude() {
        let def = one("#cutscene c\n    shake 30\n    shake 30 4");
        assert_eq!(
            def.content,
            vec![
                CutsceneContent::Shake {
                    frames: 30,
                    amplitude: DEFAULT_SHAKE_AMPLITUDE,
                },
                CutsceneContent::Shake {
                    frames: 30,
                    amplitude: 4,
                },
            ]
        );
        assert_eq!(parse("#cutscene c\n    shake").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    shake x").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    shake 30 x").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    shake 30 4 5").unwrap_err().line, 2);
    }

    #[test]
    fn errors_point_at_the_line() {
        assert_eq!(parse("#cutscene c\n    bogus 1").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    wait").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    move").unwrap_err().line, 2);
        assert_eq!(
            parse("#cutscene c\n    move\n        a: huh 1 2")
                .unwrap_err()
                .line,
            3
        );
        // init after content is an error.
        assert_eq!(
            parse("#cutscene c\n    wait 1\n    spawn a b 1 2")
                .unwrap_err()
                .line,
            3
        );
        assert_eq!(parse("#wat name").unwrap_err().line, 1);
    }

    /// An `in N` budget on a `record` motion is a parse-time error (it would
    /// otherwise be a silent no-op), pointed at its line. A budget-free `record`
    /// still parses.
    #[test]
    fn record_rejects_an_in_budget() {
        let err = parse("#cutscene c\n    move\n        a: record 1 0 10 in 5")
            .unwrap_err();
        assert_eq!(err.line, 3);
        assert!(
            parse("#cutscene c\n    move\n        a: record 1 0 10").is_ok(),
            "a budget-free record still parses",
        );
    }

    // --- #path blocks ---

    #[test]
    fn path_block_parses_a_single_line_body() {
        let file = parse("#path p\n    1 0 4 0 1 12 -1 0 4").expect("parse");
        assert_eq!(
            file.paths.get("p"),
            Some(&vec![((1, 0), 4), ((0, 1), 12), ((-1, 0), 4)])
        );
    }

    /// A `#path` body wraps across as many indented lines as needed — all
    /// lines concatenate into one flat run list.
    #[test]
    fn path_block_body_wraps_across_multiple_lines() {
        let file = parse("#path p\n    1 0 4 0 1 12\n    -1 0 4 0 -1 6").expect("parse");
        assert_eq!(
            file.paths.get("p"),
            Some(&vec![((1, 0), 4), ((0, 1), 12), ((-1, 0), 4), ((0, -1), 6)])
        );
    }

    /// Blank lines and `//` comments inside a `#path` body are tolerated the
    /// same as a `#cutscene` body.
    #[test]
    fn path_block_tolerates_comments_and_blank_lines() {
        let file = parse(
            "#path p\n\
             \x20   // leading comment\n\
             \x20   1 0 4\n\n\
             \x20   0 1 12\n\
             \x20   // trailing comment",
        )
        .expect("parse");
        assert_eq!(file.paths.get("p"), Some(&vec![((1, 0), 4), ((0, 1), 12)]));
    }

    /// Paths and cutscenes are separate namespaces (only same-kind names
    /// collide at merge time — see the merge tests below) and both parse from
    /// the same source.
    #[test]
    fn path_and_cutscene_blocks_coexist() {
        let file = parse("#cutscene c\n    wait 1\n#path p\n    1 0 4").expect("parse");
        assert!(file.get_cutscene("c").is_some());
        assert!(file.paths.contains_key("p"));
    }

    #[test]
    fn path_block_needs_a_name() {
        assert_eq!(parse("#path\n    1 0 4").unwrap_err().line, 1);
    }

    #[test]
    fn path_block_rejects_flags() {
        assert_eq!(parse("#path p extra\n    1 0 4").unwrap_err().line, 1);
    }

    /// An empty body has no line of its own to point at, so the error lands on
    /// the header.
    #[test]
    fn path_block_needs_a_body() {
        assert_eq!(parse("#path p").unwrap_err().line, 1);
    }

    /// A junk (non-integer) triplet errors at the line it actually sits on,
    /// even though the body started earlier.
    #[test]
    fn path_block_junk_triplet_errors_at_its_line() {
        assert_eq!(parse("#path p\n    1 0 4\n    x 0 4").unwrap_err().line, 3);
    }

    /// A triple count that isn't a multiple of 3 (a trailing partial triple)
    /// is an error.
    #[test]
    fn path_block_incomplete_triplet_errors() {
        assert_eq!(parse("#path p\n    1 0 4\n    1 0").unwrap_err().line, 3);
    }

    #[test]
    fn path_motion_parses_with_and_without_noclip() {
        let def = one("#cutscene c\n    move\n        a: path p; path noclip q");
        let CutsceneContent::Move(chains) = &def.content[0] else {
            panic!("move");
        };
        assert_eq!(
            chains[0].instructions,
            vec![
                Instruction::new(Motion::Path { name: "p".into(), noclip: false }, 0),
                Instruction::new(Motion::Path { name: "q".into(), noclip: true }, 0),
            ]
        );
    }

    /// A `path` motion replays at its named path's recorded frame counts, so
    /// (like `record`) an `in N` budget on it is a parse-time error.
    #[test]
    fn path_motion_rejects_an_in_budget() {
        let err = parse("#cutscene c\n    move\n        a: path p in 5").unwrap_err();
        assert_eq!(err.line, 3);
        assert!(
            parse("#cutscene c\n    move\n        a: path p").is_ok(),
            "a budget-free path still parses",
        );
    }

    // --- merging multiple sources ---

    #[test]
    fn merge_combines_disjoint_sources() {
        let a = parse("#cutscene a\n    wait 1\n#path pa\n    1 0 1").unwrap();
        let b = parse("#cutscene b\n    wait 2\n#path pb\n    0 1 1").unwrap();
        let merged = a.merge(b).expect("disjoint sources merge");
        assert!(merged.get_cutscene("a").is_some());
        assert!(merged.get_cutscene("b").is_some());
        assert!(merged.paths.contains_key("pa"));
        assert!(merged.paths.contains_key("pb"));
    }

    #[test]
    fn merge_errors_on_a_duplicate_cutscene_name() {
        let a = parse("#cutscene a\n    wait 1").unwrap();
        let b = parse("#cutscene a\n    wait 2").unwrap();
        assert!(a.merge(b).is_err());
    }

    #[test]
    fn merge_errors_on_a_duplicate_path_name() {
        let a = parse("#path p\n    1 0 1").unwrap();
        let b = parse("#path p\n    0 1 1").unwrap();
        assert!(a.merge(b).is_err());
    }

    /// Cutscenes and paths are separate namespaces: the same name in each
    /// kind, across the two sources, is not a conflict.
    #[test]
    fn merge_allows_a_cutscene_and_a_path_to_share_a_name() {
        let a = parse("#cutscene shared\n    wait 1").unwrap();
        let b = parse("#path shared\n    1 0 1").unwrap();
        let merged = a.merge(b).expect("cross-kind name reuse is fine");
        assert!(merged.get_cutscene("shared").is_some());
        assert!(merged.paths.contains_key("shared"));
    }

    // --- resolving `path` references ---

    #[test]
    fn inline_paths_resolves_runs_and_noclip() {
        let file = parse("#cutscene c\n    move\n        a: path noclip p\n#path p\n    1 0 4 0 1 6")
            .unwrap();
        let def = file.get_cutscene("c").unwrap();
        let resolved = file.inline_paths(def);
        let CutsceneContent::Move(chains) = &resolved.content[0] else {
            panic!("move");
        };
        assert_eq!(
            chains[0].instructions[0].motion,
            Motion::Record { runs: vec![((1, 0), 4), ((0, 1), 6)], noclip: true }
        );
    }

    /// An unknown path name resolves to an empty (no-op) record rather than
    /// panicking — degrades gracefully like a dangling dialogue key.
    #[test]
    fn inline_paths_unknown_name_resolves_to_an_empty_record() {
        let file = parse("#cutscene c\n    move\n        a: path missing").unwrap();
        let def = file.get_cutscene("c").unwrap();
        let resolved = file.inline_paths(def);
        let CutsceneContent::Move(chains) = &resolved.content[0] else {
            panic!("move");
        };
        assert_eq!(
            chains[0].instructions[0].motion,
            Motion::Record { runs: vec![], noclip: false }
        );
    }

    #[test]
    fn get_cutscene_resolved_inlines_paths() {
        let file = parse("#cutscene c\n    move\n        a: path p\n#path p\n    1 0 4").unwrap();
        let def = file.get_cutscene_resolved("c").expect("resolved def");
        let CutsceneContent::Move(chains) = &def.content[0] else {
            panic!("move");
        };
        assert!(matches!(chains[0].instructions[0].motion, Motion::Record { .. }));
    }

    /// `named_defs` (what feeds the editor's path-drawing overlay) hands back
    /// already-resolved defs, so a bare `Motion::Path` never reaches it.
    #[test]
    fn named_defs_are_already_path_resolved() {
        let file = parse("#cutscene c\n    move\n        a: path p\n#path p\n    1 0 4").unwrap();
        let defs = file.named_defs();
        let (_, def) = defs.iter().find(|(n, _)| n == "c").unwrap();
        let CutsceneContent::Move(chains) = &def.content[0] else {
            panic!("move");
        };
        assert!(matches!(chains[0].instructions[0].motion, Motion::Record { .. }));
    }

    // --- emitter ---

    fn round_trip(src: &str) -> (SceneFile, SceneFile) {
        let file = parse(src).expect("parse");
        let reparsed = parse(&emit_scene(&file)).expect("re-parse emitted");
        (file, reparsed)
    }

    #[test]
    fn emit_round_trips_a_multi_actor_scene() {
        let (file, reparsed) = round_trip(
            "#cutscene a\n\
             \x20   map town\n\
             \x20   spawn fido dog 1 2\n\
             \x20   bind ellie player\n\n\
             \x20   move\n\
             \x20       ellie: walk 5 6 in 12; beside fido; pose slump\n\
             \x20       fido: to ellie; pose none\n\
             \x20   camera fido\n\
             \x20   dialogue hello\n\
             \x20   camera 5 6\n\
             \x20   camera fido over 30\n\
             \x20   camera 5 6 over 45\n\
             \x20   shake 20\n\
             \x20   shake 20 4\n\
             \x20   wait 10\n\
             \x20   interact ellie fido\n\
             #cutscene b\n\
             \x20   music\n\
             \x20   set done true",
        );
        assert_eq!(file, reparsed);
    }

    /// A `dialogue` step's `on NAME [wait]` handlers — including a `move`
    /// chain nested inside one — round-trip through the emitter, indented
    /// one level deeper each nesting.
    #[test]
    fn emit_round_trips_dialogue_handlers() {
        let (file, reparsed) = round_trip(
            "#cutscene c\n\
             \x20   spawn guy critter 40 5\n\
             \x20   dialogue marathon_speech\n\
             \x20       on arrive\n\
             \x20           move\n\
             \x20               guy: to player\n\
             \x20       on meltdown wait\n\
             \x20           wait 5\n\
             \x20           shake 30\n\
             \x20   wait 1",
        );
        assert_eq!(file, reparsed);
        let def = file.get_cutscene("c").unwrap();
        let CutsceneContent::Dialogue { key, handlers } = &def.content[0] else {
            panic!("dialogue step");
        };
        assert_eq!(key, "marathon_speech");
        assert_eq!(handlers.len(), 2);
        assert_eq!(handlers[0].cue, "arrive");
        assert!(!handlers[0].wait);
        assert!(matches!(handlers[0].content[..], [CutsceneContent::Move(_)]));
        assert_eq!(handlers[1].cue, "meltdown");
        assert!(handlers[1].wait);
        assert_eq!(handlers[1].content, vec![CutsceneContent::Wait(5), CutsceneContent::Shake {
            frames: 30,
            amplitude: DEFAULT_SHAKE_AMPLITUDE,
        }]);
    }

    /// A plain `dialogue KEY` line (no `on` blocks) parses to an empty
    /// handler list and emits back exactly as authored — the common case
    /// must not grow any extra ceremony.
    #[test]
    fn dialogue_without_handlers_emits_as_a_plain_line() {
        let def = one("#cutscene c\n    dialogue hello");
        let CutsceneContent::Dialogue { key, handlers } = &def.content[0] else {
            panic!("dialogue step");
        };
        assert_eq!(key, "hello");
        assert!(handlers.is_empty());
        assert_eq!(emit_cutscene("c", &def), "#cutscene c\n    dialogue hello");
    }

    /// `on` handler grammar errors: a bare `on`, an `on` with an unknown
    /// trailing flag, an empty handler body, and a duplicate cue name within
    /// one `dialogue` step.
    #[test]
    fn on_handler_errors_point_at_the_line() {
        assert_eq!(
            parse("#cutscene c\n    dialogue d\n        on")
                .unwrap_err()
                .line,
            3
        );
        assert_eq!(
            parse("#cutscene c\n    dialogue d\n        on arrive nonsense")
                .unwrap_err()
                .line,
            3
        );
        assert_eq!(
            parse("#cutscene c\n    dialogue d\n        on arrive")
                .unwrap_err()
                .line,
            3,
            "an `on` with no body is an error"
        );
        assert_eq!(
            parse(
                "#cutscene c\n\
                 \x20   dialogue d\n\
                 \x20       on arrive\n\
                 \x20           wait 1\n\
                 \x20       on arrive\n\
                 \x20           wait 2"
            )
            .unwrap_err()
            .line,
            5,
            "a duplicate cue name in the same dialogue step is an error"
        );
    }

    /// A handler body can't contain `dialogue`, `load`, or another `on` — all
    /// parse errors pointed at the offending line.
    #[test]
    fn on_handler_body_rejects_dialogue_load_and_nested_on() {
        assert_eq!(
            parse(
                "#cutscene c\n\
                 \x20   dialogue d\n\
                 \x20       on arrive\n\
                 \x20           dialogue other"
            )
            .unwrap_err()
            .line,
            4
        );
        assert_eq!(
            parse(
                "#cutscene c\n\
                 \x20   dialogue d\n\
                 \x20       on arrive\n\
                 \x20           load other"
            )
            .unwrap_err()
            .line,
            4
        );
        assert_eq!(
            parse(
                "#cutscene c\n\
                 \x20   dialogue d\n\
                 \x20       on arrive\n\
                 \x20           on nested\n\
                 \x20               wait 1"
            )
            .unwrap_err()
            .line,
            4,
            "an unknown verb `on` inside a handler body is a parse error"
        );
    }

    /// A handler body's `move` chains, indented deeper still, parse like a
    /// top-level `move` step.
    #[test]
    fn on_handler_body_allows_move() {
        let def = one(
            "#cutscene c\n\
             \x20   dialogue d\n\
             \x20       on arrive\n\
             \x20           move\n\
             \x20               guy: walk 1 2\n\
             \x20           sound pop",
        );
        let CutsceneContent::Dialogue { handlers, .. } = &def.content[0] else {
            panic!("dialogue step");
        };
        assert_eq!(handlers[0].content.len(), 2, "{:?}", handlers[0].content);
        assert!(matches!(handlers[0].content[0], CutsceneContent::Move(_)));
        assert_eq!(handlers[0].content[1], CutsceneContent::Sound("pop".into()));
    }

    /// The strongest guarantee: the shipped `.eggscene` round-trips.
    #[test]
    fn emit_round_trips_every_shipped_cutscene() {
        let file = parse(include_str!("../../../../assets/data/main.eggscene")).expect("parse main");
        assert!(!file.cutscenes.is_empty(), "expected shipped cutscenes");
        let reparsed = parse(&emit_scene(&file)).expect("re-parse emitted");
        assert_eq!(file, reparsed);
    }

    /// `#path` blocks and `path` motions round-trip through the emitter
    /// (placed after the cutscenes — see `emit_scene`'s doc).
    #[test]
    fn emit_round_trips_a_path_block_and_motion() {
        let (file, reparsed) = round_trip(
            "#cutscene c\n\
             \x20   move\n\
             \x20       a: path noclip p\n\
             #path p\n\
             \x20   1 0 4 0 1 6 -1 0 4",
        );
        assert_eq!(file, reparsed);
        assert!(file.paths.contains_key("p"));
    }

    /// A path long enough to force `emit_path`'s ~80-column wrap still
    /// round-trips (all lines concatenate back into one run list).
    #[test]
    fn emit_wraps_a_long_path_and_round_trips() {
        let mut src = String::from("#path long\n");
        for i in 0..40 {
            src.push_str(&format!("    1 0 {}\n", i + 1));
        }
        let file = parse(&src).expect("parse");
        let runs = file.paths.get("long").expect("path defined");
        assert_eq!(runs.len(), 40);

        let emitted = emit_path("long", runs);
        assert!(
            emitted.lines().count() > 2,
            "a 40-triple path should wrap across more than one body line: {emitted}"
        );
        let reparsed = parse(&emitted).expect("re-parse emitted");
        assert_eq!(reparsed.paths.get("long"), Some(runs));
    }

    #[test]
    fn identifier_name_accepts_snake_case_rejects_the_rest() {
        assert!(is_identifier_name("house_living_room_path"));
        assert!(is_identifier_name("pet_dog"));
        assert!(is_identifier_name("scene2"));
        assert!(!is_identifier_name(""), "empty is not a name");
        assert!(!is_identifier_name("two words"), "whitespace breaks the header");
        assert!(!is_identifier_name("has-dash"));
        assert!(!is_identifier_name("bad#name"));
    }

    /// A name typed as a valid identifier survives a header round-trip (so the
    /// recorder's rename validation and the parser agree on what a name is).
    #[test]
    fn valid_identifier_names_round_trip_through_a_header() {
        for name in ["fresh_path", "scene2", "a"] {
            assert!(is_identifier_name(name));
            let def = CutsceneDef {
                content: vec![CutsceneContent::Wait(1)],
                ..Default::default()
            };
            let src = emit_cutscene(name, &def);
            let file = parse(&src).expect("re-parses");
            assert!(file.get_cutscene(name).is_some(), "name survives: {src}");
        }
    }

    /// `named_defs` returns every cutscene as `(name, def)` in name-sorted order
    /// (the registry map is unordered), so the editor's Scenes panel + paths
    /// overlay list scenes stably.
    #[test]
    fn named_defs_are_name_sorted() {
        let file = parse(
            "#cutscene zebra\n    wait 1\n\
             #cutscene apple\n    wait 2\n\
             #cutscene mango\n    wait 3",
        )
        .expect("parse");
        let defs = file.named_defs();
        let names: Vec<&str> = defs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["apple", "mango", "zebra"], "sorted by name");
        assert_eq!(
            defs[0].1,
            *file.get_cutscene("apple").unwrap(),
            "each name is paired with its own def"
        );
    }

    #[test]
    fn reserved_actor_is_player_or_companion_slot() {
        assert!(is_reserved_actor("player"));
        assert!(is_reserved_actor("companion 0"));
        assert!(is_reserved_actor("companion 3"));
        assert!(!is_reserved_actor("companion"), "a slot is required");
        assert!(!is_reserved_actor("dog"), "a map creature id is not reserved");
        assert!(!is_reserved_actor("player2"));
    }
}

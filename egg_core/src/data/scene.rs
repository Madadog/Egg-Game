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
//! held by the host (see [`crate::EggState`]) and looked up when a
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
//! | `dialogue KEY`           | play a dialogue block, done when its box closes |
//! | `interact ACTOR TARGET`  | fire TARGET's intrinsic interaction |
//! | `load NAME`              | push a sub-cutscene (popped on its finish) |
//! | `wait N`                 | hold for N frames |
//! | `camera ACTOR` / `camera X Y` | point the scene camera at an actor / a fixed map point |
//! | `camera … over N`        | same, but glide there over N frames (non-blocking — pair with `wait`) |
//! | `shake N [AMP]`          | shake the camera for N frames, ±AMP px (default 2; non-blocking) |
//! | `sound NAME` / `music [NAME]` / `set FLAG BOOL` | effects (carried over) |
//!
//! ## Chains & motions
//!
//! A chain is `ACTOR: motion args [in N]; motion args [in N]; …` — a sequence of
//! timed motions. `in N` is the frame budget (the motion takes exactly N frames,
//! speed inflated to suit); omitted ⇒ natural speed until done. Motions:
//! `walk X Y`, `noclip X Y`, `to NAME`, `beside NAME [gap]`, `face NAME`,
//! `face DX DY`, `teleport X Y`, `record [noclip] DX DY N …`.

use std::collections::HashMap;

use crate::geometry::Vec2;
use crate::world::player::PresetId;

pub use super::script::eggtext::ParseError;
use super::script::eggtext::{collect_block, is_comment, split_first_word};

/// A parsed cutscene: an optional initial map load, an `init` list that binds
/// actor names, then sequential `content` steps. The language- and host-
/// independent *definition*; the runtime
/// [`Cutscene`](crate::gamestate::walkaround::cutscene::Cutscene) is built from
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
    /// Play the `#dialogue` block named by this key; done when the box closes.
    Dialogue(String),
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
    Record {
        runs: Vec<((i8, i8), u16)>,
        noclip: bool,
    },
}

/// The parsed cutscene registry: every `#cutscene NAME` block by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneFile {
    pub cutscenes: HashMap<String, CutsceneDef>,
}

impl SceneFile {
    /// A cutscene definition by name, or `None` if undefined.
    pub fn get_cutscene(&self, name: &str) -> Option<&CutsceneDef> {
        self.cutscenes.get(name)
    }

    /// Every cutscene name, sorted — for pickers and listings (the registry is
    /// an unordered map, so a stable order needs sorting).
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.cutscenes.keys().cloned().collect();
        names.sort();
        names
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
            return Err(ParseError::new(line_no, "expected a block (`#cutscene name`)"));
        };
        let (kind, rest) = split_first_word(header);
        if kind != "cutscene" {
            return Err(ParseError::new(
                line_no,
                format!("unknown block `#{kind}` (expected `#cutscene`)"),
            ));
        }
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

    Ok(file)
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
            "move" => {
                seen_content = true;
                let block_indent = indent(raw);
                let mut chains = Vec::new();
                i += 1;
                while i < body.len() {
                    let (cl_no, craw) = body[i];
                    let clog = craw.trim_start();
                    if clog.is_empty() || is_comment(clog) {
                        i += 1;
                        continue;
                    }
                    if indent(craw) <= block_indent {
                        break;
                    }
                    chains.push(parse_chain(clog, cl_no)?);
                    i += 1;
                }
                if chains.is_empty() {
                    return Err(ParseError::new(line_no, "`move` needs at least one chain"));
                }
                def.content.push(CutsceneContent::Move(chains));
            }
            "dialogue" | "interact" | "load" | "wait" | "sound" | "music" | "set" | "camera"
            | "shake" => {
                seen_content = true;
                def.content.push(parse_content(verb, args, line_no)?);
                i += 1;
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

/// Parse a non-`move` content verb into a [`CutsceneContent`].
fn parse_content(verb: &str, args: &str, line_no: usize) -> Result<CutsceneContent, ParseError> {
    Ok(match verb {
        "dialogue" => CutsceneContent::Dialogue(require_name(args, line_no, "`dialogue` needs a key")?),
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
        other => return Err(ParseError::new(line_no, format!("unknown motion `{other}`"))),
    };
    // A `record` replays at its own frame counts; an `in N` budget on it would be
    // a silent no-op, so reject it at parse time (fail loud beats a dropped budget).
    if time != 0 && matches!(motion, Motion::Record { .. }) {
        return Err(ParseError::new(
            line_no,
            "`record` takes no `in N` budget (it replays at its recorded frame counts)",
        ));
    }
    Ok(Instruction {
        motion,
        time,
        required,
    })
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
/// same registry. Cutscenes are emitted in sorted-name order. The inverse of
/// [`parse`].
pub fn emit_scene(file: &SceneFile) -> String {
    let mut names: Vec<&String> = file.cutscenes.keys().collect();
    names.sort();
    let mut out = String::new();
    for (i, name) in names.iter().enumerate() {
        if i != 0 {
            out.push('\n');
        }
        out.push_str(&emit_cutscene(name, &file.cutscenes[*name]));
        out.push('\n');
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
        out.push_str(&emit_content(step));
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

/// Emit one content step, including its trailing newline(s).
fn emit_content(step: &CutsceneContent) -> String {
    match step {
        CutsceneContent::Move(chains) => {
            let mut out = String::from("    move\n");
            for chain in chains {
                out.push_str("        ");
                out.push_str(&emit_chain(chain));
                out.push('\n');
            }
            out
        }
        CutsceneContent::Dialogue(key) => format!("    dialogue {key}\n"),
        CutsceneContent::Interact { actor, target } => {
            format!("    interact {actor} {target}\n")
        }
        CutsceneContent::Load(name) => format!("    load {name}\n"),
        CutsceneContent::Wait(frames) => format!("    wait {frames}\n"),
        CutsceneContent::Sound(name) => format!("    sound {name}\n"),
        CutsceneContent::Music(Some(track)) => format!("    music {track}\n"),
        CutsceneContent::Music(None) => "    music\n".to_string(),
        CutsceneContent::SetFlag(name, value) => format!("    set {name} {value}\n"),
        CutsceneContent::Camera(target, over) => {
            let target = match target {
                CameraTarget::Actor(name) => name.clone(),
                CameraTarget::Point(p) => format!("{} {}", p.x, p.y),
            };
            match over {
                Some(frames) => format!("    camera {target} over {frames}\n"),
                None => format!("    camera {target}\n"),
            }
        }
        CutsceneContent::Shake { frames, amplitude } => {
            if *amplitude == DEFAULT_SHAKE_AMPLITUDE {
                format!("    shake {frames}\n")
            } else {
                format!("    shake {frames} {amplitude}\n")
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
            // A `record` never carries an `in N` budget (parse rejects one), so
            // never emit one either — otherwise a re-parse of the output would fail.
            if ins.time != 0 && !matches!(ins.motion, Motion::Record { .. }) {
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
             \x20       a: walk 1 2; noclip 3 4; to b; beside b 5; face b; face 1 -1; teleport 7 8; record noclip 1 0 10",
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
            ]
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
             \x20       ellie: walk 5 6 in 12; beside fido\n\
             \x20       fido: to ellie\n\
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

    /// The strongest guarantee: the shipped `.eggscene` round-trips.
    #[test]
    fn emit_round_trips_every_shipped_cutscene() {
        let file = parse(include_str!("../../../assets/data/main.eggscene")).expect("parse main");
        assert!(!file.cutscenes.is_empty(), "expected shipped cutscenes");
        let reparsed = parse(&emit_scene(&file)).expect("re-parse emitted");
        assert_eq!(file, reparsed);
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

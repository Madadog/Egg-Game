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

//! `.eggscene` — a small line-oriented DSL for authoring **cutscenes**: the
//! walk / wait / face / dialogue choreography a map plays. It is deliberately a
//! *separate* registry from the dialogue ([`crate::data::eggtext`] /
//! [`crate::data::script::ScriptFile`]):
//!
//! * Choreography is **language-independent** — it is authored once, in one
//!   file, and never translated. A cutscene's *dialogue* step refers to a
//!   `#dialogue` block **by key**, resolved at play time against whatever
//!   language is loaded (see [`crate::data::script::Script::get_dialogue`]), so
//!   the spoken text follows the active language while the staging does not.
//! * Keeping it out of the dialogue file stops walk/wait/face instructions from
//!   bloating `en.eggtext`, where they would be noise to a translator.
//!
//! The parsed result is a [`SceneFile`]: a registry of named [`CutsceneDef`]s,
//! held by the host alongside the [`Script`](crate::data::script::Script) (see
//! [`crate::EggState`]) and looked up when a `cutscene`-typed map object fires.
//!
//! # The format
//!
//! A `#cutscene NAME` header at column 0 opens a block; its indented body is one
//! `#verb args` per line. A **blank line within a block starts a new stage**.
//! Items inside one stage run in *parallel* each frame; stages run in
//! *sequence* (the next begins only when every item of the current one is done).
//! Blank lines and `//` comments are otherwise ignored. The line grammar, the
//! `"quoted"`/escaping rules and the block scanner are shared verbatim with the
//! `.eggtext` parser.
//!
//! ```text
//! #cutscene pet_dog
//!     // stage 0: walk up to the dog (each verb is its own stage here)
//!     walk 120 64
//!
//!     face 1 1
//!
//!     // stage 2: bark sound, then the dog's line, in parallel
//!     sound pop
//!     dialogue pet_dog_woof
//!
//!     walk 112 72
//! ```
//!
//! ## Verbs
//!
//! | verb | args | runtime [`CutsceneItem`] |
//! |------|------|--------------------------|
//! | `wait N`            | frames            | `Wait` |
//! | `dialogue KEY`      | dialogue-registry key | `Dialogue` (resolved at play time) |
//! | `set FLAG BOOL`     | flag name + `true`/`false` | `SetFlag` |
//! | `sound NAME`        | a known sound name | `Sound` |
//! | `music [NAME]`      | a track name, or nothing to stop | `Music` |
//! | `walk X Y`          | target pixel | `WalkPlayer` |
//! | `move X Y`          | target pixel (teleport-walk) | `MovePlayer` |
//! | `face DX DY`        | facing direction | `Face` |
//!
//! [`CutsceneItem`]: crate::gamestate::walkaround::cutscene::CutsceneItem

use std::collections::HashMap;

use crate::position::Vec2;

pub use super::eggtext::ParseError;
use super::eggtext::{collect_block, is_comment, split_first_word};

/// A parsed cutscene: stages in order, each stage a parallel run of [`StepDef`]s.
/// This is the language- and host-independent *definition*; the runtime
/// [`Cutscene`](crate::gamestate::walkaround::cutscene::Cutscene) is built from
/// it at launch (see
/// [`build`](crate::gamestate::walkaround::cutscene::Cutscene::from_def)).
pub type CutsceneDef = Vec<Vec<StepDef>>;

/// The parsed cutscene registry: every `#cutscene NAME` block by name. Held by
/// the host next to the script (see [`crate::EggState::scenes`]); a map object
/// of type `cutscene` names one of these.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneFile {
    pub cutscenes: HashMap<String, CutsceneDef>,
}

impl SceneFile {
    /// A cutscene definition by name, or `None` if undefined.
    pub fn get_cutscene(&self, name: &str) -> Option<&CutsceneDef> {
        self.cutscenes.get(name)
    }
}

/// One parsed cutscene step: a verb with its arguments resolved to scalars, but
/// *not* yet bound to host services (a sound/music name is still a string; the
/// runtime resolves those at build time, and a dialogue key at play time). The
/// inverse of [`emit_step`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepDef {
    /// Hold the stage for `N` frames.
    Wait(u32),
    /// Play the `#dialogue` block named by this key against the active script;
    /// done when the box closes. The key is **not** resolved here.
    Dialogue(String),
    /// Set a named save flag to a value.
    SetFlag(String, bool),
    /// Play a sound effect by name (resolved against [`sound::by_name`] at build).
    ///
    /// [`sound::by_name`]: crate::data::sound::by_name
    Sound(String),
    /// Switch the music to a named track, or stop it (`None`).
    Music(Option<String>),
    /// Walk the player to a target pixel using the normal movement/collision.
    Walk(Vec2),
    /// Move the player to a target pixel by direct steps (ignoring collision).
    Move(Vec2),
    /// Face the player in a direction.
    Face(i8, i8),
}

/// Parse a whole `.eggscene` source into a [`SceneFile`]. Errors carry the
/// 1-based source line, like [`crate::data::eggtext::parse`].
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
            return Err(ParseError::new(
                line_no,
                "indented line is not inside a block",
            ));
        }
        let Some(header) = logical.strip_prefix('#') else {
            return Err(ParseError::new(
                line_no,
                "expected a block (`#cutscene name`)",
            ));
        };
        let (kind, name) = split_first_word(header);
        if kind != "cutscene" {
            return Err(ParseError::new(
                line_no,
                format!("unknown block `#{kind}` (expected `#cutscene`)"),
            ));
        }
        if name.is_empty() {
            return Err(ParseError::new(line_no, "`#cutscene` needs a name"));
        }
        let body = collect_block(&mut lines);
        let stages = parse_stages(&body)?;
        file.cutscenes.insert(name.to_string(), stages);
    }

    Ok(file)
}

/// A `#cutscene` body: indented `#`-free verb lines grouped into stages by blank
/// lines. A blank line ends the current stage; consecutive blanks (and leading/
/// trailing ones) collapse, so an empty stage is never produced.
fn parse_stages(body: &[(usize, &str)]) -> Result<CutsceneDef, ParseError> {
    let mut stages: CutsceneDef = Vec::new();
    let mut current: Vec<StepDef> = Vec::new();
    for &(line_no, raw) in body {
        let logical = raw.trim_start();
        if logical.is_empty() {
            if !current.is_empty() {
                stages.push(std::mem::take(&mut current));
            }
            continue;
        }
        if is_comment(logical) {
            continue;
        }
        current.push(parse_step(logical, line_no)?);
    }
    if !current.is_empty() {
        stages.push(current);
    }
    Ok(stages)
}

/// Parse one verb line into a [`StepDef`]. The verb is the first word; the rest
/// are its arguments.
fn parse_step(logical: &str, line_no: usize) -> Result<StepDef, ParseError> {
    let (verb, args) = split_first_word(logical);
    Ok(match verb {
        "wait" => StepDef::Wait(parse_u32(args, line_no, "`wait` needs a frame count")?),
        "dialogue" => StepDef::Dialogue(require_name(args, line_no, "`dialogue` needs a key")?),
        "set" => {
            let (name, value) = split_first_word(args);
            if name.is_empty() {
                return Err(ParseError::new(line_no, "`set` needs `FLAG BOOL`"));
            }
            StepDef::SetFlag(name.to_string(), parse_bool(value, line_no)?)
        }
        "sound" => StepDef::Sound(require_name(args, line_no, "`sound` needs a name")?),
        // `music` with no argument stops the music; with one, plays that track.
        "music" => StepDef::Music((!args.trim().is_empty()).then(|| args.trim().to_string())),
        "walk" => StepDef::Walk(parse_vec2(args, line_no, "walk")?),
        "move" => StepDef::Move(parse_vec2(args, line_no, "move")?),
        "face" => {
            let (dx, dy) = parse_pair(args, line_no, "face")?;
            StepDef::Face(
                i8::try_from(dx)
                    .map_err(|_| ParseError::new(line_no, "`face` dx is out of range"))?,
                i8::try_from(dy)
                    .map_err(|_| ParseError::new(line_no, "`face` dy is out of range"))?,
            )
        }
        other => return Err(ParseError::new(line_no, format!("unknown verb `{other}`"))),
    })
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

/// Parse a whitespace-separated `X Y` integer pair.
fn parse_pair(args: &str, line_no: usize, verb: &str) -> Result<(i16, i16), ParseError> {
    let mut parts = args.split_whitespace();
    let err = || ParseError::new(line_no, format!("`{verb}` needs `X Y` integers"));
    let x = parts.next().and_then(|s| s.parse().ok()).ok_or_else(err)?;
    let y = parts.next().and_then(|s| s.parse().ok()).ok_or_else(err)?;
    if parts.next().is_some() {
        return Err(ParseError::new(
            line_no,
            format!("`{verb}` takes exactly `X Y`"),
        ));
    }
    Ok((x, y))
}

fn parse_vec2(args: &str, line_no: usize, verb: &str) -> Result<Vec2, ParseError> {
    let (x, y) = parse_pair(args, line_no, verb)?;
    Ok(Vec2::new(x, y))
}

// --- emitting: the inverse of `parse`, for a whole `.eggscene` file ---

/// Emit a whole [`SceneFile`] back to `.eggscene` text that re-parses to the
/// same registry. Cutscenes are emitted in sorted-name order for a stable
/// output. The inverse of [`parse`].
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

/// Emit one `#cutscene <name>` block. Stages are separated by a blank line; the
/// returned string ends without a trailing newline.
pub fn emit_cutscene(name: &str, def: &CutsceneDef) -> String {
    let mut out = format!("#cutscene {name}\n");
    for (i, stage) in def.iter().enumerate() {
        if i != 0 {
            out.push('\n');
        }
        for step in stage {
            out.push_str("    ");
            out.push_str(&emit_step(step));
            out.push('\n');
        }
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Render one [`StepDef`] as its `.eggscene` verb line. The inverse of
/// [`parse_step`].
fn emit_step(step: &StepDef) -> String {
    match step {
        StepDef::Wait(frames) => format!("wait {frames}"),
        StepDef::Dialogue(key) => format!("dialogue {key}"),
        StepDef::SetFlag(name, value) => format!("set {name} {value}"),
        StepDef::Sound(name) => format!("sound {name}"),
        StepDef::Music(Some(track)) => format!("music {track}"),
        StepDef::Music(None) => "music".to_string(),
        StepDef::Walk(pos) => format!("walk {} {}", pos.x, pos.y),
        StepDef::Move(pos) => format!("move {} {}", pos.x, pos.y),
        StepDef::Face(dx, dy) => format!("face {dx} {dy}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The single cutscene `src` defines, by name.
    fn one(src: &str) -> CutsceneDef {
        let file = parse(src).expect("parse");
        file.cutscenes.into_values().next().expect("one cutscene")
    }

    #[test]
    fn blank_lines_split_stages() {
        let def = one("#cutscene c\n    wait 5\n    sound pop\n\n    walk 1 2");
        assert_eq!(def.len(), 2);
        assert_eq!(def[0], vec![StepDef::Wait(5), StepDef::Sound("pop".into())]);
        assert_eq!(def[1], vec![StepDef::Walk(Vec2::new(1, 2))]);
    }

    #[test]
    fn consecutive_blanks_make_no_empty_stage() {
        let def = one("#cutscene c\n    wait 1\n\n\n    wait 2\n");
        assert_eq!(def.len(), 2);
        assert_eq!(def[0], vec![StepDef::Wait(1)]);
        assert_eq!(def[1], vec![StepDef::Wait(2)]);
    }

    #[test]
    fn every_verb_parses() {
        let def = one("#cutscene c\n\
             \x20   wait 30\n\
             \x20   dialogue some_key\n\
             \x20   set seen true\n\
             \x20   sound pop\n\
             \x20   music theme\n\
             \x20   walk 10 20\n\
             \x20   move 30 40\n\
             \x20   face -1 1");
        assert_eq!(
            def[0],
            vec![
                StepDef::Wait(30),
                StepDef::Dialogue("some_key".into()),
                StepDef::SetFlag("seen".into(), true),
                StepDef::Sound("pop".into()),
                StepDef::Music(Some("theme".into())),
                StepDef::Walk(Vec2::new(10, 20)),
                StepDef::Move(Vec2::new(30, 40)),
                StepDef::Face(-1, 1),
            ],
        );
    }

    #[test]
    fn music_with_no_arg_stops() {
        let def = one("#cutscene c\n    music");
        assert_eq!(def[0], vec![StepDef::Music(None)]);
    }

    #[test]
    fn comments_and_blank_only_body_are_ignored() {
        // A body that is only a comment yields a cutscene with no stages.
        let def = one("#cutscene c\n    // nothing here");
        assert!(def.is_empty());
    }

    #[test]
    fn errors_point_at_the_line() {
        assert_eq!(parse("#cutscene c\n    bogus 1").unwrap_err().line, 2);
        assert_eq!(parse("#cutscene c\n    wait").unwrap_err().line, 2);
        assert_eq!(
            parse("#cutscene c\n    set seen maybe").unwrap_err().line,
            2
        );
        assert_eq!(parse("#wat name").unwrap_err().line, 1);
        assert_eq!(parse("ok\n   stray").unwrap_err().line, 1);
        assert_eq!(parse("#cutscene").unwrap_err().line, 1);
        assert_eq!(parse("#cutscene c\n    walk 1").unwrap_err().line, 2);
    }

    // --- emitter ---

    /// Emit then re-parse a file round-trips it.
    fn round_trip(src: &str) -> (SceneFile, SceneFile) {
        let file = parse(src).expect("parse");
        let reparsed = parse(&emit_scene(&file)).expect("re-parse emitted");
        (file, reparsed)
    }

    #[test]
    fn emit_round_trips_a_multi_stage_scene() {
        let (file, reparsed) = round_trip(
            "#cutscene a\n\
             \x20   walk 5 6\n\
             \x20   sound pop\n\n\
             \x20   dialogue hello\n\n\
             \x20   move 7 8\n\
             #cutscene b\n\
             \x20   wait 10\n\
             \x20   music\n\
             \x20   face 1 0",
        );
        assert_eq!(file, reparsed);
    }

    /// The strongest guarantee: every block in the shipped `.eggscene` emits to
    /// text that re-parses to the identical registry.
    #[test]
    fn emit_round_trips_every_shipped_cutscene() {
        let file = parse(include_str!("../../../assets/script/main.eggscene")).expect("parse main");
        assert!(
            !file.cutscenes.is_empty(),
            "expected shipped cutscenes to test"
        );
        let reparsed = parse(&emit_scene(&file)).expect("re-parse emitted");
        assert_eq!(file, reparsed);
    }
}

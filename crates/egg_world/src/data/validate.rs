//! Cross-reference checker for the game's whole data web — dialogue keys,
//! cutscene names, map/warp targets, portraits, sounds, presets and save
//! flags. Every reference this module checks resolves *silently* at runtime
//! today: a dangling dialogue key falls back to the `default` entry
//! ([`Script::get_dialogue`](crate::data::script::Script::get_dialogue)), an
//! unknown portrait/sound is dropped with a `log::warn!`, a typo'd flag name
//! in a `.tmj` gate just always reads false. [`check`] turns that whole class
//! of mistake into a loud, listed [`Report`] instead — meant to run at
//! build/test time (a CI-pinned test asserting zero errors over the shipped
//! assets, and a `--check` CLI entry point), not to change any of that
//! runtime behaviour, which is deliberately left alone.
//!
//! [`check`] takes the parsed [`ScriptFile`]/[`SceneFile`], every loaded
//! map's parsed objects, and the registries names resolve against
//! ([`Portraits`], [`Presets`], and [`sound::by_name`]) — no I/O, no asset
//! path knowledge; the caller (a test, or a CLI harness) owns loading.

use std::collections::BTreeSet;
use std::collections::BTreeMap;
use std::fmt;

use crate::data::eggdata::Presets;
use crate::data::portraits::Portraits;
use crate::data::save::IS_NIGHT_FLAG;
use crate::data::scene::{CutsceneContent, GetEntity, SceneFile};
use crate::data::script::{ContentDef, DialogueDef, Entry, MessageDef, PortraitChange, ScriptFile, SegmentDef};
use crate::data::sound;
use crate::world::interact::Interaction;
use crate::world::map::{MapObject, ObjectEffect};

/// Dialogue keys the engine reaches by a hardcoded Rust string literal rather
/// than through script/scene/map content, so [`check`]'s dead-dialogue sweep
/// doesn't flag them despite nothing in the data web naming them. Found by
/// grepping the workspace for every `get_dialogue`/`Ctx::get_dialogue` call
/// site with a literal key (2026-07-18):
/// - `"default"` — [`Script::get_dialogue`](crate::data::script::Script::get_dialogue)'s
///   own fallback for an unresolvable key.
/// - `"dog_obtained"` / `"dog_relinquished"` — returned as `Option<&'static
///   str>` by `WalkaroundState::execute_interact_fn`'s `ToggleDog` arm
///   (`egg_core/src/gamestate/walkaround/mod.rs`) and passed to
///   `ctx.get_dialogue` one call site away, so they don't show up in a
///   single-hop grep for `get_dialogue("...")`.
///
/// A future literal call site should extend this list rather than get
/// silently reported as dead weight.
pub const ENGINE_DIALOGUE_ROOTS: &[&str] = &["default", "dog_obtained", "dog_relinquished"];

/// One thing [`check`] found wrong (or suspect) in the loaded data web. Every
/// variant carries exactly the context needed to find and fix it. Errors are
/// dangling references the runtime would otherwise resolve silently
/// ([`Finding::is_error`]); everything else is dead weight worth a look but
/// not build-breaking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// A map object's dialogue key (an interactable's `description`, or a
    /// warp's `narration`) names no entry in the script.
    DanglingDialogue { map: String, object: ObjectLabel, key: String },
    /// A map object's `cutscene` property names no `#cutscene` in the scene
    /// registry.
    DanglingCutscene { map: String, object: ObjectLabel, name: String },
    /// A warp's `to_map` names no loaded map.
    DanglingWarpMap { map: String, object: ObjectLabel, name: String },
    /// A map object's flag gate (`if`/`unless`/`sets`) names a flag the
    /// script never declares with `#flag`. Unlike `#set`/`#if` inside
    /// `.eggtext`, a `.tmj` gate's flag names are unchecked strings — this is
    /// the only place that typo gets caught.
    DanglingMapFlag { map: String, object: ObjectLabel, flag: String },

    /// A `#cutscene`'s `map NAME` init step names no loaded map.
    SceneDanglingMap { cutscene: String, name: String },
    /// A `#cutscene`'s `spawn`/`bind NAME PRESET X Y` names no preset.
    SceneDanglingPreset { cutscene: String, name: String },
    /// A `#cutscene`'s `dialogue KEY` step names no entry in the script.
    SceneDanglingDialogue { cutscene: String, key: String },
    /// A `#cutscene`'s `load NAME` step names no cutscene in the registry.
    SceneDanglingLoad { cutscene: String, name: String },
    /// A `#cutscene`'s `sound NAME` step names no known sound effect.
    SceneDanglingSound { cutscene: String, name: String },
    /// A `#cutscene`'s `set FLAG BOOL` step names a flag the script never
    /// declares.
    SceneDanglingFlag { cutscene: String, flag: String },

    /// A dialogue message's portrait switch (`#pic`, message-level or
    /// mid-message) names no portrait in the registry.
    DanglingPortrait { key: String, name: String },
    /// A dialogue message's `#sound` names no known sound effect.
    DanglingSound { key: String, name: String },
    /// A dialogue `#set` or `#choice` option names a flag the script's own
    /// `flags` vocabulary doesn't declare. Unreachable from `.eggtext`
    /// itself — the parser already rejects this at parse time (see
    /// `eggtext::check_flag`) — but a hand-authored or generated JSON script
    /// carries no such guarantee, which is what this covers.
    DanglingScriptFlag { key: String, flag: String },

    /// A dialogue entry no map, scene, or [`ENGINE_DIALOGUE_ROOTS`] reaches.
    UnreferencedDialogue { key: String },
    /// A declared `#flag` nothing ever sets (whether or not anything reads
    /// it — see [`Finding::FlagNeverSet`] for the "read but not set" case).
    UnusedFlag { flag: String },
    /// A declared flag some content sets but nothing ever reads.
    FlagNeverRead { flag: String },
    /// A declared flag some content reads (an `#if`/`#elif` or a map gate)
    /// but nothing ever sets — so the branch always goes the same way.
    FlagNeverSet { flag: String },
}

impl Finding {
    /// Whether this finding is a dangling reference (build-breaking) rather
    /// than dead weight (a warning worth a look).
    pub fn is_error(&self) -> bool {
        !matches!(
            self,
            Finding::UnreferencedDialogue { .. }
                | Finding::UnusedFlag { .. }
                | Finding::FlagNeverRead { .. }
                | Finding::FlagNeverSet { .. }
        )
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Finding::DanglingDialogue { map, object, key } => {
                write!(f, "map `{map}` object[{object}]: dialogue key {key:?} not found in script")
            }
            Finding::DanglingCutscene { map, object, name } => {
                write!(f, "map `{map}` object[{object}]: cutscene {name:?} not found in scene registry")
            }
            Finding::DanglingWarpMap { map, object, name } => {
                write!(f, "map `{map}` object[{object}]: warp targets unknown map {name:?}")
            }
            Finding::DanglingMapFlag { map, object, flag } => {
                write!(f, "map `{map}` object[{object}]: flag {flag:?} is not declared with `#flag`")
            }
            Finding::SceneDanglingMap { cutscene, name } => {
                write!(f, "scene `{cutscene}`: `map {name}` targets an unknown map")
            }
            Finding::SceneDanglingPreset { cutscene, name } => {
                write!(f, "scene `{cutscene}`: preset {name:?} not found")
            }
            Finding::SceneDanglingDialogue { cutscene, key } => {
                write!(f, "scene `{cutscene}`: dialogue key {key:?} not found in script")
            }
            Finding::SceneDanglingLoad { cutscene, name } => {
                write!(f, "scene `{cutscene}`: `load {name}` names an unknown cutscene")
            }
            Finding::SceneDanglingSound { cutscene, name } => {
                write!(f, "scene `{cutscene}`: sound {name:?} not found")
            }
            Finding::SceneDanglingFlag { cutscene, flag } => {
                write!(f, "scene `{cutscene}`: flag {flag:?} is not declared with `#flag`")
            }
            Finding::DanglingPortrait { key, name } => {
                write!(f, "dialogue `{key}`: unknown portrait {name:?}")
            }
            Finding::DanglingSound { key, name } => {
                write!(f, "dialogue `{key}`: unknown sound {name:?}")
            }
            Finding::DanglingScriptFlag { key, flag } => {
                write!(f, "dialogue `{key}`: flag {flag:?} is not declared with `#flag`")
            }
            Finding::UnreferencedDialogue { key } => {
                write!(f, "dialogue `{key}` is never referenced by any map, scene, or engine code path")
            }
            Finding::UnusedFlag { flag } => {
                write!(f, "flag `{flag}` is declared but never set")
            }
            Finding::FlagNeverRead { flag } => {
                write!(f, "flag `{flag}` is set but never read")
            }
            Finding::FlagNeverSet { flag } => {
                write!(f, "flag `{flag}` is read but never set")
            }
        }
    }
}

/// How a map object is identified in a [`Finding`]: its stable Tiled id, or
/// (for a hand-authored object Tiled hasn't stamped one onto) its map-pixel
/// position — always something to search a `.tmj` for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectLabel {
    Id(usize),
    Pos(i16, i16),
}

impl fmt::Display for ObjectLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectLabel::Id(id) => write!(f, "id={id}"),
            ObjectLabel::Pos(x, y) => write!(f, "pos={x},{y}"),
        }
    }
}

impl ObjectLabel {
    fn of(object: &MapObject) -> Self {
        match object.id {
            Some(id) => ObjectLabel::Id(id),
            None => ObjectLabel::Pos(object.hitbox.x, object.hitbox.y),
        }
    }
}

/// The result of [`check`]: every [`Finding`], grouped by severity.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub errors: Vec<Finding>,
    pub warnings: Vec<Finding>,
}

impl Report {
    /// Whether the data web is clean — no dangling references. Dead-weight
    /// warnings don't affect this; only [`Report::errors`] does.
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }

    fn push(&mut self, finding: Finding) {
        if finding.is_error() {
            self.errors.push(finding);
        } else {
            self.warnings.push(finding);
        }
    }
}

impl fmt::Display for Report {
    /// Errors then warnings, one finding per line, each prefixed with its
    /// severity — the shape a CLI `--check` prints and a human reads top to
    /// bottom.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for e in &self.errors {
            writeln!(f, "error: {e}")?;
        }
        for w in &self.warnings {
            writeln!(f, "warning: {w}")?;
        }
        Ok(())
    }
}

/// Cross-reference the game's whole data web and report what's dangling or
/// unused. `maps` is every loaded map's parsed object list, keyed by map name
/// (the warp/cutscene-reference target); `dialogue_roots` is
/// [`ENGINE_DIALOGUE_ROOTS`] in production use, taken as a parameter so a
/// test can extend or trim it without editing this function. Traverses maps,
/// then scenes, then script — each name-sorted internally where its source
/// collection isn't already ordered — so two runs over the same input always
/// produce [`Finding`]s in the same order.
pub fn check(
    script: &ScriptFile,
    scenes: &SceneFile,
    maps: &BTreeMap<String, Vec<MapObject>>,
    portraits: &Portraits,
    presets: &Presets,
    dialogue_roots: &[&str],
) -> Report {
    let mut report = Report::default();
    let mut referenced_dialogue: BTreeSet<String> =
        dialogue_roots.iter().map(|s| s.to_string()).collect();
    let mut set_flags: BTreeSet<String> = BTreeSet::new();
    let mut read_flags: BTreeSet<String> = BTreeSet::new();
    // `is_night` is read and set directly by the engine (the day/night swap
    // in `WalkaroundState`), never through script/scene/map content, so the
    // dead-flag sweep must not flag it just because nothing *authored* sets
    // or reads it.
    set_flags.insert(IS_NIGHT_FLAG.to_string());
    read_flags.insert(IS_NIGHT_FLAG.to_string());

    check_maps(maps, script, scenes, &mut report, &mut referenced_dialogue, &mut set_flags, &mut read_flags);
    check_scenes(scenes, script, maps, presets, &mut report, &mut referenced_dialogue, &mut set_flags);
    check_script(script, portraits, &mut report, &mut set_flags, &mut read_flags);

    let mut dialogue_keys: Vec<&String> = script.dialogue.keys().collect();
    dialogue_keys.sort();
    for key in dialogue_keys {
        if !referenced_dialogue.contains(key) {
            report.push(Finding::UnreferencedDialogue { key: key.clone() });
        }
    }
    for flag in &script.flags {
        let (is_set, is_read) = (set_flags.contains(flag), read_flags.contains(flag));
        match (is_set, is_read) {
            (false, false) => report.push(Finding::UnusedFlag { flag: flag.clone() }),
            (false, true) => report.push(Finding::FlagNeverSet { flag: flag.clone() }),
            (true, false) => report.push(Finding::FlagNeverRead { flag: flag.clone() }),
            (true, true) => {}
        }
    }

    report
}

/// The map half of [`check`]: every object's dialogue/cutscene/warp/gate
/// references, name-sorted by map for determinism.
fn check_maps(
    maps: &BTreeMap<String, Vec<MapObject>>,
    script: &ScriptFile,
    scenes: &SceneFile,
    report: &mut Report,
    referenced_dialogue: &mut BTreeSet<String>,
    set_flags: &mut BTreeSet<String>,
    read_flags: &mut BTreeSet<String>,
) {
    for (map, objects) in maps {
        for object in objects {
            let label = ObjectLabel::of(object);
            match &object.effect {
                ObjectEffect::Warp(warp) => {
                    if let Some(dest) = &warp.map
                        && !maps.contains_key(dest)
                    {
                        report.push(Finding::DanglingWarpMap {
                            map: map.clone(),
                            object: label,
                            name: dest.clone(),
                        });
                    }
                    if let Some(key) = &warp.narration {
                        referenced_dialogue.insert(key.clone());
                        if !script.dialogue.contains_key(key) {
                            report.push(Finding::DanglingDialogue {
                                map: map.clone(),
                                object: label,
                                key: key.clone(),
                            });
                        }
                    }
                }
                ObjectEffect::Interact(Interaction::Dialogue(key)) => {
                    referenced_dialogue.insert(key.clone());
                    if !script.dialogue.contains_key(key) {
                        report.push(Finding::DanglingDialogue {
                            map: map.clone(),
                            object: label,
                            key: key.clone(),
                        });
                    }
                }
                ObjectEffect::Interact(Interaction::Cutscene(name)) => {
                    if scenes.get_cutscene(name).is_none() {
                        report.push(Finding::DanglingCutscene {
                            map: map.clone(),
                            object: label,
                            name: name.clone(),
                        });
                    }
                }
                ObjectEffect::Interact(Interaction::Func(_) | Interaction::None) => {}
            }
            for flag in [&object.gate.if_flag, &object.gate.unless_flag] {
                if let Some(flag) = flag {
                    read_flags.insert(flag.clone());
                    if !script.flags.contains(flag) {
                        report.push(Finding::DanglingMapFlag {
                            map: map.clone(),
                            object: label,
                            flag: flag.clone(),
                        });
                    }
                }
            }
            if let Some(flag) = &object.gate.sets {
                set_flags.insert(flag.clone());
                if !script.flags.contains(flag) {
                    report.push(Finding::DanglingMapFlag {
                        map: map.clone(),
                        object: label,
                        flag: flag.clone(),
                    });
                }
            }
        }
    }
}

/// The scene half of [`check`]: every `#cutscene`'s init map/presets and
/// content steps' dialogue/load/sound/flag references, name-sorted for
/// determinism (`SceneFile::cutscenes` is a `HashMap`).
fn check_scenes(
    scenes: &SceneFile,
    script: &ScriptFile,
    maps: &BTreeMap<String, Vec<MapObject>>,
    presets: &Presets,
    report: &mut Report,
    referenced_dialogue: &mut BTreeSet<String>,
    set_flags: &mut BTreeSet<String>,
) {
    let mut names: Vec<&String> = scenes.cutscenes.keys().collect();
    names.sort();
    for name in names {
        let def = &scenes.cutscenes[name];
        if let Some(map_name) = &def.init_map
            && !maps.contains_key(map_name)
        {
            report.push(Finding::SceneDanglingMap { cutscene: name.clone(), name: map_name.clone() });
        }
        for entity in &def.init {
            if let GetEntity::Spawn { preset, .. } | GetEntity::GetOrSpawn { preset, .. } = entity
                && presets.get(preset).is_none()
            {
                report.push(Finding::SceneDanglingPreset {
                    cutscene: name.clone(),
                    name: preset.as_str().to_string(),
                });
            }
        }
        for step in &def.content {
            match step {
                CutsceneContent::Dialogue(key) => {
                    referenced_dialogue.insert(key.clone());
                    if !script.dialogue.contains_key(key) {
                        report.push(Finding::SceneDanglingDialogue { cutscene: name.clone(), key: key.clone() });
                    }
                }
                CutsceneContent::Load(target) => {
                    if scenes.get_cutscene(target).is_none() {
                        report.push(Finding::SceneDanglingLoad { cutscene: name.clone(), name: target.clone() });
                    }
                }
                CutsceneContent::Sound(sfx) => {
                    if sound::by_name(sfx).is_none() {
                        report.push(Finding::SceneDanglingSound { cutscene: name.clone(), name: sfx.clone() });
                    }
                }
                CutsceneContent::SetFlag(flag, _) => {
                    set_flags.insert(flag.clone());
                    if !script.flags.contains(flag) {
                        report.push(Finding::SceneDanglingFlag { cutscene: name.clone(), flag: flag.clone() });
                    }
                }
                // `music` names are directory-scanned free-form at play time
                // (no fixed vocabulary to check against — see `data/tiled.rs`'s
                // `TiledMap::music` doc); actor names in `Move`/`Interact` are
                // scene-local bindings, not a global registry.
                CutsceneContent::Music(_)
                | CutsceneContent::Move(_)
                | CutsceneContent::Interact { .. }
                | CutsceneContent::Wait(_)
                | CutsceneContent::Camera(..)
                | CutsceneContent::Shake { .. } => {}
            }
        }
    }
}

/// The script half of [`check`]: every dialogue entry's portrait/sound/flag
/// references, name-sorted for determinism (`ScriptFile::dialogue` is a
/// `HashMap`). Walks the *raw*, pre-resolution [`DialogueDef`] tree rather
/// than a resolved `Message`/`TextContent`: resolution already drops an
/// unknown sound and clears an unknown portrait (see `ContentDef::resolve`),
/// so by the time a `Message` exists the bad name is gone — this only sees it
/// here, before that happens.
fn check_script(
    script: &ScriptFile,
    portraits: &Portraits,
    report: &mut Report,
    set_flags: &mut BTreeSet<String>,
    read_flags: &mut BTreeSet<String>,
) {
    let mut keys: Vec<&String> = script.dialogue.keys().collect();
    keys.sort();
    for key in keys {
        walk_dialogue(key, &script.dialogue[key], &script.flags, portraits, report, set_flags, read_flags);
    }
}

fn walk_dialogue(
    key: &str,
    def: &DialogueDef,
    flags: &BTreeSet<String>,
    portraits: &Portraits,
    report: &mut Report,
    set_flags: &mut BTreeSet<String>,
    read_flags: &mut BTreeSet<String>,
) {
    match def {
        DialogueDef::Plain(entry) => walk_entry(key, entry, flags, portraits, report, set_flags, read_flags),
        DialogueDef::Segments { segments } => {
            for seg in segments {
                walk_segment(key, seg, flags, portraits, report, set_flags, read_flags);
            }
        }
    }
}

fn walk_segment(
    key: &str,
    seg: &SegmentDef,
    flags: &BTreeSet<String>,
    portraits: &Portraits,
    report: &mut Report,
    set_flags: &mut BTreeSet<String>,
    read_flags: &mut BTreeSet<String>,
) {
    fn check_condition(
        key: &str,
        flag: &str,
        flags: &BTreeSet<String>,
        report: &mut Report,
        read_flags: &mut BTreeSet<String>,
    ) {
        read_flags.insert(flag.to_string());
        if !flags.contains(flag) {
            report.push(Finding::DanglingScriptFlag { key: key.to_string(), flag: flag.to_string() });
        }
    }
    match seg {
        SegmentDef::Plain(entry) => walk_entry(key, entry, flags, portraits, report, set_flags, read_flags),
        SegmentDef::If { flag, then, otherwise, elifs, .. } => {
            check_condition(key, flag, flags, report, read_flags);
            walk_dialogue(key, then, flags, portraits, report, set_flags, read_flags);
            if let Some(otherwise) = otherwise {
                walk_dialogue(key, otherwise, flags, portraits, report, set_flags, read_flags);
            }
            for elif in elifs {
                check_condition(key, &elif.flag, flags, report, read_flags);
                walk_dialogue(key, &elif.then, flags, portraits, report, set_flags, read_flags);
            }
        }
    }
}

fn walk_entry(
    key: &str,
    entry: &Entry,
    flags: &BTreeSet<String>,
    portraits: &Portraits,
    report: &mut Report,
    set_flags: &mut BTreeSet<String>,
    read_flags: &mut BTreeSet<String>,
) {
    if let Entry::Conversation { messages } = entry {
        for message in messages {
            walk_message(key, message, flags, portraits, report, set_flags, read_flags);
        }
    }
    // `Entry::Line`/`Entry::Pages` are plain text — no directives to check.
}

fn walk_message(
    key: &str,
    message: &MessageDef,
    flags: &BTreeSet<String>,
    portraits: &Portraits,
    report: &mut Report,
    set_flags: &mut BTreeSet<String>,
    read_flags: &mut BTreeSet<String>,
) {
    if let PortraitChange::Set(name) = &message.portrait
        && portraits.get(name).is_none()
    {
        report.push(Finding::DanglingPortrait { key: key.to_string(), name: name.clone() });
    }
    for content in &message.content {
        walk_content(key, content, flags, portraits, report, set_flags, read_flags);
    }
}

fn walk_content(
    key: &str,
    content: &ContentDef,
    flags: &BTreeSet<String>,
    portraits: &Portraits,
    report: &mut Report,
    set_flags: &mut BTreeSet<String>,
    _read_flags: &mut BTreeSet<String>,
) {
    match content {
        ContentDef::Sound(name) => {
            if sound::by_name(name).is_none() {
                report.push(Finding::DanglingSound { key: key.to_string(), name: name.clone() });
            }
        }
        ContentDef::Portrait(Some(name)) => {
            if portraits.get(name).is_none() {
                report.push(Finding::DanglingPortrait { key: key.to_string(), name: name.clone() });
            }
        }
        ContentDef::SetFlag(name, _) => {
            set_flags.insert(name.clone());
            if !flags.contains(name) {
                report.push(Finding::DanglingScriptFlag { key: key.to_string(), flag: name.clone() });
            }
        }
        ContentDef::Choice(options) => {
            for option in options {
                for (name, _) in &option.sets {
                    set_flags.insert(name.clone());
                    if !flags.contains(name) {
                        report.push(Finding::DanglingScriptFlag { key: key.to_string(), flag: name.clone() });
                    }
                }
            }
        }
        ContentDef::Text(_)
        | ContentDef::Auto(_)
        | ContentDef::Delayed(_, _)
        | ContentDef::Delay(_)
        | ContentDef::Portrait(None)
        | ContentDef::Pause
        | ContentDef::Flip(_)
        | ContentDef::Shake(_, _)
        | ContentDef::Speed(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::script::eggtext;

    fn maps(pairs: Vec<(&str, Vec<MapObject>)>) -> BTreeMap<String, Vec<MapObject>> {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    fn script(src: &str) -> ScriptFile {
        eggtext::parse(src).expect("parse eggtext")
    }

    /// A clean data web (nothing referencing anything) reports no findings.
    #[test]
    fn empty_input_is_clean() {
        let report = check(
            &ScriptFile::default(),
            &SceneFile::default(),
            &maps(vec![]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert!(report.is_clean());
        assert!(report.warnings.is_empty());
    }

    /// A map warp's `to_map` naming an unloaded map is a dangling-map error;
    /// a warp targeting a map that IS loaded is clean.
    #[test]
    fn dangling_warp_map_is_an_error() {
        use crate::world::map::{MapObject, Warp};
        use egg_render::geometry::{Hitbox, Vec2};

        let bad = MapObject::warp(Hitbox::new(0, 0, 8, 8), Warp::new(Some("nowhere"), Vec2::new(0, 0)));
        let good = MapObject::warp(Hitbox::new(0, 0, 8, 8), Warp::new(Some("elsewhere"), Vec2::new(0, 0)));
        let report = check(
            &ScriptFile::default(),
            &SceneFile::default(),
            &maps(vec![("here", vec![bad]), ("elsewhere", vec![])]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(&report.errors[0], Finding::DanglingWarpMap { name, .. } if name == "nowhere"));

        let report = check(
            &ScriptFile::default(),
            &SceneFile::default(),
            &maps(vec![("here", vec![good]), ("elsewhere", vec![])]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert!(report.is_clean());
    }

    /// A map interactable's dialogue key resolves against the script; a
    /// dangling one is an error, and (whether or not it resolves) the key
    /// counts as referenced, so a defined-and-referenced key is never also
    /// flagged as dead weight.
    #[test]
    fn dangling_and_live_dialogue_keys() {
        use crate::world::map::MapObject;
        use egg_render::geometry::Hitbox;

        let live = MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "greet");
        let dangling = MapObject::dialogue(Hitbox::new(8, 0, 8, 8), "nope");
        let script = script("#dialogue greet\n    Hi.");
        let report = check(
            &script,
            &SceneFile::default(),
            &maps(vec![("here", vec![live, dangling])]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(&report.errors[0], Finding::DanglingDialogue { key, .. } if key == "nope"));
        assert!(
            report.warnings.iter().all(|w| !matches!(w, Finding::UnreferencedDialogue { .. })),
            "the referenced `greet` key must not also show up as dead weight: {:?}",
            report.warnings,
        );
    }

    /// An unreferenced dialogue entry is dead-weight (a warning, not an
    /// error); the engine-literal roots (`default`, …) are exempt even
    /// though nothing in the data web names them.
    #[test]
    fn unreferenced_dialogue_is_a_warning_and_roots_are_exempt() {
        let script = script("#dialogue orphan\n    Hi.\n#dialogue default\n    Fallback.");
        let report = check(
            &script,
            &SceneFile::default(),
            &maps(vec![]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert!(report.is_clean());
        assert_eq!(report.warnings.len(), 1);
        assert!(matches!(&report.warnings[0], Finding::UnreferencedDialogue { key } if key == "orphan"));
    }

    /// A `#pic`/`#sound` naming an unknown portrait/sound is an error at the
    /// dialogue key it appears in.
    #[test]
    fn dangling_portrait_and_sound_in_script() {
        let script = script(
            "#dialogue greet\n    #pic nope_portrait\n    #sound nope_sound\n    Hi.",
        );
        let report = check(
            &script,
            &SceneFile::default(),
            &maps(vec![]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert_eq!(report.errors.len(), 2);
        assert!(report.errors.iter().any(|e| matches!(e, Finding::DanglingPortrait { name, .. } if name == "nope_portrait")));
        assert!(report.errors.iter().any(|e| matches!(e, Finding::DanglingSound { name, .. } if name == "nope_sound")));
    }

    /// The three-way flag classification: declared-and-untouched (`Unused`),
    /// set-but-never-read, and read-but-never-set are distinct findings, and
    /// a flag both set and read is clean.
    #[test]
    fn flag_classification() {
        let script = script(
            "#flag untouched\n#flag written_only\n#flag read_only\n#flag both\n\
             #dialogue d\n\
             \x20   #set written_only true\n\
             \x20   #if both\n\
             \x20   Yes.\n\
             \x20   #else\n\
             \x20   No.\n\
             \x20   #end\n\
             \x20   #set both true",
        );
        // `read_only` is read only via a map gate (never set anywhere).
        use crate::world::map::{Gate, MapObject};
        use egg_render::geometry::Hitbox;
        let gated = MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "d")
            .with_gate(Gate { if_flag: Some("read_only".to_string()), unless_flag: None, sets: None });

        let report = check(
            &script,
            &SceneFile::default(),
            &maps(vec![("here", vec![gated])]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert!(report.is_clean(), "no dangling refs expected: {:?}", report.errors);
        assert!(report.warnings.contains(&Finding::UnusedFlag { flag: "untouched".into() }));
        assert!(report.warnings.contains(&Finding::FlagNeverRead { flag: "written_only".into() }));
        assert!(report.warnings.contains(&Finding::FlagNeverSet { flag: "read_only".into() }));
        assert!(!report.warnings.iter().any(|w| matches!(w, Finding::UnusedFlag { flag } | Finding::FlagNeverRead { flag } | Finding::FlagNeverSet { flag } if flag == "both")));
    }

    /// A `.tmj` gate flag name outside the declared vocabulary is an error —
    /// the one place with no parse-time enforcement (`eggtext` only checks
    /// `#set`/`#if` inside `.eggtext` itself).
    #[test]
    fn dangling_map_gate_flag_is_an_error() {
        use crate::world::map::{Gate, MapObject};
        use egg_render::geometry::Hitbox;
        let gated = MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "d")
            .with_gate(Gate { if_flag: Some("undeclared".to_string()), unless_flag: None, sets: None });
        let script = script("#dialogue d\n    Hi.");
        let report = check(
            &script,
            &SceneFile::default(),
            &maps(vec![("here", vec![gated])]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(&report.errors[0], Finding::DanglingMapFlag { flag, .. } if flag == "undeclared"));
    }

    /// A scene's `spawn`/`bind` preset, `dialogue`/`load`/`sound` targets,
    /// and `set` flag are each cross-referenced; a scene naming real targets
    /// is clean.
    #[test]
    fn scene_cross_references() {
        use crate::data::scene;
        let script = script("#flag done\n#dialogue hello\n    Hi.");
        let scenes = scene::parse(
            "#cutscene a\n\
             \x20   spawn fido dog 0 0\n\
             \x20   dialogue hello\n\
             \x20   sound pop\n\
             \x20   set done true\n\
             \x20   load b\n\
             #cutscene b\n\
             \x20   wait 1",
        )
        .expect("parse scene");
        let report = check(
            &script,
            &scenes,
            &maps(vec![]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert!(report.is_clean(), "{:?}", report.errors);

        let bad_scenes = scene::parse(
            "#cutscene a\n\
             \x20   spawn fido nonexistent_preset 0 0\n\
             \x20   dialogue nope\n\
             \x20   sound nope\n\
             \x20   set undeclared true\n\
             \x20   load nope",
        )
        .expect("parse scene");
        let report = check(
            &script,
            &bad_scenes,
            &maps(vec![]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert_eq!(report.errors.len(), 5, "{:?}", report.errors);
    }

    /// `Report`'s `Display` prints errors before warnings, one per line.
    #[test]
    fn report_display_groups_errors_before_warnings() {
        let mut report = Report::default();
        report.push(Finding::UnusedFlag { flag: "w".into() });
        report.push(Finding::DanglingSound { key: "k".into(), name: "s".into() });
        let text = report.to_string();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("error:"), "{text}");
        assert!(lines[1].starts_with("warning:"), "{text}");
    }
}

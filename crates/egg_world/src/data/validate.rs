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
use crate::data::scene::{CutsceneContent, GetEntity, Motion, SceneFile};
use crate::data::script::{
    ChoiceOptionDef, ContentDef, DialogueDef, ElifDef, Entry, MessageDef, PortraitChange, ScriptFile,
    SegmentDef,
};
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
    /// A `#cutscene`'s `path [noclip] NAME` motion names no `#path` block in
    /// the registry.
    SceneDanglingPath { cutscene: String, name: String },
    /// A `#cutscene`'s `dialogue KEY` step has an `on NAME [wait]` handler
    /// whose cue that dialogue never reaches with a matching `#cue NAME` —
    /// dead choreography, the handler can never fire. The reverse — a
    /// `#cue` no handler names — is not a finding (it may be a stage
    /// direction, or a beat for a different scene).
    SceneDanglingCue { cutscene: String, key: String, cue: String },

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
    /// A `#path` block no `path` motion in any scene references — likely a
    /// stale recording (see the `.eggscene` module doc).
    UnreferencedPath { name: String },
    /// A declared `#flag` nothing ever sets (whether or not anything reads
    /// it — see [`Finding::FlagNeverSet`] for the "read but not set" case).
    UnusedFlag { flag: String },
    /// A declared flag some content sets but nothing ever reads.
    FlagNeverRead { flag: String },
    /// A declared flag some content reads (an `#if`/`#elif` or a map gate)
    /// but nothing ever sets — so the branch always goes the same way.
    FlagNeverSet { flag: String },

    /// A language overlay's dialogue entry has drifted structurally from the
    /// base entry it translates — see [`check_overlay`]. `path` is a
    /// human-readable pointer to the first place they diverge (e.g. `message
    /// 3: directive 2: #pic "a_normal" vs "b_open"`, or `branch structure
    /// differs at #if "FLAG"`).
    OverlaySkeletonMismatch { lang: String, key: String, path: String },
    /// A language overlay defines a dialogue key the base script doesn't —
    /// it can never be reached as a coherent translation (the base is the
    /// authority on what keys exist; an overlay only *overrides* base keys).
    OverlayOrphanDialogue { lang: String, key: String },
    /// A language overlay's `#list` has a different entry count than the
    /// base's same-named list — list entries are read back by index, so a
    /// length mismatch silently shifts every entry after the drift.
    OverlayListLength { lang: String, key: String, base_len: usize, overlay_len: usize },
    /// A language overlay declares a `#flag` the base script never declares.
    /// The base is the sole authority on the flag vocabulary — an overlay
    /// only ever reads/sets flags the base already named.
    OverlayUndeclaredFlag { lang: String, flag: String },
}

impl Finding {
    /// Whether this finding is a dangling reference (build-breaking) rather
    /// than dead weight (a warning worth a look).
    pub fn is_error(&self) -> bool {
        !matches!(
            self,
            Finding::UnreferencedDialogue { .. }
                | Finding::UnreferencedPath { .. }
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
            Finding::SceneDanglingPath { cutscene, name } => {
                write!(f, "scene `{cutscene}`: `path {name}` names an unknown path")
            }
            Finding::SceneDanglingCue { cutscene, key, cue } => {
                write!(
                    f,
                    "scene `{cutscene}`: `on {cue}` on `dialogue {key}` names a cue that dialogue never reaches"
                )
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
            Finding::UnreferencedPath { name } => {
                write!(f, "path `{name}` is never referenced by any scene")
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
            Finding::OverlaySkeletonMismatch { lang, key, path } => {
                write!(f, "overlay `{lang}` dialogue `{key}`: skeleton differs from base: {path}")
            }
            Finding::OverlayOrphanDialogue { lang, key } => {
                write!(f, "overlay `{lang}`: dialogue `{key}` is not defined in the base script")
            }
            Finding::OverlayListLength { lang, key, base_len, overlay_len } => {
                write!(f, "overlay `{lang}`: list `{key}` has {overlay_len} entries, base has {base_len}")
            }
            Finding::OverlayUndeclaredFlag { lang, flag } => {
                write!(f, "overlay `{lang}`: flag `{flag}` is not declared in the base script")
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
    let mut referenced_paths: BTreeSet<String> = BTreeSet::new();
    let mut set_flags: BTreeSet<String> = BTreeSet::new();
    let mut read_flags: BTreeSet<String> = BTreeSet::new();
    // `is_night` is read and set directly by the engine (the day/night swap
    // in `WalkaroundState`), never through script/scene/map content, so the
    // dead-flag sweep must not flag it just because nothing *authored* sets
    // or reads it.
    set_flags.insert(IS_NIGHT_FLAG.to_string());
    read_flags.insert(IS_NIGHT_FLAG.to_string());

    check_maps(maps, script, scenes, &mut report, &mut referenced_dialogue, &mut set_flags, &mut read_flags);
    check_scenes(
        scenes,
        script,
        maps,
        presets,
        &mut report,
        &mut referenced_dialogue,
        &mut set_flags,
        &mut referenced_paths,
    );
    check_script(script, portraits, &mut report, &mut set_flags, &mut read_flags);

    let mut dialogue_keys: Vec<&String> = script.dialogue.keys().collect();
    dialogue_keys.sort();
    for key in dialogue_keys {
        if !referenced_dialogue.contains(key) {
            report.push(Finding::UnreferencedDialogue { key: key.clone() });
        }
    }
    let mut path_names: Vec<&String> = scenes.paths.keys().collect();
    path_names.sort();
    for name in path_names {
        if !referenced_paths.contains(name) {
            report.push(Finding::UnreferencedPath { name: name.clone() });
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
/// content steps' dialogue/load/sound/flag/path references, name-sorted for
/// determinism (`SceneFile::cutscenes` is a `HashMap`).
// Eight loosely-coupled accumulator sets (`report` + four cross-referenced
// registries), each already threaded independently through `check`/
// `check_maps`/`check_script` — bundling just this function's params into a
// struct would obscure the sibling functions' shape rather than clarify it.
#[allow(clippy::too_many_arguments)]
fn check_scenes(
    scenes: &SceneFile,
    script: &ScriptFile,
    maps: &BTreeMap<String, Vec<MapObject>>,
    presets: &Presets,
    report: &mut Report,
    referenced_dialogue: &mut BTreeSet<String>,
    set_flags: &mut BTreeSet<String>,
    referenced_paths: &mut BTreeSet<String>,
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
            check_content_step(step, name, script, scenes, report, referenced_dialogue, set_flags, referenced_paths);
        }
    }
}

/// One cutscene content step's cross-references — the per-step body of
/// [`check_scenes`]'s walk, factored out so it can also recurse into every
/// `on` handler's own content steps: a handler body gets exactly the same
/// checks as a top-level step (flags/sounds/loads/paths), plus — on the
/// `Dialogue` arm — a check of its own, that each handler's cue is one the
/// dialogue can actually reach (`Finding::SceneDanglingCue`). `Load` can't
/// actually appear inside a handler (parse-rejected — see the `.eggscene`
/// module doc), but this walker doesn't need to know that to stay correct.
#[allow(clippy::too_many_arguments)]
fn check_content_step(
    step: &CutsceneContent,
    cutscene: &str,
    script: &ScriptFile,
    scenes: &SceneFile,
    report: &mut Report,
    referenced_dialogue: &mut BTreeSet<String>,
    set_flags: &mut BTreeSet<String>,
    referenced_paths: &mut BTreeSet<String>,
) {
    match step {
        CutsceneContent::Dialogue { key, handlers } => {
            referenced_dialogue.insert(key.clone());
            match script.dialogue.get(key) {
                None => {
                    report.push(Finding::SceneDanglingDialogue {
                        cutscene: cutscene.to_string(),
                        key: key.clone(),
                    });
                }
                Some(def) => {
                    let cues = dialogue_cues(def);
                    for handler in handlers {
                        if !cues.contains(&handler.cue) {
                            report.push(Finding::SceneDanglingCue {
                                cutscene: cutscene.to_string(),
                                key: key.clone(),
                                cue: handler.cue.clone(),
                            });
                        }
                    }
                }
            }
            for handler in handlers {
                for sub in &handler.content {
                    check_content_step(
                        sub,
                        cutscene,
                        script,
                        scenes,
                        report,
                        referenced_dialogue,
                        set_flags,
                        referenced_paths,
                    );
                }
            }
        }
        CutsceneContent::Load(target) => {
            if scenes.get_cutscene(target).is_none() {
                report.push(Finding::SceneDanglingLoad { cutscene: cutscene.to_string(), name: target.clone() });
            }
        }
        CutsceneContent::Sound(sfx) => {
            if sound::by_name(sfx).is_none() {
                report.push(Finding::SceneDanglingSound { cutscene: cutscene.to_string(), name: sfx.clone() });
            }
        }
        CutsceneContent::SetFlag(flag, _) => {
            set_flags.insert(flag.clone());
            if !script.flags.contains(flag) {
                report.push(Finding::SceneDanglingFlag { cutscene: cutscene.to_string(), flag: flag.clone() });
            }
        }
        CutsceneContent::Move(chains) => {
            for chain in chains {
                for ins in &chain.instructions {
                    // `Motion::Pose` names live on presets, not this registry — and an
                    // actor's preset can be rebound at runtime (`find`/`bind`), so a
                    // pose name can't be cross-referenced statically. It falls through
                    // this `else continue` unchecked, like every non-`Path` motion.
                    let Motion::Path { name: path_name, .. } = &ins.motion else {
                        continue;
                    };
                    referenced_paths.insert(path_name.clone());
                    if !scenes.paths.contains_key(path_name) {
                        report.push(Finding::SceneDanglingPath {
                            cutscene: cutscene.to_string(),
                            name: path_name.clone(),
                        });
                    }
                }
            }
        }
        // `music` names are directory-scanned free-form at play time
        // (no fixed vocabulary to check against — see `data/tiled.rs`'s
        // `TiledMap::music` doc); actor names in `Move`/`Interact` are
        // scene-local bindings, not a global registry.
        CutsceneContent::Music(_)
        | CutsceneContent::Interact { .. }
        | CutsceneContent::Wait(_)
        | CutsceneContent::Camera(..)
        | CutsceneContent::Shake { .. } => {}
    }
}

/// Every `#cue NAME` a dialogue definition can reach, recursively through
/// `#if`/`#elif`/`#else` branches (both sides — a cue behind an untaken
/// branch is still a legitimate target: which branch runs depends on the
/// live save at playback, not on anything static). What
/// [`check_content_step`]'s `Dialogue` arm checks each `on` handler's cue
/// against.
fn dialogue_cues(def: &DialogueDef) -> BTreeSet<String> {
    let mut cues = BTreeSet::new();
    collect_cues_dialogue(def, &mut cues);
    cues
}

fn collect_cues_dialogue(def: &DialogueDef, out: &mut BTreeSet<String>) {
    match def {
        DialogueDef::Plain(entry) => collect_cues_entry(entry, out),
        DialogueDef::Segments { segments } => {
            for seg in segments {
                collect_cues_segment(seg, out);
            }
        }
    }
}

fn collect_cues_segment(seg: &SegmentDef, out: &mut BTreeSet<String>) {
    match seg {
        SegmentDef::Plain(entry) => collect_cues_entry(entry, out),
        SegmentDef::If { then, otherwise, elifs, .. } => {
            collect_cues_dialogue(then, out);
            if let Some(otherwise) = otherwise {
                collect_cues_dialogue(otherwise, out);
            }
            for elif in elifs {
                collect_cues_dialogue(&elif.then, out);
            }
        }
    }
}

fn collect_cues_entry(entry: &Entry, out: &mut BTreeSet<String>) {
    if let Entry::Conversation { messages } = entry {
        for message in messages {
            for content in &message.content {
                if let ContentDef::Cue(name) = content {
                    out.insert(name.clone());
                }
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
        // `Cue` names are free-form at parse time (unlike a `#flag`, which is
        // declared): cross-referencing them against a scene's `on` handlers
        // happens from the scene side instead — see `check_content_step`'s
        // `Dialogue` arm and `Finding::SceneDanglingCue`. A cue only means
        // something in the context of the one dialogue step subscribing to
        // it, which this per-dialogue-key walk has no visibility into.
        ContentDef::Text(_)
        | ContentDef::Auto(_)
        | ContentDef::Delayed(_, _)
        | ContentDef::Delay(_)
        | ContentDef::Portrait(None)
        | ContentDef::Pause
        | ContentDef::Flip(_)
        | ContentDef::Shake(_, _)
        | ContentDef::Speed(_, _)
        | ContentDef::Cue(_) => {}
    }
}

// --- language-overlay skeleton lint ---
//
// A translation is supposed to change only human-readable text — never the
// directives, branching, or choices around it, since those drive save flags
// and pacing the *base* author already decided once. [`check_overlay`] pins
// that: for every dialogue key an overlay shares with the base, the two
// entries' *skeletons* (everything but text payloads) must match exactly.

/// Cross-reference one language overlay against the base script it
/// translates: every dialogue key the overlay also defines in `base` must
/// keep the base entry's skeleton (see [`skeleton_diff_dialogue`]); a key the
/// overlay defines that `base` doesn't is an orphan (unreachable — a
/// dialogue key only ever resolves through the base's key set, see
/// [`crate::data::script::Script::get_dialogue`]); an overlay `#list` sharing
/// a name with a base list must have the same entry count (lists are read
/// back by index); and every overlay `#flag` must already be declared in
/// `base`. A key `base` defines that the overlay *doesn't* is not a
/// finding — that's the fallback the whole overlay mechanism exists for (see
/// [`crate::data::script::Script::set_language`]). `lang` is carried into
/// every [`Finding`] for identification, not looked up here — the caller
/// (a directory scan) owns the mapping from language name to `ScriptFile`.
pub fn check_overlay(base: &ScriptFile, overlay: &ScriptFile, lang: &str) -> Report {
    let mut report = Report::default();

    let mut keys: Vec<&String> = overlay.dialogue.keys().collect();
    keys.sort();
    for key in keys {
        match base.dialogue.get(key) {
            None => report.push(Finding::OverlayOrphanDialogue { lang: lang.to_string(), key: key.clone() }),
            Some(base_def) => {
                if let Some(path) = skeleton_diff_dialogue(base_def, &overlay.dialogue[key]) {
                    report.push(Finding::OverlaySkeletonMismatch { lang: lang.to_string(), key: key.clone(), path });
                }
            }
        }
    }

    let mut list_keys: Vec<&String> = overlay.lists.keys().collect();
    list_keys.sort();
    for key in list_keys {
        if let Some(base_list) = base.lists.get(key) {
            let overlay_list = &overlay.lists[key];
            if base_list.len() != overlay_list.len() {
                report.push(Finding::OverlayListLength {
                    lang: lang.to_string(),
                    key: key.clone(),
                    base_len: base_list.len(),
                    overlay_len: overlay_list.len(),
                });
            }
        }
    }

    for flag in &overlay.flags {
        if !base.flags.contains(flag) {
            report.push(Finding::OverlayUndeclaredFlag { lang: lang.to_string(), flag: flag.clone() });
        }
    }

    report
}

/// A [`SegmentDef::If`]/[`ElifDef`] condition's `#if`/`#elif [not] NAME`
/// spelling, for a mismatch message.
fn condition_label(flag: &str, negated: bool) -> String {
    if negated {
        format!("#if not {flag:?}")
    } else {
        format!("#if {flag:?}")
    }
}

/// The skeleton of a whole dialogue entry: identical branch structure
/// (`#if`/`#elif`/`#else` shape, flag names, negation) all the way down, with
/// every leaf [`Entry`] compared via [`skeleton_diff_entry`]. `None` means the
/// skeletons match; `Some(path)` names the first point of divergence, read
/// outside-in (e.g. `#if "a" then: message 2: directive 1: ...`).
fn skeleton_diff_dialogue(base: &DialogueDef, overlay: &DialogueDef) -> Option<String> {
    match (base, overlay) {
        (DialogueDef::Plain(b), DialogueDef::Plain(o)) => skeleton_diff_entry(b, o),
        (DialogueDef::Segments { segments: bs }, DialogueDef::Segments { segments: os }) => {
            if bs.len() != os.len() {
                return Some(format!("segment count differs ({} vs {})", bs.len(), os.len()));
            }
            bs.iter()
                .zip(os)
                .enumerate()
                .find_map(|(i, (b, o))| skeleton_diff_segment(b, o).map(|sub| format!("segment {}: {sub}", i + 1)))
        }
        (DialogueDef::Plain(_), DialogueDef::Segments { .. }) => {
            Some("branch structure differs (overlay adds #if branching the base doesn't have)".to_string())
        }
        (DialogueDef::Segments { .. }, DialogueDef::Plain(_)) => {
            Some("branch structure differs (base has #if branching the overlay doesn't have)".to_string())
        }
    }
}

/// One piece of a dialogue body's skeleton: an unconditional [`Entry`], or an
/// `#if`/`#elif`/`#else` chain's condition + each branch, recursively.
fn skeleton_diff_segment(base: &SegmentDef, overlay: &SegmentDef) -> Option<String> {
    match (base, overlay) {
        (SegmentDef::Plain(b), SegmentDef::Plain(o)) => skeleton_diff_entry(b, o),
        (
            SegmentDef::If { flag: bf, negated: bn, then: bt, otherwise: bo, elifs: be },
            SegmentDef::If { flag: of, negated: on, then: ot, otherwise: oo, elifs: oe },
        ) => {
            if bf != of || bn != on {
                return Some(format!(
                    "branch structure differs at {} (overlay has {})",
                    condition_label(bf, *bn),
                    condition_label(of, *on),
                ));
            }
            let label = condition_label(bf, *bn);
            if let Some(sub) = skeleton_diff_dialogue(bt, ot) {
                return Some(format!("{label} then: {sub}"));
            }
            match (bo, oo) {
                (None, None) => {}
                (Some(b), Some(o)) => {
                    if let Some(sub) = skeleton_diff_dialogue(b, o) {
                        return Some(format!("{label} else: {sub}"));
                    }
                }
                (Some(_), None) => return Some(format!("{label}: base has #else, overlay doesn't")),
                (None, Some(_)) => return Some(format!("{label}: overlay has #else, base doesn't")),
            }
            if be.len() != oe.len() {
                return Some(format!("{label}: elif count differs ({} vs {})", be.len(), oe.len()));
            }
            be.iter()
                .zip(oe)
                .enumerate()
                .find_map(|(i, (b, o))| skeleton_diff_elif(b, o).map(|sub| format!("{label} elif {}: {sub}", i + 1)))
        }
        (SegmentDef::Plain(_), SegmentDef::If { flag, negated, .. }) => {
            Some(format!("branch structure differs at {} (base has no matching #if)", condition_label(flag, *negated)))
        }
        (SegmentDef::If { flag, negated, .. }, SegmentDef::Plain(_)) => {
            Some(format!("branch structure differs at {} (overlay has no matching #if)", condition_label(flag, *negated)))
        }
    }
}

fn skeleton_diff_elif(base: &ElifDef, overlay: &ElifDef) -> Option<String> {
    if base.flag != overlay.flag || base.negated != overlay.negated {
        return Some(format!(
            "condition differs ({} vs {})",
            condition_label(&base.flag, base.negated).replace("#if", "#elif"),
            condition_label(&overlay.flag, overlay.negated).replace("#if", "#elif"),
        ));
    }
    skeleton_diff_dialogue(&base.then, &overlay.then)
}

/// The skeleton of a leaf [`Entry`]: its messages' directive sequences,
/// text stripped out. `Line`/`Pages`/`Conversation` are just three on-disk
/// shorthands for the same thing — a `Vec<MessageDef>` ([`entry_messages`]
/// expands the shorthands) — so a translator adding a directive that turns a
/// plain `Line` into a `Conversation` shows up as an ordinary directive-count
/// mismatch on message 1, not a spurious "shape" error; and the flat/scoped
/// `#if` authoring styles ([`crate::data::script::eggtext`]'s module doc)
/// parse to the exact same [`DialogueDef`] tree already, so they need no
/// special-casing here at all.
fn skeleton_diff_entry(base: &Entry, overlay: &Entry) -> Option<String> {
    let bm = entry_messages(base);
    let om = entry_messages(overlay);
    if bm.len() != om.len() {
        return Some(format!("page/message count differs ({} vs {})", bm.len(), om.len()));
    }
    bm.iter()
        .zip(&om)
        .enumerate()
        .find_map(|(i, (b, o))| skeleton_diff_message(b, o).map(|sub| format!("message {}: {sub}", i + 1)))
}

/// Expand an [`Entry`] to the `Vec<MessageDef>` it's shorthand for: `Line`/
/// `Pages` are exactly the plain-text, default-portrait/flip/pause message(s)
/// that the parser's `reduce_entry` (`eggtext.rs`) collapses to, so
/// re-inflating them here needs no registry and can't disagree with it.
fn entry_messages(entry: &Entry) -> Vec<MessageDef> {
    fn plain(text: &str) -> MessageDef {
        MessageDef {
            portrait: PortraitChange::Keep,
            flip: None,
            pause: true,
            content: vec![ContentDef::Text(text.to_string())],
        }
    }
    match entry {
        Entry::Line(s) => vec![plain(s)],
        Entry::Pages(pages) => pages.iter().map(|s| plain(s)).collect(),
        Entry::Conversation { messages } => messages.clone(),
    }
}

/// The skeleton of one message: portrait/flip/pause state, then each content
/// item's directive kind + non-text arguments in order.
fn skeleton_diff_message(base: &MessageDef, overlay: &MessageDef) -> Option<String> {
    if base.portrait != overlay.portrait {
        return Some(format!(
            "#pic {} vs {}",
            portrait_change_label(&base.portrait),
            portrait_change_label(&overlay.portrait),
        ));
    }
    if base.flip != overlay.flip {
        return Some(format!("#flip {:?} vs {:?}", base.flip, overlay.flip));
    }
    if base.pause != overlay.pause {
        return Some(format!("pause {} vs {} (#nopause)", base.pause, overlay.pause));
    }
    if base.content.len() != overlay.content.len() {
        return Some(format!("directive count differs ({} vs {})", base.content.len(), overlay.content.len()));
    }
    base.content
        .iter()
        .zip(&overlay.content)
        .enumerate()
        .find_map(|(i, (b, o))| skeleton_diff_content(b, o).map(|sub| format!("directive {}: {sub}", i + 1)))
}

fn portrait_change_label(p: &PortraitChange) -> String {
    match p {
        PortraitChange::Keep => "keep".to_string(),
        PortraitChange::Clear => "none".to_string(),
        PortraitChange::Set(name) => format!("{name:?}"),
    }
}

fn portrait_label(p: &Option<String>) -> String {
    match p {
        Some(name) => format!("{name:?}"),
        None => "none".to_string(),
    }
}

/// One content item's skeleton: same directive kind, same non-text argument
/// values (text/label strings are exactly what's allowed to differ).
fn skeleton_diff_content(base: &ContentDef, overlay: &ContentDef) -> Option<String> {
    match (base, overlay) {
        (ContentDef::Text(_), ContentDef::Text(_)) | (ContentDef::Auto(_), ContentDef::Auto(_)) => None,
        (ContentDef::Delayed(_, bd), ContentDef::Delayed(_, od)) => {
            (bd != od).then(|| format!("#delay {bd} vs {od}"))
        }
        (ContentDef::Delay(bd), ContentDef::Delay(od)) => (bd != od).then(|| format!("#delay {bd} vs {od}")),
        (ContentDef::Sound(bn), ContentDef::Sound(on)) => {
            (bn != on).then(|| format!("#sound {bn:?} vs {on:?}"))
        }
        (ContentDef::Portrait(bp), ContentDef::Portrait(op)) => (bp != op)
            .then(|| format!("#pic {} vs {}", portrait_label(bp), portrait_label(op))),
        (ContentDef::Pause, ContentDef::Pause) => None,
        (ContentDef::Flip(bf), ContentDef::Flip(of)) => (bf != of).then(|| format!("#flip {bf} vs {of}")),
        (ContentDef::SetFlag(bn, bv), ContentDef::SetFlag(on, ov)) => {
            (bn != on || bv != ov).then(|| format!("#set {bn:?} {bv} vs #set {on:?} {ov}"))
        }
        (ContentDef::Shake(bf, ba), ContentDef::Shake(of, oa)) => {
            (bf != of || ba != oa).then(|| format!("#shake {bf} {ba} vs #shake {of} {oa}"))
        }
        (ContentDef::Speed(bc, bf), ContentDef::Speed(oc, of)) => {
            ((bc, bf) != (oc, of)).then(|| format!("#speed {bc}/{bf} vs {oc}/{of}"))
        }
        // A cue is structure, not text: it's a beat name the scene
        // choreography subscribes to by exact spelling, so a translation may
        // not rename, drop, or reorder one.
        (ContentDef::Cue(bn), ContentDef::Cue(on)) => {
            (bn != on).then(|| format!("#cue {bn:?} vs {on:?}"))
        }
        (ContentDef::Choice(bo), ContentDef::Choice(oo)) => skeleton_diff_choice(bo, oo),
        (b, o) => Some(format!("{} vs {}", content_kind(b), content_kind(o))),
    }
}

/// A short label for a [`ContentDef`]'s kind, used only when two different
/// kinds land in the same slot (same-kind mismatches report their own
/// arguments instead — see [`skeleton_diff_content`]).
fn content_kind(c: &ContentDef) -> &'static str {
    match c {
        ContentDef::Text(_) => "text",
        ContentDef::Auto(_) => "auto text",
        ContentDef::Delayed(_, _) => "#delay text",
        ContentDef::Delay(_) => "#delay",
        ContentDef::Sound(_) => "#sound",
        ContentDef::Portrait(_) => "#pic",
        ContentDef::Pause => "pause",
        ContentDef::Flip(_) => "#flip",
        ContentDef::SetFlag(..) => "#set",
        ContentDef::Shake(..) => "#shake",
        ContentDef::Choice(_) => "#choice",
        ContentDef::Speed(..) => "#speed",
        ContentDef::Cue(..) => "#cue",
    }
}

/// A `#choice` block's skeleton: same option count, each option's `sets` in
/// the same order (option label text is free).
fn skeleton_diff_choice(base: &[ChoiceOptionDef], overlay: &[ChoiceOptionDef]) -> Option<String> {
    if base.len() != overlay.len() {
        return Some(format!("#choice option count differs ({} vs {})", base.len(), overlay.len()));
    }
    base.iter().zip(overlay).enumerate().find_map(|(i, (b, o))| {
        (b.sets != o.sets)
            .then(|| format!("#choice option {}: sets {:?} vs {:?}", i + 1, b.sets, o.sets))
    })
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

    /// A `path` motion naming an unknown `#path` block is an error; naming a
    /// real one is clean and (whether or not it resolves) counts the path as
    /// referenced, so a defined-and-referenced path never also shows up as
    /// dead weight.
    #[test]
    fn scene_dangling_path_is_an_error() {
        use crate::data::scene;
        let scenes = scene::parse(
            "#cutscene a\n\
             \x20   move\n\
             \x20       player: path real; path nope",
        )
        .expect("parse scene");
        let mut scenes_with_path = scenes.clone();
        scenes_with_path.paths.insert("real".to_string(), vec![((1, 0), 4)]);

        let report = check(
            &ScriptFile::default(),
            &scenes_with_path,
            &maps(vec![]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
        assert!(matches!(&report.errors[0], Finding::SceneDanglingPath { name, .. } if name == "nope"));
        assert!(
            report.warnings.iter().all(|w| !matches!(w, Finding::UnreferencedPath { .. })),
            "the referenced `real` path must not also show up as dead weight: {:?}",
            report.warnings,
        );
    }

    /// A `dialogue` step's `on NAME [wait]` handler naming a cue its
    /// dialogue never reaches (no `#cue NAME` anywhere in it) is an error.
    #[test]
    fn scene_dangling_cue_handler_is_an_error() {
        use crate::data::scene;
        let script = script("#dialogue talk\n    Hi.");
        let scenes = scene::parse(
            "#cutscene a\n\
             \x20   dialogue talk\n\
             \x20       on missing_cue\n\
             \x20           wait 1",
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
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
        assert!(matches!(
            &report.errors[0],
            Finding::SceneDanglingCue { cue, key, .. } if cue == "missing_cue" && key == "talk"
        ));
    }

    /// A handler's cue is clean as long as the dialogue reaches it
    /// *anywhere* — including behind an `#if not` branch: which branch runs
    /// depends on the live save at playback, not on anything static, so a
    /// cue behind an untaken branch is still a legitimate target.
    #[test]
    fn scene_cue_reachable_inside_an_if_branch_is_not_dangling() {
        use crate::data::scene;
        let script = script(
            "#flag seen\n\
             #dialogue talk\n\
             \x20   #if not seen\n\
             \x20   #cue arrive\n\
             \x20   Hi.\n\
             \x20   #else\n\
             \x20   Bye.\n\
             \x20   #end",
        );
        let scenes = scene::parse(
            "#cutscene a\n\
             \x20   dialogue talk\n\
             \x20       on arrive\n\
             \x20           wait 1",
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
    }

    /// A `#cue` no `on` handler names is fine — not a finding. It may be a
    /// stage direction, or a beat for a different scene.
    #[test]
    fn unhandled_cue_is_not_a_finding() {
        use crate::data::scene;
        let script = script("#dialogue talk\n    #cue unhandled\n    Hi.");
        let scenes = scene::parse("#cutscene a\n    dialogue talk").expect("parse scene");
        let report = check(
            &script,
            &scenes,
            &maps(vec![]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert!(report.is_clean(), "{:?}", report.errors);
        assert!(
            report.warnings.is_empty(),
            "an unhandled cue is not dead weight either: {:?}",
            report.warnings,
        );
    }

    /// A bad sound / undeclared flag inside a handler's own body is caught,
    /// same as at the top level — the recursive walk reaches handler content.
    #[test]
    fn bad_sound_and_flag_inside_a_handler_body_is_caught() {
        use crate::data::scene;
        let script = script("#dialogue talk\n    #cue go\n    Hi.");
        let scenes = scene::parse(
            "#cutscene a\n\
             \x20   dialogue talk\n\
             \x20       on go\n\
             \x20           sound nope\n\
             \x20           set undeclared true",
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
        assert_eq!(report.errors.len(), 2, "{:?}", report.errors);
        assert!(report.errors.iter().any(|e| matches!(e, Finding::SceneDanglingSound { name, .. } if name == "nope")));
        assert!(report.errors.iter().any(|e| matches!(e, Finding::SceneDanglingFlag { flag, .. } if flag == "undeclared")));
    }

    /// A `#path` block no scene references is dead weight — a warning, not an
    /// error.
    #[test]
    fn unreferenced_path_is_a_warning() {
        use crate::data::scene;
        let mut scenes = scene::SceneFile::default();
        scenes.paths.insert("orphan".to_string(), vec![((1, 0), 4)]);

        let report = check(
            &ScriptFile::default(),
            &scenes,
            &maps(vec![]),
            &Portraits::builtin(),
            &Presets::builtin(),
            ENGINE_DIALOGUE_ROOTS,
        );
        assert!(report.is_clean(), "{:?}", report.errors);
        assert_eq!(report.warnings.len(), 1);
        assert!(matches!(&report.warnings[0], Finding::UnreferencedPath { name } if name == "orphan"));
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

    // --- wave 5: language-overlay skeleton lint ---

    /// A matching skeleton with only text changed is clean — the whole point
    /// of the lint (an honest translation) must never trip it.
    #[test]
    fn overlay_matching_skeleton_different_text_is_clean() {
        let base = script("#dialogue d\n    #pic p\n    Hello.\n\n    Bye.");
        let overlay = script("#dialogue d\n    #pic p\n    Hola.\n\n    Adios.");
        let report = check_overlay(&base, &overlay, "es");
        assert!(report.is_clean(), "{:?}", report.errors);
    }

    /// A base key an overlay simply doesn't define is the fallback the whole
    /// overlay mechanism exists for ([`super::script::Script::get_dialogue`])
    /// — not a finding.
    #[test]
    fn overlay_missing_key_is_silent() {
        let base = script("#dialogue d\n    Hi.\n#dialogue untranslated\n    Only in English.");
        let overlay = script("#dialogue d\n    Hola.");
        let report = check_overlay(&base, &overlay, "es");
        assert!(report.is_clean(), "{:?}", report.errors);
    }

    /// A key the overlay defines that the base doesn't is an orphan: it can
    /// never be reached, since lookup only ever resolves through the base's
    /// key set.
    #[test]
    fn overlay_orphan_key_is_an_error() {
        let base = script("#dialogue d\n    Hi.");
        let overlay = script("#dialogue d\n    Hola.\n#dialogue extra\n    Solo en overlay.");
        let report = check_overlay(&base, &overlay, "es");
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
        assert!(matches!(&report.errors[0], Finding::OverlayOrphanDialogue { key, .. } if key == "extra"));
    }

    /// A directive's argument changing (a portrait swapped for a different
    /// one) is a skeleton mismatch even though both sides are syntactically
    /// valid `#pic` directives.
    #[test]
    fn overlay_directive_arg_change_is_an_error() {
        let base = script("#dialogue d\n    #pic a_normal\n    Hi.");
        let overlay = script("#dialogue d\n    #pic b_open\n    Hola.");
        let report = check_overlay(&base, &overlay, "es");
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
        match &report.errors[0] {
            Finding::OverlaySkeletonMismatch { key, path, .. } => {
                assert_eq!(key, "d");
                assert!(path.contains("a_normal") && path.contains("b_open"), "{path}");
            }
            other => panic!("expected OverlaySkeletonMismatch, got {other:?}"),
        }
    }

    /// A `#cue` is structure, not text: renaming it in a translation is a
    /// skeleton mismatch, exactly like swapping a `#pic`'s portrait — the
    /// scene choreography subscribes to the base's exact cue names.
    #[test]
    fn overlay_cue_rename_is_an_error() {
        let base = script("#dialogue d\n    #cue arrive\n    Hi.");
        let overlay = script("#dialogue d\n    #cue llegada\n    Hola.");
        let report = check_overlay(&base, &overlay, "es");
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
        match &report.errors[0] {
            Finding::OverlaySkeletonMismatch { key, path, .. } => {
                assert_eq!(key, "d");
                assert!(path.contains("arrive") && path.contains("llegada"), "{path}");
            }
            other => panic!("expected OverlaySkeletonMismatch, got {other:?}"),
        }
    }

    /// A directive present in the base but dropped in the overlay is a
    /// mismatch — a translator can't silently drop a `#sound`.
    #[test]
    fn overlay_missing_directive_is_an_error() {
        let base = script("#dialogue d\n    #sound gain\n    Hi.");
        let overlay = script("#dialogue d\n    Hola.");
        let report = check_overlay(&base, &overlay, "es");
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
        match &report.errors[0] {
            Finding::OverlaySkeletonMismatch { path, .. } => {
                assert!(path.contains("directive count differs"), "{path}");
            }
            other => panic!("expected OverlaySkeletonMismatch, got {other:?}"),
        }
    }

    /// Dropping (or adding) `#if` branching entirely changes the entry's
    /// structure, not just its words.
    #[test]
    fn overlay_branch_structure_change_is_an_error() {
        let base = script(
            "#flag seen\n\
             #dialogue d\n\
             \x20   #if seen\n\
             \x20   After.\n\
             \x20   #else\n\
             \x20   Before.\n\
             \x20   #end",
        );
        let overlay = script("#dialogue d\n    Siempre igual.");
        let report = check_overlay(&base, &overlay, "es");
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
        match &report.errors[0] {
            Finding::OverlaySkeletonMismatch { path, .. } => {
                assert!(path.contains("branch structure differs"), "{path}");
            }
            other => panic!("expected OverlaySkeletonMismatch, got {other:?}"),
        }
    }

    /// `#choice` needing the same option COUNT: dropping an option changes
    /// what the player can pick, not just its label.
    #[test]
    fn overlay_choice_option_count_change_is_an_error() {
        let base = script(
            "#flag chose_tea\n\
             #dialogue ask\n\
             \x20   Tea, coffee, or water?\n\
             \x20   #choice\n\
             \x20   #option Tea\n\
             \x20   #set chose_tea true\n\
             \x20   #option Coffee\n\
             \x20   #option Water",
        );
        let overlay = script(
            "#flag chose_tea\n\
             #dialogue ask\n\
             \x20   ¿Te o cafe?\n\
             \x20   #choice\n\
             \x20   #option Te\n\
             \x20   #set chose_tea true\n\
             \x20   #option Cafe",
        );
        let report = check_overlay(&base, &overlay, "es");
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
        match &report.errors[0] {
            Finding::OverlaySkeletonMismatch { path, .. } => {
                assert!(path.contains("option count"), "{path}");
            }
            other => panic!("expected OverlaySkeletonMismatch, got {other:?}"),
        }
    }

    /// Same option count, but an option's `sets` (the flag it writes) has
    /// changed underneath a translated label — the label may change freely,
    /// the flag it wires may not.
    #[test]
    fn overlay_choice_option_sets_change_is_an_error() {
        let base = script(
            "#flag chose_tea\n\
             #flag chose_coffee\n\
             #dialogue ask\n\
             \x20   Tea or coffee?\n\
             \x20   #choice\n\
             \x20   #option Tea\n\
             \x20   #set chose_tea true\n\
             \x20   #option Coffee",
        );
        let overlay = script(
            "#flag chose_tea\n\
             #flag chose_coffee\n\
             #dialogue ask\n\
             \x20   ¿Te o cafe?\n\
             \x20   #choice\n\
             \x20   #option Te\n\
             \x20   #set chose_coffee true\n\
             \x20   #option Cafe",
        );
        let report = check_overlay(&base, &overlay, "es");
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
        match &report.errors[0] {
            Finding::OverlaySkeletonMismatch { path, .. } => {
                assert!(path.contains("sets"), "{path}");
            }
            other => panic!("expected OverlaySkeletonMismatch, got {other:?}"),
        }
    }

    /// A `#list`'s entries are read back by index, so an overlay list with a
    /// different entry count than the base's same-named list is an error.
    #[test]
    fn overlay_list_length_mismatch_is_an_error() {
        let base = script("#list things\n    one\n    two\n    three");
        let overlay = script("#list things\n    uno\n    dos");
        let report = check_overlay(&base, &overlay, "es");
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
        assert!(matches!(
            &report.errors[0],
            Finding::OverlayListLength { key, base_len: 3, overlay_len: 2, .. } if key == "things"
        ));
    }

    /// An overlay `#flag` the base never declares is an error — the base is
    /// the sole authority on the flag vocabulary.
    #[test]
    fn overlay_undeclared_flag_is_an_error() {
        let base = script("#dialogue d\n    Hi.");
        let overlay = script("#flag only_in_overlay\n#dialogue d\n    Hola.");
        let report = check_overlay(&base, &overlay, "es");
        assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
        assert!(matches!(
            &report.errors[0],
            Finding::OverlayUndeclaredFlag { flag, .. } if flag == "only_in_overlay"
        ));
    }

    /// The flat and scoped `#if` authoring styles (see the `eggtext` module
    /// doc's Conditionals section) parse to the exact same `DialogueDef`
    /// tree, so an overlay reindenting flat -> scoped (or vice versa) with
    /// otherwise identical structure must pass: the lint walks parsed defs,
    /// not surface syntax, so it can't tell the two styles apart in the
    /// first place.
    #[test]
    fn overlay_flat_and_scoped_if_styles_are_equivalent() {
        let base_flat = script(
            "#flag INSULT\n\
             #dialogue d\n\
             \x20   #pic m_close\n\
             \x20   ... Hmm...\n\n\
             \x20   #if INSULT\n\
             \x20   #pic m_narrow\n\
             \x20   ... Low hanging fruit.\n\
             \x20   #else\n\
             \x20   #pic m_normal\n\
             \x20   Hey, it isn't all that bad.\n\
             \x20   #end",
        );
        let overlay_scoped = script(
            "#flag INSULT\n\
             #dialogue d\n\
             \x20   #pic m_close\n\
             \x20   ... Eh...\n\n\
             \x20   #if INSULT\n\
             \x20       #pic m_narrow\n\
             \x20       ... Fruta al alcance de la mano.\n\
             \x20   #else\n\
             \x20       #pic m_normal\n\
             \x20       Oye, no es tan malo.",
        );
        let report = check_overlay(&base_flat, &overlay_scoped, "es");
        assert!(report.is_clean(), "{:?}", report.errors);

        // And the reverse direction — a "base" authored scoped, an overlay
        // authored flat — is equally clean, confirming neither style is
        // treated as canonical.
        let report = check_overlay(&overlay_scoped, &base_flat, "es");
        assert!(report.is_clean(), "{:?}", report.errors);
    }
}

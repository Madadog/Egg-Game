//! Game data — the runtime `data.toml` file (plain TOML): the **item registry**
//! and the creature **[`PresetDef`] definitions**. Language-invariant data, the
//! way maps and cutscenes are (names/descriptions stay in the script as
//! `item_<key>` lists) — loaded from `assets/data/` at startup the way the script
//! and maps are, rather than baked into Rust.
//!
//! ## Why this shape
//! A preset's `walk` **is** the runtime [`WalkSprites`](crate::world::player::WalkSprites):
//! a preset deserialises straight into it — the flattened 9-cell grid of per-frame
//! [`SpriteOptions`](egg_render::SpriteOptions) plus its facing policy, in full.
//! There is no shorthand "pattern" layer between the file and the runtime; what
//! ships is exactly what the game reads. The grid is verbose (defaulted frame
//! fields are elided, but nine cells is nine cells), which is the deliberate
//! trade: this format is GUI-emitted and the transparency was chosen over terse
//! hand-authoring.
//!
//! ## Status
//! Both [`items`](DataFile::items) and [`presets`](DataFile::presets) are the live
//! source ([`GameItems::from_data`] / [`Presets::from_data`], installed at boot by
//! `EggState::load_data`). The embedded shipped file
//! is the built-in default ([`Presets::builtin`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::data::portraits::Portrait;
use crate::data::sound::SfxDef;
use egg_render::geometry::Hitbox;
use crate::world::player::{
    CreatureState, MoveMode, PresetId, Shell, ShellSprites, SpriteAnimation, Timer, WalkSprites,
};

/// Where the host stores the game-data file, resolved under the asset root
/// (`assets/data/data.toml`) the same way [`SAVE_PATH`](crate::data::save::SAVE_PATH)
/// and the script/map paths are.
pub const DATA_PATH: &str = "data/data.toml";

/// The authored use-effect of an item: what placing it on the bag's Use button
/// fires. Deliberately the data-file spelling of [`Interaction`](crate::world::interact::Interaction)
/// — resolved to one at fire time (an `Interaction` holds runtime payloads that
/// don't belong in TOML).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UseDef {
    /// A dialogue-registry key. `use = { dialogue = "..." }`
    Dialogue(String),
    /// A cutscene-registry name. `use = { cutscene = "..." }`
    Cutscene(String),
    /// An [`InteractFn`](crate::world::interact::InteractFn) `func` name (same
    /// vocabulary as `.tmj` objects: "toggle_dog", …). `use = { func = "..." }`
    Func(String),
}

/// The fixed, gameplay-relevant data for one item — which sprite draws it and
/// (optionally) what using it does. Its display name and description are NOT
/// here: those are text, so they live in the script (the `item_<key>` list, read
/// via `Ctx::item_name` / `Ctx::item_desc`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemDef {
    pub sprite: i32,
    /// The effect placing this item on the bag's Use button fires. Absent (the
    /// default) ⇒ the item has no use (a deny buzz, and it stays held). The TOML
    /// key is `use`; `on_use` is the field name because `use` is a Rust keyword.
    #[serde(default, rename = "use", skip_serializing_if = "Option::is_none")]
    pub on_use: Option<UseDef>,
}

/// The registry of every item the game knows about, keyed by the persistent
/// string id a save stores (and an [`InteractFn`](crate::world::interact::InteractFn)
/// names). Loaded game data, threaded through `Ctx::items`
/// like `maps`/`script`/`scenes`.
///
/// The shipped item set is loaded from `assets/data/data.toml` at boot (see
/// [`from_data`](Self::from_data) and `EggState::load_data`),
/// the way maps/script/scenes moved out to their own files. [`Default`] is the
/// built-in fallback for a missing/garbage file (and for headless/test use).
#[derive(Debug, Clone)]
pub struct GameItems {
    items: std::collections::HashMap<String, ItemDef>,
}
impl GameItems {
    pub fn new() -> Self {
        Self {
            items: std::collections::HashMap::new(),
        }
    }
    /// Build the registry from parsed `data.toml` items — the loaded source that
    /// supersedes [`Default`] once the host installs it at boot.
    pub fn from_data(items: &std::collections::BTreeMap<String, ItemDef>) -> Self {
        Self {
            items: items.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        }
    }
    pub fn add(&mut self, key: &str, sprite: i32) -> &mut Self {
        self.items.insert(
            key.to_string(),
            ItemDef {
                sprite,
                on_use: None,
            },
        );
        self
    }
    pub fn get(&self, key: &str) -> Option<&ItemDef> {
        self.items.get(key)
    }
    pub fn contains(&self, key: &str) -> bool {
        self.items.contains_key(key)
    }
}
impl Default for GameItems {
    fn default() -> Self {
        let mut i = Self::new();
        i.add("ff", 513).add("lm", 514).add("chegg", 524);
        i
    }
}

/// A parsed `data.toml` file: the whole game's language-invariant data. Every
/// section defaults to empty, so a file may carry only `[items]` (as the shipped
/// one once did) or only `[presets]`, and an absent section is simply empty.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DataFile {
    /// The item registry, keyed by the persistent item id a save stores
    /// (`"ff"`, `"lm"`, …). Built into [`GameItems`].
    #[serde(default)]
    pub items: BTreeMap<String, ItemDef>,
    /// Creature presets, keyed by the [`PresetId`](crate::world::player::PresetId)
    /// name a save stores (`"ellie"`, `"critter"`, …). The live runtime source:
    /// `EggState::load_data` derives the runtime
    /// [`Presets`] store from these.
    #[serde(default)]
    pub presets: BTreeMap<String, PresetDef>,
    /// Sound effects, keyed by the canonical name the engine and script name
    /// them by (`"pop"`, `"door"`, …). Built into the [`Sounds`](crate::data::sound::Sounds)
    /// store; the file stem + note/octave a sound plays at.
    #[serde(default)]
    pub sfx: BTreeMap<String, SfxDef>,
    /// Dialogue portraits, keyed by the script name a message names
    /// (`"y_normal"`, `"horror"`, …). Built into the
    /// [`Portraits`](crate::data::portraits::Portraits) store.
    #[serde(default)]
    pub portraits: BTreeMap<String, Portrait>,
}

/// Parse a `data.toml` document. A malformed file is the caller's to tolerate
/// (the engine logs and falls back to its built-in defaults — garbage tolerance,
/// like the save).
pub fn parse(src: &str) -> Result<DataFile, toml::de::Error> {
    toml::from_str(src)
}

/// Serialise a [`DataFile`] back to pretty TOML — the form an authoring tool (or
/// a one-off "dump the built-ins" helper) writes.
pub fn to_toml(data: &DataFile) -> Result<String, toml::ser::Error> {
    toml::to_string_pretty(data)
}

/// The runtime creature registry: every [`PresetDef`] keyed by its
/// [`PresetId`]. Built from the embedded `data.toml` ([`builtin`](Self::builtin))
/// and re-derived from the runtime file at boot ([`from_data`](Self::from_data)),
/// then threaded through gameplay as `Ctx::presets` the
/// way `items`/`maps`/`script` are. The lookup is the `presets[id]` the spawn
/// sites want; an absent id is a clean `None`, not a panic.
#[derive(Debug, Clone)]
pub struct Presets {
    defs: std::collections::HashMap<PresetId, PresetDef>,
}
impl Presets {
    /// The built-ins: the shipped `data.toml`, embedded at compile time, so the
    /// registry is never empty (web/headless/missing runtime file) and the file
    /// is the single source of the creature definitions. Panics only if the
    /// *shipped* file is malformed — a build-time-checked invariant.
    pub fn builtin() -> Self {
        let file = parse(include_str!("../../../../assets/data/data.toml"))
            .expect("shipped data.toml parses");
        Self::from_data(&file)
    }
    /// Build from a parsed [`DataFile`]'s presets, keyed by [`PresetId`]. The file
    /// is authoritative: an id it omits is absent here (the data-driven set), with
    /// [`builtin`](Self::builtin) standing in only when there is no runtime file.
    pub fn from_data(file: &DataFile) -> Self {
        Self {
            defs: file
                .presets
                .iter()
                .map(|(name, def)| (PresetId::new(name), def.clone()))
                .collect(),
        }
    }
    /// The definition filed under `id`, or `None` if the data doesn't define it.
    pub fn get(&self, id: &PresetId) -> Option<&PresetDef> {
        self.defs.get(id)
    }
    /// Spawn a fresh [`Shell`] of `id` — the `presets[id]` lookup — or `None` for
    /// an unknown id. The caller decides what an unknown id means (log + fall
    /// back, or skip).
    pub fn spawn(&self, id: &PresetId) -> Option<Shell> {
        self.get(id).map(|def| def.build_shell(id))
    }
    /// Every preset as `(name, def)`, name-sorted — the walk-sprite editor's
    /// listing + edit snapshot (pushed into the editor each frame, the way the
    /// cutscene names are).
    pub fn named_defs(&self) -> Vec<(String, PresetDef)> {
        let mut v: Vec<(String, PresetDef)> = self
            .defs
            .iter()
            .map(|(id, def)| (id.as_str().to_string(), def.clone()))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }
}

/// The compile-time-embedded shipped `data.toml` source — the same bytes
/// [`Presets::builtin`] parses. The walk-sprite editor splices into this when
/// the host store has no runtime copy yet, so its first save still writes a
/// complete file rather than a fragment.
pub fn shipped_source() -> &'static str {
    include_str!("../../../../assets/data/data.toml")
}

/// Emit one preset as TOML — the `[presets.<name>]` header plus all its
/// sub-tables — for the walk-sprite editor to splice into `data.toml` on save
/// (see [`splice_preset`]).
pub fn emit_preset(name: &str, def: &PresetDef) -> Result<String, toml::ser::Error> {
    #[derive(Serialize)]
    struct One<'a> {
        presets: std::collections::BTreeMap<&'a str, &'a PresetDef>,
    }
    let one = One {
        presets: [(name, def)].into_iter().collect(),
    };
    toml::to_string_pretty(&one)
}

/// Replace `name`'s `[presets.<name>]` span in the raw `data.toml` text with
/// `emitted` (from [`emit_preset`]), leaving every other byte — crucially the
/// file's comments, which a whole-file re-serialise would destroy — untouched.
/// The span runs from the preset's first header line to the last line before
/// the next section that isn't the preset's own, minus any trailing blank or
/// comment lines (those are the *next* section's banner, not this preset's).
/// A name the file doesn't define appends at the end instead. Assumes the bare
/// `presets.<ident>` header spelling the file (and the GUI) uses.
pub fn splice_preset(src: &str, name: &str, emitted: &str) -> String {
    let owned = |line: &str| {
        let t = line.trim_start();
        let Some(h) = t.strip_prefix("[[").or_else(|| t.strip_prefix('[')) else {
            return false;
        };
        let Some(rest) = h.strip_prefix("presets.") else {
            return false;
        };
        let Some(rest) = rest.strip_prefix(name) else {
            return false;
        };
        rest.starts_with(']') || rest.starts_with('.')
    };
    let is_header = |line: &str| line.trim_start().starts_with('[');
    let lines: Vec<&str> = src.split_inclusive('\n').collect();

    let Some(start) = lines.iter().position(|l| owned(l)) else {
        // Unknown preset: append, separated by one blank line.
        let mut out = src.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(emitted);
        return out;
    };
    // The first section header after `start` that isn't ours ends the span...
    let mut stop = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if is_header(line) && !owned(line) {
            stop = i;
            break;
        }
    }
    // ...minus the trailing blank/comment run (the next section's banner).
    while stop > start + 1 {
        let t = lines[stop - 1].trim();
        if t.is_empty() || t.starts_with('#') {
            stop -= 1;
        } else {
            break;
        }
    }

    let mut out = String::with_capacity(src.len() + emitted.len());
    lines[..start].iter().for_each(|l| out.push_str(l));
    out.push_str(emitted);
    if !emitted.ends_with('\n') {
        out.push('\n');
    }
    lines[stop..].iter().for_each(|l| out.push_str(l));
    out
}

/// One creature archetype: its collision box, wander behaviour, the extra
/// (non-walk) animations, and its walk grid. Mirrors a `Shell::<preset>()`
/// constructor in [`crate::world::player`].
///
/// Field order matters for TOML serialisation: the scalar/array values
/// (`hitbox`, `move_mode`) come before the sub-table fields (`others`, `walk`,
/// `poses`), since TOML forbids a bare key after a table within the same table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetDef {
    /// `[x, y, w, h]` for [`Hitbox::new`] — the shell's local (un-offset) box.
    pub hitbox: [i16; 4],
    /// The wander behaviour the preset spawns with (most are `wander`; the
    /// critter `amble`s). Absent ⇒ [`PresetMove::Wander`].
    #[serde(default, skip_serializing_if = "PresetMove::is_default")]
    pub move_mode: PresetMove,
    /// Non-walk animations (today just the petting sprite), as a single sprite
    /// strip — the `other_ids` of [`ShellSprites`](crate::world::player::ShellSprites).
    pub others: SpriteSet,
    /// The eight-heading walk grid, deserialised straight into the runtime
    /// [`WalkSprites`] — the nine cells and facing policy in full, no shorthand.
    pub walk: WalkSprites,
    /// Named standing-pose sprite strips a cutscene's `pose NAME` motion (see
    /// [`crate::data::scene::Motion::Pose`]) can put on a shell of this preset —
    /// e.g. `poses.slump`. Absent by default (no preset ships one yet; the art
    /// is a later, user-driven pass), so an existing `data.toml` with no
    /// `poses` table parses *and* re-serialises byte-identically.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub poses: BTreeMap<String, SpriteSet>,
}

impl PresetDef {
    /// The collision box this preset spawns with.
    pub fn hitbox(&self) -> Hitbox {
        let [x, y, w, h] = self.hitbox;
        // Sanitise inputs, avoid `Hitbox::new` assert panic.
        Hitbox::new(x, y, w.max(1), h.max(1))
    }
    /// The full [`ShellSprites`] for this preset — the walk grid (cloned straight
    /// from the deserialised [`WalkSprites`]) plus the `others` strip and every
    /// named `poses` strip, each built the same way.
    pub fn build_sprites(&self) -> ShellSprites {
        ShellSprites {
            walk: self.walk.clone(),
            others: vec![self.others.build()],
            poses: self.poses.iter().map(|(name, set)| (name.clone(), set.build())).collect(),
        }
    }
    /// The [`MoveMode`] this preset spawns with.
    pub fn move_mode(&self) -> MoveMode {
        self.move_mode.build()
    }
    /// Spawn a [`Shell`] of this preset, stamped with `id`. The store's
    /// [`Presets::spawn`] funnel.
    pub fn build_shell(&self, id: &PresetId) -> Shell {
        Shell::from_parts(id.clone(), self.hitbox(), self.build_sprites(), self.move_mode())
    }
}

/// A run of sprite ids drawn at a fixed cell size — the `(ids, w, h)` triple the
/// [`ShellSprites`](crate::world::player::ShellSprites) `others` strip is built from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpriteSet {
    pub ids: Vec<i32>,
    pub w: i32,
    pub h: i32,
    /// The strip's authored default facing. Composed with the directional mirror
    /// where it's drawn (the pet pose), so a strip whose sheet cells face the
    /// "wrong" way — e.g. the dog's `others` vs the player's — corrects itself in
    /// data without inverting the other.
    #[serde(default, skip_serializing_if = "egg_render::Flip::is_none")]
    pub flip: egg_render::Flip,
}
impl SpriteSet {
    fn build(&self) -> SpriteAnimation {
        // `SpriteAnimation::get_frame` indexes its frames unconditionally, so an
        // empty strip (a `data.toml` typo) would panic at draw time. Fall back to
        // a single frame so the animation is always non-empty — a missing strip
        // degrades to a visible placeholder sprite rather than a crash.
        let fallback = [0i32];
        let ids = if self.ids.is_empty() {
            &fallback[..]
        } else {
            &self.ids[..]
        };
        SpriteAnimation::from_sprite_ids(ids, self.w, self.h).with_flip(self.flip.clone())
    }
}

/// The wander behaviour a preset spawns with — the data form of the relevant
/// [`MoveMode`](crate::world::player::MoveMode) variants. (`Egg`/`Player` aren't preset
/// spawn states, so they're not here.)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetMove {
    /// Memoryless NPC wander ([`MoveMode::Wander`]).
    #[default]
    Wander,
    /// Dwell-then-walk critter gait ([`MoveMode::Amble`]).
    Amble,
}
impl PresetMove {
    fn is_default(&self) -> bool {
        matches!(self, PresetMove::Wander)
    }
    fn build(&self) -> MoveMode {
        match self {
            PresetMove::Wander => MoveMode::Wander,
            PresetMove::Amble => MoveMode::Amble(CreatureState::Idle(Timer(0))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egg_render::Flip;
    use crate::world::player::{MoveMode, PresetId, Shell};

    /// `Presets::builtin` embeds the shipped data and spawns each built-in with
    /// the right archetype, hitbox and behaviour — the data is the source of the
    /// shells. An unknown id is a clean miss.
    #[test]
    fn builtin_presets_spawn_the_creatures() {
        // The shipped local (un-offset) hitboxes, pinned here so a stray edit to
        // `data.toml` is caught.
        // Hitboxes are now flush at the shell's `pos` (the hitbox top-left); the
        // old per-preset `y` inset is gone, derived at draw time from the sprite
        // footprint instead (see `Shell::draw_params`).
        let hitboxes = [
            ("ellie", [0, 0, 7, 5]),
            ("may", [0, 0, 7, 5]),
            ("bro", [0, 0, 7, 5]),
            ("critter", [0, 0, 8, 8]),
            ("dog", [0, 0, 7, 5]),
        ];
        let presets = Presets::builtin();
        for (name, hitbox) in hitboxes {
            let id = PresetId::new(name);
            let shell = presets.spawn(&id).unwrap_or_else(|| panic!("spawn {name}"));
            assert_eq!(shell.preset, id, "{name} stamps its id");
            let [x, y, w, h] = hitbox;
            assert_eq!(shell.local_hitbox, Hitbox::new(x, y, w, h), "{name} hitbox");
        }
        // The critter ambles; the others wander.
        assert!(matches!(
            presets.spawn(&PresetId::critter()).unwrap().move_mode,
            MoveMode::Amble(_)
        ));
        assert!(matches!(
            presets.spawn(&PresetId::dog()).unwrap().move_mode,
            MoveMode::Wander
        ));
        // An unknown id is a clean miss, not a panic.
        assert!(presets.spawn(&PresetId::new("nope")).is_none());
    }

    /// A preset's `poses` table builds into `ShellSprites.poses`, keyed by
    /// name — what a `pose NAME` motion (see
    /// [`crate::data::scene::Motion::Pose`]) resolves against at draw time.
    /// No shipped preset has one yet, so it builds empty by default.
    #[test]
    fn preset_poses_build_into_shell_sprites() {
        let file = parse(include_str!("../../../../assets/data/data.toml")).unwrap();
        let mut def = file.presets["critter"].clone();
        assert!(def.poses.is_empty(), "no shipped preset has poses yet");
        def.poses.insert(
            "slump".into(),
            SpriteSet { ids: vec![500], w: 8, h: 8, flip: Flip::None },
        );
        let sprites = def.build_sprites();
        assert_eq!(sprites.poses.len(), 1);
        assert_eq!(sprites.poses["slump"].get_frame(0).id, 500);
    }

    /// `poses` is absent-by-default in TOML (`skip_serializing_if`): a preset
    /// with none — every shipped one, today — emits with no `poses` key at
    /// all, so the shipped file re-serialises unchanged; a preset that *does*
    /// carry one round-trips through emit/splice/parse.
    #[test]
    fn preset_poses_serialise_only_when_present() {
        let src = include_str!("../../../../assets/data/data.toml");
        let file = parse(src).unwrap();
        let bare = emit_preset("bro", &file.presets["bro"]).unwrap();
        assert!(!bare.contains("poses"), "no poses ⇒ no `poses` key: {bare}");

        let mut def = file.presets["bro"].clone();
        def.poses.insert(
            "slump".into(),
            SpriteSet { ids: vec![500, 501], w: 8, h: 8, flip: Flip::Horizontal },
        );
        let emitted = emit_preset("bro", &def).unwrap();
        assert!(emitted.contains("poses"), "poses present ⇒ `poses` key: {emitted}");
        let out = splice_preset(src, "bro", &emitted);
        let reparsed = parse(&out).unwrap();
        assert_eq!(reparsed.presets["bro"], def, "poses round-trip through TOML");
    }

    /// The shipped `data.toml` (items + every built-in preset, walk grids in
    /// full) survives a TOML serialise/parse round trip unchanged — the format
    /// the file is authored in and the engine loads through.
    #[test]
    fn toml_round_trips_data_file() {
        let data = parse(include_str!("../../../../assets/data/data.toml"))
            .expect("shipped data.toml parses");
        let toml = to_toml(&data).expect("serialise");
        let parsed = parse(&toml).expect("parse");
        assert_eq!(data, parsed);
    }

    /// A `[presets]`-less file (only `[items]`) parses with no presets.
    #[test]
    fn items_only_file_parses_with_empty_presets() {
        let src = "\
[items.ff]
sprite = 513

[items.lm]
sprite = 514
";
        let data = parse(src).expect("parse");
        assert_eq!(data.items["ff"].sprite, 513);
        assert_eq!(data.items["lm"].sprite, 514);
        assert!(data.presets.is_empty());
    }

    /// An unknown / malformed document is a parse error the caller can fall back
    /// on, not a panic.
    #[test]
    fn malformed_data_is_an_error() {
        assert!(parse("items = [not a table]").is_err());
    }

    /// An item's optional `use` key parses into the matching [`UseDef`] for each
    /// of its three forms, and an item with no `use` reads back as `on_use ==
    /// None`.
    #[test]
    fn item_use_parses_every_form() {
        let src = "\
[items.plain]
sprite = 1

[items.talk]
sprite = 2
use = { dialogue = \"hello\" }

[items.scene]
sprite = 3
use = { cutscene = \"intro\" }

[items.act]
sprite = 4
use = { func = \"toggle_dog\" }
";
        let data = parse(src).expect("parse");
        assert_eq!(data.items["plain"].on_use, None, "no `use` ⇒ None");
        assert_eq!(
            data.items["talk"].on_use,
            Some(UseDef::Dialogue("hello".into())),
        );
        assert_eq!(
            data.items["scene"].on_use,
            Some(UseDef::Cutscene("intro".into())),
        );
        assert_eq!(
            data.items["act"].on_use,
            Some(UseDef::Func("toggle_dog".into())),
        );
    }

    /// An item with no `on_use` serialises to no `use` key (the
    /// `skip_serializing_if`) — so the shipped file stays clean for items that
    /// don't use it.
    #[test]
    fn item_use_none_elides_the_key() {
        let mut file = DataFile::default();
        file.items
            .insert("plain".into(), ItemDef { sprite: 1, on_use: None });
        let toml = to_toml(&file).expect("serialise");
        assert!(toml.contains("[items.plain]"), "plain item present: {toml}");
        assert!(
            !toml.contains("use"),
            "no `on_use` ⇒ no `use` key at all: {toml}",
        );
    }

    /// A set `on_use` round-trips through a TOML serialise/parse unchanged, for
    /// each of the three forms.
    #[test]
    fn item_use_round_trips_through_toml() {
        for def in [
            UseDef::Dialogue("hello".into()),
            UseDef::Cutscene("intro".into()),
            UseDef::Func("toggle_dog".into()),
        ] {
            let mut file = DataFile::default();
            file.items.insert(
                "talk".into(),
                ItemDef {
                    sprite: 2,
                    on_use: Some(def.clone()),
                },
            );
            let toml = to_toml(&file).expect("serialise");
            let parsed = parse(&toml).expect("reparse");
            assert_eq!(parsed, file, "use round-trips: {def:?} via {toml}");
        }
    }

    /// The shipped `data.toml` parses to the expected items, and its walk grids
    /// resolve to the right cells — the permanent regression that pins the
    /// behaviour the old pattern builders used to produce, now that the grids are
    /// authored in full. A heading `(dx, dy)` buckets to a grid cell via
    /// [`WalkSprites::dir_to_sprite`](crate::world::player::WalkSprites::dir_to_sprite);
    /// `frame` picks the frame within that cell's animation.
    #[test]
    fn shipped_data_toml_resolves_the_right_cells() {
        let presets = Presets::builtin();
        assert_eq!(presets.defs.len(), 5, "five built-in presets");

        // The shipped items.
        let items = parse(include_str!("../../../../assets/data/data.toml"))
            .expect("shipped data.toml parses")
            .items;
        assert_eq!(items.len(), 6);
        assert_eq!(items["chegg"].sprite, 524);

        let spawn = |name: &str| presets.spawn(&PresetId::new(name)).unwrap();
        // `(dx, dy)` heading -> the resolved frame of its grid cell.
        let frame = |shell: &Shell, dir: (i8, i8), i: usize| {
            shell.sprites.walk.dir_to_sprite(dir).get_frame(i).clone()
        };

        // ellie (humanoid): south strip starts at 768, north at 771; both are
        // 1×2 and loop their walk pair (so frame 0 is the idle, frame 1 the first
        // walk frame). West is the east strip mirrored.
        let ellie = spawn("ellie");
        assert_eq!(frame(&ellie, (0, 1), 0).id, 768, "ellie south[0]");
        assert_eq!(frame(&ellie, (0, 1), 1).id, 769, "ellie south[1]");
        assert_eq!(frame(&ellie, (0, -1), 0).id, 771, "ellie north[0]");
        assert_eq!(frame(&ellie, (1, 0), 0).flip, Flip::None, "ellie east unflipped");
        assert_eq!(
            frame(&ellie, (-1, 0), 0).flip,
            Flip::Horizontal,
            "ellie west mirrored",
        );

        // critter (sideways): side ids [688, 689]; the left column is mirrored,
        // the right column is not.
        let critter = spawn("critter");
        assert_eq!(frame(&critter, (1, 0), 0).id, 688, "critter side[0]");
        assert_eq!(frame(&critter, (1, 0), 1).id, 689, "critter side[1]");
        assert_eq!(frame(&critter, (1, 0), 0).flip, Flip::None, "critter right unflipped");
        assert_eq!(
            frame(&critter, (-1, 0), 0).flip,
            Flip::Horizontal,
            "critter left mirrored",
        );

        // dog (compass): the east look is the wide (2-tile) sprite drawn mirrored
        // with an x_offset of 8; west is the same sprite unmirrored, no offset.
        // (The sheet redraw faces the base art west, so the mirror moved to east.)
        let dog = spawn("dog");
        let east = frame(&dog, (1, 0), 0);
        assert_eq!(east.id, 960, "dog east id");
        assert_eq!(east.x_offset, 8, "dog east x_offset");
        assert_eq!(east.flip, Flip::Horizontal, "dog east mirrored");
        let west = frame(&dog, (-1, 0), 0);
        assert_eq!(west.flip, Flip::None, "dog west unflipped");
        assert_eq!(west.x_offset, 0, "dog west has no offset");
    }

    /// `splice_preset` swaps exactly one preset's span: the file's comments and
    /// the neighbouring presets' text survive byte-for-byte, the spliced file
    /// still parses, and the named preset now equals the edited def. This is the
    /// walk-sprite editor's save path — a whole-file re-serialise would destroy
    /// every comment in `data.toml`.
    #[test]
    fn splice_preset_replaces_one_span_and_keeps_comments() {
        let file = parse(include_str!("../../../../assets/data/data.toml")).unwrap();
        let src = include_str!("../../../../assets/data/data.toml");

        // Edit bro: retile its idle cell to a recognisable sprite id.
        let mut def = file.presets["bro"].clone();
        def.walk.cell_mut(4).frames_mut()[0].id = 777;
        let emitted = emit_preset("bro", &def).unwrap();
        let out = splice_preset(src, "bro", &emitted);

        // Still parses; bro took the edit; every other preset is untouched.
        let reparsed = parse(&out).unwrap();
        assert_eq!(reparsed.presets["bro"], def, "bro took the edit");
        for (name, other) in &file.presets {
            if name != "bro" {
                assert_eq!(&reparsed.presets[name], other, "{name} untouched");
            }
        }
        // The banner comments survive — the whole point of splicing over
        // re-serialising. (Sampled: the section banners around the presets.)
        for banner in [
            "# --- sound effects ---",
            "# --- dialogue portraits ---",
            "# --- items ---",
        ] {
            assert_eq!(
                out.matches(banner).count(),
                src.matches(banner).count(),
                "{banner} survives the splice"
            );
        }
        // Splicing is idempotent modulo parse: emit + splice again → same parse.
        let again = splice_preset(&out, "bro", &emit_preset("bro", &reparsed.presets["bro"]).unwrap());
        assert_eq!(parse(&again).unwrap().presets["bro"], def);

        // An unknown name appends rather than corrupting anything.
        let appended = splice_preset(src, "brand_new", &emit_preset("brand_new", &def).unwrap());
        let reparsed = parse(&appended).unwrap();
        assert_eq!(reparsed.presets["brand_new"], def);
        assert_eq!(reparsed.presets["bro"], file.presets["bro"]);
    }

    /// The splice span ends before the next section's banner comment: replacing
    /// the last preset must not eat the comment block that introduces whatever
    /// section follows it.
    #[test]
    fn splice_preset_leaves_the_next_sections_banner() {
        let src = "# top comment
[presets.a]
hitbox = [0, 0, 8, 8]

[presets.a.walk]
facing = \"per_axis\"

# --- next section banner ---
# more banner
[items.thing]
sprite = 1
";
        // A minimal def to splice in (walk grids are verbose; reuse a real one).
        let file = parse(include_str!("../../../../assets/data/data.toml")).unwrap();
        let def = file.presets["critter"].clone();
        let out = splice_preset(src, "a", &emit_preset("a", &def).unwrap());
        assert!(out.starts_with("# top comment\n"), "leading comment kept");
        assert!(
            out.contains("# --- next section banner ---\n# more banner\n[items.thing]"),
            "the following section's banner block survives: {out}"
        );
        let reparsed = parse(&out).unwrap();
        assert_eq!(reparsed.presets["a"], def);
        assert_eq!(reparsed.items["thing"].sprite, 1);
    }
}

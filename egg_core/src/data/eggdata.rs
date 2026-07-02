//! Game data — the runtime `data.toml` file (plain TOML): the **item registry**
//! and the creature **[`PresetDef`] definitions**. Language-invariant data, the
//! way maps and cutscenes are (names/descriptions stay in the script as
//! `item_<key>` lists) — loaded from `assets/data/` at startup the way the script
//! and maps are, rather than baked into Rust.
//!
//! ## Why this shape
//! A preset's `walk` **is** the runtime [`WalkSprites`](crate::world::player::WalkSprites):
//! a preset deserialises straight into it — the flattened 9-cell grid of per-frame
//! [`SpriteOptions`](crate::render::SpriteOptions) plus its facing policy, in full.
//! There is no shorthand "pattern" layer between the file and the runtime; what
//! ships is exactly what the game reads. The grid is verbose (defaulted frame
//! fields are elided, but nine cells is nine cells), which is the deliberate
//! trade: this format is GUI-emitted and the transparency was chosen over terse
//! hand-authoring.
//!
//! ## Status
//! Both [`items`](DataFile::items) and [`presets`](DataFile::presets) are the live
//! source ([`GameItems::from_data`] / [`Presets::from_data`], installed at boot by
//! [`EggState::load_data`](crate::EggState::load_data)). The embedded shipped file
//! is the built-in default ([`Presets::builtin`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geometry::Hitbox;
use crate::world::player::{
    CreatureState, MoveMode, PresetId, Shell, ShellSprites, SpriteAnimation, Timer, WalkSprites,
};

/// Where the host stores the game-data file, resolved under the asset root
/// (`assets/data/data.toml`) the same way [`SAVE_PATH`](crate::data::save::SAVE_PATH)
/// and the script/map paths are.
pub const DATA_PATH: &str = "data/data.toml";

/// The fixed, gameplay-relevant data for one item — currently just which sprite
/// draws it. Its display name and description are NOT here: those are text, so
/// they live in the script (the `item_<key>` list, read via
/// [`Ctx::item_name`](crate::Ctx::item_name) /
/// [`Ctx::item_desc`](crate::Ctx::item_desc)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemDef {
    pub sprite: i32,
}

/// The registry of every item the game knows about, keyed by the persistent
/// string id a save stores (and an [`InteractFn`](crate::world::interact::InteractFn)
/// names). Loaded game data, threaded through [`Ctx::items`](crate::Ctx::items)
/// like `maps`/`script`/`scenes`.
///
/// The shipped item set is loaded from `assets/data/data.toml` at boot (see
/// [`from_data`](Self::from_data) and [`EggState::load_data`](crate::EggState::load_data)),
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
        self.items.insert(key.to_string(), ItemDef { sprite });
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

/// A parsed `data.toml` file: the whole game's language-invariant data. Both
/// sections default to empty, so a file may carry only `[items]` (as the shipped
/// one does today) or only `[presets]`, and an absent section is simply empty.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DataFile {
    /// The item registry, keyed by the persistent item id a save stores
    /// (`"ff"`, `"lm"`, …). Built into [`GameItems`].
    #[serde(default)]
    pub items: BTreeMap<String, ItemDef>,
    /// Creature presets, keyed by the [`PresetId`](crate::world::player::PresetId)
    /// name a save stores (`"ellie"`, `"critter"`, …). The live runtime source:
    /// [`EggState::load_data`](crate::EggState::load_data) derives the runtime
    /// [`Presets`] store from these.
    #[serde(default)]
    pub presets: BTreeMap<String, PresetDef>,
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
/// then threaded through gameplay as [`Ctx::presets`](crate::Ctx::presets) the
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
        let file = parse(include_str!("../../../assets/data/data.toml"))
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
}

/// One creature archetype: its collision box, wander behaviour, the extra
/// (non-walk) animations, and its walk grid. Mirrors a `Shell::<preset>()`
/// constructor in [`crate::world::player`].
///
/// Field order matters for TOML serialisation: the scalar/array values
/// (`hitbox`, `move_mode`) come before the sub-table fields (`others`, `walk`),
/// since TOML forbids a bare key after a table within the same table.
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
}

impl PresetDef {
    /// The collision box this preset spawns with.
    pub fn hitbox(&self) -> Hitbox {
        let [x, y, w, h] = self.hitbox;
        // Sanitise inputs, avoid `Hitbox::new` assert panic.
        Hitbox::new(x, y, w.max(1), h.max(1))
    }
    /// The full [`ShellSprites`] for this preset — the walk grid (cloned straight
    /// from the deserialised [`WalkSprites`]) plus the `others` strip.
    pub fn build_sprites(&self) -> ShellSprites {
        ShellSprites {
            walk: self.walk.clone(),
            others: vec![self.others.build()],
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
    #[serde(default, skip_serializing_if = "crate::render::Flip::is_none")]
    pub flip: crate::render::Flip,
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
    use crate::render::Flip;
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

    /// The shipped `data.toml` (items + every built-in preset, walk grids in
    /// full) survives a TOML serialise/parse round trip unchanged — the format
    /// the file is authored in and the engine loads through.
    #[test]
    fn toml_round_trips_data_file() {
        let data = parse(include_str!("../../../assets/data/data.toml"))
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
        let items = parse(include_str!("../../../assets/data/data.toml"))
            .expect("shipped data.toml parses")
            .items;
        assert_eq!(items.len(), 3);
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
}

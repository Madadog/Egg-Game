//! Game data — the runtime `data.toml` file (plain TOML): the **item registry**
//! and the creature **[`ShellPreset`](crate::player::ShellPreset) definitions**.
//! Language-invariant data, the way maps and cutscenes are (names/descriptions
//! stay in the script as `item_<key>` lists) — loaded from `assets/data/` at
//! startup the way the script and maps are, rather than baked into Rust.
//!
//! ## Why this shape
//! The format mirrors the *constructors* in [`crate::player`]
//! (`humanoid`/`compass`/`sideways`/`front_back`), **not** the expanded runtime
//! [`WalkSprites`](crate::player::WalkSprites). The runtime form is a flattened
//! 9-cell grid of per-frame [`SpriteOptions`](crate::system::SpriteOptions);
//! serialising that would be enormous and unauthorable. The patterns already are
//! the authoring vocabulary, so a preset is the pattern plus its sprite ids —
//! terse enough to hand-write (the second-class path) and exactly what a future
//! walk-sprite GUI would manipulate (the first-class path).
//!
//! ## Status
//! [`items`](DataFile::items) are the live source today
//! ([`GameItems::from_data`](crate::gamestate::inventory::GameItems::from_data),
//! installed at boot by [`EggState::load_data`](crate::EggState::load_data)).
//! The preset schema below is complete and round-trip-validated against the
//! built-in constructors (see the tests), but presets are not yet the runtime
//! source: every spawn site still constructs from code. Making presets data-
//! driven means threading a store through those spawn sites and pairs with the
//! walk-sprite authoring GUI — a deliberate follow-up, not this layer.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::gamestate::inventory::ItemDef;
use crate::player::{
    CreatureState, LoopMode, MoveMode, PresetId, Shell, ShellSprites, SpriteAnimation, Timer,
    WalkSprites,
};
use crate::position::Hitbox;
use crate::system::Flip;

/// Where the host stores the game-data file, resolved under the asset root
/// (`assets/data/data.toml`) the same way [`SAVE_PATH`](crate::data::save::SAVE_PATH)
/// and the script/map paths are.
pub const DATA_PATH: &str = "data/data.toml";

/// A parsed `data.toml` file: the whole game's language-invariant data. Both
/// sections default to empty, so a file may carry only `[items]` (as the shipped
/// one does today) or only `[presets]`, and an absent section is simply empty.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DataFile {
    /// The item registry, keyed by the persistent item id a save stores
    /// (`"ff"`, `"lm"`, …). Built into [`GameItems`](crate::gamestate::inventory::GameItems).
    #[serde(default)]
    pub items: BTreeMap<String, ItemDef>,
    /// Creature presets, keyed by the [`ShellPreset`](crate::player::ShellPreset)
    /// name a save stores (`"ellie"`, `"critter"`, …). Schema-complete and
    /// validated; not yet the runtime source (see the module docs).
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
/// (non-walk) animations, and how its walk grid is built. Mirrors a
/// `Shell::<preset>()` constructor in [`crate::player`].
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
    /// strip — the `other_ids` of [`ShellSprites`](crate::player::ShellSprites).
    pub others: SpriteSet,
    /// How the eight-heading walk grid is built (see [`WalkSpec`]).
    pub walk: WalkSpec,
}

impl PresetDef {
    /// The collision box this preset spawns with.
    pub fn hitbox(&self) -> Hitbox {
        let [x, y, w, h] = self.hitbox;
        Hitbox::new(x, y, w, h)
    }
    /// The full [`ShellSprites`] for this preset — the walk grid plus the
    /// `others` strip. Reuses the same [`WalkSprites`] constructors the built-in
    /// presets do, so a data-built shell is byte-for-byte the code-built one.
    pub fn build_sprites(&self) -> ShellSprites {
        ShellSprites {
            walk: self.walk.build(),
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
/// [`ShellSprites`](crate::player::ShellSprites) `others` strip is built from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpriteSet {
    pub ids: Vec<i32>,
    pub w: i32,
    pub h: i32,
}
impl SpriteSet {
    fn build(&self) -> SpriteAnimation {
        SpriteAnimation::from_sprite_ids(&self.ids, self.w, self.h)
    }
}

/// How a preset's eight-heading walk grid is built, one variant per authoring
/// pattern in [`WalkSprites`](crate::player::WalkSprites). Externally tagged
/// (TOML's best-supported enum form): the variant name is the table key, e.g.
/// `walk = { humanoid = { south = 768, side = 832 } }`.
///
/// `Humanoid`/`Sideways`/`FrontBack` are the terse patterns (just sprite ids);
/// `Compass` is the explicit escape hatch — four hand-specified [`AnimSpec`]
/// cells — for art that doesn't fit a pattern (the dog, and whatever the future
/// GUI authors).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WalkSpec {
    /// Humanoid four-direction walk: south/side base sprite ids (north is derived
    /// `south + 3`, west mirrors east). See [`WalkSprites::humanoid`].
    Humanoid { south: i32, side: i32 },
    /// One look for every heading, mirrored when facing left. See
    /// [`WalkSprites::sideways`].
    Sideways { ids: Vec<i32>, w: i32, h: i32 },
    /// Explicit four-direction walk: each cardinal animation given in full. See
    /// [`WalkSprites::compass`].
    Compass {
        north: AnimSpec,
        south: AnimSpec,
        east: AnimSpec,
        west: AnimSpec,
    },
    /// North/south only, no mirroring, for every heading. See
    /// [`WalkSprites::front_back`].
    FrontBack { north: AnimSpec, south: AnimSpec },
}
impl WalkSpec {
    /// Build the runtime walk grid, dispatching to the matching
    /// [`WalkSprites`](crate::player::WalkSprites) constructor.
    pub fn build(&self) -> WalkSprites {
        match self {
            WalkSpec::Humanoid { south, side } => WalkSprites::humanoid(*south, *side),
            WalkSpec::Sideways { ids, w, h } => {
                WalkSprites::sideways(SpriteAnimation::from_sprite_ids(ids, *w, *h))
            }
            WalkSpec::Compass {
                north,
                south,
                east,
                west,
            } => WalkSprites::compass(north.build(), south.build(), east.build(), west.build()),
            WalkSpec::FrontBack { north, south } => {
                WalkSprites::front_back(north.build(), south.build())
            }
        }
    }
}

/// One animation as data: either an explicit `ids` list or a `base`+`len` strip
/// (the two [`SpriteAnimation`](crate::player::SpriteAnimation) sources), drawn
/// at `w`×`h`, with optional `flip`, `x_offset` and `loop_mode` modifiers. Used
/// for the explicit [`WalkSpec::Compass`] cells.
///
/// Exactly one of `ids` / `base` is meaningful: `base` present ⇒ a strip,
/// otherwise the `ids` list. (The format is GUI-emitted and round-trip-tested,
/// so this is a documented convention rather than a type-level guarantee.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimSpec {
    /// Explicit per-frame sprite ids (used when `base` is absent).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<i32>,
    /// Strip start id; with `len`, expands to `base, base+w, …` (one frame per
    /// `w`). Mutually exclusive with `ids`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<i32>,
    /// Number of frames in a `base` strip.
    #[serde(default = "one_i32")]
    pub len: i32,
    pub w: i32,
    pub h: i32,
    /// Horizontal/vertical mirror baked into every frame (e.g. a mirrored west).
    #[serde(default, skip_serializing_if = "flip_is_none")]
    pub flip: Flip,
    /// Per-frame draw x-offset (the dog's wide east look uses `8`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub x_offset: i32,
    /// Loop behaviour; absent ⇒ loop the whole strip.
    #[serde(default, skip_serializing_if = "LoopSpec::is_default")]
    pub loop_mode: LoopSpec,
}
impl AnimSpec {
    fn build(&self) -> SpriteAnimation {
        let mut anim = match self.base {
            Some(base) => SpriteAnimation::from_base_sprite_id(base, self.len, self.w, self.h),
            None => SpriteAnimation::from_sprite_ids(&self.ids, self.w, self.h),
        };
        if !flip_is_none(&self.flip) {
            anim = anim.with_flip(self.flip.clone());
        }
        if self.x_offset != 0 {
            anim = anim.with_x_offset(self.x_offset);
        }
        if !self.loop_mode.is_default() {
            anim = anim.with_loopmode(self.loop_mode.build());
        }
        anim
    }
}

/// The wander behaviour a preset spawns with — the data form of the relevant
/// [`MoveMode`](crate::player::MoveMode) variants. (`Egg`/`Player` aren't preset
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

/// The data form of [`LoopMode`](crate::player::LoopMode): an animation's loop
/// behaviour. Inclusive `start`/`end` for the ranged variant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopSpec {
    /// Loop the whole strip.
    #[default]
    Loop,
    /// Play once, then loop only `start..=end`.
    LoopRange { start: usize, end: usize },
    /// Play to the last frame and hold it.
    Hold,
}
impl LoopSpec {
    fn is_default(&self) -> bool {
        matches!(self, LoopSpec::Loop)
    }
    fn build(&self) -> LoopMode {
        match self {
            LoopSpec::Loop => LoopMode::Loop,
            LoopSpec::LoopRange { start, end } => LoopMode::LoopRange(*start, *end),
            LoopSpec::Hold => LoopMode::Hold,
        }
    }
}

// --- serde skip-serializing helpers (keep authored/dumped TOML free of default noise) ---

fn one_i32() -> i32 {
    1
}
fn is_zero(n: &i32) -> bool {
    *n == 0
}
fn flip_is_none(f: &Flip) -> bool {
    matches!(f, Flip::None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::{MoveMode, PresetId};

    /// The built-in presets as `data.toml`, hand-mirrored from the `Shell::<x>()`
    /// constructors in `player.rs`. The tests below prove these rebuild the exact
    /// same sprites/hitboxes — i.e. that the format faithfully captures the code.
    fn builtin_preset(name: &str) -> PresetDef {
        match name {
            "ellie" => PresetDef {
                hitbox: [0, 10, 7, 5],
                move_mode: PresetMove::Wander,
                others: SpriteSet { ids: vec![774, 775], w: 1, h: 2 },
                walk: WalkSpec::Humanoid { south: 768, side: 832 },
            },
            "may" => PresetDef {
                hitbox: [0, 12, 7, 5],
                move_mode: PresetMove::Wander,
                others: SpriteSet { ids: vec![2251, 2252], w: 1, h: 2 },
                walk: WalkSpec::Humanoid { south: 2184, side: 2248 },
            },
            "bro" => PresetDef {
                hitbox: [0, 8, 7, 5],
                move_mode: PresetMove::Wander,
                others: SpriteSet { ids: vec![905, 906], w: 1, h: 2 },
                walk: WalkSpec::Humanoid { south: 896, side: 902 },
            },
            "critter" => PresetDef {
                hitbox: [0, 0, 8, 8],
                move_mode: PresetMove::Amble,
                others: SpriteSet { ids: vec![688], w: 1, h: 1 },
                walk: WalkSpec::Sideways { ids: vec![688, 689], w: 1, h: 1 },
            },
            "dog" => PresetDef {
                hitbox: [0, 12, 7, 5],
                move_mode: PresetMove::Wander,
                others: SpriteSet { ids: vec![968, 970], w: 2, h: 2 },
                walk: WalkSpec::Compass {
                    north: AnimSpec { base: Some(966), len: 2, w: 1, h: 2, ..anim_default() },
                    south: AnimSpec { base: Some(964), len: 2, w: 1, h: 2, ..anim_default() },
                    east: AnimSpec { base: Some(960), len: 2, w: 2, h: 2, x_offset: 8, ..anim_default() },
                    west: AnimSpec {
                        base: Some(960),
                        len: 2,
                        w: 2,
                        h: 2,
                        flip: Flip::Horizontal,
                        ..anim_default()
                    },
                },
            },
            other => panic!("no built-in preset {other:?}"),
        }
    }

    /// `AnimSpec` has no `Default` (it carries required `w`/`h`); this fills the
    /// optional fields so the built-ins above can use struct-update syntax.
    fn anim_default() -> AnimSpec {
        AnimSpec {
            ids: Vec::new(),
            base: None,
            len: 1,
            w: 1,
            h: 1,
            flip: Flip::None,
            x_offset: 0,
            loop_mode: LoopSpec::Loop,
        }
    }

    /// `Presets::builtin` embeds the shipped data and spawns each built-in with
    /// the right archetype, hitbox and behaviour — the data is the source of the
    /// shells. An unknown id is a clean miss.
    #[test]
    fn builtin_presets_spawn_the_creatures() {
        let presets = Presets::builtin();
        for name in ["ellie", "may", "bro", "critter", "dog"] {
            let id = PresetId::new(name);
            let shell = presets.spawn(&id).unwrap_or_else(|| panic!("spawn {name}"));
            assert_eq!(shell.preset, id, "{name} stamps its id");
            assert_eq!(
                shell.local_hitbox,
                builtin_preset(name).hitbox(),
                "{name} hitbox",
            );
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

    /// A populated `data.toml` (items + every built-in preset) survives a
    /// TOML serialise/parse round trip unchanged — the format the file is
    /// authored in and the engine loads through.
    #[test]
    fn toml_round_trips_data_file() {
        let mut data = DataFile::default();
        data.items.insert("ff".into(), ItemDef { sprite: 513 });
        data.items.insert("chegg".into(), ItemDef { sprite: 524 });
        for name in ["ellie", "may", "dog", "bro", "critter"] {
            data.presets.insert(name.into(), builtin_preset(name));
        }
        let toml = to_toml(&data).expect("serialise");
        let parsed = parse(&toml).expect("parse");
        assert_eq!(data, parsed);
    }

    /// The terse patterns omit their defaulted modifiers, and a `[presets]`-less
    /// file (only `[items]`, as shipped today) parses with no presets.
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

    /// The shipped `data.toml` parses, and its hand-written compact form lands
    /// the same structure the canonical (round-trip-validated) defs do — so
    /// `Presets::builtin` (which embeds this file) gets the real built-ins.
    #[test]
    fn shipped_data_toml_parses_to_the_builtins() {
        let data = parse(include_str!("../../../assets/data/data.toml"))
            .expect("shipped data.toml parses");
        assert_eq!(data.items.len(), 3);
        assert_eq!(data.items["chegg"].sprite, 524);
        for n in ["ellie", "may", "bro", "critter", "dog"] {
            assert_eq!(&data.presets[n], &builtin_preset(n), "preset {n} matches canonical");
        }
    }
}

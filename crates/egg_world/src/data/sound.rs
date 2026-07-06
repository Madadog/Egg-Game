//! Sound-effect data — the file stem + note/octave each sound plays at. Loaded
//! from `assets/data/data.toml` (`[sfx.<name>]`) the way items and creature
//! presets are (see [`crate::data::eggdata`]): the file is the single source,
//! embedded at build time as the built-in default and looked up by the canonical
//! name the engine and script name a sound by. There is no const island of
//! `SfxData` values anymore — the metadata is data now.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::data::eggdata;
use egg_platform::SfxOptions;

/// One sound effect's data (a `[sfx.<name>]` entry): the `.ogg` file stem the
/// host loads and the note/octave it plays at. The authored form of an
/// [`SfxData`]; [`to_sfx_data`](Self::to_sfx_data) builds the runtime value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SfxDef {
    /// The `.ogg` file stem under `assets/sfx/` (e.g. `"14_pop"`) — the id the
    /// host resolves to a real sound.
    pub file: String,
    /// The note the sound plays at ([`SfxOptions::note`]). Default 0.
    #[serde(default)]
    pub note: i32,
    /// The octave the sound plays at ([`SfxOptions::octave`]). Default 5 (the
    /// old `DEFAULT_SFX`); the piano is authored an octave lower.
    #[serde(default = "default_octave")]
    pub octave: i32,
}
fn default_octave() -> i32 {
    5
}
impl SfxDef {
    /// The runtime [`SfxData`] this entry describes.
    pub fn to_sfx_data(&self) -> SfxData {
        SfxData {
            id: self.file.clone(),
            options: SfxOptions {
                note: self.note,
                octave: self.octave,
            },
        }
    }
}

/// The resolved sound-effect value ([`SfxData`]) now lives at the platform layer
/// — the [`ConsoleApi`](egg_platform::ConsoleApi) signatures name it — and is
/// re-exported here so `crate::data::sound::SfxData` (and the host's
/// `egg_core::data::sound::SfxData`) keeps resolving. [`SfxDef`] above is the
/// authored form; [`Sounds`] builds these from the registry.
pub use egg_platform::sound::SfxData;

/// The runtime sound registry: every [`SfxDef`] keyed by its canonical name.
/// Built from data.toml `[sfx.*]` — mirrors [`eggdata::Presets`], but cached
/// from the embedded shipped file ([`builtin`]) so the name → sound lookups
/// ([`by_name`] and the named accessors) need no threaded state, matching how
/// sounds were reached through consts before.
#[derive(Debug, Clone)]
pub struct Sounds {
    defs: BTreeMap<String, SfxDef>,
}
impl Sounds {
    /// Build from a parsed [`DataFile`](eggdata::DataFile)'s `[sfx]` table.
    pub fn from_data(file: &eggdata::DataFile) -> Self {
        Self {
            defs: file.sfx.clone(),
        }
    }
    /// The sound filed under `name`, or `None` if the data doesn't define it.
    pub fn get(&self, name: &str) -> Option<SfxData> {
        self.defs.get(name).map(SfxDef::to_sfx_data)
    }
    /// Every sound's file stem, in canonical-name order — the set the web host
    /// loads when it can't scan `assets/sfx/`. Replaces the old `SFX_IDS` array
    /// (the stems are data now, not a duplicated hardcoded list).
    pub fn ids(&self) -> Vec<String> {
        self.defs.values().map(|d| d.file.clone()).collect()
    }
}

/// The built-in sounds: the shipped `data.toml`, embedded and parsed once, so
/// the named accessors and [`by_name`] resolve against the file without any
/// threaded state. Panics only if the *shipped* file is malformed — a
/// build-time-checked invariant.
fn builtin() -> &'static Sounds {
    static BUILTIN: OnceLock<Sounds> = OnceLock::new();
    BUILTIN.get_or_init(|| {
        let file = eggdata::parse(include_str!("../../../../assets/data/data.toml"))
            .expect("shipped data.toml parses");
        Sounds::from_data(&file)
    })
}

/// Resolve a sound effect by its script name (lowercased identifier, e.g.
/// `"gain"`), for sounds embedded in dialogue. Reads the built-in set.
pub fn by_name(name: &str) -> Option<SfxData> {
    builtin().get(name)
}

/// Every sfx file stem (the web host's fallback load list). Reads the embedded
/// built-in set — the single source the old `SFX_IDS` array duplicated.
pub fn sfx_ids() -> Vec<String> {
    builtin().ids()
}

/// One shipped sound by its canonical name — the replacement for the old
/// `sound::<NAME>` consts. Panics only if the *embedded* data omits `name`, a
/// build-shipped invariant the tests pin.
fn sfx(name: &str) -> SfxData {
    builtin()
        .get(name)
        .unwrap_or_else(|| panic!("shipped sfx {name:?} missing from data.toml"))
}

pub fn piano() -> SfxData {
    sfx("piano")
}
pub fn equip_obtained() -> SfxData {
    sfx("equip_obtained")
}
pub fn deny() -> SfxData {
    sfx("deny")
}
pub fn alert_up() -> SfxData {
    sfx("alert_up")
}
pub fn alert_down() -> SfxData {
    sfx("alert_down")
}
pub fn save() -> SfxData {
    sfx("save")
}
pub fn reject() -> SfxData {
    sfx("reject")
}
pub fn item_up() -> SfxData {
    sfx("item_up")
}
pub fn item_swap() -> SfxData {
    sfx("item_swap")
}
pub fn item_down() -> SfxData {
    sfx("item_down")
}
pub fn interact() -> SfxData {
    sfx("interact")
}
pub fn click() -> SfxData {
    sfx("click")
}
pub fn door() -> SfxData {
    sfx("door")
}
pub fn pop() -> SfxData {
    sfx("pop")
}
pub fn click_pop() -> SfxData {
    sfx("click_pop")
}
pub fn fanfare() -> SfxData {
    sfx("fanfare")
}
pub fn gain() -> SfxData {
    sfx("gain")
}
pub fn loss() -> SfxData {
    sfx("loss")
}
pub fn stairs_down() -> SfxData {
    sfx("stairs_down")
}
pub fn stairs_up() -> SfxData {
    sfx("stairs_up")
}
pub fn footstep_plain() -> SfxData {
    sfx("footstep_plain")
}

pub mod music {
    /// The [`MusicTrack`] value now lives at the platform layer (the
    /// [`ConsoleApi`](egg_platform::ConsoleApi) signatures name it),
    /// re-exported here so `crate::data::sound::music::MusicTrack` (and the
    /// host's `egg_core::data::sound::music::MusicTrack`) keeps resolving.
    pub use egg_platform::sound::music::MusicTrack;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 21 shipped sounds, pinned to their file stems and options, so a stray
    /// edit to `data.toml` is caught. These reproduce exactly what the old
    /// `sound::<NAME>` consts produced (all `DEFAULT_SFX` — note 0, octave 5 —
    /// except the piano, an octave lower).
    #[test]
    fn shipped_sfx_match_the_old_consts() {
        let expected: &[(&str, &str, i32, i32)] = &[
            ("piano", "1_piano", 0, 4),
            ("equip_obtained", "2_obtained", 0, 5),
            ("deny", "3_deny", 0, 5),
            ("alert_up", "4_alert_up", 0, 5),
            ("alert_down", "5_alert_down", 0, 5),
            ("save", "6_save", 0, 5),
            ("reject", "7_reject", 0, 5),
            ("item_up", "8_item_up", 0, 5),
            ("item_swap", "9_item_swap", 0, 5),
            ("item_down", "10_item_down", 0, 5),
            ("interact", "11_interact", 0, 5),
            ("click", "12_bip", 0, 5),
            ("door", "13_door", 0, 5),
            ("pop", "14_pop", 0, 5),
            ("click_pop", "15_click_pop", 0, 5),
            ("fanfare", "16_fanfare", 0, 5),
            ("gain", "17_gain", 0, 5),
            ("loss", "18_loss", 0, 5),
            ("stairs_down", "19_stairs_down", 0, 5),
            ("stairs_up", "20_stairs_up", 0, 5),
            ("footstep_plain", "21_footstep_plain", 0, 5),
        ];
        for (name, file, note, octave) in expected {
            let sfx = by_name(name).unwrap_or_else(|| panic!("shipped sfx {name}"));
            assert_eq!(sfx.id, *file, "{name} file stem");
            assert_eq!(sfx.options.note, *note, "{name} note");
            assert_eq!(sfx.options.octave, *octave, "{name} octave");
        }
        // The web fallback load list is exactly those 21 file stems.
        let ids = sfx_ids();
        assert_eq!(ids.len(), 21, "21 shipped sfx");
        for (_, file, _, _) in expected {
            assert!(ids.iter().any(|s| s == file), "ids() contains {file}");
        }
        // An unknown name is a clean miss.
        assert!(by_name("nope").is_none());
    }

    /// Each named accessor resolves to its shipped sound, and `with_note`
    /// overrides the note (the piano/footstep call path).
    #[test]
    fn named_accessors_resolve_and_with_note_overrides() {
        assert_eq!(pop().id, "14_pop");
        assert_eq!(door().id, "13_door");
        assert_eq!(piano().options.octave, 4);
        assert_eq!(piano().with_note(7).options.note, 7);
        assert_eq!(footstep_plain().with_note(17).options.note, 17);
    }
}

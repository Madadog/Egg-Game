//! Dialogue-portrait data — the four sprite cells + offset each portrait draws
//! from. Loaded from `assets/data/data.toml` (`[portrait.<name>]`) the way items
//! and creature presets are (see [`crate::data::eggdata`]): the file is the
//! single source, embedded at build time as the built-in default and looked up
//! by the script name a message names. This module is pure data — the drawing
//! lives in [`crate::ui::portrait`], so nothing here reaches up into `DrawState`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::data::eggdata;

/// One dialogue portrait: the four 8×8 sprite cells it is drawn from (row-major:
/// top-left, top-right, bottom-left, bottom-right) and the pixel offset the box
/// nudges it by. Pure data — see [`crate::ui::portrait::draw_offset`] for the draw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Portrait {
    pub spr_ids: [i32; 4],
    pub offset: (i8, i8),
}
impl Portrait {
    pub const fn new(spr_ids: [i32; 4], offset: (i8, i8)) -> Self {
        Self { spr_ids, offset }
    }
    /// A portrait whose four cells are the 2×2 block starting at `spr_id` (row
    /// stride 32) — the shorthand the shipped data is authored from, resolved to
    /// its four ids.
    pub const fn new_single(spr_id: i32, offset: (i8, i8)) -> Self {
        // Y axis stride is 32 for now...
        let spr_ids = [spr_id, spr_id + 1, spr_id + 32, spr_id + 33];
        Self { spr_ids, offset }
    }
}

/// The runtime portrait registry: every [`Portrait`] keyed by its script name.
/// Built from data.toml `[portrait.*]` — mirrors [`eggdata::Presets`], but
/// cached from the embedded shipped file ([`builtin`]) so [`by_name`] needs no
/// threaded state, matching how portraits were reached through consts before.
#[derive(Debug, Clone)]
pub struct Portraits {
    defs: BTreeMap<String, Portrait>,
}
impl Portraits {
    /// Build from a parsed [`DataFile`](eggdata::DataFile)'s `[portrait]` table.
    pub fn from_data(file: &eggdata::DataFile) -> Self {
        Self {
            defs: file.portraits.clone(),
        }
    }
    /// The portrait filed under `name`, or `None` if the data doesn't define it.
    pub fn get(&self, name: &str) -> Option<Portrait> {
        self.defs.get(name).cloned()
    }
}

/// The built-in portraits: the shipped `data.toml`, embedded and parsed once, so
/// [`by_name`] resolves against the file without any threaded state. Panics only
/// if the *shipped* file is malformed — a build-time-checked invariant.
fn builtin() -> &'static Portraits {
    static BUILTIN: OnceLock<Portraits> = OnceLock::new();
    BUILTIN.get_or_init(|| {
        let file = eggdata::parse(include_str!("../../../assets/data/data.toml"))
            .expect("shipped data.toml parses");
        Portraits::from_data(&file)
    })
}

/// Resolve a portrait by its script name (lowercased identifier, e.g. `"horror"`).
pub fn by_name(name: &str) -> Option<Portrait> {
    builtin().get(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 12 shipped portraits, pinned to their four sprite cells and offset,
    /// so a stray edit to `data.toml` is caught. These reproduce exactly what the
    /// old `Portrait` consts produced (including the `new_single` 2×2 blocks).
    #[test]
    fn shipped_portraits_match_the_old_consts() {
        let expected: &[(&str, [i32; 4], (i8, i8))] = &[
            ("y_normal", [920, 921, 952, 953], (8, 13)),
            ("y_look", [980, 981, 1012, 1013], (8, 15)),
            ("y_close", [982, 983, 1012, 1013], (8, 15)),
            ("y_oof", [1014, 1015, 1012, 1013], (8, 15)),
            ("y_no", [984, 985, 1016, 1013], (8, 15)),
            ("y_yell", [986, 987, 1018, 1019], (3, 11)),
            ("y_away", [988, 989, 1020, 1021], (8, 13)),
            ("y_smug", [990, 991, 1022, 1023], (3, 7)),
            ("y_frus", [926, 927, 958, 959], (3, 7)),
            ("y_hmm", [924, 925, 956, 957], (3, 7)),
            ("y_regret", [922, 923, 954, 955], (8, 13)),
            ("horror", [661, 662, 693, 694], (10, 10)),
        ];
        for (name, spr_ids, offset) in expected {
            let p = by_name(name).unwrap_or_else(|| panic!("shipped portrait {name}"));
            assert_eq!(p.spr_ids, *spr_ids, "{name} sprites");
            assert_eq!(p.offset, *offset, "{name} offset");
        }
        // An unknown name is a clean miss.
        assert!(by_name("nope").is_none());
    }

    /// `new_single` resolves its 2×2 block the way the shipped single-sprite
    /// portraits are authored.
    #[test]
    fn new_single_resolves_the_2x2_block() {
        assert_eq!(
            Portrait::new_single(661, (10, 10)).spr_ids,
            [661, 662, 693, 694]
        );
    }
}

//! Dialogue-portrait data — the sprite cells + offset each portrait draws
//! from. Loaded from `assets/data/data.toml` (`[portrait.<name>]`) the way items
//! and creature presets are (see [`crate::data::eggdata`]): the file is the
//! single source, embedded at build time as the built-in default and looked up
//! by the script name a message names. This module is pure data — the drawing
//! lives in `ui::portrait`, so nothing here reaches up into `DrawState`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::data::eggdata;
use crate::data::metasprite::MetaSprite;

/// One dialogue portrait: the [`MetaSprite`] it is drawn from (any size — a
/// dense row-major grid of 8×8 cells) and the pixel offset the box nudges it
/// by. Pure data — see `ui::portrait::draw_offset` for the draw.
///
/// TOML spellings (see the `data.toml` header): explicit cells
/// (`spr_ids = [...]` row-major, `w` columns wide — default 2), or the
/// sprite-style block shorthand (`spr_id = N` with `w`/`h`, cells read
/// sheet-adjacent). Serializes back in the explicit form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PortraitDef", into = "PortraitDef")]
pub struct Portrait {
    pub sprite: MetaSprite,
    pub offset: (i8, i8),
}
impl Portrait {
    /// A portrait from explicit row-major cells, `w` columns wide.
    pub fn from_grid(spr_ids: &[i32], w: usize, offset: (i8, i8)) -> Self {
        Self {
            sprite: MetaSprite::from_grid(spr_ids, w),
            offset,
        }
    }
    /// A portrait whose cells are the `w`×`h` block read sheet-adjacent from
    /// `spr_id` — the sprite-style shorthand (`spr_id`/`w`/`h` in data.toml).
    pub fn block(spr_id: i32, w: u8, h: u8, offset: (i8, i8)) -> Self {
        Self {
            sprite: MetaSprite::block(spr_id, w, h),
            offset,
        }
    }
    /// The classic 2×2 portrait starting at `spr_id` — [`block`](Self::block)
    /// at the historical default size.
    pub fn new_single(spr_id: i32, offset: (i8, i8)) -> Self {
        Self::block(spr_id, 2, 2, offset)
    }
}

/// The TOML shape of a [`Portrait`] — the serde intermediary that gives the
/// data file its two spellings. Exactly one of `spr_ids` (explicit cells) or
/// `spr_id` (block shorthand, needs `w`+`h`) must be present.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PortraitDef {
    /// Explicit cells, row-major, `w` columns wide.
    #[serde(skip_serializing_if = "Option::is_none")]
    spr_ids: Option<Vec<i32>>,
    /// Block shorthand: top-left sheet id, cells read sheet-adjacent.
    #[serde(skip_serializing_if = "Option::is_none")]
    spr_id: Option<i32>,
    /// Grid columns. Defaults to 2 (the classic portrait width).
    #[serde(skip_serializing_if = "Option::is_none")]
    w: Option<u8>,
    /// Block rows — only meaningful (and only allowed) with `spr_id`;
    /// `spr_ids`' row count is implied by its length. Defaults to 2.
    #[serde(skip_serializing_if = "Option::is_none")]
    h: Option<u8>,
    offset: (i8, i8),
}
impl TryFrom<PortraitDef> for Portrait {
    type Error = String;
    fn try_from(def: PortraitDef) -> Result<Self, Self::Error> {
        let w = def.w.unwrap_or(2);
        if w == 0 {
            return Err("portrait `w` must be ≥ 1".to_string());
        }
        match (def.spr_ids, def.spr_id) {
            (Some(_), Some(_)) => {
                Err("portrait: `spr_ids` and `spr_id` are mutually exclusive".to_string())
            }
            (Some(ids), None) => {
                if def.h.is_some() {
                    return Err(
                        "portrait: `h` only applies to the `spr_id` block shorthand".to_string(),
                    );
                }
                if ids.is_empty() {
                    return Err("portrait: `spr_ids` must not be empty".to_string());
                }
                Ok(Portrait::from_grid(&ids, w.into(), def.offset))
            }
            (None, Some(id)) => Ok(Portrait::block(id, w, def.h.unwrap_or(2), def.offset)),
            (None, None) => Err("portrait needs `spr_ids` or `spr_id`".to_string()),
        }
    }
}
impl From<Portrait> for PortraitDef {
    fn from(p: Portrait) -> Self {
        // Canonical (explicit) form: the grid width is recovered from the
        // cells, and the default width elides `w` so classic portraits
        // round-trip byte-stable.
        let w = p.sprite.grid_width();
        PortraitDef {
            spr_ids: Some(p.sprite.cells.iter().map(|c| c.spr_id).collect()),
            spr_id: None,
            w: (w != 2).then_some(w as u8),
            h: None,
            offset: p.offset,
        }
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
        let file = eggdata::parse(include_str!("../../../../assets/data/data.toml"))
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

    /// The ids of `p`'s cells in storage (row-major) order.
    fn ids(p: &Portrait) -> Vec<i32> {
        p.sprite.cells.iter().map(|c| c.spr_id).collect()
    }

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
            assert_eq!(ids(&p), spr_ids.to_vec(), "{name} sprites");
            assert_eq!(p.sprite.grid_width(), 2, "{name} grid width");
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
            ids(&Portrait::new_single(661, (10, 10))),
            vec![661, 662, 693, 694]
        );
    }

    /// Both TOML spellings parse — explicit `spr_ids` at any width, and the
    /// `spr_id`+`w`/`h` block shorthand — and a portrait serializes back to the
    /// explicit form (`toml` round-trip through [`eggdata`]).
    #[test]
    fn portrait_toml_spellings_and_round_trip() {
        // Arbitrary size: 3 columns × 2 rows of hand-picked ids.
        let file = eggdata::parse(
            "[portraits.wide]\nspr_ids = [1, 2, 3, 10, 20, 30]\nw = 3\noffset = [0, 4]\n",
        )
        .unwrap();
        let wide = &file.portraits["wide"];
        assert_eq!(
            wide.sprite.cells[4].offset,
            egg_render::geometry::Vec2::new(8, 8)
        );
        assert_eq!(wide.sprite.cells[4].spr_id, 20);

        // Block shorthand: 3×1 sheet-adjacent from 700.
        let file =
            eggdata::parse("[portraits.strip]\nspr_id = 700\nw = 3\nh = 1\noffset = [0, 0]\n")
                .unwrap();
        assert_eq!(ids(&file.portraits["strip"]), vec![700, 701, 702]);

        // `w` defaults to 2 for explicit ids (the classic shape)...
        let file = eggdata::parse("[portraits.classic]\nspr_ids = [1, 2, 3, 4]\noffset = [8, 13]\n")
            .unwrap();
        assert_eq!(file.portraits["classic"].sprite.grid_width(), 2);

        // ...and the whole file round-trips through the canonical serializer.
        let out = eggdata::to_toml(&file).unwrap();
        let reparsed = eggdata::parse(&out).unwrap();
        assert_eq!(reparsed.portraits, file.portraits);

        // Misauthored tables are caught, not misread.
        assert!(eggdata::parse("[portraits.bad]\noffset = [0, 0]\n").is_err());
        assert!(
            eggdata::parse("[portraits.bad]\nspr_ids = [1]\nspr_id = 2\noffset = [0, 0]\n")
                .is_err()
        );
        assert!(
            eggdata::parse("[portraits.bad]\nspr_ids = [1, 2]\nh = 4\noffset = [0, 0]\n").is_err()
        );
        assert!(
            eggdata::parse("[portraits.bad]\nspr_ids = [1]\nw = 0\noffset = [0, 0]\n").is_err()
        );
    }
}

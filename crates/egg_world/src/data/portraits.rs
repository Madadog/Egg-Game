//! Dialogue-portrait data — the sprite cells + offset each portrait draws
//! from. Loaded from `assets/data/data.toml` (`[portrait.<name>]`) the way items
//! and creature presets are (see [`crate::data::eggdata`]): the file is the
//! single source, with [`Portraits`] the runtime registry a message's portrait
//! name resolves against (mirrors [`eggdata::Presets`]) — installed on
//! `EggState` at boot and re-derived whenever `data.toml` reloads. This module
//! is pure data — the drawing lives in `ui::portrait`, so nothing here reaches
//! up into `DrawState`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::data::eggdata;
use crate::data::metasprite::MetaSprite;
use egg_render::{Flip, Rotate};

/// One dialogue portrait: the [`MetaSprite`] it is drawn from (any size — a
/// dense row-major grid of 8×8 cells) and the pixel offset the box nudges it
/// by. Pure data — see `ui::portrait::draw_offset` for the draw.
///
/// TOML spellings (see the `data.toml` header): explicit cells
/// (`spr_ids = [...]` row-major, `w` columns wide — default 2), or the
/// sprite-style block shorthand (`spr_id = N` with `w`/`h`, cells read
/// sheet-adjacent). Either spelling may add `flips`/`rotates` — parallel
/// row-major arrays, one entry per cell, giving each cell its own
/// [`Flip`]/[`Rotate`] (`flips` entries `"none"`/`"h"`/`"v"`/`"hv"`, `rotates`
/// entries `0`/`90`/`180`/`270`). Serializes back in the explicit form, with
/// `flips`/`rotates` present only when some cell is non-default on that axis.
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
    /// Per-cell mirror, row-major, one entry per cell: `"none"`, `"h"`, `"v"`,
    /// or `"hv"`. Applies over either spelling above; omitted when every cell
    /// is unmirrored.
    #[serde(skip_serializing_if = "Option::is_none")]
    flips: Option<Vec<String>>,
    /// Per-cell rotation, row-major, one entry per cell: `0`, `90`, `180`, or
    /// `270`. Applies over either spelling above; omitted when every cell is
    /// unrotated.
    #[serde(skip_serializing_if = "Option::is_none")]
    rotates: Option<Vec<i32>>,
    offset: (i8, i8),
}
/// Parse one `flips` entry — see [`PortraitDef::flips`].
fn parse_flip(s: &str) -> Result<Flip, String> {
    match s {
        "none" => Ok(Flip::None),
        "h" => Ok(Flip::Horizontal),
        "v" => Ok(Flip::Vertical),
        "hv" => Ok(Flip::Both),
        other => Err(format!(
            "portrait: unknown flip `{other}` (expected `none`, `h`, `v`, or `hv`)"
        )),
    }
}
/// Parse one `rotates` entry — see [`PortraitDef::rotates`].
fn parse_rotate(deg: i32) -> Result<Rotate, String> {
    match deg {
        0 => Ok(Rotate::None),
        90 => Ok(Rotate::By90),
        180 => Ok(Rotate::By180),
        270 => Ok(Rotate::By270),
        other => Err(format!(
            "portrait: unknown rotate `{other}` (expected 0, 90, 180, or 270)"
        )),
    }
}
/// The `flips` spelling of a [`Flip`] — the inverse of [`parse_flip`].
fn flip_str(f: Flip) -> &'static str {
    match f {
        Flip::None => "none",
        Flip::Horizontal => "h",
        Flip::Vertical => "v",
        Flip::Both => "hv",
    }
}
/// The `rotates` spelling of a [`Rotate`] — the inverse of [`parse_rotate`].
fn rotate_deg(r: Rotate) -> i32 {
    match r {
        Rotate::None => 0,
        Rotate::By90 => 90,
        Rotate::By180 => 180,
        Rotate::By270 => 270,
    }
}
impl TryFrom<PortraitDef> for Portrait {
    type Error = String;
    fn try_from(def: PortraitDef) -> Result<Self, Self::Error> {
        let w = def.w.unwrap_or(2);
        if w == 0 {
            return Err("portrait `w` must be ≥ 1".to_string());
        }
        let mut portrait = match (def.spr_ids, def.spr_id) {
            (Some(_), Some(_)) => {
                return Err("portrait: `spr_ids` and `spr_id` are mutually exclusive".to_string());
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
                Portrait::from_grid(&ids, w.into(), def.offset)
            }
            (None, Some(id)) => Portrait::block(id, w, def.h.unwrap_or(2), def.offset),
            (None, None) => return Err("portrait needs `spr_ids` or `spr_id`".to_string()),
        };

        let n = portrait.sprite.cells.len();
        if let Some(flips) = def.flips {
            if flips.len() != n {
                return Err(format!(
                    "portrait: `flips` has {} entries but the portrait has {n} cells",
                    flips.len()
                ));
            }
            for (cell, f) in portrait.sprite.cells.iter_mut().zip(&flips) {
                cell.flip = parse_flip(f)?;
            }
        }
        if let Some(rotates) = def.rotates {
            if rotates.len() != n {
                return Err(format!(
                    "portrait: `rotates` has {} entries but the portrait has {n} cells",
                    rotates.len()
                ));
            }
            for (cell, &r) in portrait.sprite.cells.iter_mut().zip(&rotates) {
                cell.rotate = parse_rotate(r)?;
            }
        }

        Ok(portrait)
    }
}
impl From<Portrait> for PortraitDef {
    fn from(p: Portrait) -> Self {
        // Canonical (explicit) form: the grid width is recovered from the
        // cells, and the default width elides `w` so classic portraits
        // round-trip byte-stable. `flips`/`rotates` are likewise elided
        // unless some cell actually carries that orientation.
        let w = p.sprite.grid_width();
        let flips = p
            .sprite
            .cells
            .iter()
            .any(|c| !c.flip.is_none())
            .then(|| p.sprite.cells.iter().map(|c| flip_str(c.flip).to_string()).collect());
        let rotates = p
            .sprite
            .cells
            .iter()
            .any(|c| !c.rotate.is_none())
            .then(|| p.sprite.cells.iter().map(|c| rotate_deg(c.rotate)).collect());
        PortraitDef {
            spr_ids: Some(p.sprite.cells.iter().map(|c| c.spr_id).collect()),
            spr_id: None,
            w: (w != 2).then_some(w as u8),
            h: None,
            flips,
            rotates,
            offset: p.offset,
        }
    }
}

/// The runtime portrait registry: every [`Portrait`] keyed by its script name.
/// Built from data.toml `[portrait.*]` — mirrors [`eggdata::Presets`]: threaded
/// through gameplay as `EggState::portraits`, installed at boot from the
/// embedded shipped file ([`builtin`](Self::builtin)) and re-derived from the
/// runtime file whenever `data.toml` reloads.
#[derive(Debug, Clone)]
pub struct Portraits {
    defs: BTreeMap<String, Portrait>,
}
impl Portraits {
    /// The built-ins: the shipped `data.toml`, embedded at compile time, so the
    /// registry is never empty (web/headless/missing runtime file) and the file
    /// is the single source of the portrait definitions. Panics only if the
    /// *shipped* file is malformed — a build-time-checked invariant.
    pub fn builtin() -> Self {
        let file = eggdata::parse(include_str!("../../../../assets/data/data.toml"))
            .expect("shipped data.toml parses");
        Self::from_data(&file)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The ids of `p`'s cells in storage (row-major) order.
    fn ids(p: &Portrait) -> Vec<i32> {
        p.sprite.cells.iter().map(|c| c.spr_id).collect()
    }

    /// The shipped `y_*`/`horror` portraits (the set that used to be hardcoded
    /// `Portrait` consts), pinned to their sprite cells, grid width, and
    /// offset, so a stray edit to `data.toml` is caught. `y_look` was retired
    /// from data.toml (no longer authored) and is intentionally absent here;
    /// the rest are now authored as 3×3 `spr_id` blocks (data.toml redrew the
    /// portraits), expanded to their row-major ids the way `MetaSprite::block`
    /// reads them (+1 per column, +32 per row) — `horror` alone keeps the old
    /// 2×2 `spr_ids` shape.
    #[test]
    fn shipped_portraits_match_the_old_consts() {
        // (name, sprite ids, grid width, offset)
        type Expected = (&'static str, &'static [i32], usize, (i8, i8));
        let expected: &[Expected] = &[
            (
                "y_normal",
                &[2816, 2817, 2818, 2848, 2849, 2850, 2880, 2881, 2882],
                3,
                (6, 5),
            ),
            (
                "y_close",
                &[2912, 2913, 2914, 2944, 2945, 2946, 2976, 2977, 2978],
                3,
                (6, 5),
            ),
            (
                "y_oof",
                &[3113, 3114, 3115, 3145, 3146, 3147, 3177, 3178, 3179],
                3,
                (6, 5),
            ),
            (
                "y_no",
                &[3008, 3009, 3010, 3040, 3041, 3042, 3072, 3073, 3074],
                3,
                (6, 5),
            ),
            (
                "y_yell",
                &[2927, 2928, 2929, 2959, 2960, 2961, 2991, 2992, 2993],
                3,
                (6, 5),
            ),
            (
                "y_away",
                &[2828, 2829, 2830, 2860, 2861, 2862, 2892, 2893, 2894],
                3,
                (6, 5),
            ),
            (
                "y_smug",
                &[3011, 3012, 3013, 3043, 3044, 3045, 3075, 3076, 3077],
                3,
                (6, 5),
            ),
            (
                "y_frus",
                &[2924, 2925, 2926, 2956, 2957, 2958, 2988, 2989, 2990],
                3,
                (6, 5),
            ),
            (
                "y_hmm",
                &[2921, 2922, 2923, 2953, 2954, 2955, 2985, 2986, 2987],
                3,
                (6, 5),
            ),
            (
                "y_regret",
                &[2837, 2838, 2839, 2869, 2870, 2871, 2901, 2902, 2903],
                3,
                (6, 5),
            ),
            ("horror", &[661, 662, 693, 694], 2, (10, 10)),
        ];
        let portraits = Portraits::builtin();
        for (name, spr_ids, grid_width, offset) in expected {
            let p = portraits.get(name).unwrap_or_else(|| panic!("shipped portrait {name}"));
            assert_eq!(ids(&p), spr_ids.to_vec(), "{name} sprites");
            assert_eq!(p.sprite.grid_width(), *grid_width, "{name} grid width");
            assert_eq!(p.offset, *offset, "{name} offset");
        }
        // `y_look` was retired from data.toml.
        assert!(portraits.get("y_look").is_none());
        // An unknown name is a clean miss.
        assert!(portraits.get("nope").is_none());
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

    /// `flips`/`rotates` apply row-major onto the explicit `spr_ids` cells.
    #[test]
    fn portrait_flips_and_rotates_parse_explicit_cells() {
        let file = eggdata::parse(
            "[portraits.oriented]\nspr_ids = [1, 2, 3, 4]\nw = 2\noffset = [0, 0]\n\
             flips = [\"none\", \"h\", \"v\", \"hv\"]\nrotates = [0, 90, 180, 270]\n",
        )
        .unwrap();
        let cells: Vec<(Flip, Rotate)> = file.portraits["oriented"]
            .sprite
            .cells
            .iter()
            .map(|c| (c.flip, c.rotate))
            .collect();
        assert_eq!(
            cells,
            vec![
                (Flip::None, Rotate::None),
                (Flip::Horizontal, Rotate::By90),
                (Flip::Vertical, Rotate::By180),
                (Flip::Both, Rotate::By270),
            ]
        );
    }

    /// The same arrays apply over the `spr_id`+`w`/`h` block shorthand too.
    #[test]
    fn portrait_flips_and_rotates_parse_block_shorthand() {
        let file = eggdata::parse(
            "[portraits.oriented_block]\nspr_id = 700\nw = 2\nh = 1\noffset = [0, 0]\n\
             flips = [\"h\", \"v\"]\nrotates = [90, 180]\n",
        )
        .unwrap();
        let cells: Vec<(Flip, Rotate)> = file.portraits["oriented_block"]
            .sprite
            .cells
            .iter()
            .map(|c| (c.flip, c.rotate))
            .collect();
        assert_eq!(
            cells,
            vec![(Flip::Horizontal, Rotate::By90), (Flip::Vertical, Rotate::By180)]
        );
    }

    /// Wrong-length arrays and unrecognized entries are caught, not misread.
    #[test]
    fn portrait_flips_and_rotates_validate() {
        assert!(eggdata::parse(
            "[portraits.bad]\nspr_ids = [1, 2]\noffset = [0, 0]\nflips = [\"none\"]\n"
        )
        .is_err());
        assert!(eggdata::parse(
            "[portraits.bad]\nspr_ids = [1, 2]\noffset = [0, 0]\nrotates = [0]\n"
        )
        .is_err());
        assert!(eggdata::parse(
            "[portraits.bad]\nspr_ids = [1, 2]\noffset = [0, 0]\nflips = [\"none\", \"sideways\"]\n"
        )
        .is_err());
        assert!(eggdata::parse(
            "[portraits.bad]\nspr_ids = [1, 2]\noffset = [0, 0]\nrotates = [0, 45]\n"
        )
        .is_err());
    }

    /// A portrait with mixed orientations round-trips its `flips`/`rotates`
    /// through the canonical serializer; an all-default portrait elides both
    /// arrays entirely rather than writing them out as all-`"none"`/all-`0`.
    #[test]
    fn portrait_flips_and_rotates_round_trip() {
        let file = eggdata::parse(
            "[portraits.oriented]\nspr_ids = [1, 2, 3, 4]\nw = 2\noffset = [0, 0]\n\
             flips = [\"none\", \"h\", \"v\", \"hv\"]\nrotates = [0, 90, 180, 270]\n",
        )
        .unwrap();
        let out = eggdata::to_toml(&file).unwrap();
        assert!(out.contains("flips"), "orientation survives the round-trip: {out}");
        assert!(out.contains("rotates"), "orientation survives the round-trip: {out}");
        let reparsed = eggdata::parse(&out).unwrap();
        assert_eq!(reparsed.portraits, file.portraits);

        let plain =
            eggdata::parse("[portraits.plain]\nspr_ids = [1, 2, 3, 4]\noffset = [0, 0]\n")
                .unwrap();
        let out = eggdata::to_toml(&plain).unwrap();
        assert!(!out.contains("flips"), "all-default elides the array: {out}");
        assert!(!out.contains("rotates"), "all-default elides the array: {out}");
    }
}

//! The metasprite: one logical sprite assembled from several sheet sprites —
//! cells of `(pixel offset, sprite id)` positioned relative to a shared origin.
//! This is the reusable core of every "sprite made of other sprites" in the
//! game: dialogue [`Portrait`](crate::data::portraits::Portrait)s (a dense grid
//! of arbitrary cells) and the sprite-plane map layers' flood-fill components
//! (an irregular blob of tiles, see
//! [`SpriteComponent`](crate::world::map::SpriteComponent)) both hold one.
//! Pure data — no drawing here; consumers walk [`iter_at`](MetaSprite::iter_at)
//! and draw each cell through whatever pass they own (the portrait's
//! outline+fill, the component's y-sorted [`DrawParams`](crate::draw_state::DrawParams)).

use serde::{Deserialize, Serialize};

use egg_render::geometry::Vec2;
use egg_render::{Flip, Rotate};

/// Sprite-sheet row stride: how far apart two vertically-adjacent sheet cells'
/// ids sit. The shipped sheet is 256 px wide → 32 8×8 columns. Only the
/// *authoring shorthands* ([`MetaSprite::block`], data.toml `spr_id`+`w`/`h`)
/// bake this in — an explicit cell list carries any ids it likes.
pub const SHEET_TILES_PER_ROW: i32 = 32;

/// One cell of a [`MetaSprite`]: a single 8×8 sheet sprite at a pixel offset
/// from the metasprite's origin, with its own draw orientation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaCell {
    /// Pixel offset from the metasprite's origin (the position a consumer
    /// passes to [`MetaSprite::iter_at`]).
    pub offset: Vec2,
    /// The sheet sprite id drawn here.
    pub spr_id: i32,
    /// This cell's own mirror, composed (not replaced) when a consumer draws
    /// the whole metasprite mirrored via [`MetaSprite::iter_at_flipped`].
    #[serde(default, skip_serializing_if = "Flip::is_none")]
    pub flip: Flip,
    /// This cell's own rotation. Unaffected by [`iter_at_flipped`](MetaSprite::iter_at_flipped)'s
    /// offset mirroring, but composed with the outer flip per cell — see
    /// [`Flip::compose_cell`].
    #[serde(default, skip_serializing_if = "Rotate::is_none")]
    pub rotate: Rotate,
}

impl MetaCell {
    /// A cell at `offset` drawing `spr_id` with default (identity) orientation.
    pub const fn new(offset: Vec2, spr_id: i32) -> Self {
        Self {
            offset,
            spr_id,
            flip: Flip::None,
            rotate: Rotate::None,
        }
    }
}

/// A sprite made of other sprites: any number of 8×8 cells, each with its own
/// sheet id, placed at pixel offsets from one shared origin. Cells may form a
/// dense grid (portraits) or an irregular blob (map flood-fill components) —
/// the type doesn't care.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaSprite {
    pub cells: Vec<MetaCell>,
}
impl MetaSprite {
    /// A `w`×`h` block read sheet-adjacent from `spr_id` — the "sprite-style"
    /// shorthand: ids advance by 1 per column and [`SHEET_TILES_PER_ROW`] per
    /// row, exactly the cells `spr(id, .., w, h)` would draw.
    pub fn block(spr_id: i32, w: u8, h: u8) -> Self {
        let ids: Vec<i32> = (0..i32::from(h))
            .flat_map(|row| (0..i32::from(w)).map(move |col| spr_id + col + row * SHEET_TILES_PER_ROW))
            .collect();
        Self::from_grid(&ids, w.into())
    }
    /// Arbitrary ids laid out row-major on an 8 px grid `w` columns wide (a
    /// short last row is fine). `w == 0` yields no cells rather than dividing
    /// by zero.
    pub fn from_grid(ids: &[i32], w: usize) -> Self {
        if w == 0 {
            return Self::default();
        }
        let cells = ids
            .iter()
            .zip(0..)
            .map(|(&spr_id, i): (&i32, i32)| {
                MetaCell::new(
                    Vec2::new((i % w as i32) as i16 * 8, (i / w as i32) as i16 * 8),
                    spr_id,
                )
            })
            .collect();
        Self { cells }
    }
    /// The cells as `(position, cell)` with `origin` added to every offset —
    /// the draw-loop view. Each cell carries its own orientation as-authored;
    /// use [`iter_at_flipped`](Self::iter_at_flipped) to additionally mirror
    /// the whole metasprite.
    pub fn iter_at(&self, origin: Vec2) -> impl Iterator<Item = (Vec2, &MetaCell)> + '_ {
        self.cells
            .iter()
            .map(move |cell| (origin + cell.offset, cell))
    }
    /// [`iter_at`](Self::iter_at), but mirrored by `flip` as a whole: cell
    /// offsets are reflected within the metasprite's bounding box (cells are
    /// uniform 8×8, so `mirrored = min + max − offset` on each mirrored axis
    /// preserves that box), and `flip` is composed into each yielded cell's
    /// own `(flip, rotate)` via [`Flip::compose_cell`]. `Flip::None` degenerates
    /// to exactly `iter_at`, cell-for-cell.
    pub fn iter_at_flipped(
        &self,
        origin: Vec2,
        flip: Flip,
    ) -> impl Iterator<Item = (Vec2, MetaCell)> + '_ {
        let xs = self.cells.iter().map(|c| c.offset.x);
        let ys = self.cells.iter().map(|c| c.offset.y);
        let (min_x, max_x) = (xs.clone().min().unwrap_or(0), xs.max().unwrap_or(0));
        let (min_y, max_y) = (ys.clone().min().unwrap_or(0), ys.max().unwrap_or(0));
        self.cells.iter().map(move |cell| {
            let x = if flip.x() {
                min_x + max_x - cell.offset.x
            } else {
                cell.offset.x
            };
            let y = if flip.y() {
                min_y + max_y - cell.offset.y
            } else {
                cell.offset.y
            };
            let (cell_flip, cell_rotate) = flip.compose_cell(cell.flip, cell.rotate);
            (
                origin + Vec2::new(x, y),
                MetaCell {
                    offset: Vec2::new(x, y),
                    spr_id: cell.spr_id,
                    flip: cell_flip,
                    rotate: cell_rotate,
                },
            )
        })
    }
    /// The grid width (in cells) of a dense row-major metasprite — the `w` that
    /// [`from_grid`](Self::from_grid) laid it out with, recovered from the
    /// widest offset. Used to serialize a portrait back to its authored shape;
    /// meaningless for an irregular blob.
    pub fn grid_width(&self) -> usize {
        self.cells
            .iter()
            .map(|c| c.offset.x as usize / 8 + 1)
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `block` reads the sheet the way `spr(id, .., w, h)` does: ids advance by
    /// one per column and by the sheet row stride per row.
    #[test]
    fn block_reads_sheet_adjacent_ids() {
        let m = MetaSprite::block(920, 2, 2);
        let cells: Vec<(i16, i16, i32)> = m
            .cells
            .iter()
            .map(|c| (c.offset.x, c.offset.y, c.spr_id))
            .collect();
        assert_eq!(
            cells,
            vec![(0, 0, 920), (8, 0, 921), (0, 8, 952), (8, 8, 953)]
        );
        // Non-square: 3 wide × 1 tall stays on one sheet row.
        let wide = MetaSprite::block(10, 3, 1);
        assert_eq!(
            wide.cells.iter().map(|c| c.spr_id).collect::<Vec<_>>(),
            vec![10, 11, 12]
        );
    }

    /// `from_grid` lays arbitrary ids row-major on the 8 px grid, tolerating a
    /// ragged last row, and `grid_width` recovers the authored width.
    #[test]
    fn from_grid_lays_out_row_major() {
        let m = MetaSprite::from_grid(&[5, 6, 7, 8, 9], 3);
        let cells: Vec<(i16, i16, i32)> = m
            .cells
            .iter()
            .map(|c| (c.offset.x, c.offset.y, c.spr_id))
            .collect();
        assert_eq!(
            cells,
            vec![(0, 0, 5), (8, 0, 6), (16, 0, 7), (0, 8, 8), (8, 8, 9)]
        );
        assert_eq!(m.grid_width(), 3);
        // Degenerate widths don't panic.
        assert!(MetaSprite::from_grid(&[1, 2], 0).cells.is_empty());
        assert_eq!(MetaSprite::from_grid(&[], 4).cells.len(), 0);
    }

    /// `iter_at` shifts every cell by the origin — the one loop both the
    /// portrait draw and the map component params are built on.
    #[test]
    fn iter_at_offsets_by_origin() {
        let m = MetaSprite::from_grid(&[1, 2], 2);
        let at: Vec<(Vec2, i32)> = m
            .iter_at(Vec2::new(100, 50))
            .map(|(pos, cell)| (pos, cell.spr_id))
            .collect();
        assert_eq!(at, vec![(Vec2::new(100, 50), 1), (Vec2::new(108, 50), 2)]);
    }

    /// `iter_at_flipped` with `Flip::None` degenerates to `iter_at`, cell for
    /// cell.
    #[test]
    fn iter_at_flipped_none_matches_iter_at() {
        let m = MetaSprite::from_grid(&[1, 2, 3, 4], 2);
        let plain: Vec<(Vec2, MetaCell)> = m
            .iter_at(Vec2::new(10, 20))
            .map(|(pos, cell)| (pos, cell.clone()))
            .collect();
        let flipped: Vec<(Vec2, MetaCell)> =
            m.iter_at_flipped(Vec2::new(10, 20), Flip::None).collect();
        assert_eq!(plain, flipped);
    }

    /// A horizontal mirror of a 2×2 grid swaps columns (offsets reflected
    /// within the bounding box) and every yielded cell carries `flip ==
    /// Horizontal`. A cell authored with `rotate: By90` comes out `By270`
    /// under the outer horizontal mirror, per [`Flip::compose_cell`].
    #[test]
    fn iter_at_flipped_horizontal_mirrors_grid() {
        let mut m = MetaSprite::from_grid(&[1, 2, 3, 4], 2);
        m.cells[1].rotate = Rotate::By90; // the (8, 0) cell, id 2.
        let flipped: Vec<(Vec2, i32, Flip, Rotate)> = m
            .iter_at_flipped(Vec2::zero(), Flip::Horizontal)
            .map(|(pos, cell)| (pos, cell.spr_id, cell.flip, cell.rotate))
            .collect();
        assert_eq!(
            flipped,
            vec![
                (Vec2::new(8, 0), 1, Flip::Horizontal, Rotate::None),
                (Vec2::new(0, 0), 2, Flip::Horizontal, Rotate::By270),
                (Vec2::new(8, 8), 3, Flip::Horizontal, Rotate::None),
                (Vec2::new(0, 8), 4, Flip::Horizontal, Rotate::None),
            ]
        );
    }
}

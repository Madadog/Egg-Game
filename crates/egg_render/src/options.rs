//! Per-draw option structs handed to the drawing primitives: sprite/map/font
//! settings and the [`Flip`] enum. Stateless data; the live palette/sheet state
//! lives on the game's `DrawState`, which also owns the `DrawParams` sprite-frame
//! bundle these options are embedded in.

#[derive(Clone, Debug)]
pub struct MapOptions {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub sx: i32,
    pub sy: i32,
    pub transparent: Option<u8>,
    pub scale: i8,
}

impl<'a> MapOptions {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        sx: i32,
        sy: i32,
        transparent: &'a [u8],
        scale: i8,
    ) -> Self {
        Self {
            x,
            y,
            w,
            h,
            sx,
            sy,
            transparent: Some(transparent[0]),
            scale,
        }
    }
}

impl Default for MapOptions {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            w: 30,
            h: 17,
            sx: 0,
            sy: 0,
            transparent: None,
            scale: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Flip {
    #[default]
    None,
    Horizontal,
    Vertical,
    Both,
}

impl Flip {
    /// Whether this is the default (no mirror) — the serde
    /// `skip_serializing_if` guard for [`SpriteOptions`].
    pub const fn is_none(&self) -> bool {
        matches!(self, Flip::None)
    }
    /// Whether this flip mirrors horizontally.
    pub const fn x(&self) -> bool {
        matches!(self, Flip::Horizontal | Flip::Both)
    }
    /// Whether this flip mirrors vertically.
    pub const fn y(&self) -> bool {
        matches!(self, Flip::Vertical | Flip::Both)
    }
    /// Build a flip from independent horizontal/vertical mirror flags — the
    /// inverse of [`x`](Self::x)/[`y`](Self::y).
    pub const fn from_axes(x: bool, y: bool) -> Self {
        match (x, y) {
            (false, false) => Flip::None,
            (true, false) => Flip::Horizontal,
            (false, true) => Flip::Vertical,
            (true, true) => Flip::Both,
        }
    }
    /// Compose two flips: the Klein four-group operation on independent
    /// mirror axes — an axis ends up mirrored if exactly one operand mirrors
    /// it (two mirrors on the same axis cancel). `Flip::None` is the identity.
    pub const fn compose(self, other: Flip) -> Flip {
        Self::from_axes(self.x() ^ other.x(), self.y() ^ other.y())
    }
    /// Compose this *outer* whole-sprite mirror with one cell's own `(flip,
    /// rotate)`, returning the single `(Flip, Rotate)` pair that reproduces
    /// the mirrored cell as one blit.
    ///
    /// The raster core (`sheet.rs`) maps output coordinates through `rotate`
    /// first, then `flip`, so as an image operation a cell is
    /// rotate-applied-after-flip. Mirroring the whole assembly by an outer `M`
    /// conjugates that op through the dihedral relation `M∘R = R⁻¹∘M`: the
    /// flip half always XORs with `M` (mirrors commute and cancel in pairs),
    /// while the rotate half is only ever swapped between `By90` and `By270`
    /// — and only when `M` mirrors exactly one axis (`Horizontal` or
    /// `Vertical`), since a 90° turn maps to its inverse when reflected across
    /// a single axis but is fixed by a reflection across both (`Both`) or
    /// neither (`None`). `By180` and `None` are their own inverse, so they
    /// never change.
    pub const fn compose_cell(self, cell_flip: Flip, cell_rotate: Rotate) -> (Flip, Rotate) {
        let flip = self.compose(cell_flip);
        let rotate = match (self, cell_rotate) {
            (Flip::Horizontal | Flip::Vertical, Rotate::By90) => Rotate::By270,
            (Flip::Horizontal | Flip::Vertical, Rotate::By270) => Rotate::By90,
            (_, r) => r,
        };
        (flip, rotate)
    }
}

// Sprite options share the raster core's rotation type directly.
pub use super::Rotate;

/// Per-sprite draw settings: which colour key is transparent, scale, flip,
/// rotation and the multi-tile `w`×`h` footprint. `id`/`x_offset`/`y_offset`
/// describe a sprite *frame* (used by the animation/player code to position
/// frames); the raster core ignores them. A single colour key suffices for
/// every call site, so `transparent` is one optional index rather than a slice.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpriteOptions {
    #[serde(default)]
    pub id: i32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub x_offset: i32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub y_offset: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparent: Option<u8>,
    #[serde(default = "one_i32", skip_serializing_if = "is_one")]
    pub scale: i32,
    #[serde(default, skip_serializing_if = "Flip::is_none")]
    pub flip: Flip,
    #[serde(default, skip_serializing_if = "Rotate::is_none")]
    pub rotate: Rotate,
    #[serde(default = "one_i32", skip_serializing_if = "is_one")]
    pub w: i32,
    #[serde(default = "one_i32", skip_serializing_if = "is_one")]
    pub h: i32,
}

/// Serde default for the `scale`/`w`/`h` fields, whose natural absent value is
/// `1` (a 1×1 unscaled sprite), not `0`.
fn one_i32() -> i32 {
    1
}
/// Serde `skip_serializing_if` guards: keep a defaulted offset (`0`) or a
/// defaulted `scale`/`w`/`h` (`1`) out of the dumped/authored TOML.
fn is_zero(n: &i32) -> bool {
    *n == 0
}
fn is_one(n: &i32) -> bool {
    *n == 1
}
impl SpriteOptions {
    pub const fn default() -> Self {
        Self {
            id: 0,
            x_offset: 0,
            y_offset: 0,
            transparent: None,
            scale: 1,
            flip: Flip::None,
            rotate: Rotate::None,
            w: 1,
            h: 1,
        }
    }
    pub const fn transparent_zero() -> Self {
        Self {
            transparent: Some(0),
            ..Self::default()
        }
    }
}

impl Default for SpriteOptions {
    fn default() -> Self {
        // Delegates to the inherent `const fn default`; inherent associated
        // functions shadow the trait method here, so this is not recursive.
        Self::default()
    }
}

#[derive(Clone)]
pub struct PrintOptions {
    pub color: i32,
    pub fixed: bool,
    pub scale: i32,
    pub small_text: bool,
}
impl PrintOptions {
    pub fn with_color(self, color: i32) -> Self {
        Self { color, ..self }
    }
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            color: 15,
            fixed: false,
            scale: 1,
            small_text: false,
        }
    }
}
pub struct FontOptions<'a> {
    pub transparent: &'a [u8],
    pub char_width: i8,
    pub char_height: i8,
    pub fixed: bool,
    pub scale: i32,
    pub alt_font: bool,
}

impl Default for FontOptions<'_> {
    fn default() -> Self {
        Self {
            transparent: &[],
            char_width: 8,
            char_height: 8,
            fixed: false,
            scale: 1,
            alt_font: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `compose_cell` combines an outer whole-sprite mirror with a cell's own
    /// `(flip, rotate)` into the single pair a blit applies directly. Table:
    /// `(outer, cell_flip, cell_rotate) -> (flip, rotate)`.
    #[test]
    fn compose_cell_table() {
        let cases: &[(Flip, Flip, Rotate, Flip, Rotate)] = &[
            // Outer mirrors exactly one axis: By90/By270 swap under it, but
            // an unrotated cell only picks up the flip.
            (Flip::Horizontal, Flip::None, Rotate::None, Flip::Horizontal, Rotate::None),
            (Flip::Horizontal, Flip::None, Rotate::By90, Flip::Horizontal, Rotate::By270),
            (Flip::Vertical, Flip::None, Rotate::By270, Flip::Vertical, Rotate::By90),
            // Outer `Both` mirrors both axes, so By90/By270 are fixed (only a
            // single-axis mirror swaps them).
            (Flip::Both, Flip::None, Rotate::By90, Flip::Both, Rotate::By90),
            // Outer `None` is the identity: the cell passes through unchanged.
            (Flip::None, Flip::Horizontal, Rotate::By90, Flip::Horizontal, Rotate::By90),
            // Flip composition is a pure per-axis XOR, independent of rotate.
            (Flip::Horizontal, Flip::Horizontal, Rotate::None, Flip::None, Rotate::None),
            (Flip::Horizontal, Flip::Vertical, Rotate::None, Flip::Both, Rotate::None),
            (Flip::Horizontal, Flip::Both, Rotate::None, Flip::Vertical, Rotate::None),
        ];
        for &(outer, cell_flip, cell_rotate, want_flip, want_rotate) in cases {
            assert_eq!(
                outer.compose_cell(cell_flip, cell_rotate),
                (want_flip, want_rotate),
                "outer={outer:?} cell=({cell_flip:?}, {cell_rotate:?})"
            );
        }
    }
}

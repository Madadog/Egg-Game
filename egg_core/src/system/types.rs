#[derive(Clone, Debug)]
pub struct DrawParams {
    pub index: i32,
    pub x: i32,
    pub y: i32,
    pub options: SpriteOptions,
    pub outline: Option<u8>,
    pub palette_rotate: u8,
}

impl DrawParams {
    pub fn new(
        index: i32,
        x: i32,
        y: i32,
        options: SpriteOptions,
        outline: Option<u8>,
        palette_rotate: u8,
    ) -> Self {
        Self {
            index,
            x,
            y,
            options,
            outline,
            palette_rotate,
        }
    }
    pub fn draw_to(
        self,
        draw_state: &mut crate::drawstate::DrawState,
        layer: crate::drawstate::LayerId,
    ) {
        let palette_map = crate::drawstate::palette_map_rotate(self.palette_rotate.into());
        if let Some(outline) = self.outline {
            draw_state.spr_with_outline(
                layer,
                &palette_map,
                self.index,
                self.x,
                self.y,
                self.options,
                outline,
            );
        } else {
            draw_state.spr(
                layer,
                &palette_map,
                self.index,
                self.x,
                self.y,
                self.options,
            );
        }
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.options.h * 8
    }
}

#[derive(Debug, Clone)]
pub struct SfxOptions {
    pub note: i32,
    pub octave: i32,
}
impl Default for SfxOptions {
    fn default() -> Self {
        Self {
            note: -1,
            octave: -1,
        }
    }
}

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

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Flip {
    #[default]
    None,
    Horizontal,
    Vertical,
    Both,
}

impl Flip {
    /// Whether this flip mirrors horizontally.
    pub const fn x(&self) -> bool {
        matches!(self, Flip::Horizontal | Flip::Both)
    }
    /// Whether this flip mirrors vertically.
    pub const fn y(&self) -> bool {
        matches!(self, Flip::Vertical | Flip::Both)
    }
}

// Sprite options share the raster core's rotation type directly.
pub use super::drawing::Rotate;

/// Per-sprite draw settings: which colour key is transparent, scale, flip,
/// rotation and the multi-tile `w`×`h` footprint. `id`/`x_offset`/`y_offset`
/// describe a sprite *frame* (used by the animation/player code to position
/// frames); the raster core ignores them. A single colour key suffices for
/// every call site, so `transparent` is one optional index rather than a slice.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpriteOptions {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub x_offset: i32,
    #[serde(default)]
    pub y_offset: i32,
    #[serde(default)]
    pub transparent: Option<u8>,
    #[serde(default = "one_i32")]
    pub scale: i32,
    #[serde(default)]
    pub flip: Flip,
    #[serde(default)]
    pub rotate: Rotate,
    #[serde(default = "one_i32")]
    pub w: i32,
    #[serde(default = "one_i32")]
    pub h: i32,
}

/// Serde default for the `scale`/`w`/`h` fields, whose natural absent value is
/// `1` (a 1×1 unscaled sprite), not `0`.
fn one_i32() -> i32 {
    1
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

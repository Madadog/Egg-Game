use crate::data::tiled::TileLayer;
use egg_platform::{HEIGHT, SWEETIE_16, WIDTH};
use egg_render::image::{IndexedImage, Rgba, RgbaImage};
use egg_render::{MapOptions, SpriteOptions};

pub struct DrawState {
    pub rgba_canvas: Vec<RgbaImage>,
    pub rgba_sprites: RgbaImage,

    pub indexed_canvas: Vec<IndexedImage>,
    pub indexed_sprites: IndexedImage,

    pub palettes: Vec<Vec<[u8; 3]>>,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            rgba_canvas: vec![RgbaImage::new(WIDTH as u32, HEIGHT as u32); 2],
            rgba_sprites: RgbaImage::new(0, 0),
            indexed_canvas: vec![IndexedImage::new(WIDTH as usize, HEIGHT as usize); 2],
            indexed_sprites: IndexedImage::new(0, 0),
            palettes: vec![default_palette()],
        }
    }
}

/// 256-entry palette: SWEETIE_16 plus `[255, 255, 255]` filler. Matches the
/// console's historical default.
fn default_palette() -> Vec<[u8; 3]> {
    let mut p = Vec::with_capacity(256);
    p.extend_from_slice(&SWEETIE_16);
    p.resize(256, [255, 255, 255]);
    p
}

/// A colour as game data names one: either a palette index — resolved live
/// against the default palette, so palette swaps (day/night) and fades
/// re-colour it every frame — or a literal RGB triple, which is absolute and
/// untouched by palette dynamics. Resolve with [`DrawState::resolve`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BgColour {
    Index(u8),
    Rgb([u8; 3]),
}

impl Default for BgColour {
    /// Palette slot 0 — the console's historical "clear to black".
    fn default() -> Self {
        Self::Index(0)
    }
}

impl BgColour {
    /// Parse a hex colour string: `#rrggbb` or Tiled's `#aarrggbb` (alpha
    /// ignored — a background has nothing to blend with), `#` optional,
    /// case-insensitive. `None` on anything else.
    pub fn parse_rgb(s: &str) -> Option<[u8; 3]> {
        let hex = s.trim().strip_prefix('#').unwrap_or_else(|| s.trim());
        // Length is in bytes; non-ASCII of the right byte count would panic the
        // subslicing below (char boundaries), so bounce it here.
        if !hex.is_ascii() {
            return None;
        }
        let rgb = match hex.len() {
            6 => hex,
            8 => &hex[2..],
            _ => return None,
        };
        let channel = |i: usize| u8::from_str_radix(&rgb[2 * i..2 * i + 2], 16).ok();
        Some([channel(0)?, channel(1)?, channel(2)?])
    }

    /// The `#rrggbb` form of an [`Rgb`](Self::Rgb) colour; `None` for an index
    /// (which has no fixed RGB — it follows the live palette).
    pub fn hex(&self) -> Option<String> {
        match self {
            Self::Index(_) => None,
            Self::Rgb([r, g, b]) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
        }
    }
}

/// Named index of the layer you're drawing to. Can be cast to `usize`.
#[repr(usize)]
#[derive(Clone, Copy)]
pub enum LayerId {
    /// Background layer
    BG = 0,
    /// Foreground layer
    FG,
}

impl DrawState {
    /// Default RGBA layer canvas (mutable).
    pub fn rgba(&mut self, layer: LayerId) -> &mut RgbaImage {
        &mut self.rgba_canvas[layer as usize]
    }

    /// Default indexed layer canvas (mutable).
    pub fn indexed(&mut self, layer: LayerId) -> &mut IndexedImage {
        &mut self.indexed_canvas[layer as usize]
    }

    /// The render target's size in px — the layer canvases' dimensions (all
    /// layers share one size; see [`resize`](Self::resize)). This is the surface
    /// draw calls actually land on, so screen-relative positioning (e.g. the
    /// dialogue box) should measure against it rather than the host's main-window
    /// [`ConsoleApi::width`](egg_platform::ConsoleApi::width), which differs for
    /// off-screen render targets such as the extra editor views.
    pub fn size(&self) -> (i32, i32) {
        let bg = &self.rgba_canvas[LayerId::BG as usize];
        (bg.width() as i32, bg.height() as i32)
    }

    /// Reallocate every screen layer canvas (RGBA + indexed) to `w`×`h`. Used
    /// when the framebuffer follows the window ("mirror" mode). Contents are
    /// dropped — each layer is fully redrawn per frame. The sprite sheets
    /// (`rgba_sprites`/`indexed_sprites`) are left untouched.
    pub fn resize(&mut self, w: u32, h: u32) {
        self.rgba_canvas = vec![RgbaImage::new(w, h); self.rgba_canvas.len()];
        self.indexed_canvas =
            vec![IndexedImage::new(w as usize, h as usize); self.indexed_canvas.len()];
    }

    /// Resolve a palette index to an Rgba using the default palette
    /// (`palettes[0]`).
    pub fn colour(&self, idx: u8) -> Rgba {
        self.palettes
            .first()
            .and_then(|p| p.get(usize::from(idx)))
            .copied()
            .map(Rgba::from_rgb)
            .unwrap_or(Rgba::TRANSPARENT)
    }

    /// Resolve a [`BgColour`] to an Rgba: an index through the default palette
    /// (like [`colour`](Self::colour)), a literal RGB verbatim.
    pub fn resolve(&self, colour: BgColour) -> Rgba {
        match colour {
            BgColour::Index(idx) => self.colour(idx),
            BgColour::Rgb(rgb) => Rgba::from_rgb(rgb),
        }
    }

    /// Clear an RGBA layer to the colour at `idx` in the default palette.
    pub fn cls(&mut self, layer: LayerId, idx: u8) {
        let colour = self.colour(idx);
        self.rgba_canvas[layer as usize].fill(colour);
    }

    /// Replace the first 16 entries of `palettes[0]` with the given palette.
    /// Slots beyond 16 (filler) are left untouched.
    pub fn set_palette(&mut self, palette: &[[u8; 3]; 16]) {
        for (i, c) in palette.iter().enumerate() {
            if let Some(slot) = self.palettes[0].get_mut(i) {
                *slot = *c;
            }
        }
    }

    /// Draw a sprite from the default indexed sprite sheet onto `layer`,
    /// using the default palette and the caller-supplied `palette_map`.
    pub fn spr(
        &mut self,
        layer: LayerId,
        palette_map: &[usize],
        id: i32,
        x: i32,
        y: i32,
        opts: SpriteOptions,
    ) {
        let canvas = &mut self.rgba_canvas[layer as usize];
        let palette = self.palettes[0].as_slice();
        canvas.spr_indexed(&self.indexed_sprites, palette, palette_map, id, x, y, opts);
    }

    /// Draw the 4-direction outline of a sprite from the default indexed
    /// sheet (no centre fill).
    pub fn spr_outline(
        &mut self,
        layer: LayerId,
        id: i32,
        x: i32,
        y: i32,
        opts: SpriteOptions,
        outline_colour: u8,
    ) {
        let canvas = &mut self.rgba_canvas[layer as usize];
        let palette = self.palettes[0].as_slice();
        canvas.spr_outline(
            &self.indexed_sprites,
            palette,
            id,
            x,
            y,
            opts,
            outline_colour,
        );
    }

    /// Draw a sprite with a 1-pixel outline around it. Equivalent to
    /// `spr_outline` followed by `spr`.
    #[allow(clippy::too_many_arguments)]
    pub fn spr_with_outline(
        &mut self,
        layer: LayerId,
        palette_map: &[usize],
        id: i32,
        x: i32,
        y: i32,
        opts: SpriteOptions,
        outline_colour: u8,
    ) {
        let canvas = &mut self.rgba_canvas[layer as usize];
        let palette = self.palettes[0].as_slice();
        canvas.spr_outline(
            &self.indexed_sprites,
            palette,
            id,
            x,
            y,
            opts.clone(),
            outline_colour,
        );
        canvas.spr_indexed(&self.indexed_sprites, palette, palette_map, id, x, y, opts);
    }

    /// Draw a region of `map_layer` onto the RGBA `layer`, using the default
    /// indexed sprite sheet and the caller-supplied `palette_map`.
    pub fn map_draw(
        &mut self,
        canvas_layer: LayerId,
        map_layer: &TileLayer,
        palette_map: &[usize],
        opts: MapOptions,
    ) {
        let canvas = &mut self.rgba_canvas[canvas_layer as usize];
        let palette = self.palettes[0].as_slice();
        canvas.map_draw_indexed(map_layer, &self.indexed_sprites, palette, palette_map, opts);
    }

    /// Stroke a 1-pixel debug outline of `hitbox` on the RGBA `layer`, in the
    /// default palette's `colour`. The debug-overlay counterpart to the drawing
    /// helpers — the walkaround map-info overlay and the map editor use it to
    /// visualise warp/interaction/collision hitboxes.
    pub fn stroke_hitbox(&mut self, layer: LayerId, hitbox: egg_render::geometry::Hitbox, colour: u8) {
        use egg_render::Canvas;
        let c = self.colour(colour);
        self.rgba_canvas[layer as usize].stroke_rect(
            hitbox.x.into(),
            hitbox.y.into(),
            hitbox.w.into(),
            hitbox.h.into(),
            c,
        );
    }
}

/// A single sprite frame described as a draw record: which sheet tile, where,
/// and with what options/outline/palette rotation. A game-domain bundle (the
/// animation and map code build these, then y-sort and draw them), so it lives
/// with [`DrawState`] — the state it draws itself onto — rather than with the
/// stateless render primitives.
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
    pub fn draw_to(self, draw_state: &mut DrawState, layer: LayerId) {
        let palette_map = palette_map_rotate(self.palette_rotate.into());
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

/// Identity palette map: index `i` maps to `i`.
pub const PALETTE_MAP_IDENTITY: [usize; 16] =
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// Palette map with every entry shifted by `rotate`, wrapping at 16.
pub fn palette_map_rotate(rotate: usize) -> [usize; 16] {
    std::array::from_fn(|i| (i + rotate) % 16)
}

/// Palette map with every entry pointing at `c` (used for the outline trick).
pub const fn palette_map_all(c: u8) -> [usize; 16] {
    [c as usize; 16]
}

/// Linearly interpolate each RGB triple in `target` between `from` and `to`.
/// `amount` is fixed-point with 256 = "fully `to`". Clamped at 256.
pub fn fade_palette_into(target: &mut [[u8; 3]], from: &[[u8; 3]], to: &[[u8; 3]], amount: u16) {
    let amount = amount.min(256);
    let n = target.len().min(from.len()).min(to.len());
    for i in 0..n {
        for j in 0..3 {
            target[i][j] =
                ((from[i][j] as u16 * (256 - amount) + to[i][j] as u16 * amount) >> 8) as u8;
        }
    }
}

/// Linearly interpolate one RGB triple between `from` and `to`. See
/// [`fade_palette_into`] for `amount` semantics.
pub fn fade_colour_into(target: &mut [u8; 3], from: [u8; 3], to: [u8; 3], amount: u16) {
    let amount = amount.min(256);
    for j in 0..3 {
        target[j] = ((from[j] as u16 * (256 - amount) + to[j] as u16 * amount) >> 8) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egg_render::Flip;

    fn fresh_state() -> DrawState {
        let mut s = DrawState::default();
        s.palettes[0] = vec![[0, 0, 0]; 16];
        s.palettes[0][1] = [255, 0, 0];
        s.indexed_sprites = IndexedImage::new(256, 128);
        s.indexed_sprites.data[0] = 1;
        s
    }

    /// `parse_rgb` accepts the forms map data carries — `#rrggbb`, Tiled's
    /// `#aarrggbb` (alpha dropped), hash optional, any case — and refuses
    /// everything else, so a corrupt property reads as "no colour", not garbage.
    #[test]
    fn bg_colour_hex_parses_both_widths_and_rejects_malformed() {
        assert_eq!(BgColour::parse_rgb("#b13e53"), Some([177, 62, 83]));
        assert_eq!(BgColour::parse_rgb("#FFB13E53"), Some([177, 62, 83]));
        assert_eq!(BgColour::parse_rgb("b13e53"), Some([177, 62, 83]));
        // "ｂ１" is 6 *bytes* of non-ASCII — must reject, not panic on a
        // mid-codepoint slice.
        for bad in ["", "#", "#12345", "#1234567", "#xxyyzz", "#b13e5g", "ｂ１"] {
            assert_eq!(BgColour::parse_rgb(bad), None, "{bad:?} rejected");
        }
        // An Rgb colour round-trips through its own hex form; an index has none.
        let c = BgColour::Rgb([177, 62, 83]);
        assert_eq!(c.hex().as_deref(), Some("#b13e53"));
        assert_eq!(BgColour::parse_rgb(&c.hex().unwrap()), Some([177, 62, 83]));
        assert_eq!(BgColour::Index(3).hex(), None);
    }

    /// `resolve` sends an index through the live palette (so a palette swap
    /// re-colours it) and passes a literal RGB through untouched.
    #[test]
    fn resolve_follows_palette_for_index_only() {
        let mut s = fresh_state();
        assert_eq!(s.resolve(BgColour::Index(1)), Rgba::new(255, 0, 0, 255));
        assert_eq!(s.resolve(BgColour::Rgb([7, 8, 9])), Rgba::new(7, 8, 9, 255));
        // Swap the palette: the index follows, the literal doesn't.
        s.palettes[0][1] = [0, 255, 0];
        assert_eq!(s.resolve(BgColour::Index(1)), Rgba::new(0, 255, 0, 255));
        assert_eq!(s.resolve(BgColour::Rgb([7, 8, 9])), Rgba::new(7, 8, 9, 255));
    }

    #[test]
    fn helper_form_draws_via_palette_lookup() {
        let mut s = fresh_state();
        s.spr(
            LayerId::BG,
            &PALETTE_MAP_IDENTITY,
            0,
            10,
            20,
            SpriteOptions::default(),
        );
        assert_eq!(
            s.rgba_canvas[0].get_pixel(10, 20),
            Rgba::new(255, 0, 0, 255)
        );
    }

    #[test]
    fn explicit_form_does_not_fight_borrow_checker() {
        let mut s = fresh_state();
        // Multi-field split borrow: &mut rgba_canvas + &indexed_sprites + &palettes
        // through different field paths on the same struct.
        let palette_map = PALETTE_MAP_IDENTITY;
        s.rgba_canvas[LayerId::BG as usize].spr_indexed(
            &s.indexed_sprites,
            &s.palettes[0],
            &palette_map,
            0,
            5,
            5,
            SpriteOptions {
                flip: Flip::None,
                ..SpriteOptions::default()
            },
        );
        assert_eq!(s.rgba_canvas[0].get_pixel(5, 5), Rgba::new(255, 0, 0, 255));
    }

    #[test]
    fn resize_tracks_layer_canvases_and_spares_sprites() {
        let mut s = fresh_state();
        let layers = s.rgba_canvas.len();
        assert_eq!(
            (s.rgba_canvas[0].width(), s.rgba_canvas[0].height()),
            (240, 136),
            "default screen is the base resolution"
        );

        s.resize(960, 540);

        // Every screen layer (RGBA + indexed) follows the new size...
        assert_eq!(s.rgba_canvas.len(), layers);
        assert_eq!(s.indexed_canvas.len(), layers);
        for c in &s.rgba_canvas {
            assert_eq!((c.width(), c.height()), (960, 540));
        }
        for c in &s.indexed_canvas {
            assert_eq!((c.width(), c.height()), (960, 540));
        }
        // ...while the sprite sheet is left untouched.
        assert_eq!(
            (s.indexed_sprites.width(), s.indexed_sprites.height()),
            (256, 128)
        );
    }
}

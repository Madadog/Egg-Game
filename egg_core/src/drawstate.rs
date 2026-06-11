use crate::data::tmj::TileLayer;
use crate::system::{
    HEIGHT, MapOptions, SWEETIE_16, SpriteOptions, WIDTH,
    drawing::image::{IndexedImage, Rgba, RgbaImage},
};

pub struct DrawState {
    pub rgba_canvas: Vec<RgbaImage>,
    pub rgba_sprites: RgbaImage,

    pub indexed_canvas: Vec<IndexedImage>,
    pub indexed_sprites: IndexedImage,

    pub palettes: Vec<Vec<[u8; 3]>>,

    /// Per-tile collision/behaviour flags, indexed by tile id (see
    /// [`crate::map::layer_collides_flags`]). The single source of truth for
    /// flags. Initialised from the built-in blob in
    /// [`crate::data::sprite_flags`]; the plan is to load these from the Tiled
    /// tileset's per-tile properties instead (flags-as-data), retiring the blob.
    pub sprite_flags: Vec<u8>,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            rgba_canvas: vec![RgbaImage::new(WIDTH as u32, HEIGHT as u32); 2],
            rgba_sprites: RgbaImage::new(0, 0),
            indexed_canvas: vec![IndexedImage::new(WIDTH as usize, HEIGHT as usize); 2],
            indexed_sprites: IndexedImage::new(0, 0),
            palettes: vec![default_palette()],
            sprite_flags: crate::data::sprite_flags::default_sprite_flags(),
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
        canvas.spr_outline(&self.indexed_sprites, palette, id, x, y, opts, outline_colour);
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
pub fn fade_palette_into(
    target: &mut [[u8; 3]],
    from: &[[u8; 3]],
    to: &[[u8; 3]],
    amount: u16,
) {
    let amount = amount.min(256);
    let n = target.len().min(from.len()).min(to.len());
    for i in 0..n {
        for j in 0..3 {
            target[i][j] = ((from[i][j] as u16 * (256 - amount) + to[i][j] as u16 * amount) >> 8)
                as u8;
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
    use crate::system::Flip;

    fn fresh_state() -> DrawState {
        let mut s = DrawState::default();
        s.palettes[0] = vec![[0, 0, 0]; 16];
        s.palettes[0][1] = [255, 0, 0];
        s.indexed_sprites = IndexedImage::new(256, 128);
        s.indexed_sprites.data[0] = 1;
        s
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
        assert_eq!(s.rgba_canvas[0].get_pixel(10, 20), Rgba::new(255, 0, 0, 255));
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

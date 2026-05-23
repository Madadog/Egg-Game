use crate::system::{
    MapOptions, SWEETIE_16, StaticSpriteOptions,
    image::{IndexedImage, Rgba, RgbaImage},
    types::GameMap,
};

pub struct DrawState {
    pub rgba_canvas: Vec<RgbaImage>,
    pub rgba_sprites: RgbaImage,

    pub indexed_canvas: Vec<IndexedImage>,
    pub indexed_sprites: IndexedImage,

    pub palettes: Vec<Vec<[u8; 3]>>,

    pub maps: Vec<GameMap>,
    pub sprite_flags: Vec<u8>,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            rgba_canvas: vec![RgbaImage::new(240, 136); 2],
            rgba_sprites: RgbaImage::new(0, 0),
            indexed_canvas: vec![IndexedImage::new(240, 136); 2],
            indexed_sprites: IndexedImage::new(0, 0),
            palettes: vec![default_palette()],
            maps: Vec::new(),
            sprite_flags: vec![0; 2048],
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

#[repr(usize)]
#[derive(Clone, Copy)]
pub enum LayerId {
    BG = 0,
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

    /// Draw a sprite from the default indexed sprite sheet onto `layer`,
    /// using the default palette and the caller-supplied `palette_map`.
    pub fn spr(
        &mut self,
        layer: LayerId,
        palette_map: &[usize],
        id: i32,
        x: i32,
        y: i32,
        opts: StaticSpriteOptions<'_>,
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
        opts: StaticSpriteOptions<'_>,
        outline_colour: u8,
    ) {
        let canvas = &mut self.rgba_canvas[layer as usize];
        let palette = self.palettes[0].as_slice();
        canvas.spr_outline(&self.indexed_sprites, palette, id, x, y, opts, outline_colour);
    }

    /// Draw a sprite with a 1-pixel outline around it. Equivalent to
    /// `spr_outline` followed by `spr`.
    pub fn spr_with_outline(
        &mut self,
        layer: LayerId,
        palette_map: &[usize],
        id: i32,
        x: i32,
        y: i32,
        opts: StaticSpriteOptions<'_>,
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

    /// Draw a region of `maps[bank]`'s layer `map_layer` onto the RGBA `layer`.
    pub fn map_draw(
        &mut self,
        layer: LayerId,
        maps: &[GameMap],
        bank: usize,
        map_layer: usize,
        palette_map: &[usize],
        opts: MapOptions,
    ) {
        let Some(m) = maps.get(bank) else { return };
        let canvas = &mut self.rgba_canvas[layer as usize];
        let palette = self.palettes[0].as_slice();
        canvas.map_draw_indexed(m, map_layer, &self.indexed_sprites, palette, palette_map, opts);
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
            StaticSpriteOptions::default(),
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
            StaticSpriteOptions {
                flip: Flip::None,
                ..StaticSpriteOptions::default()
            },
        );
        assert_eq!(s.rgba_canvas[0].get_pixel(5, 5), Rgba::new(255, 0, 0, 255));
    }
}

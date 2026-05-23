use crate::system::image::{IndexedImage, RgbaImage};

pub struct DrawState {
    pub rgba_canvas: Vec<RgbaImage>,
    pub rgba_sprites: RgbaImage,

    pub indexed_canvas: Vec<IndexedImage>,
    pub indexed_sprites: IndexedImage,

    pub palettes: Vec<Vec<u8>>,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            rgba_canvas: vec![RgbaImage::new(240, 136); 2],
            rgba_sprites: RgbaImage::new(0, 0),
            indexed_canvas: vec![IndexedImage::new(240, 136); 2],
            indexed_sprites: IndexedImage::new(0, 0),
            palettes: Default::default(),
        }
    }
}

#[repr(usize)]
pub enum LayerId {
    BG = 0,
    FG,
}

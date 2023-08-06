use std::ops::{Index, IndexMut};

use bevy::prelude::Image;
use tiny_skia::Color;

pub fn array_to_colour(array: [u8; 3]) -> Color {
    Color::from_rgba8(array[0], array[1], array[2], 255)
}


pub struct IndexedImage {
    width: usize,
    height: usize,
    data: Vec<u8>,
}
impl IndexedImage {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, data: vec![0; width * height] }
    }
    pub fn draw_to_image(&self, palette: &[[u8; 4]; 256], image: &mut Image) {
        for (index, pixel) in self.data.iter().zip(image.data.chunks_exact_mut(4)) {
            let colour = palette[usize::from(*index)];
            pixel.copy_from_slice(&colour);
        }
    }
}
impl Index<(usize, usize)> for IndexedImage {
    type Output = u8;

    fn index(&self, index: (usize, usize)) -> &u8 {
        self.data.get(index.0 + index.1 * self.width).unwrap()
    }
}

impl IndexMut<(usize, usize)> for IndexedImage {
    fn index_mut(&mut self, index: (usize, usize)) -> &mut Self::Output {
        self.data.get_mut(index.0 + index.1 * self.width).unwrap()
    }
}
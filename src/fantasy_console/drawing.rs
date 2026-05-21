use std::ops::{Index, IndexMut};

use bevy::prelude::Image;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const TRANSPARENT: Self = Self([0, 0, 0, 0]);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }

    pub const fn a(self) -> u8 {
        self.0[3]
    }

    pub const fn from_rgb(array: [u8; 3]) -> Self {
        Rgba::new(array[0], array[1], array[2], 255)
    }
}

/// How `blit` treats destination pixels outside the natural projection of the source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgePolicy {
    /// Pixels outside `src` are left untouched.
    #[default]
    Transparent,
    /// Edge pixels of `src` are held outwards to fill the whole destination.
    Clamp,
}

pub struct RgbaImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl RgbaImage {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height * 4) as usize],
        }
    }
    pub fn from_vec(data: Vec<u8>, width: u32, height: u32) -> Self {
        assert_eq!(data.len(), (width * height * 4) as usize);
        Self {
            width,
            height,
            data,
        }
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
    pub fn clone_from(&mut self, other: &RgbaImage) {
        assert_eq!(self.width, other.width);
        assert_eq!(self.height, other.height);
        self.data.copy_from_slice(&other.data);
    }
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> Rgba {
        let i = ((x + y * self.width) * 4) as usize;
        Rgba::new(
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        )
    }
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, colour: Rgba) {
        let i = ((x + y * self.width) * 4) as usize;
        self.data[i..i + 4].copy_from_slice(&colour.0);
    }
    #[inline]
    pub fn set_pixel_index(&mut self, index: usize, colour: Rgba) {
        let i = index * 4;
        self.data[i..i + 4].copy_from_slice(&colour.0);
    }
    #[inline]
    pub fn get_pixel_index(&self, index: usize) -> Rgba {
        let i = index * 4;
        Rgba::new(
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        )
    }
    #[inline]
    pub fn alpha_at_index(&self, index: usize) -> u8 {
        self.data[index * 4 + 3]
    }
    pub fn fill(&mut self, colour: Rgba) {
        for chunk in self.data.chunks_exact_mut(4) {
            chunk.copy_from_slice(&colour.0);
        }
    }
    /// Alpha-blit `src` at (`dx`, `dy`). Pixels with src.a == 0 are skipped.
    pub fn blit(&mut self, dx: i32, dy: i32, src: &RgbaImage, edge: EdgePolicy) {
        let sw = src.width as i32;
        let sh = src.height as i32;
        let dw = self.width as i32;
        let dh = self.height as i32;
        let (x0, y0, x1, y1) = match edge {
            EdgePolicy::Transparent => (dx.max(0), dy.max(0), (dx + sw).min(dw), (dy + sh).min(dh)),
            EdgePolicy::Clamp => (0, 0, dw, dh),
        };
        for y in y0..y1 {
            for x in x0..x1 {
                let sx = (x - dx).clamp(0, sw - 1) as u32;
                let sy = (y - dy).clamp(0, sh - 1) as u32;
                let pixel = src.get_pixel(sx, sy);
                if pixel.a() != 0 {
                    self.set_pixel(x as u32, y as u32, pixel);
                }
            }
        }
    }

    // --- Immediate-mode primitives ---

    /// Fills a horizontal run of pixels. Coordinates are clipped to the image.
    pub fn hline(&mut self, x: i32, y: i32, width: i32, colour: Rgba) {
        if y < 0 || y >= self.height as i32 || width <= 0 {
            return;
        }
        let x0 = x.max(0);
        let x1 = (x + width).min(self.width as i32);
        if x0 >= x1 {
            return;
        }
        for px in x0..x1 {
            self.set_pixel(px as u32, y as u32, colour);
        }
    }
    /// Fills a vertical run of pixels. Coordinates are clipped to the image.
    pub fn vline(&mut self, x: i32, y: i32, height: i32, colour: Rgba) {
        if x < 0 || x >= self.width as i32 || height <= 0 {
            return;
        }
        let y0 = y.max(0);
        let y1 = (y + height).min(self.height as i32);
        for py in y0..y1 {
            self.set_pixel(x as u32, py as u32, colour);
        }
    }
    /// Fills a solid rectangle. Coordinates are clipped to the image.
    pub fn fill_rect(&mut self, x: i32, y: i32, width: i32, height: i32, colour: Rgba) {
        for j in 0..height {
            self.hline(x, y + j, width, colour);
        }
    }
    /// Draws a 1-pixel rectangle border. Coordinates are clipped to the image.
    pub fn stroke_rect(&mut self, x: i32, y: i32, width: i32, height: i32, colour: Rgba) {
        if width <= 0 || height <= 0 {
            return;
        }
        self.hline(x, y, width, colour);
        self.hline(x, y + height - 1, width, colour);
        self.vline(x, y, height, colour);
        self.vline(x + width - 1, y, height, colour);
    }
    /// Bresenham line between two integer endpoints.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, colour: Rgba) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        loop {
            if x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height {
                self.set_pixel(x as u32, y as u32, colour);
            }
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }
    /// Filled circle (midpoint algorithm).
    pub fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, colour: Rgba) {
        if radius < 0 {
            return;
        }
        let mut x = radius;
        let mut y = 0;
        let mut err = 1 - x;
        while x >= y {
            self.hline(cx - x, cy + y, 2 * x + 1, colour);
            self.hline(cx - x, cy - y, 2 * x + 1, colour);
            self.hline(cx - y, cy + x, 2 * y + 1, colour);
            self.hline(cx - y, cy - x, 2 * y + 1, colour);
            y += 1;
            if err < 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }
    /// Outlined circle (midpoint algorithm).
    pub fn stroke_circle(&mut self, cx: i32, cy: i32, radius: i32, colour: Rgba) {
        if radius < 0 {
            return;
        }
        let mut x = radius;
        let mut y = 0;
        let mut err = 1 - x;
        let plot = |img: &mut RgbaImage, px: i32, py: i32| {
            if px >= 0 && py >= 0 && (px as u32) < img.width && (py as u32) < img.height {
                img.set_pixel(px as u32, py as u32, colour);
            }
        };
        while x >= y {
            plot(self, cx + x, cy + y);
            plot(self, cx - x, cy + y);
            plot(self, cx + x, cy - y);
            plot(self, cx - x, cy - y);
            plot(self, cx + y, cy + x);
            plot(self, cx - y, cy + x);
            plot(self, cx + y, cy - x);
            plot(self, cx - y, cy - x);
            y += 1;
            if err < 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }
}

pub struct IndexedImage {
    width: usize,
    _height: usize,
    pub data: Vec<u8>,
}
impl IndexedImage {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            _height: height,
            data: vec![0; width * height],
        }
    }
    /// Only works as intended if self and target image are the same width & height.
    pub fn _draw_to_image(&self, palette: &[[u8; 4]; 256], target_image: &mut [u8]) {
        for (index, pixel) in self.data.iter().zip(target_image.chunks_exact_mut(4)) {
            let colour = palette[usize::from(*index)];
            pixel.copy_from_slice(&colour);
        }
    }
    pub fn from_image(image: &Image, palette: &[[u8; 3]]) -> Self {
        let width = image.size().x as usize;
        let height = image.size().y as usize;
        let mut data = Vec::new();
        'outer: for pixel in image
            .data
            .as_ref()
            .expect("Tried to read uninitialised image.")
            .chunks_exact(4)
        {
            for (i, colour) in palette.iter().enumerate() {
                if pixel[0] == colour[0] && pixel[1] == colour[1] && pixel[2] == colour[2] {
                    data.push(i.try_into().unwrap());
                    continue 'outer;
                }
            }
            data.push(0);
        }
        Self {
            width,
            _height: height,
            data,
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

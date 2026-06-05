use std::ops::{Index, IndexMut};

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

#[derive(Clone, Debug)]
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
}

#[derive(Clone, Debug)]
pub struct IndexedImage {
    width: usize,
    height: usize,
    pub data: Vec<u8>,
}
impl IndexedImage {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![0; width * height],
        }
    }
    pub fn from_vec(data: Vec<u8>, width: usize, height: usize) -> Self {
        assert_eq!(data.len(), width * height);
        Self {
            width,
            height,
            data,
        }
    }
    pub fn width(&self) -> u32 {
        self.width as u32
    }
    pub fn height(&self) -> u32 {
        self.height as u32
    }
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> u8 {
        self.data[x as usize + y as usize * self.width]
    }
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, colour: u8) {
        self.data[x as usize + y as usize * self.width] = colour;
    }
    pub fn fill(&mut self, colour: u8) {
        self.data.fill(colour);
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

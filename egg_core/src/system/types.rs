use crate::{data::save, system::{ConsoleApi, ConsoleHelper}};

#[derive(Debug, Default)]
pub struct SyncHelper {
    already_synced: bool,
    last_bank: u8,
}

impl SyncHelper {
    pub fn step(&mut self) {
        self.already_synced = false;
    }
    /// Sync can only be called once per frame. Returns result to indicate failure or success.
    /// Mask lets you switch out sections of cart data:
    /// * all     = 0    -- 0
    /// * tiles   = 1<<0 -- 1
    /// * sprites = 1<<1 -- 2
    /// * map     = 1<<2 -- 4
    /// * sfx     = 1<<3 -- 8
    /// * music   = 1<<4 -- 16
    /// * palette = 1<<5 -- 32
    /// * flags   = 1<<6 -- 64
    /// * screen  = 1<<7 -- 128 (as of 0.90)
    pub fn sync(&mut self, _mask: i32, bank: u8) -> Result<(), ()> {
        if self.already_synced() {
            Err(())
        } else {
            self.already_synced = true;
            self.last_bank = bank;
            Ok(())
        }
    }
    pub fn already_synced(&self) -> bool {
        self.already_synced
    }
    pub fn last_bank(&self) -> u8 {
        self.last_bank
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EggMemory {
    pub memory: [u8; 1024],
}
impl Default for EggMemory {
    fn default() -> Self {
        Self::new([0; 1024])
    }
}
impl EggMemory {
    pub fn new(memory: [u8; 1024]) -> Self {
        Self { memory }
    }
    pub fn from_array(array: [u8; 1024]) -> Self {
        Self { memory: array }
    }
    pub fn is(&self, bit: save::PmemBit) -> bool {
        bit.is_true_with(&self.memory)
    }
    pub fn set(&mut self, bit: save::PmemBit) {
        bit.set_true_with(&mut self.memory);
    }
    pub fn clear(&mut self, bit: save::PmemBit) {
        bit.set_false_with(&mut self.memory);
    }
    pub fn toggle(&mut self, bit: save::PmemBit) {
        bit.toggle_with(&mut self.memory);
    }
    pub fn get_byte(&self, byte: save::PmemU8) -> u8 {
        self.memory[byte.index()]
    }
    pub fn set_byte(&mut self, byte: save::PmemU8, value: u8) {
        self.memory[byte.index()] = value;
    }
}

/// For simplicity all layers under a map have the same width and height.
/// Ordering of layers is: first at the bottom, last at the top.
#[derive(Clone, Debug)]
pub struct GameMap {
    width: usize,
    height: usize,
    pub layers: Vec<MapLayer>,
}
impl GameMap {
    pub fn new(width: usize, height: usize, layers: Vec<MapLayer>) -> Self {
        Self {
            width,
            height,
            layers,
        }
    }
    pub fn new_empty(width: usize, height: usize, layers: usize) -> Self {
        Self::new(
            width,
            height,
            (0..layers)
                .map(|_| MapLayer::new_empty(width, height))
                .collect(),
        )
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
}

#[derive(Clone, Debug)]
pub struct MapLayer {
    pub name: String,
    width: usize,
    height: usize,
    pub data: Vec<usize>,
}
impl MapLayer {
    pub fn new(name: String, width: usize, height: usize, data: Vec<usize>) -> Self {
        assert!(width * height == data.len());
        Self {
            name,
            width,
            height,
            data,
        }
    }
    pub fn new_empty(width: usize, height: usize) -> Self {
        Self::new(String::new(), width, height, vec![0; width * height])
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn get(&self, x: usize, y: usize) -> Option<usize> {
        self.data.get(y * self.width + x).copied()
    }
    pub fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut usize> {
        self.data.get_mut(y * self.width + x)
    }
}

#[derive(Clone, Debug)]
pub struct StaticDrawParams<'a> {
    pub index: i32,
    pub x: i32,
    pub y: i32,
    pub options: StaticSpriteOptions<'a>,
    pub outline: Option<u8>,
    pub palette_rotate: u8,
}

impl<'a> StaticDrawParams<'a> {
    pub fn new(
        index: i32,
        x: i32,
        y: i32,
        options: StaticSpriteOptions<'a>,
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
    pub fn draw(self, system: &mut impl ConsoleApi) {
        system.palette_map_rotate(self.palette_rotate.into());
        if let Some(outline) = self.outline {
            system.spr_outline(self.index, self.x, self.y, self.options, outline);
        } else {
            system.spr(self.index, self.x, self.y, self.options);
        }
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.options.h * 8
    }
}

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
    pub fn draw(self, system: &mut impl ConsoleApi) {
        system.palette_map_rotate(self.palette_rotate.into());
        if let Some(outline) = self.outline {
            system.spr_outline(
                self.index,
                self.x,
                self.y,
                self.options.compatibility_mode(),
                outline,
            );
        } else {
            system.spr(
                self.index,
                self.x,
                self.y,
                self.options.compatibility_mode(),
            );
        }
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.options.h * 8
    }
}

impl<'a> From<StaticDrawParams<'a>> for DrawParams {
    fn from(other: StaticDrawParams) -> Self {
        Self {
            index: other.index,
            x: other.x,
            y: other.y,
            options: other.options.into(),
            outline: other.outline,
            palette_rotate: other.palette_rotate,
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct MouseInput {
    pub x: i16,
    pub y: i16,
    pub scroll_x: i8,
    pub scroll_y: i8,
    pub left: bool,
    pub middle: bool,
    pub right: bool,
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

pub enum TextureSource {
    Tiles,
    Map,
    VBank1,
}

pub struct TTriOptions<'a> {
    pub texture_src: TextureSource,
    pub transparent: &'a [u8],
    pub z1: f32,
    pub z2: f32,
    pub z3: f32,
    pub depth: bool,
}

impl Default for TTriOptions<'_> {
    fn default() -> Self {
        Self {
            texture_src: TextureSource::Tiles,
            transparent: &[],
            z1: 0.0,
            z2: 0.0,
            z3: 0.0,
            depth: false,
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

#[derive(Debug, Clone)]
pub enum Flip {
    None,
    Horizontal,
    Vertical,
    Both,
}

#[derive(Debug, Clone)]
pub enum Rotate {
    None,
    By90,
    By180,
    By270,
}

#[derive(Debug, Clone)]
pub struct StaticSpriteOptions<'a> {
    pub transparent: &'a [u8],
    pub scale: i32,
    pub flip: Flip,
    pub rotate: Rotate,
    pub w: i32,
    pub h: i32,
}
impl<'a> StaticSpriteOptions<'a> {
    pub const fn default() -> Self {
        Self {
            transparent: &[],
            scale: 1,
            flip: Flip::None,
            rotate: Rotate::None,
            w: 1,
            h: 1,
        }
    }
    pub const fn transparent_zero() -> Self {
        Self {
            transparent: &[0],
            ..Self::default()
        }
    }
}
impl Default for StaticSpriteOptions<'_> {
    fn default() -> Self {
        Self {
            transparent: &[],
            scale: 1,
            flip: Flip::None,
            rotate: Rotate::None,
            w: 1,
            h: 1,
        }
    }
}
#[derive(Debug, Clone)]
pub struct SpriteOptions {
    pub id: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub transparent: Option<u8>,
    pub scale: i32,
    pub flip: Flip,
    pub rotate: Rotate,
    pub w: i32,
    pub h: i32,
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
    pub fn compatibility_mode(&'_ self) -> StaticSpriteOptions<'_> {
        StaticSpriteOptions {
            transparent: self.transparent.as_slice(),
            scale: self.scale,
            flip: self.flip.clone(),
            rotate: self.rotate.clone(),
            w: self.w,
            h: self.h,
        }
    }
}

impl<'a> From<StaticSpriteOptions<'a>> for SpriteOptions {
    fn from(other: StaticSpriteOptions) -> Self {
        Self {
            id: 0,
            x_offset: 0,
            y_offset: 0,
            transparent: other.transparent.first().cloned(),
            scale: other.scale,
            flip: other.flip,
            rotate: other.rotate,
            w: other.w,
            h: other.h,
        }
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

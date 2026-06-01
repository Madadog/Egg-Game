use crate::position::Vec2;

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
        let opts = self.options.compatibility_mode();
        if let Some(outline) = self.outline {
            draw_state.spr_with_outline(
                layer,
                &palette_map,
                self.index,
                self.x,
                self.y,
                opts,
                outline,
            );
        } else {
            draw_state.spr(layer, &palette_map, self.index, self.x, self.y, opts);
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

/// Mouse state holding `[current, previous]` for every field, so movement and
/// button edges are always well-defined. Index `0` is the current frame, index
/// `1` the previous. Call [`MouseInput::shift`] once per frame before writing
/// the new current values.
#[derive(Default, Clone, Copy, Debug)]
pub struct MouseInput {
    pub x: [i16; 2],
    pub y: [i16; 2],
    pub scroll_x: [i8; 2],
    pub scroll_y: [i8; 2],
    pub left: [bool; 2],
    pub middle: [bool; 2],
    pub right: [bool; 2],
}

impl MouseInput {
    /// Current cursor position.
    pub fn pos(&self) -> Vec2 {
        Vec2::new(self.x[0], self.y[0])
    }
    /// Cursor position on the previous frame.
    pub fn previous_pos(&self) -> Vec2 {
        Vec2::new(self.x[1], self.y[1])
    }
    /// Whether the cursor moved since last frame.
    pub fn moved(&self) -> bool {
        self.x[0] != self.x[1] || self.y[0] != self.y[1]
    }
    /// Roll the current values into the previous slot, making room for this
    /// frame's values to be written into the current (index `0`) slot.
    pub fn step(&mut self) {
        self.x[1] = self.x[0];
        self.y[1] = self.y[0];
        self.scroll_x[1] = self.scroll_x[0];
        self.scroll_y[1] = self.scroll_y[0];
        self.left[1] = self.left[0];
        self.middle[1] = self.middle[0];
        self.right[1] = self.right[0];
    }
}

/// Whether a `[current, previous]` button is held this frame.
pub fn pressed(button: [bool; 2]) -> bool {
    button[0]
}

/// Whether a `[current, previous]` button was just pressed this frame — down
/// now, up last frame (rising edge).
pub fn just_pressed(button: [bool; 2]) -> bool {
    button[0] && !button[1]
}

/// Gamepad state holding `[current, previous]` for every button, mirroring
/// [`MouseInput`]. Index `0` is the current frame, `1` the previous. Buttons
/// follow the TIC-80 layout: directions (`up`/`down`/`left`/`right`) then the
/// `a`/`b`/`x`/`y` face buttons. Read edges with the shared [`pressed`] and
/// [`just_pressed`] helpers, exactly as with the mouse buttons.
#[derive(Default, Clone, Copy, Debug)]
pub struct Controller {
    pub up: [bool; 2],
    pub down: [bool; 2],
    pub left: [bool; 2],
    pub right: [bool; 2],
    pub a: [bool; 2],
    pub b: [bool; 2],
    pub x: [bool; 2],
    pub y: [bool; 2],
}

impl Controller {
    /// All eight buttons in TIC-80 index order: up, down, left, right, A, B, X, Y.
    fn buttons(&self) -> [[bool; 2]; 8] {
        [
            self.up, self.down, self.left, self.right, self.a, self.b, self.x, self.y,
        ]
    }
    /// Whether any button is held this frame.
    pub fn any_pressed(&self) -> bool {
        self.buttons().into_iter().any(pressed)
    }
    /// Whether any button had a rising edge this frame (down now, up last frame).
    pub fn any_just_pressed(&self) -> bool {
        self.buttons().into_iter().any(just_pressed)
    }
    /// Whether any button changed state since last frame (press or release).
    pub fn changed(&self) -> bool {
        self.buttons().iter().any(|b| b[0] != b[1])
    }
    /// Release buttons, update last frame state (for `just_pressed`). Call once per frame.
    pub fn step(&mut self) {
        for b in [
            &mut self.up,
            &mut self.down,
            &mut self.left,
            &mut self.right,
            &mut self.a,
            &mut self.b,
            &mut self.x,
            &mut self.y,
        ] {
            b[1] = b[0];
            b[0] = false;
        }
    }
}

#[cfg(test)]
mod mouse_tests {
    use super::*;

    #[test]
    fn edges_and_movement() {
        let mut m = MouseInput::default();
        m.x = [5, 5];
        m.y = [9, 7];
        assert_eq!(m.pos(), Vec2::new(5, 9));
        assert!(m.moved()); // y differs from last frame

        m.y = [7, 7];
        assert!(!m.moved());

        m.left = [true, false];
        assert!(pressed(m.left));
        assert!(just_pressed(m.left));

        m.left = [true, true];
        assert!(pressed(m.left)); // still held...
        assert!(!just_pressed(m.left)); // ...but not a new press

        m.left = [false, true];
        assert!(!pressed(m.left));
        assert!(!just_pressed(m.left));
    }

    #[test]
    fn shift_rolls_current_into_previous() {
        let mut m = MouseInput::default();
        m.x = [3, 0];
        m.left = [true, false];
        m.step();
        assert_eq!(m.x, [3, 3]);
        assert_eq!(m.left, [true, true]);
    }
}

#[cfg(test)]
mod controller_tests {
    use super::*;

    #[test]
    fn edges_and_aggregates() {
        let mut c = Controller::default();
        c.a = [true, false];
        assert!(pressed(c.a));
        assert!(just_pressed(c.a));
        assert!(c.any_pressed());
        assert!(c.any_just_pressed());
        assert!(c.changed());

        c.a = [true, true];
        assert!(pressed(c.a)); // still held...
        assert!(!just_pressed(c.a)); // ...but not a new press
        assert!(!c.changed());
    }

    #[test]
    fn shift_rolls_current_and_clears() {
        let mut c = Controller::default();
        c.up = [true, false];
        c.step();
        // Previous holds last frame's press; current resets to released.
        assert_eq!(c.up, [false, true]);
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

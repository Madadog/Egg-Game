use crate::{
    data::sound::{SfxData, music::MusicTrack},
    rand::Lcg64Xsh32,
    system::{drawing::Canvas, image::RgbaImage},
};

pub use consts::*;
pub use scancode::*;
pub use types::*;

pub mod consts;
pub mod drawing;
pub mod image;
pub mod scancode;
pub mod types;

/// Abstracts away all static memory accesses
pub trait ConsoleApi {
    // TIC-80 RAM
    fn get_gamepads(&mut self) -> &mut [u8; 4];
    fn get_mouse(&mut self) -> &mut MouseInput;
    fn memory(&mut self) -> &mut EggMemory;
    fn get_sprite_flags(&mut self) -> &mut [u8];

    // TIC-80 VRAM
    fn get_palette(&mut self) -> &mut [[u8; 3]];
    fn get_palette_map(&mut self) -> &mut [usize];
    fn get_border_colour(&mut self) -> &mut [u8; 3];
    fn get_screen_offset(&mut self) -> &mut [i8; 2];

    // TIC-80 API
    fn btn(&self, index: i32) -> bool;
    fn btnp(&self, index: i32, hold: i32, period: i32) -> bool;
    fn cls(&mut self, color: u8);
    fn circ(&mut self, x: i32, y: i32, radius: i32, color: u8);
    fn circb(&mut self, x: i32, y: i32, radius: i32, color: u8);
    fn elli(&mut self, x: i32, y: i32, a: i32, b: i32, color: u8);
    fn ellib(&mut self, x: i32, y: i32, a: i32, b: i32, color: u8);
    fn exit(&mut self);
    fn key(&self, scancode: ScanCode) -> bool;
    fn keyp(&self, scancode: ScanCode, hold: i32, period: i32) -> bool;
    /// Latest character entered by the user this frame (for text entry).
    fn key_chars(&self) -> &[char];
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: u8);
    fn map(&mut self, opts: MapOptions);

    fn mouse(&self) -> MouseInput;
    fn music(&mut self, track: Option<&MusicTrack>);
    fn pix(&mut self, x: i32, y: i32, color: u8) -> u8;
    fn pmem(&mut self, address: i32, value: i64) -> i32;
    fn print_alloc(&mut self, text: impl AsRef<str>, x: i32, y: i32, opts: PrintOptions) -> i32;
    fn print_raw(&mut self, text: &str, x: i32, y: i32, opts: PrintOptions) -> i32;
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8);
    fn rectb(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8);
    fn sfx(&mut self, sfx_id: &str, opts: SfxOptions);
    fn spr(&mut self, id: i32, x: i32, y: i32, opts: StaticSpriteOptions);
    fn sync(&mut self, mask: i32, bank: u8, to_cart: bool);
    fn time(&self) -> f32;
    fn tstamp(&self) -> u32;
    fn trace_alloc(text: impl AsRef<str>, color: u8);
    fn tri(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, color: u8);
    fn trib(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, color: u8);
    fn ttri(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
        u1: f32,
        v1: f32,
        u2: f32,
        v2: f32,
        u3: f32,
        v3: f32,
        opts: TTriOptions,
    );
    fn vbank(&mut self, bank: u8) -> u8;

    // Other things
    fn sync_helper(&mut self) -> &mut SyncHelper;
    fn rng(&mut self) -> &mut Lcg64Xsh32;
    fn previous_gamepad(&mut self) -> &mut [u8; 4];
    fn previous_mouse(&mut self) -> &mut MouseInput;

    // Proprietary extensions to the TIC80 API

    /// Get access to map storage
    /// TODO: Separate storage. Console drawing functions consume storage (maps, sprites, etc) and modify a mutable image.
    fn maps(&mut self) -> &mut Vec<GameMap>;
    /// Gets a tile from a specific map.
    fn map_get(&self, bank: usize, layer: usize, x: i32, y: i32) -> usize;
    /// Sets a tile on a specific map.
    fn map_set(&mut self, bank: usize, layer: usize, x: i32, y: i32, value: usize);
    /// Writes data to the virtual filesystem
    fn write_file(&mut self, filename: String, data: &[u8]);
    /// Reads data from the virtual filesystem
    fn read_file(&mut self, filename: String) -> Option<&[u8]>;
    /// Sprite with more options
    fn sprite(&mut self, id: i32, x: i32, y: i32, opts: StaticSpriteOptions, palette_map: &[usize]);
    /// Draws a specific map.
    fn map_draw(&mut self, bank: usize, layer: usize, opts: MapOptions);
    // TODO: No screen. Just expose `bitmaps: Vec<bitmap>`. By convention we can have 0=screen, 1=ovr, 2=sprites etc.
    // get_bitmap(index)
    fn screen_size(&self) -> (u32, u32);
    /// Grab a whole bitmap. By convention:
    ///
    /// 0. Screen
    /// 1. OVR layer
    /// 2. Indexed sprites
    /// 3. RGBA sprites
    fn get_bitmap_indexed(&self, id: usize) -> &[u8];

    /// Canonical final surface drawn by console to screen
    fn output_image(&mut self) -> &mut RgbaImage;

    /// The default 8x8 font used by `print_to` and friends. 16 chars per row.
    fn font(&self) -> &RgbaImage;

    /// Called by `EggState::run` at the start of each frame, before any
    /// drawing happens. Implementations should clear any frame-scoped
    /// buffers (output_image, legacy screen/overlay if present) to
    /// transparent so migrated and legacy draw paths start each frame on
    /// a clean canvas.
    fn frame_start(&mut self);

    // helpers
    fn palette_map_swap(&mut self, from: usize, to: usize) {
        let from: i32 = i32::try_from(from % 16).unwrap();
        assert!(from >= 0);
        self.get_palette_map()[from as usize] = to;
    }
    fn palette_map_set_all(&mut self, to: usize) {
        for i in 0..=15 {
            self.get_palette_map()[i] = to;
        }
    }
    fn set_palette_map(&mut self, map: &[usize]) {
        for (map, target) in map.iter().zip(self.get_palette_map()) {
            *target = *map;
        }
    }
    fn palette_map_reset(&mut self) {
        for i in 0..=15 {
            self.get_palette_map()[i] = i;
        }
    }
    fn palette_map_rotate(&mut self, amount: usize) {
        for i in 0..=15 {
            self.get_palette_map()[i] = i + amount;
        }
    }
    fn set_palette_colour(&mut self, index: u8, rgb: [u8; 3]) {
        let index: usize = (index % 16).into();
        self.get_palette()[index] = rgb;
    }
    fn set_palette(&mut self, colours: [[u8; 3]; 16]) {
        for (i, colour) in colours.iter().enumerate() {
            self.set_palette_colour(i as u8, *colour);
        }
    }
    fn draw_outline(
        &mut self,
        id: i32,
        x: i32,
        y: i32,
        sprite_options: StaticSpriteOptions,
        outline_colour: u8,
    ) {
        let old_map: Vec<usize> = self.get_palette_map().iter_mut().map(|x| *x).collect();
        self.palette_map_set_all(outline_colour.into());
        self.spr(id, x + 1, y, sprite_options.clone());
        self.spr(id, x - 1, y, sprite_options.clone());
        self.spr(id, x, y + 1, sprite_options.clone());
        self.spr(id, x, y - 1, sprite_options);
        self.set_palette_map(&old_map);
    }
}

impl<T: ConsoleApi> ConsoleHelper for T {}

pub trait ConsoleHelper: ConsoleApi {
    // Helper functions
    fn play_sound(&mut self, sfx_data: SfxData) {
        self.sfx(sfx_data.id, sfx_data.options);
    }
    fn update_previous_gamepad(&mut self) {
        let buttons = self.get_gamepads();
        *self.previous_gamepad() = *buttons;
    }
    fn update_previous_mouse(&mut self) {
        let mouse = self.get_mouse();
        *self.previous_mouse() = mouse.clone();
    }
    fn mem_btn(&mut self, id: u8) -> bool {
        let controller: usize = (id / 8).min(3).into();
        let id = id % 8;
        let buttons = self.get_gamepads()[controller];
        (1 << id) & buttons != 0
    }
    fn mem_btnp(&mut self, id: u8) -> bool {
        let controller: usize = (id / 8).min(3).into();
        let id = id % 8;
        let buttons = self.get_gamepads()[controller];
        let previous = self.previous_gamepad()[controller];
        (1 << id) & buttons != (1 << id) & previous && (1 << id) & buttons != 0
    }
    /// Returns true if any button was pressed. Ignores button releases.
    fn any_btnp(&mut self) -> bool {
        let buttons = *self.get_gamepads();
        let previous = *self.previous_gamepad();
        let mut flag = false;
        for (b0, b1) in previous.iter().zip(buttons.iter()) {
            flag |= b0.count_ones() < b1.count_ones();
        }
        flag
    }
    /// Returns true if any button was pressed or released
    fn any_btnpr(&mut self) -> bool {
        let buttons = *self.get_gamepads();
        let previous = *self.previous_gamepad();
        buttons != previous
    }
    fn mouse_delta(&mut self) -> MouseInput {
        let old = self.previous_mouse().clone();
        let new = self.get_mouse();
        MouseInput {
            x: new.x - old.x,
            y: new.y - old.y,
            left: new.left && !old.left,
            middle: new.middle && !old.middle,
            right: new.right && !old.right,
            ..*new
        }
    }
    fn zero_pmem(&mut self) {
        self.memory().memory.fill(0);
    }
    fn fade_palette(&mut self, from: [[u8; 3]; 16], to: [[u8; 3]; 16], amount: u16) {
        let amount = amount.min(256);
        for (index, (colour1, colour2)) in from.iter().zip(to.iter()).enumerate() {
            let mut rgb = [0; 3];
            for (j, (component1, component2)) in colour1.iter().zip(colour2.iter()).enumerate() {
                rgb[j] = ((*component1 as u16 * (256 - amount) + *component2 as u16 * amount) >> 8)
                    as u8;
            }
            self.set_palette_colour(index as u8, rgb);
        }
    }
    fn fade_palette_colour(&mut self, index: u8, from: [u8; 3], to: [u8; 3], amount: u16) {
        let amount = amount.min(256);
        let index: usize = (index % 16).into();
        let mut rgb = [0; 3];
        for (j, (component1, component2)) in from.iter().zip(to.iter()).enumerate() {
            rgb[j] =
                ((*component1 as u16 * (256 - amount) + *component2 as u16 * amount) >> 8) as u8;
        }
        self.set_palette_colour(index as u8, rgb);
    }
    fn set_border_colour(&mut self, colour: u8) {
        if let Some(colour) = self.get_palette().get(usize::from(colour)) {
            *self.get_border_colour() = *colour;
        }
    }
    fn screen_offset(&mut self, horizontal: i8, vertical: i8) {
        self.get_screen_offset()[0] = horizontal;
        self.get_screen_offset()[1] = vertical;
    }
    fn draw_ovr2<T: FnMut(&mut Self)>(&mut self, mut draw: T) {
        self.vbank(1);
        draw(self);
        self.vbank(0);
    }
    fn draw_ovr<T: FnMut()>(&mut self, mut draw: T) {
        self.vbank(1);
        draw();
        self.vbank(0);
    }
    fn get_pmem(&mut self, address: usize) -> u8 {
        let address = address.min(1023);
        self.memory().memory[address]
    }
    fn set_pmem(&mut self, address: usize, value: u8) {
        let address = address.min(1023);
        self.memory().memory[address] = value;
    }

    fn spr_outline(
        &mut self,
        id: i32,
        x: i32,
        y: i32,
        sprite_options: StaticSpriteOptions,
        outline_colour: u8,
    ) {
        self.draw_outline(id, x, y, sprite_options.clone(), outline_colour);
        self.spr(id, x, y, sprite_options);
    }
    fn rect_outline(&mut self, x: i32, y: i32, w: i32, h: i32, fill: u8, outline: u8) {
        self.rect(x, y, w, h, fill);
        self.rectb(x, y, w, h, outline);
    }
    fn print_raw_centered(&mut self, string: &str, x: i32, y: i32, options: PrintOptions) {
        let string_width = self.print_raw(string, 999, 999, options.clone());
        self.print_raw(string, x - string_width / 2, y, options);
    }
    fn print_alloc_centered(&mut self, string: &str, x: i32, y: i32, options: PrintOptions) {
        let string_width = self.print_alloc(string, 999, 999, options.clone());
        self.print_alloc(string, x - string_width / 2, y, options);
    }
    fn print_raw_shadow(
        &mut self,
        string: &str,
        x: i32,
        y: i32,
        options: PrintOptions,
        shadow_colour: i32,
    ) {
        let shadow_options = PrintOptions {
            color: shadow_colour,
            ..options
        };
        self.print_raw(string, x + 1, y + 1, shadow_options);
        self.print_raw(string, x, y, options);
    }

    /// Render `text` onto `target` using the console's default font
    /// (`self.font()`). Returns the maximum line width in pixels. `colour`
    /// is the pixel value (RGBA, palette index, …) used for non-transparent
    /// font pixels — the font itself is read as alpha-only.
    fn print_to<C: Canvas>(
        &self,
        target: &mut C,
        text: &str,
        x: i32,
        y: i32,
        colour: C::Pixel,
        opts: PrintOptions,
    ) -> i32 {
        print_to_with_font(self.font(), target, text, x, y, colour, opts)
    }

    fn print_to_centered<C: Canvas>(
        &self,
        target: &mut C,
        text: &str,
        x: i32,
        y: i32,
        colour: C::Pixel,
        opts: PrintOptions,
    ) -> i32 {
        let width = self.print_to(target, text, 999, 999, colour, opts.clone());
        self.print_to(target, text, x - width / 2, y, colour, opts)
    }

    fn print_to_shadow<C: Canvas>(
        &self,
        target: &mut C,
        text: &str,
        x: i32,
        y: i32,
        colour: C::Pixel,
        shadow: C::Pixel,
        opts: PrintOptions,
    ) -> i32 {
        self.print_to(target, text, x + 1, y + 1, shadow, opts.clone());
        self.print_to(target, text, x, y, colour, opts)
    }
}

/// Render `text` onto `target` using the supplied `font`. Free-function
/// variant of [`ConsoleHelper::print_to`] for callers that already hold a
/// `&RgbaImage` font reference (e.g. when split-borrowing the console's
/// font and output_image at the same time).
pub fn print_to_with_font<C: Canvas>(
    font: &RgbaImage,
    target: &mut C,
    text: &str,
    x: i32,
    y: i32,
    colour: C::Pixel,
    opts: PrintOptions,
) -> i32 {
    let mut max_width = 0;
    let mut dx = x;
    let mut dy = y;
    for char in text.chars() {
        match char as u8 {
            10 => {
                dx = x;
                dy += 6;
            }
            32 => {
                dx += if opts.small_text { 3 } else { 4 };
            }
            0 => {}
            _ => {
                let glyph = if opts.small_text {
                    (char as u8 + 128) as char
                } else {
                    char
                };
                let width = draw_letter_to(font, target, glyph, dx, dy, colour);
                dx += width + 1;
            }
        }
        max_width = max_width.max(dx - x);
    }
    let _ = dy;
    max_width
}

/// Draw one 8×8 glyph from `font` onto `target` at (`x`, `y`) using `colour`
/// for every non-transparent font pixel. Returns the visual width of the
/// glyph (rightmost non-transparent column + 1).
fn draw_letter_to<C: Canvas>(
    font: &RgbaImage,
    target: &mut C,
    char: char,
    x: i32,
    y: i32,
    colour: C::Pixel,
) -> i32 {
    let char_index = char as u8 as usize;
    let glyph_x = (char_index % 16) * 8;
    let glyph_y = (char_index / 16) * 8;
    let target_w = target.width() as i32;
    let target_h = target.height() as i32;
    let mut letter_width = 0;
    for j in 0..8 {
        for i in 0..8 {
            let font_index = (glyph_x + i as usize) + (glyph_y + j as usize) * 128;
            if font.alpha_at_index(font_index) == 0 {
                continue;
            }
            letter_width = letter_width.max(i + 1);
            let px = x + i;
            let py = y + j;
            if px < 0 || py < 0 || px >= target_w || py >= target_h {
                continue;
            }
            target.set_pixel(px as u32, py as u32, colour);
        }
    }
    letter_width
}

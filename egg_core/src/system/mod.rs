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

/// IO + asset surface used by `egg_core`. Drawing is no longer done through
/// this trait — see `DrawState`, the `Canvas` trait, and `print_to_with_font`.
/// What stays here is input, audio, persistent memory, asset access, and
/// the final `output_image()` surface that consoles composite into.
pub trait ConsoleApi {
    // Input + memory
    fn get_gamepads(&mut self) -> &mut [u8; 4];
    fn get_mouse(&mut self) -> &mut MouseInput;
    fn memory(&mut self) -> &mut EggMemory;
    fn get_sprite_flags(&mut self) -> &mut [u8];

    fn btn(&self, index: i32) -> bool;
    fn btnp(&self, index: i32, hold: i32, period: i32) -> bool;
    fn exit(&mut self);
    fn key(&self, scancode: ScanCode) -> bool;
    fn keyp(&self, scancode: ScanCode, hold: i32, period: i32) -> bool;
    /// Latest character entered by the user this frame (for text entry).
    fn key_chars(&self) -> &[char];
    fn mouse(&self) -> MouseInput;

    // Audio + IO
    fn music(&mut self, track: Option<&MusicTrack>);
    fn sfx(&mut self, sfx_id: &str, opts: SfxOptions);
    fn sync(&mut self, mask: i32, bank: u8, to_cart: bool);
    fn trace_alloc(text: impl AsRef<str>, color: u8);

    // Per-frame state helpers
    fn sync_helper(&mut self) -> &mut SyncHelper;
    fn rng(&mut self) -> &mut Lcg64Xsh32;
    fn previous_gamepad(&mut self) -> &mut [u8; 4];
    fn previous_mouse(&mut self) -> &mut MouseInput;

    // Asset access. Maps + indexed sprites also live on DrawState; these
    // accessors exist for asset-loading and a few non-draw queries (collider
    // generation, layer collision checks).
    fn maps(&mut self) -> &mut Vec<GameMap>;
    fn map_get(&self, bank: usize, layer: usize, x: i32, y: i32) -> usize;
    fn map_set(&mut self, bank: usize, layer: usize, x: i32, y: i32, value: usize);
    fn write_file(&mut self, filename: String, data: &[u8]);
    fn read_file(&mut self, filename: String) -> Option<&[u8]>;
    /// Grab a whole bitmap. By convention:
    ///
    /// 0. Screen
    /// 1. OVR layer
    /// 2. Indexed sprites
    /// 3. RGBA sprites
    fn get_bitmap_indexed(&self, id: usize) -> &[u8];

    /// Canonical final surface composited by gamestate draw fns each frame.
    fn output_image(&mut self) -> &mut RgbaImage;

    /// Default 8x8 font (16 chars per row) used by `print_to_with_font`.
    fn font(&self) -> &RgbaImage;

    /// Called by `EggState::run` at the start of each frame, before any
    /// drawing happens. Implementations should clear `output_image()` (and
    /// any legacy scratch surfaces) so the gamestate draw paths start each
    /// frame on a clean canvas.
    fn frame_start(&mut self);
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
    fn get_pmem(&mut self, address: usize) -> u8 {
        let address = address.min(1023);
        self.memory().memory[address]
    }
    fn set_pmem(&mut self, address: usize, value: u8) {
        let address = address.min(1023);
        self.memory().memory[address] = value;
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

/// Measure the maximum line width of `text` rendered with `font`. Equivalent
/// to a print_to dry-run that doesn't touch a real target. Useful for
/// centering / wrapping.
pub fn text_width(font: &RgbaImage, text: &str, opts: PrintOptions) -> i32 {
    let mut throwaway = crate::system::image::IndexedImage::new(1, 1);
    print_to_with_font(font, &mut throwaway, text, 0, 0, 0u8, opts)
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

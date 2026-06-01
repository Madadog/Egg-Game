use crate::{
    data::sound::{SfxData, music::MusicTrack},
    rand::Lcg64Xsh32,
    system::{drawing::Canvas, image::RgbaImage},
};

pub use consts::*;
pub use font::*;
pub use scancode::*;
pub use types::*;

pub mod consts;
pub mod drawing;
pub mod font;
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
    fn trace_alloc(text: impl AsRef<str>, color: u8);

    // Per-frame state helpers
    fn bank(&mut self) -> &mut u8;
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

    /// Default 8×8 bitmap [`Font`] used by `print_to_with_font` and text
    /// measurement. The font caches each glyph's width so text can be
    /// measured without rasterising to a throwaway canvas.
    fn font(&self) -> &Font;

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

    fn text_width(&self, text: &str, opts: PrintOptions) -> i32 {
        text_width(self.font(), text, opts)
    }
}

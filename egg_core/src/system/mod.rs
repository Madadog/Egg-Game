use crate::{
    data::{
        save::SaveData,
        sound::{SfxData, music::MusicTrack},
    },
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
    /// The four gamepads, mirroring [`ConsoleApi::mouse`]. Each [`Controller`]
    /// holds `[current, previous]` per button; read edges with the shared
    /// [`pressed`]/[`just_pressed`] helpers. See [`ConsoleHelper::controller`]
    /// for the single-player shorthand.
    fn controllers(&self) -> &[Controller; 4];
    fn memory(&mut self) -> &mut SaveData;
    fn get_sprite_flags(&mut self) -> &mut [u8];

    fn exit(&mut self);
    fn key(&self, scancode: ScanCode) -> bool;
    fn keyp(&self, scancode: ScanCode) -> bool;
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
}

impl<T: ConsoleApi> ConsoleHelper for T {}

pub trait ConsoleHelper: ConsoleApi {
    // Helper functions
    fn play_sound(&mut self, sfx_data: SfxData) {
        self.sfx(sfx_data.id, sfx_data.options);
    }
    /// Player one's [`Controller`], mirroring [`ConsoleApi::mouse`]. Returns a
    /// copy; read it with the shared [`pressed`]/[`just_pressed`] helpers, e.g.
    /// `just_pressed(system.controller().a)`.
    fn controller(&self) -> Controller {
        self.controllers()[0]
    }
    /// Returns true if any button on any controller was just pressed this
    /// frame. Ignores button releases.
    fn any_btnp(&self) -> bool {
        self.controllers().iter().any(Controller::any_just_pressed)
    }
    /// Returns true if any button on any controller was pressed or released.
    fn any_btnpr(&self) -> bool {
        self.controllers().iter().any(Controller::changed)
    }
    /// Reset all persisted save data to its default (fresh-game) state.
    fn reset_save_data(&mut self) {
        *self.memory() = SaveData::default();
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

use crate::{
    data::sound::{SfxData, music::MusicTrack},
    render::{
        Canvas, Font, PrintOptions, image::RgbaImage, print_to_with_font, text_width,
    },
};

pub use audio::*;
pub use consts::*;
pub use input::*;
pub use scancode::*;

pub mod audio;
pub mod consts;
pub mod input;
pub mod scancode;

/// IO + asset surface used by `egg_core`. Drawing is no longer done through
/// this trait — see `DrawState`, the `Canvas` trait, and `print_to_with_font`.
/// What stays here is input, audio, asset access, and the final
/// `output_image()` surface that consoles composite into. Persistent progress
/// is no longer a hardware concern: `SaveData` is game state (on `EggState`),
/// flushed through the string-named file store below.
pub trait ConsoleApi {
    // Input
    /// The four gamepads, mirroring [`ConsoleApi::mouse`]. Each [`Controller`]
    /// holds `[current, previous]` per button; read edges with the shared
    /// [`pressed`]/[`just_pressed`] helpers. See [`ConsoleHelper::controller`]
    /// for the single-player shorthand.
    fn controllers(&self) -> &[Controller; 4];

    fn exit(&mut self);
    fn key(&self, scancode: ScanCode) -> bool;
    fn keyp(&self, scancode: ScanCode) -> bool;
    /// Edge-or-repeat: like [`keyp`](Self::keyp) on the initial press, then again
    /// while held — `delay` fixed steps before the first repeat, then every
    /// `rate`. Default = no repeat (the press edge only); the real console
    /// overrides it with the held-key timing from `EggInput`.
    fn key_repeat(&self, scancode: ScanCode, _delay: u16, _rate: u16) -> bool {
        self.keyp(scancode)
    }
    /// Latest character entered by the user this frame (for text entry).
    fn key_chars(&self) -> &[char];
    fn mouse(&self) -> MouseInput;

    /// Read / write the host clipboard (for the text editor's copy/cut/paste).
    /// Default: no clipboard — `get` is `None`, `set` is a no-op — so a minimal
    /// console needs nothing. The real console backs it with an app-local string
    /// (shared across windows); OS-clipboard interop is a future host concern.
    fn clipboard_get(&mut self) -> Option<String> {
        None
    }
    fn clipboard_set(&mut self, _text: &str) {}

    // Audio
    fn music(&mut self, track: Option<&MusicTrack>);
    fn sfx(&mut self, sfx_id: &str, opts: SfxOptions);
    /// The names of every available music track (file stems under
    /// `assets/music/`), for the editor's track picker. Default: none — a host
    /// without a scannable music directory (e.g. headless/web) reports an empty
    /// set, which is fine since the map stores the chosen track by name anyway.
    fn music_tracks(&self) -> Vec<String> {
        Vec::new()
    }

    // Asset access.
    /// Persist `bytes` to the host's string-named file store. `path` is a
    /// relative, forward-slash path (e.g. `maps/office.tmj`) — the engine
    /// names files, the host decides where they really live (under its data
    /// root). Hosts without writable storage may log and drop the write.
    fn write_file(&mut self, path: &str, bytes: &[u8]);

    /// Read back a file from the host's string-named file store (same namespace
    /// as [`write_file`](Self::write_file)). `None` when the file doesn't exist
    /// or the host has no readable storage for that path.
    fn read_file(&mut self, path: &str) -> Option<Vec<u8>>;

    /// Canonical final surface composited by gamestate draw fns each frame.
    fn output_image(&mut self) -> &mut RgbaImage;

    /// Current screen/framebuffer size in pixels. Defaults to the base
    /// [`WIDTH`]/[`HEIGHT`]; a host whose framebuffer can grow (e.g. the
    /// "mirror window size" mode) overrides these to the live dimensions.
    /// Engine code reads these instead of the consts so content re-centres at
    /// any resolution.
    fn width(&self) -> i32 {
        WIDTH
    }
    fn height(&self) -> i32 {
        HEIGHT
    }

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

    /// Render `text` onto `target` using the console's default font
    /// (`self.font()`). `colour` is the pixel value (RGBA, palette index, …)
    /// used for non-transparent font pixels — the font itself is read as
    /// alpha-only. To measure text without drawing it, use
    /// [`text_width`](Self::text_width).
    fn print_to<C: Canvas>(
        &self,
        target: &mut C,
        text: &str,
        x: i32,
        y: i32,
        colour: C::Pixel,
        opts: PrintOptions,
    ) {
        print_to_with_font(self.font(), target, text, x, y, colour, opts);
    }

    /// Render `text` horizontally centred on `x`. Measures with
    /// [`text_width`](Self::text_width) rather than a throwaway off-screen draw.
    fn print_to_centered<C: Canvas>(
        &self,
        target: &mut C,
        text: &str,
        x: i32,
        y: i32,
        colour: C::Pixel,
        opts: PrintOptions,
    ) {
        let width = self.text_width(text, opts.clone());
        self.print_to(target, text, x - width / 2, y, colour, opts);
    }

    #[allow(clippy::too_many_arguments)]
    fn print_to_shadow<C: Canvas>(
        &self,
        target: &mut C,
        text: &str,
        x: i32,
        y: i32,
        colour: C::Pixel,
        shadow: C::Pixel,
        opts: PrintOptions,
    ) {
        self.print_to(target, text, x + 1, y + 1, shadow, opts.clone());
        self.print_to(target, text, x, y, colour, opts);
    }

    fn text_width(&self, text: &str, opts: PrintOptions) -> i32 {
        text_width(self.font(), text, opts)
    }
}

#[cfg(test)]
pub mod test_console {
    //! A minimal in-memory [`ConsoleApi`] for unit tests. Every method returns
    //! an inert default (no input, no audio), which is all the engine's
    //! pure-logic tests need from the hardware surface, plus an in-memory
    //! `files` store so the save-flush/load methods are testable. Shared across
    //! the crate's `#[cfg(test)]` modules so they don't each re-stub the trait.

    use std::collections::HashMap;

    use crate::data::sound::music::MusicTrack;
    use crate::platform::{Controller, MouseInput, ScanCode, SfxOptions};
    use crate::render::Font;
    use crate::render::image::{IndexedImage, RgbaImage};

    use super::ConsoleApi;

    /// Inert console used by tests. Holds just enough state to satisfy the
    /// trait (output/font) plus an `indexed_sprites` sheet some tests hand to
    /// sheet-reading helpers like [`crate::world::map::map_by_name`] — those read the
    /// sheet directly now (it lives on `DrawState`), not through the console.
    pub struct TestConsole {
        pub controllers: [Controller; 4],
        /// In-memory stand-in for the host's string-named file store, so tests
        /// can drive [`ConsoleApi::write_file`]/[`read_file`](ConsoleApi::read_file)
        /// (e.g. the engine's autosave round trip).
        pub files: HashMap<String, Vec<u8>>,
        /// App-local clipboard, mirroring the real console, so the text editor's
        /// copy/cut/paste round-trips are testable.
        pub clipboard: Option<String>,
        /// A blank sprite sheet fixture: enough for collider-deriving helpers
        /// to read any low tile id.
        pub indexed_sprites: IndexedImage,
        pub output: RgbaImage,
        pub font: Font,
    }

    impl TestConsole {
        pub fn new() -> Self {
            Self {
                controllers: [Controller::default(); 4],
                files: HashMap::new(),
                clipboard: None,
                // One blank 256px-wide sheet row block: enough for the
                // modern-map collider derivation to read any low tile id.
                indexed_sprites: IndexedImage::new(256, 64),
                output: RgbaImage::new(1, 1),
                font: Font::blank(),
            }
        }
    }

    impl Default for TestConsole {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ConsoleApi for TestConsole {
        fn controllers(&self) -> &[Controller; 4] {
            &self.controllers
        }
        fn exit(&mut self) {}
        fn key(&self, _scancode: ScanCode) -> bool {
            false
        }
        fn keyp(&self, _scancode: ScanCode) -> bool {
            false
        }
        fn key_chars(&self) -> &[char] {
            &[]
        }
        fn mouse(&self) -> MouseInput {
            MouseInput::default()
        }
        fn clipboard_get(&mut self) -> Option<String> {
            self.clipboard.clone()
        }
        fn clipboard_set(&mut self, text: &str) {
            self.clipboard = Some(text.to_string());
        }
        fn music(&mut self, _track: Option<&MusicTrack>) {}
        fn sfx(&mut self, _sfx_id: &str, _opts: SfxOptions) {}
        fn write_file(&mut self, path: &str, bytes: &[u8]) {
            self.files.insert(path.to_string(), bytes.to_vec());
        }
        fn read_file(&mut self, path: &str) -> Option<Vec<u8>> {
            self.files.get(path).cloned()
        }
        fn output_image(&mut self) -> &mut RgbaImage {
            &mut self.output
        }
        fn font(&self) -> &Font {
            &self.font
        }
    }
}

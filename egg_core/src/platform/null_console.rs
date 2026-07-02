//! A production headless [`ConsoleApi`] — an inert host surface for stepping the
//! game without real hardware: muted audio, swallowed writes, no readable files,
//! and a throwaway draw surface. Stepping a cloned world through it advances the
//! sim deterministically without touching audio or the save file. Used by the
//! cutscene scrubber's frame-by-frame re-simulation today, and available to any
//! future headless stepping (tests, replay).
//!
//! Input is not a console concern — a whole frame's input is threaded in as data
//! (see [`EggInput`](crate::platform::EggInput) and [`Ctx::input`](crate::Ctx::input)),
//! so this console carries no input state at all; a headless caller supplies its
//! own [`EggInput`] (the scrubber keeps a neutral held-`A` one, see
//! [`CutsceneScrubber`](crate::gamestate::CutsceneScrubber)). Text rendering no
//! longer goes through the console (the font is `EggState` data, threaded via
//! [`Ctx::font`](crate::Ctx::font)), so it carries no font either — that
//! decoupling is what makes it inert.

use super::{ConsoleApi, SfxOptions};
use crate::data::sound::music::MusicTrack;
use crate::render::image::RgbaImage;

/// An inert, muted, fileless console for headless stepping.
#[derive(Debug)]
pub struct NullConsole {
    /// A throwaway compositing surface. A headless caller either doesn't draw or
    /// draws its ghost frames through the *real* console, so nothing meaningful is
    /// rendered here; it only exists to satisfy [`ConsoleApi::output_image`].
    output: RgbaImage,
}

impl NullConsole {
    /// A fresh console: muted audio, a 1×1 scratch surface (headless stepping
    /// advances the world, it doesn't draw through here).
    pub fn new() -> Self {
        Self {
            output: RgbaImage::new(1, 1),
        }
    }
}

impl Default for NullConsole {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleApi for NullConsole {
    fn exit(&mut self) {}
    fn music(&mut self, _track: Option<&MusicTrack>) {}
    fn sfx(&mut self, _sfx_id: &str, _opts: SfxOptions) {}
    /// Swallowed: headless stepping must never flush a save or write any asset
    /// (the scrubber's re-sim clones `SaveData`, so progress can't leak out).
    fn write_file(&mut self, _path: &str, _bytes: &[u8]) {}
    fn read_file(&mut self, _path: &str) -> Option<Vec<u8>> {
        None
    }
    fn output_image(&mut self) -> &mut RgbaImage {
        &mut self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_console_is_inert() {
        let mut c = NullConsole::new();
        // No file store, muted audio, a scratch surface — nothing that could
        // leak into real state or panic.
        assert_eq!(c.read_file("anything"), None, "no readable files");
        c.write_file("x", b"y"); // swallowed, must not panic
        c.music(None); // muted, must not panic
        let _ = c.output_image();
    }
}

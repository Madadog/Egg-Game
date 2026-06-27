//! A headless [`ConsoleApi`] for the cutscene scrubber's re-simulation: neutral
//! input, muted audio, no file IO, and a throwaway draw surface. Stepping a
//! cloned world through this console advances the sim deterministically without
//! touching real hardware, input, audio, or the save file — the property the
//! scrubber's frame-by-frame replay relies on.
//!
//! Modeled on the test-only console, but this is production code: the scrubber
//! ships in the editor. Text rendering no longer goes through the console (the
//! font is `EggState` data, threaded via [`Ctx::font`](crate::Ctx::font)), so
//! this shim carries no font at all — that decoupling is what makes it inert.

use super::{ConsoleApi, Controller, MouseInput, ScanCode, SfxOptions};
use crate::data::sound::music::MusicTrack;
use crate::render::image::RgbaImage;

/// A neutral, muted, fileless console for deterministic cutscene re-simulation.
/// [`controllers`](Self::controllers) is public so the scrubber can inject input
/// — e.g. hold `A` to auto-advance a `dialogue` step instead of stalling the
/// timeline on it (a `dialogue` beat waits for `A`, which neutral input never
/// supplies).
#[derive(Debug)]
pub struct ScrubConsole {
    /// Injected gamepad state — all buttons released by default, so an
    /// `interruptible` scene never self-cancels and `B` never skips.
    pub controllers: [Controller; 4],
    /// A throwaway compositing surface. The scrubber draws each ghost frame
    /// through the *real* console, so nothing meaningful is rendered here; it
    /// only exists to satisfy [`ConsoleApi::output_image`].
    output: RgbaImage,
}

impl ScrubConsole {
    /// A fresh neutral console: released input, muted audio, a 1×1 scratch
    /// surface (the re-sim steps the world, it doesn't draw through here).
    pub fn new() -> Self {
        Self {
            controllers: [Controller::default(); 4],
            output: RgbaImage::new(1, 1),
        }
    }
}

impl Default for ScrubConsole {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleApi for ScrubConsole {
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
    fn music(&mut self, _track: Option<&MusicTrack>) {}
    fn sfx(&mut self, _sfx_id: &str, _opts: SfxOptions) {}
    /// Swallowed: the re-sim must not flush a save or write any asset (the
    /// scrubber clones `SaveData`, so progress can't leak out).
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
    use crate::platform::input::pressed;

    #[test]
    fn scrub_console_is_inert_and_injectable() {
        let mut c = ScrubConsole::new();
        // Neutral by default: no keys, no input, no file store.
        assert!(!c.key(ScanCode::Z));
        assert!(!c.keyp(ScanCode::Z));
        assert!(c.key_chars().is_empty());
        assert!(!pressed(c.controllers()[0].a), "released by default");
        assert_eq!(c.read_file("anything"), None, "no file store");
        c.write_file("x", b"y"); // swallowed, must not panic
        c.music(None); // muted, must not panic
        let _ = c.output_image();
        // The scrubber can inject input (e.g. hold A to auto-advance dialogue).
        c.controllers[0].a = [true, false];
        assert!(pressed(c.controllers()[0].a), "injected input reads back");
    }
}

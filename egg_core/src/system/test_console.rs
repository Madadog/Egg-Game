//! A minimal in-memory [`ConsoleApi`] for unit tests. Every method returns an
//! inert default (no input, no audio, no real file store), which is all the
//! engine's pure-logic tests need from the hardware surface. Shared across the
//! crate's `#[cfg(test)]` modules so they don't each re-stub the trait.

use crate::data::save::SaveData;
use crate::data::script::Script;
use crate::data::sound::music::MusicTrack;
use crate::system::drawing::image::{IndexedImage, RgbaImage};
use crate::system::{Controller, Font, MouseInput, ScanCode, SfxOptions};

use super::ConsoleApi;

/// Inert console used by tests. Holds just enough state to satisfy the trait
/// (memory/script/output/font) plus an `indexed_sprites` sheet some tests hand
/// to sheet-reading helpers like [`crate::map::map_by_name`] — those read the
/// sheet directly now (it lives on `DrawState`), not through the console.
pub struct TestConsole {
    pub controllers: [Controller; 4],
    pub memory: SaveData,
    /// A blank sprite sheet fixture: enough for collider-deriving helpers to
    /// read any low tile id.
    pub indexed_sprites: IndexedImage,
    pub script: Script,
    pub output: RgbaImage,
    pub font: Font,
}

impl TestConsole {
    pub fn new() -> Self {
        Self {
            controllers: [Controller::default(); 4],
            memory: SaveData::default(),
            // One blank 256px-wide sheet row block: enough for the modern-map
            // collider derivation to read any low tile id.
            indexed_sprites: IndexedImage::new(256, 64),
            script: Script::new(),
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
    fn memory(&mut self) -> &mut SaveData {
        &mut self.memory
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
    fn script(&self) -> &Script {
        &self.script
    }
    fn script_mut(&mut self) -> &mut Script {
        &mut self.script
    }
    fn write_file(&mut self, _path: &str, _bytes: &[u8]) {}
    fn output_image(&mut self) -> &mut RgbaImage {
        &mut self.output
    }
    fn font(&self) -> &Font {
        &self.font
    }
}

use std::collections::HashMap;

use bevy::prelude::{Image, info};
use egg_core::{
    data::{save::SaveData, sound::music::MusicTrack},
    gamestate::EggInput,
    system::{
        ConsoleApi, Controller, Font, HEIGHT, MouseInput, ScanCode,
        SfxOptions,
        WIDTH,
        drawing::image::{IndexedImage, RgbaImage},
    },
};

// TODO:
// Load interactables from tiled maps
// Separate BG & FG palettes, upgrade BGs.
// Serialize save/load state, use structs, remove bits.
// Dialogue hashmap
// Make UI actually work: Hierarchical layout, compositional widgets.
// Unified walkaround collision space
// Yolkomatic

// TODO:
// Turn `Creature` into normal entities
// Remove Static{name} from everything, load data from files like a sane program.
// Level editor that serialises to json
// Dialogue dsl and previewer
// Cutscene editor
// Resizable screen

pub struct FantasyConsole {
    pub output_screen: RgbaImage,
    pub font: Font,
    music: Option<(MusicTrack, bool)>,
    memory: SaveData,
    sounds: HashMap<String, SfxOptions>,
    input: EggInput,
}

impl FantasyConsole {
    pub fn new() -> Self {
        Self {
            output_screen: RgbaImage::new(WIDTH as u32, HEIGHT as u32),
            font: Font::blank(),
            music: None,
            sounds: HashMap::new(),
            memory: SaveData::default(),
            input: EggInput::new(),
        }
    }
    pub fn input(&mut self) -> &mut EggInput {
        &mut self.input
    }
    pub fn sounds(&mut self) -> &mut HashMap<String, SfxOptions> {
        &mut self.sounds
    }
    pub fn music_track(&mut self) -> &mut Option<(MusicTrack, bool)> {
        &mut self.music
    }
    /// A snapshot of the current persistent save data, for the autosave system
    /// to diff against the last value written to disk.
    pub fn save_data(&self) -> SaveData {
        self.memory.clone()
    }
    pub fn blit_to_image(&self, image: &mut [u8]) {
        // Gamestate draw fns composite directly into output_screen each frame.
        image.copy_from_slice(self.output_screen.data());
    }
    /// Reallocate the final screen surface to `w`×`h` (no-op if already that
    /// size). Used by "mirror window" mode, where the framebuffer follows the
    /// window. Must stay in lock-step with [`DrawState::resize`] (the layer
    /// canvases) and the host's presentation texture, since
    /// [`blit_to_image`](Self::blit_to_image) copies `output_screen` verbatim.
    pub fn resize_screen(&mut self, w: u32, h: u32) {
        if self.output_screen.width() != w || self.output_screen.height() != h {
            self.output_screen = RgbaImage::new(w, h);
        }
    }
    pub fn set_font(&mut self, font: &Image) {
        assert!(font.size().x == 128);
        assert!(font.size().y >= 128);
        for (i, c) in self
            .font
            .image_mut()
            .data_mut()
            .iter_mut()
            .zip(font.data.iter().flatten())
        {
            *i = *c;
        }
        self.font.refresh();
    }

    /// Convert a Bevy RGBA sprite-sheet `Image` into the engine's [`RgbaImage`].
    /// Host-side asset plumbing: the result is stored on
    /// [`DrawState`](egg_core::drawstate::DrawState), the single owner of the
    /// sheets, by `load_assets`.
    pub fn sprites_from_image(sheet: &Image) -> RgbaImage {
        RgbaImage::from_vec(
            sheet
                .data
                .as_ref()
                .expect("Tried to load uninitialised spritesheet.")
                .clone(),
            sheet.size().x,
            sheet.size().y,
        )
    }
    /// Convert an RGBA sprite sheet to indexed form by matching each pixel
    /// against `palette`. Pixels that don't match a palette entry become
    /// index 0. Host-side: the palette-matching policy is the host's, and the
    /// result is stored on [`DrawState`](egg_core::drawstate::DrawState).
    pub fn indexed_sprites_from_image(sheet: &Image, palette: &[[u8; 3]]) -> IndexedImage {
        let width = sheet.size().x as usize;
        let height = sheet.size().y as usize;
        let mut data = Vec::with_capacity(width * height);
        'outer: for pixel in sheet
            .data
            .as_ref()
            .expect("Tried to read uninitialised image.")
            .chunks_exact(4)
        {
            for (i, colour) in palette.iter().enumerate() {
                if pixel[0] == colour[0] && pixel[1] == colour[1] && pixel[2] == colour[2] {
                    data.push(i.try_into().unwrap());
                    continue 'outer;
                }
            }
            data.push(0);
        }
        IndexedImage::from_vec(data, width, height)
    }
}

impl ConsoleApi for FantasyConsole {
    fn controllers(&self) -> &[Controller; 4] {
        &self.input.controllers
    }

    fn memory(&mut self) -> &mut SaveData {
        &mut self.memory
    }

    fn exit(&mut self) {
        panic!("Perfectly normal shutdown.")
    }

    fn key(&self, scancode: ScanCode) -> bool {
        self.input.key(scancode)
    }

    fn keyp(&self, scancode: ScanCode) -> bool {
        self.input.keyp(scancode)
    }

    fn key_chars(&self) -> &[char] {
        self.input.key_chars()
    }

    fn mouse(&self) -> MouseInput {
        self.input.mouse
    }

    fn music(&mut self, track: Option<&MusicTrack>) {
        info!("Playing track \"{:?}\"", track);
        if let Some(track) = track {
            self.music = Some((track.clone(), false));
        } else {
            self.music = None;
        }
    }

    fn sfx(&mut self, sfx_id: &str, opts: egg_core::system::SfxOptions) {
        self.sounds.insert(sfx_id.to_string(), opts);
    }

    /// Write `path` under `assets/`, backing up any existing file to
    /// `<path>.bak` first. The engine only hands over relative forward-slash
    /// paths; anything absolute or escaping the data root is refused.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_file(&mut self, path: &str, bytes: &[u8]) {
        use std::path::{Component, Path};
        let relative = Path::new(path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| matches!(c, Component::ParentDir))
        {
            info!("write_file: refusing non-relative path {path:?}");
            return;
        }
        let dest = Path::new("assets").join(relative);
        if dest.exists() {
            let backup = format!("{}.bak", dest.display());
            if let Err(e) = std::fs::copy(&dest, &backup) {
                info!("write_file: backup of {} failed: {e}", dest.display());
            }
        }
        match std::fs::write(&dest, bytes) {
            Ok(()) => info!("Saved {} ({} bytes)", dest.display(), bytes.len()),
            Err(e) => info!("write_file: failed to write {}: {e}", dest.display()),
        }
    }
    /// Web build: no filesystem, so file writes are logged and dropped.
    #[cfg(target_arch = "wasm32")]
    fn write_file(&mut self, path: &str, bytes: &[u8]) {
        info!("File write not persisted on web ({path}, {} bytes)", bytes.len());
    }

    fn output_image(&mut self) -> &mut RgbaImage {
        &mut self.output_screen
    }

    /// Live framebuffer size — tracks `output_screen`, which grows to match the
    /// window in "mirror" mode. Engine code reads these so content re-centres.
    fn width(&self) -> i32 {
        self.output_screen.width() as i32
    }
    fn height(&self) -> i32 {
        self.output_screen.height() as i32
    }

    fn font(&self) -> &Font {
        &self.font
    }
}

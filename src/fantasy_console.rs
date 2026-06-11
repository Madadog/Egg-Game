use std::collections::HashMap;

use bevy::prelude::{Image, info};
use egg_core::{
    data::sound::music::MusicTrack,
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
    sounds: HashMap<String, SfxOptions>,
    input: EggInput,
    /// Set by [`ConsoleApi::exit`]; a host system polls
    /// [`exit_requested`](Self::exit_requested) and sends `AppExit`. (No engine
    /// caller exercises this yet — it replaces the old shutdown `panic!`.)
    exit_requested: bool,
}

impl FantasyConsole {
    pub fn new() -> Self {
        Self {
            output_screen: RgbaImage::new(WIDTH as u32, HEIGHT as u32),
            font: Font::blank(),
            music: None,
            sounds: HashMap::new(),
            input: EggInput::new(),
            exit_requested: false,
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
    /// Whether the engine has asked to quit (via [`ConsoleApi::exit`]). The host
    /// translates this into a Bevy `AppExit`.
    pub fn exit_requested(&self) -> bool {
        self.exit_requested
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

    fn exit(&mut self) {
        self.exit_requested = true;
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

    /// Route a write by namespace (see [`is_user_data`]): user-data (the save)
    /// goes to the host's user-data backend, everything else to the authoring
    /// `assets/` tree.
    ///
    /// * **user data, native** — the file `save.json` in the working directory,
    ///   rewritten in place (no `.bak`: it changes constantly).
    /// * **assets, native** — `assets/<path>`, backing up any existing file to
    ///   `<path>.bak` first. The engine only hands over relative forward-slash
    ///   paths; anything absolute or escaping the data root is refused.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_file(&mut self, path: &str, bytes: &[u8]) {
        if is_user_data(path) {
            if let Err(e) = std::fs::write(path, bytes) {
                info!("Failed to write save file {path}: {e}");
            }
            return;
        }
        let Some(dest) = asset_path(path) else {
            info!("write_file: refusing non-relative path {path:?}");
            return;
        };
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
    /// Web build: user data is a `localStorage` entry keyed by the path string
    /// (so the historical `"save.json"` key keeps working byte-for-byte); other
    /// writes have no filesystem to land in, so they're logged and dropped.
    #[cfg(target_arch = "wasm32")]
    fn write_file(&mut self, path: &str, bytes: &[u8]) {
        if is_user_data(path) {
            let json = String::from_utf8_lossy(bytes);
            if let Some(storage) = local_storage()
                && let Err(e) = storage.set_item(path, &json)
            {
                info!("Failed to write save to localStorage: {e:?}");
            }
            return;
        }
        info!("File write not persisted on web ({path}, {} bytes)", bytes.len());
    }

    /// Read a file back from the same namespaces [`write_file`](Self::write_file)
    /// writes to. User data (the save) comes from the host's user-data backend;
    /// other paths from `assets/<path>`. `None` on a missing file or an
    /// unreadable/refused path.
    #[cfg(not(target_arch = "wasm32"))]
    fn read_file(&mut self, path: &str) -> Option<Vec<u8>> {
        if is_user_data(path) {
            if !std::path::Path::new(path).exists() {
                return None;
            }
            return match std::fs::read(path) {
                Ok(bytes) => Some(bytes),
                Err(e) => {
                    info!("Failed to read save file {path}: {e}");
                    None
                }
            };
        }
        let dest = asset_path(path)?;
        std::fs::read(&dest).ok()
    }
    /// Web build: user data is read from `localStorage`; nothing else is
    /// readable (no filesystem), so other paths return `None`.
    #[cfg(target_arch = "wasm32")]
    fn read_file(&mut self, path: &str) -> Option<Vec<u8>> {
        if is_user_data(path) {
            return match local_storage()?.get_item(path) {
                Ok(json) => json.map(String::into_bytes),
                Err(e) => {
                    info!("Failed to read save from localStorage: {e:?}");
                    None
                }
            };
        }
        None
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

/// Routing rule for the string-named file store: the engine names files, the
/// host decides where they live. The save (and anything under `save/`) is the
/// player's *user data*, persisted to a per-user backend; everything else is
/// authoring/asset data under the `assets/` tree.
fn is_user_data(path: &str) -> bool {
    path == egg_core::data::save::SAVE_PATH || path.starts_with("save/")
}

/// Validate an asset-namespace `path` and resolve it under `assets/`. The
/// engine only hands over relative forward-slash paths; anything absolute or
/// escaping the data root is refused (`None`).
#[cfg(not(target_arch = "wasm32"))]
fn asset_path(path: &str) -> Option<std::path::PathBuf> {
    use std::path::{Component, Path};
    let relative = Path::new(path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return None;
    }
    Some(Path::new("assets").join(relative))
}

/// The browser's `localStorage`, or `None` if it's unavailable (e.g. disabled
/// by the user or accessed from a non-browser context).
#[cfg(target_arch = "wasm32")]
fn local_storage() -> Option<web_sys::Storage> {
    match web_sys::window()?.local_storage() {
        Ok(storage) => storage,
        Err(e) => {
            info!("localStorage unavailable: {e:?}");
            None
        }
    }
}

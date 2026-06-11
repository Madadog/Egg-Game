use std::collections::HashMap;

use bevy::prelude::{Image, info};
use egg_core::{
    data::{save::SaveData, script::Script, sound::music::MusicTrack},
    gamestate::EggInput,
    rand::Lcg64Xsh32,
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
    // sprites + indexed_sprites + sprite_flags also live on
    // EggState::draw_state. These copies are kept around for the asset
    // loaders and Collider::from_sprite reads via get_bitmap_indexed.
    pub sprites: RgbaImage,
    pub indexed_sprites: IndexedImage,
    pub sprite_flags: Vec<u8>,
    /// UI labels + dialogue, loaded from `script/<lang>.eggtext`.
    script: Script,
    /// A language requested at runtime via `set_language`, awaiting load by the
    /// host's asset loop (see `take_pending_language`).
    pending_language: Option<String>,
    music: Option<(MusicTrack, bool)>,
    memory: SaveData,
    sounds: HashMap<String, SfxOptions>,
    input: EggInput,
    rng: Lcg64Xsh32,
}

impl FantasyConsole {
    pub fn new() -> Self {
        let mut x = Self {
            output_screen: RgbaImage::new(WIDTH as u32, HEIGHT as u32),
            font: Font::blank(),
            sprites: RgbaImage::new(1, 1),
            indexed_sprites: IndexedImage::new(1, 1),
            script: Script::new(),
            pending_language: None,
            sprite_flags: vec![0; 2048],
            music: None,
            sounds: HashMap::new(),
            memory: SaveData::default(),
            input: EggInput::new(),
            rng: Lcg64Xsh32::default(),
        };
        let mut spr_flags = String::from(
            "00100000000000000000000000000000000000801000000000000000002020000010101010500000001000000000000000101030101000000000001010000000101010002000000000301010400000001000100000400010500000000000000010101010108020100000000000101010203000301080302000000000001010101010100000100010001010100000000010001000001000100010100000000000000000000010101030303010000010100000000000000000000000002030203000000000000000000000101010400000000000000010000000203010102000100000000000000000000000000010101000000000000000100010a060b0101020",
        );
        spr_flags.push_str("00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
        spr_flags.push_str("000000001010101000000000000000000070601010700000000000000000000010000000001000000000000000000000601010606060000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000100000000000000000000000000000000010100000000000000000000000001010101000000000000000700000302030200000000000000000d0006000");
        spr_flags.push_str("00000000101010100000000000000000000000200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000010001000000000000000000000000000100010000000000000000000000010001010100000000000000000000000000000000000000000000000000000000000001010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
        x.load_sprite_flags(&spr_flags);
        x
    }
    pub fn load_sprite_flags(&mut self, string: &str) {
        for i in 0..string.len() / 2 {
            let (char1, char2) = (
                string.chars().nth(i * 2).unwrap(),
                string.chars().nth(i * 2 + 1).unwrap(),
            );
            let mut string = String::new();
            string.push(char2);
            string.push(char1);
            let flag = u8::from_str_radix(&string, 16).unwrap();
            let (x, y) = (i % 16, i / 16);
            let index = x + y * 32;
            self.sprite_flags[index] = flag;
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
    /// Take any language requested at runtime via [`ConsoleApi::set_language`],
    /// for the host's asset loop to load and apply.
    pub fn take_pending_language(&mut self) -> Option<String> {
        self.pending_language.take()
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

    pub fn set_sprites(&mut self, sheet: &Image) {
        self.sprites = RgbaImage::from_vec(
            sheet
                .data
                .as_ref()
                .expect("Tried to load uninitialised spritesheet.")
                .clone(),
            sheet.size().x,
            sheet.size().y,
        );
    }
    /// Convert an RGBA sprite sheet to indexed form by matching each pixel
    /// against `palette`. Pixels that don't match a palette entry become
    /// index 0.
    pub fn set_indexed_sprites(&mut self, sheet: &Image, palette: &[[u8; 3]]) {
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
        self.indexed_sprites = IndexedImage::from_vec(data, width, height);
    }
}

impl ConsoleApi for FantasyConsole {
    fn controllers(&self) -> &[Controller; 4] {
        &self.input.controllers
    }

    fn memory(&mut self) -> &mut SaveData {
        &mut self.memory
    }

    fn get_sprite_flags(&mut self) -> &mut [u8] {
        self.sprite_flags.as_mut_slice()
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

    fn rng(&mut self) -> &mut egg_core::rand::Lcg64Xsh32 {
        &mut self.rng
    }

    fn script(&self) -> &Script {
        &self.script
    }
    fn script_mut(&mut self) -> &mut Script {
        &mut self.script
    }
    fn set_language(&mut self, language: &str) {
        self.pending_language = Some(language.to_string());
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
    fn get_bitmap_indexed(&self, id: usize) -> &[u8] {
        match id {
            2 => &self.indexed_sprites.data,
            _ => panic!("bitmap {id} does not exist"),
        }
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

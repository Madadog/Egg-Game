//! The console host: the engine's [`ConsoleApi`] implementation
//! ([`FantasyConsole`]) plus the Bevy systems that present its output — the
//! screen framebuffer (camera + screen sprite, blit, Fit/Mirror resizing) and
//! the audio (sfx/music playback). The console produces a pixel buffer and a
//! sound/music queue each frame; everything here turns that into what the player
//! sees and hears, so the visual-present and audio-present systems live together
//! as [`ConsolePlugin`]. The cross-Bevy→engine input conversion
//! ([`crate::keycode_to_scancode`]) is the one console-I/O helper that stays in
//! `main.rs`, next to its sole caller (`step_state`).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::platform::collections::HashMap as BevyHashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use egg_core::data::sound::music::MusicTrack;
use egg_core::platform::{
    ConsoleApi, Controller, EggInput, HEIGHT, MouseInput, ScanCode, SfxOptions, WIDTH,
};
use egg_core::render::Font;
use egg_core::render::image::{IndexedImage, RgbaImage};

use crate::{EggGame, ScaleMode};

pub struct FantasyConsole {
    pub output_screen: RgbaImage,
    pub font: Font,
    music: Option<(MusicTrack, bool)>,
    /// Available music tracks, keyed by name (file stem), discovered from
    /// `assets/music/` at construction. Drives the editor's track picker and
    /// validates a map's requested track. Empty where the dir can't be scanned
    /// (web), in which case any requested track plays as-is.
    music_registry: HashMap<String, MusicTrack>,
    sounds: HashMap<String, SfxOptions>,
    input: EggInput,
    /// App-local clipboard for the text editor's copy/cut/paste. Shared across all
    /// windows (one console), but not wired to the OS clipboard yet.
    clipboard: String,
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
            music_registry: scan_music_dir(),
            sounds: HashMap::new(),
            input: EggInput::new(),
            clipboard: String::new(),
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
        // The bundled font is 128 wide and >=128 tall; a replaced asset of another
        // size shouldn't hard-crash at boot. Ignore it with a warning instead.
        if font.size().x != 128 || font.size().y < 128 {
            bevy::log::warn!(
                "set_font: ignoring font sized {}x{} (expected width 128, height >= 128)",
                font.size().x,
                font.size().y
            );
            return;
        }
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
    /// [`DrawState`](egg_core::draw_state::DrawState), the single owner of the
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
    /// result is stored on [`DrawState`](egg_core::draw_state::DrawState).
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

    fn key_repeat(&self, scancode: ScanCode, delay: u16, rate: u16) -> bool {
        self.input.key_repeat(scancode, delay, rate)
    }

    fn key_chars(&self) -> &[char] {
        self.input.key_chars()
    }

    fn mouse(&self) -> MouseInput {
        self.input.mouse
    }

    fn clipboard_get(&mut self) -> Option<String> {
        (!self.clipboard.is_empty()).then(|| self.clipboard.clone())
    }
    fn clipboard_set(&mut self, text: &str) {
        self.clipboard = text.to_string();
    }

    fn music(&mut self, track: Option<&MusicTrack>) {
        info!("Playing track \"{:?}\"", track);
        match track {
            // Only play a track the music dir actually has (an unknown name — a
            // typo or removed file — is a silent no-op, like a dangling warp).
            // Where the dir can't be scanned the registry is empty, so play as-is.
            Some(track)
                if self.music_registry.is_empty()
                    || self.music_registry.contains_key(&*track.id) =>
            {
                self.music = Some((track.clone(), false));
            }
            Some(track) => {
                info!("music: no track named {:?} in assets/music", track.id);
                self.music = None;
            }
            None => self.music = None,
        }
    }

    fn music_tracks(&self) -> Vec<String> {
        let mut names: Vec<String> = self.music_registry.keys().cloned().collect();
        names.sort();
        names
    }

    fn sfx(&mut self, sfx_id: &str, opts: egg_core::platform::SfxOptions) {
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
        // Create the parent directory if it doesn't exist yet (e.g. the editor's
        // `config/` for the dock layout, or a fresh `maps/` for a new map).
        if let Some(parent) = dest.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            info!("write_file: mkdir {} failed: {e}", parent.display());
        }
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
        info!(
            "File write not persisted on web ({path}, {} bytes)",
            bytes.len()
        );
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
}

/// Routing rule for the string-named file store: the engine names files, the
/// host decides where they live. The save (and anything under `save/`) is the
/// player's *user data*, persisted to a per-user backend; everything else is
/// authoring/asset data under the `assets/` tree.
fn is_user_data(path: &str) -> bool {
    path == egg_core::data::save::SAVE_PATH || path.starts_with("save/")
}

/// Discover the music tracks under `assets/music/` — one [`MusicTrack`] per
/// `.ogg`, keyed by file stem (the name a map's `music` property stores and the
/// host loads as `music/<stem>.ogg`). Native scans the directory; web has no
/// filesystem to scan, so it reports none (the editor's picker is a native
/// authoring tool, and a map plays its stored name regardless).
#[cfg(not(target_arch = "wasm32"))]
fn scan_music_dir() -> HashMap<String, MusicTrack> {
    let mut tracks = HashMap::new();
    let Ok(entries) = std::fs::read_dir("assets/music") else {
        return tracks;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("ogg")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            tracks.insert(stem.to_string(), MusicTrack::named(stem));
        }
    }
    tracks
}

#[cfg(target_arch = "wasm32")]
fn scan_music_dir() -> HashMap<String, MusicTrack> {
    HashMap::new()
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

/// Console output presentation: turns the console's per-frame framebuffer and
/// sound queue into what the player sees and hears.
///
/// Registers:
/// * `Startup`: [`setup`] (spawns the 2D camera and the main screen sprite).
/// * `Update`: [`resize_screen`] (reconcile the framebuffer/sprite with the
///   window + screen mode).
///
/// The per-fixed-step presentation systems ([`play_sounds`], [`play_music`],
/// [`update_texture`]) are deliberately *not* added here: they are tail members
/// of the single ordered `FixedUpdate` chain (`step_state → update_views →
/// play_sounds → play_music → update_texture`), which spans domains and is
/// assembled as one `.chain()` by `CorePlugin` in `main.rs` to keep that strict
/// ordering. The [`SfxAssets`] resource they read is inserted by the asset
/// loader (`setup_assets`).
pub struct ConsolePlugin;

impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(Update, resize_screen);
    }
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn(Camera2d);
    let screen_handle = images.add(new_screen_image(WIDTH as u32, HEIGHT as u32));
    commands.spawn((
        Sprite {
            image: screen_handle.clone(),
            ..default()
        },
        Transform::from_xyz(0., 0., 0.),
        GameScreenSprite,
    ));
}

#[derive(Debug, Resource)]
pub struct SfxAssets {
    pub sounds: BevyHashMap<String, Handle<AudioSource>>,
}
impl SfxAssets {
    /// Load every sound effect under `assets/sfx/`, keyed by file stem (the name
    /// an engine `#sound`/`sfx` cue refers to). The set is discovered from the
    /// directory — the same single-source-of-truth approach as
    /// [`scan_music_dir`] — so adding a `.ogg` makes it playable without editing
    /// the host. Web has no filesystem to scan, so it falls back to loading the
    /// names declared in [`egg_core::data::sound`] by path (handles still resolve
    /// against the bundled `assets/` on web).
    pub fn new(assets: &AssetServer) -> Self {
        let load = |stem: &str| -> (String, Handle<AudioSource>) {
            (stem.to_string(), assets.load(format!("sfx/{stem}.ogg")))
        };
        let sounds = sfx_stems().iter().map(|s| load(s)).collect();
        Self { sounds }
    }
}

/// The set of sound-effect names to load, by file stem. Native discovers them
/// from `assets/sfx/` (so the directory is the single source of truth); web has
/// no filesystem to scan and falls back to the names declared in
/// [`egg_core::data::sound`].
#[cfg(not(target_arch = "wasm32"))]
fn sfx_stems() -> Vec<String> {
    let mut stems = Vec::new();
    let Ok(entries) = std::fs::read_dir("assets/sfx") else {
        return egg_core::data::sound::SFX_IDS
            .iter()
            .map(|s| s.to_string())
            .collect();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("ogg")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            stems.push(stem.to_string());
        }
    }
    stems
}

#[cfg(target_arch = "wasm32")]
fn sfx_stems() -> Vec<String> {
    egg_core::data::sound::SFX_IDS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Standard audio playback at the game's mixing volume.
fn playback_settings(mode: bevy::audio::PlaybackMode, speed: f32) -> PlaybackSettings {
    PlaybackSettings {
        mode,
        volume: bevy::audio::Volume::Decibels(-6.0),
        speed,
        paused: false,
        ..Default::default()
    }
}

pub fn play_sounds(
    mut commands: Commands,
    game_assets: Res<SfxAssets>,
    mut state: ResMut<EggGame>,
) {
    for (name, options) in state.system.sounds() {
        if let Some(sound) = game_assets.sounds.get(&name.to_string()) {
            let speed =
                2.0_f32.powf((options.note as f32 + (options.octave as f32 - 5.0) * 12.0) / 12.0);
            commands.spawn((
                AudioPlayer(sound.clone()),
                playback_settings(bevy::audio::PlaybackMode::Despawn, speed),
            ));
        } else {
            // An unknown name — a `#sound` typo, or a sound added without its
            // `.ogg` — is logged and skipped, never fatal. Mirrors the
            // silent-no-op resilience of `music()` and dangling warps.
            warn!("sfx: no sound named {name:?} in assets/sfx (cue {options:?})");
        }
    }
    state.system.sounds().clear();
}

pub fn play_music(
    mut commands: Commands,
    mut query: Query<(Entity, &mut AudioSink), With<MusicPlayer>>,
    mut state: ResMut<EggGame>,
    assets: Res<AssetServer>,
) {
    if let Some((x, playing)) = state.system.music_track() {
        if query.is_empty() && !*playing {
            let music: Handle<AudioSource> = assets.load(format!("music/{}.ogg", x.id));
            commands.spawn((
                AudioPlayer(music.clone()),
                playback_settings(bevy::audio::PlaybackMode::Loop, x.speed),
                MusicPlayer,
            ));
            *playing = true;
        }
    } else {
        for (entity, sink) in query.iter_mut() {
            commands.entity(entity).despawn();
            sink.stop();
        }
    }
}

#[derive(Component)]
pub struct MusicPlayer;

#[derive(Component)]
pub struct GameScreenSprite;

pub fn update_texture(
    state: ResMut<EggGame>,
    mut images: ResMut<Assets<Image>>,
    mut border_colour: ResMut<ClearColor>,
    sprite: Query<&Sprite, With<GameScreenSprite>>,
) {
    for sprite in sprite.iter() {
        state.system.blit_to_image(
            images
                .get_mut(&sprite.image)
                .unwrap()
                .data
                .as_mut()
                .expect("Main screen texture uninitialized, can't draw game."),
        );
    }
    // Use the current default palette's first colour for the border surround.
    if let Some(colour) = state.state.draw_state.palettes[0].first() {
        border_colour.0 = Color::srgb_u8(colour[0], colour[1], colour[2]);
    }
}

/// Smallest framebuffer Mirror mode will allocate, so a tiny window or a large
/// scale factor can't produce a degenerate (or zero-sized) screen.
pub const MIN_FB_W: u32 = 64;
pub const MIN_FB_H: u32 = 48;

/// A fresh black RGBA screen texture of `width`×`height`. Shared by [`setup`] and
/// the resize path so the format/usages always match.
pub fn new_screen_image(width: u32, height: u32) -> Image {
    Image::new_fill(
        Extent3d {
            width,
            height,
            ..default()
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::all(),
    )
}

/// Integer/linear scale factor that fits the framebuffer into `window` (Fit mode).
pub fn screen_scale(window: &Window, mode: &ScaleMode) -> f32 {
    let fit = (window.width() / WIDTH as f32).min(window.height() / HEIGHT as f32);
    match mode {
        // Never floor to 0 — a window smaller than the base resolution would
        // otherwise scale the screen out of existence.
        ScaleMode::Integer => fit.floor().max(1.0),
        ScaleMode::Linear => fit,
    }
}

/// Reconcile the framebuffer with the window size, then scale the screen sprite
/// so it fills the window. The framebuffer stays at the fixed base resolution
/// and the sprite scales to fit the window.
fn resize_screen(
    mut sprite: Query<(&Sprite, &mut Transform), With<GameScreenSprite>>,
    mut window: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut game: ResMut<EggGame>,
) {
    // The main framebuffer follows the PRIMARY window only; extra view windows
    // are sized independently by `views::resize_views`.
    let Ok(mut window) = window.single_mut() else {
        return;
    };
    window.resolution.set_scale_factor_override(Some(1.0));
    let Ok((sprite, mut transform)) = sprite.single_mut() else {
        return;
    };

    let target = (WIDTH as u32, HEIGHT as u32);
    let scale = screen_scale(&window, &game.scale_mode);

    // Resize the three lock-step buffers (console screen, draw layers, GPU
    // texture) together, only when the size actually changes — `blit_to_image`
    // copies the screen verbatim, so all three must match.
    if (game.system.width() as u32, game.system.height() as u32) != target {
        let g = &mut *game;
        g.system.resize_screen(target.0, target.1);
        g.state.draw_state.resize(target.0, target.1);
        if let Some(image) = images.get_mut(&sprite.image) {
            *image = new_screen_image(target.0, target.1);
        }
    }

    transform.scale = Vec3::new(scale, scale, 1.0);
}

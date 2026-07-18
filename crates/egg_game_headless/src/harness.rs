//! Headless CLI harness: boot the game to a chosen state, drive it with scripted
//! input, and write PNG screenshots — no window, no Bevy `App`, no real disk
//! writes. The point is a scriptable, deterministic way for an agent (or a
//! person) to *see* what a change does: `egg_game_headless …` loads the same
//! assets the windowed game does, runs the identical per-frame funnel
//! ([`crate::run_frame`], shared verbatim with the Bevy host's `EggGame::run`),
//! and captures the framebuffer through [`RgbaImage::encode_png`].
//!
//! Native-only (the whole module is `#[cfg(not(target_arch = "wasm32"))]` at its
//! `mod` site): it reads bundled assets off disk and decodes PNGs with the
//! `image` crate, neither of which the web build wants. [`crate::run_frame`] is
//! not gated — the web host calls it.
//!
//! Determinism is the contract: the console is a pure in-memory
//! [`HeadlessConsole`] (writes never touch disk — they land in a `HashMap` and a
//! log, reads fall back to the read-only `assets/` tree), the RNG is seeded to a
//! fixed constant unless `--seed` overrides it, and neutral input is empty, so
//! the same command line produces the same pixels every run.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use egg_core::EggState;
use egg_core::data::save::SAVE_PATH;
use egg_core::data::sound::music::MusicTrack;
use egg_core::gamestate::GameMode;
use egg_core::geometry::Vec2;
use egg_core::platform::{ConsoleApi, EggInput, HEIGHT, ScanCode, SfxOptions, WIDTH};
use egg_core::rand::Lcg64Xsh32;
use egg_core::render::Font;
use egg_core::render::image::RgbaImage;
use egg_editor::map::MapViewer;

use crate::run_frame;

/// The map a `--map`-less run starts on — the new-game start map
/// (`WalkaroundState::new_game`), so the default is "the beginning".
const DEFAULT_MAP: &str = "bedroom";

/// Fixed RNG seed for a run without `--seed`, so screenshots are reproducible.
/// Distinct from [`Lcg64Xsh32::default`] only in that `--seed N` reuses this
/// stream (`stream` = PCG's default) with `N` as the state.
const DEFAULT_STREAM: u64 = 0x0a02_bdbf_7bb3_c0a7;

const USAGE: &str = "\
egg_game_headless — boot the game headless, script input, capture PNGs

USAGE:
    egg_game_headless [OPTIONS]

OPTIONS:
    --map NAME        Start on this map (default: bedroom). Errors listing the
                      loaded maps if NAME is unknown.
    --pos X,Y         Place the player at map-pixel (X, Y) and frame the camera
                      there.
    --flag NAME       Set a save flag after boot (repeatable), e.g. --flag is_night.
    --seed N          Seed the RNG (default: a fixed constant, so runs are
                      deterministic).
    --save FILE       Pre-seed the in-memory save store with FILE's bytes before
                      the first frame (loaded like the real save.json).
    --frames N        Run N frames before the screenshot (default: 60). Ignored
                      with --script.
    --out FILE        Screenshot path for the no-script run (default:
                      headless_shot.png). Ignored with --script.
    --script FILE     Run an input script (see below); --frames/--out are ignored
                      and shots land in --out-dir.
    --out-dir DIR     Directory for a script's `shot` commands (default: .).
    --assets DIR      Read bundled game assets from DIR instead of auto-detecting
                      (tries ./assets, then ../../assets).
    --editor          Open the map editor overlay (like pressing L) before frame 1.
    --list-maps       Print the loaded map names and exit.
    --check           Cross-reference the data web (dialogue/cutscene/map/
                      portrait/sound/preset/flag references), lint every
                      script/*.eggtext language overlay against the base
                      script's skeleton, and print a report; exits nonzero
                      iff it found any error.
    --help            Print this help and exit.

SCRIPT (line-based; blank lines and `#` comments skipped):
    wait N                 advance N frames with neutral input
    hold <btn> N           hold controller button <btn> for N frames
    press <btn>            hold <btn> for one frame
    key <name>             tap keyboard key <name> for one frame
    type TEXT              type TEXT (one frame)
    mouse X Y              move the cursor to (X, Y) (one frame)
    click X Y              move to (X, Y) and hold left for one frame
    editor on|off          toggle the map editor overlay (no frame)
    shot NAME              write <out-dir>/NAME.png now (no frame)
  <btn>: up down left right a b x y
  <name>: a-z, 0-9, arrow keys (up/down/left/right), escape, return, space, tab,
          backspace, delete, insert, home, end, pageup, pagedown, f1-f12, and the
          punctuation scancodes (minus, equals, comma, period, slash, …).";

/// The harness entry, the headless binary's `main`. Parses the command line,
/// boots the assets, sets the requested state, then either runs a fixed frame
/// count or an input script, writing PNGs and a run summary. Exits the process
/// directly on a usage error (code 2) or a fatal boot/IO error (code 1); returns
/// normally on success.
pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    if args.help {
        println!("{USAGE}");
        return;
    }

    // Resolve the asset root once (a `--assets` override, else auto-detect), so
    // boot and the console both read from the same tree.
    let root = resolve_asset_root(args.assets.as_deref());

    let mut state = EggState::default();
    if let Err(e) = boot(&mut state, &root) {
        eprintln!("fatal: {e}");
        std::process::exit(1);
    }

    if args.list_maps {
        let mut names = state.maps.names();
        names.sort();
        for name in names {
            println!("{name}");
        }
        return;
    }

    if args.check {
        let mut console = HeadlessConsole::with_root(root.clone());
        std::process::exit(run_check(&mut state, &mut console, &root));
    }

    let mut console = HeadlessConsole::with_root(root);
    // `--save` pre-seeds the in-memory store so the one-time save load below
    // picks it up — exactly as the real console would serve save.json.
    if let Some(path) = &args.save {
        match std::fs::read(path) {
            Ok(bytes) => {
                console.files.insert(SAVE_PATH.to_string(), bytes);
            }
            Err(e) => {
                eprintln!("fatal: --save {path}: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(seed) = args.seed {
        state.rng = Lcg64Xsh32::new(seed, DEFAULT_STREAM);
    }

    // The one-time data + save load (and inventory rehydrate) that
    // `EggState::run`'s first frame performs, done up front so `--flag` edits
    // below survive it: a pre-seeded save would otherwise be read *over* them on
    // frame 1, wiping the flags. Doing the read here makes frame 1's guarded
    // load a no-op, so the sequence stays single-shot exactly as in `run`.
    state.load_data(&mut console);
    if state.load_save(&mut console) {
        state
            .walkaround
            .load_inventory(&state.save.inventory, &state.items);
    }
    for flag in &args.flags {
        state.save.set_flag(flag, true);
    }

    // Force walkaround (the default gamestate is the intro animation) and load
    // the requested map through the tested path, mirroring the new-game route.
    state.enter(GameMode::Walkaround);
    let map = args.map.as_deref().unwrap_or(DEFAULT_MAP);
    if !state.maps.contains(map) {
        let mut names = state.maps.names();
        names.sort();
        eprintln!("fatal: no map named `{map}`. Loaded maps: {names:?}");
        std::process::exit(1);
    }
    let input = EggInput::new();
    {
        let mut ctx = egg_core::Ctx {
            draw: &mut state.draw_state,
            system: &mut console,
            input: &input,
            maps: &mut state.maps,
            rng: &mut state.rng,
            script: &state.script,
            scenes: &state.scenes,
            save: &mut state.save,
            items: &state.items,
            presets: &state.presets,
            font: &state.font,
        };
        state.walkaround.load_map_by_name(&mut ctx, map);
    }
    if let Some((x, y)) = args.pos {
        state.walkaround.player().pos = Vec2::new(x, y);
    }
    let player_pos = state.walkaround.player_ref().pos;
    state.walkaround.center_camera_on(player_pos, WIDTH, HEIGHT);

    let mut runner = Runner {
        state,
        console,
        input,
        map_viewer: MapViewer::default(),
        mouse: (0, 0),
        frames: 0,
        shots: 0,
    };
    runner.map_viewer.focused = args.editor;

    let out_dir = PathBuf::from(args.out_dir.as_deref().unwrap_or("."));
    if let Some(script_path) = &args.script {
        let text = match std::fs::read_to_string(script_path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("fatal: --script {script_path}: {e}");
                std::process::exit(1);
            }
        };
        let commands = match parse_script(&text) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("fatal: {e}");
                std::process::exit(1);
            }
        };
        for cmd in &commands {
            if let Err(e) = runner.exec(cmd, &out_dir) {
                eprintln!("fatal: {e}");
                std::process::exit(1);
            }
        }
    } else {
        for _ in 0..args.frames.unwrap_or(60) {
            runner.frame(|_| {});
        }
        let out = PathBuf::from(args.out.as_deref().unwrap_or("headless_shot.png"));
        if let Err(e) = runner.shot(&out) {
            eprintln!("fatal: failed to write {}: {e}", out.display());
            std::process::exit(1);
        }
        println!("shot: {}", out.display());
        runner.shots += 1;
    }

    let writes = &runner.console.written;
    let write_note = if writes.is_empty() {
        String::new()
    } else {
        format!(", console writes captured (in memory, not disk): {writes:?}")
    };
    println!(
        "done: {} frame(s) run, {} shot(s) written{write_note}",
        runner.frames, runner.shots,
    );
}

/// The in-memory, disk-isolated [`ConsoleApi`] the harness steps the game
/// through. Audio and exit are inert; the framebuffer is a fixed-size surface
/// gamestate composites into; and the string-named file store is a `HashMap`, so
/// a save flush (or any engine write) is captured in memory and logged, never
/// written to the real `assets/` tree the windowed host would touch.
struct HeadlessConsole {
    output: RgbaImage,
    /// In-memory stand-in for the host's file store. Writes land here only.
    files: HashMap<String, Vec<u8>>,
    /// Every path [`write_file`](ConsoleApi::write_file) was handed, in order —
    /// surfaced in the run summary so a test/agent can see what the game tried to
    /// persist without anything reaching disk.
    written: Vec<String>,
    /// Where read fallbacks resolve bundled assets from (see
    /// [`resolve_asset_root`]). Held so a read and the run's boot agree on the
    /// tree, and so the crate's own tests can point at `../../assets`.
    asset_root: PathBuf,
}

impl HeadlessConsole {
    /// A console rooted at the auto-detected asset tree — used by the tests,
    /// which run with the crate directory as CWD. (Runtime always resolves the
    /// root explicitly and calls [`with_root`](Self::with_root), so this is
    /// test-only.)
    #[cfg(test)]
    fn new() -> Self {
        Self::with_root(resolve_asset_root(None))
    }
    /// A console whose read fallbacks resolve under `asset_root`.
    fn with_root(asset_root: PathBuf) -> Self {
        Self {
            output: RgbaImage::new(WIDTH as u32, HEIGHT as u32),
            files: HashMap::new(),
            written: Vec::new(),
            asset_root,
        }
    }
}

impl ConsoleApi for HeadlessConsole {
    fn exit(&mut self) {}
    fn music(&mut self, _track: Option<&MusicTrack>) {}
    fn sfx(&mut self, _sfx_id: &str, _opts: SfxOptions) {}

    /// Capture the write in memory (and log its path). Never touches disk — the
    /// whole reason the harness uses its own console rather than the windowed
    /// host's, whose native `write_file` rewrites real files.
    fn write_file(&mut self, path: &str, bytes: &[u8]) {
        self.written.push(path.to_string());
        self.files.insert(path.to_string(), bytes.to_vec());
    }

    /// Read the in-memory store first (so a `--save` pre-seed or a write made
    /// this run wins), then fall back to the read-only on-disk `assets/` tree
    /// (how `data/data.toml` and any bundled asset resolve). A path that is
    /// absolute or escapes the data root is refused, mirroring the real console;
    /// the save (`SAVE_PATH`, not under `assets/`) resolves to a non-existent
    /// `assets/save.json`, so it only loads when `--save` pre-seeded it.
    fn read_file(&mut self, path: &str) -> Option<Vec<u8>> {
        if let Some(bytes) = self.files.get(path) {
            return Some(bytes.clone());
        }
        read_asset(&self.asset_root, path)
    }

    fn output_image(&mut self) -> &mut RgbaImage {
        &mut self.output
    }
}

/// Resolve the `assets/` root the harness reads bundled game data from. A
/// `cargo run -p egg_game_headless` invocation keeps the repo root as CWD (so
/// `./assets` is right there), but the crate's own tests run with CWD =
/// `crates/egg_game_headless`, where the tree is two levels up. An explicit
/// `--assets DIR` override wins unconditionally (a bad path then fails loudly at
/// read time rather than silently falling back); otherwise try `assets`, then
/// `../../assets`, and take the first that exists.
fn resolve_asset_root(override_dir: Option<&str>) -> PathBuf {
    if let Some(dir) = override_dir {
        return PathBuf::from(dir);
    }
    for candidate in ["assets", "../../assets"] {
        if Path::new(candidate).exists() {
            return PathBuf::from(candidate);
        }
    }
    PathBuf::from("assets")
}

/// Validate an asset-namespace `path` and resolve it under `root` — the same
/// rule the real console's `asset_path` applies (no absolute paths, no `..`
/// climbing out of the data root). The validation is on the engine's relative
/// path; `root` only picks which tree it lands in.
fn asset_path(root: &Path, path: &str) -> Option<PathBuf> {
    let rel = Path::new(path);
    if rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    Some(root.join(rel))
}

/// Read a bundled asset off disk (read-only), or `None` if it's missing or the
/// path is refused.
fn read_asset(root: &Path, path: &str) -> Option<Vec<u8>> {
    std::fs::read(asset_path(root, path)?).ok()
}

/// Decode PNG `bytes` into the engine's [`RgbaImage`] with the `image` crate.
///
/// A *different* decoder from the hand-written [`RgbaImage::encode_png`] the
/// harness screenshots with — which is exactly what makes the round-trip test
/// (encode there, decode here) a genuine independent oracle. `to_rgba8` yields
/// straight 8-bit RGBA with no premultiplication, so a fully-transparent but
/// coloured pixel keeps its colour channels, matching the encoder byte-for-byte.
///
/// Public beyond this crate: it's the Bevy host's PNG-decode seam too. The host
/// links Bevy's async asset loader for *initial* sheet load but has no
/// equivalent for a hot-reload triggered by an mtime poll outside that loader —
/// rather than duplicate the `image`-crate call, its hot-reload path (see
/// `src/hot_reload.rs`) calls straight through here.
pub fn decode_png(bytes: &[u8]) -> Result<RgbaImage, String> {
    let rgba = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Ok(RgbaImage::from_vec(rgba.into_raw(), w, h))
}

/// Build the engine [`Font`] from a decoded 128×(≥128) font atlas, copying the
/// pixels exactly as the console's `set_font` does (a blank 128×128 font zipped
/// against the source bytes, so a taller sheet is truncated to the first 128
/// rows).
fn build_font(img: &RgbaImage) -> Result<Font, String> {
    let (w, h) = (img.width(), img.height());
    if w != 128 || h < 128 {
        return Err(format!("font sized {w}x{h} (expected width 128, height >= 128)"));
    }
    let mut font = Font::blank();
    for (dst, s) in font.image_mut().data_mut().iter_mut().zip(img.data().iter()) {
        *dst = *s;
    }
    font.refresh();
    Ok(font)
}

/// The `.tmj` file stems under `<root>/maps/`, sorted for determinism — the
/// directory scan `boot` uses in place of a manifest, mirroring the windowed
/// host's native `fantasy_console::map_stems`. A missing/unreadable maps
/// directory is fatal (like the font): unlike an individual bad map, there's no
/// sensible way to boot with none at all.
fn map_stems(root: &Path) -> Result<Vec<String>, String> {
    let dir = root.join("maps");
    let entries =
        std::fs::read_dir(&dir).map_err(|e| format!("reading maps dir {}: {e}", dir.display()))?;
    let mut stems = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // `.extension()` only matches the last dot, so `office_backup.tmj.bak`
        // reads as extension `bak` and is excluded automatically.
        if path.extension().and_then(|e| e.to_str()) == Some("tmj")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            stems.push(stem.to_string());
        }
    }
    stems.sort();
    Ok(stems)
}

/// The `.eggtext` file stems under `<root>/script/`, excluding the base
/// language (`en`, loaded separately by [`load_script_file`]) — each is a
/// language overlay `--check` lints against the base script's skeleton (see
/// [`egg_core::data::validate::check_overlay`]). Mirrors [`map_stems`], but a
/// missing/unreadable `script` dir is not fatal here: `load_script_file`
/// already requires `en.eggtext` to exist, so an empty result (no overlays
/// yet shipped) is a perfectly normal outcome, not an error.
fn script_overlay_stems(root: &Path) -> Vec<String> {
    let dir = root.join("script");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut stems = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("eggtext")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && stem != "en"
        {
            stems.push(stem.to_string());
        }
    }
    stems.sort();
    stems
}

/// Boot the game's assets from disk into `state`, no Bevy `App`: the maps
/// (discovered from `<root>/maps/*.tmj` — see [`map_stems`] — with image
/// layers attached), the font, the sprite sheet (RGBA + indexed), the dialogue
/// script, and the cutscene registry. Mirrors `load_assets`' phase 2
/// conversions exactly. A missing/broken essential (the maps dir, font, sheet,
/// script, scenes) is fatal; an individual map or image layer that fails is
/// warned and skipped, so one bad map can't wedge boot.
fn boot(state: &mut EggState, root: &Path) -> Result<(), String> {
    let names = map_stems(root)?;

    let mut loaded = 0usize;
    for name in &names {
        let Some(bytes) = read_asset(root, &format!("maps/{name}.tmj")) else {
            eprintln!("warning: skipping map `{name}` (missing maps/{name}.tmj)");
            continue;
        };
        let mut map = match egg_core::data::tiled::from_json(&bytes) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("warning: skipping map `{name}` (parse error: {e})");
                continue;
            }
        };
        // Image paths are authored relative to the map file (which lives in
        // `maps/`); resolve each under it and attach its decoded pixels, exactly
        // as `attach_map_images` does in the windowed loader.
        let rel_paths: Vec<String> = map.image_layer_paths().iter().map(|s| s.to_string()).collect();
        for rel in rel_paths {
            let Some(img_bytes) = read_asset(root, &format!("maps/{rel}")) else {
                eprintln!("warning: image layer `{rel}` for map `{name}` missing; layer has no pixels");
                continue;
            };
            match decode_png(&img_bytes) {
                // The decoded RgbaImage *is* the layer's pixels — attach it.
                Ok(pixels) => map.attach_image(&rel, pixels),
                Err(e) => {
                    eprintln!("warning: image layer `{rel}` for map `{name}` failed to decode: {e}")
                }
            }
        }
        state.maps.insert(name.clone(), map);
        loaded += 1;
    }
    eprintln!("loaded {loaded}/{} map(s)", names.len());

    let font_bytes = read_asset(root, "fonts/tic80_font.png").ok_or("missing assets/fonts/tic80_font.png")?;
    let font_img = decode_png(&font_bytes).map_err(|e| format!("font decode: {e}"))?;
    state.set_font(build_font(&font_img)?);

    let sheet_bytes = read_asset(root, "sprites/sheet.png").ok_or("missing assets/sprites/sheet.png")?;
    let sheet_img = decode_png(&sheet_bytes).map_err(|e| format!("sheet decode: {e}"))?;
    let palette = state.draw_state.palettes[0].clone();
    // The decoded sheet is the RGBA form; derive the indexed form from it with
    // the engine's palette-matching policy, then hand both to DrawState (the
    // single owner of the sheets).
    let indexed = sheet_img.to_indexed(&palette);
    state.draw_state.rgba_sprites = sheet_img;
    state.draw_state.indexed_sprites = indexed;

    let script_file = load_script_file(root)?;
    state.script.set_base(script_file, &state.portraits);

    let scene_file = load_scene_file(root)?;
    state.set_scenes(scene_file);

    Ok(())
}

/// Read + parse `script/en.eggtext` under `root` into a raw [`ScriptFile`].
/// Shared by [`boot`] (which resolves and installs it) and `--check` (which
/// wants the raw, pre-resolution tree to cross-reference — see
/// [`run_check`]), so the two never drift on how the script is loaded.
fn load_script_file(root: &Path) -> Result<egg_core::data::script::ScriptFile, String> {
    let script_bytes = read_asset(root, "script/en.eggtext").ok_or("missing assets/script/en.eggtext")?;
    let script_text = std::str::from_utf8(&script_bytes).map_err(|e| format!("script utf8: {e}"))?;
    egg_core::data::script::eggtext::parse(script_text).map_err(|e| format!("script parse: {e}"))
}

/// Read + parse `data/main.eggscene` under `root` into a [`SceneFile`].
/// Shared by [`boot`] and `--check`, mirroring [`load_script_file`].
fn load_scene_file(root: &Path) -> Result<egg_core::data::scene::SceneFile, String> {
    let scene_bytes = read_asset(root, "data/main.eggscene").ok_or("missing assets/data/main.eggscene")?;
    let scene_text = std::str::from_utf8(&scene_bytes).map_err(|e| format!("scenes utf8: {e}"))?;
    egg_core::data::scene::parse(scene_text).map_err(|e| format!("scenes parse: {e}"))
}

/// Run [`egg_core::data::validate::check`] over the assets under `root`, then
/// [`egg_core::data::validate::check_overlay`] over every language overlay
/// under `script/` (see [`script_overlay_stems`]) against the base script,
/// and print the combined report. `state` must already be booted (for its
/// map store) — this additionally runs `load_data` on it to pick up the
/// *live* `data/data.toml` (presets/portraits), rather than the binary's
/// compiled-in defaults, so `--check` always reports on the tree it read
/// maps/script/scenes from, `--assets` override included. Returns the
/// process exit code: 0 if the report has no errors (warnings don't fail the
/// run), 1 otherwise.
fn run_check(state: &mut EggState, console: &mut HeadlessConsole, root: &Path) -> i32 {
    state.load_data(console);

    let script_file = match load_script_file(root) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("fatal: {e}");
            return 1;
        }
    };
    let scene_file = match load_scene_file(root) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("fatal: {e}");
            return 1;
        }
    };

    let mut maps = std::collections::BTreeMap::new();
    for name in state.maps.names() {
        if let Some(map) = state.maps.get(name) {
            maps.insert(name.to_string(), map.parse_objects());
        }
    }

    let mut report = egg_core::data::validate::check(
        &script_file,
        &scene_file,
        &maps,
        &state.portraits,
        &state.presets,
        egg_core::data::validate::ENGINE_DIALOGUE_ROOTS,
    );

    // Lint every language overlay under `script/` (besides the base `en`)
    // against the base script's skeleton — see `script_overlay_stems`.
    for lang in script_overlay_stems(root) {
        let overlay_bytes = match read_asset(root, &format!("script/{lang}.eggtext")) {
            Some(bytes) => bytes,
            None => continue,
        };
        let overlay_text = match std::str::from_utf8(&overlay_bytes) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("fatal: script/{lang}.eggtext utf8: {e}");
                return 1;
            }
        };
        let overlay_file = match egg_core::data::script::eggtext::parse(overlay_text) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("fatal: script/{lang}.eggtext parse: {e}");
                return 1;
            }
        };
        let overlay_report = egg_core::data::validate::check_overlay(&script_file, &overlay_file, &lang);
        report.errors.extend(overlay_report.errors);
        report.warnings.extend(overlay_report.warnings);
    }

    print!("{report}");
    if report.is_clean() {
        println!("clean: 0 error(s), {} warning(s)", report.warnings.len());
        0
    } else {
        eprintln!("FAILED: {} error(s), {} warning(s)", report.errors.len(), report.warnings.len());
        1
    }
}

/// The live harness: the booted engine, the disk-isolated console, one reused
/// per-window input, the headless map editor, and the counters the run summary
/// reports. Holds a sticky cursor position so a cursor move persists across
/// frames the way an OS cursor would.
struct Runner {
    state: EggState,
    console: HeadlessConsole,
    input: EggInput,
    map_viewer: MapViewer,
    mouse: (i16, i16),
    frames: u64,
    shots: u64,
}

impl Runner {
    /// Advance exactly one frame: refresh the input (rolling this-frame values
    /// into `previous`, clearing current), re-assert the sticky cursor position,
    /// apply `setup`'s this-frame input, then drive the shared [`run_frame`]
    /// funnel. The map editor is never in text mode here (that's a host-only F2
    /// concern), so `text_mode` is always `false`.
    fn frame(&mut self, setup: impl FnOnce(&mut EggInput)) {
        self.input.refresh();
        self.input.mouse.x[0] = self.mouse.0;
        self.input.mouse.y[0] = self.mouse.1;
        setup(&mut self.input);
        run_frame(
            &mut self.state,
            &mut self.console,
            &self.input,
            &mut self.map_viewer,
            false,
        );
        self.frames += 1;
    }

    /// Write the current framebuffer to `path` as a PNG (harness output — it goes
    /// straight to disk, not through the console's in-memory store, since it's a
    /// screenshot, not a game file).
    fn shot(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.console.output.encode_png())
    }

    /// Execute one script command, advancing the frame loop as the command
    /// dictates (see [`USAGE`]). `mouse`/`editor`/`shot` set state without a
    /// frame; the rest each run one or more frames.
    fn exec(&mut self, cmd: &Command, out_dir: &Path) -> Result<(), String> {
        match cmd {
            Command::Wait(n) => {
                for _ in 0..*n {
                    self.frame(|_| {});
                }
            }
            Command::Hold(btn, n) => {
                let btn = *btn;
                for _ in 0..*n {
                    self.frame(|i| press_button(i, btn));
                }
            }
            Command::Press(btn) => {
                let btn = *btn;
                self.frame(|i| press_button(i, btn));
            }
            Command::Key(sc) => {
                let sc = *sc;
                self.frame(|i| i.press_key(sc));
            }
            Command::Type(text) => {
                self.frame(|i| {
                    for c in text.chars() {
                        i.push_char(c);
                    }
                });
            }
            Command::Mouse(x, y) => {
                self.mouse = (*x, *y);
                self.frame(|_| {});
            }
            Command::Click(x, y) => {
                self.mouse = (*x, *y);
                self.frame(|i| i.mouse.left[0] = true);
            }
            Command::Editor(on) => self.map_viewer.focused = *on,
            Command::Shot(name) => {
                let path = out_dir.join(format!("{name}.png"));
                self.shot(&path)
                    .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
                println!("shot: {}", path.display());
                self.shots += 1;
            }
        }
        Ok(())
    }
}

/// A controller face/direction button, the target of `hold`/`press`.
#[derive(Clone, Copy, Debug)]
enum Button {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
}

/// Assert `btn` held on controller 0 for the current frame.
fn press_button(input: &mut EggInput, btn: Button) {
    let c = &mut input.controllers[0];
    match btn {
        Button::Up => c.up[0] = true,
        Button::Down => c.down[0] = true,
        Button::Left => c.left[0] = true,
        Button::Right => c.right[0] = true,
        Button::A => c.a[0] = true,
        Button::B => c.b[0] = true,
        Button::X => c.x[0] = true,
        Button::Y => c.y[0] = true,
    }
}

/// One parsed script command (see [`USAGE`] for the surface syntax).
#[derive(Debug)]
enum Command {
    Wait(u32),
    Hold(Button, u32),
    Press(Button),
    Key(ScanCode),
    Type(String),
    Mouse(i16, i16),
    Click(i16, i16),
    Editor(bool),
    Shot(String),
}

/// Parse a whole input script, skipping blank lines and `#` comments. Any error
/// carries the 1-based line number (matching the file) so a bad command is easy
/// to locate.
fn parse_script(text: &str) -> Result<Vec<Command>, String> {
    let mut commands = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (cmd, rest) = match line.split_once(char::is_whitespace) {
            Some((c, r)) => (c, r.trim_start()),
            None => (line, ""),
        };
        commands.push(parse_command(cmd, rest, line, idx + 1)?);
    }
    Ok(commands)
}

/// Parse one command word + its arguments into a [`Command`], reporting arity /
/// value errors against `line`/`lineno`.
fn parse_command(cmd: &str, rest: &str, line: &str, lineno: usize) -> Result<Command, String> {
    let err = |msg: String| format!("line {lineno}: {msg} (in `{line}`)");
    match cmd {
        "wait" => {
            let n = rest
                .trim()
                .parse()
                .map_err(|_| err("wait expects a frame count".into()))?;
            Ok(Command::Wait(n))
        }
        "hold" => {
            let mut it = rest.split_whitespace();
            let (Some(btn), Some(n)) = (it.next(), it.next()) else {
                return Err(err("hold expects <button> <frames>".into()));
            };
            let btn = button_from_name(btn).ok_or_else(|| err(format!("unknown button `{btn}`")))?;
            let n = n
                .parse()
                .map_err(|_| err("hold frame count not a number".into()))?;
            Ok(Command::Hold(btn, n))
        }
        "press" => {
            let name = rest.trim();
            let btn =
                button_from_name(name).ok_or_else(|| err(format!("unknown button `{name}`")))?;
            Ok(Command::Press(btn))
        }
        "key" => {
            let name = rest.trim();
            let sc =
                scancode_from_name(name).ok_or_else(|| err(format!("unknown key `{name}`")))?;
            Ok(Command::Key(sc))
        }
        "type" => {
            if rest.is_empty() {
                return Err(err("type expects text".into()));
            }
            Ok(Command::Type(rest.to_string()))
        }
        "mouse" => {
            let (x, y) = parse_xy(rest).map_err(err)?;
            Ok(Command::Mouse(x, y))
        }
        "click" => {
            let (x, y) = parse_xy(rest).map_err(err)?;
            Ok(Command::Click(x, y))
        }
        "editor" => match rest.trim() {
            "on" => Ok(Command::Editor(true)),
            "off" => Ok(Command::Editor(false)),
            _ => Err(err("editor expects `on` or `off`".into())),
        },
        "shot" => {
            let name = rest.trim();
            if name.is_empty() {
                return Err(err("shot expects a name".into()));
            }
            Ok(Command::Shot(name.to_string()))
        }
        other => Err(err(format!("unknown command `{other}`"))),
    }
}

/// Two whitespace-separated integers (`mouse`/`click` coordinates).
fn parse_xy(s: &str) -> Result<(i16, i16), String> {
    let mut it = s.split_whitespace();
    let (Some(x), Some(y)) = (it.next(), it.next()) else {
        return Err("expects X and Y".to_string());
    };
    let x = x.parse().map_err(|_| format!("X not an integer: {x:?}"))?;
    let y = y.parse().map_err(|_| format!("Y not an integer: {y:?}"))?;
    Ok((x, y))
}

/// Map a `hold`/`press` button name to a [`Button`].
fn button_from_name(name: &str) -> Option<Button> {
    Some(match name {
        "up" => Button::Up,
        "down" => Button::Down,
        "left" => Button::Left,
        "right" => Button::Right,
        "a" => Button::A,
        "b" => Button::B,
        "x" => Button::X,
        "y" => Button::Y,
        _ => return None,
    })
}

/// Map a `key` command name to a [`ScanCode`]. Covers the letters, digits, arrow
/// keys, and the named editing/function/punctuation keys the in-game text fields
/// and hotkeys read (see [`ScanCode`]).
fn scancode_from_name(name: &str) -> Option<ScanCode> {
    use ScanCode::*;
    Some(match name {
        "a" => A,
        "b" => B,
        "c" => C,
        "d" => D,
        "e" => E,
        "f" => F,
        "g" => G,
        "h" => H,
        "i" => I,
        "j" => J,
        "k" => K,
        "l" => L,
        "m" => M,
        "n" => N,
        "o" => O,
        "p" => P,
        "q" => Q,
        "r" => R,
        "s" => S,
        "t" => T,
        "u" => U,
        "v" => V,
        "w" => W,
        "x" => X,
        "y" => Y,
        "z" => Z,
        "0" => Digit0,
        "1" => Digit1,
        "2" => Digit2,
        "3" => Digit3,
        "4" => Digit4,
        "5" => Digit5,
        "6" => Digit6,
        "7" => Digit7,
        "8" => Digit8,
        "9" => Digit9,
        "minus" => Minus,
        "equals" => Equals,
        "leftbracket" => LeftBracket,
        "rightbracket" => RightBracket,
        "backslash" => Backslash,
        "semicolon" => Semicolon,
        "apostrophe" => Apostrophe,
        "grave" => Grave,
        "comma" => Comma,
        "period" => Period,
        "slash" => Slash,
        "space" => Space,
        "tab" => Tab,
        "return" | "enter" => Return,
        "backspace" => Backspace,
        "delete" | "del" => Delete,
        "insert" => Insert,
        "pageup" => PageUp,
        "pagedown" => PageDown,
        "home" => Home,
        "end" => End,
        "up" => Up,
        "down" => Down,
        "left" => Left,
        "right" => Right,
        "capslock" => CapsLock,
        "ctrl" | "control" => Ctrl,
        "shift" => Shift,
        "alt" => Alt,
        "escape" | "esc" => Escape,
        "f1" => F1,
        "f2" => F2,
        "f3" => F3,
        "f4" => F4,
        "f5" => F5,
        "f6" => F6,
        "f7" => F7,
        "f8" => F8,
        "f9" => F9,
        "f10" => F10,
        "f11" => F11,
        "f12" => F12,
        _ => return None,
    })
}

/// Parsed command line. Every field defaults to "unset"; [`run`] applies the
/// defaults (map = bedroom, frames = 60, out = headless_shot.png, out-dir = ".").
#[derive(Default)]
struct Args {
    help: bool,
    list_maps: bool,
    check: bool,
    editor: bool,
    map: Option<String>,
    pos: Option<(i16, i16)>,
    flags: Vec<String>,
    seed: Option<u64>,
    save: Option<String>,
    frames: Option<u64>,
    out: Option<String>,
    script: Option<String>,
    out_dir: Option<String>,
    /// Override the auto-detected asset root (`--assets DIR`).
    assets: Option<String>,
}

impl Args {
    /// Parse the harness args. Hand-rolled (no clap — no new deps): value-taking
    /// flags consume the next arg, an unknown flag or a missing value is an error
    /// the caller turns into a usage message + exit 2.
    fn parse(args: &[String]) -> Result<Args, String> {
        let mut out = Args::default();
        let mut i = 0;
        while i < args.len() {
            let arg = args[i].as_str();
            match arg {
                "--help" | "-h" => out.help = true,
                "--list-maps" => out.list_maps = true,
                "--check" => out.check = true,
                "--editor" => out.editor = true,
                "--map" => out.map = Some(take(args, &mut i, arg)?),
                "--pos" => out.pos = Some(parse_pos(&take(args, &mut i, arg)?)?),
                "--flag" => out.flags.push(take(args, &mut i, arg)?),
                "--seed" => {
                    let v = take(args, &mut i, arg)?;
                    out.seed = Some(v.parse().map_err(|_| format!("--seed: not a number: {v}"))?);
                }
                "--save" => out.save = Some(take(args, &mut i, arg)?),
                "--frames" => {
                    let v = take(args, &mut i, arg)?;
                    out.frames =
                        Some(v.parse().map_err(|_| format!("--frames: not a number: {v}"))?);
                }
                "--out" => out.out = Some(take(args, &mut i, arg)?),
                "--script" => out.script = Some(take(args, &mut i, arg)?),
                "--out-dir" => out.out_dir = Some(take(args, &mut i, arg)?),
                "--assets" => out.assets = Some(take(args, &mut i, arg)?),
                other => return Err(format!("unknown argument: {other}")),
            }
            i += 1;
        }
        Ok(out)
    }
}

/// Consume the value following a value-taking flag, advancing `i` onto it.
fn take(args: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}

/// Parse a `--pos X,Y` string into map-pixel coordinates.
fn parse_pos(s: &str) -> Result<(i16, i16), String> {
    let (x, y) = s
        .split_once(',')
        .ok_or_else(|| format!("--pos must be X,Y (got {s:?})"))?;
    let x = x
        .trim()
        .parse()
        .map_err(|_| format!("--pos X not an integer: {:?}", x.trim()))?;
    let y = y
        .trim()
        .parse()
        .map_err(|_| format!("--pos Y not an integer: {:?}", y.trim()))?;
    Ok((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egg_core::render::image::Rgba;

    /// A PNG the engine's [`RgbaImage::encode_png`] produces decodes back —
    /// through the `image` crate ([`decode_png`], an independent decoder) — to
    /// the exact same RGBA bytes, fully-transparent pixels' colour included.
    #[test]
    fn png_round_trips_through_image_decode() {
        let mut img = RgbaImage::new(3, 2);
        img.set_pixel(0, 0, Rgba::new(255, 0, 0, 255));
        img.set_pixel(1, 0, Rgba::new(0, 255, 0, 128));
        img.set_pixel(2, 0, Rgba::new(0, 0, 255, 0)); // transparent but coloured
        img.set_pixel(0, 1, Rgba::new(10, 20, 30, 40));
        img.set_pixel(1, 1, Rgba::new(200, 150, 100, 255));
        img.set_pixel(2, 1, Rgba::new(1, 2, 3, 4));

        let png = img.encode_png();
        let decoded = decode_png(&png).expect("image crate decodes our PNG");
        assert_eq!((decoded.width(), decoded.height()), (3, 2));
        assert_eq!(decoded.data(), img.data());
    }

    /// The console is disk-isolated: writes land in memory + the log (never on
    /// disk), reads prefer memory, and an un-seeded read falls back to the
    /// read-only `assets/` tree — while `SAVE_PATH` (outside `assets/`) stays
    /// absent unless pre-seeded.
    #[test]
    fn headless_console_isolation() {
        let mut console = HeadlessConsole::new();
        console.write_file("maps/should_not_exist.tmj", b"hi");
        assert_eq!(
            console.files.get("maps/should_not_exist.tmj").cloned(),
            Some(b"hi".to_vec())
        );
        assert_eq!(console.written, vec!["maps/should_not_exist.tmj".to_string()]);
        // The write must not have reached disk — check the resolved on-disk path,
        // not a bare `assets/` (which needn't exist from the crate's CWD).
        let on_disk = asset_path(&console.asset_root, "maps/should_not_exist.tmj").unwrap();
        assert!(!on_disk.exists(), "the write must not have reached disk");

        // Memory wins over a same-named on-disk asset.
        console
            .files
            .insert("data/data.toml".to_string(), b"OVERRIDE".to_vec());
        assert_eq!(console.read_file("data/data.toml").as_deref(), Some(&b"OVERRIDE"[..]));

        // A fresh console with nothing seeded reads the real bundled asset from
        // disk, but never resolves the save (not under `assets/`).
        let mut fresh = HeadlessConsole::new();
        assert!(
            fresh.read_file("data/data.toml").is_some(),
            "falls back to assets/data/data.toml on disk"
        );
        assert!(
            fresh.read_file(SAVE_PATH).is_none(),
            "save.json is user data, not an asset — absent unless pre-seeded"
        );
    }

    /// The whole-harness regression net: a real asset boot from the auto-detected
    /// root, the default map loaded, 60 neutral frames run through [`run_frame`].
    /// The composited output must not be a single flat colour (the world drew),
    /// and [`RgbaImage::encode_png`] must still emit a valid PNG signature.
    #[test]
    fn boot_and_run_produces_a_non_uniform_frame() {
        let root = resolve_asset_root(None);
        let mut state = EggState::default();
        boot(&mut state, &root).expect("assets boot from the resolved root");

        let mut console = HeadlessConsole::new();
        state.load_data(&mut console);
        state.enter(GameMode::Walkaround);
        let input = EggInput::new();
        {
            let mut ctx = egg_core::Ctx {
                draw: &mut state.draw_state,
                system: &mut console,
                input: &input,
                maps: &mut state.maps,
                rng: &mut state.rng,
                script: &state.script,
                scenes: &state.scenes,
                save: &mut state.save,
                items: &state.items,
                presets: &state.presets,
                font: &state.font,
            };
            state.walkaround.load_map_by_name(&mut ctx, DEFAULT_MAP);
        }
        let player_pos = state.walkaround.player_ref().pos;
        state.walkaround.center_camera_on(player_pos, WIDTH, HEIGHT);

        let mut runner = Runner {
            state,
            console,
            input,
            map_viewer: MapViewer::default(),
            mouse: (0, 0),
            frames: 0,
            shots: 0,
        };
        for _ in 0..60 {
            runner.frame(|_| {});
        }

        let out = &runner.console.output;
        let first = out.get_pixel(0, 0);
        let varied = (0..out.height())
            .flat_map(|y| (0..out.width()).map(move |x| (x, y)))
            .any(|(x, y)| out.get_pixel(x, y) != first);
        assert!(varied, "composited frame is a single flat colour — nothing drew");

        let png = out.encode_png();
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']),
            "encode_png emits a valid PNG signature"
        );
    }

    /// Each command surface parses to the right variant, and coordinate/arity
    /// forms are read correctly.
    #[test]
    fn script_parses_every_command() {
        let script = "\
# a comment
wait 5

hold up 10
press a
key escape
type hello world
mouse 12 34
click 5 6
editor on
editor off
shot frame_one";
        let commands = parse_script(script).expect("parses");
        assert_eq!(commands.len(), 10, "comment + blank line skipped");
        assert!(matches!(commands[0], Command::Wait(5)));
        assert!(matches!(commands[1], Command::Hold(Button::Up, 10)));
        assert!(matches!(commands[2], Command::Press(Button::A)));
        assert!(matches!(commands[3], Command::Key(ScanCode::Escape)));
        // `type` keeps the rest of the line verbatim, internal spaces and all.
        match &commands[4] {
            Command::Type(t) => assert_eq!(t, "hello world"),
            other => panic!("expected Type, got {other:?}"),
        }
        assert!(matches!(commands[5], Command::Mouse(12, 34)));
        assert!(matches!(commands[6], Command::Click(5, 6)));
        assert!(matches!(commands[7], Command::Editor(true)));
        assert!(matches!(commands[8], Command::Editor(false)));
        assert!(matches!(commands[9], Command::Shot(_)));
    }

    /// A bad command or wrong arity reports the 1-based line number of the
    /// offending line (comments and blanks still count toward it).
    #[test]
    fn script_errors_carry_line_numbers() {
        // `wobble` is on line 3 (after a comment and a blank line).
        let err = parse_script("# header\n\nwobble 3").unwrap_err();
        assert!(err.contains("line 3"), "got: {err}");
        assert!(err.contains("unknown command"), "got: {err}");

        // Missing arity on `hold`.
        let err = parse_script("hold up").unwrap_err();
        assert!(err.contains("line 1"), "got: {err}");

        // Unknown button / key names are rejected with their line.
        assert!(parse_script("press start").unwrap_err().contains("line 1"));
        assert!(parse_script("key mega").unwrap_err().contains("line 1"));
        // Non-numeric frame count.
        assert!(parse_script("wait soon").unwrap_err().contains("line 1"));
    }

    /// The CLI parser reads values, collects repeated `--flag`s, and rejects
    /// unknown flags / missing values.
    #[test]
    fn args_parse_values_and_reject_unknowns() {
        let ok = Args::parse(&strs(&[
            "--map", "town", "--pos", "10,20", "--flag", "is_night", "--flag", "met_dog",
            "--seed", "7", "--frames", "3", "--editor", "--assets", "../../assets",
        ]))
        .expect("parses");
        assert_eq!(ok.map.as_deref(), Some("town"));
        assert_eq!(ok.pos, Some((10, 20)));
        assert_eq!(ok.flags, vec!["is_night", "met_dog"]);
        assert_eq!(ok.seed, Some(7));
        assert_eq!(ok.frames, Some(3));
        assert!(ok.editor);
        assert_eq!(ok.assets.as_deref(), Some("../../assets"));

        assert!(Args::parse(&strs(&["--bogus"])).is_err(), "unknown flag");
        assert!(Args::parse(&strs(&["--map"])).is_err(), "missing value");
        assert!(Args::parse(&strs(&["--pos", "nope"])).is_err(), "bad pos");
        assert!(Args::parse(&strs(&["--seed", "x"])).is_err(), "bad seed");
    }

    fn strs(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }
}

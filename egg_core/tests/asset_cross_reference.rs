//! Cross-reference the *shipped* data web: every map interactable/warp's
//! dialogue key and cutscene name, every scene's dialogue/load/sound/preset/
//! flag reference, and every dialogue message's portrait/sound/flag name
//! should resolve against the registry it targets — the class of typo that
//! today fails silently at playback (a dangling dialogue key falls back to
//! `default`, an unknown portrait/sound just warns and drops). Pins that
//! clean with a build/test-time assertion instead of hoping a playtester
//! notices.
//!
//! Mirrors [`warp_destinations`](super::warp_destinations)'s own-tests-own-
//! `assets/maps` shape (no sprite sheet / Bevy needed — object layers parse
//! standalone), extended across the whole data web via
//! [`egg_core::data::validate::check`]. Also lints every shipped language
//! overlay under `assets/script` (besides the base `en.eggtext`) against the
//! base script's skeleton via [`egg_core::data::validate::check_overlay`] —
//! there are none shipped today, but the wiring runs regardless.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use egg_core::data::eggdata::Presets;
use egg_core::data::portraits::Portraits;
use egg_core::data::scene;
use egg_core::data::script::eggtext;
use egg_core::data::tiled;
use egg_core::data::validate::{self, ENGINE_DIALOGUE_ROOTS};
use egg_core::world::map::MapObject;

fn maps_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/maps")
}

fn script_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/script")
}

/// Every `*.eggtext` language overlay under `assets/script` besides the base
/// (`en.eggtext`), parsed and keyed by language name — mirrors
/// `egg_game_headless::harness::script_overlay_stems`, so the CLI `--check`
/// and this test never drift on what counts as an overlay.
fn load_overlays() -> Vec<(String, egg_core::data::script::ScriptFile)> {
    let mut overlays = Vec::new();
    for entry in fs::read_dir(script_dir()).expect("read assets/script") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("eggtext") {
            continue;
        }
        let lang = path.file_stem().unwrap().to_str().unwrap().to_string();
        if lang == "en" {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {lang}.eggtext: {e}"));
        let file = eggtext::parse(&text).unwrap_or_else(|e| panic!("parse {lang}.eggtext: {e}"));
        overlays.push((lang, file));
    }
    overlays
}

/// Every `.tmj` under `assets/maps`, keyed by file stem (the name a warp/
/// cutscene reference targets), reduced to its parsed objects — the same
/// directory scan as `warp_destinations::load_maps`, minus the intermediate
/// `MapInfo` wrapper this checker doesn't need.
fn load_maps() -> BTreeMap<String, Vec<MapObject>> {
    let mut maps = BTreeMap::new();
    for entry in fs::read_dir(maps_dir()).expect("read assets/maps") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("tmj") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        let bytes = fs::read(&path).expect("read tmj");
        let map = tiled::from_json(&bytes).unwrap_or_else(|e| panic!("parse {stem}.tmj: {e}"));
        maps.insert(stem, map.parse_objects());
    }
    assert!(!maps.is_empty(), "no maps found under {:?}", maps_dir());
    maps
}

/// The shipped data web has no dangling reference. Warnings (dead dialogue,
/// flag hygiene) are printed for visibility but don't fail the test — only
/// [`validate::Report::is_clean`] (zero errors) does, so this is a floor, not
/// a snapshot: it catches a reference that breaks, not a warning count that
/// drifts.
#[test]
fn shipped_assets_have_no_dangling_references() {
    let script = eggtext::parse(include_str!("../../assets/script/en.eggtext")).expect("parse en.eggtext");
    let scenes = scene::parse(include_str!("../../assets/data/main.eggscene")).expect("parse main.eggscene");
    let maps = load_maps();

    let mut report = validate::check(
        &script,
        &scenes,
        &maps,
        &Portraits::builtin(),
        &Presets::builtin(),
        ENGINE_DIALOGUE_ROOTS,
    );

    // Every language overlay's dialogue must keep the base script's
    // skeleton — see `validate::check_overlay`.
    for (lang, overlay) in load_overlays() {
        let overlay_report = validate::check_overlay(&script, &overlay, &lang);
        report.errors.extend(overlay_report.errors);
        report.warnings.extend(overlay_report.warnings);
    }

    if !report.warnings.is_empty() {
        eprintln!("{} warning(s) (not fatal):\n{report}", report.warnings.len());
    }
    assert!(report.is_clean(), "{} dangling reference(s):\n{report}", report.errors.len());
}

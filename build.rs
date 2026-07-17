//! Build-time codegen for the wasm32 host: bakes the set of map file stems
//! into a compile-time list, since the web build has no filesystem to scan
//! `assets/maps/` at runtime the way native's `fantasy_console::map_stems`
//! does. Runs for every target (cheap either way), but only the wasm32 half of
//! `map_stems` actually `include!`s the generated file.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=assets/maps");

    let mut stems = Vec::new();
    if let Ok(entries) = fs::read_dir("assets/maps") {
        for entry in entries.flatten() {
            let path = entry.path();
            // `.extension()` only matches the last dot, so `foo.tmj.bak` reads
            // as extension `bak` and is excluded — mirrors `map_stems`'s rule.
            if path.extension().and_then(|e| e.to_str()) == Some("tmj")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                stems.push(stem.to_string());
            }
        }
    }
    stems.sort();

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let dest = Path::new(&out_dir).join("map_names.rs");
    let mut body = String::from("pub const MAP_NAMES: &[&str] = &[\n");
    for stem in &stems {
        body.push_str(&format!("    {stem:?},\n"));
    }
    body.push_str("];\n");
    fs::write(&dest, body).expect("failed to write map_names.rs");
}

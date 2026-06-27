//! Authored-map sanity check: no warp may land the player on a warp on its
//! destination map. Such a landing re-fires instantly for an [`auto`] warp (a
//! teleport loop) or drops the player inside a door for an [`interact`] warp.
//!
//! Reuses [`MapInfo::warp_landing_conflict`] — the same guard the map editor's
//! warp-destination placement check is built on — over the real `.tmj` files,
//! so it catches both hand-authored and editor-placed regressions.
//!
//! [`auto`]: egg_core::world::map::WarpMode::Auto
//! [`interact`]: egg_core::world::map::WarpMode::Interact

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use egg_core::data::tiled;
use egg_core::world::map::{MapInfo, ObjectEffect};
use egg_core::world::player::Shell;

fn maps_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/maps")
}

/// Every `.tmj` under `assets/maps`, keyed by file stem (the name warps target),
/// reduced to the objects it contributes. Objects parse straight from the Tiled
/// object layer, so no sprite sheet / collision build is needed.
fn load_maps() -> HashMap<String, MapInfo> {
    let mut maps = HashMap::new();
    for entry in fs::read_dir(maps_dir()).expect("read assets/maps") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("tmj") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        let bytes = fs::read(&path).expect("read tmj");
        let map = tiled::from_json(&bytes).unwrap_or_else(|e| panic!("parse {stem}.tmj: {e}"));
        maps.insert(
            stem,
            MapInfo {
                objects: map.parse_objects(),
                ..Default::default()
            },
        );
    }
    assert!(!maps.is_empty(), "no maps found under {:?}", maps_dir());
    maps
}

#[test]
fn no_warp_destination_lands_on_a_warp() {
    let player = Shell::default().local_hitbox;
    let maps = load_maps();
    let mut conflicts = Vec::new();

    for (name, info) in &maps {
        for (i, obj) in info.objects.iter().enumerate() {
            let ObjectEffect::Warp(warp) = &obj.effect else {
                continue;
            };
            // Destination map: the named one (skip an unresolvable name — that
            // warp is a runtime no-op), else this same map.
            let dest = match &warp.map {
                Some(dest_name) => match maps.get(dest_name) {
                    Some(dest) => dest,
                    None => continue,
                },
                None => info,
            };
            let dest_name = warp.map.as_deref().unwrap_or(name);
            if let Some(hit) = dest.warp_landing_conflict(warp.to, player) {
                conflicts.push(format!(
                    "  {name} warp[{i}] -> {dest_name}: lands at ({}, {}) on warp[{hit}] {:?}",
                    warp.to.x, warp.to.y, dest.objects[hit].hitbox,
                ));
            }
        }
    }

    assert!(
        conflicts.is_empty(),
        "{} warp destination(s) land on a warp (instant re-warp):\n{}",
        conflicts.len(),
        conflicts.join("\n"),
    );
}

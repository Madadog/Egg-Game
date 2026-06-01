//! Disk/browser persistence for [`SaveData`].
//!
//! `egg_core` owns the `SaveData` struct and decides *when* its fields change;
//! the Bevy frontend owns the persistence policy — JSON format, storage
//! location, and when to flush. We autosave whenever the data changes (and once
//! more on exit). The JSON (de)serialisation is shared across platforms; only
//! the raw read/write differs:
//!
//! * **native** — a `save.json` file in the working directory.
//! * **web (wasm)** — a `localStorage` entry keyed by `SAVE_PATH`.

use bevy::log::error;
use bevy::prelude::Resource;
use egg_core::data::save::SaveData;

/// On native, the file the game's progress is serialised to (relative to the
/// working directory). On web, the `localStorage` key it's stored under.
pub const SAVE_PATH: &str = "save.json";

/// The last [`SaveData`] written to storage. The autosave system diffs the live
/// data against this so it only writes when something actually changed.
#[derive(Resource)]
pub struct SaveTracker {
    pub last: SaveData,
}

/// Load persisted [`SaveData`]. Returns `None` when there is no save yet, or
/// when it can't be read or parsed (logged, not fatal — the game falls back to
/// a fresh `SaveData::default()`).
pub fn load() -> Option<SaveData> {
    let json = read_raw()?;
    match serde_json::from_str(&json) {
        Ok(data) => Some(data),
        Err(e) => {
            error!("Failed to parse save ({SAVE_PATH}): {e}");
            None
        }
    }
}

/// Serialise and persist `data`. Any serialisation or storage error is logged
/// and swallowed so a failed write never crashes the game.
pub fn write(data: &SaveData) {
    let json = match serde_json::to_string_pretty(data) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to serialise save data: {e}");
            return;
        }
    };
    write_raw(&json);
}

#[cfg(not(target_arch = "wasm32"))]
fn read_raw() -> Option<String> {
    if !std::path::Path::new(SAVE_PATH).exists() {
        return None;
    }
    match std::fs::read_to_string(SAVE_PATH) {
        Ok(json) => Some(json),
        Err(e) => {
            error!("Failed to read save file {SAVE_PATH}: {e}");
            None
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_raw(json: &str) {
    if let Err(e) = std::fs::write(SAVE_PATH, json) {
        error!("Failed to write save file {SAVE_PATH}: {e}");
    }
}

#[cfg(target_arch = "wasm32")]
fn read_raw() -> Option<String> {
    match local_storage()?.get_item(SAVE_PATH) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to read save from localStorage: {e:?}");
            None
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn write_raw(json: &str) {
    if let Some(storage) = local_storage()
        && let Err(e) = storage.set_item(SAVE_PATH, json)
    {
        error!("Failed to write save to localStorage: {e:?}");
    }
}

/// The browser's `localStorage`, or `None` if it's unavailable (e.g. disabled
/// by the user or accessed from a non-browser context).
#[cfg(target_arch = "wasm32")]
fn local_storage() -> Option<web_sys::Storage> {
    match web_sys::window()?.local_storage() {
        Ok(storage) => storage,
        Err(e) => {
            error!("localStorage unavailable: {e:?}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips_save_data() {
        let data = SaveData {
            intro_anim_seen: true,
            instructions_read: true,
            egg_count: 1234,
            inventory: [1, 2, 3, 4, 5, 6, 7, 8],
            current_map: 9,
            player_x: -42,
            player_y: 300,
            ..SaveData::default()
        };
        let json = serde_json::to_string_pretty(&data).expect("serialise");
        let parsed: SaveData = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(data, parsed);
    }
}

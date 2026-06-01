//! Disk persistence for [`SaveData`].
//!
//! `egg_core` owns the `SaveData` struct and decides *when* its fields change;
//! the Bevy frontend owns the persistence policy — JSON format, file location,
//! and when to flush. We autosave whenever the data changes (and once more on
//! exit), reading and writing `save.json` in the working directory.

use std::fs;
use std::path::Path;

use bevy::log::error;
use bevy::prelude::Resource;
use egg_core::data::save::SaveData;

/// File the game's progress is serialised to, relative to the working directory.
pub const SAVE_PATH: &str = "save.json";

/// The last [`SaveData`] written to disk. The autosave system diffs the live
/// data against this so it only writes when something actually changed.
#[derive(Resource)]
pub struct SaveTracker {
    pub last: SaveData,
}

/// Load persisted [`SaveData`] from [`SAVE_PATH`]. Returns `None` when there is
/// no save yet, or when the file can't be read or parsed (logged, not fatal —
/// the game falls back to a fresh `SaveData::default()`).
pub fn load() -> Option<SaveData> {
    if !Path::new(SAVE_PATH).exists() {
        return None;
    }
    let json = match fs::read_to_string(SAVE_PATH) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to read save file {SAVE_PATH}: {e}");
            return None;
        }
    };
    match serde_json::from_str(&json) {
        Ok(data) => Some(data),
        Err(e) => {
            error!("Failed to parse save file {SAVE_PATH}: {e}");
            None
        }
    }
}

/// Serialise `data` to [`SAVE_PATH`]. Any I/O or serialisation error is logged
/// and swallowed so a failed write never crashes the game.
pub fn write(data: &SaveData) {
    let json = match serde_json::to_string_pretty(data) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to serialise save data: {e}");
            return;
        }
    };
    if let Err(e) = fs::write(SAVE_PATH, json) {
        error!("Failed to write save file {SAVE_PATH}: {e}");
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

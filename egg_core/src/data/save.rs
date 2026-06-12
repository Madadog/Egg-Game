use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The path the engine persists progress under. The engine names the file; a
/// host routes it to whatever user-data backend it has (a file on native, a
/// `localStorage` entry on web) — see `ConsoleApi::write_file`/`read_file`.
pub const SAVE_PATH: &str = "save.json";

/// Misc. progression flags and numbers. Persisted to the player's storage
/// device and restored across runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveData {
    // UI / general flags
    pub intro_anim_seen: bool,
    pub small_text_on: bool,
    pub instructions_read: bool,
    /// If true, you have to press the interact button to use doors.
    pub manual_doors: bool,

    /// Named story flags, the open-ended replacement for the old packed
    /// bitfields and one-off typed bools: dialogue toggles them with `#set` and
    /// branches on them with `#if` (see [`crate::data::eggtext`]), and the
    /// vocabulary the script declares (`#flag NAME`) is what an in-game editor
    /// autocompletes against. Only set flags are stored, so an absent name reads
    /// as `false` and old saves simply lack any flag they never set.
    #[serde(default)]
    pub flags: BTreeSet<String>,

    // House
    pub dog_fed: bool,
    pub living_room_seen: bool,

    // Egg
    pub egg_count: u16,

    // Supermarket
    pub supermarket_thief: bool,
    pub supermarket_key_access: bool,
    pub supermarket_backroom: bool,

    pub wilderness_egg_found: bool,

    pub egg_pop_count: u8,

    pub is_night: bool,

    // Shell
    pub shell_key: bool,
    pub shell_curiosity: bool,
    pub shell_matryoshka: bool,
    pub shell_monster: bool,

    /// Inventory slots, each holding a u8 ItemID. There's no way I'll use ALL
    /// 255 items......
    /// TODO: Convert between item and id.
    pub inventory: [u8; 8],

    /// Legacy numeric map id (a `MapIndex`), retained so saves written here
    /// stay readable by old binaries and old saves keep loading. When both
    /// are present, `current_map_name` wins.
    pub current_map: u8,
    /// Name of the map the player saved on. `None` in saves written before
    /// maps were named — loading then falls back to the numeric `current_map`.
    #[serde(default)]
    pub current_map_name: Option<String>,
    pub player_x: i16,
    pub player_y: i16,

    /// Number of times the game has saved
    pub save_count: u32,
}

impl SaveData {
    /// Set (or clear) a named story [`flag`](Self::flag). Setting inserts the
    /// name; clearing removes it, so the stored set only ever holds the flags
    /// that are currently true.
    pub fn set_flag(&mut self, name: &str, value: bool) {
        if value {
            // Avoid allocating when the flag is already present.
            if !self.flags.contains(name) {
                self.flags.insert(name.to_string());
            }
        } else {
            self.flags.remove(name);
        }
    }

    /// Read a named story flag. An undeclared/unset name reads as `false`.
    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::map_data::MapIndex;

    /// Saves written before maps were named carry only the numeric map id and
    /// no `current_map_name` key at all; they must still deserialise, and the
    /// numeric id must resolve to the right legacy name.
    #[test]
    fn old_numeric_save_resolves_to_legacy_name() {
        // Serialise a current save, then strip the name field to reproduce the
        // exact shape an old binary wrote (all fields present except the name).
        let mut value = serde_json::to_value(SaveData {
            current_map: 4,
            ..SaveData::default()
        })
        .unwrap();
        value.as_object_mut().unwrap().remove("current_map_name");

        let save: SaveData = serde_json::from_value(value).expect("old save still loads");
        assert_eq!(save.current_map_name, None);
        assert_eq!(MapIndex(save.current_map.into()).name(), "bedroom");
    }

    /// A populated save survives a pretty-print/parse round trip unchanged —
    /// the format the engine autosaves through (see [`SAVE_PATH`]).
    #[test]
    fn json_round_trips_save_data() {
        let mut data = SaveData {
            intro_anim_seen: true,
            instructions_read: true,
            egg_count: 1234,
            inventory: [1, 2, 3, 4, 5, 6, 7, 8],
            current_map: 9,
            player_x: -42,
            player_y: 300,
            ..SaveData::default()
        };
        data.set_flag("house_stairwell_window_interacted", true);
        data.set_flag("met_the_dog", true);
        let json = serde_json::to_string_pretty(&data).expect("serialise");
        let parsed: SaveData = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(data, parsed);
        assert!(parsed.flag("house_stairwell_window_interacted"));
        assert!(parsed.flag("met_the_dog"));
    }

    /// `set_flag`/`flag` insert and remove names, and an unset name reads false.
    #[test]
    fn flag_helpers_set_and_clear() {
        let mut save = SaveData::default();
        assert!(!save.flag("seen_sunrise"));
        save.set_flag("seen_sunrise", true);
        assert!(save.flag("seen_sunrise"));
        // Setting an already-set flag is idempotent.
        save.set_flag("seen_sunrise", true);
        assert!(save.flag("seen_sunrise"));
        // Clearing removes it from the stored set entirely.
        save.set_flag("seen_sunrise", false);
        assert!(!save.flag("seen_sunrise"));
        assert!(save.flags.is_empty());
    }

    /// A save written before `flags` existed has no `flags` key at all; it must
    /// still load, with an empty flag set.
    #[test]
    fn old_save_without_flags_loads_empty() {
        let mut value = serde_json::to_value(SaveData::default()).unwrap();
        value.as_object_mut().unwrap().remove("flags");
        let save: SaveData = serde_json::from_value(value).expect("old save still loads");
        assert!(save.flags.is_empty());
        assert!(!save.flag("anything"));
    }
}

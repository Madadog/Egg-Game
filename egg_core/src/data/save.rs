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

    // House
    pub house_stairwell_window_interacted: bool,
    pub dog_fed: bool,
    pub living_room_seen: bool,

    // Egg
    pub egg_count: u16,
    pub egg_flags: u8,
    pub town_flags: u8,

    // Supermarket
    pub supermarket_thief: bool,
    pub supermarket_key_access: bool,
    pub supermarket_backroom: bool,

    pub hospital_flags: u8,

    pub wilderness_egg_found: bool,

    pub factory_flags: u8,
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

use serde::{Deserialize, Serialize};

/// Misc. progression flags and numbers. Persisted to the player's storage
/// device and restored across runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

    pub current_map: u8,
    pub player_x: i16,
    pub player_y: i16,

    /// Number of times the game has saved
    pub save_count: u32,
}

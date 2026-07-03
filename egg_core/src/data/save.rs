use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::world::player::Shell;

/// The path the engine persists progress under. The engine names the file; a
/// host routes it to whatever user-data backend it has (a file on native, a
/// `localStorage` entry on web) — see `ConsoleApi::write_file`/`read_file`.
pub const SAVE_PATH: &str = "save.json";

/// The [`flags`](SaveData::flags) name the day/night palette rides on. A plain
/// story flag like any other, so dialogue (`#set is_night true` / `#if is_night`),
/// object gates (`if`/`unless is_night`) and cutscene `set` steps can all read and
/// write the world's day/night state with no dedicated machinery — the walkaround
/// paints [`NIGHT_16`](crate::platform::NIGHT_16) when it is set and
/// [`SWEETIE_16`](crate::platform::SWEETIE_16) otherwise. Named here (not spelled
/// as a bare literal at each site) because the engine reads it from several files.
pub const IS_NIGHT_FLAG: &str = "is_night";

/// Misc. progression flags and numbers. Persisted to the player's storage
/// device and restored across runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveData {
    // UI / general flags
    pub intro_anim_seen: bool,
    pub small_text_on: bool,
    pub instructions_read: bool,
    /// If true, you have to press the interact button to use doors.
    pub manual_doors: bool,

    /// Named story flags, the open-ended replacement for the old packed
    /// bitfields and one-off typed bools: dialogue toggles them with `#set` and
    /// branches on them with `#if` (see [`crate::data::script::eggtext`]), and the
    /// vocabulary the script declares (`#flag NAME`) is what an in-game editor
    /// autocompletes against. Only set flags are stored, so an absent name reads
    /// as `false` and old saves simply lack any flag they never set.
    #[serde(default)]
    pub flags: BTreeSet<String>,

    /// Stable ids of removable interactables the player has consumed (pickups),
    /// keyed `"<map>#<object id>"` — see
    /// [`MapObject::removable`](crate::world::map::MapObject::removable). Mirrors
    /// [`flags`](Self::flags): only taken entries are stored, presence means
    /// taken, an absent key reads as "still there". Kept separate from `flags` so
    /// a pickup needs no authored flag name and never pollutes the `#flag`
    /// vocabulary an editor autocompletes. An edit that *deletes and recreates* an
    /// object changes its [`id`](crate::world::map::MapObject::id) and leaves a harmless
    /// dangling entry; ordinary edits keep the id, so the pickup stays taken.
    #[serde(default)]
    pub taken: BTreeSet<String>,

    // Egg
    pub egg_count: u16,

    pub egg_pop_count: u8,

    // Shell
    pub shell_key: bool,
    pub shell_curiosity: bool,
    pub shell_matryoshka: bool,
    pub shell_monster: bool,

    /// Inventory slots, each holding an item key (`None` = empty slot). The
    /// default seeds the three starting items (ff/lm/chegg), matching the live
    /// [`Inventory::new`](crate::gamestate::walkaround::inventory::Inventory::new); a key the
    /// item registry no longer knows is dropped on load (garbage tolerance, see
    /// [`Inventory::load_from_save`](crate::gamestate::walkaround::inventory::Inventory::load_from_save)).
    #[serde(default = "default_inventory")]
    pub inventory: [Option<String>; 8],

    /// Name of the map the player saved on. `None` in saves written before
    /// maps were named — loading then falls back to the bedroom (see
    /// [`WalkaroundState::load_pmem`](crate::gamestate::walkaround::WalkaroundState::load_pmem)).
    /// Old saves carrying the long-removed numeric `current_map` field still
    /// load: that key is simply ignored (no `deny_unknown_fields`).
    #[serde(default)]
    pub current_map_name: Option<String>,

    /// Legacy player position, kept only to load saves written before the whole
    /// player entity was persisted (see [`player`](Self::player)): when `player`
    /// is absent these place the restored default player. New saves carry the
    /// position inside `player` instead and leave these at `0`.
    #[serde(default)]
    pub player_x: i16,
    #[serde(default)]
    pub player_y: i16,

    /// Number of times the game has saved
    pub save_count: u32,

    /// The whole player entity, persisted like any other [`Shell`] — so its
    /// position **and** its nested `companions` (the dog) ride along for free,
    /// along with any future player state (form, hp). The player is `entities[0]`
    /// but travels across maps, so it gets its own slot here rather than living in
    /// [`map_entities`](Self::map_entities) (which is per-map). Every field
    /// round-trips except the derived `sprites`/`trail`/`interaction`, rebuilt on
    /// load. `None` only in older saves, which fall back to
    /// [`player_x`](Self::player_x)/[`player_y`](Self::player_y).
    #[serde(default)]
    pub player: Option<Shell>,

    /// Non-player entities (creatures) parked by map name — the persisted form of
    /// the runtime [`WalkaroundState::map_entities`](crate::gamestate::walkaround::WalkaroundState),
    /// folded together with the current map's live `entities[1..]` at save time so
    /// creatures resume on the map that spawned them. Each [`Shell`] round-trips
    /// every field but its (derived) `sprites`, which are rebuilt from the
    /// `preset` on load. Last field so the autosave diff short-circuits on the
    /// cheap scalars before walking it. Absent in older saves ⇒ empty.
    #[serde(default)]
    pub map_entities: BTreeMap<String, Vec<Shell>>,
}

/// The starting inventory a fresh save (and a save written before items were
/// keyed) carries: the three default items, matching
/// [`Inventory::new`](crate::gamestate::walkaround::inventory::Inventory::new). Used as both
/// the [`SaveData::default`] inventory and the `serde` default for the field, so
/// an old save lacking the key reads back the original starting items (the old
/// `[1,2,3,4,5,6,7,8]` resolved to exactly these, ids 4–8 being unknown).
fn default_inventory() -> [Option<String>; 8] {
    [
        Some("ff".to_string()),
        Some("lm".to_string()),
        Some("chegg".to_string()),
        None,
        None,
        None,
        None,
        None,
    ]
}

impl Default for SaveData {
    fn default() -> Self {
        Self {
            inventory: default_inventory(),
            // Every other field is its own type's default; only `inventory`
            // departs from a derived `Default` (it seeds the starting items).
            intro_anim_seen: false,
            small_text_on: false,
            instructions_read: false,
            manual_doors: false,
            flags: BTreeSet::new(),
            taken: BTreeSet::new(),
            egg_count: 0,
            egg_pop_count: 0,
            shell_key: false,
            shell_curiosity: false,
            shell_matryoshka: false,
            shell_monster: false,
            current_map_name: None,
            player_x: 0,
            player_y: 0,
            save_count: 0,
            player: None,
            map_entities: BTreeMap::new(),
        }
    }
}

impl SaveData {
    /// Parse a save from its stored JSON, folding legacy fields forward. The
    /// derive tolerates unknown keys (no `deny_unknown_fields`), so a stale key
    /// is normally dropped silently — but a field that has since *become a flag*
    /// must be carried over, not lost. This reads those keys off the raw JSON
    /// before [`from_value`](serde_json::from_value) discards them and
    /// re-expresses them as flags. Today that is the old `is_night` bool: a save
    /// written when it was a dedicated field loads with the [`IS_NIGHT_FLAG`]
    /// flag set to match, so the world's day/night state survives the promotion.
    pub fn from_json(bytes: &[u8]) -> serde_json::Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        // Read the legacy bool before `from_value` drops the now-unknown key.
        let legacy_night = value.get("is_night").and_then(|v| v.as_bool()) == Some(true);
        let mut save: Self = serde_json::from_value(value)?;
        if legacy_night {
            save.set_flag(IS_NIGHT_FLAG, true);
        }
        Ok(save)
    }

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

    /// The four shell story flags in their canonical (declaration) order — the
    /// unlock state of the four Eggs-page shell slots (index `0..4`). Each gates
    /// the emblem (sprite `10 + index`) drawn centred on that egg once its shell
    /// is unlocked. The single source of truth for "shell N unlocked"; the bag
    /// reads it at draw time rather than storing a duplicate.
    pub fn shell_flags(&self) -> [bool; 4] {
        [
            self.shell_key,
            self.shell_curiosity,
            self.shell_matryoshka,
            self.shell_monster,
        ]
    }

    /// The [`taken`](Self::taken) key a removable object is recorded under: its
    /// map name and stable [`id`](crate::world::map::MapObject::id), joined so the same
    /// local id on two different maps never collides. `pub(crate)` so the map
    /// editor can name the same key when un-taking / re-taking for testing.
    pub(crate) fn taken_key(map: &str, id: usize) -> String {
        format!("{map}#{id}")
    }

    /// Record a removable object (by map name + stable id) as consumed, so every
    /// later use of that map skips it — its interaction won't fire and its sprite
    /// won't draw (the object stays in the map data so the editor still shows it).
    pub fn mark_taken(&mut self, map: &str, id: usize) {
        self.taken.insert(Self::taken_key(map, id));
    }

    /// Whether a removable object (by map name + stable id) has been consumed.
    pub fn is_taken(&self, map: &str, id: usize) -> bool {
        self.taken.contains(&Self::taken_key(map, id))
    }

    /// Flip a taken entry by its full `<map>#<id>` key (see
    /// [`taken_key`](Self::taken_key)) — the map editor's un-take / re-take test
    /// toggle. Removes the key if present, inserts it otherwise.
    pub fn toggle_taken(&mut self, key: &str) {
        if !self.taken.remove(key) {
            self.taken.insert(key.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::eggdata::Presets;
    use crate::world::player::PresetId;
    use crate::geometry::Vec2;

    /// A pre-name save carries the long-removed numeric `current_map` field and
    /// no `current_map_name` key at all; it must still deserialise (the unknown
    /// numeric field is ignored), with the name defaulting cleanly to `None` so
    /// the loader falls back to the bedroom.
    #[test]
    fn old_numeric_save_still_loads_ignoring_numeric_field() {
        // Reproduce the exact shape an old binary wrote: a current save plus the
        // numeric `current_map` field, with no `current_map_name` key.
        let mut value = serde_json::to_value(SaveData::default()).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.insert("current_map".to_string(), serde_json::json!(4));
        obj.remove("current_map_name");

        let save: SaveData = serde_json::from_value(value).expect("old save still loads");
        assert_eq!(save.current_map_name, None);
    }

    /// A save written before the inert progression bools were removed still
    /// carries their keys (`dog_fed`, `supermarket_thief`, …). With no
    /// `deny_unknown_fields` on [`SaveData`], those now-unknown keys are ignored
    /// on load rather than erroring, so old saves keep working.
    #[test]
    fn old_save_with_removed_bools_still_loads() {
        let mut value = serde_json::to_value(SaveData::default()).unwrap();
        let obj = value.as_object_mut().unwrap();
        for removed in [
            "dog_fed",
            "living_room_seen",
            "supermarket_thief",
            "supermarket_key_access",
            "supermarket_backroom",
            "wilderness_egg_found",
        ] {
            obj.insert(removed.to_string(), serde_json::json!(true));
        }
        let save: SaveData =
            serde_json::from_value(value).expect("save with removed bools still loads");
        // The unknown keys are simply dropped; the rest of the save is intact.
        assert_eq!(save, SaveData::default());
    }

    /// A populated save survives a pretty-print/parse round trip unchanged —
    /// the format the engine autosaves through (see [`SAVE_PATH`]).
    #[test]
    fn json_round_trips_save_data() {
        let mut data = SaveData {
            intro_anim_seen: true,
            instructions_read: true,
            egg_count: 1234,
            inventory: [
                Some("ff".to_string()),
                Some("lm".to_string()),
                Some("chegg".to_string()),
                None,
                None,
                None,
                None,
                None,
            ],
            current_map_name: Some("town".to_string()),
            player_x: -42,
            player_y: 300,
            ..SaveData::default()
        };
        data.set_flag("house_stairwell_window_interacted", true);
        data.set_flag("met_the_dog", true);
        data.mark_taken("town", 7);
        data.map_entities.insert(
            "town".to_string(),
            vec![
                Presets::builtin()
                    .spawn(&PresetId::critter())
                    .unwrap()
                    .with_pos(Vec2::new(9, 9)),
            ],
        );
        let json = serde_json::to_string_pretty(&data).expect("serialise");
        let parsed: SaveData = serde_json::from_str(&json).expect("deserialise");
        // Equal despite the round-tripped creature's sprites coming back as a
        // placeholder — `Shell`'s equality ignores the (derived, skipped) sprites.
        assert_eq!(data, parsed);
        assert!(parsed.flag("house_stairwell_window_interacted"));
        assert!(parsed.flag("met_the_dog"));
        assert!(parsed.is_taken("town", 7));
        assert_eq!(parsed.map_entities["town"][0].pos, Vec2::new(9, 9));
        assert_eq!(parsed.map_entities["town"][0].preset, PresetId::critter());
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

    /// `shell_flags` returns the four shell flags in declaration order, pinning
    /// the Eggs-page emblem mapping (slot index `i` → sprite `10 + i`) to the
    /// right shell. If the field order or this method ever drift apart, the
    /// wrong emblem would render on the wrong egg.
    #[test]
    fn shell_flags_are_ordered() {
        let mut save = SaveData::default();
        assert_eq!(save.shell_flags(), [false; 4]);
        save.shell_curiosity = true; // second flag → index 1
        assert_eq!(save.shell_flags(), [false, true, false, false]);
        save.shell_monster = true; // fourth flag → index 3
        assert_eq!(save.shell_flags(), [false, true, false, true]);
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

    /// A save written when `is_night` was a dedicated bool (since promoted to the
    /// [`IS_NIGHT_FLAG`] flag) migrates on load: a legacy night save comes back
    /// with the flag set, so day/night survives the field's removal. A legacy day
    /// save (the bool `false`, or the key absent entirely) leaves the flag clear.
    #[test]
    fn legacy_is_night_bool_migrates_to_flag() {
        // Night: the legacy bool was `true` -> the flag is set on load.
        let mut value = serde_json::to_value(SaveData::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("is_night".to_string(), serde_json::json!(true));
        let bytes = serde_json::to_vec(&value).unwrap();
        let night = SaveData::from_json(&bytes).expect("legacy night save loads");
        assert!(
            night.flag(IS_NIGHT_FLAG),
            "the legacy is_night bool migrates to the flag"
        );

        // Day: the legacy bool was `false` -> the flag stays clear.
        let mut value = serde_json::to_value(SaveData::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("is_night".to_string(), serde_json::json!(false));
        let bytes = serde_json::to_vec(&value).unwrap();
        let day = SaveData::from_json(&bytes).expect("legacy day save loads");
        assert!(!day.flag(IS_NIGHT_FLAG));

        // A modern save carries no `is_night` key at all and also reads as day.
        let bytes = serde_json::to_vec(&SaveData::default()).unwrap();
        let modern = SaveData::from_json(&bytes).expect("modern save loads");
        assert!(!modern.flag(IS_NIGHT_FLAG));
    }

    /// `mark_taken`/`is_taken` record and read consumed removable pickups by map
    /// name + stable id, and the same local id on two maps never collides (the
    /// key folds in the map name).
    #[test]
    fn taken_marks_and_reads_per_map() {
        let mut save = SaveData::default();
        assert!(!save.is_taken("town", 5));
        save.mark_taken("town", 5);
        assert!(save.is_taken("town", 5));
        // The same id on another map, and a different id on the same map, are
        // both independent — so two pickups never alias.
        assert!(!save.is_taken("supermarket", 5));
        assert!(!save.is_taken("town", 6));
    }

    /// A save written before `taken` existed has no `taken` key; it must still
    /// load, with nothing taken (every removable pickup intact).
    #[test]
    fn old_save_without_taken_loads_empty() {
        let mut value = serde_json::to_value(SaveData::default()).unwrap();
        value.as_object_mut().unwrap().remove("taken");
        let save: SaveData = serde_json::from_value(value).expect("old save still loads");
        assert!(save.taken.is_empty());
        assert!(!save.is_taken("town", 1));
    }

    /// A save written before per-map entities has no `map_entities` key; it must
    /// still load, with no parked creatures.
    #[test]
    fn old_save_without_map_entities_loads_empty() {
        let mut value = serde_json::to_value(SaveData::default()).unwrap();
        value.as_object_mut().unwrap().remove("map_entities");
        let save: SaveData = serde_json::from_value(value).expect("old save still loads");
        assert!(save.map_entities.is_empty());
    }
}

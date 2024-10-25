# egg_core
Most of the game code/logic is in this crate. The core game is separated from its frontend (TIC-80, Bevy, etc) and only communicates with it via the `ConsoleApi` trait (which provides graphics, sound and input).

Essentially, the entry point is the `GameState` struct in `src/gamestate/mod.rs`. Once constructed, the game can be run one frame at a time by calling `GameState::run()` (with all the required parameters).

The `ConsoleApi` trait (defined in `src/system.rs`) is used by the game to display graphics, play sounds, and receive input. It replicates the original TIC-80 API (and was used to port the game away from that platform). Currently there is only one implementation (`FantasyConsole` in the top-level Bevy crate). There will probably never be any more frontends, so the trait could probably be rolled into the `FantasyConsole` struct.

TODO: Simplify game setup.

# Repo Layout

|File|Description|
|---|---|
|`src/data`|Game data (levels, text, misc. metadata).|
|`src/data/dialogue_data.rs`|All text strings used in game.|
|`src/data/map_data.rs`|Contains game dialogue, maps/levels, a bit of graphics data, save/load convenience functions for PERSISTENT_RAM and sound data.|
|`src/data/portraits.rs`|Metadata for character portraits.|
|`src/data/save.rs`|Functions for saving/loading to save data|
|`src/data/sound.rs`|Sound metadata.|
|---|---|
|`src/gamestate`|Logic for switching between game menus and gameplay.|
|`src/gamestate/intro.rs`|Intro animation.|
|`src/gamestate/inventory.rs`|Nightmarish inventory logic.|
|`src/gamestate/menu.rs`|Main menu / options menu logic.|
|`src/gamestate/walkaround.rs`|RPG-style map navigation.|
|---|---|
|`src/animation.rs`|Small helper structs for animating sprite indexes and positions.|
|`src/camera.rs`|Different camera behaviours and logic to automatically pick based on map layout.|
|`src/debug.rs`|Contains a struct used to display graphical information used to debug the game.|
|`src/dialogue.rs`|Draws text character-by-character, automatically adding line-breaks.|
|`src/interact.rs`|Struct for interactable map objects.|
|`src/lib.rs`|Exports everything to the front-end.|
|`src/map.rs`|Structs used in `map_data.rs`.|
|`src/particles.rs`|Particles. After generation, they act and are drawn according to their initial parameters.|
|`src/player.rs`|Player struct and animation helper functions.|
|`src/position.rs`|Vec2 and Hitbox types used for collision detection.|
|`src/rand.rs`|PCG32 RNG from Rust Rand crate.|
|`src/system.rs`|API used for graphics, sound and input.|
|`tic80_api`|Crate containing structs which I'm still using from the original TIC-80 Rust template. TODO: Remove|

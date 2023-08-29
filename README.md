# Egg Game

Egg-flavoured video game.

Simulation/adventure game made with Bevy. A work-in-progress.

Originally a [TIC-80 game](http://tic80.com/play?cart=3193) using the Rust bindings to the WASM API, but I hit the 80kb memory limit 🤷. Ported to Bevy to simplify build process and make debugging easier. As a result of porting, it does not use much Bevy functionality for now (Mostly just file-loading and multiple platform support).

Play the original version at [http://tic80.com/play?cart=3193](http://tic80.com/play?cart=3193)

![Screenshot](screen.png)

Controls are:
* Arrow Keys: Navigate
* Z: Activate
* X: Inventory / Cancel
* M: Toggle debug info

## Build / Run
To build or run, first make sure you have [Rust installed](https://rustup.rs/). 

After that, clone the repository then enter the directory:

```
git clone https://github.com/Madadog/Egg-Game
cd Egg-Game
```

Run: 

```
cargo run --release
```

Build:

```
cargo build --release
```

# Repo Layout
The layout of the repo is a bit scrambled. Rough guide for navigation:
|Folder|Description|
|---|---|
|`assets`|Sprite sheets, sound files, fonts and world maps.|
|`egg_core`|Base game code.|
|`src/fantasy_console.rs`|Interface for graphics, sound and input to/from base game|
|`src/fantasy_console/drawing.rs`|Structs used for drawing.|
|`src/main.rs`|Bevy wrapper around `egg_core`|
|`src/tiled.rs`|Loads maps from the Tiled Map Editor.|

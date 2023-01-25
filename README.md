# Egg Game

Simple TIC-80 game project using the Rust / TIC-80 starter template.

Collect the eggs. Hatch mind-stopping prizes! Avoid various hazards along the way.

## Building / Running

To run, just load the game.tic file into TIC-80 (Requires TIC-80 version 1.0 or above).

To build, first make sure you have installed the `wasm32-unknown-unknown` target using rustup:

```
rustup target add wasm32-unknown-unknown
```

Then, to build a cart.wasm file, run:

```
cargo build --release
```

To import the resulting WASM to a cartridge:

```
tic80 --fs . --cmd 'load game.tic & import binary target/wasm32-unknown-unknown/release/cart.wasm & save'
```

Or from the TIC-80 console:

```
load game.tic
import binary target/wasm32-unknown-unknown/release/cart.wasm
save
```

This is assuming you've run TIC-80 with `--fs .` inside your project directory.


## wasm-opt
It is highly recommended that you run `wasm-opt` on the output `cart.wasm` file, especially if using the usual unoptimised builds. To do so, make sure `wasm-opt` is installed, then run:
```
wasm-opt -Os target/wasm32-unknown-unknown/release/cart.wasm -o cart.wasm
```
This will create a new, smaller `cart.wasm` file in the working directory.

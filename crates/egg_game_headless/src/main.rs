//! The `egg_game_headless` binary: a native, window-less way to boot the game to
//! a chosen state, drive it with scripted input, and capture PNG screenshots.
//! All the logic lives in the crate's library ([`egg_game_headless::run`]); this
//! is just its entry point. Invoke it as `cargo run -p egg_game_headless -- …`
//! (or run the built binary directly) — see `--help` for the options.

fn main() {
    // The harness is native-only (it reads assets off disk and decodes PNGs with
    // the `image` crate), so `run` exists only off wasm. The crate is a workspace
    // member checked on the wasm target too, where this `main` compiles to a
    // no-op — the binary is never actually built for the browser.
    #[cfg(not(target_arch = "wasm32"))]
    egg_game_headless::run();
}

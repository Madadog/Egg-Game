//! The engine's no-Bevy home for the shared per-frame funnel and the headless
//! CLI test harness.
//!
//! [`run_frame`] is THE single frame funnel both frontends drive: the Bevy host's
//! `EggGame::run` delegates to it (on native and web), and the headless harness
//! ([`run`]) drives the identical funnel around its own [`ConsoleApi`]. It lives
//! here — in a crate that links no Bevy — so an agent iterating on engine code
//! gets a fast `cargo run -p egg_game_headless` loop: boot the game to a chosen
//! state, script input, and capture PNG screenshots without a window or a GPU.
//!
//! The harness itself ([`run`] and its module) is native-only: it reads bundled
//! assets off disk and decodes PNGs with the `image` crate. [`run_frame`] is
//! deliberately *not* gated — the web host calls it too, so it must compile for
//! wasm (it touches no filesystem, no Bevy, and no `image`).
//!
//! [`decode_png`] is also exported for the Bevy host to reuse directly: it
//! needs a Bevy-free PNG decode for asset hot-reload, which runs outside
//! Bevy's async asset loader (see `src/hot_reload.rs`).

use egg_core::EggState;
use egg_core::platform::{ConsoleApi, EggInput};
use egg_editor::map::MapViewer;

#[cfg(not(target_arch = "wasm32"))]
mod harness;
#[cfg(not(target_arch = "wasm32"))]
pub use harness::run;
#[cfg(not(target_arch = "wasm32"))]
pub use harness::decode_png;

/// Drive one whole frame around an arbitrary [`ConsoleApi`]: advance the sim,
/// then — while the primary map editor is focused (in walkaround, no scrubber,
/// not in text mode) — step it, drain its five `pending_*` requests, and draw
/// its overlay back over the composited world.
///
/// This is the single shared funnel: the Bevy host's `EggGame::run` destructures
/// itself and delegates here (a behaviour-preserving move — no logic change),
/// and the headless harness ([`run`]) runs the identical funnel — the same editor
/// step, the same drains, the same re-composite — over its own console. Keeping
/// it in this Bevy-free crate is what lets the harness link without Bevy.
pub fn run_frame(
    state: &mut EggState,
    system: &mut impl ConsoleApi,
    input: &EggInput,
    map_viewer: &mut MapViewer,
    text_mode: bool,
) {
    state.run(system, input, map_viewer.focused);

    // The primary map editor is stepped + drawn over the world only in
    // walkaround, with no scrubber and not in text mode. (The pause path never
    // reaches here, so a paused editor stays frozen, as before.)
    if map_viewer.focused
        && matches!(state.gamestate, egg_core::gamestate::GameMode::Walkaround)
        && state.scrubber.is_none()
        && !text_mode
    {
        // A running cutscene preempts the editor *step* (it kept winning the
        // frame inside `WalkaroundState::step`, which early-returned before the
        // editor block); the overlay is still drawn below, cutscene or not.
        if state.walkaround.cutscene.is_empty() {
            // Hand the editor the engine-owned snapshots its panels list — the
            // scene picker's cutscene names, the preset palette, and the live
            // actors the recorder picks from (it can't see the registries or
            // the entity tree itself). Refreshed each focused frame, so a
            // just-recorded scene shows up.
            map_viewer.scene_defs = state.scenes.named_defs();
            map_viewer.preset_defs = state.presets.named_defs();
            map_viewer.recorder_actors = state.walkaround.recorder_actors();
            let sheet = (
                state.draw_state.indexed_sprites.width() as usize / 8,
                state.draw_state.indexed_sprites.height() as usize / 8,
            );
            map_viewer.step_map_viewer(
                system,
                input,
                &mut state.walkaround.current_map,
                &mut state.maps,
                state.walkaround.camera.pos,
                sheet,
                &state.script,
                &state.save,
            );
            // The browser can't resolve a map itself (it lacks the sprite
            // sheet), so it parks the request; load it through the tested path.
            if let Some((name, focus)) = map_viewer.pending_open.take() {
                {
                    let mut ctx = egg_core::Ctx {
                        draw: &mut state.draw_state,
                        system: &mut *system,
                        input,
                        maps: &mut state.maps,
                        rng: &mut state.rng,
                        script: &state.script,
                        scenes: &state.scenes,
                        save: &mut state.save,
                        items: &state.items,
                        presets: &state.presets,
                        font: &state.font,
                    };
                    state.walkaround.load_map_by_name(&mut ctx, &name);
                }
                // A warp "open" carries its landing point: frame it as gameplay
                // would when the player arrives there.
                if let Some(p) = focus {
                    state
                        .walkaround
                        .center_camera_on(p, system.width(), system.height());
                }
            }
            // The editor never gets `&mut` engine state, so its un-take /
            // re-take test toggle parks the object's `<map>#<id>` key here; flip
            // it in `save.taken` now.
            if let Some(key) = map_viewer.pending_taken_toggle.take() {
                state.save.toggle_taken(&key);
            }
            // A layer or Setup edit changed the stored map: re-derive the
            // runtime layer lists and the scalar metadata (bg colour, camera
            // framing), so a colour / camera / resize edit applies live. Objects
            // and the player stay as they are. (The PRIMARY drain calls
            // `apply_map_framing`; the views' drain deliberately does not.)
            if map_viewer.pending_reload {
                map_viewer.pending_reload = false;
                if let Some(fresh) = egg_core::world::map::map_by_name(
                    &state.draw_state.indexed_sprites,
                    &state.walkaround.current_map.source,
                    &state.maps,
                ) {
                    state.walkaround.apply_map_framing(system, &fresh);
                    state.walkaround.current_map.bg_colour = fresh.bg_colour;
                    state.walkaround.current_map.camera_bounds = fresh.camera_bounds;
                    state.walkaround.current_map.layers = fresh.layers;
                    state.walkaround.current_map.fg_layers = fresh.fg_layers;
                    // Sprite-plane layers + their derived components re-derive
                    // too, so an editor paint/plane-cycle recomputes the
                    // y-sorting blobs live (a stale `sprite_components` would
                    // draw the old shape).
                    state.walkaround.current_map.sprite_layers = fresh.sprite_layers;
                    state.walkaround.current_map.sprite_components = fresh.sprite_components;
                }
            }
            // The editor can request a scrubber (the `P` shortcut, or save-and-
            // play in the recorder); open it here, where the full state is in
            // reach. A recorded def opens directly — no registry round-trip.
            if let Some(req) = map_viewer.pending_scrub.take() {
                match req {
                    egg_core::data::scene::ScrubRequest::ByName(name) => {
                        state.open_scrubber(&name)
                    }
                    egg_core::data::scene::ScrubRequest::Recorded(name, def) => {
                        state.open_scrubber_def(name, def)
                    }
                }
            }
            // A walk-sprite editor save rewrote `data.toml`: re-install the live
            // item/preset registries from the store so the next spawn uses the
            // edit (works on web too, where no mtime watcher notices the write).
            if map_viewer.pending_data_reload {
                map_viewer.pending_data_reload = false;
                state.reload_data(system);
            }
        }

        // Draw the editor overlay over the world the sim already composited
        // into the output — unconditionally within the gate, since `draw_at`
        // ran every walkaround frame before the inversion (cutscene or not).
        // This re-composites the world+editor a second time on editor-open
        // frames: an accepted cost of hoisting the overlay out of the engine's
        // `draw`.
        map_viewer.draw_at(
            &mut state.draw_state,
            input,
            &state.font,
            &state.walkaround.current_map,
            &state.maps,
            state.walkaround.camera.pos,
        );
        egg_core::gamestate::walkaround::WalkaroundState::composite_into(
            &mut state.draw_state,
            system.output_image(),
        );
    }
}

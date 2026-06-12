//! Edge-triggered hotkeys (pause, fullscreen, screen modes, debug/cheat keys…).
//!
//! These run in the `Update` schedule — the canonical Bevy home for reacting to
//! discrete input. `ButtonInput::just_pressed` is per-*render-frame* state
//! (cleared in `PreUpdate`), and `Update` runs exactly once per frame, so a key
//! tap fires exactly once. In `FixedUpdate` (where the simulation steps) the
//! same flag is seen by every catch-up step of a lagging frame, which is how
//! one `F8` used to spawn several windows.
//!
//! Held-key behaviours stay in the fixed step on purpose: the player
//! controller, `N` fast-forward and the `Digit3` entity spam repeat *per
//! simulation step* (`step_state`), so they stay frame-rate independent.

use bevy::prelude::*;

use egg_core::gamestate::GameMode;

use crate::{EggGame, ScaleMode, ScreenMode, views};

/// Primary-window debug/control hotkeys plugin.
///
/// Registers:
/// * `Update`: [`primary_hotkeys`] (window/screen modes, `F8` view spawning,
///   pause/single-step, and the debug/cheat toggles). The `Update` schedule is
///   required for correctness — see the module docs on why edge-triggered keys
///   must not live in `FixedUpdate`.
pub struct HotkeysPlugin;

impl Plugin for HotkeysPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, primary_hotkeys);
    }
}

/// Primary-window hotkeys: window/screen modes, F8 view spawning, pause, and
/// the debug/cheat toggles. The same suppression rules `step_state` applies to
/// its held keys (editor typing, window focus, editor-owns-keyboard) come from
/// the shared [`views::InputRouting`], so the two stay in lock-step by sharing
/// one derivation rather than by hand-kept parallel checks.
#[allow(clippy::too_many_arguments)]
pub fn primary_hotkeys(
    mut game: ResMut<EggGame>,
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<(Entity, &mut Window, Has<bevy::window::PrimaryWindow>)>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut views: ResMut<views::ViewWindows>,
) {
    if !game.loaded {
        return;
    }

    // Same routing decisions as `step_state`, from the shared brain
    // ([`views::InputRouting`]): most hotkeys only apply while the primary window
    // is focused, so keys aimed at an extra view (or its editor) can't fire them.
    // Computed here (in `Update`) at this schedule's moment — see `InputRouting`
    // for why each consumer computes its own rather than sharing a `Resource`.
    let focused_entity = windows.iter().find(|(_, w, _)| w.focused).map(|(e, ..)| e);
    let routing = views::InputRouting::compute(focused_entity, &game, &views);
    let drives_player = routing.drives_player;

    // Window/screen-mode hotkeys drive the PRIMARY window's framebuffer.
    if let Some((_, mut window, _)) = windows.iter_mut().find(|(.., primary)| *primary) {
        if keys.just_pressed(KeyCode::F11) {
            use bevy::window::WindowMode;
            window.mode = match window.mode {
                WindowMode::Windowed => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                _ => WindowMode::Windowed,
            };
        }
        if keys.just_pressed(KeyCode::F5) {
            game.scale_mode = match game.scale_mode {
                ScaleMode::Linear => ScaleMode::Integer,
                _ => ScaleMode::Linear,
            };
        }
        // F2 toggles fixed-resolution Fit vs. window-mirroring. F3 cycles the
        // Mirror pixel ratio 1→2→4→8 — and since that ratio only means anything
        // in Mirror, F3 also flips Fit→Mirror so it's never a silent no-op (the
        // "F3 does nothing in Fit" surprise). Both log so it's clear the key
        // registered and what state you're now in. Both only apply while the
        // PRIMARY window is focused — F3 over an extra view cycles *that* view's
        // pixel ratio instead (see [`views::view_hotkeys`]).
        if drives_player && keys.just_pressed(KeyCode::F2) {
            game.screen_mode = match game.screen_mode {
                ScreenMode::Fit => ScreenMode::Mirror,
                ScreenMode::Mirror => ScreenMode::Fit,
            };
            let mode = if matches!(game.screen_mode, ScreenMode::Fit) { "Fit" } else { "Mirror" };
            info!("Screen mode: {mode} ({}x)", game.mirror_scale);
        }
        if drives_player && keys.just_pressed(KeyCode::F3) {
            if matches!(game.screen_mode, ScreenMode::Fit) {
                // Enter Mirror first (at the current ratio) so F3 always shows
                // a visible change rather than silently doing nothing in Fit.
                game.screen_mode = ScreenMode::Mirror;
            } else {
                game.mirror_scale = match game.mirror_scale {
                    1 => 2,
                    2 => 4,
                    4 => 8,
                    _ => 1,
                };
            }
            info!("Screen mode: Mirror ({}x)", game.mirror_scale);
        }
    }

    // F8 spawns an extra walkaround window with its own free camera, starting at
    // the main camera's current position. Only in walkaround (where the camera
    // is meaningful and the editor lives).
    if keys.just_pressed(KeyCode::F8) && matches!(game.state.gamestate, GameMode::Walkaround) {
        let start = game.state.walkaround.camera.pos;
        views::spawn_view(
            &mut commands,
            &mut images,
            &mut views,
            start,
            &game.state.draw_state,
        );
    }

    // Everything below is a letter/digit key, so it's suppressed while any map
    // editor is capturing typed text — dialogue labels like "town_lamppost"
    // must not toggle pause or fire the m/n/k/l shortcuts.
    if routing.editor_typing {
        return;
    }

    // Pause works from any window (it freezes the shared sim that every view
    // renders); N single-steps while paused. The overlay itself is drawn by
    // `step_state`, which keeps early-returning while `pause` is set.
    if keys.just_pressed(KeyCode::KeyP) {
        game.pause = !game.pause;
    }
    if game.pause {
        if keys.just_pressed(KeyCode::KeyN) {
            game.run();
        }
        return;
    }

    // The debug/cheat keys below are primary-window gameplay shortcuts: skip
    // them when an extra view owns the keyboard…
    if !drives_player {
        return;
    }
    // …or when the primary map editor is open — it owns the keyboard, so only
    // `L` (toggle the editor off) passes, while the editor's own shortcuts
    // (Ctrl+Z/Y/S, Delete, 1-4) are read inside `step_map_viewer` via the
    // shared console.
    if routing.primary_editor_open {
        if keys.just_pressed(KeyCode::KeyL) {
            game.state.walkaround.map_viewer.focused = false;
        }
        return;
    }

    if keys.just_pressed(KeyCode::KeyD) && keys.pressed(KeyCode::ShiftLeft) {
        let d = &mut game.state.debug_info;
        d.player_info = !d.player_info;
    }
    if keys.just_pressed(KeyCode::KeyM) {
        let d = &mut game.state.debug_info;
        d.map_info = !d.map_info;
    }
    if keys.just_pressed(KeyCode::KeyN) {
        let d = &mut game.state.debug_info;
        d.memory_info = !d.memory_info;
    }
    // Shift+digit: swap player one for a preset shell.
    if keys.pressed(KeyCode::ShiftLeft) {
        use egg_core::player::Shell;
        let swap = if keys.just_pressed(KeyCode::Digit1) {
            Some(Shell::ellie())
        } else if keys.just_pressed(KeyCode::Digit2) {
            Some(Shell::may())
        } else if keys.just_pressed(KeyCode::Digit4) {
            Some(Shell::dog())
        } else if keys.just_pressed(KeyCode::Digit5) {
            Some(Shell::bro())
        } else {
            None
        };
        if let Some(shell) = swap {
            game.state.walkaround.player().replace(shell);
        }
    }
    if keys.just_pressed(KeyCode::Digit6) && keys.pressed(KeyCode::ShiftLeft) {
        let player = game.state.walkaround.player().clone();
        if let Some(shell) = game.state.walkaround.entities.get_mut(0) {
            let temp = shell.clone();
            *shell = player;
            *game.state.walkaround.player() = temp;
        }
    }

    if keys.just_pressed(KeyCode::KeyL) && keys.pressed(KeyCode::ShiftLeft) {
        info!("------------------------");
        info!("START CURRENT MAP");
        info!("------------------------");
        info!("{:#?}", game.state.walkaround.current_map);
        info!("------------------------");
        info!("END CURRENT MAP");
        info!("------------------------");
    } else if keys.just_pressed(KeyCode::KeyL) {
        game.state.walkaround.map_viewer.focused = !game.state.walkaround.map_viewer.focused;
        game.state.walkaround.map_viewer.layer_index = 0;
    }
    if keys.just_pressed(KeyCode::Semicolon) && !game.state.walkaround.current_map.layers.is_empty()
    {
        info!("------------------------");
        info!("REMOVED BG LAYER");
        info!("{:#?}", game.state.walkaround.current_map.layers.remove(0));
        info!("------------------------");
    }
    if keys.just_pressed(KeyCode::Quote) && !game.state.walkaround.current_map.fg_layers.is_empty()
    {
        info!("------------------------");
        info!("REMOVED FG LAYER");
        info!(
            "{:#?}",
            game.state.walkaround.current_map.fg_layers.remove(0)
        );
        info!("------------------------");
    }
    if keys.just_pressed(KeyCode::KeyK) {
        game.state.gamestate = GameMode::SpriteTest(0);
    }
}

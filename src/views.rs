//! Extra walkaround windows. Each opens a second OS window showing the same
//! game world through its own **independent free camera** (panned with the arrow
//! keys when that window is focused) and can run the in-game map editor (`L`) on
//! its own view. The primary window keeps normal gameplay: the player moves with
//! the arrows/WASD and its camera follows the player.
//!
//! Architecture (the Bevy multi-window way, with one shared console):
//! - Every extra view is a [`Window`] entity + a [`Camera2d`] whose
//!   [`RenderTarget`] is that window and which carries a distinct
//!   [`RenderLayers`]; the view's screen [`Sprite`] carries the same layer. So
//!   each camera renders only its own view's sprite (the main camera/sprite stay
//!   on the default layer 0).
//! - Each view owns its own framebuffer: an `RgbaImage` `output`, a `DrawState`
//!   (the layer canvases), a free-camera position, and a `MapViewer` (the
//!   editor). The shared [`FantasyConsole`](crate::fantasy_console::FantasyConsole)
//!   is read-only here (assets/maps), so rendering an extra view never touches
//!   the main window's framebuffer or the player.
//! - `egg_core` exposes `WalkaroundState::draw_world` /
//!   `composite_into` (engine-agnostic) to render the world from an arbitrary
//!   camera into a given `DrawState` + output — that's what each view calls.

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::window::{WindowClosed, WindowRef};

use egg_core::drawstate::DrawState;
use egg_core::gamestate::mapeditor::MapViewer;
use egg_core::position::Vec2 as EggVec2;

use crate::{EggGame, new_screen_image};

/// Marker for an extra view's screen sprite. Deliberately distinct from the main
/// window's `GameScreenSprite` so the main `update_texture`/`resize_screen`
/// systems never blit the main framebuffer into (or rescale) an extra view.
#[derive(Component)]
pub struct ViewScreenSprite;

/// Internal resolution each extra view renders at. Fixed (the base resolution),
/// matching the classic look; the window is bigger and the sprite scales to fit.
const VIEW_W: u32 = egg_core::system::WIDTH as u32;
const VIEW_H: u32 = egg_core::system::HEIGHT as u32;

/// Free-camera pan speed (framebuffer px per fixed step), and the faster speed
/// while a Shift key is held.
const CAM_SPEED: i16 = 2;
const CAM_SPEED_FAST: i16 = 5;

/// One extra walkaround window: its OS window + render entities, its private
/// framebuffer/draw state, and its free camera + editor.
pub struct ViewWindow {
    /// The OS [`Window`] entity.
    pub window: Entity,
    /// The [`Camera2d`] rendering this view (despawned with the window).
    pub camera: Entity,
    /// The screen [`Sprite`] entity showing `image`.
    pub sprite: Entity,
    /// GPU texture this view's framebuffer is blitted into each frame.
    pub image: Handle<Image>,
    /// This view's final composited frame (size `VIEW_W`×`VIEW_H`).
    pub output: egg_core::system::drawing::image::RgbaImage,
    /// This view's private layer canvases (never the main `EggState.draw_state`).
    pub draw_state: DrawState,
    /// Independent free camera, panned by the arrow keys while focused.
    pub free_cam: EggVec2,
    /// This view's own map editor (toggled with `L` while focused).
    pub editor: MapViewer,
}

/// All currently-open extra views. Empty until the first `F8`.
#[derive(Resource, Default)]
pub struct ViewWindows {
    pub views: Vec<ViewWindow>,
    /// Monotonic counter so each new view gets a fresh, unique render layer even
    /// after others close (layers double as `Camera.order`, must stay distinct).
    next_layer: usize,
}

impl ViewWindows {
    /// True while any extra window has its map editor focused — the host then
    /// suppresses global debug hotkeys (same rule as the primary editor) so
    /// typed dialogue keys don't fire them.
    pub fn any_editor_typing(&self) -> bool {
        self.views.iter().any(|v| v.editor.is_typing())
    }
}

/// Spawn one extra walkaround window, with its own camera (render layer + order),
/// screen sprite, and framebuffer. The free camera starts at `start_cam` (the
/// main camera's current position). `main_draw` is the loaded main `DrawState`:
/// the sprite sheets/flags are copied from it into the view's own draw state
/// (a bare `DrawState::default()` has an empty sheet, which the tile blitter
/// can't draw from).
pub fn spawn_view(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    views: &mut ViewWindows,
    start_cam: EggVec2,
    main_draw: &DrawState,
) {
    views.next_layer += 1;
    let layer = views.next_layer;

    let window = commands
        .spawn(Window {
            title: format!("Egg Game — view {layer}"),
            resolution: (VIEW_W * 3, VIEW_H * 3).into(),
            ..default()
        })
        .id();

    let image = images.add(new_screen_image(VIEW_W, VIEW_H));

    // Render only this view's layer, to this view's window. `RenderTarget` is a
    // separate (required) component in Bevy 0.18, not a `Camera` field. `order`
    // is bumped so the extra cameras have distinct, non-zero orders (the main
    // camera is 0).
    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: layer as isize,
                ..default()
            },
            RenderTarget::Window(WindowRef::Entity(window)),
            RenderLayers::layer(layer),
        ))
        .id();

    let sprite = commands
        .spawn((
            Sprite {
                image: image.clone(),
                ..default()
            },
            Transform::from_xyz(0., 0., 0.),
            RenderLayers::layer(layer),
            ViewScreenSprite,
        ))
        .id();

    // Give the view its own draw state, but share the loaded sprite assets —
    // the default state's sheets are empty (0×0) and would render nothing.
    let mut draw_state = DrawState::default();
    draw_state.rgba_sprites = main_draw.rgba_sprites.clone();
    draw_state.indexed_sprites = main_draw.indexed_sprites.clone();
    draw_state.sprite_flags = main_draw.sprite_flags.clone();

    views.views.push(ViewWindow {
        window,
        camera,
        sprite,
        image,
        output: egg_core::system::drawing::image::RgbaImage::new(VIEW_W, VIEW_H),
        draw_state,
        free_cam: start_cam,
        editor: MapViewer::default(),
    });
    info!("Opened extra view window (layer {layer}); {} open.", views.views.len());
}

/// Despawn an extra view's render entities and its `ViewWindow`. The OS window
/// itself is closed by Bevy (close button) or despawned by the caller.
fn drop_view(commands: &mut Commands, view: &ViewWindow) {
    commands.entity(view.camera).despawn();
    commands.entity(view.sprite).despawn();
}

/// Free-camera pan delta (framebuffer px) for the held arrow keys this step.
/// Pure so the routing/speed logic is unit-testable. `fast` (Shift) swaps in the
/// faster speed.
pub fn free_cam_delta(up: bool, down: bool, left: bool, right: bool, fast: bool) -> (i16, i16) {
    let speed = if fast { CAM_SPEED_FAST } else { CAM_SPEED };
    let dx = i16::from(right) - i16::from(left);
    let dy = i16::from(down) - i16::from(up);
    (dx * speed, dy * speed)
}

/// Which window currently owns keyboard input. The primary window drives the
/// player; an extra window drives that view's free camera/editor instead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Primary,
    /// Index into [`ViewWindows::views`].
    Extra(usize),
}

/// Routing rule: the player controller is only driven while the **primary**
/// window is focused, so arrow keys panning an extra view's camera can't also
/// move the player. Pure + unit-tested.
pub fn drives_player(focus: Focus) -> bool {
    matches!(focus, Focus::Primary)
}

/// Resolve which window is focused into a [`Focus`]. Returns [`Focus::Extra`]
/// only when the focused window matches one of `views` (else primary). Pure so
/// the mapping from a focused `Entity` to a view index is unit-testable.
pub fn resolve_focus(focused: Option<Entity>, view_windows: &[Entity]) -> Focus {
    match focused {
        Some(e) => view_windows
            .iter()
            .position(|w| *w == e)
            .map(Focus::Extra)
            .unwrap_or(Focus::Primary),
        None => Focus::Primary,
    }
}

/// Drop the `ViewWindow` bookkeeping (and render entities) for any extra window
/// the user closed with its OS close button. Bevy despawns the `Window` entity
/// itself and raises `WindowClosed`.
pub fn handle_closed_views(
    mut closed: MessageReader<WindowClosed>,
    mut commands: Commands,
    mut views: ResMut<ViewWindows>,
) {
    for event in closed.read() {
        if let Some(i) = views.views.iter().position(|v| v.window == event.window) {
            let view = views.views.remove(i);
            drop_view(&mut commands, &view);
            info!("Closed extra view window; {} open.", views.views.len());
        }
    }
}

/// Per-frame update for every extra view: pan/route input for the focused view,
/// then render the world (or its editor) from each view's free camera into its
/// own framebuffer and blit that into its GPU texture. The main window is never
/// touched here.
pub fn update_views(
    mut game: ResMut<EggGame>,
    mut views: ResMut<ViewWindows>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<(Entity, &Window)>,
    mut images: ResMut<Assets<Image>>,
) {
    if !game.loaded || views.views.is_empty() {
        return;
    }

    // Which window is focused? `Window.focused` is the live OS focus state.
    let focused = windows
        .iter()
        .find(|(_, w)| w.focused)
        .map(|(e, _)| e);
    let view_entities: Vec<Entity> = views.views.iter().map(|v| v.window).collect();
    let focus = resolve_focus(focused, &view_entities);

    // Route arrow keys + `L` to the focused extra view (if any). The player is
    // handled in `step_state`, which reads the same `Focus` to decide whether to
    // drive the controller — so we only move a free camera / toggle an editor.
    if let Focus::Extra(i) = focus {
        let fast = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        let editor_typing = views.views[i].editor.is_typing();
        // While the editor captures text, leave panning + the `L` toggle alone
        // so typed keys (incl. a literal "l") don't pan or close the editor.
        if !editor_typing {
            let (dx, dy) = free_cam_delta(
                keys.pressed(KeyCode::ArrowUp),
                keys.pressed(KeyCode::ArrowDown),
                keys.pressed(KeyCode::ArrowLeft),
                keys.pressed(KeyCode::ArrowRight),
                fast,
            );
            let cam = &mut views.views[i].free_cam;
            cam.x = cam.x.saturating_add(dx);
            cam.y = cam.y.saturating_add(dy);

            if keys.just_pressed(KeyCode::KeyL) {
                let editor = &mut views.views[i].editor;
                editor.focused = !editor.focused;
                editor.layer_index = 0;
            }
        }

        // The shared console's mouse was already mapped from the focused window's
        // cursor in `step_state`. Step this view's editor against its own map +
        // free camera so painting/placing works on the extra window.
        if views.views[i].editor.focused {
            let cam = views.views[i].free_cam;
            let g = &mut *game;
            views.views[i].editor.step_map_viewer(
                &mut g.system,
                &mut g.state.walkaround.current_map,
                cam,
            );
        }
    }

    // Render + present every extra view from its own free camera. The view's
    // DrawState + output are created at the fixed `VIEW_W`×`VIEW_H` and never
    // change size, so no per-frame reallocation is needed.
    let g = &mut *game;
    for view in views.views.iter_mut() {
        // Draw the world from this view's free camera + editor into its own
        // DrawState/output — never the main framebuffer.
        g.state.walkaround.draw_world(
            &mut view.draw_state,
            &mut g.system,
            view.free_cam,
            &view.editor,
            &g.state.debug_info,
        );
        egg_core::gamestate::walkaround::WalkaroundState::composite_into(
            &mut view.draw_state,
            &mut view.output,
        );
    }

    // Blit each view's finished frame into its GPU texture.
    for view in views.views.iter() {
        if let Some(image) = images.get_mut(&view.image)
            && let Some(data) = image.data.as_mut()
        {
            data.copy_from_slice(view.output.data());
        }
    }
}

/// Scale each extra view's screen sprite to fill its window (centred fit), the
/// same letterboxed integer/linear fit the main window uses, but always at the
/// fixed `VIEW_W`×`VIEW_H` internal resolution.
pub fn resize_views(
    views: Res<ViewWindows>,
    windows: Query<&Window>,
    mut transforms: Query<&mut Transform, With<ViewScreenSprite>>,
) {
    for view in views.views.iter() {
        let Ok(window) = windows.get(view.window) else {
            continue;
        };
        let Ok(mut transform) = transforms.get_mut(view.sprite) else {
            continue;
        };
        let scale = (window.width() / VIEW_W as f32).min(window.height() / VIEW_H as f32);
        transform.scale = Vec3::new(scale, scale, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_cam_delta_directions_and_speed() {
        // No keys: no movement.
        assert_eq!(free_cam_delta(false, false, false, false, false), (0, 0));
        // Right + down at base speed.
        assert_eq!(
            free_cam_delta(false, true, false, true, false),
            (CAM_SPEED, CAM_SPEED)
        );
        // Up + left at base speed (negative).
        assert_eq!(
            free_cam_delta(true, false, true, false, false),
            (-CAM_SPEED, -CAM_SPEED)
        );
        // Shift swaps in the faster speed.
        assert_eq!(
            free_cam_delta(false, false, false, true, true),
            (CAM_SPEED_FAST, 0)
        );
        // Opposing keys cancel out.
        assert_eq!(free_cam_delta(true, true, true, true, false), (0, 0));
    }

    #[test]
    fn player_only_driven_by_primary() {
        assert!(drives_player(Focus::Primary));
        assert!(!drives_player(Focus::Extra(0)));
        assert!(!drives_player(Focus::Extra(3)));
    }

    #[test]
    fn focus_resolves_to_matching_view_else_primary() {
        // Stand-in entities for three windows: index 0 = primary, 1/2 = extra.
        let primary = Entity::from_raw_u32(10).unwrap();
        let extra_a = Entity::from_raw_u32(11).unwrap();
        let extra_b = Entity::from_raw_u32(12).unwrap();
        let views = [extra_a, extra_b];

        // Focused window is an extra view -> Extra(index into views).
        assert_eq!(resolve_focus(Some(extra_a), &views), Focus::Extra(0));
        assert_eq!(resolve_focus(Some(extra_b), &views), Focus::Extra(1));
        // Focused window is the primary (not in `views`) -> Primary.
        assert_eq!(resolve_focus(Some(primary), &views), Focus::Primary);
        // Nothing focused -> Primary (player keeps control; no view panned).
        assert_eq!(resolve_focus(None, &views), Focus::Primary);
    }
}

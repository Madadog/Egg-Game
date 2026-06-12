//! Extra walkaround windows. Each opens a second OS window showing the same
//! game world through its own **independent free camera** (panned with the arrow
//! keys when that window is focused) and can run the in-game map editor (`L`) on
//! its own view. The primary window keeps normal gameplay: the player moves with
//! the arrows/WASD and its camera follows the player. Each view is resizable:
//! its framebuffer follows its window (Mirror-style) at a per-view pixel ratio
//! cycled with `F3` while that view is focused.
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
//!   camera into a `Ctx` built around the view's own `DrawState`, then out to
//!   the view's framebuffer — that's what each view calls.

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::window::{WindowClosed, WindowRef};

use egg_core::drawstate::DrawState;
use egg_core::gamestate::mapeditor::MapViewer;
use egg_core::position::Vec2 as EggVec2;

use crate::EggGame;
use crate::fantasy_console::{MIN_FB_H, MIN_FB_W, new_screen_image};

/// Extra-view windows plugin. Owns the [`ViewWindows`] resource and the
/// view-management systems.
///
/// Registers:
/// * `Update`: [`view_hotkeys`] (edge-triggered per-view `L`/`F3`),
///   [`resize_views`] (scale each view's sprite to its window), and
///   [`handle_closed_views`] (reap a view closed via its OS button) — registered
///   unordered, exactly as in the original `main.rs` Update tuple.
///
/// The per-fixed-step [`update_views`] is *not* added here: it is a member of
/// the single ordered `FixedUpdate` chain assembled by `CorePlugin` in
/// `main.rs` (its position after `step_state` is load-bearing — `step_state`
/// maps the focused window's cursor before this renders each view), so it must
/// be registered there to keep that ordering.
pub struct ViewsPlugin;

impl Plugin for ViewsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewWindows>().add_systems(
            Update,
            (view_hotkeys, resize_views, handle_closed_views),
        );
    }
}

/// Marker for an extra view's screen sprite. Deliberately distinct from the main
/// window's `GameScreenSprite` so the main `update_texture`/`resize_screen`
/// systems never blit the main framebuffer into (or rescale) an extra view.
#[derive(Component)]
pub struct ViewScreenSprite;

/// Internal resolution each extra view *starts* at (the base resolution). The
/// framebuffer then follows the window — like the main window's Mirror mode —
/// at the view's pixel ratio ([`ViewWindow::scale`], cycled with `F3`).
const VIEW_W: u32 = egg_core::system::WIDTH as u32;
const VIEW_H: u32 = egg_core::system::HEIGHT as u32;

/// Pixel ratio a fresh view starts at (window px per framebuffer px); the spawn
/// resolution is the base resolution at this ratio, so a new view looks exactly
/// like the classic fixed-resolution one until resized.
const VIEW_SCALE: u32 = 3;

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
    /// Window px per framebuffer px — the view's framebuffer is the window size
    /// divided by this (Mirror-style), so resizing the window resizes the view.
    /// Cycled 1→2→4→8 with `F3` while the view is focused.
    pub scale: u32,
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
            resolution: (VIEW_W * VIEW_SCALE, VIEW_H * VIEW_SCALE).into(),
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
    let draw_state = DrawState {
        rgba_sprites: main_draw.rgba_sprites.clone(),
        indexed_sprites: main_draw.indexed_sprites.clone(),
        sprite_flags: main_draw.sprite_flags.clone(),
        ..DrawState::default()
    };

    views.views.push(ViewWindow {
        window,
        camera,
        sprite,
        image,
        output: egg_core::system::drawing::image::RgbaImage::new(VIEW_W, VIEW_H),
        draw_state,
        free_cam: start_cam,
        editor: MapViewer::default(),
        scale: VIEW_SCALE,
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

/// One frame's keyboard/window-focus routing decisions, derived once by
/// [`InputRouting::compute`] so every consumer (the fixed-step
/// [`step_state`](crate::step_state), the [`hotkeys`](crate::hotkeys), and this
/// module's view systems) reads the same answers instead of re-deriving them by
/// hand and drifting apart.
///
/// Deliberately a plain value, **not** a Bevy `Resource`: `step_state` runs in
/// `FixedUpdate` and the hotkeys in `Update`, and the engine's typing/focus
/// state (`MapViewer`) mutates between those schedules. Each consumer therefore
/// computes its own routing at its own moment — exactly as the duplicated blocks
/// did — but now through one shared derivation rather than four hand-kept copies.
pub struct InputRouting {
    pub focus: Focus,
    pub drives_player: bool,
    /// A map editor (the primary window's or any extra view's) is capturing
    /// typed text — the host then suppresses its global letter/digit hotkeys so
    /// typed dialogue keys don't fire them.
    pub editor_typing: bool,
    /// The primary window's map editor is open (focused): it owns the keyboard,
    /// so the primary held-key cheats are suppressed and only its `L`-off toggle
    /// passes through.
    pub primary_editor_open: bool,
}

impl InputRouting {
    /// Derive this frame's routing from the focused window, the shared game
    /// state, and the open extra views. `focused_entity` is the OS-focused
    /// window (the caller queries `Window.focused`); `views` supplies the
    /// extra-view windows that [`resolve_focus`] maps against and whose editors
    /// feed `editor_typing`.
    pub fn compute(focused_entity: Option<Entity>, game: &EggGame, views: &ViewWindows) -> Self {
        let view_entities: Vec<Entity> = views.views.iter().map(|v| v.window).collect();
        let focus = resolve_focus(focused_entity, &view_entities);
        let map_viewer = &game.state.walkaround.map_viewer;
        Self {
            focus,
            drives_player: drives_player(focus),
            editor_typing: map_viewer.is_typing() || views.any_editor_typing(),
            primary_editor_open: map_viewer.focused,
        }
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

/// Edge-triggered hotkeys for the focused extra view: `L` toggles its map
/// editor, `F3` cycles its pixel ratio. In the `Update` schedule (like
/// [`hotkeys`](crate::hotkeys)) so `just_pressed` fires exactly once per tap —
/// the held-key panning stays in the fixed step ([`update_views`]).
pub fn view_hotkeys(
    game: Res<EggGame>,
    mut views: ResMut<ViewWindows>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<(Entity, &Window)>,
) {
    // Shared routing brain ([`InputRouting`]); these hotkeys only act on the
    // focused *extra* view, so anything else (primary focused, nothing focused)
    // falls through.
    let focused = windows.iter().find(|(_, w)| w.focused).map(|(e, _)| e);
    let Focus::Extra(i) = InputRouting::compute(focused, &game, &views).focus else {
        return;
    };
    let view = &mut views.views[i];

    // While the editor captures text, leave the `L` toggle alone so a typed
    // literal "l" doesn't close the editor. F3 is a function key — no clash.
    if !view.editor.is_typing() && keys.just_pressed(KeyCode::KeyL) {
        view.editor.focused = !view.editor.focused;
        view.editor.layer_index = 0;
    }
    // F3 cycles this view's pixel ratio (the framebuffer follows in
    // `update_views`), mirroring the main window's Mirror-ratio hotkey.
    if keys.just_pressed(KeyCode::F3) {
        view.scale = match view.scale {
            1 => 2,
            2 => 4,
            4 => 8,
            _ => 1,
        };
        info!("View pixel ratio: {}x", view.scale);
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

    // Which window is focused? `Window.focused` is the live OS focus state. Read
    // it through the shared routing brain ([`InputRouting`]) so this matches what
    // `step_state` decides for the player from the same moment.
    let focused = windows
        .iter()
        .find(|(_, w)| w.focused)
        .map(|(e, _)| e);
    let focus = InputRouting::compute(focused, &game, &views).focus;

    // Route the held arrow keys to the focused extra view's free camera (if
    // any). The player is handled in `step_state`, which reads the same `Focus`
    // to decide whether to drive the controller; the view's edge-triggered
    // hotkeys (`L`/`F3`) live in `view_hotkeys` (Update schedule).
    if let Focus::Extra(i) = focus {
        let fast = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        // While the editor captures text, leave panning alone so typed keys
        // don't pan the camera.
        if !views.views[i].editor.is_typing() {
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
        }

        // The shared console's mouse was already mapped from the focused window's
        // cursor in `step_state`. Step this view's editor against its own map +
        // free camera — at this view's framebuffer size, so the panel layout and
        // hit-testing match what `draw_at` renders below.
        if views.views[i].editor.focused {
            let cam = views.views[i].free_cam;
            let screen = (
                views.views[i].output.width() as f32,
                views.views[i].output.height() as f32,
            );
            let g = &mut *game;
            views.views[i].editor.step_map_viewer_at(
                &mut g.system,
                &mut g.state.walkaround.current_map,
                &mut g.state.maps,
                cam,
                screen,
            );
        }
    }

    // Reconcile each view's framebuffer with its window size ÷ pixel ratio
    // (Mirror-style), keeping the three lock-step buffers (draw state, output,
    // GPU texture) the same size — `update_views` blits them verbatim.
    for view in views.views.iter_mut() {
        let Ok((_, window)) = windows.get(view.window) else {
            continue;
        };
        let target = view_fb_size(window.width(), window.height(), view.scale);
        if (view.output.width(), view.output.height()) != target {
            view.draw_state.resize(target.0, target.1);
            view.output = egg_core::system::drawing::image::RgbaImage::new(target.0, target.1);
            if let Some(image) = images.get_mut(&view.image) {
                *image = new_screen_image(target.0, target.1);
            }
        }
    }

    // Render + present every extra view from its own free camera.
    let g = &mut *game;
    for view in views.views.iter_mut() {
        // Draw the world from this view's free camera + editor into its own
        // DrawState/output — never the main framebuffer.
        let mut ctx = egg_core::Ctx {
            draw: &mut view.draw_state,
            system: &mut g.system,
            maps: &mut g.state.maps,
            rng: &mut g.state.rng,
            script: &g.state.script,
            save: &mut g.state.save,
        };
        g.state
            .walkaround
            .draw_world(&mut ctx, view.free_cam, &view.editor, &g.state.debug_info);
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

/// A view's framebuffer size for its window size and pixel ratio: the window ÷
/// the ratio, floored at the same minimum as the main Mirror mode so a tiny
/// window or large ratio can't produce a degenerate framebuffer. Pure so the
/// sizing rule is unit-testable.
pub fn view_fb_size(window_w: f32, window_h: f32, scale: u32) -> (u32, u32) {
    let n = scale.max(1);
    (
        (window_w as u32 / n).max(MIN_FB_W),
        (window_h as u32 / n).max(MIN_FB_H),
    )
}

/// Scale each extra view's screen sprite by exactly its pixel ratio (crisp N×N
/// pixels, like the main window's Mirror mode) — the framebuffer itself follows
/// the window in `update_views`, so the scaled sprite always fills the window
/// (bar a sub-ratio remainder strip). Also pins the OS scale factor to 1 so
/// window units equal device pixels, matching the primary window.
pub fn resize_views(
    views: Res<ViewWindows>,
    mut windows: Query<&mut Window, Without<bevy::window::PrimaryWindow>>,
    mut transforms: Query<&mut Transform, With<ViewScreenSprite>>,
) {
    for view in views.views.iter() {
        let Ok(mut window) = windows.get_mut(view.window) else {
            continue;
        };
        window.resolution.set_scale_factor_override(Some(1.0));
        let Ok(mut transform) = transforms.get_mut(view.sprite) else {
            continue;
        };
        let scale = view.scale.max(1) as f32;
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
    fn view_fb_follows_window_at_ratio_with_floor() {
        // Window ÷ ratio, truncated.
        assert_eq!(view_fb_size(720.0, 408.0, 3), (240, 136));
        assert_eq!(view_fb_size(721.0, 409.0, 3), (240, 136));
        // Ratio 1 mirrors the window exactly.
        assert_eq!(view_fb_size(640.0, 480.0, 1), (640, 480));
        // Tiny window / large ratio is floored at the Mirror-mode minimum.
        assert_eq!(view_fb_size(100.0, 100.0, 8), (MIN_FB_W, MIN_FB_H));
        // A zero ratio is treated as 1, not a divide-by-zero.
        assert_eq!(view_fb_size(320.0, 240.0, 0), (320, 240));
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

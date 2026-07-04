//! Extra walkaround windows. Each opens a second OS window showing the same
//! game world through its own **independent free camera** (panned with the arrow
//! keys when that window is focused) and can run the in-game map editor (`L`) on
//! its own view. The primary window keeps normal gameplay: the player moves with
//! the arrows/WASD and its camera follows the player. Each view is resizable:
//! its framebuffer follows its window (Mirror-style) at a fixed base pixel ratio,
//! capped so the internal resolution never exceeds [`MAX_FB_W`]×[`MAX_FB_H`].
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

use egg_core::draw_state::DrawState;
use egg_core::platform::EggInput;
use egg_core::editor::map::MapViewer;
use egg_core::editor::text::{TextEditor, TextOpenReq};
use egg_core::geometry::Vec2 as EggVec2;

use crate::EggGame;
use crate::fantasy_console::{MIN_FB_H, MIN_FB_W, new_screen_image};

/// Extra-view windows plugin. Owns the [`ViewWindows`] resource and the
/// view-management systems.
///
/// Registers:
/// * `Update`: [`view_hotkeys`] (edge-triggered per-view `L`/`F1`/`F2`),
///   [`poll_text_open`] (act on the Dialog panel's "edit in text editor" link),
///   [`resize_views`] (scale each view's sprite to its window), and
///   [`handle_closed_views`] (reap a view closed via its OS button) — registered
///   unordered.
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
            (
                view_hotkeys,
                poll_text_open,
                resize_views,
                handle_closed_views,
            ),
        );
    }
}

/// Marker for an extra view's screen sprite. Deliberately distinct from the main
/// window's `GameScreenSprite` so the main `update_texture`/`resize_screen`
/// systems never blit the main framebuffer into (or rescale) an extra view.
#[derive(Component)]
pub struct ViewScreenSprite;

/// What an extra view shows: the walkaround world + its map editor (the default),
/// or a full-window raw text editor for the script files. Flipped per view with
/// **F2** (→ text) and **F1** (→ walkaround).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Walkaround,
    Text,
}

/// Internal resolution each extra view *starts* at (the base resolution). The
/// framebuffer then follows the window — like the main window's Mirror mode —
/// at the view's base pixel ratio ([`VIEW_SCALE`]).
const VIEW_W: u32 = egg_core::platform::WIDTH as u32;
const VIEW_H: u32 = egg_core::platform::HEIGHT as u32;

/// The view's base pixel ratio (window px per framebuffer px): the spawn
/// resolution is the base resolution at this ratio, so a new view looks exactly
/// like the classic fixed-resolution one until resized, and resizing keeps each
/// game pixel covering this many window pixels until the resolution cap bumps it.
const VIEW_SCALE: u32 = 3;

/// The largest internal resolution an extra view renders at. However big the
/// window, the framebuffer is capped here and the viewer is *scaled up* (its
/// effective pixel ratio bumped past the base [`VIEW_SCALE`]) to keep filling the
/// window — so a maximised view renders the world at a bounded resolution rather
/// than an unbounded, and slow, one. The cap is per-axis: a window that exceeds
/// it on *either* width or height triggers the bump.
const MAX_FB_W: u32 = 640;
const MAX_FB_H: u32 = 480;

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
    pub output: egg_core::render::image::RgbaImage,
    /// This view's private layer canvases (never the main `EggState.draw_state`).
    pub draw_state: DrawState,
    /// Independent free camera, panned by the arrow keys while focused.
    pub free_cam: EggVec2,
    /// This view's own map editor (toggled with `L` while focused, in
    /// [`ViewMode::Walkaround`]).
    pub editor: MapViewer,
    /// Whether this view shows the walkaround/editor or the text editor (F1/F2).
    pub mode: ViewMode,
    /// This view's raw text editor for the script files ([`ViewMode::Text`]).
    pub text_editor: TextEditor,
    /// This view's own input. `step_state` populates it (mapped to this view's
    /// framebuffer) only while the view is focused, and `update_views` threads it
    /// straight into this view's editor step + draw `Ctx` as data (no swap through
    /// a shared console). Keeping it per-view preserves this view's own
    /// edge-detection history and stops a focused view's keys/clicks reaching the
    /// primary; a non-focused view's input stays empty (refreshed, never
    /// populated), so it draws with no cursor over it.
    pub input: EggInput,
    /// Base window px per framebuffer px — the view's framebuffer is the window
    /// size divided by this (Mirror-style), so resizing the window resizes the
    /// view. Fixed at [`VIEW_SCALE`]; [`effective_scale`] bumps it higher when a
    /// large window would exceed the [`MAX_FB_W`]×[`MAX_FB_H`] resolution cap.
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
    /// True while any extra window is capturing typed text — its map editor field
    /// is focused, or it's in text-editor mode (where every key feeds the buffer).
    /// The host then suppresses global debug hotkeys (same rule as the primary
    /// editor) so typed dialogue/script text doesn't fire them.
    pub fn any_editor_typing(&self) -> bool {
        self.views
            .iter()
            .any(|v| v.mode == ViewMode::Text || v.editor.is_typing())
    }
}

/// Spawn one extra walkaround window, with its own camera (render layer + order),
/// screen sprite, and framebuffer. The free camera starts at `start_cam` (the
/// main camera's current position). `main_draw` is the loaded main `DrawState`:
/// the sprite sheets are copied from it into the view's own draw state (a bare
/// `DrawState::default()` has an empty sheet, which the tile blitter can't draw
/// from).
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
        ..DrawState::default()
    };

    views.views.push(ViewWindow {
        window,
        camera,
        sprite,
        image,
        output: egg_core::render::image::RgbaImage::new(VIEW_W, VIEW_H),
        draw_state,
        free_cam: start_cam,
        editor: MapViewer::default(),
        mode: ViewMode::Walkaround,
        text_editor: TextEditor::default(),
        input: EggInput::new(),
        scale: VIEW_SCALE,
    });
    info!(
        "Opened extra view window (layer {layer}); {} open.",
        views.views.len()
    );
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
/// editor; `F2` switches the view to the text editor and `F1` back to the
/// walkaround. In the `Update` schedule (like [`hotkeys`](crate::hotkeys)) so
/// `just_pressed` fires exactly once per tap — the held-key panning stays in the
/// fixed step ([`update_views`]).
pub fn view_hotkeys(
    game: Res<EggGame>,
    mut views: ResMut<ViewWindows>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<(Entity, &Window)>,
) {
    // Shared routing brain ([`InputRouting`]); this hotkey only acts on the
    // focused *extra* view, so anything else (primary focused, nothing focused)
    // falls through.
    let focused = windows.iter().find(|(_, w)| w.focused).map(|(e, _)| e);
    let Focus::Extra(i) = InputRouting::compute(focused, &game, &views).focus else {
        return;
    };
    let view = &mut views.views[i];

    // Function keys flip the view mode. They're never typed, so they fire even
    // while the text editor (or a map-editor field) is capturing keys — F1 is the
    // escape hatch out of text mode.
    if keys.just_pressed(KeyCode::F2) {
        view.mode = ViewMode::Text;
    }
    if keys.just_pressed(KeyCode::F1) {
        view.mode = ViewMode::Walkaround;
    }

    // While the map editor captures text, leave the `L` toggle alone so a typed
    // literal "l" doesn't close the editor. (Only meaningful in walkaround.)
    if view.mode == ViewMode::Walkaround
        && !view.editor.is_typing()
        && keys.just_pressed(KeyCode::KeyL)
    {
        view.editor.focused = !view.editor.focused;
        view.editor.layer_index = 0;
    }
}

/// Act on "edit in text editor" requests parked by the Dialog panel — on the
/// primary map editor or any view's editor. Each routes *in place*: the primary
/// panel's link switches the **primary** window to text mode; a view panel's link
/// switches **that view**. Both open the requested file at the requested anchor
/// (e.g. a dialogue key's `#dialogue` block). The text editor is the canonical
/// route for editing dialogue, so the panel's link routes here.
pub fn poll_text_open(mut views: ResMut<ViewWindows>, mut game: ResMut<EggGame>) {
    if !game.loaded {
        return;
    }
    // Primary panel's link → the primary window's text editor.
    if let Some(TextOpenReq { path, anchor }) =
        game.state.walkaround.map_viewer.pending_text_open.take()
    {
        game.text_mode = true;
        let g = &mut *game;
        g.text_editor.open(&mut g.system, &path, anchor);
    }
    // Each view panel's link → that view's own text editor (index-iterated so the
    // `views` borrow doesn't overlap the `game` borrow for `open`).
    for i in 0..views.views.len() {
        if let Some(TextOpenReq { path, anchor }) = views.views[i].editor.pending_text_open.take() {
            views.views[i].mode = ViewMode::Text;
            let g = &mut *game;
            views.views[i]
                .text_editor
                .open(&mut g.system, &path, anchor);
        }
    }

    // The map editor's path recorder writes `main.eggscene` itself, then asks the
    // host to live-reload it (the editor never gets `&mut EggState`). Same shape as
    // the text editor's `pending_scene` drain.
    if let Some(src) = game.state.walkaround.map_viewer.pending_scene.take() {
        match egg_core::data::scene::parse(&src) {
            Ok(file) => game.state.set_scenes(file),
            Err(e) => warn!("path recorder: invalid eggscene on save: {e}"),
        }
    }
    for i in 0..views.views.len() {
        if let Some(src) = views.views[i].editor.pending_scene.take() {
            match egg_core::data::scene::parse(&src) {
                Ok(file) => game.state.set_scenes(file),
                Err(e) => warn!("path recorder: invalid eggscene on save: {e}"),
            }
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

    // Which window is focused? `Window.focused` is the live OS focus state. Read
    // it through the shared routing brain ([`InputRouting`]) so this matches what
    // `step_state` decides for the player from the same moment.
    let focused = windows.iter().find(|(_, w)| w.focused).map(|(e, _)| e);
    let focus = InputRouting::compute(focused, &game, &views).focus;

    // Route the held arrow keys to the focused extra view's free camera (if
    // any). The player is handled in `step_state`, which reads the same `Focus`
    // to decide whether to drive the controller; the view's edge-triggered
    // hotkey (`L`) lives in `view_hotkeys` (Update schedule).
    if let Focus::Extra(i) = focus {
        // This view owns input this frame. `step_state` populated this view's own
        // `EggInput` (mapped to its framebuffer) and left the primary's empty, so
        // the editors below read the view's input directly — threaded into their
        // step as data, no swap through a shared console.

        // Text-editor mode: every key feeds the buffer. Step the editor at this
        // view's framebuffer size (so its click regions match `draw`), then drain
        // its live-reload requests — eggtext → base script, eggscene → cutscenes.
        if views.views[i].mode == ViewMode::Text {
            let (fb_w, fb_h) = (
                views.views[i].output.width() as i32,
                views.views[i].output.height() as i32,
            );
            let g = &mut *game;
            let view = &mut views.views[i];
            view.text_editor
                .step(&mut g.system, &view.input, &g.state.font, fb_w, fb_h);
            if let Some(source) = view.text_editor.pending_script.take() {
                match egg_core::data::script::eggtext::parse(&source) {
                    Ok(file) => g.state.script.set_base(file),
                    Err(e) => warn!("text editor: invalid eggtext on save: {e}"),
                }
            }
            if let Some(source) = view.text_editor.pending_scene.take() {
                match egg_core::data::scene::parse(&source) {
                    Ok(file) => g.state.set_scenes(file),
                    Err(e) => warn!("text editor: invalid eggscene on save: {e}"),
                }
            }
        }

        let fast = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        // While the editor captures text (or this is text mode), leave panning
        // alone so typed keys don't pan the camera.
        if views.views[i].mode == ViewMode::Walkaround && !views.views[i].editor.is_typing() {
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

        // This view's cursor was already mapped into its own input in
        // `step_state`. Step this view's editor against its own map + free camera —
        // at this view's framebuffer size, so the panel layout and hit-testing
        // match what `draw_at` renders below — reading the view's own input.
        if views.views[i].mode == ViewMode::Walkaround && views.views[i].editor.focused {
            let g = &mut *game;
            let view = &mut views.views[i];
            // Refresh the engine-owned snapshots this view's editor panels list
            // (the primary editor gets these pushed in the walkaround step).
            view.editor.preset_defs = g.state.presets.named_defs();
            let cam = view.free_cam;
            let screen = (view.output.width() as f32, view.output.height() as f32);
            let sheet = (
                view.draw_state.indexed_sprites.width() as usize / 8,
                view.draw_state.indexed_sprites.height() as usize / 8,
            );
            view.editor.step_map_viewer_at(
                &mut g.system,
                &view.input,
                &mut g.state.walkaround.current_map,
                &mut g.state.maps,
                cam,
                screen,
                sheet,
                &g.state.script,
                &g.state.save,
            );
            // Open a map the view's browser requested, into the shared map (so
            // every window sees it). Uses this view's framebuffer sprite sheet.
            if let Some((name, focus)) = view.editor.pending_open.take() {
                {
                    let mut ctx = egg_core::Ctx {
                        draw: &mut view.draw_state,
                        system: &mut g.system,
                        input: &view.input,
                        maps: &mut g.state.maps,
                        rng: &mut g.state.rng,
                        script: &g.state.script,
                        scenes: &g.state.scenes,
                        save: &mut g.state.save,
                        items: &g.state.items,
                        presets: &g.state.presets,
                        font: &g.state.font,
                    };
                    g.state.walkaround.load_map_by_name(&mut ctx, &name);
                }
                // A warp "open" carries its landing point: centre THIS view's own
                // free camera on it (the map load is shared, but each view has its
                // own camera, so the window that asked is the one that moves).
                if let Some(p) = focus {
                    let (vw, vh) = (view.output.width() as i32, view.output.height() as i32);
                    view.free_cam = EggVec2::new(
                        (i32::from(p.x) + 4 - vw / 2) as i16,
                        (i32::from(p.y) + 8 - vh / 2) as i16,
                    );
                }
            }
            // The editor never gets `&mut` engine state, so its un-take / re-take
            // test toggle parks the object's `<map>#<id>` key; flip it in the
            // shared save here (the toggle affects every window at once).
            if let Some(key) = view.editor.pending_taken_toggle.take() {
                g.state.save.toggle_taken(&key);
            }
            // A walk-sprite save from this view rewrote `data.toml`: re-install
            // the shared live registries (mirrors the primary drain in
            // `EggState::run`).
            if view.editor.pending_data_reload {
                view.editor.pending_data_reload = false;
                g.state.reload_data(&mut g.system);
            }
            // A layer or Setup edit from this view: re-derive the shared map's
            // layer lists and scalar metadata (bg colour, camera framing) using
            // this view's sprite sheet, preserving objects/camera/player. The live
            // background colour is pushed too so a swatch click shows immediately.
            if view.editor.pending_reload {
                view.editor.pending_reload = false;
                let name = g.state.walkaround.current_map.source.clone();
                let fresh = egg_core::world::map::map_by_name(
                    &view.draw_state.indexed_sprites,
                    &name,
                    &g.state.maps,
                );
                if let Some(fresh) = fresh {
                    g.state.walkaround.bg_colour = fresh.bg_colour;
                    g.state.walkaround.current_map.bg_colour = fresh.bg_colour;
                    g.state.walkaround.current_map.camera_bounds = fresh.camera_bounds;
                    g.state.walkaround.current_map.layers = fresh.layers;
                    g.state.walkaround.current_map.fg_layers = fresh.fg_layers;
                    // Sprite-plane layers + their derived components re-derive too,
                    // so a plane-cycle or spr-layer paint from this window doesn't
                    // strand the layer in a stale list (it would look deleted).
                    g.state.walkaround.current_map.sprite_layers = fresh.sprite_layers;
                    g.state.walkaround.current_map.sprite_components = fresh.sprite_components;
                }
            }
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
            view.output = egg_core::render::image::RgbaImage::new(target.0, target.1);
            if let Some(image) = images.get_mut(&view.image) {
                *image = new_screen_image(target.0, target.1);
            }
        }
    }

    // An extra view's editor mutates the shared map but never runs through
    // `WalkaroundState::step` (which only syncs the cached object animations
    // while the *primary* editor is focused). Refresh them here too, so a frame
    // edit made in an extra "map preview" window updates its in-world sprite
    // live. Cheap and idempotent — gated to when an extra editor is actually open.
    if views.views.iter().any(|v| v.editor.focused) {
        game.state.walkaround.sync_map_animations();
    }

    // Render + present every extra view from its own free camera.
    let g = &mut *game;
    for view in views.views.iter_mut() {
        // Draw this view into its own DrawState BG layer — never the main
        // framebuffer — then composite that to its output. The world (+ editor)
        // from the free camera, or the text editor, per the view's mode. The draw
        // reads this view's OWN input — the editor's hover preview / tile cursor in
        // `draw_at` reads the mouse. A non-focused view's input is empty (refreshed
        // but never populated this frame), so no cursor draws over it.
        match view.mode {
            ViewMode::Walkaround => {
                let mut ctx = egg_core::Ctx {
                    draw: &mut view.draw_state,
                    system: &mut g.system,
                    input: &view.input,
                    maps: &mut g.state.maps,
                    rng: &mut g.state.rng,
                    script: &g.state.script,
                    scenes: &g.state.scenes,
                    save: &mut g.state.save,
                    items: &g.state.items,
                    presets: &g.state.presets,
                    font: &g.state.font,
                };
                g.state.walkaround.draw_world(&mut ctx, view.free_cam, &g.state.debug_info);
                // This view's own editor overlay, after the world (draw_world
                // itself is editor-free — the crate-extraction seam).
                view.editor.draw_at(
                    &mut view.draw_state,
                    &view.input,
                    &g.state.font,
                    &g.state.walkaround.current_map,
                    &g.state.maps,
                    view.free_cam,
                );
            }
            ViewMode::Text => view.text_editor.draw(&mut view.draw_state, &g.state.font),
        }
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

/// A view's *effective* pixel ratio: the base `scale` ([`VIEW_SCALE`]), bumped
/// up by whatever further integer factor is needed to keep the framebuffer's
/// internal resolution within [`MAX_FB_W`]×[`MAX_FB_H`]. A window larger than the
/// cap therefore renders at a capped resolution scaled up to fill it, instead of
/// at an unbounded framebuffer. Both the framebuffer sizing ([`view_fb_size`])
/// and the screen-sprite scaling ([`resize_views`]) read this, so they stay in
/// lock-step. Pure + unit-tested.
pub fn effective_scale(window_w: f32, window_h: f32, scale: u32) -> u32 {
    // `div_ceil` gives the smallest ratio whose floored division fits the cap.
    let need_w = (window_w as u32).div_ceil(MAX_FB_W);
    let need_h = (window_h as u32).div_ceil(MAX_FB_H);
    scale.max(1).max(need_w).max(need_h)
}

/// A view's framebuffer size for its window size and pixel ratio: the window ÷
/// the [`effective_scale`] ratio (which already enforces the [`MAX_FB_W`]×
/// [`MAX_FB_H`] cap), floored at the same minimum as the main Mirror mode so a
/// tiny window or large ratio can't produce a degenerate framebuffer. Pure so
/// the sizing rule is unit-testable.
pub fn view_fb_size(window_w: f32, window_h: f32, scale: u32) -> (u32, u32) {
    let n = effective_scale(window_w, window_h, scale);
    (
        (window_w as u32 / n).max(MIN_FB_W),
        (window_h as u32 / n).max(MIN_FB_H),
    )
}

/// Scale each extra view's screen sprite by exactly its *effective* pixel ratio
/// (crisp N×N pixels, like the main window's Mirror mode) — the framebuffer
/// itself follows the window in `update_views` at the same [`effective_scale`],
/// so the scaled sprite always fills the window (bar a sub-ratio remainder
/// strip), including when a large window bumps the ratio past the base
/// [`VIEW_SCALE`] to honour the [`MAX_FB_W`]×[`MAX_FB_H`] cap. Also pins the OS
/// scale factor to 1 so window units equal device pixels, matching the primary
/// window.
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
        let scale = effective_scale(window.width(), window.height(), view.scale) as f32;
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
    fn view_capped_at_max_resolution() {
        // At or below the cap, the user's ratio is used unchanged.
        assert_eq!(effective_scale(640.0, 480.0, 1), 1);
        assert_eq!(view_fb_size(640.0, 480.0, 1), (640, 480));
        // One pixel over on *either* axis bumps the effective ratio up.
        assert_eq!(effective_scale(641.0, 480.0, 1), 2);
        assert_eq!(effective_scale(640.0, 481.0, 1), 2);
        // A big window at ratio 1 is scaled up until the framebuffer fits the
        // cap (1920×1080 ÷ 3 = 640×360, both within 640×480).
        assert_eq!(effective_scale(1920.0, 1080.0, 1), 3);
        assert_eq!(view_fb_size(1920.0, 1080.0, 1), (640, 360));
        // The user's chosen ratio still wins when it already exceeds the floor.
        assert_eq!(effective_scale(640.0, 480.0, 4), 4);
        assert_eq!(view_fb_size(640.0, 480.0, 4), (160, 120));
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

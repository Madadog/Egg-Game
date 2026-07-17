//! In-game editor for modern Tiled maps: toggle layers, paint tiles, and place
//! or drag map objects (warps and interactions). Opened with `L` in walkaround;
//! freezes the sim while focused and writes edits back to the map's `.tmj`.
//!
//! Warps and interactions live in one [`MapInfo::objects`] list. The two object
//! tools (Interacts / Warps) are *filtered views* over that single list — each
//! tab lists only objects of its kind, mapping its display rows to real vector
//! indices — so the UX is unchanged while the data model is unified.
//!
//! This module holds the [`MapViewer`] state and the shared types, constants and
//! free helpers; the one large `impl MapViewer` is carved by concern into sibling
//! submodules (`step`, `panels`, `scroll`, `tools`, `objects`, `layers`,
//! `history`, `modal`, `draw`), each `use super::*` over what lives here, with
//! its methods `pub(super)`.

use std::collections::BTreeSet;

use egg_world::data::script::message::Message;
use egg_world::data::{
    eggdata,
    save::SaveData,
    script::Script,
    sound::{self, SfxData},
    tiled::{TiledMap, TiledMapLayer},
};
use egg_world::draw_state::{
    BgColour, DrawParams, DrawState, LayerId, PALETTE_MAP_IDENTITY, palette_map_rotate,
};
use egg_render::geometry::{Hitbox, Vec2};
use egg_world::data::scene::{
    self, Chain, CutsceneContent, CutsceneDef, Instruction, Motion, ScrubRequest,
};
use egg_platform::{
    ConsoleApi, EggInput, MouseInput, ScanCode, dpad_delta, just_pressed, pressed,
};
use egg_render::image::{Rgba, RgbaImage};
use egg_render::{
    Canvas, EdgePolicy, Flip, Font, MapOptions, PrintOptions, Rotate, SpriteOptions, Transform,
    print_to_shadow_with_font, print_to_with_font,
};
use egg_ui::dialogue::Dialogue;
use egg_ui::layout::{NodeId, Rect, Ui, UiBuilder};
use egg_world::world::animation::AnimFrame;
use egg_world::world::interact::{InteractFn, Interaction};
use egg_world::world::map::{
    Axis, LayerInfo, LayerKind, MapInfo, MapObject, MapStore, ObjectEffect, Plane, Trigger, Warp,
    WarpMode, map_by_name,
};
use egg_world::world::player::Shell;

// `pub(crate)` so the text editor can reuse the shared dock primitives (`Side`
// and the resize-size constants) for its own outline dock — see
// `super::text`. The multi-panel `DockManager` itself stays map-specific.
pub(crate) mod dock;
use dock::{DockLayout, DockManager, DragState, PanelKind, Placement, Side};
use walk_editor::WalkEditor;

use super::text::{TextAnchor, TextOpenReq};
use egg_ui::text_field::{TextEvent, TextField, TextOp};

/// Where the editor persists its dock arrangement (native only; on web the
/// asset writes are silent no-ops, so the layout is session-only there).
const LAYOUT_PATH: &str = "config/layout.json";

/// The canonical (English base) script the Dialog editor reads back and writes
/// a single spliced `#dialogue` block to. v1 edits this base file only.
const SCRIPT_PATH: &str = "script/en.eggtext";

/// The active editing tool. The map editor is the old layer viewer grown into a
/// tabbed tool: toggle layers, paint tiles, or place map objects. The Interacts
/// and Warps tabs are filtered views over the one objects list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorTool {
    #[default]
    Layers,
    Paint,
    /// Marquee-select a rectangle of tiles on the active layer, then copy / cut /
    /// paste / delete it. A sub-mode of the Paint panel (shares its dock slot).
    Select,
    Interactables,
    Warps,
}

/// A field the editor focuses for keyboard text/number entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditField {
    Key,
    /// A cutscene interaction's registry name (see [`egg_world::data::scene`]).
    Scene,
    ToMap,
    ToX,
    ToY,
    /// A warp's pre-warp narration dialogue key (empty buffer ⇒ no narration).
    Narration,
    /// The object's flag [`Gate`](egg_world::world::map::Gate) fields — a story-flag
    /// name each (an empty buffer clears that condition to `None`). Common to
    /// every object kind. `CondIf` = fires only while set; `CondUnless` = fires
    /// only while clear; `Sets` = the flag set when the object fires (the one-shot
    /// latch).
    CondIf,
    CondUnless,
    Sets,
    /// The map's literal background colour as a `#rrggbb` hex triple (the
    /// Setup panel's `rgb:` field). A parseable commit writes the RGB form of
    /// `bg_colour`; the palette swatches write the indexed form back.
    BgRgb,
    /// The `note` Func interaction's pitch (an `i32`).
    Pitch,
    /// The `add_creatures` Func interaction's spawn count (a `usize`).
    Count,
    /// The `give_item` Func interaction's item key (the granted item's registry
    /// key, e.g. `"chegg"`; a free-text string, empty until typed).
    Item,
    /// The selected object's trigger-hitbox geometry (`i16` px) — the numeric
    /// counterpart to dragging the box. Common to every object kind.
    HitX,
    HitY,
    HitW,
    HitH,
    /// The selected animation frame's editable fields — the object's
    /// [`sprite`](egg_world::world::map::MapObject::sprite) frame indexed by
    /// [`MapViewer::sprite_frame`]. Tile id / duration, the draw offset from the
    /// hitbox (`pos`), the multi-tile span and pixel scale, the palette rotation,
    /// and the transparent / outline palette indices (an empty buffer clears the
    /// latter two to `None`).
    FrameTile,
    FrameDuration,
    FrameOffX,
    FrameOffY,
    FrameW,
    FrameH,
    FrameScale,
    FramePaletteRot,
    FrameTransparent,
    FrameOutline,
    /// The selected layer's name (lives on the store's `TiledMap`, not an object;
    /// the layer is captured in [`TextEdit::target`]).
    LayerName,
    /// The selected tile layer's pixel offset / palette rotation (also stored on
    /// the `TiledMap`, targeted via [`TextEdit::target`]).
    LayerOffX,
    LayerOffY,
    LayerRotate,
}

/// A numeric, undoable property of a tile layer (the unit of [`EditAction::LayerSetProp`]).
/// All three are carried as `f64` for one uniform revert/reapply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayerProp {
    OffsetX,
    OffsetY,
    Rotate,
}

/// An enum-field the editor advances with a click. [`Flip`](Self::Flip)/
/// [`Mode`](Self::Mode)/[`Sound`](Self::Sound) live on the [`Warp`] effect;
/// [`Trigger`](Self::Trigger) lives on the owning [`MapObject`] and so shows on
/// both object tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CycleField {
    Flip,
    Mode,
    Sound,
    Trigger,
    /// Whether the selected interaction object is a consume-on-interact pickup
    /// ([`MapObject::removable`](egg_world::world::map::MapObject::removable)) — toggled
    /// no/yes. Interacts tab only (warps are never "taken").
    Removable,
    /// The selected sprite frame's mirror ([`Flip`]) and 90° rotation
    /// ([`Rotate`]), cycled in place.
    FrameFlip,
    FrameRotate,
    /// The interaction kind (none / dialogue / the named Func behaviours) — only
    /// on the Interacts tab. Cycling rebuilds the effect, keeping a usable param.
    IntKind,
    /// A warp's destination map: steps through `[same-map] + existing maps`, so a
    /// target can be picked without typing (and can't be a typo'd dangling name).
    WarpTarget,
}

/// Which kind of object a tool creates / filters its view to. The object lists
/// are unified now, so this no longer routes between collections — it only
/// distinguishes the two object tabs (Interacts vs. Warps).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjKind {
    Interactable,
    Warp,
}
impl ObjKind {
    /// Whether `object` belongs in this kind's filtered tab view.
    fn matches(self, object: &MapObject) -> bool {
        match object.effect {
            ObjectEffect::Warp(_) => self == ObjKind::Warp,
            ObjectEffect::Interact(_) => self == ObjKind::Interactable,
        }
    }
}

/// A whole-object snapshot used by the object undo entries. Cloning a
/// [`MapObject`] is cheap (a hitbox + a small effect) and keeps undo trivially
/// correct without per-field diffing.
type ObjSnapshot = MapObject;

/// One reversible edit, the unit of the undo/redo stacks. Tile paints batch a
/// whole press-drag-release stroke into a single entry (so one Ctrl+Z undoes a
/// brush stroke, not one pixel of it); object edits snapshot the affected object
/// before and/or after so they replay exactly. Object `index` is the real index
/// into the single [`MapInfo::objects`] list (not a per-tab display row).
#[derive(Debug, Clone)]
enum EditAction {
    /// Tiles changed by one paint stroke: `(x, y, old, new)` per cell, in the
    /// `(source, layer)` the stroke painted into.
    Tiles {
        source: String,
        layer: usize,
        cells: Vec<(i32, i32, usize, usize)>,
    },
    /// An object was appended at `index` (always the end of the objects list).
    Add { index: usize, after: ObjSnapshot },
    /// An object was removed from `index`; `before` is the object as it was.
    Remove { index: usize, before: ObjSnapshot },
    /// An object was mutated in place (moved, retyped, or a field edited).
    Modify {
        index: usize,
        before: ObjSnapshot,
        after: ObjSnapshot,
    },
    /// A layer was inserted at `index` in `source`'s layer list (add / duplicate
    /// layer). Undo removes it; redo re-inserts the stored layer. Boxed so the
    /// common (object/tile) actions don't pay for the layer payload's size.
    LayerInsert {
        source: String,
        index: usize,
        layer: Box<TiledMapLayer>,
    },
    /// A layer was removed from `index` in `source`. Undo re-inserts the stored
    /// layer; redo removes it again.
    LayerRemove {
        source: String,
        index: usize,
        layer: Box<TiledMapLayer>,
    },
    /// Two layers in `source` were swapped (move up / down) — self-inverse.
    LayerSwap { source: String, a: usize, b: usize },
    /// A layer in `source` was drag-reordered from index `from` to index `to`
    /// (remove + re-insert). Undo moves it back (`to` → `from`); redo replays it.
    LayerMove { source: String, from: usize, to: usize },
    /// A layer at `index` in `source` was renamed.
    LayerRename {
        source: String,
        index: usize,
        before: String,
        after: String,
    },
    /// A tile layer at `index` in `source` had its draw [`Plane`] cycled (the
    /// three-way BG → Sprite → FG toggle, writing the `plane` property).
    LayerPlane {
        source: String,
        index: usize,
        before: Plane,
        after: Plane,
    },
    /// A tile layer's numeric property (offset / palette rotation) changed.
    LayerSetProp {
        source: String,
        index: usize,
        prop: LayerProp,
        before: f64,
        after: f64,
    },
    /// A cutscene block in `main.eggscene` was created / replaced / renamed by the
    /// path recorder. The registry lives with the host, not the editor, and the
    /// smallest thing the editor owns is the file's full source, so undo/redo swap
    /// the whole `.eggscene` text (`before`/`after`) and re-install it (write +
    /// live-reload) — the same seam the forward save uses.
    SceneEdit { before: String, after: String },
}

/// Cap on each undo/redo stack. Tile strokes can be large, so this is a count of
/// *actions*, not cells — generous enough for a long editing session while still
/// bounding memory.
const HISTORY_LIMIT: usize = 128;

/// A bounded, linear undo/redo history of actions `A`.
///
/// This is the pure stack discipline behind the editor's Ctrl+Z/Ctrl+Y, factored
/// out of [`MapViewer`] so it can be reasoned about and tested in isolation. It
/// knows nothing about *what* an action does — applying an [`EditAction`] to a
/// [`MapInfo`] stays on `MapViewer`, which owns the `&mut MapInfo`. The history
/// only shuffles finished actions between two stacks:
///
/// - [`push`](Self::push) records a freshly performed action. It **clears the
///   redo stack**: a new edit invalidates any redone future (the standard linear
///   model — you can't fork history). Once the undo stack exceeds
///   [`HISTORY_LIMIT`] it drops the oldest entry, bounding memory.
/// - [`undo`](Self::undo) moves the newest undo entry onto the redo stack and
///   hands it back by reference for the caller to revert.
/// - [`redo`](Self::redo) is the inverse: it moves the newest redo entry back
///   onto the undo stack and hands it back for the caller to re-apply.
///
/// Entries are kept (cloned onto the other stack) rather than handed out by
/// value so an undone action can be redone, and a redone action undone again,
/// any number of times.
#[derive(Debug, Clone)]
struct History<A> {
    undo: Vec<A>,
    redo: Vec<A>,
    /// Maximum entries on each stack; the oldest undo entry is dropped past it.
    limit: usize,
}

impl<A: Clone> History<A> {
    /// An empty history bounded at `limit` actions per stack.
    fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit,
        }
    }

    /// Record a freshly performed action, invalidating any redo future and
    /// evicting the oldest undo entry if the stack is now over its cap.
    fn push(&mut self, action: A) {
        self.redo.clear();
        self.undo.push(action);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    /// Take the most recent action to be undone, moving it onto the redo stack
    /// and returning a reference for the caller to revert. `None` if nothing is
    /// left to undo.
    fn undo(&mut self) -> Option<&A> {
        let action = self.undo.pop()?;
        self.redo.push(action);
        self.redo.last()
    }

    /// Take the most recently undone action to be redone, moving it back onto the
    /// undo stack and returning a reference for the caller to re-apply. `None` if
    /// nothing is left to redo.
    fn redo(&mut self) -> Option<&A> {
        let action = self.redo.pop()?;
        self.undo.push(action);
        self.undo.last()
    }

    /// Drop both stacks (e.g. on loading a different map).
    fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Whether there is an action available to [`undo`](Self::undo) — drives the
    /// greyed-out state of the panel's `<undo` button.
    fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is an action available to [`redo`](Self::redo) — drives the
    /// greyed-out state of the panel's `redo>` button.
    fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

impl<A: Clone> Default for History<A> {
    /// A history with the editor's default [`HISTORY_LIMIT`].
    fn default() -> Self {
        Self::new(HISTORY_LIMIT)
    }
}

/// Hit-test keys for the editor's left-hand panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EditorKey {
    Tool(EditorTool),
    /// A Layers-panel plane filter toggle (bg / sprite / fg): hides that plane's
    /// rows from the (map-ordered) list without moving the selection.
    LayerFilter(Plane),
    /// Select the layer at this **store** index (`source_layer`); the row is keyed
    /// by the absolute layer index, not a per-plane display row.
    Layer(usize),
    /// A layer row's visibility (eye) toggle, keyed by store index.
    LayerVis(usize),
    /// Layers panel toolbar: add tile layer / duplicate / delete / move up /
    /// move down / rename / cycle draw plane (BG → Sprite → FG).
    LayerAdd,
    LayerDup,
    LayerDel,
    LayerUp,
    LayerDown,
    LayerRename,
    LayerPlane,
    /// The scrollable tile palette viewport (drag to pan, click to pick a tile).
    PaletteView,
    Object(usize),
    NewObject,
    DupObject,
    DeleteObject,
    /// Select animation frame `n` of the selected object's sprite for editing.
    SpriteFrame(usize),
    /// Append / remove a frame, move the selected frame earlier / later in the
    /// animation, or set the selected frame's tile from the palette brush — the
    /// object's animated-sprite controls. (Frames also drag-reorder; the buttons
    /// are the click/keyboard counterpart, mirroring the Layers panel.)
    SpriteAddFrame,
    SpriteDelFrame,
    SpriteFrameUp,
    SpriteFrameDown,
    SpriteFromBrush,
    /// The live animated preview box of the selected object's sprite.
    SpritePreview,
    /// The selected warp's destination-map preview: a rendered, click-to-place
    /// map of the warp target with the player shown at the landing point.
    WarpPreview,
    /// Open the selected warp's destination map in the editor, centred on the
    /// landing point. A no-op for a same-map warp (no destination to load).
    OpenWarpDest,
    /// Open the fullscreen warp-destination placement overlay (the "place" button).
    WarpPreviewOpen,
    /// A Presets-panel row: open the walk-sprite editor on that preset.
    PresetRow(usize),
    /// A Scenes-panel row: replay saved cutscene `n` (index into
    /// [`scene_defs`](MapViewer::scene_defs)) in the scrubber — the panel
    /// counterpart to the `P` scene picker's Enter.
    SceneRow(usize),
    /// The Scenes panel's "record new path" action: open the live path recorder
    /// (the panel counterpart to the `R` shortcut).
    RecordPath,
    /// The Scenes panel's show/hide-paths toggle: flip
    /// [`show_paths`](MapViewer::show_paths), the overlay that draws every saved
    /// cutscene's movement paths over the world.
    TogglePaths,
    /// The walk-sprite editor's Save / Cancel buttons (hit only in that modal).
    WalkEdOk,
    WalkEdCancel,
    /// Confirm the fullscreen placement — commit the working landing point.
    WarpPreviewOk,
    /// Cancel the fullscreen placement — leave the warp untouched.
    WarpPreviewCancel,
    /// Save the live path recording as a cutscene.
    PathRecOk,
    /// Discard the live path recording.
    PathRecCancel,
    /// Focus the recorder's scene-name field to rename the recording.
    PathRecName,
    /// Cycle the recorded actor (player / companion / a live map creature).
    PathRecActor,
    /// The recorder's map canvas — a click here drops a `MoveToPoint` waypoint.
    PathRecCanvas,
    Field(EditField),
    Cycle(CycleField),
    /// Selects the empty tile (0) as the brush — i.e. an eraser.
    Eraser,
    /// Select-tool ops on the current marquee / clipboard.
    SelCopy,
    SelCut,
    SelPaste,
    SelDelete,
    SelClear,
    Undo,
    Redo,
    Save,
    /// A panel's title bar (press to focus / begin a move), carrying the panel's
    /// index. Close is a global-bar [`TogglePanel`](Self::TogglePanel); resize
    /// handles are picked geometrically in `step_drag` — neither routes here.
    Dock(usize),
    /// A global-bar button that shows/hides a panel.
    TogglePanel(PanelKind),
    /// A map cell in the Maps browser grid (index into the modern-map list).
    MapSlot(usize),
    /// Page the Maps browser grid up / down.
    MapPrev,
    MapNext,
    /// Maps-browser CRUD toolbar: new / duplicate / rename / delete.
    MapNew,
    MapDup,
    MapRename,
    MapDelete,
    /// Setup panel: a background-colour swatch (palette index).
    BgColour(u8),
    /// Setup panel: pin the camera at the current view centre / clear the pin.
    CamPin,
    CamAuto,
    /// Setup panel: open the resize-map dialog.
    MapResize,
    /// Setup panel: cycle the map's music track.
    MusicCycle,
    /// Setup panel: cycle the map's music playback speed.
    MusicSpeedCycle,
    /// A scrollable panel's scroll bar (carries the panel index): press to drag it.
    PanelScroll(usize),
    /// Dialog panel: pick dialogue key `n` (index into [`MapViewer::dialogue_keys`])
    /// — previews it and, if an object is selected, assigns it as that object's key.
    DlgPick(usize),
    /// Dialog panel: page the previewed message back / forward.
    DlgMsgPrev,
    DlgMsgNext,
    /// Dialog panel: open the current dialogue in the text editor (the canonical
    /// edit route) — parks a [`TextOpenReq`] for the host to act on.
    DlgOpenText,
    /// Interacts tab: un-take / re-take the selected pickup for testing — flips
    /// its collected state in this save (parks [`MapViewer::pending_taken_toggle`]
    /// for the host). Distinct from the [`Removable`](CycleField::Removable)
    /// authoring toggle, which marks *whether* the object is a pickup at all.
    TakenToggle,
    /// Objects panel: accept autocomplete suggestion `n` (index into the live
    /// [`autocomplete_suggestions`](MapViewer::autocomplete_suggestions) list) for
    /// the interaction key / gate-flag field being edited — commits that whole
    /// vocabulary entry and closes the field. The click counterpart to Tab, which
    /// accepts the top (highlighted) match.
    Suggest(usize),
}

/// The music playback-speed presets the Setup panel cycles through (1.0 = normal).
const MUSIC_SPEEDS: [f32; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

/// Fallback sprite-sheet dimensions in tiles (32×128, the current
/// `assets/sprites/sheet.png`), used only until the first draw caches the real
/// size from the live sheet (see [`MapViewer::sheet_cols`]). The palette shows
/// *every* tile so collision-marker sprites are reachable.
const SHEET_COLS_DEFAULT: usize = 32;
const SHEET_ROWS_DEFAULT: usize = 128;
/// Grab width (px) of a palette scroll bar at the viewport's edge.
const PALETTE_BAR_GRAB: i16 = 4;
/// The global undo/redo/save + panel-toggle toolbar's size, px. Width fits its
/// row: undo 8 + redo 8 + save 13 + 8 icon tabs × 8 = 93, plus 10 × 1px gaps and
/// 1px padding a side (2) = 105; height is the tallest child (an 8px icon) plus
/// that 2px padding = 10.
const GLOBAL_BAR_W: f32 = 105.0;
const GLOBAL_BAR_H: f32 = 10.0;
/// A Maps-browser cell's thumbnail box size, px (a name label sits below it).
const THUMB_W: f32 = 40.0;
const THUMB_H: f32 = 22.0;
/// The selected warp's destination preview box height, px. Its width fills the
/// Objects panel, so floating/widening that panel grows the click target.
const WARP_PREVIEW_H: f32 = 64.0;

/// Arrow-key pan speed (map px/frame) for the fullscreen warp-preview overlay;
/// holding Shift uses the faster one.
const WARP_CAM_PAN: i16 = 2;
const WARP_CAM_PAN_FAST: i16 = 6;

/// An open fullscreen warp-destination placement session (see
/// [`MapViewer::open_warp_preview`]). The destination map renders 1:1 offset by
/// `camera`; `point` is the working landing, committed to the warp only on
/// confirm so a cancel leaves it untouched.
#[derive(Debug, Clone)]
struct WarpPreview {
    /// Index into the current map's `objects` of the warp being placed.
    object: usize,
    /// The destination map's name (the warp's target, or the current map for a
    /// same-map warp). Resolved once on open.
    dest: String,
    /// Working landing point, destination-map pixels. Committed on confirm.
    point: Vec2,
    /// Camera top-left, destination-map pixels — the 1:1 render's pan offset.
    camera: Vec2,
    /// Placement is gated on this until the mouse releases once, so the click that
    /// opened the overlay can't instantly drop the landing under the button.
    armed: bool,
    /// `(cursor, camera)` captured at a right-button press, for grab-drag panning.
    pan_anchor: Option<(Vec2, Vec2)>,
}

/// Where to write the cutscene registry (mirrors the private `EGGSCENE_PATH` the
/// text editor uses).
const SCENE_PATH: &str = "data/main.eggscene";

/// An open scene-picker session (fully modal): a list of cutscene names; pick
/// one with ↑/↓ + Enter to replay it in the scrubber. Populated from the
/// editor's [`scene_defs`](MapViewer::scene_defs) snapshot (names only) when opened.
#[derive(Debug, Clone, Default)]
struct ScenePicker {
    /// Names to choose from (a snapshot taken at open).
    names: Vec<String>,
    /// The highlighted row.
    selected: usize,
}

/// An open live path-recorder session (fully modal): drive a puppet actor with
/// the dpad over the current map, capturing its per-frame heading as an RLE
/// [`Motion::Record`], then save it as a one-actor cutscene to `main.eggscene`.
/// The camera auto-follows the puppet, so the dpad is free to drive.
///
/// The recording is a single actor's chain built as an ordered mix of two kinds
/// of segment: **walked** runs (the dpad, RLE — buffered live in [`runs`](Self::runs)
/// then folded into a [`Motion::Record`]) and **clicked** waypoints (a map click
/// drops a [`Motion::MoveToPoint`]). Committed segments accumulate in
/// [`instructions`](Self::instructions); the trailing walk buffer is folded in at
/// commit, so walking and clicking compose in author order.
#[derive(Debug, Clone)]
struct PathRecorder {
    /// The driven puppet — `pos` is the live cursor; it uses the player's sprites.
    puppet: Shell,
    /// The *trailing* walked RLE: `(heading, frames-held)` runs since the last
    /// waypoint (or the start). Stores the *commanded* heading each frame
    /// (collision is re-applied at replay), idle `(0,0)` included, so the path
    /// round-trips through `Motion::Record`. Folded into a `Record` instruction
    /// when a waypoint interrupts it or at commit.
    runs: Vec<((i8, i8), u16)>,
    /// Committed instructions in author order: folded `Record` runs interleaved
    /// with clicked `MoveToPoint` waypoints. The live [`runs`](Self::runs) tail is
    /// appended to this at commit.
    instructions: Vec<Instruction>,
    /// The puppet's position each frame (and each waypoint), for the path polyline.
    path: Vec<Vec2>,
    /// Camera top-left (map px) — the 1:1 render's pan offset, tracking the puppet.
    camera: Vec2,
    /// Whether collision is ignored while driving (toggled with `N`).
    noclip: bool,
    /// The cutscene name the recording saves under.
    name: String,
    /// The selectable actors — `(token, start position)` — snapshotted at open
    /// from the host-pushed [`recorder_actors`](MapViewer::recorder_actors). The
    /// `token` is what a chain names (`player`, `companion N`, or a creature id);
    /// the position seeds the puppet so clicked waypoints land relative to where
    /// the actor really is. Never empty (falls back to `player` at the view centre).
    actors: Vec<(String, Vec2)>,
    /// Which of [`actors`](Self::actors) this recording drives.
    actor: usize,
    /// The active scene-name edit, when the name field is focused. `Some` swallows
    /// all keys (so typing a name can't drive the puppet or trip a hotkey); `None`
    /// is the normal driving mode.
    naming: Option<TextField>,
    /// A transient one-line hint under the banner (rename validation, "replaces
    /// existing", the picked actor). Cleared when it no longer applies.
    status: Option<String>,
}

impl PathRecorder {
    /// The token of the actor this recording drives (`player` / `companion N` / a
    /// creature id) — what the emitted chain names.
    fn actor_token(&self) -> &str {
        self.actors
            .get(self.actor)
            .map_or("player", |(token, _)| token.as_str())
    }

    /// Fold the live walked buffer into a trimmed [`Motion::Record`] instruction,
    /// leaving the buffer empty. Leading + trailing idle is trimmed per run so a
    /// reaction pause (before a click, or between segments) isn't baked in;
    /// interior idle is intentional timing and survives. An all-idle buffer folds
    /// to nothing.
    fn fold_walk_run(&mut self) {
        let mut runs = std::mem::take(&mut self.runs);
        while runs.first().is_some_and(|(d, _)| *d == (0, 0)) {
            runs.remove(0);
        }
        while runs.last().is_some_and(|(d, _)| *d == (0, 0)) {
            runs.pop();
        }
        if !runs.is_empty() {
            self.instructions.push(Instruction::new(
                Motion::Record {
                    runs,
                    noclip: self.noclip,
                },
                0,
            ));
        }
    }

    /// Append a clicked waypoint: flush the walked buffer, add a `walk`/`noclip`
    /// move-to (per the `noclip` flag), and jump the puppet there so the next
    /// segment continues from the waypoint.
    fn place_waypoint(&mut self, point: Vec2) {
        self.fold_walk_run();
        let motion = if self.noclip {
            Motion::MoveToPointNoclip(point)
        } else {
            Motion::MoveToPoint(point)
        };
        self.instructions.push(Instruction::new(motion, 0));
        self.puppet.pos = point;
        self.path.push(point);
    }

    /// Switch the recorded actor, discarding any in-progress path and re-seating
    /// the puppet on the new actor's start position (so its waypoints land
    /// relative to where it really is).
    fn select_actor(&mut self, idx: usize) {
        if self.actors.is_empty() {
            return;
        }
        self.actor = idx % self.actors.len();
        let start = self.actors[self.actor].1;
        self.puppet.pos = start;
        self.runs.clear();
        self.instructions.clear();
        self.path = vec![start];
        let token = self.actor_token().to_string();
        self.status = Some(format!("actor: {token}"));
    }

    /// Whether any path has been laid down yet (a walked frame or a waypoint) —
    /// gates the noclip toggle, which must be fixed before the first segment.
    fn has_recorded(&self) -> bool {
        !self.instructions.is_empty() || self.runs.iter().any(|(d, _)| *d != (0, 0))
    }

    /// Pan the follow-camera to keep the puppet centred, clamped to the map.
    fn follow_camera(&mut self, sw: i16, sh: i16, fw: i16, fh: i16) {
        self.camera.x = clamp_camera(self.puppet.pos.x - sw / 2, fw, sw);
        self.camera.y = clamp_camera(self.puppet.pos.y - sh / 2, fh, sh);
    }
}

#[cfg(test)]
impl PathRecorder {
    /// A player recorder for tests: a walked `runs` buffer under `name`, no
    /// waypoints, no naming, at the origin. Tests override individual fields.
    fn test(runs: Vec<((i8, i8), u16)>, name: &str) -> Self {
        Self {
            puppet: Shell::default(),
            runs,
            instructions: Vec::new(),
            path: vec![Vec2::new(0, 0)],
            camera: Vec2::new(0, 0),
            noclip: false,
            name: name.to_string(),
            actors: vec![("player".to_string(), Vec2::new(0, 0))],
            actor: 0,
            naming: None,
            status: None,
        }
    }
}
/// Side (px) of the square animated-sprite preview box in the Objects panel.
const SPRITE_PREVIEW_PX: f32 = 40.0;
/// Height (px) of a panel's pinned title bar — the `full_width(7.0)` title row.
/// A scrolled panel's body begins below this, and the title stays put.
const PANEL_TITLE_H: i16 = 7;
/// A panel scroll bar's width and its edge grab zone, px.
const SCROLLBAR_W: i16 = 3;
const SCROLLBAR_GRAB: i16 = 4;
/// Px a panel body scrolls per mouse-wheel notch.
const SCROLL_STEP: i16 = 10;
/// Default dimensions (tiles) of a newly-created blank map — roughly one screen.
const NEW_MAP_W: usize = 30;
const NEW_MAP_H: usize = 17;
/// Frames a save-confirmation toast stays on screen. At ~64 fps this is ~1.4s.
const SAVE_TOAST_FRAMES: u32 = 90;

/// What the save button should display, derived from [`SaveStatus`] once per
/// frame and consumed by the footer's button rendering — so the button's three
/// looks live in one place rather than being recomputed inline.
enum SaveButton {
    /// A transient "saved!" toast is showing (it takes priority over dirtiness).
    Toast,
    /// There are unsaved edits — the button wears a `*` and an amber outline.
    Dirty,
    /// Everything on disk is current — the plain green button.
    Clean,
}

/// The editor's save/unsaved state: an unsaved-changes flag plus a cosmetic
/// "saved!" toast countdown. Folded into one type so the two pieces of save UX
/// transition together and the save button reads a single query.
///
/// Transitions are explicit:
/// - [`edited`](Self::edited) marks the map dirty (every recorded edit calls it).
/// - [`saved`](Self::saved) clears the dirty flag and starts the toast.
/// - [`tick`](Self::tick) counts the toast down one frame, expiring it at zero.
#[derive(Debug, Clone, Default)]
struct SaveStatus {
    /// Set on any edit, cleared on save — drives the unsaved-changes marker.
    dirty: bool,
    /// Frames left on the post-save "saved!" toast (purely cosmetic).
    toast: u32,
}

impl SaveStatus {
    /// Flag the map as having unsaved edits.
    fn edited(&mut self) {
        self.dirty = true;
    }

    /// Record a successful write: no longer dirty, and start the confirm toast.
    fn saved(&mut self) {
        self.dirty = false;
        self.toast = SAVE_TOAST_FRAMES;
    }

    /// Advance the toast one frame, expiring it at zero. No-op once expired.
    fn tick(&mut self) {
        self.toast = self.toast.saturating_sub(1);
    }

    /// Which of the save button's three looks to draw this frame. The toast wins
    /// over dirtiness so a fresh save reads as confirmed even if (re)edited the
    /// same frame would otherwise re-dirty it.
    fn button(&self) -> SaveButton {
        if self.toast > 0 {
            SaveButton::Toast
        } else if self.dirty {
            SaveButton::Dirty
        } else {
            SaveButton::Clean
        }
    }
}

/// An open map-management dialog over the Maps browser. Keyboard-driven: typed
/// keys plus Return commit, Escape cancels (reusing [`TextField`]). Rendered as
/// a small centred modal so it works regardless of the Maps panel's size.
#[derive(Debug, Clone, Default)]
enum MapsDialog {
    #[default]
    None,
    /// Naming a new blank map and its size: `name` / `w` / `h` fields, `focus`
    /// the one being typed (0=name, 1=w, 2=h). Enter advances, then commits.
    New {
        name: TextField,
        w: TextField,
        h: TextField,
        focus: u8,
    },
    /// Renaming `from` to the typed new name.
    Rename { from: String, name: TextField },
    /// Confirming deletion of `name` (Return = delete, Escape = keep).
    ConfirmDelete(String),
    /// Resizing `source` to typed `w`×`h`; `focus` is the field being typed
    /// (0=w, 1=h). Enter advances, then commits.
    Resize {
        source: String,
        w: TextField,
        h: TextField,
        focus: u8,
    },
}

impl MapsDialog {
    fn is_active(&self) -> bool {
        !matches!(self, MapsDialog::None)
    }
    /// Whether a text field is capturing input (so the host suppresses its global
    /// hotkeys while the user types a map name).
    fn is_typing(&self) -> bool {
        matches!(
            self,
            MapsDialog::New { .. } | MapsDialog::Rename { .. } | MapsDialog::Resize { .. }
        )
    }
}

/// An in-progress palette drag. `Select` drags out the brush box from its
/// `anchor` tile (a 1×1 box if you just click); `ScrollV`/`ScrollH` drag a
/// scroll bar. (Navigation is the scroll bars + wheel, so a body drag is free to
/// mean box-select.)
#[derive(Debug, Clone, Copy)]
enum PalDrag {
    Select {
        anchor_col: usize,
        anchor_row: usize,
    },
    /// A scroll-bar drag. `grab` is the px offset between the cursor and the
    /// thumb's near edge at press, preserved so the thumb tracks the cursor.
    ScrollV {
        grab: i16,
    },
    ScrollH {
        grab: i16,
    },
}

/// A reorderable list in the editor's panels, identifying which one a
/// [`CanvasDrag::Reorder`] is rearranging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReorderList {
    /// The Layers panel's layer rows (the active bg / fg list per [`MapViewer::fg`]).
    Layers,
    /// The Objects panel's sprite animation frames for the selected object.
    SpriteFrames,
}

/// A manual press-drag the editor's hit-testing owns across frames (distinct from
/// the dock's own [`DragState`]). The palette and a panel scroll bar are never
/// grabbed at once — only one keyed surface is hit per frame — so they share one
/// slot instead of two `Option`s that must stay mutually exclusive by convention.
#[derive(Debug, Clone, Copy, Default)]
enum CanvasDrag {
    #[default]
    None,
    /// A palette drag: a brush box-select or one of the two palette scroll bars.
    Palette(PalDrag),
    /// A panel scroll-bar drag: the panel index plus the grab offset within the
    /// thumb, so the thumb tracks the cursor rather than snapping under it.
    Scrollbar { idx: usize, grab: i16 },
    /// A list row lifted for drag-reordering. `from` is the grabbed item's index
    /// in `list`; `at` is the row the cursor currently hovers (where a release
    /// drops it), re-read each frame. They stay equal until the cursor moves to
    /// another row, so a press-and-release in place is an ordinary click
    /// (select), not a reorder.
    Reorder {
        list: ReorderList,
        from: usize,
        at: usize,
    },
}

/// The Select tool's marquee: a rectangle of tiles on the active layer, in tile
/// coordinates (`x`/`y` may sit off the layer's left/top before a copy clamps
/// them). Copy / Cut / Delete operate on the cells it covers.
#[derive(Debug, Clone, Copy)]
struct TileSelection {
    x: i32,
    y: i32,
    w: usize,
    h: usize,
}

/// Tiles lifted by Copy / Cut, ready for Paste — row-major `w`×`h` tile ids
/// (`0` = empty). Held on the viewer across map switches, so a block can be
/// copied from one map and pasted into another.
#[derive(Debug, Clone)]
struct Clipboard {
    w: usize,
    h: usize,
    tiles: Vec<usize>,
}

/// The outcome of stepping a [`MapsDialog`], applied by the caller after the
/// dialog's borrow ends (so the CRUD op can take `&mut self`).
enum DialogAction {
    Keep,
    Close,
    Create(String, usize, usize),
    Rename(String, String),
    Delete(String),
    /// Resize the named map to `(w, h)` tiles.
    Resize(String, usize, usize),
}

/// An in-progress drag of an existing map object. One session: set at grab,
/// cleared at drop — so the half-set state the two former `Option`s allowed
/// (and the box-drag branch's clear-one-but-not-the-other) can't happen.
#[derive(Clone, Copy, Debug)]
struct ObjectDrag {
    /// Cursor − hitbox origin at grab time, so the object tracks the cursor.
    grab_offset: Vec2,
    /// The object's origin at grab time, so a completed drag records one
    /// [`EditAction::Modify`] (start → drop), not one per moved frame.
    from: Vec2,
}

/// An open text-entry session: which field has focus, its live line buffer, and
/// — for the store-backed layer edits ([`EditField::LayerName`] / offsets /
/// rotate) — the captured *source* layer index the commit targets (those edits
/// live on the store's `TiledMap`, not on an object). `target` is `0` and unused
/// for object-field edits. The three were once separate `Option`s kept in lockstep
/// by hand; bundling them makes a half-open session or a stale target unrepresentable.
#[derive(Clone, Debug)]
struct TextEdit {
    field: EditField,
    buffer: TextField,
    target: usize,
}

/// The Layers panel's per-plane row filters. The panel lists **every** layer in
/// map order; the chips narrow it by plane. All three default on (the whole
/// list shows). UI-only state — narrowing the view never changes which layer is
/// selected or paints (a hidden selected layer still edits), so it isn't
/// persisted. Replaces the old single-plane paging.
///
/// A chip click reads as "show me this plane" ([`click`](Self::click)), not
/// "hide this plane": from the everything-shown default it *solos* the clicked
/// plane, further clicks build the shown set up chip by chip, and emptying the
/// set snaps back to all-on — the list can never go blank, which would read as
/// the layers having been erased.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerFilter {
    pub bg: bool,
    pub sprite: bool,
    pub fg: bool,
}
impl Default for LayerFilter {
    fn default() -> Self {
        Self {
            bg: true,
            sprite: true,
            fg: true,
        }
    }
}
impl LayerFilter {
    /// Whether a layer on `plane` is listed (its filter toggle is on).
    fn shows(&self, plane: Plane) -> bool {
        match plane {
            Plane::Bg => self.bg,
            Plane::Sprite => self.sprite,
            Plane::Fg => self.fg,
        }
    }
    /// Flip `plane`'s toggle.
    fn toggle(&mut self, plane: Plane) {
        match plane {
            Plane::Bg => self.bg = !self.bg,
            Plane::Sprite => self.sprite = !self.sprite,
            Plane::Fg => self.fg = !self.fg,
        }
    }
    /// A chip click, with "show me this plane" semantics: from all-on the click
    /// **solos** `plane`; on a narrowed set it toggles `plane` in or out; and a
    /// click that would empty the set restores all-on instead (an all-off list
    /// looks like the layers were erased).
    fn click(&mut self, plane: Plane) {
        if self.bg && self.sprite && self.fg {
            *self = Self {
                bg: false,
                sprite: false,
                fg: false,
            };
            self.toggle(plane);
            return;
        }
        self.toggle(plane);
        if !(self.bg || self.sprite || self.fg) {
            *self = Self::default();
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MapViewer {
    pub focused: bool,
    /// Which draw planes the Layers panel currently lists (bg / sprite / fg
    /// toggles). Filtering hides rows only — it never moves the selection.
    pub filter: LayerFilter,
    /// The **store** index (into `TiledMap.layers`, i.e. a [`LayerInfo::source_layer`])
    /// of the selected layer — an absolute, map-order index that is stable across
    /// filtering and the same value on any plane. `0` (the collision layer) by
    /// default; a stale index just leaves the selection empty (Paint no-ops).
    pub layer_index: usize,
    tool: EditorTool,
    /// The brush's top-left sheet tile. The brush spans `brush_w`×`brush_h` tiles
    /// from here (a box selected in the palette); 1×1 is a single tile.
    selected_tile: usize,
    /// Brush size in tiles (`0` is treated as `1` — see [`brush_size`](Self::brush_size)).
    brush_w: usize,
    brush_h: usize,
    /// Top-left visible tile of the Paint palette (column, row) — the palette is
    /// a fixed [`self.sheet_cols()`]-wide grid that scrolls rather than reflows.
    pal_col: usize,
    pal_row: usize,
    /// The palette viewport's screen rect, cached each frame (the palette is
    /// drawn/hit manually, not as flex nodes).
    pal_rect: Rect,
    /// The active manual press-drag (palette or panel scroll bar), or
    /// [`CanvasDrag::None`].
    canvas_drag: CanvasDrag,
    /// The live sprite sheet's `(cols, rows)` in tiles, passed into `step` by the
    /// host each frame so the palette/brush math adapts as the sheet grows. `(0,
    /// 0)` until the first step (see the `*_DEFAULT` fallback).
    sheet: (usize, usize),
    selected: Option<usize>,
    drag: Option<Vec2>,
    /// The Select tool's committed marquee (tile coords), or `None`. A fresh
    /// click-drag replaces it; Copy / Cut / Delete act on it.
    selection: Option<TileSelection>,
    /// The last Copy / Cut buffer, stamped at the selection origin by Paste.
    clipboard: Option<Clipboard>,
    /// The in-progress drag of an existing object ([`ObjectDrag`]), or `None`.
    moving: Option<ObjectDrag>,
    /// Which animation frame of the selected object's sprite the panel edits.
    /// Reset to `0` when the selection or map changes; clamped to the frame count
    /// at every use so it can't dangle past a shorter sprite.
    sprite_frame: usize,
    /// Playback cursor for the live preview box — independent of `sprite_frame`
    /// (which frame is *edited*) so the preview cycles every frame while you edit
    /// one. Advanced each step against the selected object's live frames.
    preview_frame: usize,
    preview_tick: u16,
    /// The active text-entry session ([`TextEdit`]), or `None` when no field is
    /// being typed into. Bundles the focused field, its line buffer, and the
    /// store-backed layer target so they can only move together (see
    /// [`begin_edit`](Self::begin_edit) / [`stop_editing`](Self::stop_editing)).
    editing: Option<TextEdit>,
    /// In-progress paint stroke: the cells touched since the mouse went down,
    /// flushed into one history entry on release so a stroke undoes atomically.
    stroke: Option<EditAction>,
    /// Bounded undo/redo stacks for tile and object edits.
    history: History<EditAction>,
    /// Unsaved-changes flag + post-save toast countdown, driving the save button.
    status: SaveStatus,
    /// `source` of the map this viewer last stepped. When it changes the viewer
    /// drops its per-map state (see [`reset_for_new_map`](Self::reset_for_new_map)).
    last_map: String,
    /// Where the editor's panels live + the live drag FSM. Owns the per-frame
    /// solved geometry both the hit pass and draw pass read.
    dock: DockManager,
    /// The Maps browser's selected map name (a second click on it opens it).
    maps_selected: Option<String>,
    /// First page row shown in the Maps browser grid (paging, no scroll widget).
    maps_scroll: usize,
    /// Set when the browser (or a warp's "open" button) asks to open a map;
    /// drained by the host (which has the sprite sheet needed to resolve it) so
    /// the editor stays engine-agnostic. The optional point is a map-pixel focus
    /// the camera centres on after loading — a warp's landing point, so opening a
    /// warp's destination frames where it lands.
    pub pending_open: Option<(String, Option<Vec2>)>,
    /// An open fullscreen warp-destination placement session (fully modal): preview
    /// the selected warp's destination 1:1, pan, click to set its landing, then
    /// confirm or cancel. `None` when not placing. See
    /// [`open_warp_preview`](Self::open_warp_preview).
    warp_preview: Option<WarpPreview>,
    /// An open live path-recorder session (fully modal): drive a puppet to record
    /// a cutscene path. `None` when not recording. See
    /// [`open_path_recorder`](Self::open_path_recorder).
    path_recorder: Option<PathRecorder>,
    /// Set after a recorded cutscene is saved: the emitted `.eggscene` source, for
    /// the host to re-parse + `set_scenes` (live-reload). Mirrors the text
    /// editor's `pending_scene`; the editor writes the file itself.
    pub pending_scene: Option<String>,
    /// A scrubber the editor wants opened (the picker's choice, or save-and-play
    /// in the recorder). The engine drains it in `step_mode` — where the cutscene
    /// registry lives — and opens the scrubber. `None` when nothing's requested.
    pub pending_scrub: Option<ScrubRequest>,
    /// Every saved cutscene as `(name, def)`, pushed in by the engine each focused
    /// frame (it owns the registry, the editor doesn't). The Scenes panel and the
    /// `P` scene picker list them by name; the paths overlay ([`scene_paths`]) reads
    /// each `def` to draw its movement paths over the world.
    pub scene_defs: Vec<(String, CutsceneDef)>,
    /// The live actors the engine pushes in each focused frame — `(token, map
    /// position)` for the player, its companions, and every named creature on the
    /// current map — so the path recorder can pick which one it records (it can't
    /// see the walkaround's entities itself). Same refresh cadence as
    /// [`scene_defs`](Self::scene_defs). Pushed by the primary window and every
    /// extra view; if a frame ever leaves it empty the recorder falls back to the
    /// player alone.
    pub recorder_actors: Vec<(String, Vec2)>,
    /// The declared `#flag` vocabulary the engine pushes in each focused frame
    /// (it owns the loaded script, the editor doesn't), so an object's gate
    /// fields (`if` / `unless` / `sets`) can flag an undeclared name — a typo that
    /// would otherwise make the object silently never fire. Same refresh cadence
    /// as [`scene_defs`](Self::scene_defs).
    pub flag_names: Vec<String>,
    /// An open scene-picker session (fully modal): choose a scene to scrub.
    scene_picker: Option<ScenePicker>,
    /// The creature presets the engine pushes in each focused frame (it owns the
    /// registry, the editor doesn't), name-sorted — the Presets panel's listing
    /// and the walk-sprite editor's source of truth. Same refresh cadence as
    /// [`scene_defs`](Self::scene_defs).
    pub preset_defs: Vec<(String, eggdata::PresetDef)>,
    /// An open walk-sprite authoring session (fully modal): edit a preset's
    /// nine-cell walk grid and save it back into `data.toml`. `None` when not
    /// editing. See [`open_walk_editor`](Self::open_walk_editor).
    walk_editor: Option<WalkEditor>,
    /// Set after a walk-editor save wrote `data.toml`: the host re-installs the
    /// live item/preset registries (`EggState::reload_data`), so a spawned
    /// creature picks up the edit. Mirrors [`pending_reload`](Self::pending_reload).
    pub pending_data_reload: bool,
    /// Set after a layer add/delete/move (which edits the stored `TiledMap`);
    /// the host re-derives `current_map`'s layer lists from the store, preserving
    /// the in-memory objects, camera and player.
    pub pending_reload: bool,
    /// The open map-management dialog (new / rename / delete), if any.
    maps_dialog: MapsDialog,
    /// Whether this editor loads/saves the dock layout. Only the primary editor
    /// does (`MapViewer::primary`); extra views are session-only so they don't
    /// race the one layout file. Default `false`.
    persist: bool,
    /// Whether the tile grid + cursor tile-coordinate readout overlay is shown
    /// over the world (toggled with `G`).
    show_grid: bool,
    /// Whether every saved cutscene's movement paths are drawn over the world
    /// (toggled from the Scenes panel). Session-only, like [`show_grid`](Self::show_grid);
    /// the overlay reads [`scene_defs`](Self::scene_defs) via [`scene_paths`].
    show_paths: bool,
    /// The dialogue key the Dialog panel currently previews — the selected
    /// object's dialogue key (or a warp's narration key) followed live, or a key
    /// picked from the browser. `None` until a key is in view. Editing happens in
    /// the text editor, so the panel only tracks and previews the key.
    dialogue_key: Option<String>,
    /// Which previewed message is shown, paged by ◂ ▸. Clamped to the preview
    /// length each step.
    dialogue_msg: usize,
    /// Every dialogue key the script defines, refreshed each step — the Dialog
    /// browser's pick list.
    dialogue_keys: Vec<String>,
    /// The faithful preview, resolved each step by `sync_dialogue` from
    /// `script.get_dialogue` with every `#if` carrier flattened against the
    /// live save (`resolve_if_carriers`) — this panel pages through it by
    /// index rather than driving a live `Dialogue` widget, so it needs
    /// ordinary, always-displayable messages, not unpicked branch carriers.
    /// Drawn as the real dialogue box; `build_dialogue`/`draw` read it without
    /// needing the `Script`/`SaveData` again.
    dialogue_preview: Vec<Message>,
    /// Set when the Dialog panel's "edit in text editor" link is clicked: a
    /// request for the host to open the text editor at this dialogue's block,
    /// drained by the frontend's `poll_text_open`. The text editor is the
    /// canonical (lossless) route for editing dialogue.
    pub pending_text_open: Option<TextOpenReq>,
    /// A dialogue key clicked in the browser, awaiting load by [`sync_dialogue`]
    /// (which holds the `Script` the input handlers don't). Loaded next step.
    dialogue_pick: Option<String>,
    /// The save's small-text setting, cached each step so the preview box wraps
    /// exactly as in-game without threading `SaveData` into the draw pass.
    dialogue_small_text: bool,
    /// The save's [`taken`](SaveData::taken) set, cached each step so the objects
    /// panel can badge collected pickups without threading `SaveData` into the
    /// draw pass (the same reason `dialogue_*` are cached). Read via
    /// [`is_object_taken`](Self::is_object_taken).
    taken: BTreeSet<String>,
    /// The `<map>#<id>` key of a pickup whose collected state the editor wants
    /// flipped — its un-take / re-take test toggle. Parked here because the editor
    /// never holds `&mut SaveData`; the host drains it into `save.taken` (see the
    /// `pending_*` drain seam in walkaround `step` / views `update_views`). `None`
    /// when nothing's requested.
    pub pending_taken_toggle: Option<String>,
}

// The editor grew by accretion into one file; these submodules carve it into
// cohesive slices of the one `MapViewer`. Each is `use super::*` over the
// shared state/types/helpers kept here; methods there are `pub(super)`.
mod draw;
mod history;
mod layers;
mod modal;
mod objects;
mod panels;
mod scroll;
mod step;
mod tools;
mod walk_editor;

/// Floor-divide a world pixel coordinate to its tile index. Defined over `i32`
/// (callers promote from `i16` first) so the conversion keeps overflow headroom.
fn px_to_tile(v: i32) -> i32 {
    v.div_euclid(8)
}

/// The grab offset captured when a scroll thumb is pressed: where on the thumb
/// (clamped to its `extent`) the cursor landed, so the thumb tracks the cursor
/// rather than snapping its near edge under it. Shared by the panel + palette bars.
fn grab_offset(cursor: i32, thumb_edge: i32, extent: i32) -> i32 {
    (cursor - thumb_edge).clamp(0, extent)
}

/// The near edge (top / left) of a scroll thumb at scroll position `scroll`:
/// `travel` px of track from `origin`, mapped over `0..=max`.
fn thumb_pos(origin: i32, travel: i32, scroll: i32, max: i32) -> i32 {
    origin + travel * scroll / max
}

/// Invert a thumb drag back to a scroll position: the desired thumb edge
/// (`cursor − grab`) relative to `track_origin`, clamped to the thumb's `travel`
/// and mapped linearly onto `0..=max`. Caller still clamps the result to its own
/// range (the panel's `set_scroll` does no clamping of its own).
fn scroll_from_drag(cursor: i32, grab: i32, track_origin: i32, travel: i32, max: i32) -> i32 {
    (cursor - grab - track_origin).clamp(0, travel) * max / travel
}

/// The map tile (8px grid) under the cursor, in world coordinates.
fn world_tile(mouse: &MouseInput, camera_pos: Vec2) -> (i32, i32) {
    let p = mouse.pos();
    (
        px_to_tile(i32::from(p.x) + i32::from(camera_pos.x)),
        px_to_tile(i32::from(p.y) + i32::from(camera_pos.y)),
    )
}

fn released(button: [bool; 2]) -> bool {
    button[1] && !button[0]
}

/// A unique `<base>_copy[N]` name not already in the store (for Duplicate).
fn dedup_name(base: &str, maps: &MapStore) -> String {
    let first = format!("{base}_copy");
    if !maps.contains(&first) {
        return first;
    }
    (2..)
        .map(|n| format!("{base}_copy{n}"))
        .find(|name| !maps.contains(name))
        .unwrap_or(first)
}

/// Whether `name` is a usable new/renamed map stem: non-empty, no path
/// separators, and not already taken.
fn valid_map_name(name: &str, maps: &MapStore) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && !maps.contains(name)
}

/// Parse a new-map dimension (tiles), clamping to a sane 1..=512 and falling back
/// to `default` on empty/invalid input.
fn parse_dim(s: &str, default: usize) -> usize {
    s.trim()
        .parse::<usize>()
        .ok()
        .filter(|&n| (1..=512).contains(&n))
        .unwrap_or(default)
}

/// Parse an optional palette index from a trimmed buffer for the frame's
/// transparent / outline fields: an empty buffer is `Some(None)` (clear it), a
/// valid `u8` is `Some(Some(n))` (set it), and anything else is `None` — ignored,
/// so a typo doesn't clobber the current value.
fn parse_optional_index(buffer: &str) -> Option<Option<u8>> {
    if buffer.is_empty() {
        Some(None)
    } else {
        buffer.parse::<u8>().ok().map(Some)
    }
}

/// The short label for an interaction kind shown in the Objects panel.
fn interaction_kind_label(i: &Interaction) -> &'static str {
    match i {
        Interaction::None => "none",
        Interaction::Dialogue(_) => "dialog",
        Interaction::Func(f) => f.name().unwrap_or("func"),
        Interaction::Cutscene(_) => "scene",
    }
}

/// Advance an interaction to the next kind, preserving a sensible default param.
/// Cycle: none → dialogue → toggle_dog → piano → note → add_creatures →
/// give_item → cutscene → none. `origin` seeds a fresh `piano` (it sounds the
/// note under its own position); a fresh `give_item` starts with an empty item
/// key (typed into the `item` field).
fn cycle_interaction(current: &Interaction, origin: Vec2) -> Interaction {
    match current {
        Interaction::None => Interaction::Dialogue(String::new()),
        Interaction::Dialogue(_) => Interaction::Func(InteractFn::ToggleDog),
        Interaction::Func(InteractFn::ToggleDog) => Interaction::Func(InteractFn::Piano(origin)),
        Interaction::Func(InteractFn::Piano(_)) => Interaction::Func(InteractFn::Note(0)),
        Interaction::Func(InteractFn::Note(_)) => Interaction::Func(InteractFn::AddCreatures(0)),
        Interaction::Func(InteractFn::AddCreatures(_)) => {
            Interaction::Func(InteractFn::GiveItem(String::new()))
        }
        Interaction::Func(InteractFn::GiveItem(_)) => Interaction::Cutscene(String::new()),
        // Pet (no `func` name) can't be authored; cycle it back to none.
        Interaction::Func(_) => Interaction::None,
        Interaction::Cutscene(_) => Interaction::None,
    }
}

/// Clip `s` to at most `n` characters (so a long map name fits a browser cell).
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect()
    }
}

/// How many autocomplete suggestions the objects panel offers under an
/// interaction key / gate-flag field while it's being edited.
const AUTOCOMPLETE_MAX: usize = 5;

/// The vocabulary entries that prefix-match `prefix`, for the key/flag field
/// autocomplete dropdown: a case-sensitive `starts_with` filter that keeps the
/// vocabulary's own (already alphabetically sorted) order and caps the result at
/// [`AUTOCOMPLETE_MAX`]. An entry equal to `prefix` is dropped — a fully-typed
/// name has nothing left to complete, so the dropdown vanishes once it matches.
fn autocomplete_matches(vocab: &[String], prefix: &str) -> Vec<String> {
    vocab
        .iter()
        .filter(|entry| entry.as_str() != prefix && entry.starts_with(prefix))
        .take(AUTOCOMPLETE_MAX)
        .cloned()
        .collect()
}

/// A cutscene actor's absolute spawn position from the `init` binds, or `None`
/// when it isn't bound to a fixed point (a `player`/`companion` alias, a `find`,
/// or not bound at all) — the seed [`scene_paths`] anchors that actor's path to.
fn actor_spawn_pos(def: &CutsceneDef, actor: &str) -> Option<Vec2> {
    def.init.iter().find_map(|entity| match entity {
        scene::GetEntity::Spawn { name, pos, .. } | scene::GetEntity::GetOrSpawn { name, pos, .. }
            if name == actor =>
        {
            Some(*pos)
        }
        _ => None,
    })
}

/// Best-effort static movement paths for a cutscene's actors, as polylines in
/// absolute map pixels — the Scenes-panel paths overlay's geometry.
///
/// Each actor's last-known absolute position is seeded by a `spawn` / `bind`
/// init ([`actor_spawn_pos`]); the `content` is then walked in order, following
/// only the motions with static geometry:
/// * `walk` / `noclip` ([`Motion::MoveToPoint`] / [`Motion::MoveToPointNoclip`])
///   append their target, starting the polyline from the last-known point;
/// * `teleport` ([`Motion::Teleport`]) ends the current polyline and starts a
///   fresh one at the jump point — a discontinuity, not a walk;
/// * `record` ([`Motion::Record`]) integrates run-by-run from the last-known
///   point (`pos += heading × frames`, one point per run — the same arithmetic
///   as the cutscene skip-snap), and is skipped entirely with no known position;
/// * `to` / `beside` ([`Motion::MoveToEntity`] / [`Motion::MoveBesideHorizontal`])
///   are entity-relative, so the actor's position becomes unknown: the polyline
///   ends and nothing anchors again until the next absolute motion;
/// * `face` motions and non-`move` content don't move the actor.
///
/// Single-point polylines (a spawn with no motion) are kept — the overlay draws
/// them as a marker dot. Collision isn't re-walked (a `walk` blocked in-game
/// still draws its straight-line intent), hence "best-effort". Actors are walked
/// in a stable order (init binds first, then content-first appearance) so the
/// output — and the overlay's per-scene label anchor — is deterministic.
fn scene_paths(def: &CutsceneDef) -> Vec<Vec<Vec2>> {
    // The actors to trace, in a stable order.
    let mut actors: Vec<&str> = Vec::new();
    for entity in &def.init {
        let name = match entity {
            scene::GetEntity::Spawn { name, .. }
            | scene::GetEntity::GetOrSpawn { name, .. }
            | scene::GetEntity::GetOrIgnore { name }
            | scene::GetEntity::Alias { name, .. } => name.as_str(),
        };
        if !actors.contains(&name) {
            actors.push(name);
        }
    }
    for step in &def.content {
        if let CutsceneContent::Move(chains) = step {
            for chain in chains {
                let name = chain.actor.as_str();
                if !actors.contains(&name) {
                    actors.push(name);
                }
            }
        }
    }

    // Close an open polyline into the output, keeping single-point dots.
    let flush = |open: &mut Vec<Vec2>, paths: &mut Vec<Vec<Vec2>>| {
        if !open.is_empty() {
            paths.push(std::mem::take(open));
        }
    };

    let mut paths: Vec<Vec<Vec2>> = Vec::new();
    for actor in actors {
        // This actor's last-known absolute position and the polyline it's extending.
        let mut pos: Option<Vec2> = actor_spawn_pos(def, actor);
        let mut open: Vec<Vec2> = pos.map(|p| vec![p]).unwrap_or_default();
        for step in &def.content {
            let CutsceneContent::Move(chains) = step else {
                continue;
            };
            for chain in chains {
                if chain.actor != actor {
                    continue;
                }
                for ins in &chain.instructions {
                    match &ins.motion {
                        Motion::MoveToPoint(p) | Motion::MoveToPointNoclip(p) => {
                            if open.is_empty() && let Some(start) = pos {
                                open.push(start);
                            }
                            open.push(*p);
                            pos = Some(*p);
                        }
                        Motion::Teleport(p) => {
                            flush(&mut open, &mut paths);
                            open.push(*p);
                            pos = Some(*p);
                        }
                        Motion::Record { runs, .. } => {
                            let Some(mut cur) = pos else {
                                continue;
                            };
                            if open.is_empty() {
                                open.push(cur);
                            }
                            // Integrate each run as the skip-snap does: heading held
                            // for `frames` at 1 px/frame.
                            for ((dx, dy), frames) in runs {
                                cur = Vec2::new(
                                    cur.x + *dx as i16 * *frames as i16,
                                    cur.y + *dy as i16 * *frames as i16,
                                );
                                open.push(cur);
                            }
                            pos = Some(cur);
                        }
                        Motion::MoveToEntity(_) | Motion::MoveBesideHorizontal { .. } => {
                            // Entity-relative: the absolute position is now unknown.
                            flush(&mut open, &mut paths);
                            pos = None;
                        }
                        Motion::FaceEntity(_) | Motion::FaceDir(_, _) => {}
                    }
                }
            }
        }
        flush(&mut open, &mut paths);
    }
    paths
}

/// Splice one emitted `#cutscene NAME …` block into raw `.eggscene` source,
/// preserving every other line verbatim — comments, blank lines, and other
/// scenes all survive (unlike a parse-then-re-emit, which discards them). If a
/// block with the same `name` already exists it's replaced in place; otherwise
/// the block is appended. Blocks are delimited by a `#cutscene NAME` header at
/// column 0, running to the next such header or end of file.
fn merge_cutscene_source(raw: &str, name: &str, block: &str) -> String {
    // The name token of a `#cutscene` header line (col 0), else `None`.
    fn header_name(line: &str) -> Option<&str> {
        line.strip_prefix("#cutscene ")
            .and_then(|r| r.split_whitespace().next())
    }
    let block = block.trim_end();
    let lines: Vec<&str> = raw.lines().collect();

    let Some(start) = lines.iter().position(|l| header_name(l) == Some(name)) else {
        // Absent: append after the existing content, blank-line separated.
        let mut out = raw.trim_end().to_string();
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(block);
        out.push('\n');
        return out;
    };

    // Present: replace `start..end`, where `end` is the next header (or EOF).
    let end = lines[start + 1..]
        .iter()
        .position(|l| header_name(l).is_some())
        .map_or(lines.len(), |i| start + 1 + i);
    let mut out = String::new();
    for l in &lines[..start] {
        out.push_str(l);
        out.push('\n');
    }
    out.push_str(block);
    out.push('\n');
    let rest = &lines[end..];
    if !rest.is_empty() {
        out.push('\n'); // blank line before the next block
        for l in rest {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

/// Frame a 1:1 map preview: clamp a camera coordinate (top-left, map px) so the
/// map stays in view — pan within `[0, content - view]` when the map is larger
/// than the viewport, pin to a centred negative offset when it's smaller.
fn clamp_camera(v: i16, content: i16, view: i16) -> i16 {
    if content <= view {
        -((view - content) / 2)
    } else {
        v.clamp(0, content - view)
    }
}

/// Render map `name` 1:1 at the map origin (no camera) into a fresh image, using
/// the live sprite sheet from `draw_state`. The same per-layer render as the
/// world: tile layers via the sheet, image layers blitted. Returns the image and
/// its `(width, height)` px. `None` if the map is unknown.
fn render_map_full(
    name: &str,
    maps: &MapStore,
    draw_state: &DrawState,
) -> Option<(RgbaImage, u32, u32)> {
    let info = map_by_name(&draw_state.indexed_sprites, name, maps)?;
    let tiled = maps.get(name)?;
    let (fw, fh) = (
        (tiled.width as u32 * 8).max(1),
        (tiled.height as u32 * 8).max(1),
    );
    let sprites = &draw_state.indexed_sprites;
    let palette = draw_state.palettes[0].as_slice();

    let mut full = RgbaImage::new(fw, fh);
    for layer in info
        .layers
        .iter()
        .chain(info.sprite_layers.iter())
        .chain(info.fg_layers.iter())
    {
        if !layer.visible {
            continue;
        }
        let opts: MapOptions = layer.clone().into();
        match tiled.layers.get(layer.source_layer) {
            Some(TiledMapLayer::TileLayer(tl)) => {
                let pmap = palette_map_rotate(layer.palette_rotate() as usize);
                full.map_draw_indexed(tl, sprites, palette, &pmap, opts);
            }
            Some(TiledMapLayer::ImageLayer(img)) => {
                if let Some(px) = &img.pixels {
                    full.blit::<RgbaImage>(
                        opts.sx,
                        opts.sy,
                        px,
                        EdgePolicy::Transparent,
                        Transform::default(),
                        |p| p.a() == 0,
                    );
                }
            }
            _ => {}
        }
    }
    Some((full, fw, fh))
}

/// Nearest-neighbour resample of `full` (`fw`×`fh`) to `(tw, th)` px.
fn downscale(full: &RgbaImage, fw: u32, fh: u32, tw: u32, th: u32) -> RgbaImage {
    let (tw, th) = (tw.max(1), th.max(1));
    let mut out = RgbaImage::new(tw, th);
    for y in 0..th {
        for x in 0..tw {
            let sx = (x * fw / tw).min(fw - 1);
            let sy = (y * fh / th).min(fh - 1);
            out.set_pixel(x, y, full.get_pixel(sx, sy));
        }
    }
    out
}

/// Render a downscaled preview of map `name` to fit `(max_w, max_h)` px,
/// nearest-neighbour (downscale only). `None` if the map is unknown or `max_*`
/// is zero.
fn render_map_thumbnail(
    name: &str,
    maps: &MapStore,
    draw_state: &DrawState,
    max_w: u32,
    max_h: u32,
) -> Option<RgbaImage> {
    if max_w == 0 || max_h == 0 {
        return None;
    }
    let (full, fw, fh) = render_map_full(name, maps, draw_state)?;
    let s = (max_w as f32 / fw as f32)
        .min(max_h as f32 / fh as f32)
        .min(1.0);
    Some(downscale(
        &full,
        fw,
        fh,
        (fw as f32 * s) as u32,
        (fh as f32 * s) as u32,
    ))
}

/// Letterbox a `(fw, fh)`-px image inside `outer`, downscale-only, centred.
/// Returns the inner rect the image occupies and the scale (preview px per source
/// px). Shared by the warp preview's draw (where to blit) and click handling
/// (how to invert a click back to a map coordinate) so they can't disagree.
fn fit_preview(outer: Rect, fw: u32, fh: u32) -> (Rect, f32) {
    let s = (outer.w as f32 / fw as f32)
        .min(outer.h as f32 / fh as f32)
        .clamp(0.0, 1.0);
    let (iw, ih) = (
        ((fw as f32 * s) as i16).max(1),
        ((fh as f32 * s) as i16).max(1),
    );
    let ix = outer.x + (outer.w - iw) / 2;
    let iy = outer.y + (outer.h - ih) / 2;
    (
        Rect {
            x: ix,
            y: iy,
            w: iw,
            h: ih,
        },
        s,
    )
}

/// `rect` ∩ `clip`, or `None` if they don't overlap — for cropping a preview to a
/// scrolling panel's visible body.
fn clamp_to(rect: Rect, clip: Rect) -> Option<Rect> {
    let x0 = rect.x.max(clip.x);
    let y0 = rect.y.max(clip.y);
    let x1 = (rect.x + rect.w).min(clip.x + clip.w);
    let y1 = (rect.y + rect.h).min(clip.y + clip.h);
    (x1 > x0 && y1 > y0).then_some(Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// Blit `img` at `at` (its top-left), optionally cropped to `clip`: only the
/// sub-image inside the clip is copied, so a half-scrolled preview stays within
/// its panel's body.
fn blit_clipped(draw_state: &mut DrawState, at: Rect, img: &RgbaImage, clip: Option<Rect>) {
    let region = match clip {
        Some(c) => match clamp_to(at, c) {
            Some(r) => r,
            None => return,
        },
        None => at,
    };
    // Sub-rectangle of `img` that lands in `region`.
    let (sx0, sy0) = ((region.x - at.x) as u32, (region.y - at.y) as u32);
    let canvas = draw_state.rgba(LayerId::BG);
    for dy in 0..region.h as u32 {
        for dx in 0..region.w as u32 {
            let (sx, sy) = (sx0 + dx, sy0 + dy);
            if sx >= img.width() || sy >= img.height() {
                continue;
            }
            let p = img.get_pixel(sx, sy);
            if p.a() == 0 {
                continue;
            }
            canvas.set_pixel((region.x as u32) + dx, (region.y as u32) + dy, p);
        }
    }
}

/// Punch a checkerboard of holes in `img` so a rendered preview reads as a
/// translucent "ghost" over the world beneath it (every other pixel cleared).
fn knockout_checker(img: &mut RgbaImage) {
    for gy in 0..img.height() {
        for gx in 0..img.width() {
            if (gx + gy) % 2 == 1 {
                img.set_pixel(gx, gy, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

/// Blit a (knocked-out) ghost onto `canvas` at `(x, y)`, skipping fully
/// transparent pixels so only the surviving checker cells land.
fn blit_ghost(canvas: &mut RgbaImage, x: i32, y: i32, ghost: &RgbaImage) {
    canvas.blit::<RgbaImage>(
        x,
        y,
        ghost,
        EdgePolicy::Transparent,
        Transform::default(),
        |p| p.a() == 0,
    );
}

/// A hitbox spanning the rectangle between two world points (min size 1px).
fn hitbox_between(a: Vec2, b: Vec2) -> Hitbox {
    Hitbox::new(
        a.x.min(b.x),
        a.y.min(b.y),
        (a.x - b.x).abs().max(1),
        (a.y - b.y).abs().max(1),
    )
}

/// Inclusive `(x0, y0, x1, y1)` tile range (8px grid) covered by the rectangle
/// between two world points — used by the rectangle-fill paint mode.
fn tile_bounds(a: Vec2, b: Vec2) -> (i32, i32, i32, i32) {
    let ax = px_to_tile(i32::from(a.x));
    let ay = px_to_tile(i32::from(a.y));
    let bx = px_to_tile(i32::from(b.x));
    let by = px_to_tile(i32::from(b.y));
    (ax.min(bx), ay.min(by), ax.max(bx), ay.max(by))
}

/// Write a tile layer's numeric property in the store — the shared apply step
/// for [`EditAction::LayerSetProp`]'s commit / revert / reapply.
fn apply_layer_prop(maps: &mut MapStore, source: &str, index: usize, prop: LayerProp, value: f64) {
    let Some(tm) = maps.get_mut(source) else {
        return;
    };
    match prop {
        LayerProp::OffsetX => tm.set_layer_offset_x(index, value),
        LayerProp::OffsetY => tm.set_layer_offset_y(index, value),
        // Palette rotation is mod-16; normalise here (the single write site) so
        // every path agrees and revert/reapply are exact inverses.
        LayerProp::Rotate => tm.set_layer_palette_rotate(index, value.rem_euclid(16.0) as u8),
    }
}

/// The short label for a draw [`Plane`], shown on the Layers panel's plane
/// button and (upper-cased) the Paint tool's target readout.
fn plane_short(plane: Plane) -> &'static str {
    match plane {
        Plane::Bg => "bg",
        Plane::Sprite => "spr",
        Plane::Fg => "fg",
    }
}

/// The tile [`TileSelection`] spanning world points `a`..=`b` (inclusive cells).
/// `x`/`y` may be negative when dragged past the layer's top-left; the ops skip
/// the off-layer cells.
fn selection_between(a: Vec2, b: Vec2) -> TileSelection {
    let (x0, y0, x1, y1) = tile_bounds(a, b);
    TileSelection {
        x: x0,
        y: y0,
        w: (x1 - x0 + 1) as usize,
        h: (y1 - y0 + 1) as usize,
    }
}

/// Return a copy of `snapshot` with its hitbox origin set to `origin` — used to
/// reconstruct the pre-drag "before" snapshot for a move's undo entry.
fn move_snapshot(mut snapshot: ObjSnapshot, origin: Vec2) -> ObjSnapshot {
    snapshot.hitbox.x = origin.x;
    snapshot.hitbox.y = origin.y;
    snapshot
}

/// Structural equality for object snapshots. [`MapObject`] doesn't derive
/// `PartialEq` (its effect can hold a fn pointer), so compare the fields the
/// editor can actually change — enough to skip recording no-op edits. The
/// `trigger` axis lives on the object itself (editable on both tabs), so it's
/// compared here alongside the hitbox, effect and animated sprite (`AnimFrame`
/// derives `PartialEq`), so frame edits record an undo step.
fn snapshot_eq(a: &ObjSnapshot, b: &ObjSnapshot) -> bool {
    let same_box = a.hitbox.x == b.hitbox.x
        && a.hitbox.y == b.hitbox.y
        && a.hitbox.w == b.hitbox.w
        && a.hitbox.h == b.hitbox.h;
    same_box
        && a.trigger == b.trigger
        && a.removable == b.removable
        && a.sprite == b.sprite
        && effect_eq(&a.effect, &b.effect)
}

/// Compare two object effects by their editable content (warp fields / dialogue
/// key / interaction kind). Cross-kind never compares equal.
fn effect_eq(a: &ObjectEffect, b: &ObjectEffect) -> bool {
    match (a, b) {
        (ObjectEffect::Warp(x), ObjectEffect::Warp(y)) => {
            x.map == y.map
                && x.to == y.to
                && axis_label(&x.flip) == axis_label(&y.flip)
                && mode_label(&x.mode) == mode_label(&y.mode)
                && sound_label(&x.sound) == sound_label(&y.sound)
                && x.narration == y.narration
        }
        (ObjectEffect::Interact(x), ObjectEffect::Interact(y)) => interaction_eq(x, y),
        _ => false,
    }
}

/// Compare two interactions by their editable content (dialogue key / kind).
fn interaction_eq(a: &Interaction, b: &Interaction) -> bool {
    match (a, b) {
        (Interaction::Dialogue(x), Interaction::Dialogue(y)) => x == y,
        (Interaction::Cutscene(x), Interaction::Cutscene(y)) => x == y,
        (Interaction::None, Interaction::None) => true,
        (Interaction::Func(x), Interaction::Func(y)) => x == y,
        _ => false,
    }
}

/// Remove object `i` from the objects list, ignoring out-of-range indices.
fn remove_object(map: &mut MapInfo, i: usize) {
    if i < map.objects.len() {
        map.objects.remove(i);
    }
}

/// The mutable [`AnimFrame`] at `frame` of object `i`'s sprite, if it exists.
/// Returns `None` for a missing object, a spriteless object, or a stale index.
fn frame_mut(map: &mut MapInfo, i: usize, frame: usize) -> Option<&mut AnimFrame> {
    map.objects
        .get_mut(i)
        .and_then(|o| o.sprite.as_mut())
        .and_then(|frames| frames.get_mut(frame))
}

/// Insert `snapshot` at index `i`, clamping past-the-end inserts to a push so
/// undo of a delete always lands the object back.
fn insert_object(map: &mut MapInfo, i: usize, snapshot: ObjSnapshot) {
    let i = i.min(map.objects.len());
    map.objects.insert(i, snapshot);
}

/// Overwrite object `i` in place with `snapshot` (used to replay an in-place
/// modify). No-op if the index no longer exists.
fn set_object(map: &mut MapInfo, i: usize, snapshot: ObjSnapshot) {
    if let Some(slot) = map.objects.get_mut(i) {
        *slot = snapshot;
    }
}

fn axis_label(axis: &Axis) -> &'static str {
    match axis {
        Axis::None => "none",
        Axis::X => "x",
        Axis::Y => "y",
        Axis::Both => "xy",
    }
}

/// Terse label for a sprite frame's [`Flip`] cycle row.
fn flip_label(flip: &Flip) -> &'static str {
    match flip {
        Flip::None => "none",
        Flip::Horizontal => "horiz",
        Flip::Vertical => "vert",
        Flip::Both => "both",
    }
}

/// Terse label for a sprite frame's [`Rotate`] cycle row (degrees clockwise).
fn rotate_label(rotate: &Rotate) -> &'static str {
    match rotate {
        Rotate::None => "0",
        Rotate::By90 => "90",
        Rotate::By180 => "180",
        Rotate::By270 => "270",
    }
}

/// Advance a sprite frame's mirror: none → horiz → vert → both → none.
fn cycle_anim_flip(flip: &Flip) -> Flip {
    match flip {
        Flip::None => Flip::Horizontal,
        Flip::Horizontal => Flip::Vertical,
        Flip::Vertical => Flip::Both,
        Flip::Both => Flip::None,
    }
}

/// Advance a sprite frame's rotation: 0 → 90 → 180 → 270 → 0.
fn cycle_rotate(rotate: &Rotate) -> Rotate {
    match rotate {
        Rotate::None => Rotate::By90,
        Rotate::By90 => Rotate::By180,
        Rotate::By180 => Rotate::By270,
        Rotate::By270 => Rotate::None,
    }
}

fn mode_label(mode: &WarpMode) -> &'static str {
    match mode {
        WarpMode::Auto => "auto",
        WarpMode::Interact => "act",
    }
}

fn sound_label(sound: &Option<SfxData>) -> &'static str {
    match sound {
        None => "none",
        Some(s) if s.id == sound::door().id => "door",
        Some(s) if s.id == sound::stairs_down().id => "dn",
        Some(s) if s.id == sound::stairs_up().id => "up",
        Some(_) => "?",
    }
}

/// Label for the [`CycleField::Removable`] toggle (consume-on-interact pickup).
fn removable_label(removable: bool) -> &'static str {
    if removable { "yes" } else { "no" }
}

fn cycle_flip(axis: &Axis) -> Axis {
    match axis {
        Axis::None => Axis::X,
        Axis::X => Axis::Y,
        Axis::Y => Axis::Both,
        Axis::Both => Axis::None,
    }
}

fn cycle_mode(mode: &WarpMode) -> WarpMode {
    match mode {
        WarpMode::Interact => WarpMode::Auto,
        WarpMode::Auto => WarpMode::Interact,
    }
}

/// Advance the trigger cycle row: Touch → Press → Any → Enter → Touch. `Enter`
/// (the map-enter hook) only does anything on a cutscene interaction, but it's in
/// the cycle for every object so it can be authored in place; on other kinds it
/// simply never fires (see [`Trigger::Enter`]).
fn cycle_trigger(trigger: Trigger) -> Trigger {
    match trigger {
        Trigger::Touch => Trigger::Press,
        Trigger::Press => Trigger::Any,
        Trigger::Any => Trigger::Enter,
        Trigger::Enter => Trigger::Touch,
    }
}

fn cycle_sound(sound: &Option<SfxData>) -> Option<SfxData> {
    match sound {
        None => Some(sound::door()),
        Some(s) if s.id == sound::door().id => Some(sound::stairs_down()),
        Some(s) if s.id == sound::stairs_down().id => Some(sound::stairs_up()),
        _ => None,
    }
}

/// Re-parse the just-written map JSON back into the store, so re-entering the
/// map rebuilds from the saved state. Tile paints edit the store's `TiledMap` in
/// place, but object edits live only on the running [`MapInfo`] until a save —
/// without this write-back, leaving and re-entering the map would parse the
/// stale pre-edit object layer (the disk file is right, the memory copy isn't).
/// Runtime image pixels aren't serialised, so they're carried over from the old
/// entry by path before the swap; a parse failure leaves the store untouched
/// (the written file is still good — it round-trips by construction).
fn sync_store(maps: &mut MapStore, name: &str, json: &str) {
    use egg_world::data::tiled::{TiledMapLayer, from_json};
    match from_json(json.as_bytes()) {
        Ok(mut fresh) => {
            if let Some(old) = maps.get(name) {
                let pixels: Vec<(String, _)> = old
                    .layers
                    .iter()
                    .filter_map(|layer| match layer {
                        TiledMapLayer::ImageLayer(image) => {
                            Some((image.image.clone(), image.pixels.clone()?))
                        }
                        _ => None,
                    })
                    .collect();
                for (path, pixels) in pixels {
                    fresh.attach_image(&path, pixels);
                }
            }
            maps.insert(name, fresh);
        }
        Err(e) => log::warn!("save: re-parsing {name}.tmj for the store failed: {e}"),
    }
}

// Test-only helpers shared by the split-out submodule test modules; kept at
// module scope so each submodule's `use super::*` picks them up under cfg(test).
#[cfg(test)]
fn tiles(cells: Vec<(i32, i32, usize, usize)>) -> EditAction {
    EditAction::Tiles {
        source: String::new(),
        layer: 0,
        cells,
    }
}

/// The dialogue key of an object's interaction effect, or `""` otherwise.
#[cfg(test)]
fn dialogue_key(object: &MapObject) -> &str {
    match &object.effect {
        ObjectEffect::Interact(Interaction::Dialogue(k)) => k.as_str(),
        _ => "",
    }
}

#[cfg(test)]
mod scene_paths_tests {
    use super::*;

    /// The single cutscene `src` defines.
    fn def(src: &str) -> CutsceneDef {
        scene::parse(src)
            .expect("parse")
            .cutscenes
            .into_values()
            .next()
            .expect("one cutscene")
    }

    /// A spawn then two `walk`s trace one polyline: the spawn point followed by
    /// each waypoint, in order.
    #[test]
    fn spawn_then_walks_is_one_polyline() {
        let paths = scene_paths(&def(
            "#cutscene c\n    spawn a dog 0 0\n    move\n        a: walk 10 0; walk 10 10",
        ));
        assert_eq!(
            paths,
            vec![vec![Vec2::new(0, 0), Vec2::new(10, 0), Vec2::new(10, 10)]]
        );
    }

    /// A `record` integrates run-by-run from the spawn (`pos += heading × frames`),
    /// one appended point per run — the same arithmetic as the skip-snap.
    #[test]
    fn record_runs_integrate_from_the_spawn() {
        let paths = scene_paths(&def(
            "#cutscene c\n    spawn a dog 0 0\n    move\n        a: record 1 0 10 0 1 5",
        ));
        // (0,0) +(1,0)×10 -> (10,0) +(0,1)×5 -> (10,5).
        assert_eq!(
            paths,
            vec![vec![Vec2::new(0, 0), Vec2::new(10, 0), Vec2::new(10, 5)]]
        );
    }

    /// A `teleport` ends the current polyline and starts a fresh one at the jump
    /// point — a discontinuity, not a walked segment.
    #[test]
    fn teleport_splits_into_two_polylines() {
        let paths = scene_paths(&def(
            "#cutscene c\n    spawn a dog 0 0\n    move\n        a: walk 10 0; teleport 50 50; walk 60 50",
        ));
        assert_eq!(
            paths,
            vec![
                vec![Vec2::new(0, 0), Vec2::new(10, 0)],
                vec![Vec2::new(50, 50), Vec2::new(60, 50)],
            ]
        );
    }

    /// A `to NAME` (entity-relative) un-anchors the actor: the polyline ends and a
    /// following `record` contributes nothing until the next absolute motion.
    #[test]
    fn move_to_entity_unanchors_the_actor() {
        let paths = scene_paths(&def(
            "#cutscene c\n    spawn a dog 0 0\n    move\n        a: walk 10 0; to b; record 1 0 10",
        ));
        assert_eq!(paths, vec![vec![Vec2::new(0, 0), Vec2::new(10, 0)]]);
    }

    /// A chain with no spawn anchor and only a `record` produces no polyline (its
    /// position is never known, so the run has nothing to integrate from).
    #[test]
    fn record_without_an_anchor_yields_nothing() {
        let paths = scene_paths(&def("#cutscene c\n    move\n        a: record 1 0 10"));
        assert!(paths.is_empty(), "no anchor ⇒ no path: {paths:?}");
    }

    /// A bare spawn with no motion keeps a single-point polyline — a marker dot at
    /// the spawn.
    #[test]
    fn bare_spawn_is_a_single_point_polyline() {
        let paths = scene_paths(&def("#cutscene c\n    spawn a dog 0 0"));
        assert_eq!(paths, vec![vec![Vec2::new(0, 0)]]);
    }
}

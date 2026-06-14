//! In-game editor for modern Tiled maps: toggle layers, paint tiles, and place
//! or drag map objects (warps and interactions). Opened with `L` in walkaround;
//! freezes the sim while focused and writes edits back to the map's `.tmj`.
//!
//! Warps and interactions live in one [`MapInfo::objects`] list. The two object
//! tools (Interacts / Warps) are *filtered views* over that single list — each
//! tab lists only objects of its kind, mapping its display rows to real vector
//! indices — so the UX is unchanged while the data model is unified.

use crate::{
    data::{
        sound::{self, SfxData},
        tmj::{GameManifest, TiledMap, TiledMapLayer, manifest_from_json, manifest_to_json},
    },
    drawstate::{DrawState, LayerId, PALETTE_MAP_IDENTITY, palette_map_rotate},
    interact::{InteractFn, Interaction},
    map::{
        Axis, LayerInfo, LayerKind, MapInfo, MapObject, MapStore, ObjectEffect, Trigger, Warp,
        WarpMode, map_by_name,
    },
    player::Shell,
    position::{Hitbox, Vec2},
    system::{
        ConsoleApi, ConsoleHelper, MapOptions, MouseInput, ScanCode, SpriteOptions,
        drawing::{
            Canvas, EdgePolicy, Transform,
            image::{Rgba, RgbaImage},
        },
        just_pressed, pressed,
    },
    ui::{NodeId, Rect, Ui, UiBuilder},
};

use super::walkaround::WalkaroundState;

mod dock;
use dock::{Chrome, DockLayout, DockManager, DragState, PanelKind, Placement, Side};

/// Where the editor persists its dock arrangement (native only; on web the
/// asset writes are silent no-ops, so the layout is session-only there).
const LAYOUT_PATH: &str = "config/layout.json";

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
    ToMap,
    ToX,
    ToY,
    /// A warp's pre-warp narration dialogue key (empty buffer ⇒ no narration).
    Narration,
    /// The `note` Func interaction's pitch (an `i32`).
    Pitch,
    /// The `add_creatures` Func interaction's spawn count (a `usize`).
    Count,
    /// The selected object's trigger-hitbox geometry (`i16` px) — the numeric
    /// counterpart to dragging the box. Common to every object kind.
    HitX,
    HitY,
    HitW,
    HitH,
    /// The selected layer's name (lives on the store's `TiledMap`, not an object;
    /// see [`MapViewer::rename_target`]).
    LayerName,
    /// The selected tile layer's pixel offset / palette rotation (also stored on
    /// the `TiledMap`, targeted via [`MapViewer::rename_target`]).
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
    LayerSwap {
        source: String,
        a: usize,
        b: usize,
    },
    /// A layer at `index` in `source` was renamed (also the FG/BG toggle, which
    /// flips the `fg` name prefix).
    LayerRename {
        source: String,
        index: usize,
        before: String,
        after: String,
    },
    /// A tile layer's numeric property (offset / palette rotation) changed.
    LayerSetProp {
        source: String,
        index: usize,
        prop: LayerProp,
        before: f64,
        after: f64,
    },
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
    Title,
    Layer(usize),
    /// A layer row's visibility (eye) toggle.
    LayerVis(usize),
    /// Layers panel toolbar: add tile layer / duplicate / delete / move up /
    /// move down / rename / toggle foreground.
    LayerAdd,
    LayerDup,
    LayerDel,
    LayerUp,
    LayerDown,
    LayerRename,
    LayerFg,
    /// The scrollable tile palette viewport (drag to pan, click to pick a tile).
    PaletteView,
    Object(usize),
    NewObject,
    DupObject,
    DeleteObject,
    /// The selected warp's destination-map preview: a rendered, click-to-place
    /// map of the warp target with the player shown at the landing point.
    WarpPreview,
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
    /// A panel's frame chrome (title bar / close / resize handle), carrying the
    /// panel's index so the dock can act on the right one.
    Dock(usize, Chrome),
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
/// The global undo/redo/save + panel-toggle toolbar's size, px.
const GLOBAL_BAR_W: f32 = 72.0;
const GLOBAL_BAR_H: f32 = 11.0;
/// A Maps-browser cell's thumbnail box size, px (a name label sits below it).
const THUMB_W: f32 = 40.0;
const THUMB_H: f32 = 22.0;
/// The selected warp's destination preview box height, px. Its width fills the
/// Objects panel, so floating/widening that panel grows the click target.
const WARP_PREVIEW_H: f32 = 64.0;
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

/// A single character-level edit to a [`TextField`], the pure unit its console
/// input decodes into. Splitting the keyboard read (which needs a
/// [`ConsoleApi`]) from the buffer mutation (which doesn't) is what lets the
/// field's behaviour be unit-tested without a live console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextOp {
    /// Insert a character at the cursor (a typed, non-control key).
    Push(char),
    /// Delete the character before the cursor (Backspace).
    Pop,
    /// Empty the whole buffer (Ctrl+Backspace).
    Clear,
    /// Move the cursor one character left / right (Arrow keys).
    Left,
    Right,
    /// Move the cursor one word left / right (Ctrl+Arrow).
    WordLeft,
    WordRight,
    /// Finish editing, keeping the buffer (Return).
    Commit,
    /// Finish editing, discarding the buffer (Escape).
    Cancel,
}

/// How a [`TextField`] resolved this frame: still editing, or finished one way or
/// the other. The caller maps [`Commit`](Self::Commit)/[`Cancel`](Self::Cancel)
/// onto its own "apply the buffer" / "abandon" handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextEvent {
    /// The field absorbed input but is still being edited.
    Active,
    /// Return was pressed — commit the (trimmed-by-the-caller) buffer.
    Commit,
    /// Escape was pressed — drop the edit.
    Cancel,
}

/// A line-editing buffer: the string a focused field is accumulating plus the
/// keystroke handling that grows and finishes it. Lifted out of [`MapViewer`]
/// (which used to keep a bare `buffer: String` and hand-roll the per-frame key
/// reads) so the editing logic is one reusable, testable widget-state type.
///
/// [`step`](Self::step) reads the shared console and decodes it into [`TextOp`]s;
/// [`apply`](Self::apply) performs one op on the buffer. Tests drive `apply`
/// directly to exercise push/backspace/commit/cancel without a console. The
/// field tracks only the text — *which* field is being edited and what to do
/// with a committed value stay with the caller.
#[derive(Debug, Clone, Default)]
struct TextField {
    buffer: String,
    /// Caret position as a byte index into `buffer`, always on a char boundary.
    /// Inserts/deletes happen here, and the arrow keys move it.
    cursor: usize,
}

impl TextField {
    /// A field primed with `initial` as its starting contents (e.g. the existing
    /// value of the property being edited), with the caret at the end.
    fn new(initial: impl Into<String>) -> Self {
        let buffer = initial.into();
        let cursor = buffer.len();
        Self { buffer, cursor }
    }

    /// The current buffer contents (the committed/parsed value).
    fn text(&self) -> &str {
        &self.buffer
    }

    /// The buffer with a caret marker inserted at the cursor — what a focused
    /// field renders, so the arrow keys' position is visible.
    fn display(&self) -> String {
        let mut s = self.buffer.clone();
        s.insert(self.cursor, '_');
        s
    }

    /// The byte index of the char boundary just before `cursor`.
    fn prev_boundary(&self) -> usize {
        self.buffer[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    /// The byte index of the char boundary just after `cursor`.
    fn next_boundary(&self) -> usize {
        self.buffer[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |c| self.cursor + c.len_utf8())
    }

    /// Step left over a run of whitespace then a run of word characters — the
    /// start of the word before the cursor.
    fn word_left(&self) -> usize {
        let s = &self.buffer;
        let mut i = self.cursor;
        let is_ws = |i: usize| s[..i].chars().next_back().is_some_and(char::is_whitespace);
        while i > 0 && is_ws(i) {
            i = s[..i].char_indices().next_back().map_or(0, |(j, _)| j);
        }
        while i > 0 && !is_ws(i) {
            i = s[..i].char_indices().next_back().map_or(0, |(j, _)| j);
        }
        i
    }

    /// Step right over a run of whitespace then a run of word characters — the
    /// end of the word after the cursor.
    fn word_right(&self) -> usize {
        let s = &self.buffer;
        let mut i = self.cursor;
        let next = |i: usize| s[i..].chars().next();
        while let Some(c) = next(i).filter(|c| c.is_whitespace()) {
            i += c.len_utf8();
        }
        while let Some(c) = next(i).filter(|c| !c.is_whitespace()) {
            i += c.len_utf8();
        }
        i
    }

    /// Apply one editing op, returning how the field resolved. Edits mutate the
    /// buffer/cursor and stay [`Active`](TextEvent::Active); commit/cancel leave
    /// the buffer untouched and report the terminal event for the caller.
    fn apply(&mut self, op: TextOp) -> TextEvent {
        match op {
            TextOp::Push(c) => {
                self.buffer.insert(self.cursor, c);
                self.cursor += c.len_utf8();
            }
            TextOp::Pop => {
                if self.cursor > 0 {
                    let prev = self.prev_boundary();
                    self.buffer.replace_range(prev..self.cursor, "");
                    self.cursor = prev;
                }
            }
            TextOp::Clear => {
                self.buffer.clear();
                self.cursor = 0;
            }
            TextOp::Left => self.cursor = self.prev_boundary(),
            TextOp::Right => self.cursor = self.next_boundary(),
            TextOp::WordLeft => self.cursor = self.word_left(),
            TextOp::WordRight => self.cursor = self.word_right(),
            TextOp::Commit => return TextEvent::Commit,
            TextOp::Cancel => return TextEvent::Cancel,
        }
        TextEvent::Active
    }

    /// Apply this frame's cursor-editing keys: Backspace (Ctrl ⇒ clear all), and
    /// Left/Right caret motion (Ctrl ⇒ by word). Shared by [`step`](Self::step)
    /// and the map dialogs, which read typed characters themselves (to filter to
    /// digits) but want the same caret behaviour.
    fn edit_keys(&mut self, system: &impl ConsoleApi) {
        let ctrl = system.key(ScanCode::Ctrl);
        if system.keyp(ScanCode::Backspace) {
            self.apply(if ctrl { TextOp::Clear } else { TextOp::Pop });
        }
        if system.keyp(ScanCode::Left) {
            self.apply(if ctrl { TextOp::WordLeft } else { TextOp::Left });
        }
        if system.keyp(ScanCode::Right) {
            self.apply(if ctrl { TextOp::WordRight } else { TextOp::Right });
        }
    }

    /// Consume this frame's keyboard input from `system` and fold it into the
    /// buffer, returning whether the field is still active or finished.
    ///
    /// Typed non-control characters insert at the caret; the arrow keys (and
    /// Ctrl+arrow / Ctrl+Backspace) edit via [`edit_keys`](Self::edit_keys);
    /// Escape cancels and Return commits. Escape takes priority over Return when
    /// (improbably) both fire in one frame.
    fn step(&mut self, system: &impl ConsoleApi) -> TextEvent {
        for c in system.key_chars() {
            if !c.is_control() {
                self.apply(TextOp::Push(*c));
            }
        }
        self.edit_keys(system);
        if system.keyp(ScanCode::Escape) {
            self.apply(TextOp::Cancel)
        } else if system.keyp(ScanCode::Return) {
            self.apply(TextOp::Commit)
        } else {
            TextEvent::Active
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
    Select { anchor_col: usize, anchor_row: usize },
    /// A scroll-bar drag. `grab` is the px offset between the cursor and the
    /// thumb's near edge at press, preserved so the thumb tracks the cursor.
    ScrollV { grab: i16 },
    ScrollH { grab: i16 },
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

#[derive(Debug, Clone, Default)]
pub struct MapViewer {
    pub focused: bool,
    pub fg: bool,
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
    /// An in-progress palette drag (box-select or a scroll-bar drag).
    pal_drag: Option<PalDrag>,
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
    /// While dragging an existing object: the grab offset (cursor − hitbox origin).
    moving: Option<Vec2>,
    /// Origin of the object being dragged, captured at grab time so a completed
    /// drag records a single [`EditAction::Modify`] from there to the drop point.
    move_from: Option<Vec2>,
    /// Which object field, if any, currently has keyboard focus for text entry.
    /// `Some` exactly when a [`TextField`] is open in `field`; the two are set
    /// and cleared together (see [`begin_edit`](Self::begin_edit)).
    editing: Option<EditField>,
    /// The line-editing buffer for the focused `editing` field, or `None` when no
    /// field is being typed into.
    field: Option<TextField>,
    /// While renaming a layer ([`EditField::LayerName`]): the *source* layer index
    /// being renamed, captured at the start so the commit targets the right layer
    /// even though the name lives in the store, not on an object.
    rename_target: Option<usize>,
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
    /// Set when the browser asks to open a map; drained by the host (which has
    /// the sprite sheet needed to resolve it) so the editor stays engine-agnostic.
    pub pending_open: Option<String>,
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
    /// An in-progress panel scroll-bar drag: `(panel index, grab offset within the
    /// thumb)`, so the thumb tracks the cursor rather than snapping under it.
    scroll_drag: Option<(usize, i16)>,
}

impl MapViewer {
    /// True while a text field is capturing keyboard input — the host suppresses
    /// its global debug hotkeys so typed dialogue keys don't trigger them.
    pub fn is_typing(&self) -> bool {
        self.editing.is_some() || self.maps_dialog.is_typing()
    }

    /// The primary editor: like [`default`](Default::default) but it persists its
    /// dock layout to disk (extra views stay session-only).
    pub fn primary() -> Self {
        Self {
            persist: true,
            ..Default::default()
        }
    }

    /// Load the persisted dock layout (primary only), once, on first focus. A
    /// missing/corrupt/old file leaves the default layout. Floats are clamped
    /// into the screen by `recompute`, so a smaller screen can't strand a panel.
    fn load_layout(&mut self, system: &mut impl ConsoleApi) {
        self.dock.loaded = true;
        let Some(layout) = system
            .read_file(LAYOUT_PATH)
            .and_then(|b| serde_json::from_slice::<DockLayout>(&b).ok())
        else {
            return;
        };
        if !layout.panels.is_empty() {
            self.dock.z_top = layout
                .panels
                .iter()
                .map(|p| p.z)
                .max()
                .unwrap_or(0)
                .wrapping_add(1);
            self.dock.panels = layout.panels;
        }
        // A pre-upgrade layout file may predate a panel kind (e.g. Setup); add any
        // missing one so its toggle still works.
        self.dock.ensure_all_kinds();
    }

    /// Write the current dock layout (primary only), clearing the dirty flag.
    fn save_layout(&mut self, system: &mut impl ConsoleApi) {
        self.dock.dirty = false;
        let layout = DockLayout {
            panels: self.dock.panels.clone(),
            version: 1,
        };
        if let Ok(json) = serde_json::to_string_pretty(&layout) {
            system.write_file(LAYOUT_PATH, json.as_bytes());
        }
    }

    // --- Layout (rebuilt each frame for both hit-testing and drawing) ---------

    /// Build one panel — a title-bar chrome row plus the panel kind's body —
    /// laid out at the origin and sized to fill its placed `rect`. The dock
    /// translates it to `rect`'s screen position when hit-testing
    /// ([`Ui::hit_at`]) and drawing ([`Ui::draw_at`]).
    fn build_panel(&self, idx: usize, rect: Rect, map: &MapInfo, maps: &MapStore) -> Ui<EditorKey> {
        let mut b = UiBuilder::new();
        let mut rows: Vec<NodeId> = Vec::new();
        let kind = self.dock.panels[idx].kind;
        let active = self.active_kind() == Some(kind);

        // Title bar: the panel name — a focus/drag handle filling the width. It
        // never shrinks (and on a scrollable panel it stays pinned above the body).
        let title = b
            .text(kind.title())
            .small(true)
            .center()
            .color(if active { 0 } else { 12 })
            .full_width(7.0)
            .no_shrink()
            .fill_if(active, 11)
            .outline(13)
            .key(EditorKey::Dock(idx, Chrome::TitleBar))
            .id();

        match kind {
            PanelKind::Layers => self.build_layers(&mut b, &mut rows, map, maps),
            PanelKind::Paint => {
                self.build_paint_tabs(&mut b, &mut rows);
                if self.tool == EditorTool::Select {
                    self.build_select(&mut b, &mut rows);
                } else {
                    self.build_paint(&mut b, &mut rows);
                }
            }
            PanelKind::Objects => {
                self.build_obj_tabs(&mut b, &mut rows);
                self.build_objects(&mut b, &mut rows, map, rect);
            }
            PanelKind::Maps => self.build_maps(&mut b, &mut rows, rect, maps),
            PanelKind::Map => self.build_setup(&mut b, &mut rows, map, maps),
        }

        let size = (rect.w as f32, rect.h as f32);
        // A scrollable kind keeps its body at natural height (no_shrink) so it
        // overflows the panel and can be scrolled+clipped; other kinds stay in one
        // column that shrinks to fit, exactly as before.
        let root = if Self::is_scroll_kind(kind) {
            let body = b.column(0.0, rows).no_shrink().id();
            b.column(0.0, [title, body]).size(size.0, size.1).fill(0).id()
        } else {
            let mut all = Vec::with_capacity(rows.len() + 1);
            all.push(title);
            all.extend(rows);
            b.column(0.0, all).size(size.0, size.1).fill(0).id()
        };
        b.finish(root, size)
    }

    /// Whether a panel of `kind` scrolls when its content overflows. The Maps
    /// browser pages instead, and the Paint palette scrolls its own viewport, so
    /// both opt out.
    fn is_scroll_kind(kind: PanelKind) -> bool {
        !matches!(kind, PanelKind::Maps | PanelKind::Paint)
    }

    /// This panel's scroll state for the frame: the clamped scroll offset and
    /// whether it is actually scrolling (a scroll-kind whose content overflows
    /// `rect`). `content_h` comes from the built [`Ui::content_height`].
    fn panel_scroll(&self, idx: usize, rect: Rect, content_h: i16) -> (i16, bool) {
        let kind = self.dock.panels[idx].kind;
        let overflow = content_h - rect.h;
        if !Self::is_scroll_kind(kind) || overflow <= 0 {
            return (0, false);
        }
        (self.dock.scroll(idx).clamp(0, overflow), true)
    }

    /// The body region of a scrolling panel (below the pinned title), where the
    /// scrolled content is clipped to.
    fn panel_body(rect: Rect) -> Rect {
        Rect {
            x: rect.x,
            y: rect.y + PANEL_TITLE_H,
            w: rect.w,
            h: (rect.h - PANEL_TITLE_H).max(0),
        }
    }

    /// The scroll bar's grab band — a thin strip down the body's right edge.
    fn scrollbar_zone(rect: Rect) -> Rect {
        let body = Self::panel_body(rect);
        Rect {
            x: body.x + body.w - SCROLLBAR_GRAB,
            y: body.y,
            w: SCROLLBAR_GRAB,
            h: body.h,
        }
    }

    /// The scroll thumb's `(top y, height)` px within the body track, for a given
    /// scroll offset and content height.
    fn scroll_thumb(body: Rect, scroll: i16, content_h: i16) -> (i16, i16) {
        let track = body.h.max(1);
        let body_content = (content_h - PANEL_TITLE_H).max(1);
        // Thumb ∝ visible fraction, at least 4px where the track allows, never
        // taller than the track (the `min` keeps `clamp`'s bounds well-ordered on
        // a very short panel).
        let thumb_h = ((i32::from(track) * i32::from(track) / i32::from(body_content)) as i16)
            .clamp(track.min(4), track);
        let travel = (track - thumb_h).max(0);
        let max_scroll = (content_h - body.h - PANEL_TITLE_H).max(1);
        let top = body.y + (i32::from(travel) * i32::from(scroll) / i32::from(max_scroll)) as i16;
        (top, thumb_h)
    }

    /// Draw a panel's vertical scroll bar down the right edge of its body: a dim
    /// track with a brighter thumb sized/positioned by the scroll fraction.
    fn draw_panel_scrollbar(&self, rect: Rect, scroll: i16, content_h: i16, draw_state: &mut DrawState) {
        let body = Self::panel_body(rect);
        if body.h <= 0 {
            return;
        }
        let bx = (body.x + body.w - SCROLLBAR_W) as i32;
        let track = draw_state.colour(0);
        draw_state.rgba(LayerId::BG).fill_rect(bx, body.y as i32, SCROLLBAR_W as i32, body.h as i32, track);
        let (top, thumb_h) = Self::scroll_thumb(body, scroll, content_h);
        let thumb = draw_state.colour(13);
        draw_state.rgba(LayerId::BG).fill_rect(bx, top as i32, SCROLLBAR_W as i32, thumb_h as i32, thumb);
    }

    /// Front-to-back pick across one panel, scroll-aware: a non-scrolling panel
    /// hits normally; a scrolling one hits its scroll-bar grab band, then its
    /// pinned title, then its scrolled (clipped) body.
    fn hit_panel(&self, idx: usize, rect: Rect, ui: &Ui<EditorKey>, cursor: Vec2) -> Option<EditorKey> {
        let (scroll, scrolling) = self.panel_scroll(idx, rect, ui.content_height());
        if !scrolling {
            return ui.hit_at(rect.x, rect.y, cursor);
        }
        if Self::scrollbar_zone(rect).contains(cursor) {
            return Some(EditorKey::PanelScroll(idx));
        }
        let title_clip = Rect { x: rect.x, y: rect.y, w: rect.w, h: PANEL_TITLE_H };
        let body = Self::panel_body(rect);
        ui.hit_at_clipped(rect.x, rect.y, title_clip, cursor)
            .or_else(|| ui.hit_at_clipped(rect.x, rect.y - scroll, body, cursor))
    }

    /// A mouse-wheel notch over a scrollable panel scrolls its body (independent
    /// of any keyed row beneath the cursor). Topmost panel under the cursor wins.
    fn handle_panel_wheel(&mut self, mouse: &MouseInput, map: &MapInfo, maps: &MapStore) {
        let sy = mouse.scroll_y[0];
        if sy == 0 {
            return;
        }
        let cursor = mouse.pos();
        let Some(&(idx, rect)) = self.dock.solved.rects.iter().rev().find(|(_, r)| r.contains(cursor))
        else {
            return;
        };
        if !Self::is_scroll_kind(self.dock.panels[idx].kind) {
            return;
        }
        let content_h = self.build_panel(idx, rect, map, maps).content_height();
        let overflow = content_h - rect.h;
        if overflow <= 0 {
            return;
        }
        // Wheel up (positive) scrolls toward the top, matching the palette.
        let cur = self.dock.scroll(idx).clamp(0, overflow);
        self.dock
            .set_scroll(idx, (cur - sy.signum() as i16 * SCROLL_STEP).clamp(0, overflow));
    }

    /// Advance an active scroll-bar drag: map the cursor (less the grab offset)
    /// onto the panel's scroll range. Release ends it. Returns `true` while it
    /// owns the mouse, so the caller suppresses other panel/canvas input.
    fn step_scroll_drag(&mut self, mouse: &MouseInput, map: &MapInfo, maps: &MapStore) -> bool {
        let Some((idx, grab)) = self.scroll_drag else {
            return false;
        };
        if released(mouse.left) {
            self.scroll_drag = None;
            return true;
        }
        let Some(rect) = self.dock.solved.rect_of(idx) else {
            self.scroll_drag = None;
            return true;
        };
        let content_h = self.build_panel(idx, rect, map, maps).content_height();
        let body = Self::panel_body(rect);
        let overflow = (content_h - rect.h).max(1);
        let (_, thumb_h) = Self::scroll_thumb(body, 0, content_h);
        let travel = (body.h - thumb_h).max(1);
        let desired_top = mouse.pos().y - grab - body.y;
        let scroll = (i32::from(desired_top) * i32::from(overflow) / i32::from(travel)) as i16;
        self.dock.set_scroll(idx, scroll.clamp(0, overflow));
        true
    }

    /// A small always-on toolbar — undo / redo / save — pinned to the world's
    /// top-left. The global editor controls, independent of any panel (so they
    /// survive whatever the user does with the tool panels).
    fn build_global_bar(&self) -> Ui<EditorKey> {
        let mut b = UiBuilder::new();
        let undo = b
            .text("<")
            .small(true)
            .center()
            .color(if self.history.can_undo() { 12 } else { 13 })
            .size(8.0, 7.0)
            .outlined(0, 13)
            .key(EditorKey::Undo)
            .id();
        let redo = b
            .text(">")
            .small(true)
            .center()
            .color(if self.history.can_redo() { 12 } else { 13 })
            .size(8.0, 7.0)
            .outlined(0, 13)
            .key(EditorKey::Redo)
            .id();
        // Save: `*` = unsaved, `OK` = just-saved toast, plain `S` = clean.
        let (label, oc) = match self.status.button() {
            SaveButton::Toast => ("OK", 11),
            SaveButton::Dirty => ("S*", 9),
            SaveButton::Clean => ("S", 6),
        };
        let save = b
            .text(label)
            .small(true)
            .center()
            .color(oc)
            .size(13.0, 7.0)
            .outlined(0, oc)
            .key(EditorKey::Save)
            .id();
        // Panel show/hide toggles (L / P / O / M), highlighted when open — the
        // one way to reopen a closed panel (e.g. the Maps browser).
        let mut buttons = vec![undo, redo, save];
        for kind in PanelKind::ALL {
            let open = self.dock.panels.iter().any(|p| p.kind == kind && p.open);
            let letter = &kind.title()[..1];
            buttons.push(
                b.text(letter)
                    .small(true)
                    .center()
                    .color(if open { 0 } else { 12 })
                    .size(7.0, 7.0)
                    .fill_if(open, 11)
                    .outlined(0, 12)
                    .key(EditorKey::TogglePanel(kind))
                    .id(),
            );
        }
        let root = b.row(1.0, buttons).fill(0).pad(1.0).id();
        b.finish(root, (GLOBAL_BAR_W, GLOBAL_BAR_H))
    }

    /// The centred modal for the active map dialog (new / rename / delete). Pure
    /// display — driven entirely by the keyboard in [`step_maps_dialog`].
    fn build_dialog(&self) -> Ui<EditorKey> {
        let (sw, sh) = self.dock.solved.screen;
        let mut b = UiBuilder::new();
        let (title, body, hint) = match &self.maps_dialog {
            MapsDialog::New { name, w, h, focus } => {
                // The focused field shows the caret at the cursor; the others show
                // their plain text.
                let shown = |field: &TextField, i: u8| {
                    if *focus == i { field.display() } else { field.text().to_string() }
                };
                (
                    "New map".to_string(),
                    format!(
                        "name: {}\nw: {}  h: {}",
                        shown(name, 0),
                        shown(w, 1),
                        shown(h, 2),
                    ),
                    "Enter=next/ok  Esc=cancel",
                )
            }
            MapsDialog::Rename { name, .. } => (
                "Rename map".to_string(),
                format!("name: {}", name.display()),
                "Enter=ok  Esc=cancel",
            ),
            MapsDialog::ConfirmDelete(n) => (
                "Delete map".to_string(),
                format!("delete '{}'?", truncate(n, 14)),
                "Enter=yes  Esc=no",
            ),
            MapsDialog::Resize { w, h, focus, .. } => {
                let shown = |field: &TextField, i: u8| {
                    if *focus == i { field.display() } else { field.text().to_string() }
                };
                (
                    "Resize map".to_string(),
                    format!("w: {}  h: {}", shown(w, 0), shown(h, 1)),
                    "Enter=next/ok  Esc=cancel",
                )
            }
            MapsDialog::None => (String::new(), String::new(), ""),
        };
        let t = b.text(title).small(true).color(11).full_width(8.0).id();
        let bd = b.text(body).small(true).color(12).full_width(16.0).id();
        let h = b.text(hint).small(true).color(13).full_width(8.0).id();
        let panel = b
            .column(1.0, [t, bd, h])
            .size(120.0, 44.0)
            .pad(3.0)
            .outlined(0, 11)
            .id();
        let root = b.centered(panel).size(sw as f32, sh as f32).id();
        b.finish(root, (sw as f32, sh as f32))
    }

    /// The Objects panel's sub-tabs: Interactables vs Warps, each a tool switch.
    fn build_obj_tabs(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>) {
        let mk = |b: &mut UiBuilder<EditorKey>, tool: EditorTool, label: &str, sel: bool| {
            b.text(label)
                .small(true)
                .center()
                .color(if sel { 0 } else { 12 })
                .full_width(7.0)
                .grow(1.0)
                .fill_if(sel, 11)
                .outlined(0, 12)
                .key(EditorKey::Tool(tool))
                .id()
        };
        let it = mk(b, EditorTool::Interactables, "Intr", self.tool == EditorTool::Interactables);
        let wp = mk(b, EditorTool::Warps, "Warp", self.tool == EditorTool::Warps);
        rows.push(b.row(1.0, [it, wp]).id());
    }

    /// The Maps browser: a paged grid of map cells. A thumbnail is blitted over
    /// each cell in `draw`; the name labels it. A first click selects a map, a
    /// second click on the selected map opens it (via `pending_open`).
    fn build_maps(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        rect: Rect,
        maps: &MapStore,
    ) {
        let names = self.modern_names(maps);
        let (cols, grid_rows) = self.maps_grid(rect);
        let per_page = (cols * grid_rows).max(1);
        let pages = names.len().div_ceil(per_page).max(1);
        let page = self.maps_scroll.min(pages - 1);
        let start = page * per_page;

        // Header: prev / page-count / next.
        let prev = b
            .text("<")
            .small(true)
            .center()
            .color(if page > 0 { 12 } else { 13 })
            .full_width(7.0)
            .grow(1.0)
            .outlined(0, 12)
            .key(EditorKey::MapPrev)
            .id();
        let count = b
            .text(format!("{}/{}", page + 1, pages))
            .small(true)
            .center()
            .color(13)
            .full_width(7.0)
            .grow(2.0)
            .id();
        let next = b
            .text(">")
            .small(true)
            .center()
            .color(if page + 1 < pages { 12 } else { 13 })
            .full_width(7.0)
            .grow(1.0)
            .outlined(0, 12)
            .key(EditorKey::MapNext)
            .id();
        rows.push(b.row(1.0, [prev, count, next]).id());

        // CRUD toolbar: new is always live; dup/rename/delete need a selection.
        let has_sel = self.maps_selected.is_some();
        let tool = |b: &mut UiBuilder<EditorKey>, label: &str, colour: u8, on: bool, key: EditorKey| {
            b.text(label)
                .small(true)
                .center()
                .color(if on { colour } else { 13 })
                .full_width(7.0)
                .grow(1.0)
                .outlined(0, if on { colour } else { 13 })
                .key(key)
                .id()
        };
        let new = tool(b, "+", 11, true, EditorKey::MapNew);
        let dup = tool(b, "dup", 12, has_sel, EditorKey::MapDup);
        let ren = tool(b, "ren", 12, has_sel, EditorKey::MapRename);
        let del = tool(b, "del", 8, has_sel, EditorKey::MapDelete);
        rows.push(b.row(1.0, [new, dup, ren, del]).id());

        // Grid cells: an outlined box (the thumbnail target) over a name label.
        let mut cells = Vec::new();
        for (i, name) in names.iter().enumerate().skip(start).take(per_page) {
            let sel = self.maps_selected.as_deref() == Some(name.as_str());
            let oc = if sel { 11 } else { 12 };
            let thumb = b
                .boxed([])
                .size(THUMB_W, THUMB_H)
                .fill(0)
                .outline(oc)
                .key(EditorKey::MapSlot(i))
                .id();
            let label = b
                .text(truncate(name, 7))
                .small(true)
                .center()
                .color(oc)
                .size(THUMB_W, 6.0)
                .id();
            cells.push(b.column(0.0, [thumb, label]).id());
        }
        rows.push(
            b.wrap_row(1.0, cells)
                .width(cols as f32 * (THUMB_W + 1.0))
                .id(),
        );
    }

    /// The Setup panel: map-level settings read from the store — camera pin,
    /// background palette colour, and the map size with a resize button.
    fn build_setup(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        map: &MapInfo,
        maps: &MapStore,
    ) {
        let tm = maps.get(&map.source);
        let stick = tm.and_then(|t| t.camera_stick());
        let bg = tm.and_then(|t| t.bg_colour()).unwrap_or(0);
        let (w, h) = tm.map(|t| (t.width, t.height)).unwrap_or((0, 0));

        // Camera: auto-frame vs. a fixed pin at the current view centre.
        rows.push(b.text("CAMERA:").small(true).color(13).full_width(8.0).id());
        let cam = match stick {
            Some((x, y)) => format!("stick {x},{y}"),
            None => "auto".to_string(),
        };
        rows.push(b.text(cam).small(true).color(12).full_width(7.0).id());
        let toggle = |b: &mut UiBuilder<EditorKey>, label: &str, on: bool, key: EditorKey| {
            b.text(label)
                .small(true)
                .center()
                .color(if on { 0 } else { 12 })
                .full_width(7.0)
                .grow(1.0)
                .fill_if(on, 11)
                .outlined(0, 12)
                .key(key)
                .id()
        };
        let auto = toggle(b, "auto", stick.is_none(), EditorKey::CamAuto);
        let pin = toggle(b, "pin", stick.is_some(), EditorKey::CamPin);
        rows.push(b.row(1.0, [auto, pin]).id());

        // Background colour: 16 palette swatches, the current one ringed.
        rows.push(b.text(format!("BG: {bg}")).small(true).color(13).full_width(8.0).id());
        let mut swatches = Vec::new();
        for c in 0..16u8 {
            swatches.push(
                b.boxed([])
                    .size(8.0, 8.0)
                    .fill(c)
                    .outline(if c == bg { 11 } else { 0 })
                    .key(EditorKey::BgColour(c))
                    .id(),
            );
        }
        rows.push(b.wrap_row(1.0, swatches).width(64.0).id());

        // Size + resize.
        rows.push(b.text(format!("SIZE: {w}x{h}")).small(true).color(13).full_width(8.0).id());
        rows.push(
            b.text("resize")
                .small(true)
                .center()
                .color(12)
                .full_width(7.0)
                .grow(1.0)
                .outlined(0, 12)
                .key(EditorKey::MapResize)
                .id(),
        );

        // Music: a track name (string-indexed, resolved at load like a warp);
        // `pick` cycles through `[none] + the known tracks`.
        let music = tm.and_then(|t| t.music()).unwrap_or("-");
        rows.push(
            b.text(format!("MUSIC: {}", truncate(music, 11)))
                .small(true)
                .color(13)
                .full_width(8.0)
                .id(),
        );
        let speed = tm.map(|t| t.music_speed()).unwrap_or(1.0);
        let pick = b
            .text("pick")
            .small(true)
            .center()
            .color(12)
            .full_width(7.0)
            .grow(1.0)
            .outlined(0, 12)
            .key(EditorKey::MusicCycle)
            .id();
        // Playback speed sits beside the track picker (it only bites with a track).
        let spd = b
            .text(format!("{speed}x"))
            .small(true)
            .center()
            .color(12)
            .full_width(7.0)
            .grow(1.0)
            .outlined(0, 12)
            .key(EditorKey::MusicSpeedCycle)
            .id();
        rows.push(b.row(1.0, [pick, spd]).id());
    }

    /// The sorted modern-map names — the Maps browser's contents.
    fn modern_names(&self, maps: &MapStore) -> Vec<String> {
        maps.names()
            .into_iter()
            .filter(|n| maps.is_modern(n))
            .map(str::to_string)
            .collect()
    }

    /// How many `(cols, rows)` of map cells fit the Maps panel `rect`.
    fn maps_grid(&self, rect: Rect) -> (usize, usize) {
        let cols = (((rect.w as i32) - 2) / (THUMB_W as i32 + 1)).max(1) as usize;
        let rows = (((rect.h as i32) - 16) / (THUMB_H as i32 + 7)).max(1) as usize;
        (cols, rows)
    }

    /// The panel kind that currently owns the canvas (drives the active-panel
    /// highlight), derived from the active [`EditorTool`].
    fn active_kind(&self) -> Option<PanelKind> {
        Some(match self.tool {
            EditorTool::Layers => PanelKind::Layers,
            EditorTool::Paint | EditorTool::Select => PanelKind::Paint,
            EditorTool::Interactables | EditorTool::Warps => PanelKind::Objects,
        })
    }

    /// The tool a panel of `kind` should activate, given the `current` tool (so
    /// re-activating the Objects panel keeps its Interact/Warp sub-tab). `None`
    /// for panels (Maps) that don't own the canvas.
    fn panel_tool(kind: PanelKind, current: EditorTool) -> Option<EditorTool> {
        match kind {
            // The Layers panel only edits shared layer state (selection /
            // visibility / order); picking a layer must not steal the canvas tool
            // from Paint. (`Layers` stays reachable as a neutral tool via key 1.)
            PanelKind::Layers => None,
            // The Paint panel hosts both Paint and its Select sub-mode; keep
            // whichever is current so clicking the panel body doesn't flip it.
            PanelKind::Paint => Some(if matches!(current, EditorTool::Paint | EditorTool::Select) {
                current
            } else {
                EditorTool::Paint
            }),
            PanelKind::Objects => Some(
                if matches!(current, EditorTool::Interactables | EditorTool::Warps) {
                    current
                } else {
                    EditorTool::Interactables
                },
            ),
            // Map settings and the Maps browser don't own the canvas tool.
            PanelKind::Maps | PanelKind::Map => None,
        }
    }

    /// Visible palette dimensions `(cols, rows)` in tiles, from the cached
    /// viewport rect.
    fn palette_visible(&self) -> (usize, usize) {
        (
            (self.pal_rect.w as usize / 8).max(1),
            (self.pal_rect.h as usize / 8).max(1),
        )
    }

    /// Vertical scroll-thumb metrics in px: `(thumb height, travel)`, where
    /// `travel` is the track length the thumb's top moves over as `pal_row` runs
    /// `0..=max_r`. Shared by [`draw_palette`](Self::draw_palette) and the drag
    /// math so the thumb the user grabs is exactly the thumb they move.
    fn palette_thumb_v(&self) -> (i32, i32) {
        let v = self.pal_rect;
        let (_, vr) = self.palette_visible();
        let total_rows = self.sheet_tiles().div_ceil(self.sheet_cols()).max(1);
        let th = ((v.h as usize * vr) / total_rows).max(2) as i32;
        (th, (v.h as i32 - th).max(1))
    }

    /// Horizontal counterpart of [`palette_thumb_v`](Self::palette_thumb_v):
    /// `(thumb width, travel)`.
    fn palette_thumb_h(&self) -> (i32, i32) {
        let v = self.pal_rect;
        let (vc, _) = self.palette_visible();
        let cols = self.sheet_cols().max(1);
        let tw = ((v.w as usize * vc) / cols).max(2) as i32;
        (tw, (v.w as i32 - tw).max(1))
    }

    /// The maximum scroll `(col, row)` so the last column/row can reach the edge.
    fn palette_scroll_max(&self) -> (usize, usize) {
        let (vc, vr) = self.palette_visible();
        let total_rows = self.sheet_tiles().div_ceil(self.sheet_cols());
        (self.sheet_cols().saturating_sub(vc), total_rows.saturating_sub(vr))
    }

    /// Advance an in-progress palette drag. A `Pan` that barely moved picks the
    /// tile under the press on release; a larger one pans (content follows the
    /// cursor). A scroll-bar drag maps the cursor to the scroll position. Started
    /// by a `PaletteView` press in `handle_panel`.
    fn step_palette_drag(&mut self, mouse: &MouseInput) {
        let Some(drag) = self.pal_drag else {
            return;
        };
        let p = mouse.pos();
        let up = released(mouse.left);
        match drag {
            // Extend the brush box from the anchor to the tile under the cursor.
            PalDrag::Select { anchor_col, anchor_row } => {
                let (c, r) = self.palette_tile_at(p);
                self.set_brush_box(anchor_col, anchor_row, c, r);
                if up {
                    self.pal_drag = None;
                }
            }
            PalDrag::ScrollV { grab } => {
                if up {
                    self.pal_drag = None;
                } else {
                    self.scroll_palette_bar(true, p, grab);
                }
            }
            PalDrag::ScrollH { grab } => {
                if up {
                    self.pal_drag = None;
                } else {
                    self.scroll_palette_bar(false, p, grab);
                }
            }
        }
    }

    /// Map a scroll-bar drag to a scroll position, preserving the `grab` offset
    /// captured at press — so the thumb moves *with* the cursor rather than
    /// snapping its top under it. The desired thumb edge (`cursor − grab`) maps
    /// linearly across the thumb's travel onto `0..=max`.
    fn scroll_palette_bar(&mut self, vertical: bool, p: Vec2, grab: i16) {
        let (max_c, max_r) = self.palette_scroll_max();
        if vertical && max_r > 0 {
            let (_, travel) = self.palette_thumb_v();
            let top = (i32::from(p.y) - i32::from(grab) - i32::from(self.pal_rect.y)).clamp(0, travel);
            self.pal_row = (top * max_r as i32 / travel) as usize;
        } else if !vertical && max_c > 0 {
            let (_, travel) = self.palette_thumb_h();
            let left = (i32::from(p.x) - i32::from(grab) - i32::from(self.pal_rect.x)).clamp(0, travel);
            self.pal_col = (left * max_c as i32 / travel) as usize;
        }
    }

    /// Scroll the palette by a mouse-wheel delta (vertical by default; the
    /// horizontal wheel/touchpad axis scrolls columns). Up scrolls toward row 0.
    fn palette_wheel(&mut self, sx: i8, sy: i8) {
        let (max_c, max_r) = self.palette_scroll_max();
        self.pal_row = (self.pal_row as i32 - sy as i32).clamp(0, max_r as i32) as usize;
        self.pal_col = (self.pal_col as i32 - sx as i32).clamp(0, max_c as i32) as usize;
    }

    /// The brush size in tiles, treating an unset `0` as `1`.
    fn brush_size(&self) -> (usize, usize) {
        (self.brush_w.max(1), self.brush_h.max(1))
    }

    /// Live sprite-sheet width in tiles (the palette's column count), from the
    /// draw-cached size, falling back to the current sheet until the first draw.
    fn sheet_cols(&self) -> usize {
        if self.sheet.0 == 0 { SHEET_COLS_DEFAULT } else { self.sheet.0 }
    }
    /// Live sprite-sheet height in tiles.
    fn sheet_rows(&self) -> usize {
        if self.sheet.1 == 0 { SHEET_ROWS_DEFAULT } else { self.sheet.1 }
    }
    /// Total tiles in the live sheet — every one is selectable in the palette.
    fn sheet_tiles(&self) -> usize {
        self.sheet_cols() * self.sheet_rows()
    }

    /// The sheet `(col, row)` under `point`, clamped into the visible viewport and
    /// the sheet bounds — so a drag that runs off the edge sticks to the last
    /// visible tile rather than wrapping.
    fn palette_tile_at(&self, point: Vec2) -> (usize, usize) {
        let v = self.pal_rect;
        let (vc, vr) = self.palette_visible();
        let cx = (point.x - v.x).clamp(0, (v.w - 1).max(0)) as usize / 8;
        let cy = (point.y - v.y).clamp(0, (v.h - 1).max(0)) as usize / 8;
        let total_rows = self.sheet_tiles().div_ceil(self.sheet_cols());
        let col = (self.pal_col + cx.min(vc - 1)).min(self.sheet_cols() - 1);
        let row = (self.pal_row + cy.min(vr - 1)).min(total_rows - 1);
        (col, row)
    }

    /// Set the brush to the box spanning the anchor and current `(col, row)`.
    fn set_brush_box(&mut self, ac: usize, ar: usize, cc: usize, cr: usize) {
        let (c0, c1) = (ac.min(cc), ac.max(cc));
        let (r0, r1) = (ar.min(cr), ar.max(cr));
        self.selected_tile = r0 * self.sheet_cols() + c0;
        self.brush_w = c1 - c0 + 1;
        self.brush_h = r1 - r0 + 1;
    }

    fn build_layers(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        map: &MapInfo,
        maps: &MapStore,
    ) {
        let layers = if self.fg { &map.fg_layers } else { &map.layers };
        let title = if self.fg { "FG LAYERS:" } else { "BG LAYERS:" };
        rows.push(
            b.text(title)
                .color(13)
                .full_width(8.0)
                .key(EditorKey::Title)
                .id(),
        );
        // Toolbar (two rows): add / duplicate / delete; then move up / down /
        // rename / toggle-foreground. The collision layer (bg #0) is protected
        // from delete / move / rename / fg-flip — its identity is "first tile
        // layer" and an fg-prefix would move it out and break collision.
        let collision = !self.fg && self.layer_index == 0;
        let lt = |b: &mut UiBuilder<EditorKey>, label: &str, c: u8, on: bool, key: EditorKey| {
            b.text(label)
                .small(true)
                .center()
                .color(if on { c } else { 13 })
                .full_width(7.0)
                .grow(1.0)
                .outlined(0, if on { c } else { 13 })
                .key(key)
                .id()
        };
        let add = lt(b, "+L", 11, true, EditorKey::LayerAdd);
        let dup = lt(b, "dup", 12, !collision, EditorKey::LayerDup);
        let del = lt(b, "del", 8, !collision, EditorKey::LayerDel);
        rows.push(b.row(1.0, [add, dup, del]).id());
        let up = lt(b, "^", 12, !collision, EditorKey::LayerUp);
        let dn = lt(b, "v", 12, !collision, EditorKey::LayerDown);
        let ren = lt(b, "ren", 12, !collision, EditorKey::LayerRename);
        // `fg` highlights when the current view is the foreground list.
        let fg = lt(b, "fg", if self.fg { 11 } else { 12 }, !collision, EditorKey::LayerFg);
        rows.push(b.row(1.0, [up, dn, ren, fg]).id());

        let store = maps.get(&map.source);
        for (i, layer) in layers.iter().enumerate() {
            // Eye toggles visibility; the name selects the layer (sticky, by
            // click). The colour flags the kind: red = the protected collision
            // layer (bg #0), grey = an image layer (never a paint target), else
            // a plain tile layer.
            let eye = b
                .text(if layer.visible { "O" } else { "-" })
                .small(true)
                .center()
                .color(if layer.visible { 11 } else { 13 })
                .size(7.0, 7.0)
                .key(EditorKey::LayerVis(i))
                .id();
            let is_collision = !self.fg && i == 0;
            let renaming = self.editing == Some(EditField::LayerName)
                && self.rename_target == Some(layer.source_layer);
            let src_name = store.and_then(|tm| tm.layer_name(layer.source_layer)).unwrap_or("");
            let label = if renaming {
                format!("{}_", self.field.as_ref().map(|f| f.text()).unwrap_or(""))
            } else if is_collision {
                "collision".to_string()
            } else if src_name.is_empty() {
                format!("Layer {i}")
            } else {
                src_name.to_string()
            };
            let colour = if is_collision {
                8
            } else if layer.kind == LayerKind::Image {
                13
            } else {
                12
            };
            let name = b
                .text(label)
                .small(true)
                .color(if renaming { 0 } else { colour })
                .full_width(7.0)
                .grow(1.0)
                .fill_if(renaming, 14)
                .fill_if(!renaming && i == self.layer_index, 15)
                .key(EditorKey::Layer(i))
                .id();
            rows.push(b.row(1.0, [eye, name]).id());
        }

        // The selected tile layer's pixel offset + palette rotation (tile layers
        // only — `layer_offset` is `None` for image/object layers). Click a value
        // to edit it; each edit is one undo step.
        if let Some(src) = self.selected_source_layer(map)
            && let Some((ox, oy)) = store.and_then(|tm| tm.layer_offset(src))
        {
            let rot = store.map(|tm| tm.layer_palette_rotate(src)).unwrap_or(0);
            rows.push(b.spacer(2.0).id());
            self.field_row(b, rows, EditField::LayerOffX, "offx", &ox.to_string());
            self.field_row(b, rows, EditField::LayerOffY, "offy", &oy.to_string());
            self.field_row(b, rows, EditField::LayerRotate, "rot", &rot.to_string());
        }
    }

    /// The Paint tool: a tile-info line, the eraser, then a scrollable palette
    /// viewport. The palette mirrors the sheet's 32-wide layout and is drawn/hit
    /// manually (see [`draw_palette`](Self::draw_palette) / the `PaletteView`
    /// handling), so it just reserves a box here.
    fn build_paint(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>) {
        let target = if self.fg { "FG" } else { "BG" };
        let (bw, bh) = self.brush_size();
        let info = if bw > 1 || bh > 1 {
            format!("T{} {bw}x{bh} {target}{}", self.selected_tile, self.layer_index)
        } else {
            format!("Tile {} {target}{}", self.selected_tile, self.layer_index)
        };
        rows.push(
            b.text(info)
                .small(true)
                .color(13)
                .full_width(8.0)
                .id(),
        );

        // Eraser: paints the empty tile (0). Highlights when it's the brush.
        let erasing = self.selected_tile == 0;
        let eraser = b
            .text("eraser")
            .small(true)
            .center()
            .color(if erasing { 0 } else { 12 })
            .full_width(7.0)
            .key(EditorKey::Eraser);
        let eraser = if erasing { eraser.fill(8) } else { eraser.outlined(0, 8) };
        rows.push(eraser.id());

        // The palette viewport fills the rest of the panel; its tiles + scroll
        // bars are blitted in `draw_palette`, and it captures clicks/drags.
        rows.push(
            b.boxed([])
                .full_width(8.0)
                .grow(1.0)
                .fill(0)
                .key(EditorKey::PaletteView)
                .id(),
        );
    }

    /// The Paint panel's sub-tabs: freehand Paint vs. the marquee Select tool
    /// (mirrors [`build_obj_tabs`](Self::build_obj_tabs)).
    fn build_paint_tabs(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>) {
        let mk = |b: &mut UiBuilder<EditorKey>, tool: EditorTool, label: &str, sel: bool| {
            b.text(label)
                .small(true)
                .center()
                .color(if sel { 0 } else { 12 })
                .full_width(7.0)
                .grow(1.0)
                .fill_if(sel, 11)
                .outlined(0, 12)
                .key(EditorKey::Tool(tool))
                .id()
        };
        let pt = mk(b, EditorTool::Paint, "Paint", self.tool == EditorTool::Paint);
        let sl = mk(b, EditorTool::Select, "Sel", self.tool == EditorTool::Select);
        rows.push(b.row(1.0, [pt, sl]).id());
    }

    /// The Select tool's body: the marquee/clipboard sizes and the ops that act
    /// on them (copy / cut / paste / delete / clear). Disabled ops grey out.
    fn build_select(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>) {
        let sel = match self.selection {
            Some(s) => format!("sel {}x{}", s.w, s.h),
            None => "drag to select".to_string(),
        };
        let clip = match &self.clipboard {
            Some(c) => format!("clip {}x{}", c.w, c.h),
            None => "clip -".to_string(),
        };
        rows.push(b.text(sel).small(true).color(13).full_width(8.0).id());
        rows.push(b.text(clip).small(true).color(13).full_width(8.0).id());

        let has_sel = self.selection.is_some();
        let has_clip = self.clipboard.is_some();
        let op = |b: &mut UiBuilder<EditorKey>, label: &str, colour: u8, on: bool, key: EditorKey| {
            b.text(label)
                .small(true)
                .center()
                .color(if on { colour } else { 13 })
                .full_width(7.0)
                .grow(1.0)
                .outlined(0, if on { colour } else { 13 })
                .key(key)
                .id()
        };
        let copy = op(b, "copy", 12, has_sel, EditorKey::SelCopy);
        let cut = op(b, "cut", 12, has_sel, EditorKey::SelCut);
        rows.push(b.row(1.0, [copy, cut]).id());
        let paste = op(b, "paste", 11, has_sel && has_clip, EditorKey::SelPaste);
        let del = op(b, "del", 8, has_sel, EditorKey::SelDelete);
        rows.push(b.row(1.0, [paste, del]).id());
        let clear = op(b, "clear", 12, has_sel, EditorKey::SelClear);
        rows.push(b.row(1.0, [clear]).id());
    }

    fn build_objects(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        map: &MapInfo,
        rect: Rect,
    ) {
        let warps = self.tool == EditorTool::Warps;
        rows.push(
            b.text(if warps { "WARPS:" } else { "INTERACTS:" })
                .color(13)
                .full_width(8.0)
                .id(),
        );
        let has_sel = self.selected.is_some();
        let obj_btn = |b: &mut UiBuilder<EditorKey>, label: &str, colour: u8, on: bool, key: EditorKey| {
            b.text(label)
                .small(true)
                .center()
                .color(if on { colour } else { 13 })
                .full_width(7.0)
                .grow(1.0)
                .outlined(0, if on { colour } else { 13 })
                .key(key)
                .id()
        };
        let new = obj_btn(b, "+new", 11, true, EditorKey::NewObject);
        let dup = obj_btn(b, "dup", 12, has_sel, EditorKey::DupObject);
        let del = obj_btn(b, "-del", 8, has_sel, EditorKey::DeleteObject);
        rows.push(b.row(2.0, [new, dup, del]).id());

        // Filtered view: list only this tab's kind, numbering rows by their
        // position *within the tab* (`row`), but keying each by its real index
        // into `map.objects` so selection/click map straight back to the vec.
        let kind = self.obj_kind();
        for (row, (i, object)) in map
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| kind.matches(o))
            .enumerate()
        {
            let label = match &object.effect {
                ObjectEffect::Warp(w) => {
                    let dest = w.map.as_deref().unwrap_or("-");
                    format!("{row}: ->{dest}")
                }
                ObjectEffect::Interact(Interaction::Dialogue(k)) => format!("{row}: {k}"),
                ObjectEffect::Interact(Interaction::Func(_)) => format!("{row}: <fn>"),
                ObjectEffect::Interact(Interaction::None) => format!("{row}: <->"),
            };
            rows.push(
                b.text(label)
                    .small(true)
                    .full_width(7.0)
                    .fill_if(Some(i) == self.selected, 15)
                    .key(EditorKey::Object(i))
                    .id(),
            );
        }

        if let Some(object) = self.selected.and_then(|i| map.objects.get(i)) {
            rows.push(b.spacer(2.0).id());
            // Hitbox geometry — numeric pos/size for every object, alongside drag.
            let hb = object.hitbox;
            rows.push(b.text("box:").small(true).color(13).full_width(7.0).id());
            self.field_row(b, rows, EditField::HitX, "x", &hb.x.to_string());
            self.field_row(b, rows, EditField::HitY, "y", &hb.y.to_string());
            self.field_row(b, rows, EditField::HitW, "w", &hb.w.to_string());
            self.field_row(b, rows, EditField::HitH, "h", &hb.h.to_string());
            rows.push(b.spacer(2.0).id());
            match &object.effect {
                ObjectEffect::Warp(w) => {
                    let dest = w.map.as_deref().unwrap_or("-");
                    // `map` is free text (for a not-yet-created target); `pick`
                    // cycles through existing maps so a target can't be a typo.
                    self.field_row(b, rows, EditField::ToMap, "map", dest);
                    self.cycle_row(b, rows, CycleField::WarpTarget, "pick", dest);
                    self.field_row(b, rows, EditField::ToX, "x", &w.to.x.to_string());
                    self.field_row(b, rows, EditField::ToY, "y", &w.to.y.to_string());
                    self.cycle_row(b, rows, CycleField::Flip, "flip", axis_label(&w.flip));
                    self.cycle_row(b, rows, CycleField::Mode, "mode", mode_label(&w.mode));
                    self.cycle_row(b, rows, CycleField::Sound, "snd", sound_label(&w.sound));
                    self.cycle_row(b, rows, CycleField::Trigger, "trig", trigger_label(object.trigger));
                    let narr = w.narration.as_deref().unwrap_or("-");
                    self.field_row(b, rows, EditField::Narration, "narr", narr);
                    // Click-to-place destination preview: a rendered map of the
                    // warp target with the player at the landing point. Drawn over
                    // this box (see `draw_warp_preview`); clicks land here. Last so
                    // it (not the essential fields) is what overflows a short panel.
                    rows.push(b.text("land:").small(true).color(13).full_width(7.0).id());
                    rows.push(
                        b.boxed([])
                            .size((rect.w as f32 - 2.0).max(THUMB_W), WARP_PREVIEW_H)
                            .fill(0)
                            .outline(13)
                            .key(EditorKey::WarpPreview)
                            .id(),
                    );
                }
                ObjectEffect::Interact(interaction) => {
                    // Interaction kind (click to cycle) + its one editable param.
                    self.cycle_row(
                        b,
                        rows,
                        CycleField::IntKind,
                        "type",
                        interaction_kind_label(interaction),
                    );
                    match interaction {
                        Interaction::Dialogue(k) => {
                            self.field_row(b, rows, EditField::Key, "key", k)
                        }
                        Interaction::Func(InteractFn::Note(p)) => {
                            self.field_row(b, rows, EditField::Pitch, "pitch", &p.to_string())
                        }
                        Interaction::Func(InteractFn::AddCreatures(c)) => {
                            self.field_row(b, rows, EditField::Count, "count", &c.to_string())
                        }
                        // None / toggle_dog / piano have no editable param.
                        _ => {}
                    }
                    self.cycle_row(b, rows, CycleField::Trigger, "trig", trigger_label(object.trigger));
                }
            }
        }
    }

    fn field_row(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        field: EditField,
        label: &str,
        value: &str,
    ) {
        let editing = self.editing == Some(field);
        let text = match (editing, &self.field) {
            (true, Some(f)) => format!("{label}:{}", f.display()),
            _ => format!("{label}:{value}"),
        };
        rows.push(
            b.text(text)
                .small(true)
                .color(if editing { 0 } else { 12 })
                .full_width(7.0)
                .fill_if(editing, 14)
                .key(EditorKey::Field(field))
                .id(),
        );
    }

    fn cycle_row(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        field: CycleField,
        label: &str,
        value: &str,
    ) {
        rows.push(
            b.text(format!("{label}:{value}"))
                .small(true)
                .full_width(7.0)
                .key(EditorKey::Cycle(field))
                .id(),
        );
    }

    // --- Helpers --------------------------------------------------------------

    /// Toggle the visibility of the currently selected layer.
    fn toggle_layer(&self, map: &mut MapInfo) {
        let layer = if self.fg {
            map.fg_layers.get_mut(self.layer_index)
        } else {
            map.layers.get_mut(self.layer_index)
        };
        if let Some(layer) = layer {
            layer.visible = !layer.visible;
        }
    }

    fn layer_list_len(&self, map: &MapInfo) -> usize {
        if self.fg {
            map.fg_layers.len()
        } else {
            map.layers.len()
        }
    }

    /// The `TiledMap` layer index of the currently-selected display layer (bg or
    /// fg list at `layer_index`).
    fn selected_source_layer(&self, map: &MapInfo) -> Option<usize> {
        let list = if self.fg { &map.fg_layers } else { &map.layers };
        list.get(self.layer_index).map(|l| l.source_layer)
    }

    /// Open the rename text field on the layer at store `index`, seeded with its
    /// current name. The commit ([`commit_layer_rename`](Self::commit_layer_rename))
    /// writes it back to the store and records the undo step.
    fn begin_layer_rename(&mut self, maps: &MapStore, source: &str, index: usize) {
        let name = maps
            .get(source)
            .and_then(|tm| tm.layer_name(index))
            .unwrap_or("")
            .to_string();
        self.stop_editing();
        self.rename_target = Some(index);
        self.editing = Some(EditField::LayerName);
        self.field = Some(TextField::new(name));
    }

    /// Open a numeric text field on the selected tile layer's offset / palette
    /// rotation, seeded from the store and targeting `index` (via `rename_target`,
    /// the captured layer for store-backed text edits).
    fn begin_layer_field(&mut self, maps: &MapStore, source: &str, index: usize, field: EditField) {
        let tm = maps.get(source);
        let value = match field {
            EditField::LayerOffX => {
                tm.and_then(|t| t.layer_offset(index)).map(|(x, _)| x.to_string()).unwrap_or_default()
            }
            EditField::LayerOffY => {
                tm.and_then(|t| t.layer_offset(index)).map(|(_, y)| y.to_string()).unwrap_or_default()
            }
            EditField::LayerRotate => tm.map(|t| t.layer_palette_rotate(index)).unwrap_or(0).to_string(),
            _ => String::new(),
        };
        self.stop_editing();
        self.rename_target = Some(index);
        self.editing = Some(field);
        self.field = Some(TextField::new(value));
    }

    /// Flip the layer at store `index` between the bg and fg draw lists by
    /// toggling its `fg` name prefix (the one convention that decides it),
    /// recorded as a rename so it undoes like any other.
    fn toggle_layer_fg(&mut self, map: &MapInfo, maps: &mut MapStore, index: usize) {
        let Some(before) = maps.get(&map.source).and_then(|tm| tm.layer_name(index)).map(str::to_string)
        else {
            return;
        };
        let after = toggle_fg_prefix(&before);
        if after == before {
            return;
        }
        if let Some(tm) = maps.get_mut(&map.source) {
            tm.set_layer_name(index, &after);
        }
        // Follow the layer to whichever list it now belongs to, so it doesn't
        // vanish from view (the re-derive next frame settles its row).
        self.fg = after.to_lowercase().starts_with("fg");
        self.layer_index = 0;
        self.record(EditAction::LayerRename {
            source: map.source.clone(),
            index,
            before,
            after,
        });
        self.pending_reload = true;
    }

    /// The layer the paint tool writes into (selected in the Layers tool).
    fn active_layer<'a>(&self, map: &'a MapInfo) -> Option<&'a LayerInfo> {
        let layers = if self.fg { &map.fg_layers } else { &map.layers };
        layers.get(self.layer_index)
    }

    /// Real `map.objects` index of the active tab's object whose hitbox contains
    /// `world` (px) — so clicking in the Warps tab only grabs warps, and the
    /// returned index is the vec index selection works in.
    fn object_at(&self, map: &MapInfo, world: Vec2) -> Option<usize> {
        let kind = self.obj_kind();
        map.objects
            .iter()
            .position(|o| kind.matches(o) && o.hitbox.touches_point(world))
    }

    /// Top-left (px) of object `i`'s hitbox.
    fn object_origin(&self, map: &MapInfo, i: usize) -> Vec2 {
        map.objects
            .get(i)
            .map(|o| Vec2::new(o.hitbox.x, o.hitbox.y))
            .unwrap_or(Vec2::new(0, 0))
    }

    /// Move object `i`'s hitbox top-left to `pos`, keeping its size.
    fn set_object_origin(&self, map: &mut MapInfo, i: usize, pos: Vec2) {
        if let Some(o) = map.objects.get_mut(i) {
            o.hitbox.x = pos.x;
            o.hitbox.y = pos.y;
        }
    }

    /// The object kind the active object tool creates / filters its view to.
    fn obj_kind(&self) -> ObjKind {
        if self.tool == EditorTool::Warps {
            ObjKind::Warp
        } else {
            ObjKind::Interactable
        }
    }

    /// Clone object `i` into a snapshot, if it exists.
    fn snapshot(map: &MapInfo, i: usize) -> Option<ObjSnapshot> {
        map.objects.get(i).cloned()
    }

    // --- History --------------------------------------------------------------

    /// Record an action onto the undo stack and flag the map as unsaved. Every
    /// mutating editor operation funnels through here so dirty-tracking and
    /// history stay in lock-step.
    fn record(&mut self, action: EditAction) {
        self.history.push(action);
        self.status.edited();
    }

    /// Undo the most recent edit (Ctrl+Z). Object indices may shift on
    /// add/remove, so undo restores list shape as well as contents. The action is
    /// cloned out of the history before reverting because `revert` needs `&mut
    /// self`, which can't coexist with a borrow into `self.history`.
    fn undo(&mut self, map: &mut MapInfo, maps: &mut MapStore) {
        if let Some(action) = self.history.undo().cloned() {
            self.revert(map, maps, &action);
            self.status.edited();
        }
    }

    /// Redo the most recently undone edit (Ctrl+Y / Ctrl+Shift+Z).
    fn redo(&mut self, map: &mut MapInfo, maps: &mut MapStore) {
        if let Some(action) = self.history.redo().cloned() {
            self.reapply(map, maps, &action);
            self.status.edited();
        }
    }

    /// Flag a collider re-derive if `layer` is the collision (first tile) layer.
    /// The collision layer's tile art is what `Collider::from_sprite` derives
    /// from, so any change to it — a forward edit *or* an undo/redo — must
    /// re-derive, or in-game collision goes stale (see the host's `pending_reload`
    /// drain). Forward tile edits flag it inline; this keeps undo/redo in step.
    fn flag_collision_reload(&mut self, map: &MapInfo, layer: usize) {
        if map.layers.first().map(|l| l.source_layer) == Some(layer) {
            self.pending_reload = true;
        }
    }

    /// Reverse an action's effect (the undo direction).
    fn revert(&mut self, map: &mut MapInfo, maps: &mut MapStore, action: &EditAction) {
        match action {
            EditAction::Tiles { source, layer, cells } => {
                if let Some(tiles) = maps.get_mut(source) {
                    for &(x, y, old, _new) in cells {
                        tiles.set(*layer, x as usize, y as usize, old);
                    }
                }
                self.flag_collision_reload(map, *layer);
            }
            // Undo an add by removing the (last) object it appended.
            EditAction::Add { index, .. } => {
                remove_object(map, *index);
                self.selected = None;
            }
            // Undo a remove by re-inserting the snapshot at its old index.
            EditAction::Remove { index, before } => {
                insert_object(map, *index, before.clone());
            }
            // Undo a modify by restoring the "before" snapshot.
            EditAction::Modify { index, before, .. } => {
                set_object(map, *index, before.clone());
            }
            // Undo an insert by removing the layer; undo a remove by restoring it;
            // a swap is its own inverse. All change the layer list, so re-derive.
            EditAction::LayerInsert { source, index, .. } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.remove_layer_at(*index);
                }
                self.pending_reload = true;
            }
            EditAction::LayerRemove { source, index, layer } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.insert_layer(*index, (**layer).clone());
                }
                self.pending_reload = true;
            }
            EditAction::LayerSwap { source, a, b } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.swap_layers(*a, *b);
                }
                self.pending_reload = true;
            }
            EditAction::LayerRename { source, index, before, .. } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.set_layer_name(*index, before);
                }
                self.pending_reload = true;
            }
            EditAction::LayerSetProp { source, index, prop, before, .. } => {
                apply_layer_prop(maps, source, *index, *prop, *before);
                self.pending_reload = true;
            }
        }
    }

    /// Re-perform an action's effect (the redo direction).
    fn reapply(&mut self, map: &mut MapInfo, maps: &mut MapStore, action: &EditAction) {
        match action {
            EditAction::Tiles { source, layer, cells } => {
                if let Some(tiles) = maps.get_mut(source) {
                    for &(x, y, _old, new) in cells {
                        tiles.set(*layer, x as usize, y as usize, new);
                    }
                }
                self.flag_collision_reload(map, *layer);
            }
            EditAction::Add { index, after } => {
                insert_object(map, *index, after.clone());
            }
            EditAction::Remove { index, .. } => {
                remove_object(map, *index);
                self.selected = None;
            }
            EditAction::Modify { index, after, .. } => {
                set_object(map, *index, after.clone());
            }
            // Redo: re-insert / re-remove / re-swap, mirroring `revert`.
            EditAction::LayerInsert { source, index, layer } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.insert_layer(*index, (**layer).clone());
                }
                self.pending_reload = true;
            }
            EditAction::LayerRemove { source, index, .. } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.remove_layer_at(*index);
                }
                self.pending_reload = true;
            }
            EditAction::LayerSwap { source, a, b } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.swap_layers(*a, *b);
                }
                self.pending_reload = true;
            }
            EditAction::LayerRename { source, index, after, .. } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.set_layer_name(*index, after);
                }
                self.pending_reload = true;
            }
            EditAction::LayerSetProp { source, index, prop, after, .. } => {
                apply_layer_prop(maps, source, *index, *prop, *after);
                self.pending_reload = true;
            }
        }
    }

    // --- Step (input) ---------------------------------------------------------

    /// The keys the map editor consumes from the shared console — text-entry
    /// control keys plus its command shortcuts (Ctrl+Z/Y/S, the Select clipboard
    /// chords Ctrl+C/X/V, Delete, the 1-5 tool switches and their modifiers). The
    /// host forwards these even when the key
    /// wasn't aimed at the primary window, so editor shortcuts work over any
    /// view; they're inert unless an editor is actually reading them (see
    /// [`step_text_entry`](Self::step_text_entry) / [`handle_shortcuts`](Self::handle_shortcuts)).
    pub fn wants_key(scancode: ScanCode) -> bool {
        matches!(
            scancode,
            ScanCode::Backspace
                | ScanCode::Escape
                | ScanCode::Return
                | ScanCode::Ctrl
                | ScanCode::Shift
                | ScanCode::Z
                | ScanCode::Y
                | ScanCode::S
                // Select-tool clipboard chords (Ctrl+C/X/V).
                | ScanCode::C
                | ScanCode::X
                | ScanCode::V
                | ScanCode::Delete
                // Grid toggle + arrow-key object nudge.
                | ScanCode::G
                | ScanCode::Up
                | ScanCode::Down
                | ScanCode::Left
                | ScanCode::Right
                | ScanCode::Digit1
                | ScanCode::Digit2
                | ScanCode::Digit3
                | ScanCode::Digit4
                | ScanCode::Digit5
        )
    }

    pub fn step_map_viewer(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        maps: &mut MapStore,
        camera_pos: Vec2,
        sheet: (usize, usize),
    ) {
        let screen = (system.width() as f32, system.height() as f32);
        self.step_map_viewer_at(system, map, maps, camera_pos, screen, sheet);
    }

    /// Like [`step_map_viewer`](Self::step_map_viewer) but with an explicit
    /// `screen` size for the panel layout/hit-testing. An extra view's
    /// framebuffer can be any size, while `system.width()/height()` is always
    /// the *main* window's framebuffer.
    pub fn step_map_viewer_at(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        maps: &mut MapStore,
        camera_pos: Vec2,
        screen: (f32, f32),
        sheet: (usize, usize),
    ) {
        // The live sprite-sheet size (in tiles), so the palette spans the whole
        // sheet and the brush/scroll math adapt as it grows.
        self.sheet = sheet;
        // A different map under the editor invalidates the per-map state: object
        // undo entries and the selection index point into the *old* map's objects
        // list, so replaying them here would edit the wrong things. Self-detected
        // (rather than hooked into `load_map`) so every viewer instance heals,
        // including the extra views' own editors stepping the same shared map.
        if self.last_map != map.source {
            self.reset_for_new_map();
            self.last_map = map.source.clone();
        }

        self.status.tick();

        // Restore the saved dock layout once, lazily, on first focus (primary).
        if self.persist && !self.dock.loaded {
            self.load_layout(system);
        }

        if self.maps_dialog.is_active() {
            // A modal map dialog (new / rename / delete) captures all input.
            self.step_maps_dialog(system, maps);
        } else if self.editing.is_some() {
            // While a text field is focused all keys feed the buffer — don't let
            // editor shortcuts (incl. a typed "z") fire.
            self.step_text_entry(system, map, maps);
        } else {
            self.handle_shortcuts(system, map, maps);
        }

        // Tile the panels once; both this hit pass and the later draw pass read
        // the same `self.dock.solved`, so they can't disagree about geometry.
        self.dock.recompute(screen);
        // Cache the Paint palette's viewport rect for the pan/pick + draw math.
        let pal_rect = self.dock.open_panel(PanelKind::Paint).and_then(|(idx, rect)| {
            self.build_panel(idx, rect, map, maps)
                .rect_at(rect.x, rect.y, EditorKey::PaletteView)
        });
        self.pal_rect = pal_rect.unwrap_or_default();
        let mouse = system.mouse();
        let cursor = mouse.pos();

        // A modal dialog swallows mouse interaction with the panels/world.
        if self.maps_dialog.is_active() {
            // nothing
        } else if self.scroll_drag.is_some() {
            // A panel scroll-bar drag owns the mouse.
            self.step_scroll_drag(&mouse, map, maps);
        } else if self.pal_drag.is_some() {
            // A palette drag (pan / tile-pick / scroll bar) owns the mouse.
            self.step_palette_drag(&mouse);
        } else if self.step_drag(&mouse, screen) {
            // A panel drag (move / tear-off / resize) owns the mouse this frame —
            // suppress panel and canvas input so it can't paint or re-select.
        } else if let Some(key) = self.global_bar_hit(cursor) {
            // The always-on undo/redo/save bar wins over the world beneath it.
            self.handle_panel(system, map, maps, usize::MAX, key, camera_pos);
        } else {
            // A wheel notch scrolls the panel under the cursor (any region of it).
            self.handle_panel_wheel(&mouse, map, maps);
            // Front-to-back pick across panels (reverse draw order); first keyed
            // node under the cursor wins. Each panel is laid out at the origin and
            // translated to its placed rect, scroll-aware, for the hit test.
            let mut panel_hit = None;
            for &(idx, rect) in self.dock.solved.rects.iter().rev() {
                let ui = self.build_panel(idx, rect, map, maps);
                if let Some(key) = self.hit_panel(idx, rect, &ui, cursor) {
                    panel_hit = Some((idx, key));
                    break;
                }
            }
            match panel_hit {
                Some((idx, key)) => {
                    self.handle_panel(system, map, maps, idx, key, camera_pos)
                }
                // World gate: canvas tools fire only over the leftover world view
                // (not behind a docked strip) and only when nothing is dragging.
                None if self.dock.solved.world.contains(cursor) => {
                    self.handle_canvas(system, map, maps, camera_pos, &mouse)
                }
                None => {}
            }
        }

        // Controller fallback for the Layers tool (matches the old viewer).
        if self.tool == EditorTool::Layers {
            let pad = system.controller();
            if just_pressed(pad.up) {
                self.layer_index = self.layer_index.saturating_sub(1);
            }
            if just_pressed(pad.down) {
                let len = self.layer_list_len(map);
                self.layer_index = (self.layer_index + 1).min(len.saturating_sub(1));
            }
            if just_pressed(pad.a) {
                self.toggle_layer(map);
            }
            if just_pressed(pad.b) {
                self.fg = !self.fg;
            }
        }

        // Debounced layout save: only after a committed dock change (the drag/
        // resize/toggle handlers set `dirty`), and only the primary editor.
        if self.persist && self.dock.dirty {
            self.save_layout(system);
        }
    }

    /// Advance (or start) a panel drag — splitter resize, float move/tear-off, or
    /// float resize. Returns `true` while a drag is active so the caller
    /// suppresses panel/canvas input. Mutations re-solve immediately, so the draw
    /// pass shows the panel under the cursor this frame (no one-frame lag).
    fn step_drag(&mut self, mouse: &MouseInput, screen: (f32, f32)) -> bool {
        let p = mouse.pos();
        let up = released(mouse.left);
        match self.dock.drag {
            DragState::Idle => {
                if just_pressed(mouse.left) {
                    // A float's SE handle wins over the splitter beneath it.
                    if let Some(idx) = self.dock.float_handle_at(p) {
                        let anchor = self.dock.solved.rect_of(idx).unwrap_or_default();
                        self.dock.raise(idx);
                        self.dock.drag = DragState::ResizeFloat { idx, anchor };
                        return true;
                    }
                    if let Some(side) = self.dock.splitter_at(p) {
                        self.dock.drag = DragState::ResizeDock { side };
                        return true;
                    }
                }
                false
            }
            DragState::ResizeDock { side } => {
                if up {
                    self.dock.drag = DragState::Idle;
                    self.dock.dirty = true;
                } else {
                    let (sw, sh) = (screen.0 as i16, screen.1 as i16);
                    let thick = match side {
                        Side::Left => p.x,
                        Side::Right => sw - p.x,
                        Side::Top => p.y,
                        Side::Bottom => sh - p.y,
                    };
                    self.dock.set_side_thickness(side, thick);
                    self.dock.recompute(screen);
                }
                true
            }
            DragState::ResizeFloat { idx, anchor } => {
                if up {
                    self.dock.drag = DragState::Idle;
                    self.dock.dirty = true;
                } else {
                    self.dock.resize_float(idx, anchor, p);
                    self.dock.recompute(screen);
                }
                true
            }
            DragState::MovePanel { idx, grab_dx, grab_dy, arming } => {
                if up {
                    // Drop: snap to the edge under the cursor (computed fresh —
                    // `recompute` clears `solved.hot_edge` at the top of step),
                    // else stay where it floats.
                    if let Some(side) = self.dock.edge_near(p, screen) {
                        self.dock.dock_panel(idx, side);
                    }
                    self.dock.drag = DragState::Idle;
                    self.dock.dirty = true;
                    return true;
                }
                if arming {
                    // Tear off only once dragged past the threshold; the panel is
                    // still docked, so its origin is stable for measuring drag.
                    let rect = self.dock.solved.rect_of(idx).unwrap_or_default();
                    let press = Vec2::new(rect.x + grab_dx, rect.y + grab_dy);
                    if (p.x - press.x).abs() + (p.y - press.y).abs() > dock::TEAR_THRESHOLD {
                        self.dock
                            .set_float(idx, Vec2::new(p.x - grab_dx, p.y - grab_dy), rect.w, rect.h);
                        self.dock.drag = DragState::MovePanel { idx, grab_dx, grab_dy, arming: false };
                        self.dock.recompute(screen);
                    }
                    return true;
                }
                // Following the cursor: move, flag the drop edge, then re-solve so
                // draw places it under the cursor and shows the drop highlight.
                self.dock.move_float(idx, Vec2::new(p.x - grab_dx, p.y - grab_dy));
                self.dock.recompute(screen);
                self.dock.solved.hot_edge = self.dock.edge_near(p, screen);
                true
            }
        }
    }

    /// Pick the always-on global bar (pinned to the world's top-left), if the
    /// cursor is over one of its buttons.
    fn global_bar_hit(&self, cursor: Vec2) -> Option<EditorKey> {
        let world = self.dock.solved.world;
        self.build_global_bar()
            .hit_at(world.x + 1, world.y + 1, cursor)
    }

    /// Make panel `idx` the active one: switch the canvas tool to match its kind
    /// (so its content + the world interaction line up) and raise it to the
    /// front. A no-op `idx` (the global bar's `usize::MAX`) just returns.
    fn activate_panel(&mut self, idx: usize) {
        let Some(panel) = self.dock.panels.get(idx) else {
            return;
        };
        if let Some(tool) = Self::panel_tool(panel.kind, self.tool)
            && tool != self.tool
        {
            self.switch_tool(tool);
        }
        self.dock.raise(idx);
    }

    /// Global editor keyboard shortcuts (only while no text field is focused):
    /// Ctrl+Z undo, Ctrl+Y / Ctrl+Shift+Z redo, Ctrl+S save, the Select tool's
    /// Ctrl+C/X/V clipboard ops, Delete removes the selected object (or clears the
    /// Select marquee), Escape drops the marquee, and `1`–`5` switch tools. These
    /// keys are forwarded to the console by the host's editor-key gate (see `main.rs`).
    fn handle_shortcuts(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        maps: &mut MapStore,
    ) {
        let ctrl = system.key(ScanCode::Ctrl);
        let shift = system.key(ScanCode::Shift);
        if ctrl {
            if system.keyp(ScanCode::Z) {
                if shift {
                    self.redo(map, maps);
                } else {
                    self.undo(map, maps);
                }
            }
            if system.keyp(ScanCode::Y) {
                self.redo(map, maps);
            }
            if system.keyp(ScanCode::S) {
                self.save(system, map, maps);
            }
            // Select-tool clipboard ops (Ctrl+C/X/V) on the active layer.
            if self.tool == EditorTool::Select {
                if system.keyp(ScanCode::C) {
                    self.selection_copy(maps, map);
                }
                if system.keyp(ScanCode::X) {
                    self.selection_cut(maps, map);
                }
                if system.keyp(ScanCode::V) {
                    self.selection_paste(maps, map);
                }
            }
            // Ctrl-chorded: don't also treat the digit as a tool switch.
            return;
        }

        // Delete: removes the selected object, or clears the Select marquee.
        if system.keyp(ScanCode::Delete) {
            if matches!(self.tool, EditorTool::Interactables | EditorTool::Warps) {
                self.delete_object(map);
            } else if self.tool == EditorTool::Select {
                self.selection_delete(maps, map);
            }
        }
        // Escape drops the Select marquee.
        if system.keyp(ScanCode::Escape) && self.tool == EditorTool::Select {
            self.selection = None;
        }
        // G toggles the tile-grid + coordinate overlay.
        if system.keyp(ScanCode::G) {
            self.show_grid = !self.show_grid;
        }
        // Arrow keys nudge the selected object's hitbox (8px with Shift), each
        // press one undo step — the keyboard companion to the x/y/w/h fields.
        if matches!(self.tool, EditorTool::Interactables | EditorTool::Warps) {
            let step = if shift { 8 } else { 1 };
            let (mut dx, mut dy) = (0i16, 0i16);
            if system.keyp(ScanCode::Left) { dx -= step; }
            if system.keyp(ScanCode::Right) { dx += step; }
            if system.keyp(ScanCode::Up) { dy -= step; }
            if system.keyp(ScanCode::Down) { dy += step; }
            if (dx != 0 || dy != 0) && self.selected.is_some() {
                self.modify_object(map, |map, i| {
                    if let Some(o) = map.objects.get_mut(i) {
                        o.hitbox.x += dx;
                        o.hitbox.y += dy;
                    }
                });
            }
        }

        // Number-row tool switching, mirroring the tab order (5 = Select, the
        // Paint panel's sub-mode).
        let tool = if system.keyp(ScanCode::Digit1) {
            Some(EditorTool::Layers)
        } else if system.keyp(ScanCode::Digit2) {
            Some(EditorTool::Paint)
        } else if system.keyp(ScanCode::Digit3) {
            Some(EditorTool::Interactables)
        } else if system.keyp(ScanCode::Digit4) {
            Some(EditorTool::Warps)
        } else if system.keyp(ScanCode::Digit5) {
            Some(EditorTool::Select)
        } else {
            None
        };
        if let Some(tool) = tool {
            self.switch_tool(tool);
        }
    }

    /// Switch the active tool, clearing any per-tool transient state (selection,
    /// in-progress drag/stroke) so it can't leak across tools.
    fn switch_tool(&mut self, tool: EditorTool) {
        self.tool = tool;
        self.selected = None;
        self.stop_editing();
        self.drag = None;
        self.stroke = None;
        self.moving = None;
        self.move_from = None;
    }

    fn handle_panel(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        maps: &mut MapStore,
        idx: usize,
        key: EditorKey,
        camera_pos: Vec2,
    ) {
        let mouse = system.mouse();
        let click = just_pressed(mouse.left);
        // Clicking anywhere in a panel makes it the active canvas tool (the
        // global bar passes `usize::MAX`, which `activate_panel` ignores).
        if click {
            self.activate_panel(idx);
        }
        match key {
            // Title-bar press begins a move: a float follows the cursor at once;
            // a docked panel arms a tear-off (a still click just focuses, via
            // `activate_panel` above).
            EditorKey::Dock(pidx, Chrome::TitleBar) => {
                if click {
                    let rect = self.dock.solved.rect_of(pidx).unwrap_or_default();
                    let cur = mouse.pos();
                    self.dock.drag = DragState::MovePanel {
                        idx: pidx,
                        grab_dx: cur.x - rect.x,
                        grab_dy: cur.y - rect.y,
                        arming: matches!(self.dock.panels[pidx].place, Placement::Dock { .. }),
                    };
                }
            }
            // Resize handles are picked geometrically (see `step_drag`).
            EditorKey::Dock(..) => {}
            EditorKey::TogglePanel(kind) => {
                if click {
                    self.dock.toggle_panel(kind);
                }
            }
            EditorKey::Tool(tool) => {
                if click {
                    self.switch_tool(tool);
                }
            }
            EditorKey::Title => {
                if click {
                    self.fg = !self.fg;
                    self.layer_index = 0;
                }
            }
            // Sticky select: a click sets the active layer (and stays — no
            // hover-select, and the canvas tool isn't changed; see `panel_tool`).
            EditorKey::Layer(i) => {
                if click {
                    self.layer_index = i;
                }
            }
            // The eye toggles that layer's visibility (and selects it).
            EditorKey::LayerVis(i) => {
                if click {
                    self.layer_index = i;
                    self.toggle_layer(map);
                }
            }
            EditorKey::LayerAdd => {
                if click
                    && let Some(tm) = maps.get_mut(&map.source)
                {
                    let name = format!("Layer {}", tm.layers.len());
                    let index = tm.add_tile_layer(&name);
                    let layer = Box::new(tm.layers[index].clone());
                    self.record(EditAction::LayerInsert { source: map.source.clone(), index, layer });
                    self.pending_reload = true;
                }
            }
            EditorKey::LayerDel => {
                if click
                    && let Some(src) = self.selected_source_layer(map)
                    && let Some(layer) = maps.get_mut(&map.source).and_then(|tm| tm.remove_layer_at(src))
                {
                    self.record(EditAction::LayerRemove {
                        source: map.source.clone(),
                        index: src,
                        layer: Box::new(layer),
                    });
                    self.layer_index = self.layer_index.saturating_sub(1);
                    self.pending_reload = true;
                }
            }
            EditorKey::LayerUp => {
                if click
                    && let Some(src) = self.selected_source_layer(map)
                    && let Some((a, b)) = maps.get_mut(&map.source).and_then(|tm| tm.move_layer(src, true))
                {
                    self.record(EditAction::LayerSwap { source: map.source.clone(), a, b });
                    self.layer_index = self.layer_index.saturating_sub(1);
                    self.pending_reload = true;
                }
            }
            EditorKey::LayerDown => {
                if click
                    && let Some(src) = self.selected_source_layer(map)
                    && let Some((a, b)) = maps.get_mut(&map.source).and_then(|tm| tm.move_layer(src, false))
                {
                    self.record(EditAction::LayerSwap { source: map.source.clone(), a, b });
                    // Follow the moved layer, clamped to the (unchanged-length) list.
                    self.layer_index = (self.layer_index + 1).min(self.layer_list_len(map).saturating_sub(1));
                    self.pending_reload = true;
                }
            }
            EditorKey::LayerDup => {
                if click
                    && let Some(src) = self.selected_source_layer(map)
                    && map.layers.first().map(|l| l.source_layer) != Some(src)
                    && let Some(tm) = maps.get_mut(&map.source)
                {
                    let index = src + 1;
                    let dup = tm.layers[src].clone();
                    tm.insert_layer(index, dup);
                    let layer = Box::new(tm.layers[index].clone());
                    self.record(EditAction::LayerInsert { source: map.source.clone(), index, layer });
                    self.pending_reload = true;
                }
            }
            EditorKey::LayerRename => {
                if click
                    && let Some(src) = self.selected_source_layer(map)
                    && map.layers.first().map(|l| l.source_layer) != Some(src)
                {
                    self.begin_layer_rename(maps, &map.source, src);
                }
            }
            EditorKey::LayerFg => {
                if click
                    && let Some(src) = self.selected_source_layer(map)
                    && map.layers.first().map(|l| l.source_layer) != Some(src)
                {
                    self.toggle_layer_fg(map, maps, src);
                }
            }
            // Palette: wheel scrolls; a press starts a scroll-bar drag (edge grab
            // zones) or a content pan / tile-pick (`step_palette_drag`).
            EditorKey::PaletteView => {
                if mouse.scroll_y[0] != 0 || mouse.scroll_x[0] != 0 {
                    self.palette_wheel(mouse.scroll_x[0], mouse.scroll_y[0]);
                }
                if just_pressed(mouse.left) {
                    let p = mouse.pos();
                    let v = self.pal_rect;
                    let (max_c, max_r) = self.palette_scroll_max();
                    if max_r > 0 && p.x >= v.x + v.w - PALETTE_BAR_GRAB {
                        // Grab offset within the thumb (clamped to its height, so
                        // a click off the thumb snaps the near edge under the cursor).
                        let (th, travel) = self.palette_thumb_v();
                        let thumb_top = i32::from(v.y) + travel * self.pal_row as i32 / max_r as i32;
                        let grab = (i32::from(p.y) - thumb_top).clamp(0, th) as i16;
                        self.pal_drag = Some(PalDrag::ScrollV { grab });
                        self.scroll_palette_bar(true, p, grab);
                    } else if max_c > 0 && p.y >= v.y + v.h - PALETTE_BAR_GRAB {
                        let (tw, travel) = self.palette_thumb_h();
                        let thumb_left = i32::from(v.x) + travel * self.pal_col as i32 / max_c as i32;
                        let grab = (i32::from(p.x) - thumb_left).clamp(0, tw) as i16;
                        self.pal_drag = Some(PalDrag::ScrollH { grab });
                        self.scroll_palette_bar(false, p, grab);
                    } else {
                        // Start a brush box-select (a click stays 1×1).
                        let (c, r) = self.palette_tile_at(p);
                        self.set_brush_box(c, r, c, r);
                        self.pal_drag = Some(PalDrag::Select { anchor_col: c, anchor_row: r });
                    }
                }
            }
            EditorKey::Object(i) => {
                if click {
                    self.selected = Some(i);
                    self.stop_editing();
                }
            }
            EditorKey::NewObject => {
                if click {
                    self.new_object(
                        map,
                        camera_pos,
                        system.width() as i16,
                        system.height() as i16,
                    );
                }
            }
            EditorKey::DupObject => {
                if click {
                    self.duplicate_object(map);
                }
            }
            EditorKey::DeleteObject => {
                if click {
                    self.delete_object(map);
                }
            }
            // A click in the warp destination preview sets the landing point to
            // the clicked map pixel (clamped to the target's bounds).
            EditorKey::WarpPreview => {
                if click
                    && let Some(rect) = self.dock.solved.rect_of(idx)
                {
                    let ui = self.build_panel(idx, rect, map, maps);
                    let (scroll, _) = self.panel_scroll(idx, rect, ui.content_height());
                    if let Some(box_rect) =
                        ui.rect_at(rect.x, rect.y - scroll, EditorKey::WarpPreview)
                    {
                        self.place_warp_from_preview(map, maps, box_rect, mouse.pos());
                    }
                }
            }
            // Press on a panel's scroll bar: begin a thumb drag, capturing the grab
            // offset so the thumb tracks the cursor rather than snapping under it.
            EditorKey::PanelScroll(pidx) => {
                if click
                    && let Some(rect) = self.dock.solved.rect_of(pidx)
                {
                    let content_h = self.build_panel(pidx, rect, map, maps).content_height();
                    let body = Self::panel_body(rect);
                    let (scroll, _) = self.panel_scroll(pidx, rect, content_h);
                    let (top, th) = Self::scroll_thumb(body, scroll, content_h);
                    let grab = (mouse.pos().y - top).clamp(0, th);
                    self.scroll_drag = Some((pidx, grab));
                }
            }
            EditorKey::Field(field) => {
                if click {
                    // Layer offset/rotation fields read from the store and target
                    // the selected layer; object fields read from the object.
                    if matches!(
                        field,
                        EditField::LayerOffX | EditField::LayerOffY | EditField::LayerRotate
                    ) {
                        if let Some(src) = self.selected_source_layer(map) {
                            self.begin_layer_field(maps, &map.source, src, field);
                        }
                    } else {
                        self.begin_edit(field, map);
                    }
                }
            }
            EditorKey::Cycle(field) => {
                if click {
                    // WarpTarget steps through the map store, so it needs `maps`;
                    // the rest only touch the object.
                    if field == CycleField::WarpTarget {
                        self.cycle_warp_target(map, maps);
                    } else {
                        self.cycle(map, field);
                    }
                }
            }
            EditorKey::Eraser => {
                if click {
                    self.selected_tile = 0;
                    self.brush_w = 1;
                    self.brush_h = 1;
                }
            }
            EditorKey::SelCopy => {
                if click {
                    self.selection_copy(maps, map);
                }
            }
            EditorKey::SelCut => {
                if click {
                    self.selection_cut(maps, map);
                }
            }
            EditorKey::SelPaste => {
                if click {
                    self.selection_paste(maps, map);
                }
            }
            EditorKey::SelDelete => {
                if click {
                    self.selection_delete(maps, map);
                }
            }
            EditorKey::SelClear => {
                if click {
                    self.selection = None;
                }
            }
            EditorKey::Undo => {
                if click {
                    self.undo(map, maps);
                }
            }
            EditorKey::Redo => {
                if click {
                    self.redo(map, maps);
                }
            }
            EditorKey::Save => {
                if click {
                    self.save(system, map, maps);
                }
            }
            // Maps browser: first click selects, a click on the already-selected
            // map opens it (the host drains `pending_open` to load it).
            EditorKey::MapSlot(i) => {
                if click {
                    let name = self.modern_names(maps).get(i).cloned();
                    if let Some(name) = name {
                        if self.maps_selected.as_deref() == Some(name.as_str()) {
                            self.pending_open = Some(name);
                        } else {
                            self.maps_selected = Some(name);
                        }
                    }
                }
            }
            EditorKey::MapPrev => {
                if click {
                    self.maps_scroll = self.maps_scroll.saturating_sub(1);
                }
            }
            EditorKey::MapNext => {
                if click {
                    self.maps_scroll += 1; // clamped against the page count in build_maps
                }
            }
            EditorKey::MapNew => {
                if click {
                    self.maps_dialog = MapsDialog::New {
                        name: TextField::new(""),
                        w: TextField::new(NEW_MAP_W.to_string()),
                        h: TextField::new(NEW_MAP_H.to_string()),
                        focus: 0,
                    };
                }
            }
            EditorKey::MapDup => {
                if click
                    && let Some(sel) = self.maps_selected.clone()
                {
                    self.duplicate_map(system, maps, &sel);
                }
            }
            EditorKey::MapRename => {
                if click
                    && let Some(sel) = self.maps_selected.clone()
                {
                    self.maps_dialog = MapsDialog::Rename {
                        from: sel.clone(),
                        name: TextField::new(sel),
                    };
                }
            }
            EditorKey::MapDelete => {
                if click
                    && let Some(sel) = self.maps_selected.clone()
                {
                    self.maps_dialog = MapsDialog::ConfirmDelete(sel);
                }
            }
            // Setup panel. These map-level settings aren't on the undo stack —
            // they just mutate the stored map and ask for a re-derive.
            EditorKey::BgColour(c) => {
                if click {
                    if let Some(tm) = maps.get_mut(&map.source) {
                        tm.set_bg_colour(c);
                    }
                    self.status.edited();
                    self.pending_reload = true;
                }
            }
            EditorKey::CamAuto => {
                if click {
                    if let Some(tm) = maps.get_mut(&map.source) {
                        tm.set_camera_stick(None);
                    }
                    self.status.edited();
                    self.pending_reload = true;
                }
            }
            EditorKey::CamPin => {
                if click {
                    // `camera_stick` is consumed as the camera's top-left (see
                    // `CameraBounds::stick`) — exactly `camera_pos` — so pinning
                    // reproduces the framing the editor is showing, in any view.
                    if let Some(tm) = maps.get_mut(&map.source) {
                        tm.set_camera_stick(Some((camera_pos.x, camera_pos.y)));
                    }
                    self.status.edited();
                    self.pending_reload = true;
                }
            }
            EditorKey::MapResize => {
                if click {
                    let (w, h) = maps
                        .get(&map.source)
                        .map(|t| (t.width, t.height))
                        .unwrap_or((NEW_MAP_W, NEW_MAP_H));
                    self.maps_dialog = MapsDialog::Resize {
                        source: map.source.clone(),
                        w: TextField::new(w.to_string()),
                        h: TextField::new(h.to_string()),
                        focus: 0,
                    };
                }
            }
            EditorKey::MusicCycle => {
                if click {
                    // The available tracks are discovered from the music dir by
                    // the host (engine-agnostic); the map stores the chosen name.
                    let tracks = system.music_tracks();
                    self.cycle_music(map, maps, &tracks);
                }
            }
            EditorKey::MusicSpeedCycle => {
                if click {
                    self.cycle_music_speed(map, maps);
                }
            }
        }
    }

    /// Step the map's `music` property through `[none] + tracks`, by name — the
    /// same string-indexed model as a warp's `to_map`. `tracks` are the music
    /// directory's file stems (from [`ConsoleApi::music_tracks`]). Stored on the
    /// map (saved + resolved at load); not on the undo stack, like the panel's
    /// other map settings, and it takes effect on the next map load.
    fn cycle_music(&mut self, map: &MapInfo, maps: &mut MapStore, tracks: &[String]) {
        let Some(tm) = maps.get_mut(&map.source) else { return };
        let current = match tm.music() {
            None => 0,
            Some(c) => tracks.iter().position(|n| n == c).map_or(0, |i| i + 1),
        };
        let next = (current + 1) % (tracks.len() + 1);
        tm.set_music((next > 0).then(|| tracks[next - 1].as_str()));
        self.status.edited();
    }

    /// Step the map's `music_speed` through [`MUSIC_SPEEDS`], wrapping. Stored on
    /// the map and applied on the next load, like the track itself.
    fn cycle_music_speed(&mut self, map: &MapInfo, maps: &mut MapStore) {
        let Some(tm) = maps.get_mut(&map.source) else { return };
        let speed = tm.music_speed();
        // Snap to the nearest preset, then advance one (wrapping).
        let current = MUSIC_SPEEDS
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (*a - speed).abs().total_cmp(&(*b - speed).abs())
            })
            .map_or(0, |(i, _)| i);
        let next = (current + 1) % MUSIC_SPEEDS.len();
        tm.set_music_speed(MUSIC_SPEEDS[next]);
        self.status.edited();
    }

    fn handle_canvas(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        maps: &mut MapStore,
        camera_pos: Vec2,
        mouse: &MouseInput,
    ) {
        match self.tool {
            EditorTool::Paint => self.handle_paint(system, map, maps, camera_pos, mouse),
            EditorTool::Select => self.handle_select(camera_pos, mouse),
            EditorTool::Interactables | EditorTool::Warps => {
                let world = Vec2::new(mouse.pos().x + camera_pos.x, mouse.pos().y + camera_pos.y);
                if just_pressed(mouse.left) {
                    if let Some(i) = self.object_at(map, world) {
                        // Grab the object under the cursor to drag it around. Note
                        // the start origin so a completed drag records one undo
                        // step (start → drop), not a step per moved frame.
                        self.selected = Some(i);
                        self.stop_editing();
                        self.drag = None;
                        self.moving = Some(world - self.object_origin(map, i));
                        self.move_from = Some(self.object_origin(map, i));
                    } else {
                        // Empty space: drag out a box for a new object.
                        self.drag = Some(world);
                        self.moving = None;
                    }
                }
                // Drag the grabbed object's hitbox to follow the cursor.
                if pressed(mouse.left)
                    && let (Some(i), Some(offset)) = (self.selected, self.moving)
                {
                    self.set_object_origin(map, i, world - offset);
                }
                if released(mouse.left) {
                    self.finish_move(map);
                    if let Some(start) = self.drag.take() {
                        let hitbox = hitbox_between(start, world);
                        if hitbox.w >= 4 && hitbox.h >= 4 {
                            self.create_object(map, hitbox);
                        }
                    }
                }
            }
            EditorTool::Layers => {}
        }
    }

    /// Paint tool input: drag-paint with the brush, erase with right-click, pick
    /// the tile under the cursor with middle-click (eyedropper), or hold Shift to
    /// drag out a filled rectangle. All of a press-drag-release is one undo step.
    fn handle_paint(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        maps: &mut MapStore,
        camera_pos: Vec2,
        mouse: &MouseInput,
    ) {
        // Paint only targets tile layers — an image layer carries a bitmap, not
        // editable tile cells, so it's never a paint target (and `TiledMap::set`
        // would no-op on it anyway). The Layers list flags those as `img`.
        let Some((source, layer)) = self
            .active_layer(map)
            .filter(|l| l.kind == LayerKind::Tiles)
            .map(|l| (map.source.clone(), l.source_layer))
        else {
            return;
        };
        // The first tile layer is the collision layer; painting it changes the
        // derived colliders, so flag a re-derive (see the host drain) — collision
        // then takes effect immediately, without a map reload.
        let is_collision = map.layers.first().map(|l| l.source_layer) == Some(layer);
        let (tx, ty) = world_tile(mouse, camera_pos);

        // Middle-click eyedropper: lift the existing tile into a 1×1 brush.
        if just_pressed(mouse.middle) && tx >= 0 && ty >= 0 {
            self.selected_tile = maps
                .get(&source)
                .and_then(|m| m.get(layer, tx as usize, ty as usize))
                .unwrap_or(0);
            self.brush_w = 1;
            self.brush_h = 1;
            return;
        }

        let rect_mode = system.key(ScanCode::Shift);
        if rect_mode {
            // Shift held: drag a rectangle in world space, fill it on release.
            let world = Vec2::new(mouse.pos().x + camera_pos.x, mouse.pos().y + camera_pos.y);
            if just_pressed(mouse.left) {
                self.drag = Some(world);
            }
            if released(mouse.left)
                && let Some(start) = self.drag.take()
            {
                self.fill_rect(maps, &source, layer, start, world);
                if is_collision {
                    self.pending_reload = true;
                }
            }
            return;
        }

        // Freehand mode: drop any rectangle-drag start left over from releasing
        // Shift mid-drag (so its preview/fill can't bleed into a freehand stroke).
        if !pressed(mouse.left) {
            self.drag = None;
        }

        // Freehand: paint (or erase with right-click) each cell the cursor passes
        // over, batching the whole stroke into `self.stroke` until release.
        if pressed(mouse.left) || pressed(mouse.right) {
            if self.stroke.is_none() {
                self.stroke = Some(EditAction::Tiles {
                    source: source.clone(),
                    layer,
                    cells: Vec::new(),
                });
            }
            if tx >= 0 && ty >= 0 {
                self.paint_brush(maps, &source, layer, tx, ty, pressed(mouse.right));
                if is_collision {
                    self.pending_reload = true;
                }
            }
        }
        if released(mouse.left) || released(mouse.right) {
            self.flush_stroke();
        }
    }

    /// Stamp the brush (its `brush_w`×`brush_h` tile block) at world tile
    /// `(tx, ty)`, or erase that footprint to the empty tile. Each cell records
    /// into the in-progress stroke, so the whole stamp undoes together.
    fn paint_brush(
        &mut self,
        maps: &mut MapStore,
        source: &str,
        layer: usize,
        tx: i32,
        ty: i32,
        erase: bool,
    ) {
        let (bw, bh) = self.brush_size();
        let (bc, br) = (self.selected_tile % self.sheet_cols(), self.selected_tile / self.sheet_cols());
        for dy in 0..bh {
            for dx in 0..bw {
                if bc + dx >= self.sheet_cols() {
                    continue; // don't wrap past the sheet's right edge
                }
                let value = if erase { 0 } else { (br + dy) * self.sheet_cols() + (bc + dx) };
                self.paint_cell(maps, source, layer, tx + dx as i32, ty + dy as i32, value);
            }
        }
    }

    /// Set one tile, recording its `(old, new)` into the in-progress stroke.
    /// Skips no-op writes so an undo step only holds cells that actually changed.
    fn paint_cell(
        &mut self,
        maps: &mut MapStore,
        source: &str,
        layer: usize,
        tx: i32,
        ty: i32,
        value: usize,
    ) {
        let Some(tiles) = maps.get_mut(source) else {
            return;
        };
        // `None` ⇒ off the layer (out of bounds / not a tile layer): skip, so a
        // drag past the edge can't paint (or record a phantom undo cell).
        let Some(old) = tiles.get(layer, tx as usize, ty as usize) else {
            return;
        };
        if old == value {
            return;
        }
        tiles.set(layer, tx as usize, ty as usize, value);
        if let Some(EditAction::Tiles { cells, .. }) = &mut self.stroke {
            cells.push((tx, ty, old, value));
        }
    }

    /// Flush the in-progress paint stroke into history, if it changed anything.
    fn flush_stroke(&mut self) {
        if let Some(EditAction::Tiles { cells, .. }) = &self.stroke
            && cells.is_empty()
        {
            self.stroke = None;
            return;
        }
        if let Some(action) = self.stroke.take() {
            self.record(action);
        }
    }

    /// Fill the tile rectangle between two world points, tiling the brush across
    /// it, as a single undo step.
    fn fill_rect(&mut self, maps: &mut MapStore, source: &str, layer: usize, a: Vec2, b: Vec2) {
        let (x0, y0, x1, y1) = tile_bounds(a, b);
        let (bw, bh) = self.brush_size();
        let (bc, br) = (self.selected_tile % self.sheet_cols(), self.selected_tile / self.sheet_cols());
        self.stroke = Some(EditAction::Tiles {
            source: source.to_string(),
            layer,
            cells: Vec::new(),
        });
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                if tx < 0 || ty < 0 {
                    continue;
                }
                // Repeat the brush pattern across the fill region.
                let ox = (tx - x0) as usize % bw;
                let oy = (ty - y0) as usize % bh;
                let value = if bc + ox < self.sheet_cols() {
                    (br + oy) * self.sheet_cols() + (bc + ox)
                } else {
                    0
                };
                self.paint_cell(maps, source, layer, tx, ty, value);
            }
        }
        self.flush_stroke();
    }

    /// Select-tool input: drag the left button to rubber-band a tile marquee on
    /// the active layer (a click is a 1×1), right-click to clear it. The clipboard
    /// ops fire from the panel buttons / shortcuts, not the canvas.
    fn handle_select(&mut self, camera_pos: Vec2, mouse: &MouseInput) {
        let world = Vec2::new(mouse.pos().x + camera_pos.x, mouse.pos().y + camera_pos.y);
        if just_pressed(mouse.left) {
            self.drag = Some(world);
        }
        if pressed(mouse.left) {
            // Grow the marquee live so the panel's `WxH` readout tracks the drag.
            if let Some(start) = self.drag {
                self.selection = Some(selection_between(start, world));
            }
        } else {
            // Not held: drop any drag start stranded by releasing over a panel.
            self.drag = None;
        }
        if just_pressed(mouse.right) {
            self.selection = None;
            self.drag = None;
        }
    }

    /// The active tile layer for a Select op: `(source, source_layer,
    /// is_collision)`, or `None` if the active layer isn't an editable tile
    /// layer (an image layer carries a bitmap, not cells). Mirrors the
    /// [`handle_paint`](Self::handle_paint) target guard.
    fn selection_layer(&self, map: &MapInfo) -> Option<(String, usize, bool)> {
        let (source, layer) = self
            .active_layer(map)
            .filter(|l| l.kind == LayerKind::Tiles)
            .map(|l| (map.source.clone(), l.source_layer))?;
        let is_collision = map.layers.first().map(|l| l.source_layer) == Some(layer);
        Some((source, layer, is_collision))
    }

    /// Copy the active layer's tiles under the marquee into the clipboard (cells
    /// off the layer read as empty). Non-destructive — no undo entry.
    fn selection_copy(&mut self, maps: &MapStore, map: &MapInfo) {
        let (Some(sel), Some((source, layer, _))) = (self.selection, self.selection_layer(map))
        else {
            return;
        };
        let Some(tiles) = maps.get(&source) else { return };
        let mut buf = Vec::with_capacity(sel.w * sel.h);
        for dy in 0..sel.h {
            for dx in 0..sel.w {
                let (tx, ty) = (sel.x + dx as i32, sel.y + dy as i32);
                let id = if tx < 0 || ty < 0 {
                    0
                } else {
                    tiles.get(layer, tx as usize, ty as usize).unwrap_or(0)
                };
                buf.push(id);
            }
        }
        self.clipboard = Some(Clipboard { w: sel.w, h: sel.h, tiles: buf });
    }

    /// Copy the marquee, then clear it to empty — a single undo step.
    fn selection_cut(&mut self, maps: &mut MapStore, map: &MapInfo) {
        self.selection_copy(maps, map);
        self.selection_delete(maps, map);
    }

    /// Clear every cell under the marquee to the empty tile, as one undo step.
    fn selection_delete(&mut self, maps: &mut MapStore, map: &MapInfo) {
        let (Some(sel), Some((source, layer, is_collision))) =
            (self.selection, self.selection_layer(map))
        else {
            return;
        };
        self.stroke = Some(EditAction::Tiles {
            source: source.clone(),
            layer,
            cells: Vec::new(),
        });
        for dy in 0..sel.h {
            for dx in 0..sel.w {
                self.paint_cell(maps, &source, layer, sel.x + dx as i32, sel.y + dy as i32, 0);
            }
        }
        self.flush_stroke();
        if is_collision {
            self.pending_reload = true;
        }
    }

    /// Stamp the clipboard with its top-left at the marquee's origin, as one undo
    /// step (cells off the layer are skipped). Click to drop a 1×1 marquee where
    /// you want the paste to land.
    fn selection_paste(&mut self, maps: &mut MapStore, map: &MapInfo) {
        let (Some(sel), Some(clip)) = (self.selection, self.clipboard.clone()) else {
            return;
        };
        let Some((source, layer, is_collision)) = self.selection_layer(map) else {
            return;
        };
        self.stroke = Some(EditAction::Tiles {
            source: source.clone(),
            layer,
            cells: Vec::new(),
        });
        for dy in 0..clip.h {
            for dx in 0..clip.w {
                let value = clip.tiles[dy * clip.w + dx];
                self.paint_cell(maps, &source, layer, sel.x + dx as i32, sel.y + dy as i32, value);
            }
        }
        self.flush_stroke();
        if is_collision {
            self.pending_reload = true;
        }
    }

    /// Settle a finished object drag: if the origin actually changed, record a
    /// single move as one undo step.
    fn finish_move(&mut self, map: &mut MapInfo) {
        let (Some(i), Some(from)) = (self.selected, self.move_from.take()) else {
            self.moving = None;
            return;
        };
        self.moving = None;
        let to = self.object_origin(map, i);
        if to != from
            && let Some(after) = Self::snapshot(map, i)
        {
            // Rebuild the "before" snapshot by re-deriving it from `after`'s
            // contents with the original origin restored.
            let before = move_snapshot(after.clone(), from);
            self.record(EditAction::Modify {
                index: i,
                before,
                after,
            });
        }
    }

    fn new_object(&mut self, map: &mut MapInfo, camera_pos: Vec2, w: i16, h: i16) {
        let x = camera_pos.x + w / 2;
        let y = camera_pos.y + h / 2;
        self.create_object(map, Hitbox::new(x, y, 16, 16));
    }

    fn create_object(&mut self, map: &mut MapInfo, hitbox: Hitbox) {
        // The active tab decides the kind; both append to the one objects list.
        let object = if self.obj_kind() == ObjKind::Warp {
            let to = Vec2::new(hitbox.x, hitbox.y);
            MapObject::warp(hitbox, Warp::new(None, to))
        } else {
            MapObject::dialogue(hitbox, "new_key")
        };
        map.objects.push(object);
        let index = map.objects.len() - 1;
        self.selected = Some(index);
        self.stop_editing();
        if let Some(after) = Self::snapshot(map, index) {
            self.record(EditAction::Add { index, after });
        }
    }

    fn delete_object(&mut self, map: &mut MapInfo) {
        let Some(i) = self.selected else { return };
        let before = Self::snapshot(map, i);
        remove_object(map, i);
        self.selected = None;
        self.stop_editing();
        if let Some(before) = before {
            self.record(EditAction::Remove { index: i, before });
        }
    }

    /// Duplicate the selected object, nudged a tile down-right so the copy is
    /// visible, select it, and record the append as one undo step.
    fn duplicate_object(&mut self, map: &mut MapInfo) {
        let Some(i) = self.selected else { return };
        let Some(mut copy) = map.objects.get(i).cloned() else {
            return;
        };
        copy.hitbox.x += 8;
        copy.hitbox.y += 8;
        map.objects.push(copy);
        let index = map.objects.len() - 1;
        self.selected = Some(index);
        self.stop_editing();
        if let Some(after) = Self::snapshot(map, index) {
            self.record(EditAction::Add { index, after });
        }
    }

    /// Clear text-entry focus: forget which field was being edited and drop its
    /// buffer. `editing` and `field` are always set/cleared together so
    /// [`is_typing`](Self::is_typing) stays in step with the live buffer.
    fn stop_editing(&mut self) {
        self.editing = None;
        self.field = None;
        self.rename_target = None;
    }

    /// Forget all per-map editor state: undo/redo history, text-entry focus,
    /// object selection, and any in-progress drag/stroke. Deliberately keeps
    /// [`SaveStatus`]: tile paints land in the shared [`MapStore`], so
    /// unsaved-ness genuinely survives a map switch. (Tile undo entries are
    /// source-tagged and would replay correctly across maps, but object entries
    /// index into the replaced objects list — so the whole history goes.)
    fn reset_for_new_map(&mut self) {
        self.history.clear();
        self.stop_editing();
        self.selected = None;
        self.drag = None;
        self.stroke = None;
        self.moving = None;
        self.move_from = None;
        // The marquee indexes the old map's tiles; drop it. (The clipboard
        // survives, so a block can be pasted into a different map.)
        self.selection = None;
    }

    fn begin_edit(&mut self, field: EditField, map: &MapInfo) {
        let object = self.selected.and_then(|i| map.objects.get(i));
        let effect = object.map(|o| &o.effect);
        let value = match (effect, field) {
            (Some(ObjectEffect::Interact(Interaction::Dialogue(k))), EditField::Key) => k.clone(),
            (Some(ObjectEffect::Warp(w)), EditField::ToMap) => w.map.clone().unwrap_or_default(),
            (Some(ObjectEffect::Warp(w)), EditField::ToX) => w.to.x.to_string(),
            (Some(ObjectEffect::Warp(w)), EditField::ToY) => w.to.y.to_string(),
            (Some(ObjectEffect::Warp(w)), EditField::Narration) => {
                w.narration.clone().unwrap_or_default()
            }
            (Some(ObjectEffect::Interact(Interaction::Func(InteractFn::Note(p)))), EditField::Pitch) => {
                p.to_string()
            }
            (
                Some(ObjectEffect::Interact(Interaction::Func(InteractFn::AddCreatures(c)))),
                EditField::Count,
            ) => c.to_string(),
            // Hitbox geometry lives on the object itself, not the effect.
            (_, EditField::HitX) => object.map(|o| o.hitbox.x.to_string()).unwrap_or_default(),
            (_, EditField::HitY) => object.map(|o| o.hitbox.y.to_string()).unwrap_or_default(),
            (_, EditField::HitW) => object.map(|o| o.hitbox.w.to_string()).unwrap_or_default(),
            (_, EditField::HitH) => object.map(|o| o.hitbox.h.to_string()).unwrap_or_default(),
            _ => String::new(),
        };
        self.editing = Some(field);
        self.field = Some(TextField::new(value));
    }

    fn step_text_entry(&mut self, system: &mut impl ConsoleApi, map: &mut MapInfo, maps: &mut MapStore) {
        let Some(field) = self.field.as_mut() else {
            return;
        };
        match field.step(system) {
            TextEvent::Active => {}
            TextEvent::Commit => {
                self.commit_edit(map, maps);
                self.stop_editing();
            }
            TextEvent::Cancel => self.stop_editing(),
        }
    }

    /// Snapshot the selected object, run `f` to mutate it, then record a single
    /// [`EditAction::Modify`] if it actually changed. The before/after snapshots
    /// make every field edit undoable without per-field bookkeeping.
    fn modify_object(&mut self, map: &mut MapInfo, f: impl FnOnce(&mut MapInfo, usize)) {
        let Some(i) = self.selected else { return };
        let Some(before) = Self::snapshot(map, i) else {
            return;
        };
        f(map, i);
        let Some(after) = Self::snapshot(map, i) else {
            return;
        };
        if !snapshot_eq(&before, &after) {
            self.record(EditAction::Modify {
                index: i,
                before,
                after,
            });
        }
    }

    /// Mutate the selected object's [`Warp`] effect via `f` (no-op if it isn't a
    /// warp), recording the change as one undo step.
    fn modify_warp(&mut self, map: &mut MapInfo, f: impl FnOnce(&mut Warp)) {
        self.modify_object(map, |map, i| {
            if let Some(ObjectEffect::Warp(w)) = map.objects.get_mut(i).map(|o| &mut o.effect) {
                f(w);
            }
        });
    }

    fn commit_edit(&mut self, map: &mut MapInfo, maps: &mut MapStore) {
        let Some(field) = self.editing else {
            return;
        };
        let buffer = self
            .field
            .as_ref()
            .map(|f| f.text().trim().to_string())
            .unwrap_or_default();
        // Layer text edits target the store, not the selected object — handle
        // them up front (no object selection required).
        match field {
            EditField::LayerName => {
                self.commit_layer_rename(map, maps, &buffer);
                return;
            }
            // `f64::parse` accepts "NaN"/"inf"; reject them — a non-finite offset
            // serialises to `null` and breaks the next reload.
            EditField::LayerOffX => {
                if let Ok(v) = buffer.parse::<f64>()
                    && v.is_finite()
                {
                    self.commit_layer_prop(map, maps, LayerProp::OffsetX, v);
                }
                return;
            }
            EditField::LayerOffY => {
                if let Ok(v) = buffer.parse::<f64>()
                    && v.is_finite()
                {
                    self.commit_layer_prop(map, maps, LayerProp::OffsetY, v);
                }
                return;
            }
            EditField::LayerRotate => {
                if let Ok(v) = buffer.parse::<u8>() {
                    self.commit_layer_prop(map, maps, LayerProp::Rotate, f64::from(v % 16));
                }
                return;
            }
            _ => {}
        }
        if self.selected.is_none() {
            return;
        }
        match field {
            EditField::Key => self.modify_object(map, |map, i| {
                if let Some(ObjectEffect::Interact(interaction)) =
                    map.objects.get_mut(i).map(|o| &mut o.effect)
                {
                    *interaction = Interaction::Dialogue(buffer.clone());
                }
            }),
            EditField::ToMap => self.modify_warp(map, |w| {
                // The name is stored verbatim (empty = same-map warp); it's
                // resolved against the map store when the warp fires.
                w.map = (!buffer.is_empty()).then(|| buffer.clone());
            }),
            EditField::ToX => {
                if let Ok(x) = buffer.parse() {
                    self.modify_warp(map, |w| w.to.x = x);
                }
            }
            EditField::ToY => {
                if let Ok(y) = buffer.parse() {
                    self.modify_warp(map, |w| w.to.y = y);
                }
            }
            EditField::Narration => self.modify_warp(map, |w| {
                // Empty buffer clears narration; otherwise it's the dialogue key.
                w.narration = (!buffer.is_empty()).then(|| buffer.clone());
            }),
            EditField::Pitch => {
                if let Ok(pitch) = buffer.parse::<i32>() {
                    self.modify_object(map, |map, i| {
                        if let Some(ObjectEffect::Interact(Interaction::Func(InteractFn::Note(p)))) =
                            map.objects.get_mut(i).map(|o| &mut o.effect)
                        {
                            *p = pitch;
                        }
                    });
                }
            }
            EditField::Count => {
                if let Ok(count) = buffer.parse::<usize>() {
                    self.modify_object(map, |map, i| {
                        if let Some(ObjectEffect::Interact(Interaction::Func(
                            InteractFn::AddCreatures(c),
                        ))) = map.objects.get_mut(i).map(|o| &mut o.effect)
                        {
                            *c = count;
                        }
                    });
                }
            }
            // Hitbox geometry: width/height keep a 1px floor so a box stays usable.
            EditField::HitX => {
                if let Ok(x) = buffer.parse() {
                    self.modify_object(map, |map, i| {
                        if let Some(o) = map.objects.get_mut(i) {
                            o.hitbox.x = x;
                        }
                    });
                }
            }
            EditField::HitY => {
                if let Ok(y) = buffer.parse() {
                    self.modify_object(map, |map, i| {
                        if let Some(o) = map.objects.get_mut(i) {
                            o.hitbox.y = y;
                        }
                    });
                }
            }
            EditField::HitW => {
                if let Ok(w) = buffer.parse::<i16>() {
                    self.modify_object(map, |map, i| {
                        if let Some(o) = map.objects.get_mut(i) {
                            o.hitbox.w = w.max(1);
                        }
                    });
                }
            }
            EditField::HitH => {
                if let Ok(h) = buffer.parse::<i16>() {
                    self.modify_object(map, |map, i| {
                        if let Some(o) = map.objects.get_mut(i) {
                            o.hitbox.h = h.max(1);
                        }
                    });
                }
            }
            // Layer fields are handled by the early return above (they target the
            // store, not an object).
            EditField::LayerName
            | EditField::LayerOffX
            | EditField::LayerOffY
            | EditField::LayerRotate => {}
        }
    }

    /// Apply a finished layer rename to the store and record it for undo. Empty
    /// names are ignored (a layer must stay identifiable). A rename can move the
    /// layer between the bg/fg draw lists, so it flags a re-derive.
    fn commit_layer_rename(&mut self, map: &mut MapInfo, maps: &mut MapStore, name: &str) {
        let Some(index) = self.rename_target else { return };
        if name.is_empty() {
            return;
        }
        let Some(before) = maps.get(&map.source).and_then(|tm| tm.layer_name(index)).map(str::to_string)
        else {
            return;
        };
        if before == name {
            return;
        }
        if let Some(tm) = maps.get_mut(&map.source) {
            tm.set_layer_name(index, name);
        }
        self.record(EditAction::LayerRename {
            source: map.source.clone(),
            index,
            before,
            after: name.to_string(),
        });
        self.pending_reload = true;
    }

    /// Apply a finished tile-layer offset / rotation edit to the store and record
    /// it for undo (a no-op if the value is unchanged or the layer is gone).
    fn commit_layer_prop(&mut self, map: &MapInfo, maps: &mut MapStore, prop: LayerProp, value: f64) {
        let Some(index) = self.rename_target else { return };
        let before = maps.get(&map.source).and_then(|tm| match prop {
            LayerProp::OffsetX => tm.layer_offset(index).map(|(x, _)| x),
            LayerProp::OffsetY => tm.layer_offset(index).map(|(_, y)| y),
            // Normalise to the 0..=15 the writer produces, so revert restores an
            // exact value even for a hand-authored palette_rotate > 15.
            LayerProp::Rotate => Some(f64::from(tm.layer_palette_rotate(index) % 16)),
        });
        let Some(before) = before else { return };
        if before == value {
            return;
        }
        apply_layer_prop(maps, &map.source, index, prop, value);
        self.record(EditAction::LayerSetProp {
            source: map.source.clone(),
            index,
            prop,
            before,
            after: value,
        });
        self.pending_reload = true;
    }

    fn cycle(&mut self, map: &mut MapInfo, field: CycleField) {
        // Trigger lives on the MapObject (both kinds), so it cycles through
        // `modify_object`; the warp-only fields go through `modify_warp`.
        match field {
            CycleField::Trigger => self.modify_object(map, |map, i| {
                if let Some(object) = map.objects.get_mut(i) {
                    object.trigger = cycle_trigger(object.trigger);
                }
            }),
            CycleField::Flip => self.modify_warp(map, |w| w.flip = cycle_flip(&w.flip)),
            CycleField::Mode => self.modify_warp(map, |w| w.mode = cycle_mode(&w.mode)),
            CycleField::Sound => self.modify_warp(map, |w| w.sound = cycle_sound(&w.sound)),
            // Advance the interaction kind, rebuilding the effect in place and
            // carrying a sensible default param (piano's origin = the hitbox).
            CycleField::IntKind => self.modify_object(map, |map, i| {
                if let Some(object) = map.objects.get_mut(i)
                    && let ObjectEffect::Interact(interaction) = &object.effect
                {
                    let origin = Vec2::new(object.hitbox.x, object.hitbox.y);
                    let next = cycle_interaction(interaction, origin);
                    object.effect = ObjectEffect::Interact(next);
                }
            }),
            // Handled in `handle_panel` (it needs the map store) — see
            // [`cycle_warp_target`](Self::cycle_warp_target).
            CycleField::WarpTarget => {}
        }
    }

    /// Step the selected warp's destination through `[same-map] + the existing
    /// modern maps`, so a target is picked from real maps rather than typed (and
    /// can't become a dangling name). Recorded as one undo step.
    fn cycle_warp_target(&mut self, map: &mut MapInfo, maps: &MapStore) {
        let names = self.modern_names(maps);
        self.modify_warp(map, move |w| {
            // Options are indexed 0 = same-map (None), then each name at +1.
            let current = match w.map.as_deref() {
                None => 0,
                Some(c) => names.iter().position(|n| n == c).map_or(0, |i| i + 1),
            };
            let next = (current + 1) % (names.len() + 1);
            w.map = (next > 0).then(|| names[next - 1].clone());
        });
    }

    /// Persist the map and start the save-confirmation toast. A map only writes
    /// back when it's in the store as a modern map; anything else (e.g. the
    /// empty default map, source `""`) has no `.tmj` to save to and just logs.
    fn save(&mut self, system: &mut impl ConsoleApi, map: &MapInfo, maps: &mut MapStore) {
        if maps.is_modern(&map.source) {
            let json = maps.get(&map.source).unwrap().to_tmj(&map.objects);
            system.write_file(&format!("maps/{}.tmj", map.source), json.as_bytes());
            sync_store(maps, &map.source, &json);
        } else {
            log::info!("save: {:?} is not a modern map; not saving", map.source);
        }
        self.status.saved();
    }

    // --- Map CRUD -------------------------------------------------------------

    /// Drive the active map dialog from the keyboard: New/Rename take a typed name
    /// (Return commits, Escape cancels); ConfirmDelete confirms on Return. The
    /// dialog is read under one borrow, the resulting op applied after it ends.
    fn step_maps_dialog(&mut self, system: &mut impl ConsoleApi, maps: &mut MapStore) {
        let action = match &mut self.maps_dialog {
            MapsDialog::None => DialogAction::Keep,
            MapsDialog::New { name, w, h, focus } => {
                if system.keyp(ScanCode::Escape) {
                    DialogAction::Close
                } else if system.keyp(ScanCode::Return) {
                    if *focus >= 2 {
                        DialogAction::Create(
                            name.text().trim().to_string(),
                            parse_dim(w.text(), NEW_MAP_W),
                            parse_dim(h.text(), NEW_MAP_H),
                        )
                    } else {
                        *focus += 1; // Enter advances to the next field, then commits.
                        DialogAction::Keep
                    }
                } else {
                    // Type into the focused field — digits only for w/h.
                    let field = match focus {
                        0 => name,
                        1 => w,
                        _ => h,
                    };
                    let digits_only = *focus != 0;
                    for c in system.key_chars() {
                        let allowed = !c.is_control() && (!digits_only || c.is_ascii_digit());
                        if allowed {
                            field.apply(TextOp::Push(*c));
                        }
                    }
                    field.edit_keys(system);
                    DialogAction::Keep
                }
            }
            MapsDialog::Rename { from, name } => match name.step(system) {
                TextEvent::Commit => {
                    DialogAction::Rename(from.clone(), name.text().trim().to_string())
                }
                TextEvent::Cancel => DialogAction::Close,
                TextEvent::Active => DialogAction::Keep,
            },
            MapsDialog::ConfirmDelete(name) => {
                if system.keyp(ScanCode::Return) {
                    DialogAction::Delete(name.clone())
                } else if system.keyp(ScanCode::Escape) {
                    DialogAction::Close
                } else {
                    DialogAction::Keep
                }
            }
            MapsDialog::Resize { source, w, h, focus } => {
                if system.keyp(ScanCode::Escape) {
                    DialogAction::Close
                } else if system.keyp(ScanCode::Return) {
                    if *focus >= 1 {
                        // An emptied / invalid field keeps the map's CURRENT size
                        // (not the new-map default) so a stray backspace can't
                        // silently crop the map down to 30x17.
                        let (cw, ch) = maps
                            .get(source)
                            .map(|t| (t.width, t.height))
                            .unwrap_or((NEW_MAP_W, NEW_MAP_H));
                        DialogAction::Resize(
                            source.clone(),
                            parse_dim(w.text(), cw),
                            parse_dim(h.text(), ch),
                        )
                    } else {
                        *focus += 1; // Enter advances w -> h, then commits.
                        DialogAction::Keep
                    }
                } else {
                    let field = if *focus == 0 { w } else { h };
                    for c in system.key_chars() {
                        if c.is_ascii_digit() {
                            field.apply(TextOp::Push(*c));
                        }
                    }
                    field.edit_keys(system);
                    DialogAction::Keep
                }
            }
        };
        match action {
            DialogAction::Keep => {}
            DialogAction::Close => self.maps_dialog = MapsDialog::None,
            DialogAction::Create(name, w, h) => {
                self.maps_dialog = MapsDialog::None;
                if valid_map_name(&name, maps) {
                    self.create_map(system, maps, &name, w, h);
                }
            }
            DialogAction::Rename(from, to) => {
                self.maps_dialog = MapsDialog::None;
                if valid_map_name(&to, maps) {
                    self.rename_map(system, maps, &from, &to);
                }
            }
            DialogAction::Delete(name) => {
                self.maps_dialog = MapsDialog::None;
                self.delete_map(system, maps, &name);
            }
            DialogAction::Resize(source, w, h) => {
                self.maps_dialog = MapsDialog::None;
                if let Some(tm) = maps.get_mut(&source) {
                    tm.resize(w, h);
                }
                self.status.edited();
                self.pending_reload = true;
            }
        }
    }

    /// Create a blank modern map: insert it, write its `.tmj`, and add it to the
    /// manifest. (Disk writes are silent no-ops on web — the map still lives in
    /// the store for the session.)
    fn create_map(
        &mut self,
        system: &mut impl ConsoleApi,
        maps: &mut MapStore,
        name: &str,
        w: usize,
        h: usize,
    ) {
        let map = TiledMap::blank_modern(w, h);
        let json = map.to_tmj(&[]);
        maps.insert(name, map);
        system.write_file(&format!("maps/{name}.tmj"), json.as_bytes());
        self.manifest_mutate(system, maps, |m| {
            if !m.maps.iter().any(|n| n == name) {
                m.maps.push(name.to_string());
            }
        });
        self.maps_selected = Some(name.to_string());
    }

    /// Duplicate `src` under a deduped `<src>_copy` name. Byte-copies the on-disk
    /// `.tmj` so objects and tilesets survive verbatim; falls back to re-serialising
    /// the tiles if the source file can't be read (e.g. web).
    fn duplicate_map(&mut self, system: &mut impl ConsoleApi, maps: &mut MapStore, src: &str) {
        let Some(orig) = maps.get(src).cloned() else {
            return;
        };
        let name = dedup_name(src, maps);
        let bytes = system
            .read_file(&format!("maps/{src}.tmj"))
            .unwrap_or_else(|| orig.to_tmj(&[]).into_bytes());
        maps.insert(name.clone(), orig);
        system.write_file(&format!("maps/{name}.tmj"), &bytes);
        let added = name.clone();
        self.manifest_mutate(system, maps, |m| {
            if !m.maps.contains(&added) {
                m.maps.push(added.clone());
            }
        });
        self.maps_selected = Some(name);
    }

    /// Rename `from` to `to`: write the new `.tmj` (byte-copy), re-key the store,
    /// and update the manifest. The old `.tmj` is orphaned (no `remove_file`); the
    /// manifest drop keeps it from reloading. Warps pointing at `from` are left
    /// dangling — they no-op at runtime ([`map_by_name`] returns `None`).
    fn rename_map(&mut self, system: &mut impl ConsoleApi, maps: &mut MapStore, from: &str, to: &str) {
        if let Some(bytes) = system.read_file(&format!("maps/{from}.tmj")) {
            system.write_file(&format!("maps/{to}.tmj"), &bytes);
        } else if let Some(map) = maps.get(from) {
            // No source file to copy (web): re-serialise the tiles.
            let json = map.to_tmj(&[]);
            system.write_file(&format!("maps/{to}.tmj"), json.as_bytes());
        }
        maps.rename(from, to);
        let (from_s, to_s) = (from.to_string(), to.to_string());
        self.manifest_mutate(system, maps, |m| {
            for n in m.maps.iter_mut() {
                if *n == from_s {
                    *n = to_s.clone();
                }
            }
        });
        self.maps_selected = Some(to.to_string());
    }

    /// Delete a map: drop it from the store and the manifest. The `.tmj` is left
    /// on disk (no `remove_file` in the console API) but won't reload.
    fn delete_map(&mut self, system: &mut impl ConsoleApi, maps: &mut MapStore, name: &str) {
        maps.remove(name);
        self.manifest_mutate(system, maps, |m| m.maps.retain(|n| n != name));
        if self.maps_selected.as_deref() == Some(name) {
            self.maps_selected = None;
        }
    }

    /// Read-modify-write the asset manifest. Falls back to the store's current
    /// names if no manifest file is present, so a fresh manifest is still correct.
    fn manifest_mutate(
        &self,
        system: &mut impl ConsoleApi,
        maps: &MapStore,
        f: impl FnOnce(&mut GameManifest),
    ) {
        let mut manifest = system
            .read_file("game.manifest")
            .and_then(|b| manifest_from_json(&b).ok())
            .unwrap_or_else(|| GameManifest {
                maps: maps.names().iter().map(|s| s.to_string()).collect(),
            });
        f(&mut manifest);
        system.write_file("game.manifest", manifest_to_json(&manifest).as_bytes());
    }

    // --- Draw -----------------------------------------------------------------

    pub fn draw_map_viewer(
        &self,
        draw_state: &mut DrawState,
        system: &mut impl ConsoleApi,
        maps: &MapStore,
        walkaround: &WalkaroundState,
    ) {
        self.draw_at(
            draw_state,
            system,
            &walkaround.current_map,
            maps,
            walkaround.camera.pos,
        );
    }

    /// Draw the dock resize bars (the inner-edge splitter band per occupied dock
    /// side). Drawn between the docked panels and the floats so a floating window
    /// sits on top of any bar it overlaps.
    fn draw_splitters(&self, draw_state: &mut DrawState) {
        let splitter = draw_state.colour(13);
        for &(_side, band) in &self.dock.solved.splitters {
            draw_state.rgba(LayerId::BG).fill_rect(
                band.x as i32,
                band.y as i32,
                band.w as i32,
                band.h as i32,
                splitter,
            );
        }
    }

    /// When toggled on (`G`), dot the 8px tile grid over the world and show the
    /// cursor's tile coordinate in the world's top-right corner. Clipped to the
    /// world rect so it never bleeds under a docked panel.
    fn draw_grid(&self, draw_state: &mut DrawState, system: &mut impl ConsoleApi, camera_pos: Vec2) {
        if !self.show_grid {
            return;
        }
        let world = self.dock.solved.world;
        if world.w <= 0 || world.h <= 0 {
            return;
        }
        let dot = draw_state.colour(13);
        let (cx, cy) = (i32::from(camera_pos.x), i32::from(camera_pos.y));
        let (wx0, wy0) = (i32::from(world.x), i32::from(world.y));
        let (wx1, wy1) = (wx0 + i32::from(world.w), wy0 + i32::from(world.h));
        // First grid line at/after edge `w0` such that `(line + c)` is a multiple of 8.
        let first = |w0: i32, c: i32| w0 + (-c - w0).rem_euclid(8);
        let mut gy = first(wy0, cy);
        while gy < wy1 {
            let mut gx = first(wx0, cx);
            while gx < wx1 {
                draw_state.rgba(LayerId::BG).fill_rect(gx, gy, 1, 1, dot);
                gx += 8;
            }
            gy += 8;
        }
        // Cursor tile-coordinate readout, bottom-right — clear of the global
        // undo/redo/save bar at the world's top-left (which draws on top), even
        // when the world is only a little wider than that bar.
        if world.contains(system.mouse().pos()) {
            let (tx, ty) = world_tile(&system.mouse(), camera_pos);
            let mut b = UiBuilder::<EditorKey>::new();
            let t = b
                .text(format!("{tx},{ty}"))
                .small(true)
                .center()
                .color(0)
                .fill(11)
                .size(30.0, 7.0)
                .id();
            b.finish(t, (30.0, 7.0)).draw_at(
                world.x + world.w - 31,
                world.y + world.h - 8,
                draw_state,
                system,
                LayerId::BG,
            );
        }
    }

    /// Draw the editor overlay + panels for `map` from an explicit `camera_pos`.
    /// Generalises [`draw_map_viewer`](Self::draw_map_viewer) so an extra view
    /// can run its own editor against its own free camera, rather than the live
    /// walkaround camera. No-op while unfocused.
    pub fn draw_at(
        &self,
        draw_state: &mut DrawState,
        system: &mut impl ConsoleApi,
        map: &MapInfo,
        maps: &MapStore,
        camera_pos: Vec2,
    ) {
        if !self.focused {
            return;
        }
        self.draw_hidden_active_layer(draw_state, map, maps, camera_pos);
        self.draw_grid(draw_state, system, camera_pos);
        self.draw_canvas_overlay(draw_state, system, map, camera_pos);
        // Draw each panel back-to-front from the geometry `step` already solved
        // (not a fresh layout against the live canvas) — so a framebuffer resize
        // between step and draw can't misregister hit vs. draw; it heals next
        // frame. A floating panel gets a small SE resize-handle mark, and a Maps
        // panel gets its thumbnails blitted over the cells.
        // `rects` is ordered docked-first then floats (ascending z). Draw the
        // dock splitters at that boundary — after the docked panels (so a bar sits
        // on top of its own dock's edge) but before the floats (so a floating
        // window covers a bar it overlaps, rather than the bar drawing over it).
        let handle = draw_state.colour(13);
        let mut splitters_drawn = false;
        for &(idx, rect) in &self.dock.solved.rects {
            if !splitters_drawn && self.dock.is_float(idx) {
                self.draw_splitters(draw_state);
                splitters_drawn = true;
            }
            let ui = self.build_panel(idx, rect, map, maps);
            let (scroll, scrolling) = self.panel_scroll(idx, rect, ui.content_height());
            if scrolling {
                // Pinned title above a scrolled, clipped body, plus a scroll bar.
                let body = Self::panel_body(rect);
                let title_clip = Rect { x: rect.x, y: rect.y, w: rect.w, h: PANEL_TITLE_H };
                ui.draw_at_clipped(rect.x, rect.y - scroll, body, draw_state, system, LayerId::BG);
                ui.draw_at_clipped(rect.x, rect.y, title_clip, draw_state, system, LayerId::BG);
                self.draw_panel_scrollbar(rect, scroll, ui.content_height(), draw_state);
            } else {
                ui.draw_at(rect.x, rect.y, draw_state, system, LayerId::BG);
            }
            match self.dock.panels[idx].kind {
                PanelKind::Maps => self.draw_map_thumbnails(&ui, rect, maps, draw_state),
                PanelKind::Paint => self.draw_palette(draw_state),
                PanelKind::Objects => self.draw_warp_preview(&ui, rect, idx, map, maps, draw_state),
                _ => {}
            }
            if self.dock.is_float(idx) {
                let s = dock::FLOAT_HANDLE as i32;
                draw_state.rgba(LayerId::BG).fill_rect(
                    (rect.x + rect.w) as i32 - s,
                    (rect.y + rect.h) as i32 - s,
                    s,
                    s,
                    handle,
                );
            }
        }
        // No floats this frame: the splitters still draw, after the docked panels.
        if !splitters_drawn {
            self.draw_splitters(draw_state);
        }
        // Drop-zone highlight: while dragging a panel near an edge, outline where
        // a release would dock it.
        if let Some(side) = self.dock.solved.hot_edge {
            let (sw, sh) = self.dock.solved.screen;
            let z = DockManager::edge_zone(side, (sw as f32, sh as f32));
            let hot = draw_state.colour(11);
            draw_state
                .rgba(LayerId::BG)
                .stroke_rect(z.x as i32, z.y as i32, z.w as i32, z.h as i32, hot);
        }
        // The always-on global bar, on top of everything, at the world's corner.
        let world = self.dock.solved.world;
        self.build_global_bar()
            .draw_at(world.x + 1, world.y + 1, draw_state, system, LayerId::BG);
        // A modal map dialog, centred over everything.
        if self.maps_dialog.is_active() {
            self.build_dialog()
                .draw_at(0, 0, draw_state, system, LayerId::BG);
        }
    }

    /// While painting a *hidden* layer (e.g. the collision layer), ghost its
    /// tiles over the world — checkerboard-dithered — so you can see what you're
    /// editing without un-hiding it.
    fn draw_hidden_active_layer(
        &self,
        draw_state: &mut DrawState,
        map: &MapInfo,
        maps: &MapStore,
        camera_pos: Vec2,
    ) {
        if !matches!(self.tool, EditorTool::Paint | EditorTool::Select) {
            return;
        }
        let Some(active) = self.active_layer(map) else {
            return;
        };
        if active.visible || active.kind != LayerKind::Tiles {
            return;
        }
        let Some(TiledMapLayer::TileLayer(tl)) = maps
            .get(&map.source)
            .and_then(|m| m.layers.get(active.source_layer))
        else {
            return;
        };
        let (fw, fh) = ((tl.width * 8) as u32, (tl.height * 8) as u32);
        if fw == 0 || fh == 0 {
            return;
        }
        let mut ghost = RgbaImage::new(fw, fh);
        let mut opts: MapOptions = active.clone().into();
        opts.sx = 0;
        opts.sy = 0;
        let pmap = palette_map_rotate(active.palette_rotate() as usize);
        ghost.map_draw_indexed(tl, &draw_state.indexed_sprites, &draw_state.palettes[0], &pmap, opts);
        for gy in 0..fh {
            for gx in 0..fw {
                if (gx + gy) % 2 == 1 {
                    ghost.set_pixel(gx, gy, Rgba([0, 0, 0, 0]));
                }
            }
        }
        draw_state.rgba(LayerId::BG).blit::<RgbaImage>(
            -(camera_pos.x as i32),
            -(camera_pos.y as i32),
            &ghost,
            EdgePolicy::Transparent,
            Transform::default(),
            |p| p.a() == 0,
        );
    }

    /// Blit the Paint palette into its viewport: the sheet's 32-wide tile grid,
    /// scrolled to `(pal_col, pal_row)`, the selected tile outlined, plus thin
    /// scroll bars on the overflowing edges.
    fn draw_palette(&self, draw_state: &mut DrawState) {
        let v = self.pal_rect;
        if v.w <= 0 || v.h <= 0 {
            return;
        }
        let (vc, vr) = self.palette_visible();
        let (bw, bh) = self.brush_size();
        let (bc, br) = (self.selected_tile % self.sheet_cols(), self.selected_tile / self.sheet_cols());
        for r in 0..vr {
            for c in 0..vc {
                let (col, row) = (self.pal_col + c, self.pal_row + r);
                if col >= self.sheet_cols() {
                    continue;
                }
                let id = row * self.sheet_cols() + col;
                if id >= self.sheet_tiles() {
                    continue;
                }
                let x = v.x as i32 + c as i32 * 8;
                let y = v.y as i32 + r as i32 * 8;
                let opts = SpriteOptions { transparent: Some(0), ..Default::default() };
                let in_brush = col >= bc && col < bc + bw && row >= br && row < br + bh;
                if in_brush {
                    draw_state.spr_with_outline(
                        LayerId::BG,
                        &PALETTE_MAP_IDENTITY,
                        id as i32,
                        x,
                        y,
                        opts,
                        11,
                    );
                } else {
                    draw_state.spr(LayerId::BG, &PALETTE_MAP_IDENTITY, id as i32, x, y, opts);
                }
            }
        }

        // Scroll bars (2px) on the right / bottom when the grid overflows.
        let (max_c, max_r) = self.palette_scroll_max();
        let track = draw_state.colour(0);
        let thumb = draw_state.colour(13);
        if max_r > 0 {
            let bx = (v.x + v.w - 2) as i32;
            draw_state.rgba(LayerId::BG).fill_rect(bx, v.y as i32, 2, v.h as i32, track);
            let (th, travel) = self.palette_thumb_v();
            let ty = v.y as i32 + travel * self.pal_row as i32 / max_r as i32;
            draw_state.rgba(LayerId::BG).fill_rect(bx, ty, 2, th, thumb);
        }
        if max_c > 0 {
            let by = (v.y + v.h - 2) as i32;
            draw_state.rgba(LayerId::BG).fill_rect(v.x as i32, by, v.w as i32, 2, track);
            let (tw, travel) = self.palette_thumb_h();
            let tx = v.x as i32 + travel * self.pal_col as i32 / max_c as i32;
            draw_state.rgba(LayerId::BG).fill_rect(tx, by, tw, 2, thumb);
        }
    }

    /// Blit a rendered preview of each visible map over its browser cell. Drawn
    /// after the panel UI so the thumbnail lands on top of the cell's outline.
    fn draw_map_thumbnails(
        &self,
        ui: &Ui<EditorKey>,
        rect: Rect,
        maps: &MapStore,
        draw_state: &mut DrawState,
    ) {
        let names = self.modern_names(maps);
        let (cols, grid_rows) = self.maps_grid(rect);
        let per_page = (cols * grid_rows).max(1);
        let pages = names.len().div_ceil(per_page).max(1);
        let start = self.maps_scroll.min(pages - 1) * per_page;
        for (i, name) in names.iter().enumerate().skip(start).take(per_page) {
            let Some(slot) = ui.rect_at(rect.x, rect.y, EditorKey::MapSlot(i)) else {
                continue;
            };
            // 1px inset so the thumbnail sits inside the cell outline.
            let Some(thumb) =
                render_map_thumbnail(name, maps, draw_state, slot.w as u32 - 2, slot.h as u32 - 2)
            else {
                continue;
            };
            let ox = slot.x as i32 + (slot.w as i32 - thumb.width() as i32) / 2;
            let oy = slot.y as i32 + (slot.h as i32 - thumb.height() as i32) / 2;
            draw_state.rgba(LayerId::BG).blit::<RgbaImage>(
                ox,
                oy,
                &thumb,
                EdgePolicy::Transparent,
                Transform::default(),
                |p| p.a() == 0,
            );
        }
    }

    /// Set the selected warp's landing point from a click in its destination
    /// preview box: invert the same letterbox fit the draw used to recover the
    /// clicked map pixel, clamped to the target's bounds. A click in the
    /// letterbox margin (outside the rendered map) is ignored. One undo step.
    fn place_warp_from_preview(
        &mut self,
        map: &mut MapInfo,
        maps: &MapStore,
        box_rect: Rect,
        cursor: Vec2,
    ) {
        let dest = match self.selected.and_then(|i| map.objects.get(i)).map(|o| &o.effect) {
            Some(ObjectEffect::Warp(w)) => w.map.clone().unwrap_or_else(|| map.source.clone()),
            _ => return,
        };
        let Some(tiled) = maps.get(&dest) else {
            return;
        };
        let (fw, fh) = ((tiled.width as u32 * 8).max(1), (tiled.height as u32 * 8).max(1));
        let (inner, s) = fit_preview(box_rect, fw, fh);
        if s <= 0.0 || !inner.contains(cursor) {
            return;
        }
        let mx = (((cursor.x - inner.x) as f32) / s).clamp(0.0, fw as f32 - 1.0) as i16;
        let my = (((cursor.y - inner.y) as f32) / s).clamp(0.0, fh as f32 - 1.0) as i16;
        self.modify_warp(map, |w| {
            w.to.x = mx;
            w.to.y = my;
        });
    }

    /// Blit a rendered preview of the selected warp's destination map over its
    /// preview box (after the panel UI, so it lands on top of the box outline),
    /// with the player drawn at the landing point and a crosshair pinpointing it.
    /// A no-op unless a warp is selected (the box is only emitted then). An
    /// unknown target (a free-typed name with no map) gets an `X` instead.
    ///
    /// `scroll` shifts the box with the panel body and `clip` (the body region,
    /// when the panel scrolls) crops the blit/marks so a half-scrolled preview
    /// stays inside the panel.
    fn draw_warp_preview(
        &self,
        ui: &Ui<EditorKey>,
        rect: Rect,
        idx: usize,
        map: &MapInfo,
        maps: &MapStore,
        draw_state: &mut DrawState,
    ) {
        let (scroll, scrolling) = self.panel_scroll(idx, rect, ui.content_height());
        let clip = scrolling.then(|| Self::panel_body(rect));
        let (dest, landing) = match self.selected.and_then(|i| map.objects.get(i)).map(|o| &o.effect)
        {
            Some(ObjectEffect::Warp(w)) => (w.map.clone().unwrap_or_else(|| map.source.clone()), w.to),
            _ => return,
        };
        let Some(box_rect) = ui.rect_at(rect.x, rect.y - scroll, EditorKey::WarpPreview) else {
            return;
        };
        // Nothing to draw if the box scrolled entirely out of the body.
        if clip.is_some_and(|c| clamp_to(box_rect, c).is_none()) {
            return;
        }
        let Some((mut full, fw, fh)) = render_map_full(&dest, maps, draw_state) else {
            // Unknown target: cross the box out so the dangling name is obvious.
            let bad = draw_state.colour(8);
            let (x0, y0) = (box_rect.x as i32, box_rect.y as i32);
            let (x1, y1) = ((box_rect.x + box_rect.w) as i32 - 1, (box_rect.y + box_rect.h) as i32 - 1);
            let layer = draw_state.rgba(LayerId::BG);
            layer.line(x0, y0, x1, y1, bad);
            layer.line(x0, y1, x1, y0, bad);
            return;
        };

        // Draw the player at the spawn point (its feet on the landing pixel, as in
        // the live game), outlined so it reads even once downscaled.
        let (sprite, _) = Shell::default().sprite_options();
        let opts = SpriteOptions { transparent: Some(0), ..sprite };
        let (px, py) = (landing.x as i32 - opts.x_offset, landing.y as i32 - opts.y_offset);
        let (sprites, palette) = (&draw_state.indexed_sprites, &draw_state.palettes[0]);
        full.spr_outline(sprites, palette, opts.id, px, py, opts.clone(), 0);
        full.spr_indexed(sprites, palette, &PALETTE_MAP_IDENTITY, opts.id, px, py, opts);

        // Letterbox the rendered map into the box, then blit it — cropping to the
        // clip (body) region when the panel scrolls so it can't spill out.
        let (inner, s) = fit_preview(box_rect, fw, fh);
        let thumb = downscale(&full, fw, fh, inner.w as u32, inner.h as u32);
        blit_clipped(draw_state, inner, &thumb, clip);

        // A bright crosshair marks the exact landing pixel, clamped to the box (and
        // the clip) so a near-edge or half-scrolled landing can't draw outside.
        let bound = match clip {
            Some(c) => clamp_to(box_rect, c),
            None => Some(box_rect),
        };
        if let Some(bound) = bound {
            let mark = draw_state.colour(11);
            let (x0, y0) = (bound.x as i32, bound.y as i32);
            let (x1, y1) = ((bound.x + bound.w) as i32 - 1, (bound.y + bound.h) as i32 - 1);
            let cx = (inner.x as i32 + (landing.x as f32 * s) as i32).clamp(x0, x1);
            let cy = (inner.y as i32 + (landing.y as f32 * s) as i32).clamp(y0, y1);
            let arm = 4;
            let layer = draw_state.rgba(LayerId::BG);
            layer.line((cx - arm).max(x0), cy, (cx + arm).min(x1), cy, mark);
            layer.line(cx, (cy - arm).max(y0), cx, (cy + arm).min(y1), mark);
        }
    }

    /// Draw tool overlays onto the live world: a tile cursor (paint) or object
    /// hitboxes + the in-progress drag rect (interactables/warps).
    fn draw_canvas_overlay(
        &self,
        draw_state: &mut DrawState,
        system: &mut impl ConsoleApi,
        map: &MapInfo,
        camera_pos: Vec2,
    ) {
        let cx = i32::from(camera_pos.x);
        let cy = i32::from(camera_pos.y);
        match self.tool {
            EditorTool::Paint => {
                let colour = draw_state.colour(11);
                if let Some(start) = self.drag {
                    // Shift+drag rectangle fill: outline the tile-snapped region.
                    let m = system.mouse();
                    let world = Vec2::new(m.pos().x + camera_pos.x, m.pos().y + camera_pos.y);
                    let (x0, y0, x1, y1) = tile_bounds(start, world);
                    draw_state.rgba(LayerId::BG).stroke_rect(
                        x0 * 8 - cx,
                        y0 * 8 - cy,
                        (x1 - x0 + 1) * 8,
                        (y1 - y0 + 1) * 8,
                        colour,
                    );
                } else {
                    // Soft brush preview: a dithered ghost of the tiles the brush
                    // would stamp here, under a footprint outline.
                    let (tx, ty) = world_tile(&system.mouse(), camera_pos);
                    let (px, py) = (tx * 8 - cx, ty * 8 - cy);
                    let (bw, bh) = self.brush_size();
                    let (bc, br) = (self.selected_tile % self.sheet_cols(), self.selected_tile / self.sheet_cols());
                    let mut ghost = RgbaImage::new((bw * 8) as u32, (bh * 8) as u32);
                    for dy in 0..bh {
                        for dx in 0..bw {
                            if bc + dx >= self.sheet_cols() {
                                continue;
                            }
                            let id = ((br + dy) * self.sheet_cols() + (bc + dx)) as i32;
                            ghost.spr_indexed(
                                &draw_state.indexed_sprites,
                                &draw_state.palettes[0],
                                &PALETTE_MAP_IDENTITY,
                                id,
                                (dx * 8) as i32,
                                (dy * 8) as i32,
                                SpriteOptions { transparent: Some(0), ..Default::default() },
                            );
                        }
                    }
                    // Knock out a checkerboard so it reads as a preview, not paint.
                    for gy in 0..ghost.height() {
                        for gx in 0..ghost.width() {
                            if (gx + gy) % 2 == 1 {
                                ghost.set_pixel(gx, gy, Rgba([0, 0, 0, 0]));
                            }
                        }
                    }
                    draw_state.rgba(LayerId::BG).blit::<RgbaImage>(
                        px,
                        py,
                        &ghost,
                        EdgePolicy::Transparent,
                        Transform::default(),
                        |p| p.a() == 0,
                    );
                    draw_state.rgba(LayerId::BG).stroke_rect(
                        px,
                        py,
                        (bw * 8) as i32,
                        (bh * 8) as i32,
                        colour,
                    );
                }
            }
            EditorTool::Select => {
                let Some(sel) = self.selection else {
                    return;
                };
                let outline = draw_state.colour(11);
                let (px, py) = (sel.x * 8 - cx, sel.y * 8 - cy);
                // Paste preview: a dithered ghost of the clipboard at the marquee
                // origin, exactly where `SelPaste` would stamp it.
                if let Some(clip) = &self.clipboard {
                    let mut ghost = RgbaImage::new((clip.w * 8) as u32, (clip.h * 8) as u32);
                    for dy in 0..clip.h {
                        for dx in 0..clip.w {
                            let id = clip.tiles[dy * clip.w + dx];
                            if id == 0 {
                                continue;
                            }
                            ghost.spr_indexed(
                                &draw_state.indexed_sprites,
                                &draw_state.palettes[0],
                                &PALETTE_MAP_IDENTITY,
                                id as i32,
                                (dx * 8) as i32,
                                (dy * 8) as i32,
                                SpriteOptions { transparent: Some(0), ..Default::default() },
                            );
                        }
                    }
                    for gy in 0..ghost.height() {
                        for gx in 0..ghost.width() {
                            if (gx + gy) % 2 == 1 {
                                ghost.set_pixel(gx, gy, Rgba([0, 0, 0, 0]));
                            }
                        }
                    }
                    draw_state.rgba(LayerId::BG).blit::<RgbaImage>(
                        px,
                        py,
                        &ghost,
                        EdgePolicy::Transparent,
                        Transform::default(),
                        |p| p.a() == 0,
                    );
                }
                // The marquee outline on top.
                draw_state.rgba(LayerId::BG).stroke_rect(
                    px,
                    py,
                    (sel.w * 8) as i32,
                    (sel.h * 8) as i32,
                    outline,
                );
            }
            EditorTool::Interactables | EditorTool::Warps => {
                // Filtered overlay: only the active tab's kind, warps in colour
                // 12 and interactions in 14, the selected object highlighted.
                let kind = self.obj_kind();
                let base = draw_state.colour(if kind == ObjKind::Warp { 12 } else { 14 });
                let sel = draw_state.colour(11);
                let canvas = draw_state.rgba(LayerId::BG);
                for (i, object) in map.objects.iter().enumerate() {
                    if !kind.matches(object) {
                        continue;
                    }
                    let colour = if Some(i) == self.selected { sel } else { base };
                    let h = object.hitbox;
                    canvas.stroke_rect(
                        i32::from(h.x) - cx,
                        i32::from(h.y) - cy,
                        i32::from(h.w),
                        i32::from(h.h),
                        colour,
                    );
                }
                self.draw_drag_preview(draw_state, system, camera_pos);
            }
            EditorTool::Layers => {}
        }
    }

    fn draw_drag_preview(
        &self,
        draw_state: &mut DrawState,
        system: &mut impl ConsoleApi,
        camera_pos: Vec2,
    ) {
        if let Some(start) = self.drag {
            let m = system.mouse();
            let world = Vec2::new(m.pos().x + camera_pos.x, m.pos().y + camera_pos.y);
            let h = hitbox_between(start, world);
            let colour = draw_state.colour(11);
            draw_state.rgba(LayerId::BG).stroke_rect(
                i32::from(h.x) - i32::from(camera_pos.x),
                i32::from(h.y) - i32::from(camera_pos.y),
                i32::from(h.w),
                i32::from(h.h),
                colour,
            );
        }
    }
}

/// The map tile (8px grid) under the cursor, in world coordinates.
fn world_tile(mouse: &MouseInput, camera_pos: Vec2) -> (i32, i32) {
    let p = mouse.pos();
    (
        (i32::from(p.x) + i32::from(camera_pos.x)).div_euclid(8),
        (i32::from(p.y) + i32::from(camera_pos.y)).div_euclid(8),
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

/// The short label for an interaction kind shown in the Objects panel.
fn interaction_kind_label(i: &Interaction) -> &'static str {
    match i {
        Interaction::None => "none",
        Interaction::Dialogue(_) => "dialog",
        Interaction::Func(f) => f.name().unwrap_or("func"),
    }
}

/// Advance an interaction to the next kind, preserving a sensible default param.
/// Cycle: none → dialogue → toggle_dog → piano → note → add_creatures → none.
/// `origin` seeds a fresh `piano` (it sounds the note under its own position).
fn cycle_interaction(current: &Interaction, origin: Vec2) -> Interaction {
    match current {
        Interaction::None => Interaction::Dialogue(String::new()),
        Interaction::Dialogue(_) => Interaction::Func(InteractFn::ToggleDog),
        Interaction::Func(InteractFn::ToggleDog) => Interaction::Func(InteractFn::Piano(origin)),
        Interaction::Func(InteractFn::Piano(_)) => Interaction::Func(InteractFn::Note(0)),
        Interaction::Func(InteractFn::Note(_)) => Interaction::Func(InteractFn::AddCreatures(0)),
        Interaction::Func(InteractFn::AddCreatures(_)) => Interaction::None,
        // Pet (no `func` name) can't be authored; cycle it back to none.
        Interaction::Func(_) => Interaction::None,
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

/// Render map `name` 1:1 at the map origin (no camera) into a fresh image, using
/// the live sprite sheet from `draw_state`. The same per-layer render as the
/// world: tile layers via the sheet, image layers blitted. Returns the image and
/// its `(width, height)` px. `None` if the map is unknown.
fn render_map_full(name: &str, maps: &MapStore, draw_state: &DrawState) -> Option<(RgbaImage, u32, u32)> {
    let info = map_by_name(&draw_state.indexed_sprites, name, maps)?;
    let tiled = maps.get(name)?;
    let (fw, fh) = ((tiled.width as u32 * 8).max(1), (tiled.height as u32 * 8).max(1));
    let sprites = &draw_state.indexed_sprites;
    let palette = draw_state.palettes[0].as_slice();

    let mut full = RgbaImage::new(fw, fh);
    for layer in info.layers.iter().chain(info.fg_layers.iter()) {
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
    let s = (max_w as f32 / fw as f32).min(max_h as f32 / fh as f32).min(1.0);
    Some(downscale(&full, fw, fh, (fw as f32 * s) as u32, (fh as f32 * s) as u32))
}

/// Letterbox a `(fw, fh)`-px image inside `outer`, downscale-only, centred.
/// Returns the inner rect the image occupies and the scale (preview px per source
/// px). Shared by the warp preview's draw (where to blit) and click handling
/// (how to invert a click back to a map coordinate) so they can't disagree.
fn fit_preview(outer: Rect, fw: u32, fh: u32) -> (Rect, f32) {
    let s = (outer.w as f32 / fw as f32)
        .min(outer.h as f32 / fh as f32)
        .clamp(0.0, 1.0);
    let (iw, ih) = (((fw as f32 * s) as i16).max(1), ((fh as f32 * s) as i16).max(1));
    let ix = outer.x + (outer.w - iw) / 2;
    let iy = outer.y + (outer.h - ih) / 2;
    (Rect { x: ix, y: iy, w: iw, h: ih }, s)
}

/// `rect` ∩ `clip`, or `None` if they don't overlap — for cropping a preview to a
/// scrolling panel's visible body.
fn clamp_to(rect: Rect, clip: Rect) -> Option<Rect> {
    let x0 = rect.x.max(clip.x);
    let y0 = rect.y.max(clip.y);
    let x1 = (rect.x + rect.w).min(clip.x + clip.w);
    let y1 = (rect.y + rect.h).min(clip.y + clip.h);
    (x1 > x0 && y1 > y0).then_some(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
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
    let ax = i32::from(a.x).div_euclid(8);
    let ay = i32::from(a.y).div_euclid(8);
    let bx = i32::from(b.x).div_euclid(8);
    let by = i32::from(b.y).div_euclid(8);
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

/// Toggle a layer name's foreground `fg` prefix (the marker `push_bg_or_fg` keys
/// on). A leading `fg` counts as the marker only when it stands alone or is
/// followed by a separator — so `fg water` toggles back to `water`, while a
/// plain word like `fgrass` is left untouched (a harmless no-op) rather than
/// corrupted to `rass`. A name that is only the prefix falls back to `layer`.
fn toggle_fg_prefix(name: &str) -> String {
    if let Some(rest) = name.to_lowercase().strip_prefix("fg") {
        if rest.is_empty() || !rest.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            let stripped = name[2..].trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
            return if stripped.is_empty() { "layer".to_string() } else { stripped.to_string() };
        }
        return name.to_string(); // "fg" glued to a word — not a clean marker.
    }
    format!("fg {name}")
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
/// compared here alongside the hitbox and effect.
fn snapshot_eq(a: &ObjSnapshot, b: &ObjSnapshot) -> bool {
    let same_box = a.hitbox.x == b.hitbox.x
        && a.hitbox.y == b.hitbox.y
        && a.hitbox.w == b.hitbox.w
        && a.hitbox.h == b.hitbox.h;
    same_box && a.trigger == b.trigger && effect_eq(&a.effect, &b.effect)
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
        (Interaction::None, Interaction::None) => true,
        (Interaction::Func(_), Interaction::Func(_)) => true,
        _ => false,
    }
}

/// Remove object `i` from the objects list, ignoring out-of-range indices.
fn remove_object(map: &mut MapInfo, i: usize) {
    if i < map.objects.len() {
        map.objects.remove(i);
    }
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

fn mode_label(mode: &WarpMode) -> &'static str {
    match mode {
        WarpMode::Auto => "auto",
        WarpMode::Interact => "act",
    }
}

/// Short label for the trigger cycle row (kept terse for the 84px column).
fn trigger_label(trigger: Trigger) -> &'static str {
    match trigger {
        Trigger::Touch => "touch",
        Trigger::Press => "press",
        Trigger::Any => "any",
    }
}

fn sound_label(sound: &Option<SfxData>) -> &'static str {
    match sound {
        None => "none",
        Some(s) if s.id == sound::DOOR.id => "door",
        Some(s) if s.id == sound::STAIRS_DOWN.id => "dn",
        Some(s) if s.id == sound::STAIRS_UP.id => "up",
        Some(_) => "?",
    }
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

/// Advance the trigger cycle row: Touch → Press → Any → Touch.
fn cycle_trigger(trigger: Trigger) -> Trigger {
    match trigger {
        Trigger::Touch => Trigger::Press,
        Trigger::Press => Trigger::Any,
        Trigger::Any => Trigger::Touch,
    }
}

fn cycle_sound(sound: &Option<SfxData>) -> Option<SfxData> {
    match sound {
        None => Some(sound::DOOR),
        Some(s) if s.id == sound::DOOR.id => Some(sound::STAIRS_DOWN),
        Some(s) if s.id == sound::STAIRS_DOWN.id => Some(sound::STAIRS_UP),
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
    use crate::data::tmj::{TiledMapLayer, from_json};
    match from_json(json.as_bytes()) {
        Ok(mut fresh) => {
            if let Some(old) = maps.get(name) {
                let pixels: Vec<(String, _)> = old
                    .layers
                    .iter()
                    .filter_map(|layer| match layer {
                        TiledMapLayer::ImageLayer(image) => Some((
                            image.image.clone(),
                            image.pixels.clone()?,
                        )),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tiles(cells: Vec<(i32, i32, usize, usize)>) -> EditAction {
        EditAction::Tiles {
            source: String::new(),
            layer: 0,
            cells,
        }
    }

    /// Pushing a new action clears any redo future: a fresh edit invalidates the
    /// redone branch, the standard linear-history model.
    #[test]
    fn history_push_clears_redo() {
        let mut h: History<EditAction> = History::default();
        h.push(tiles(vec![(0, 0, 1, 2)]));
        // An undo parks the action on the redo stack.
        assert!(h.undo().is_some());
        assert_eq!(h.redo.len(), 1);
        assert!(h.can_redo());
        // A new push discards that redo entry — you can't fork history.
        h.push(tiles(vec![(1, 1, 0, 3)]));
        assert!(h.redo.is_empty(), "new push invalidates redo");
        assert!(!h.can_redo());
    }

    /// Once the undo stack is full, the oldest entry is evicted so the stack stays
    /// bounded at its `limit` — a small explicit cap keeps the test cheap.
    #[test]
    fn history_caps_and_evicts_oldest() {
        let mut h: History<i32> = History::new(3);
        for n in 0..5 {
            h.push(n);
        }
        // Capped at 3, holding the three most recent pushes (2, 3, 4).
        assert_eq!(h.undo.len(), 3);
        assert_eq!(h.undo, vec![2, 3, 4]);

        // The editor's real default uses [`HISTORY_LIMIT`].
        let mut h: History<EditAction> = History::default();
        for n in 0..(HISTORY_LIMIT + 10) {
            h.push(tiles(vec![(n as i32, 0, 0, 1)]));
        }
        assert_eq!(h.undo.len(), HISTORY_LIMIT);
    }

    /// `undo`/`redo` move entries between the two stacks (returning the moved
    /// entry by reference) so a sequence of undo→redo→undo round-trips correctly.
    #[test]
    fn history_undo_redo_round_trip() {
        let mut h: History<i32> = History::new(8);
        assert!(!h.can_undo() && !h.can_redo()); // empty: nothing either way.
        h.push(10);
        h.push(20);

        // Undo the latest, then redo it back; the returned reference is the entry.
        assert_eq!(h.undo().copied(), Some(20));
        assert_eq!((h.undo.len(), h.redo.len()), (1, 1));
        assert_eq!(h.redo().copied(), Some(20));
        assert_eq!((h.undo.len(), h.redo.len()), (2, 0));
        // Nothing left to redo; undo still has both entries.
        assert!(h.redo().is_none());
        assert!(h.can_undo());

        // `clear` empties both stacks.
        h.clear();
        assert!(!h.can_undo() && !h.can_redo());
    }

    /// A [`TextField`] grows with `Push`, shrinks with `Pop`, and reports the
    /// terminal `Commit`/`Cancel` without otherwise touching its buffer — the pure
    /// char-level operations the console `step` decodes into.
    #[test]
    fn text_field_pure_ops() {
        let mut f = TextField::new("ab");
        assert_eq!(f.text(), "ab");
        // Push appends and stays active.
        assert_eq!(f.apply(TextOp::Push('c')), TextEvent::Active);
        assert_eq!(f.text(), "abc");
        // Pop deletes the last char.
        assert_eq!(f.apply(TextOp::Pop), TextEvent::Active);
        assert_eq!(f.text(), "ab");
        // Pop past empty is harmless.
        let mut empty = TextField::default();
        assert_eq!(empty.apply(TextOp::Pop), TextEvent::Active);
        assert_eq!(empty.text(), "");
        // Commit/Cancel are terminal and leave the buffer for the caller to read.
        assert_eq!(f.apply(TextOp::Commit), TextEvent::Commit);
        assert_eq!(f.text(), "ab", "commit doesn't alter the buffer");
        assert_eq!(f.apply(TextOp::Cancel), TextEvent::Cancel);
        assert_eq!(f.text(), "ab", "cancel doesn't alter the buffer");
    }

    /// The save status transitions dirty → saved (toast) → expired across ticks,
    /// and the save button reflects each phase, with the toast taking priority.
    #[test]
    fn save_status_dirty_saved_toast_expiry() {
        let mut s = SaveStatus::default();
        assert!(matches!(s.button(), SaveButton::Clean));

        // An edit marks it dirty.
        s.edited();
        assert!(s.dirty);
        assert!(matches!(s.button(), SaveButton::Dirty));

        // Saving clears dirty and starts the toast; the toast wins over dirtiness.
        s.saved();
        assert!(!s.dirty);
        assert_eq!(s.toast, SAVE_TOAST_FRAMES);
        assert!(matches!(s.button(), SaveButton::Toast));
        s.edited(); // even re-dirtied this frame, the toast still shows.
        assert!(matches!(s.button(), SaveButton::Toast));

        // Ticking the toast down to zero expires it; dirtiness shows through again.
        for _ in 0..SAVE_TOAST_FRAMES {
            s.tick();
        }
        assert_eq!(s.toast, 0);
        assert!(matches!(s.button(), SaveButton::Dirty));
        // Ticking an expired toast is a harmless no-op (saturating).
        s.tick();
        assert_eq!(s.toast, 0);
    }

    /// Stepping the viewer with a *different* map under it drops all per-map
    /// state (history, selection, text focus) — object undo entries and the
    /// selection index would otherwise replay against the new map's objects
    /// list. Same-map steps keep everything.
    #[test]
    fn map_change_resets_per_map_editor_state() {
        use crate::system::test_console::TestConsole;

        let mut console = TestConsole::new();
        let mut store = MapStore::default();
        let screen = (240.0, 136.0);

        let mut viewer = MapViewer::default();
        let mut map_a = MapInfo {
            source: "a".to_string(),
            ..MapInfo::default()
        };
        viewer.step_map_viewer_at(&mut console, &mut map_a, &mut store, Vec2::new(0, 0), screen, (0, 0));

        // Seed per-map state on map "a".
        viewer.record(tiles(vec![(0, 0, 1, 2)]));
        viewer.selected = Some(0);
        viewer.editing = Some(EditField::Key);
        viewer.field = Some(TextField::new("x"));

        // Stepping the same map keeps it all.
        viewer.step_map_viewer_at(&mut console, &mut map_a, &mut store, Vec2::new(0, 0), screen, (0, 0));
        assert!(viewer.history.can_undo());
        assert!(viewer.is_typing());
        assert_eq!(viewer.selected, Some(0));

        // Stepping a different map drops it.
        let mut map_b = MapInfo {
            source: "b".to_string(),
            ..MapInfo::default()
        };
        viewer.step_map_viewer_at(&mut console, &mut map_b, &mut store, Vec2::new(0, 0), screen, (0, 0));
        assert!(!viewer.history.can_undo(), "object undo entries went stale");
        assert!(!viewer.is_typing(), "text focus dropped");
        assert_eq!(viewer.selected, None, "selection index went stale");
    }

    /// Stepping then drawing a layout that exercises docked panels, a floating
    /// panel, the global bar, splitters and a drop-zone highlight must not panic —
    /// coverage for `build_panel`/`draw_at` across the dock features.
    #[test]
    fn draw_across_dock_features_does_not_panic() {
        use crate::system::test_console::TestConsole;

        let mut console = TestConsole::new();
        let mut draw = DrawState::default();
        let mut store = MapStore::default();
        let screen = (240.0, 136.0);
        let mut map = MapInfo::default();

        let mut viewer = MapViewer { focused: true, show_grid: true, ..Default::default() };
        viewer.dock.toggle_panel(PanelKind::Maps); // open the Maps panel too
        viewer.dock.toggle_panel(PanelKind::Map); // and the Setup panel
        viewer.dock.set_float(1, Vec2::new(100, 30), 80, 60); // float the Paint panel
        viewer.step_map_viewer_at(&mut console, &mut map, &mut store, Vec2::new(0, 0), screen, (0, 0));
        // Force the drop-zone highlight branch.
        viewer.dock.solved.hot_edge = Some(Side::Right);
        viewer.draw_at(&mut draw, &mut console, &map, &store, Vec2::new(0, 0));
    }

    /// Create → duplicate → rename → delete a map, checking the store and the
    /// written manifest stay consistent at each step (native file path).
    #[test]
    fn map_crud_round_trip() {
        use crate::system::test_console::TestConsole;

        let mut console = TestConsole::new();
        let mut maps = MapStore::default();
        let mut viewer = MapViewer::default();

        let manifest = |c: &TestConsole| -> Vec<String> {
            manifest_from_json(c.files.get("game.manifest").expect("manifest written"))
                .expect("manifest parses")
                .maps
        };

        // Create at an explicit size.
        viewer.create_map(&mut console, &mut maps, "newmap", 20, 15);
        assert!(maps.is_modern("newmap"));
        assert_eq!(maps.get("newmap").map(|m| (m.width, m.height)), Some((20, 15)));
        assert!(console.files.contains_key("maps/newmap.tmj"));
        assert!(manifest(&console).contains(&"newmap".to_string()));

        // Duplicate the selected map.
        viewer.maps_selected = Some("newmap".to_string());
        viewer.duplicate_map(&mut console, &mut maps, "newmap");
        assert!(maps.contains("newmap_copy"));
        assert!(console.files.contains_key("maps/newmap_copy.tmj"));

        // Rename.
        viewer.rename_map(&mut console, &mut maps, "newmap", "renamed");
        assert!(!maps.contains("newmap"));
        assert!(maps.contains("renamed"));
        assert!(console.files.contains_key("maps/renamed.tmj"));
        let m = manifest(&console);
        assert!(m.contains(&"renamed".to_string()));
        assert!(!m.contains(&"newmap".to_string()));

        // Delete.
        viewer.delete_map(&mut console, &mut maps, "renamed");
        assert!(!maps.contains("renamed"));
        assert!(!manifest(&console).contains(&"renamed".to_string()));
    }

    /// The Select tool's clipboard ops: copy lifts the marquee's tiles, paste
    /// stamps them at a new origin as one undo step, cut clears the source while
    /// keeping the buffer, and a collision-layer edit flags an immediate re-derive.
    #[test]
    fn select_copy_cut_paste_and_delete() {
        let mut maps = MapStore::default();
        maps.insert("m", crate::data::tmj::TiledMap::blank_modern(6, 4));
        // A 2×2 block of tile 5 at the origin on the drawable layer (source 1).
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            maps.get_mut("m").unwrap().set(1, x, y, 5);
        }
        let map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo { source_layer: 0, ..LayerInfo::DEFAULT_LAYER }, // collision
                LayerInfo { source_layer: 1, ..LayerInfo::DEFAULT_LAYER }, // drawable
            ],
            ..MapInfo::default()
        };
        let mut viewer = MapViewer { tool: EditorTool::Select, layer_index: 1, ..Default::default() };

        // Copy the 2×2 block.
        viewer.selection = Some(TileSelection { x: 0, y: 0, w: 2, h: 2 });
        viewer.selection_copy(&maps, &map);
        assert_eq!(viewer.clipboard.as_ref().map(|c| (c.w, c.h)), Some((2, 2)));
        assert_eq!(viewer.clipboard.as_ref().unwrap().tiles, vec![5, 5, 5, 5]);

        // Paste at (3,1): the block lands there as one undo step.
        viewer.selection = Some(TileSelection { x: 3, y: 1, w: 1, h: 1 });
        viewer.selection_paste(&mut maps, &map);
        let m = maps.get("m").unwrap();
        assert_eq!(
            [m.get(1, 3, 1), m.get(1, 4, 1), m.get(1, 3, 2), m.get(1, 4, 2)],
            [Some(5), Some(5), Some(5), Some(5)]
        );
        assert!(viewer.history.can_undo(), "paste records an undo step");

        // Cut the original: it clears, but the clipboard still holds the block.
        viewer.selection = Some(TileSelection { x: 0, y: 0, w: 2, h: 2 });
        viewer.selection_cut(&mut maps, &map);
        let m = maps.get("m").unwrap();
        assert_eq!([m.get(1, 0, 0), m.get(1, 1, 1)], [Some(0), Some(0)]);
        assert_eq!(viewer.clipboard.as_ref().unwrap().tiles, vec![5, 5, 5, 5]);

        // Deleting on the collision layer (index 0) clears + flags a re-derive.
        maps.get_mut("m").unwrap().set(0, 0, 0, 9);
        viewer.layer_index = 0;
        viewer.selection = Some(TileSelection { x: 0, y: 0, w: 2, h: 2 });
        viewer.pending_reload = false;
        viewer.selection_delete(&mut maps, &map);
        assert_eq!(maps.get("m").unwrap().get(0, 0, 0), Some(0));
        assert!(viewer.pending_reload, "collision edits re-derive colliders");

        // Undoing that collision edit must also re-derive (restore tile + flag).
        let mut map = map; // undo/redo take &mut MapInfo
        viewer.pending_reload = false;
        viewer.undo(&mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().get(0, 0, 0), Some(9), "undo restores the tile");
        assert!(viewer.pending_reload, "undoing a collision edit re-derives too");
    }

    /// Editing an object's hitbox x/y/w/h commits to the box and is undoable;
    /// w/h keep a 1px floor.
    #[test]
    fn object_hitbox_fields_edit_and_undo() {
        let mut maps = MapStore::default();
        let mut map = MapInfo {
            objects: vec![MapObject::dialogue(Hitbox::new(10, 10, 16, 16), "k")],
            ..MapInfo::default()
        };
        let mut v = MapViewer { selected: Some(0), ..Default::default() };

        let edit = |v: &mut MapViewer, map: &mut MapInfo, maps: &mut MapStore, field: EditField, text: &str| {
            v.editing = Some(field);
            v.field = Some(TextField::new(text));
            v.commit_edit(map, maps);
            v.stop_editing();
        };
        edit(&mut v, &mut map, &mut maps, EditField::HitX, "40");
        edit(&mut v, &mut map, &mut maps, EditField::HitY, "24");
        edit(&mut v, &mut map, &mut maps, EditField::HitW, "8");
        edit(&mut v, &mut map, &mut maps, EditField::HitH, "0"); // floored to 1
        let hb = map.objects[0].hitbox;
        assert_eq!((hb.x, hb.y, hb.w, hb.h), (40, 24, 8, 1));

        // Each edit is one undo step; undoing the last reverts just the height.
        v.undo(&mut map, &mut maps);
        assert_eq!(map.objects[0].hitbox.h, 16);
        assert_eq!(map.objects[0].hitbox.w, 8);
    }

    /// Adding a layer is undoable (and redoable), and deleting a layer restores
    /// its tile content on undo.
    #[test]
    fn layer_ops_are_undoable() {
        use crate::data::tmj::TiledMap;
        let mut maps = MapStore::default();
        maps.insert("m", TiledMap::blank_modern(4, 4));
        let n0 = maps.get("m").unwrap().layers.len(); // collision + Layer 1 + objects
        let mut map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo { source_layer: 0, ..LayerInfo::DEFAULT_LAYER },
                LayerInfo { source_layer: 1, ..LayerInfo::DEFAULT_LAYER },
            ],
            ..MapInfo::default()
        };
        let mut v = MapViewer::default();

        // Add (mirror the handler): record the insert.
        let index = maps.get_mut("m").unwrap().add_tile_layer("Layer 2");
        let layer = Box::new(maps.get_mut("m").unwrap().layers[index].clone());
        v.record(EditAction::LayerInsert { source: "m".to_string(), index, layer });
        assert_eq!(maps.get("m").unwrap().layers.len(), n0 + 1);

        v.undo(&mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layers.len(), n0);
        assert!(v.pending_reload, "layer undo re-derives");
        v.redo(&mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layers.len(), n0 + 1);

        // Delete a tile layer holding content; undo brings it (and the tile) back.
        maps.get_mut("m").unwrap().set(1, 0, 0, 7);
        let removed = maps.get_mut("m").unwrap().remove_layer_at(1).unwrap();
        v.record(EditAction::LayerRemove { source: "m".to_string(), index: 1, layer: Box::new(removed) });
        assert_eq!(maps.get("m").unwrap().layers.len(), n0); // back down one
        v.undo(&mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().get(1, 0, 0), Some(7), "content restored");

        // The collision layer (index 0) is protected — remove returns None.
        assert!(maps.get_mut("m").unwrap().remove_layer_at(0).is_none());
    }

    /// Renaming a layer commits to the store and is undoable; the FG toggle flips
    /// the `fg` name prefix (and back), itself undoable.
    #[test]
    fn layer_rename_and_fg_toggle() {
        use crate::data::tmj::TiledMap;
        let mut maps = MapStore::default();
        maps.insert("m", TiledMap::blank_modern(4, 4));
        let mut map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo { source_layer: 0, ..LayerInfo::DEFAULT_LAYER },
                LayerInfo { source_layer: 1, ..LayerInfo::DEFAULT_LAYER },
            ],
            ..MapInfo::default()
        };
        let mut v = MapViewer { layer_index: 1, ..Default::default() };

        // Rename "Layer 1" -> "water" via the begin/commit flow.
        v.begin_layer_rename(&maps, "m", 1);
        assert_eq!(v.editing, Some(EditField::LayerName));
        v.field = Some(TextField::new("water"));
        v.commit_edit(&mut map, &mut maps);
        v.stop_editing();
        assert_eq!(maps.get("m").unwrap().layer_name(1), Some("water"));
        v.undo(&mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layer_name(1), Some("Layer 1"));
        v.redo(&mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layer_name(1), Some("water"));

        // FG toggle adds the `fg` prefix; a second toggle strips it; both undoable.
        v.toggle_layer_fg(&map, &mut maps, 1);
        assert_eq!(maps.get("m").unwrap().layer_name(1), Some("fg water"));
        v.toggle_layer_fg(&map, &mut maps, 1);
        assert_eq!(maps.get("m").unwrap().layer_name(1), Some("water"));
        v.undo(&mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layer_name(1), Some("fg water"));

        // An empty rename is ignored (a layer stays identifiable).
        v.rename_target = Some(1);
        v.editing = Some(EditField::LayerName);
        v.field = Some(TextField::new("   "));
        v.commit_edit(&mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layer_name(1), Some("fg water"));
    }

    /// The warp-target picker steps through `[same-map] + existing maps` and
    /// wraps, recording each step for undo.
    #[test]
    fn warp_target_cycles_through_maps() {
        use crate::data::tmj::TiledMap;
        let mut maps = MapStore::default();
        maps.insert("a", TiledMap::blank_modern(4, 4));
        maps.insert("b", TiledMap::blank_modern(4, 4));
        let mut map = MapInfo {
            objects: vec![MapObject::warp(
                Hitbox::new(0, 0, 8, 8),
                Warp::new(None, Vec2::new(0, 0)),
            )],
            ..MapInfo::default()
        };
        let mut v = MapViewer { selected: Some(0), ..Default::default() };
        let target = |map: &MapInfo| match &map.objects[0].effect {
            ObjectEffect::Warp(w) => w.map.clone(),
            _ => panic!("the object is a warp"),
        };

        assert_eq!(target(&map), None); // same-map
        v.cycle_warp_target(&mut map, &maps);
        assert_eq!(target(&map).as_deref(), Some("a"));
        v.cycle_warp_target(&mut map, &maps);
        assert_eq!(target(&map).as_deref(), Some("b"));
        v.cycle_warp_target(&mut map, &maps); // wraps back to same-map
        assert_eq!(target(&map), None);
        assert!(v.history.can_undo(), "each pick is an undo step");
    }

    /// The warp preview's letterbox fit and its inverse agree: a click at the
    /// centre of a placed map pixel round-trips back to that pixel. This is the
    /// contract the draw (where to blit / mark) and the click handler (how to
    /// invert a click to a coordinate) both depend on.
    #[test]
    fn warp_preview_fit_inverts() {
        // A 240×136 map letterboxed into an 82×64 box: downscales, centres.
        let outer = Rect { x: 10, y: 20, w: 82, h: 64 };
        let (fw, fh) = (240u32, 136u32);
        let (inner, s) = fit_preview(outer, fw, fh);
        assert!(s > 0.0 && s < 1.0, "a large map downscales: {s}");
        // The inner map sits inside the box, centred (letterboxed).
        assert!(inner.w <= outer.w && inner.h <= outer.h);
        assert!(inner.x >= outer.x && inner.y >= outer.y);

        // Click the middle of where map pixel (100, 50) renders → recover (100,50).
        let (mx, my) = (100i16, 50i16);
        let cursor = Vec2::new(
            inner.x + (mx as f32 * s) as i16,
            inner.y + (my as f32 * s) as i16,
        );
        let inv_x = (((cursor.x - inner.x) as f32) / s) as i16;
        let inv_y = (((cursor.y - inner.y) as f32) / s) as i16;
        // Within one source pixel (the scale's quantisation).
        assert!((inv_x - mx).abs() <= 1, "x round-trips: {inv_x} vs {mx}");
        assert!((inv_y - my).abs() <= 1, "y round-trips: {inv_y} vs {my}");

        // A tiny map (smaller than the box) is shown 1:1, not upscaled, and centred.
        let (inner, s) = fit_preview(outer, 16, 16);
        assert_eq!(s, 1.0, "downscale only — a small map stays 1:1");
        assert_eq!((inner.w, inner.h), (16, 16));
        assert_eq!(inner.x, outer.x + (outer.w - 16) / 2);
    }

    /// A scroll-kind panel whose content overflows scrolls, with the offset
    /// clamped to the overflow; panels that page/scroll their own viewport (Maps,
    /// Paint) never panel-scroll.
    #[test]
    fn panel_scroll_clamps_and_skips_non_scroll_kinds() {
        let mut v = MapViewer::default();
        let rect = Rect { x: 0, y: 0, w: 84, h: 40 };
        // Layers (idx 0) is a scroll kind: a 100px content over a 40px panel
        // overflows 60px, and an over-large stored scroll clamps to it.
        v.dock.set_scroll(0, 500);
        assert_eq!(v.panel_scroll(0, rect, 100), (60, true));
        // Content that fits doesn't scroll.
        assert_eq!(v.panel_scroll(0, rect, 30), (0, false));
        // Paint (idx 1) and Maps (idx 3) opt out — their own widgets handle size.
        v.dock.set_scroll(1, 500);
        assert_eq!(v.panel_scroll(1, rect, 100), (0, false));
        v.dock.set_scroll(3, 500);
        assert_eq!(v.panel_scroll(3, rect, 100), (0, false));
    }

    /// The text field's caret: arrow motion, insert/delete at the cursor, word
    /// motion over whitespace, and ctrl-backspace clearing the buffer. `display`
    /// shows the caret as `_` at its position.
    #[test]
    fn text_field_cursor_editing() {
        let mut f = TextField::new("cat");
        assert_eq!(f.display(), "cat_", "caret starts at the end");
        f.apply(TextOp::Left);
        f.apply(TextOp::Left);
        assert_eq!(f.display(), "c_at");
        f.apply(TextOp::Push('X'));
        assert_eq!(f.text(), "cXat");
        assert_eq!(f.display(), "cX_at", "insert lands at the caret");
        f.apply(TextOp::Pop);
        assert_eq!(f.text(), "cat", "backspace deletes before the caret");
        assert_eq!(f.display(), "c_at");

        // Word motion skips a run of whitespace then a run of word characters.
        let mut g = TextField::new("foo bar baz");
        g.apply(TextOp::WordLeft);
        assert_eq!(g.display(), "foo bar _baz");
        g.apply(TextOp::WordLeft);
        assert_eq!(g.display(), "foo _bar baz");
        g.apply(TextOp::WordRight);
        assert_eq!(g.display(), "foo bar_ baz");
        // Ctrl+Backspace empties the field.
        g.apply(TextOp::Clear);
        assert_eq!((g.text(), g.display().as_str()), ("", "_"));
    }

    /// A tile layer's offset / palette-rotation fields edit the store and are
    /// undoable, one step each.
    #[test]
    fn layer_offset_and_rotate_edit_and_undo() {
        use crate::data::tmj::TiledMap;
        let mut maps = MapStore::default();
        maps.insert("m", TiledMap::blank_modern(4, 4));
        let mut map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo { source_layer: 0, ..LayerInfo::DEFAULT_LAYER },
                LayerInfo { source_layer: 1, ..LayerInfo::DEFAULT_LAYER },
            ],
            ..MapInfo::default()
        };
        let mut v = MapViewer { layer_index: 1, ..Default::default() };
        let edit = |v: &mut MapViewer, map: &mut MapInfo, maps: &mut MapStore, field: EditField, text: &str| {
            v.rename_target = Some(1);
            v.editing = Some(field);
            v.field = Some(TextField::new(text));
            v.commit_edit(map, maps);
            v.stop_editing();
        };
        edit(&mut v, &mut map, &mut maps, EditField::LayerOffX, "3");
        edit(&mut v, &mut map, &mut maps, EditField::LayerOffY, "-2");
        edit(&mut v, &mut map, &mut maps, EditField::LayerRotate, "5");
        assert_eq!(maps.get("m").unwrap().layer_offset(1), Some((3.0, -2.0)));
        assert_eq!(maps.get("m").unwrap().layer_palette_rotate(1), 5);

        v.undo(&mut map, &mut maps); // rotation
        assert_eq!(maps.get("m").unwrap().layer_palette_rotate(1), 0);
        v.undo(&mut map, &mut maps); // y offset
        assert_eq!(maps.get("m").unwrap().layer_offset(1), Some((3.0, 0.0)));
        v.redo(&mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layer_offset(1), Some((3.0, -2.0)));
    }

    /// Rotation edits normalise mod-16 (so revert is exact even for a
    /// hand-authored value > 15), and a non-finite offset is rejected.
    #[test]
    fn layer_prop_edits_normalise_and_reject_bad_input() {
        use crate::data::tmj::TiledMap;
        let mut maps = MapStore::default();
        maps.insert("m", TiledMap::blank_modern(4, 4));
        maps.get_mut("m").unwrap().set_layer_palette_rotate(1, 20); // out of range, as if hand-authored
        let mut map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo { source_layer: 0, ..LayerInfo::DEFAULT_LAYER },
                LayerInfo { source_layer: 1, ..LayerInfo::DEFAULT_LAYER },
            ],
            ..MapInfo::default()
        };
        let mut v = MapViewer { layer_index: 1, ..Default::default() };
        let commit = |v: &mut MapViewer, map: &mut MapInfo, maps: &mut MapStore, field: EditField, text: &str| {
            v.rename_target = Some(1);
            v.editing = Some(field);
            v.field = Some(TextField::new(text));
            v.commit_edit(map, maps);
            v.stop_editing();
        };

        commit(&mut v, &mut map, &mut maps, EditField::LayerRotate, "5");
        assert_eq!(maps.get("m").unwrap().layer_palette_rotate(1), 5);
        v.undo(&mut map, &mut maps);
        // Restores 20 mod 16 = 4 (the normalised prior), not clamp(20)=15.
        assert_eq!(maps.get("m").unwrap().layer_palette_rotate(1), 4);

        // NaN / inf typed into an offset are rejected — the offset stays put.
        commit(&mut v, &mut map, &mut maps, EditField::LayerOffX, "NaN");
        commit(&mut v, &mut map, &mut maps, EditField::LayerOffX, "inf");
        assert_eq!(maps.get("m").unwrap().layer_offset(1), Some((0.0, 0.0)));
    }

    /// The Setup music picker steps through `[none] + the available tracks` (the
    /// host's music-dir listing) and wraps.
    #[test]
    fn music_picker_cycles_tracks() {
        use crate::data::tmj::TiledMap;
        let mut maps = MapStore::default();
        maps.insert("m", TiledMap::blank_modern(2, 2));
        let map = MapInfo { source: "m".to_string(), ..MapInfo::default() };
        let mut v = MapViewer::default();
        let tracks = vec!["filler".to_string(), "intro".to_string()];
        let music = |maps: &MapStore| maps.get("m").unwrap().music().map(str::to_string);

        assert_eq!(music(&maps), None);
        v.cycle_music(&map, &mut maps, &tracks);
        assert_eq!(music(&maps).as_deref(), Some("filler"));
        v.cycle_music(&map, &mut maps, &tracks);
        assert_eq!(music(&maps).as_deref(), Some("intro"));
        v.cycle_music(&map, &mut maps, &tracks); // wraps back to none
        assert_eq!(music(&maps), None);
        // An empty track list keeps it at none (no host music dir).
        v.cycle_music(&map, &mut maps, &[]);
        assert_eq!(music(&maps), None);
    }

    /// The fg-prefix toggle round-trips a separated marker, leaves a glued word
    /// alone (no corruption), and is case-insensitive.
    #[test]
    fn fg_prefix_toggle_edge_cases() {
        assert_eq!(toggle_fg_prefix("water"), "fg water");
        assert_eq!(toggle_fg_prefix("fg water"), "water"); // round-trips
        assert_eq!(toggle_fg_prefix("FG Bed"), "Bed"); // case-insensitive marker
        assert_eq!(toggle_fg_prefix("fgrass"), "fgrass"); // glued word: untouched
        assert_eq!(toggle_fg_prefix("fg"), "layer"); // bare prefix -> fallback
    }

    /// A name collision, a path-separator name and an empty name are all rejected.
    #[test]
    fn new_map_name_validation() {
        let mut maps = MapStore::default();
        maps.insert("town", crate::data::tmj::TiledMap::blank_modern(4, 4));
        assert!(valid_map_name("forest", &maps));
        assert!(!valid_map_name("town", &maps)); // already exists
        assert!(!valid_map_name("a/b", &maps)); // path separator
        assert!(!valid_map_name("", &maps)); // empty
        assert_eq!(dedup_name("town", &maps), "town_copy");

        // Dimension parsing: valid in-range, else the default; clamps the range.
        assert_eq!(parse_dim("48", 30), 48);
        assert_eq!(parse_dim("", 30), 30);
        assert_eq!(parse_dim("nope", 30), 30);
        assert_eq!(parse_dim("0", 30), 30); // below min
        assert_eq!(parse_dim("9999", 30), 30); // above max
    }

    /// The fixed-32 palette maps a cursor to the right sheet tile, box-selects a
    /// brush, clamps a drag that runs off the viewport, and bounds scroll so the
    /// last column/row can reach the edge.
    #[test]
    fn palette_box_select_and_scroll_bounds() {
        let mut v = MapViewer {
            pal_rect: Rect { x: 4, y: 20, w: 80, h: 64 }, // 10 cols x 8 rows visible
            pal_col: 5,
            pal_row: 2,
            ..Default::default()
        };
        // 3rd visible column, 1st visible row -> sheet (col 7, row 2).
        let (c, r) = v.palette_tile_at(Vec2::new(4 + 2 * 8 + 1, 20 + 1));
        assert_eq!((c, r), (7, 2));
        v.set_brush_box(c, r, c, r); // a click is a 1x1 brush
        assert_eq!(v.selected_tile, 2 * v.sheet_cols() + 7);
        assert_eq!(v.brush_size(), (1, 1));
        // Drag a 3x2 box from (7,2) to (9,3): top-left tile + size.
        v.set_brush_box(7, 2, 9, 3);
        assert_eq!(v.selected_tile, 2 * v.sheet_cols() + 7);
        assert_eq!(v.brush_size(), (3, 2));
        // A point off the viewport clamps to the last visible tile, not wraps.
        assert_eq!(v.palette_tile_at(Vec2::new(500, 500)), (5 + 10 - 1, 2 + 8 - 1));
        // Scroll bounds: 10 of 32 cols, 8 of the sheet's 128 rows visible.
        assert_eq!(v.palette_scroll_max(), (32 - 10, 128 - 8));
    }

    /// A scroll-bar drag preserves the grab offset: the thumb tracks the cursor
    /// instead of snapping its top under it. (A bigger grab offset at the same
    /// cursor scrolls less, and the old behaviour ignored grab entirely.)
    #[test]
    fn scroll_bar_drag_preserves_grab_offset() {
        let base = MapViewer {
            pal_rect: Rect { x: 0, y: 0, w: 80, h: 80 }, // 10x10 visible
            sheet: (10, 30),                              // 10 cols, 30 rows -> max_r = 20
            pal_row: 5,
            ..Default::default()
        };
        assert_eq!(base.palette_scroll_max(), (0, 20));
        let (_, travel) = base.palette_thumb_v();
        assert!(travel > 0);

        // Same cursor y, different grab: the larger offset puts the thumb (and so
        // pal_row) higher — proof the offset shifts the thumb, not the cursor.
        let mut top_grab = base.clone();
        top_grab.scroll_palette_bar(true, Vec2::new(0, 40), 0);
        let mut mid_grab = base.clone();
        mid_grab.scroll_palette_bar(true, Vec2::new(0, 40), 20);
        assert!(mid_grab.pal_row < top_grab.pal_row);

        // Same grab, cursor moves down: pal_row follows it (moves *with* the mouse).
        let mut near = base.clone();
        near.scroll_palette_bar(true, Vec2::new(0, 20), 10);
        let mut far = base.clone();
        far.scroll_palette_bar(true, Vec2::new(0, 40), 10);
        assert!(far.pal_row > near.pal_row);

        // Dragging past the end clamps to max_r — no overscroll.
        let mut overshoot = base.clone();
        overshoot.scroll_palette_bar(true, Vec2::new(0, 1000), 0);
        assert_eq!(overshoot.pal_row, 20);
    }

    /// The palette adapts to the live sheet size (passed in by the host each
    /// step): it falls back to the current sheet until set, then reflects a
    /// grown sheet immediately.
    #[test]
    fn palette_adapts_to_sheet_size() {
        let mut v = MapViewer {
            pal_rect: Rect { x: 0, y: 0, w: 80, h: 64 }, // 10 x 8 tiles visible
            ..Default::default()
        };
        assert_eq!((v.sheet_cols(), v.sheet_rows()), (32, 128)); // fallback
        v.sheet = (40, 200); // a bigger sheet after an art update
        assert_eq!(v.sheet_cols(), 40);
        assert_eq!(v.sheet_tiles(), 8000);
        assert_eq!(v.palette_scroll_max(), (40 - 10, 200 - 8));
    }

    /// Cycling an interaction reaches every authorable kind — the GUI's way to
    /// place Func interactions (toggle_dog / piano / note / add_creatures).
    #[test]
    fn interaction_kind_cycles_through_func_variants() {
        let o = Vec2::new(5, 7);
        let mut i = Interaction::None;
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::Dialogue(_)));
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::Func(InteractFn::ToggleDog)));
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::Func(InteractFn::Piano(p)) if p == o));
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::Func(InteractFn::Note(0))));
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::Func(InteractFn::AddCreatures(0))));
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::None));
        assert_eq!(
            interaction_kind_label(&Interaction::Func(InteractFn::Note(3))),
            "note"
        );
        assert_eq!(interaction_kind_label(&Interaction::None), "none");
    }

    /// A primary editor saves its dock arrangement and a fresh primary restores
    /// it; a view editor (non-persistent) is gated off.
    #[test]
    fn layout_persists_round_trip() {
        use crate::system::test_console::TestConsole;

        let mut console = TestConsole::new();
        let mut a = MapViewer::primary();
        a.dock.set_side_thickness(Side::Left, 50);
        a.dock.toggle_panel(PanelKind::Maps); // open Maps (closed by default)
        a.save_layout(&mut console);
        assert!(console.files.contains_key(LAYOUT_PATH));

        let mut b = MapViewer::primary();
        b.load_layout(&mut console);
        assert!(
            b.dock.panels.iter().any(|p| p.kind == PanelKind::Maps && p.open),
            "Maps stays open after reload",
        );
        assert!(
            b.dock
                .panels
                .iter()
                .any(|p| matches!(p.place, Placement::Dock { side: Side::Left, size: 50 })),
            "left dock thickness restored",
        );

        // View editors never persist.
        assert!(!MapViewer::default().persist);
        assert!(MapViewer::primary().persist);
    }

    /// Rendering a thumbnail for a blank modern map produces an image that fits
    /// the cell — panic coverage for the P3 preview render path.
    #[test]
    fn thumbnail_renders_within_the_cell() {
        // A real sheet so the modern-map collider derivation has art to read.
        let draw = DrawState {
            indexed_sprites: crate::system::drawing::image::IndexedImage::new(256, 256),
            ..Default::default()
        };
        let mut maps = MapStore::default();
        maps.insert("m", crate::data::tmj::TiledMap::blank_modern(10, 8));

        let thumb = render_map_thumbnail("m", &maps, &draw, 40, 22).expect("thumbnail");
        assert!(thumb.width() <= 40 && thumb.height() <= 22);
        assert!(thumb.width() >= 1 && thumb.height() >= 1);
    }

    /// `tile_bounds` returns an inclusive, normalised tile range regardless of
    /// drag direction — the basis for rectangle fill.
    #[test]
    fn tile_bounds_normalises_and_snaps() {
        // (3..=20) px on x spans tiles 0..=2; y from 9..=1 normalises and snaps.
        assert_eq!(tile_bounds(Vec2::new(20, 1), Vec2::new(3, 9)), (0, 0, 2, 1),);
        // A point within one tile is a 1x1 range.
        assert_eq!(tile_bounds(Vec2::new(4, 4), Vec2::new(7, 7)), (0, 0, 0, 0));
    }

    /// `hitbox_between` spans two points with a 1px minimum (so a click without a
    /// drag still yields a valid, non-panicking hitbox).
    #[test]
    fn hitbox_between_min_size() {
        let h = hitbox_between(Vec2::new(10, 20), Vec2::new(4, 5));
        assert_eq!((h.x, h.y, h.w, h.h), (4, 5, 6, 15));
        let dot = hitbox_between(Vec2::new(7, 7), Vec2::new(7, 7));
        assert_eq!((dot.w, dot.h), (1, 1));
    }

    /// The dialogue key of an object's interaction effect, or `""` otherwise.
    fn dialogue_key(object: &MapObject) -> &str {
        match &object.effect {
            ObjectEffect::Interact(Interaction::Dialogue(k)) => k.as_str(),
            _ => "",
        }
    }

    /// `move_snapshot` relocates an object's origin without touching its size or
    /// payload, so a drag's "before" snapshot is exact for undo.
    #[test]
    fn move_snapshot_relocates_origin() {
        let it = MapObject::dialogue(Hitbox::new(40, 50, 16, 8), "k");
        let out = move_snapshot(it, Vec2::new(1, 2));
        assert_eq!((out.hitbox.x, out.hitbox.y), (1, 2));
        assert_eq!((out.hitbox.w, out.hitbox.h), (16, 8)); // size preserved
        assert_eq!(dialogue_key(&out), "k"); // payload untouched
    }

    /// `snapshot_eq` is true only for identical editable content, so no-op edits
    /// aren't recorded as undo steps.
    #[test]
    fn snapshot_eq_detects_changes() {
        let a = MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "x");
        let same = MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "x");
        let diff_key = MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "y");
        let diff_box = MapObject::dialogue(Hitbox::new(1, 0, 8, 8), "x");
        assert!(snapshot_eq(&a, &same));
        assert!(!snapshot_eq(&a, &diff_key));
        assert!(!snapshot_eq(&a, &diff_box));
        // Cross-kind (interaction vs. warp) never compares equal.
        let warp = MapObject::warp(Hitbox::new(0, 0, 8, 8), Warp::new(None, Vec2::new(0, 0)));
        assert!(!snapshot_eq(&a, &warp));
    }

    /// `snapshot_eq` detects the two new editable fields: a trigger change (on the
    /// MapObject, either tab) and a narration change (on the Warp) — so the editor
    /// records those edits as undo steps via [`MapViewer::modify_object`].
    #[test]
    fn snapshot_eq_detects_trigger_and_narration_edits() {
        // Trigger lives on the object, so a trigger-only change is detected on an
        // interaction snapshot whose key/box are otherwise identical.
        let base = MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "x");
        let retriggered = base.clone().with_trigger(Trigger::Touch);
        assert!(!snapshot_eq(&base, &retriggered), "trigger edit detected");

        // Narration lives on the Warp; a narration-only change is detected too.
        let warp = MapObject::warp(Hitbox::new(0, 0, 8, 8), Warp::new(None, Vec2::new(0, 0)));
        let narrated =
            MapObject::warp(Hitbox::new(0, 0, 8, 8), Warp::new(None, Vec2::new(0, 0)).with_narration("creak"));
        assert!(!snapshot_eq(&warp, &narrated), "narration edit detected");
        // Same narration compares equal (no spurious undo entry).
        let narrated2 =
            MapObject::warp(Hitbox::new(0, 0, 8, 8), Warp::new(None, Vec2::new(0, 0)).with_narration("creak"));
        assert!(snapshot_eq(&narrated, &narrated2));
    }

    /// Object add/remove undo replays into the one list at the right index:
    /// undo of a remove re-inserts the exact object, undo of an add removes it.
    #[test]
    fn object_insert_remove_round_trip() {
        let mut map = MapInfo::default();
        map.objects
            .push(MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "a"));
        map.objects
            .push(MapObject::dialogue(Hitbox::new(8, 0, 8, 8), "b"));

        // Snapshot + remove index 0, then re-insert it: list shape is restored.
        let snap = MapViewer::snapshot(&map, 0).unwrap();
        remove_object(&mut map, 0);
        assert_eq!(map.objects.len(), 1);
        insert_object(&mut map, 0, snap);
        assert_eq!(map.objects.len(), 2);
        assert_eq!(dialogue_key(&map.objects[0]), "a", "re-inserted at original index");

        // A past-the-end insert clamps to a push rather than panicking.
        let extra = MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "c");
        insert_object(&mut map, 99, extra);
        assert_eq!(map.objects.len(), 3);
    }

    /// Saving writes the `.tmj` *and* re-syncs the store, so leaving and
    /// re-entering the map sees the edited objects — without the sync, the disk
    /// file was right but `map_by_name` rebuilt from the stale pre-edit object
    /// layer until a restart. Attached image pixels survive the swap (they
    /// aren't serialised, so the sync carries them over by path).
    #[test]
    fn save_syncs_the_store() {
        use crate::data::tmj::{
            ImageLayer, ObjectLayer, TileLayer, TiledMap, TiledMapLayer, Tileset,
        };
        use crate::system::drawing::image::RgbaImage;
        use crate::system::test_console::TestConsole;

        let mut console = TestConsole::new();
        let mut store = MapStore::default();
        let mut map = TiledMap {
            width: 2,
            height: 2,
            layers: vec![
                TiledMapLayer::TileLayer(TileLayer {
                    width: 2,
                    height: 2,
                    data: vec![0; 4],
                    name: "collision".to_string(),
                    ..Default::default()
                }),
                TiledMapLayer::ImageLayer(ImageLayer {
                    name: "bg".to_string(),
                    image: "images/bg.png".to_string(),
                    offsetx: 0.0,
                    offsety: 0.0,
                    visible: true,
                    opacity: 1.0,
                    properties: Vec::new(),
                    pixels: None,
                }),
                TiledMapLayer::ObjectLayer(ObjectLayer {
                    name: "objects".to_string(),
                    objects: Vec::new(),
                }),
            ],
            tilesets: vec![Tileset {
                firstgid: 1,
                source: "tiles.tsj".to_string(),
            }],
            properties: Vec::new(),
        };
        map.attach_image("images/bg.png", RgbaImage::new(8, 8));
        store.insert("m", map);

        // The running MapInfo carries an object edit the store doesn't have yet.
        let info = MapInfo {
            source: "m".to_string(),
            objects: vec![MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "hello")],
            ..MapInfo::default()
        };
        let mut viewer = MapViewer::default();
        viewer.save(&mut console, &info, &mut store);

        // The file was written, and the store now parses to the edited objects.
        assert!(console.files.contains_key("maps/m.tmj"));
        let synced = store.get("m").unwrap();
        let objects = synced.parse_objects();
        assert_eq!(objects.len(), 1, "the edited object reached the store");
        assert!(matches!(
            &objects[0].effect,
            ObjectEffect::Interact(Interaction::Dialogue(key)) if key == "hello"
        ));
        // The runtime pixels survive the swap.
        assert!(
            synced.layers.iter().any(
                |l| matches!(l, TiledMapLayer::ImageLayer(i) if i.pixels.is_some())
            ),
            "attached pixels survive the sync"
        );
    }
}

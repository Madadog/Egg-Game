//! In-game editor for modern Tiled maps: toggle layers, paint tiles, and place
//! or drag map objects (warps and interactions). Opened with `L` in walkaround;
//! freezes the sim while focused and writes edits back to the map's `.tmj`.
//!
//! Warps and interactions live in one [`MapInfo::objects`] list. The two object
//! tools (Interacts / Warps) are *filtered views* over that single list — each
//! tab lists only objects of its kind, mapping its display rows to real vector
//! indices — so the UX is unchanged while the data model is unified.

use crate::{
    data::sound::{self, SfxData},
    drawstate::{DrawState, LayerId},
    interact::Interaction,
    map::{
        Axis, LayerInfo, LayerKind, MapInfo, MapObject, MapStore, ObjectEffect, Trigger, Warp,
        WarpMode,
    },
    position::{Hitbox, Vec2},
    system::{
        ConsoleApi, ConsoleHelper, MouseInput, ScanCode, drawing::Canvas, just_pressed, pressed,
    },
    ui::{NodeId, Ui, UiBuilder},
};

use super::walkaround::WalkaroundState;

/// The active editing tool. The map editor is the old layer viewer grown into a
/// tabbed tool: toggle layers, paint tiles, or place map objects. The Interacts
/// and Warps tabs are filtered views over the one objects list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorTool {
    #[default]
    Layers,
    Paint,
    Interactables,
    Warps,
}
impl EditorTool {
    const ALL: [EditorTool; 4] = [Self::Layers, Self::Paint, Self::Interactables, Self::Warps];
    fn label(self) -> &'static str {
        match self {
            Self::Layers => "Layers",
            Self::Paint => "Paint",
            Self::Interactables => "Interact",
            Self::Warps => "Warps",
        }
    }
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
    Tile(usize),
    PaletteUp,
    PaletteDown,
    Object(usize),
    NewObject,
    DeleteObject,
    Field(EditField),
    Cycle(CycleField),
    /// Selects the empty tile (0) as the brush — i.e. an eraser.
    Eraser,
    Undo,
    Redo,
    Save,
}

const PANEL_W: f32 = 84.0;
const PALETTE_COLS: usize = 9;
const PALETTE_ROWS: usize = 7;
const SHEET_TILES: usize = 2048;
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
    /// Append a character (a typed, non-control key).
    Push(char),
    /// Delete the last character (Backspace).
    Pop,
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
}

impl TextField {
    /// A field primed with `initial` as its starting contents (e.g. the existing
    /// value of the property being edited).
    fn new(initial: impl Into<String>) -> Self {
        Self {
            buffer: initial.into(),
        }
    }

    /// The current buffer contents (for rendering the in-progress `value_`).
    fn text(&self) -> &str {
        &self.buffer
    }

    /// Apply one character-level op, returning how the field resolved. Push/pop
    /// mutate the buffer and stay [`Active`](TextEvent::Active); commit/cancel
    /// leave the buffer untouched and report the terminal event for the caller.
    fn apply(&mut self, op: TextOp) -> TextEvent {
        match op {
            TextOp::Push(c) => {
                self.buffer.push(c);
                TextEvent::Active
            }
            TextOp::Pop => {
                self.buffer.pop();
                TextEvent::Active
            }
            TextOp::Commit => TextEvent::Commit,
            TextOp::Cancel => TextEvent::Cancel,
        }
    }

    /// Consume this frame's keyboard input from `system` and fold it into the
    /// buffer, returning whether the field is still active or finished.
    ///
    /// Mirrors the editor's original inline handling exactly: typed non-control
    /// characters append, Backspace deletes one, Escape cancels and Return
    /// commits. Escape takes priority over Return when (improbably) both fire in
    /// the same frame, matching the original `if Escape … else if Return` order.
    fn step(&mut self, system: &impl ConsoleApi) -> TextEvent {
        for c in system.key_chars() {
            if !c.is_control() {
                self.apply(TextOp::Push(*c));
            }
        }
        if system.keyp(ScanCode::Backspace) {
            self.apply(TextOp::Pop);
        }
        if system.keyp(ScanCode::Escape) {
            self.apply(TextOp::Cancel)
        } else if system.keyp(ScanCode::Return) {
            self.apply(TextOp::Commit)
        } else {
            TextEvent::Active
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MapViewer {
    pub focused: bool,
    pub fg: bool,
    pub layer_index: usize,
    tool: EditorTool,
    selected_tile: usize,
    palette_scroll: usize,
    selected: Option<usize>,
    drag: Option<Vec2>,
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
}

impl MapViewer {
    /// True while a text field is capturing keyboard input — the host suppresses
    /// its global debug hotkeys so typed dialogue keys don't trigger them.
    pub fn is_typing(&self) -> bool {
        self.editing.is_some()
    }

    // --- Layout (rebuilt each frame for both hit-testing and drawing) ---------

    /// Lay the editor out as a fixed-width black column: tool tabs, the active
    /// tool's controls, then a save button.
    fn build_ui(&self, map: &MapInfo, screen: (f32, f32)) -> Ui<EditorKey> {
        let mut b = UiBuilder::new();
        let mut rows: Vec<NodeId> = Vec::new();

        for tool in EditorTool::ALL {
            let selected = tool == self.tool;
            rows.push(
                b.text(tool.label())
                    .small(true)
                    .color(if selected { 0 } else { 12 })
                    .full_width(7.0)
                    .fill_if(selected, 11)
                    .key(EditorKey::Tool(tool))
                    .id(),
            );
        }
        rows.push(b.spacer(2.0).id());

        match self.tool {
            EditorTool::Layers => self.build_layers(&mut b, &mut rows, map),
            EditorTool::Paint => self.build_paint(&mut b, &mut rows),
            EditorTool::Interactables | EditorTool::Warps => {
                self.build_objects(&mut b, &mut rows, map)
            }
        }

        rows.push(b.spacer(2.0).id());
        self.build_footer(&mut b, &mut rows);

        let root = b.column(0.0, rows).size(PANEL_W, screen.1).fill(0).id();
        b.finish(root, screen)
    }

    /// The fixed bottom of the panel: an undo/redo row, the save button (with an
    /// unsaved `*` marker / save toast), and a one-line shortcut/status hint.
    fn build_footer(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>) {
        // Undo/redo, greyed when the respective stack is empty.
        let undo = b
            .text("<undo")
            .small(true)
            .center()
            .color(if self.history.can_undo() { 12 } else { 13 })
            .size(PANEL_W / 2.0 - 1.0, 7.0)
            .outlined(0, 13)
            .key(EditorKey::Undo)
            .id();
        let redo = b
            .text("redo>")
            .small(true)
            .center()
            .color(if self.history.can_redo() { 12 } else { 13 })
            .size(PANEL_W / 2.0 - 1.0, 7.0)
            .outlined(0, 13)
            .key(EditorKey::Redo)
            .id();
        rows.push(b.row(2.0, [undo, redo]).id());

        // Save button: a `*` flags unsaved edits; a transient toast confirms a
        // write. Green outline normally, amber while dirty.
        let (label, outline) = match self.status.button() {
            SaveButton::Toast => ("[  SAVED!  ]", 11),
            SaveButton::Dirty => ("[ SAVE * ]", 9),
            SaveButton::Clean => ("[ SAVE MAP ]", 6),
        };
        rows.push(
            b.text(label)
                .small(true)
                .center()
                .color(outline)
                .full_width(8.0)
                .outlined(0, outline)
                .key(EditorKey::Save)
                .id(),
        );

        // Status line: the keys most worth remembering for the active tool. Kept
        // short so it fits the 84px column; `^Z`/`^Y` are the undo/redo chords.
        let hint = match self.tool {
            EditorTool::Paint => "Rclk:erase Mclk:pick\nShift+drag:fill\n1-4:tool ^Z/^Y:undo",
            EditorTool::Interactables | EditorTool::Warps => {
                "drag:move Del:remove\n1-4:tool ^Z/^Y:undo"
            }
            EditorTool::Layers => "1-4:tool ^S:save\n^Z/^Y:undo/redo",
        };
        rows.push(b.text(hint).small(true).color(14).full_width(20.0).id());
    }

    fn build_layers(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>, map: &MapInfo) {
        let layers = if self.fg { &map.fg_layers } else { &map.layers };
        let title = if self.fg { "FG LAYERS:" } else { "BG LAYERS:" };
        rows.push(
            b.text(title)
                .color(13)
                .full_width(8.0)
                .key(EditorKey::Title)
                .id(),
        );
        for (i, layer) in layers.iter().enumerate() {
            let hidden = if layer.visible { "" } else { "(H)" };
            // Image layers are flagged `img` so the painter knows they're not a
            // paint target (paint silently refuses them; see `handle_paint`).
            let label = match layer.kind {
                LayerKind::Image => format!("img {i} {hidden}"),
                LayerKind::Tiles => format!("Layer {i} {hidden}"),
            };
            rows.push(
                b.text(label)
                    .small(true)
                    .full_width(7.0)
                    .fill_if(i == self.layer_index, 15)
                    .key(EditorKey::Layer(i))
                    .id(),
            );
        }
    }

    fn build_paint(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>) {
        let target = if self.fg { "FG" } else { "BG" };
        rows.push(
            b.text(format!("Tile {} {target}{}", self.selected_tile, self.layer_index))
                .small(true)
                .color(13)
                .full_width(8.0)
                .id(),
        );
        let up = b
            .text("-up")
            .small(true)
            .center()
            .size(PANEL_W / 2.0 - 1.0, 7.0)
            .outlined(0, 12)
            .key(EditorKey::PaletteUp)
            .id();
        let down = b
            .text("dn+")
            .small(true)
            .center()
            .size(PANEL_W / 2.0 - 1.0, 7.0)
            .outlined(0, 12)
            .key(EditorKey::PaletteDown)
            .id();
        rows.push(b.row(2.0, [up, down]).id());

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

        let start = self.palette_scroll * PALETTE_COLS;
        let mut tiles = Vec::with_capacity(PALETTE_COLS * PALETTE_ROWS);
        for n in 0..(PALETTE_COLS * PALETTE_ROWS) {
            let id = start + n;
            if id >= SHEET_TILES {
                break;
            }
            tiles.push(
                b.sprite(id as i32, 1, 1)
                    .size(8.0, 8.0)
                    .sprite_outline((id == self.selected_tile).then_some(11))
                    .key(EditorKey::Tile(id))
                    .id(),
            );
        }
        rows.push(
            b.wrap_row(0.0, tiles)
                .width(PALETTE_COLS as f32 * 8.0)
                .fill(0)
                .id(),
        );
    }

    fn build_objects(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>, map: &MapInfo) {
        let warps = self.tool == EditorTool::Warps;
        rows.push(
            b.text(if warps { "WARPS:" } else { "INTERACTS:" })
                .color(13)
                .full_width(8.0)
                .id(),
        );
        let new = b
            .text("+new")
            .small(true)
            .center()
            .color(11)
            .size(PANEL_W / 2.0 - 1.0, 7.0)
            .outlined(0, 11)
            .key(EditorKey::NewObject)
            .id();
        let del = b
            .text("-del")
            .small(true)
            .center()
            .color(8)
            .size(PANEL_W / 2.0 - 1.0, 7.0)
            .outlined(0, 8)
            .key(EditorKey::DeleteObject)
            .id();
        rows.push(b.row(2.0, [new, del]).id());

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
            match &object.effect {
                ObjectEffect::Warp(w) => {
                    let dest = w.map.as_deref().unwrap_or("-");
                    self.field_row(b, rows, EditField::ToMap, "map", dest);
                    self.field_row(b, rows, EditField::ToX, "x", &w.to.x.to_string());
                    self.field_row(b, rows, EditField::ToY, "y", &w.to.y.to_string());
                    self.cycle_row(b, rows, CycleField::Flip, "flip", axis_label(&w.flip));
                    self.cycle_row(b, rows, CycleField::Mode, "mode", mode_label(&w.mode));
                    self.cycle_row(b, rows, CycleField::Sound, "snd", sound_label(&w.sound));
                    self.cycle_row(b, rows, CycleField::Trigger, "trig", trigger_label(object.trigger));
                    let narr = w.narration.as_deref().unwrap_or("-");
                    self.field_row(b, rows, EditField::Narration, "narr", narr);
                }
                ObjectEffect::Interact(interaction) => {
                    let key = match interaction {
                        Interaction::Dialogue(k) => k.as_str(),
                        _ => "-",
                    };
                    self.field_row(b, rows, EditField::Key, "key", key);
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
            (true, Some(f)) => format!("{label}:{}_", f.text()),
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

    /// Reverse an action's effect (the undo direction).
    fn revert(&mut self, map: &mut MapInfo, maps: &mut MapStore, action: &EditAction) {
        match action {
            EditAction::Tiles { source, layer, cells } => {
                if let Some(tiles) = maps.get_mut(source) {
                    for &(x, y, old, _new) in cells {
                        tiles.set(*layer, x as usize, y as usize, old);
                    }
                }
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
        }
    }

    // --- Step (input) ---------------------------------------------------------

    /// The keys the map editor consumes from the shared console — text-entry
    /// control keys plus its command shortcuts (Ctrl+Z/Y/S, Delete, the 1-4 tool
    /// switches and their modifiers). The host forwards these even when the key
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
                | ScanCode::Delete
                | ScanCode::Digit1
                | ScanCode::Digit2
                | ScanCode::Digit3
                | ScanCode::Digit4
        )
    }

    pub fn step_map_viewer(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        maps: &mut MapStore,
        camera_pos: Vec2,
    ) {
        let screen = (system.width() as f32, system.height() as f32);
        self.step_map_viewer_at(system, map, maps, camera_pos, screen);
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
    ) {
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

        if self.editing.is_some() {
            // While a text field is focused all keys feed the buffer — don't let
            // editor shortcuts (incl. a typed "z") fire.
            self.step_text_entry(system, map);
        } else {
            self.handle_shortcuts(system, map, maps);
        }

        let panel_hit = self.build_ui(map, screen).hit(system.mouse().pos());
        let mouse = system.mouse();
        match panel_hit {
            Some(key) => self.handle_panel(system, map, maps, key, &mouse, camera_pos),
            None => self.handle_canvas(system, map, maps, camera_pos, &mouse),
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
    }

    /// Global editor keyboard shortcuts (only while no text field is focused):
    /// Ctrl+Z undo, Ctrl+Y / Ctrl+Shift+Z redo, Ctrl+S save, Delete removes the
    /// selected object, and `1`–`4` switch tools. These keys are forwarded to the
    /// console by the host's editor-key gate (see `main.rs`).
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
            // Ctrl-chorded: don't also treat the digit as a tool switch.
            return;
        }

        // Delete the selected object (object tools only).
        if system.keyp(ScanCode::Delete)
            && matches!(self.tool, EditorTool::Interactables | EditorTool::Warps)
        {
            self.delete_object(map);
        }

        // Number-row tool switching, mirroring the tab order.
        let tool = if system.keyp(ScanCode::Digit1) {
            Some(EditorTool::Layers)
        } else if system.keyp(ScanCode::Digit2) {
            Some(EditorTool::Paint)
        } else if system.keyp(ScanCode::Digit3) {
            Some(EditorTool::Interactables)
        } else if system.keyp(ScanCode::Digit4) {
            Some(EditorTool::Warps)
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
        key: EditorKey,
        mouse: &MouseInput,
        camera_pos: Vec2,
    ) {
        let click = just_pressed(mouse.left);
        match key {
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
            EditorKey::Layer(i) => {
                if mouse.moved() {
                    self.layer_index = i;
                }
                if click {
                    self.layer_index = i;
                    self.toggle_layer(map);
                }
            }
            EditorKey::Tile(id) => {
                if click {
                    self.selected_tile = id;
                }
            }
            EditorKey::PaletteUp => {
                if click {
                    self.palette_scroll = self.palette_scroll.saturating_sub(PALETTE_ROWS);
                }
            }
            EditorKey::PaletteDown => {
                if click {
                    let max = SHEET_TILES
                        .div_ceil(PALETTE_COLS)
                        .saturating_sub(PALETTE_ROWS);
                    self.palette_scroll = (self.palette_scroll + PALETTE_ROWS).min(max);
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
            EditorKey::DeleteObject => {
                if click {
                    self.delete_object(map);
                }
            }
            EditorKey::Field(field) => {
                if click {
                    self.begin_edit(field, map);
                }
            }
            EditorKey::Cycle(field) => {
                if click {
                    self.cycle(map, field);
                }
            }
            EditorKey::Eraser => {
                if click {
                    self.selected_tile = 0;
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
        }
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
        let (tx, ty) = world_tile(mouse, camera_pos);

        // Middle-click eyedropper: lift the existing tile into the brush.
        if just_pressed(mouse.middle) && tx >= 0 && ty >= 0 {
            self.selected_tile = maps
                .get(&source)
                .and_then(|m| m.get(layer, tx as usize, ty as usize))
                .unwrap_or(0);
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
                let value = if pressed(mouse.right) {
                    0
                } else {
                    self.selected_tile
                };
                self.paint_cell(maps, &source, layer, tx, ty, value);
            }
        }
        if released(mouse.left) || released(mouse.right) {
            self.flush_stroke();
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
        let old = tiles.get(layer, tx as usize, ty as usize).unwrap_or(0);
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

    /// Fill the tile rectangle between two world points with the current brush,
    /// as a single undo step.
    fn fill_rect(&mut self, maps: &mut MapStore, source: &str, layer: usize, a: Vec2, b: Vec2) {
        let (x0, y0, x1, y1) = tile_bounds(a, b);
        self.stroke = Some(EditAction::Tiles {
            source: source.to_string(),
            layer,
            cells: Vec::new(),
        });
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                if tx >= 0 && ty >= 0 {
                    self.paint_cell(maps, source, layer, tx, ty, self.selected_tile);
                }
            }
        }
        self.flush_stroke();
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

    /// Clear text-entry focus: forget which field was being edited and drop its
    /// buffer. `editing` and `field` are always set/cleared together so
    /// [`is_typing`](Self::is_typing) stays in step with the live buffer.
    fn stop_editing(&mut self) {
        self.editing = None;
        self.field = None;
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
    }

    fn begin_edit(&mut self, field: EditField, map: &MapInfo) {
        let effect = self.selected.and_then(|i| map.objects.get(i)).map(|o| &o.effect);
        let value = match (effect, field) {
            (Some(ObjectEffect::Interact(Interaction::Dialogue(k))), EditField::Key) => k.clone(),
            (Some(ObjectEffect::Warp(w)), EditField::ToMap) => w.map.clone().unwrap_or_default(),
            (Some(ObjectEffect::Warp(w)), EditField::ToX) => w.to.x.to_string(),
            (Some(ObjectEffect::Warp(w)), EditField::ToY) => w.to.y.to_string(),
            (Some(ObjectEffect::Warp(w)), EditField::Narration) => {
                w.narration.clone().unwrap_or_default()
            }
            _ => String::new(),
        };
        self.editing = Some(field);
        self.field = Some(TextField::new(value));
    }

    fn step_text_entry(&mut self, system: &mut impl ConsoleApi, map: &mut MapInfo) {
        let Some(field) = self.field.as_mut() else {
            return;
        };
        match field.step(system) {
            TextEvent::Active => {}
            TextEvent::Commit => {
                self.commit_edit(map);
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

    fn commit_edit(&mut self, map: &mut MapInfo) {
        let (Some(_), Some(field)) = (self.selected, self.editing) else {
            return;
        };
        let buffer = self
            .field
            .as_ref()
            .map(|f| f.text().trim().to_string())
            .unwrap_or_default();
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
        }
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
        }
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

    // --- Draw -----------------------------------------------------------------

    pub fn draw_map_viewer(
        &self,
        draw_state: &mut DrawState,
        system: &mut impl ConsoleApi,
        walkaround: &WalkaroundState,
    ) {
        self.draw_at(
            draw_state,
            system,
            &walkaround.current_map,
            walkaround.camera.pos,
        );
    }

    /// Draw the editor overlay + panel for `map` from an explicit `camera_pos`.
    /// Generalises [`draw_map_viewer`](Self::draw_map_viewer) so an extra view
    /// can run its own editor against its own free camera, rather than the live
    /// walkaround camera. No-op while unfocused.
    pub fn draw_at(
        &self,
        draw_state: &mut DrawState,
        system: &mut impl ConsoleApi,
        map: &MapInfo,
        camera_pos: Vec2,
    ) {
        if !self.focused {
            return;
        }
        // Lay the panel out against the canvas actually being drawn to — an
        // extra view's framebuffer can differ from the console's screen size.
        let canvas = draw_state.rgba(LayerId::BG);
        let screen = (canvas.width() as f32, canvas.height() as f32);
        self.draw_canvas_overlay(draw_state, system, map, camera_pos);
        self.build_ui(map, screen)
            .draw(draw_state, system, LayerId::BG);
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
                    let (tx, ty) = world_tile(&system.mouse(), camera_pos);
                    draw_state.rgba(LayerId::BG).stroke_rect(
                        tx * 8 - cx,
                        ty * 8 - cy,
                        8,
                        8,
                        colour,
                    );
                }
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
        viewer.step_map_viewer_at(&mut console, &mut map_a, &mut store, Vec2::new(0, 0), screen);

        // Seed per-map state on map "a".
        viewer.record(tiles(vec![(0, 0, 1, 2)]));
        viewer.selected = Some(0);
        viewer.editing = Some(EditField::Key);
        viewer.field = Some(TextField::new("x"));

        // Stepping the same map keeps it all.
        viewer.step_map_viewer_at(&mut console, &mut map_a, &mut store, Vec2::new(0, 0), screen);
        assert!(viewer.history.can_undo());
        assert!(viewer.is_typing());
        assert_eq!(viewer.selected, Some(0));

        // Stepping a different map drops it.
        let mut map_b = MapInfo {
            source: "b".to_string(),
            ..MapInfo::default()
        };
        viewer.step_map_viewer_at(&mut console, &mut map_b, &mut store, Vec2::new(0, 0), screen);
        assert!(!viewer.history.can_undo(), "object undo entries went stale");
        assert!(!viewer.is_typing(), "text focus dropped");
        assert_eq!(viewer.selected, None, "selection index went stale");
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

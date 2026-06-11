//! In-game editor for modern Tiled maps: toggle layers, paint tiles, and place
//! or drag interactables and warps. Opened with `L` in walkaround; freezes the
//! sim while focused and writes edits back to the map's `.tmj`.

use crate::{
    data::sound::{self, SfxData},
    drawstate::{DrawState, LayerId},
    interact::{Interactable, Interaction},
    map::{Axis, LayerInfo, MapInfo, MapStore, Warp, WarpMode},
    position::{Hitbox, Vec2},
    system::{
        ConsoleApi, ConsoleHelper, MouseInput, ScanCode, drawing::Canvas, just_pressed, pressed,
    },
    ui::{NodeId, Ui, UiBuilder},
};

use super::walkaround::WalkaroundState;

/// The active editing tool. The map editor is the old layer viewer grown into a
/// tabbed tool: toggle layers, paint tiles, or place interactables/warps.
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
}

/// A warp enum-field the editor advances with a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CycleField {
    Flip,
    Mode,
    Sound,
}

/// Which object list an [`EditAction`] refers to. Captured at record time so
/// undo/redo replays into the right collection even if the active tool has
/// since changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjKind {
    Interactable,
    Warp,
}

/// A whole-object snapshot used by the object undo entries. One of the two
/// fields is always `Some`, matching the [`ObjKind`] — cloning the object is
/// cheap (a hitbox + a small interaction/warp) and keeps undo trivially correct
/// without per-field diffing.
#[derive(Debug, Clone)]
enum ObjSnapshot {
    Interactable(Interactable),
    Warp(Warp),
}

/// One reversible edit, the unit of the undo/redo stacks. Tile paints batch a
/// whole press-drag-release stroke into a single entry (so one Ctrl+Z undoes a
/// brush stroke, not one pixel of it); object edits snapshot the affected object
/// before and/or after so they replay exactly.
#[derive(Debug, Clone)]
enum EditAction {
    /// Tiles changed by one paint stroke: `(x, y, old, new)` per cell, in the
    /// `(source, layer)` the stroke painted into.
    Tiles {
        source: String,
        layer: usize,
        cells: Vec<(i32, i32, usize, usize)>,
    },
    /// An object was appended at `index` (always the end of its list).
    Add {
        kind: ObjKind,
        index: usize,
        after: ObjSnapshot,
    },
    /// An object was removed from `index`; `before` is the object as it was.
    Remove {
        kind: ObjKind,
        index: usize,
        before: ObjSnapshot,
    },
    /// An object was mutated in place (moved, retyped, or a field edited).
    Modify {
        kind: ObjKind,
        index: usize,
        before: ObjSnapshot,
        after: ObjSnapshot,
    },
}

/// Cap on each undo/redo stack. Tile strokes can be large, so this is a count of
/// *actions*, not cells — generous enough for a long editing session while still
/// bounding memory.
const HISTORY_LIMIT: usize = 128;

/// Bounded undo/redo history. Pushing a new action clears the redo stack (the
/// usual linear-history model) and drops the oldest undo entry once full.
#[derive(Debug, Clone, Default)]
struct History {
    undo: Vec<EditAction>,
    redo: Vec<EditAction>,
}

impl History {
    /// Record a freshly performed action, invalidating any redo future.
    fn push(&mut self, action: EditAction) {
        self.redo.clear();
        self.undo.push(action);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
    }
    /// Pop the most recent action to be undone, moving it onto the redo stack.
    fn take_undo(&mut self) -> Option<EditAction> {
        let action = self.undo.pop()?;
        self.redo.push(action.clone());
        Some(action)
    }
    /// Pop the most recently undone action to be redone, moving it back onto undo.
    fn take_redo(&mut self) -> Option<EditAction> {
        let action = self.redo.pop()?;
        self.undo.push(action.clone());
        Some(action)
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
    editing: Option<EditField>,
    buffer: String,
    /// In-progress paint stroke: the cells touched since the mouse went down,
    /// flushed into one history entry on release so a stroke undoes atomically.
    stroke: Option<EditAction>,
    /// Bounded undo/redo stacks for tile and object edits.
    history: History,
    /// Set on any edit, cleared on save — drives the unsaved-changes marker.
    dirty: bool,
    /// Counts down a "saved" toast after a successful write (purely cosmetic).
    save_toast: u32,
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
            .color(if self.history.undo.is_empty() { 13 } else { 12 })
            .size(PANEL_W / 2.0 - 1.0, 7.0)
            .outlined(0, 13)
            .key(EditorKey::Undo)
            .id();
        let redo = b
            .text("redo>")
            .small(true)
            .center()
            .color(if self.history.redo.is_empty() { 13 } else { 12 })
            .size(PANEL_W / 2.0 - 1.0, 7.0)
            .outlined(0, 13)
            .key(EditorKey::Redo)
            .id();
        rows.push(b.row(2.0, [undo, redo]).id());

        // Save button: a `*` flags unsaved edits; a transient toast confirms a
        // write. Green outline normally, amber while dirty.
        let (label, outline) = if self.save_toast > 0 {
            ("[  SAVED!  ]", 11)
        } else if self.dirty {
            ("[ SAVE * ]", 9)
        } else {
            ("[ SAVE MAP ]", 6)
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
            rows.push(
                b.text(format!("Layer {i} {hidden}"))
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

        let count = if warps {
            map.warps.len()
        } else {
            map.interactables.len()
        };
        for i in 0..count {
            let label = if warps {
                let dest = map.warps[i].map.as_deref().unwrap_or("-");
                format!("{i}: ->{dest}")
            } else {
                match &map.interactables[i].interaction {
                    Interaction::Dialogue(k) => format!("{i}: {k}"),
                    Interaction::Func(_) => format!("{i}: <fn>"),
                    Interaction::None => format!("{i}: <->"),
                }
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

        if let Some(i) = self.selected {
            rows.push(b.spacer(2.0).id());
            if warps {
                if let Some(w) = map.warps.get(i) {
                    let dest = w.map.as_deref().unwrap_or("-");
                    self.field_row(b, rows, EditField::ToMap, "map", dest);
                    self.field_row(b, rows, EditField::ToX, "x", &w.to.x.to_string());
                    self.field_row(b, rows, EditField::ToY, "y", &w.to.y.to_string());
                    self.cycle_row(b, rows, CycleField::Flip, "flip", axis_label(&w.flip));
                    self.cycle_row(b, rows, CycleField::Mode, "mode", mode_label(&w.mode));
                    self.cycle_row(b, rows, CycleField::Sound, "snd", sound_label(&w.sound));
                }
            } else if let Some(it) = map.interactables.get(i) {
                let key = match &it.interaction {
                    Interaction::Dialogue(k) => k.as_str(),
                    _ => "-",
                };
                self.field_row(b, rows, EditField::Key, "key", key);
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
        let text = if editing {
            format!("{label}:{}_", self.buffer)
        } else {
            format!("{label}:{value}")
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

    /// Index of the interactable/warp whose hitbox contains `world` (px).
    fn object_at(&self, map: &MapInfo, world: Vec2) -> Option<usize> {
        if self.tool == EditorTool::Warps {
            map.warps
                .iter()
                .position(|w| w.hitbox().touches_point(world))
        } else {
            map.interactables
                .iter()
                .position(|it| it.hitbox.touches_point(world))
        }
    }

    /// Top-left (px) of object `i`'s hitbox for the active object tool.
    fn object_origin(&self, map: &MapInfo, i: usize) -> Vec2 {
        if self.tool == EditorTool::Warps {
            map.warps
                .get(i)
                .map(|w| w.from.0)
                .unwrap_or(Vec2::new(0, 0))
        } else {
            map.interactables
                .get(i)
                .map(|it| Vec2::new(it.hitbox.x, it.hitbox.y))
                .unwrap_or(Vec2::new(0, 0))
        }
    }

    /// Move object `i`'s hitbox top-left to `pos`, keeping its size.
    fn set_object_origin(&self, map: &mut MapInfo, i: usize, pos: Vec2) {
        if self.tool == EditorTool::Warps {
            if let Some(w) = map.warps.get_mut(i) {
                w.from.0 = pos;
            }
        } else if let Some(it) = map.interactables.get_mut(i) {
            it.hitbox.x = pos.x;
            it.hitbox.y = pos.y;
        }
    }

    /// The object kind the active object tool edits.
    fn obj_kind(&self) -> ObjKind {
        if self.tool == EditorTool::Warps {
            ObjKind::Warp
        } else {
            ObjKind::Interactable
        }
    }

    /// Clone object `i` of `kind` into a snapshot, if it exists.
    fn snapshot(map: &MapInfo, kind: ObjKind, i: usize) -> Option<ObjSnapshot> {
        match kind {
            ObjKind::Interactable => map
                .interactables
                .get(i)
                .cloned()
                .map(ObjSnapshot::Interactable),
            ObjKind::Warp => map.warps.get(i).cloned().map(ObjSnapshot::Warp),
        }
    }

    // --- History --------------------------------------------------------------

    /// Record an action onto the undo stack and flag the map as unsaved. Every
    /// mutating editor operation funnels through here so dirty-tracking and
    /// history stay in lock-step.
    fn record(&mut self, action: EditAction) {
        self.history.push(action);
        self.dirty = true;
    }

    /// Undo the most recent edit (Ctrl+Z). Object indices may shift on
    /// add/remove, so undo restores list shape as well as contents.
    fn undo(&mut self, map: &mut MapInfo, maps: &mut MapStore) {
        if let Some(action) = self.history.take_undo() {
            self.revert(map, maps, &action);
            self.dirty = true;
        }
    }

    /// Redo the most recently undone edit (Ctrl+Y / Ctrl+Shift+Z).
    fn redo(&mut self, map: &mut MapInfo, maps: &mut MapStore) {
        if let Some(action) = self.history.take_redo() {
            self.reapply(map, maps, &action);
            self.dirty = true;
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
            EditAction::Add { kind, index, .. } => {
                remove_object(map, *kind, *index);
                self.selected = None;
            }
            // Undo a remove by re-inserting the snapshot at its old index.
            EditAction::Remove {
                kind,
                index,
                before,
            } => {
                insert_object(map, *kind, *index, before.clone());
            }
            // Undo a modify by restoring the "before" snapshot.
            EditAction::Modify {
                kind,
                index,
                before,
                ..
            } => {
                set_object(map, *kind, *index, before.clone());
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
            EditAction::Add { kind, index, after } => {
                insert_object(map, *kind, *index, after.clone());
            }
            EditAction::Remove { kind, index, .. } => {
                remove_object(map, *kind, *index);
                self.selected = None;
            }
            EditAction::Modify {
                kind, index, after, ..
            } => {
                set_object(map, *kind, *index, after.clone());
            }
        }
    }

    // --- Step (input) ---------------------------------------------------------

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
        if self.save_toast > 0 {
            self.save_toast -= 1;
        }

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
        self.editing = None;
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
                    self.editing = None;
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
                        self.editing = None;
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
        let Some((source, layer)) = self
            .active_layer(map)
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
        let kind = self.obj_kind();
        let to = self.object_origin(map, i);
        if to != from
            && let Some(after) = Self::snapshot(map, kind, i)
        {
            // Rebuild the "before" snapshot by re-deriving it from `after`'s
            // contents with the original origin restored.
            let before = move_snapshot(after.clone(), from);
            self.record(EditAction::Modify {
                kind,
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
        let kind = self.obj_kind();
        let index = if kind == ObjKind::Warp {
            let to = Vec2::new(hitbox.x, hitbox.y);
            map.warps.push(Warp::new(hitbox, None, to));
            map.warps.len() - 1
        } else {
            map.interactables
                .push(Interactable::dialogue(hitbox, "new_key"));
            map.interactables.len() - 1
        };
        self.selected = Some(index);
        self.editing = None;
        if let Some(after) = Self::snapshot(map, kind, index) {
            self.record(EditAction::Add { kind, index, after });
        }
    }

    fn delete_object(&mut self, map: &mut MapInfo) {
        let Some(i) = self.selected else { return };
        let kind = self.obj_kind();
        let before = Self::snapshot(map, kind, i);
        remove_object(map, kind, i);
        self.selected = None;
        self.editing = None;
        if let Some(before) = before {
            self.record(EditAction::Remove {
                kind,
                index: i,
                before,
            });
        }
    }

    fn begin_edit(&mut self, field: EditField, map: &MapInfo) {
        let value = match (self.selected, field) {
            (Some(i), EditField::Key) => match map.interactables.get(i).map(|it| &it.interaction) {
                Some(Interaction::Dialogue(k)) => k.clone(),
                _ => String::new(),
            },
            (Some(i), EditField::ToMap) => map
                .warps
                .get(i)
                .and_then(|w| w.map.clone())
                .unwrap_or_default(),
            (Some(i), EditField::ToX) => map
                .warps
                .get(i)
                .map(|w| w.to.x.to_string())
                .unwrap_or_default(),
            (Some(i), EditField::ToY) => map
                .warps
                .get(i)
                .map(|w| w.to.y.to_string())
                .unwrap_or_default(),
            _ => String::new(),
        };
        self.editing = Some(field);
        self.buffer = value;
    }

    fn step_text_entry(&mut self, system: &mut impl ConsoleApi, map: &mut MapInfo) {
        for c in system.key_chars() {
            if !c.is_control() {
                self.buffer.push(*c);
            }
        }
        if system.keyp(ScanCode::Backspace) {
            self.buffer.pop();
        }
        if system.keyp(ScanCode::Escape) {
            self.editing = None;
        } else if system.keyp(ScanCode::Return) {
            self.commit_edit(map);
            self.editing = None;
        }
    }

    /// Snapshot the selected object, run `f` to mutate it, then record a single
    /// [`EditAction::Modify`] if it actually changed. The before/after snapshots
    /// make every field edit undoable without per-field bookkeeping.
    fn modify_object(
        &mut self,
        map: &mut MapInfo,
        kind: ObjKind,
        f: impl FnOnce(&mut MapInfo, usize),
    ) {
        let Some(i) = self.selected else { return };
        let Some(before) = Self::snapshot(map, kind, i) else {
            return;
        };
        f(map, i);
        let Some(after) = Self::snapshot(map, kind, i) else {
            return;
        };
        if !snapshot_eq(&before, &after) {
            self.record(EditAction::Modify {
                kind,
                index: i,
                before,
                after,
            });
        }
    }

    fn commit_edit(&mut self, map: &mut MapInfo) {
        let (Some(_), Some(field)) = (self.selected, self.editing) else {
            return;
        };
        let buffer = self.buffer.trim().to_string();
        let kind = match field {
            EditField::Key => ObjKind::Interactable,
            _ => ObjKind::Warp,
        };
        self.modify_object(map, kind, |map, i| match field {
            EditField::Key => {
                if let Some(it) = map.interactables.get_mut(i) {
                    it.interaction = Interaction::Dialogue(buffer.clone());
                }
            }
            EditField::ToMap => {
                if let Some(w) = map.warps.get_mut(i) {
                    // The name is stored verbatim (empty = same-map warp);
                    // numeric strings keep working via `map_by_name`'s fallback.
                    w.map = (!buffer.is_empty()).then(|| buffer.clone());
                }
            }
            EditField::ToX => {
                if let (Some(w), Ok(x)) = (map.warps.get_mut(i), buffer.parse()) {
                    w.to.x = x;
                }
            }
            EditField::ToY => {
                if let (Some(w), Ok(y)) = (map.warps.get_mut(i), buffer.parse()) {
                    w.to.y = y;
                }
            }
        });
    }

    fn cycle(&mut self, map: &mut MapInfo, field: CycleField) {
        self.modify_object(map, ObjKind::Warp, |map, i| {
            let Some(w) = map.warps.get_mut(i) else {
                return;
            };
            match field {
                CycleField::Flip => w.flip = cycle_flip(&w.flip),
                CycleField::Mode => w.mode = cycle_mode(&w.mode),
                CycleField::Sound => w.sound = cycle_sound(&w.sound),
            }
        });
    }

    /// Persist the map and start the save-confirmation toast. Only modern maps
    /// have a `.tmj` to write back to; legacy windows just log.
    fn save(&mut self, system: &mut impl ConsoleApi, map: &MapInfo, maps: &MapStore) {
        if maps.is_modern(&map.source) {
            let json = maps
                .get(&map.source)
                .unwrap()
                .to_tmj(&map.interactables, &map.warps);
            system.write_file(&format!("maps/{}.tmj", map.source), json.as_bytes());
        } else {
            log::info!("save: {:?} is not a modern map; not saving", map.source);
        }
        self.dirty = false;
        self.save_toast = SAVE_TOAST_FRAMES;
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
            EditorTool::Interactables => {
                let base = draw_state.colour(14);
                let sel = draw_state.colour(11);
                let canvas = draw_state.rgba(LayerId::BG);
                for (i, it) in map.interactables.iter().enumerate() {
                    let colour = if Some(i) == self.selected { sel } else { base };
                    let h = it.hitbox;
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
            EditorTool::Warps => {
                let base = draw_state.colour(12);
                let sel = draw_state.colour(11);
                let canvas = draw_state.rgba(LayerId::BG);
                for (i, w) in map.warps.iter().enumerate() {
                    let h = w.hitbox();
                    let colour = if Some(i) == self.selected { sel } else { base };
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
fn move_snapshot(snapshot: ObjSnapshot, origin: Vec2) -> ObjSnapshot {
    match snapshot {
        ObjSnapshot::Interactable(mut it) => {
            it.hitbox.x = origin.x;
            it.hitbox.y = origin.y;
            ObjSnapshot::Interactable(it)
        }
        ObjSnapshot::Warp(mut w) => {
            w.from.0 = origin;
            ObjSnapshot::Warp(w)
        }
    }
}

/// Structural equality for object snapshots. `Interactable`/`Warp` don't derive
/// `PartialEq` (their interaction can hold a fn pointer), so compare the fields
/// the editor can actually change — enough to skip recording no-op edits.
fn snapshot_eq(a: &ObjSnapshot, b: &ObjSnapshot) -> bool {
    match (a, b) {
        (ObjSnapshot::Interactable(x), ObjSnapshot::Interactable(y)) => {
            let same_box = x.hitbox.x == y.hitbox.x
                && x.hitbox.y == y.hitbox.y
                && x.hitbox.w == y.hitbox.w
                && x.hitbox.h == y.hitbox.h;
            same_box && interaction_eq(&x.interaction, &y.interaction)
        }
        (ObjSnapshot::Warp(x), ObjSnapshot::Warp(y)) => {
            x.from == y.from
                && x.map == y.map
                && x.to == y.to
                && axis_label(&x.flip) == axis_label(&y.flip)
                && mode_label(&x.mode) == mode_label(&y.mode)
                && sound_label(&x.sound) == sound_label(&y.sound)
        }
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

/// Remove object `i` of `kind` from its list, ignoring out-of-range indices.
fn remove_object(map: &mut MapInfo, kind: ObjKind, i: usize) {
    match kind {
        ObjKind::Interactable if i < map.interactables.len() => {
            map.interactables.remove(i);
        }
        ObjKind::Warp if i < map.warps.len() => {
            map.warps.remove(i);
        }
        _ => {}
    }
}

/// Insert `snapshot` at index `i` of its list, clamping past-the-end inserts to
/// a push so undo of a delete always lands the object back.
fn insert_object(map: &mut MapInfo, kind: ObjKind, i: usize, snapshot: ObjSnapshot) {
    match snapshot {
        ObjSnapshot::Interactable(it) if kind == ObjKind::Interactable => {
            let i = i.min(map.interactables.len());
            map.interactables.insert(i, it);
        }
        ObjSnapshot::Warp(w) if kind == ObjKind::Warp => {
            let i = i.min(map.warps.len());
            map.warps.insert(i, w);
        }
        _ => {}
    }
}

/// Overwrite object `i` of `kind` in place with `snapshot` (used to replay an
/// in-place modify). No-op if the index or kind no longer match.
fn set_object(map: &mut MapInfo, kind: ObjKind, i: usize, snapshot: ObjSnapshot) {
    match snapshot {
        ObjSnapshot::Interactable(it) if kind == ObjKind::Interactable => {
            if let Some(slot) = map.interactables.get_mut(i) {
                *slot = it;
            }
        }
        ObjSnapshot::Warp(w) if kind == ObjKind::Warp => {
            if let Some(slot) = map.warps.get_mut(i) {
                *slot = w;
            }
        }
        _ => {}
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

fn cycle_sound(sound: &Option<SfxData>) -> Option<SfxData> {
    match sound {
        None => Some(sound::DOOR),
        Some(s) if s.id == sound::DOOR.id => Some(sound::STAIRS_DOWN),
        Some(s) if s.id == sound::STAIRS_DOWN.id => Some(sound::STAIRS_UP),
        _ => None,
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

    /// Pushing onto the undo stack clears any redo future and bounds the stack at
    /// [`HISTORY_LIMIT`], dropping the oldest entry — the standard linear model.
    #[test]
    fn history_push_clears_redo_and_bounds() {
        let mut h = History::default();
        h.push(tiles(vec![(0, 0, 1, 2)]));
        // An undo then push should discard the redo entry.
        assert!(h.take_undo().is_some());
        assert_eq!(h.redo.len(), 1);
        h.push(tiles(vec![(1, 1, 0, 3)]));
        assert!(h.redo.is_empty(), "new push invalidates redo");

        // Overflow drops the oldest, keeping the cap.
        let mut h = History::default();
        for n in 0..(HISTORY_LIMIT + 10) {
            h.push(tiles(vec![(n as i32, 0, 0, 1)]));
        }
        assert_eq!(h.undo.len(), HISTORY_LIMIT);
    }

    /// `take_undo`/`take_redo` move entries between the two stacks so a sequence
    /// of undo→redo→undo round-trips correctly.
    #[test]
    fn history_undo_redo_round_trip() {
        let mut h = History::default();
        h.push(tiles(vec![(0, 0, 1, 2)]));
        h.push(tiles(vec![(1, 0, 3, 4)]));

        // Undo the latest, then redo it back.
        assert!(
            matches!(h.take_undo(), Some(EditAction::Tiles { cells, .. }) if cells == vec![(1, 0, 3, 4)])
        );
        assert_eq!((h.undo.len(), h.redo.len()), (1, 1));
        assert!(
            matches!(h.take_redo(), Some(EditAction::Tiles { cells, .. }) if cells == vec![(1, 0, 3, 4)])
        );
        assert_eq!((h.undo.len(), h.redo.len()), (2, 0));
        // Nothing left to redo.
        assert!(h.take_redo().is_none());
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

    /// `move_snapshot` relocates an object's origin without touching its size or
    /// payload, so a drag's "before" snapshot is exact for undo.
    #[test]
    fn move_snapshot_relocates_origin() {
        let it = Interactable::dialogue(Hitbox::new(40, 50, 16, 8), "k");
        let moved = move_snapshot(ObjSnapshot::Interactable(it), Vec2::new(1, 2));
        let ObjSnapshot::Interactable(out) = moved else {
            panic!("kind")
        };
        assert_eq!((out.hitbox.x, out.hitbox.y), (1, 2));
        assert_eq!((out.hitbox.w, out.hitbox.h), (16, 8)); // size preserved
    }

    /// `snapshot_eq` is true only for identical editable content, so no-op edits
    /// aren't recorded as undo steps.
    #[test]
    fn snapshot_eq_detects_changes() {
        let a = ObjSnapshot::Interactable(Interactable::dialogue(Hitbox::new(0, 0, 8, 8), "x"));
        let same = ObjSnapshot::Interactable(Interactable::dialogue(Hitbox::new(0, 0, 8, 8), "x"));
        let diff_key =
            ObjSnapshot::Interactable(Interactable::dialogue(Hitbox::new(0, 0, 8, 8), "y"));
        let diff_box =
            ObjSnapshot::Interactable(Interactable::dialogue(Hitbox::new(1, 0, 8, 8), "x"));
        assert!(snapshot_eq(&a, &same));
        assert!(!snapshot_eq(&a, &diff_key));
        assert!(!snapshot_eq(&a, &diff_box));
        // Cross-kind never compares equal.
        let warp = ObjSnapshot::Warp(Warp::new(Hitbox::new(0, 0, 8, 8), None, Vec2::new(0, 0)));
        assert!(!snapshot_eq(&a, &warp));
    }

    /// Object add/remove undo replays into the right list at the right index:
    /// undo of a remove re-inserts the exact object, undo of an add removes it.
    #[test]
    fn object_insert_remove_round_trip() {
        let mut map = MapInfo::default();
        map.interactables
            .push(Interactable::dialogue(Hitbox::new(0, 0, 8, 8), "a"));
        map.interactables
            .push(Interactable::dialogue(Hitbox::new(8, 0, 8, 8), "b"));

        // Snapshot + remove index 0, then re-insert it: list shape is restored.
        let snap = MapViewer::snapshot(&map, ObjKind::Interactable, 0).unwrap();
        remove_object(&mut map, ObjKind::Interactable, 0);
        assert_eq!(map.interactables.len(), 1);
        insert_object(&mut map, ObjKind::Interactable, 0, snap);
        assert_eq!(map.interactables.len(), 2);
        let key = match &map.interactables[0].interaction {
            Interaction::Dialogue(k) => k.as_str(),
            _ => "",
        };
        assert_eq!(key, "a", "re-inserted at original index");

        // A past-the-end insert clamps to a push rather than panicking.
        let extra = ObjSnapshot::Interactable(Interactable::dialogue(Hitbox::new(0, 0, 8, 8), "c"));
        insert_object(&mut map, ObjKind::Interactable, 99, extra);
        assert_eq!(map.interactables.len(), 3);
    }
}

//! In-game editor for modern Tiled maps: toggle layers, paint tiles, and place
//! or drag interactables and warps. Opened with `L` in walkaround; freezes the
//! sim while focused and writes edits back to the map's `.tmj`.

use crate::{
    data::{
        map_data::MapIndex,
        sound::{self, SfxData},
    },
    drawstate::{DrawState, LayerId},
    interact::{Interactable, Interaction},
    map::{Axis, LayerInfo, MapInfo, Warp, WarpMode},
    position::{Hitbox, Vec2},
    system::{
        ConsoleApi, ConsoleHelper, MouseInput, ScanCode, drawing::Canvas, just_pressed, pressed,
    },
    ui::{self, Content, Decoration, NodeId, Style, Ui, UiBuilder},
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
    Save,
}

const PANEL_W: f32 = 84.0;
const PALETTE_COLS: usize = 9;
const PALETTE_ROWS: usize = 7;
const SHEET_TILES: usize = 2048;

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
    editing: Option<EditField>,
    buffer: String,
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
    fn build_ui(&self, map: &MapInfo) -> Ui<EditorKey> {
        use crate::system::HEIGHT;
        let mut b = UiBuilder::new();
        let mut rows: Vec<NodeId> = Vec::new();

        for tool in EditorTool::ALL {
            let selected = tool == self.tool;
            rows.push(b.leaf(
                Style { size: ui::full_width(7.0), ..Default::default() },
                Content::Text {
                    text: tool.label().to_string(),
                    color: if selected { 0 } else { 12 },
                    center: false,
                    small: true,
                },
                if selected { Decoration::fill(11) } else { Decoration::default() },
                Some(EditorKey::Tool(tool)),
            ));
        }
        rows.push(spacer(&mut b, 2.0));

        match self.tool {
            EditorTool::Layers => self.build_layers(&mut b, &mut rows, map),
            EditorTool::Paint => self.build_paint(&mut b, &mut rows),
            EditorTool::Interactables | EditorTool::Warps => self.build_objects(&mut b, &mut rows, map),
        }

        rows.push(spacer(&mut b, 2.0));
        rows.push(b.leaf(
            Style { size: ui::full_width(8.0), ..Default::default() },
            Content::Text { text: "[ SAVE MAP ]".to_string(), color: 6, center: true, small: true },
            Decoration::outlined(0, 6),
            Some(EditorKey::Save),
        ));

        let root = b.container(
            Style { size: ui::size(PANEL_W, HEIGHT as f32), ..ui::column(0.0) },
            Decoration::fill(0),
            None,
            &rows,
        );
        b.finish(root)
    }

    fn build_layers(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>, map: &MapInfo) {
        let layers = if self.fg { &map.fg_layers } else { &map.layers };
        let title = if self.fg { "FG LAYERS:" } else { "BG LAYERS:" };
        rows.push(b.leaf(
            Style { size: ui::full_width(8.0), ..Default::default() },
            Content::Text { text: title.to_string(), color: 13, center: false, small: false },
            Decoration::default(),
            Some(EditorKey::Title),
        ));
        for (i, layer) in layers.iter().enumerate() {
            let hidden = if layer.visible { "" } else { "(H)" };
            rows.push(b.leaf(
                Style { size: ui::full_width(7.0), ..Default::default() },
                Content::Text { text: format!("Layer {i} {hidden}"), color: 12, center: false, small: true },
                if i == self.layer_index { Decoration::fill(15) } else { Decoration::default() },
                Some(EditorKey::Layer(i)),
            ));
        }
    }

    fn build_paint(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>) {
        let target = if self.fg { "FG" } else { "BG" };
        rows.push(b.leaf(
            Style { size: ui::full_width(8.0), ..Default::default() },
            Content::Text {
                text: format!("Tile {} {target}{}", self.selected_tile, self.layer_index),
                color: 13,
                center: false,
                small: true,
            },
            Decoration::default(),
            None,
        ));
        let up = b.leaf(
            Style { size: ui::size(PANEL_W / 2.0 - 1.0, 7.0), ..Default::default() },
            Content::Text { text: "-up".to_string(), color: 12, center: true, small: true },
            Decoration::outlined(0, 12),
            Some(EditorKey::PaletteUp),
        );
        let down = b.leaf(
            Style { size: ui::size(PANEL_W / 2.0 - 1.0, 7.0), ..Default::default() },
            Content::Text { text: "dn+".to_string(), color: 12, center: true, small: true },
            Decoration::outlined(0, 12),
            Some(EditorKey::PaletteDown),
        );
        rows.push(b.container(ui::row(2.0), Decoration::default(), None, &[up, down]));

        let start = self.palette_scroll * PALETTE_COLS;
        let mut tiles = Vec::with_capacity(PALETTE_COLS * PALETTE_ROWS);
        for n in 0..(PALETTE_COLS * PALETTE_ROWS) {
            let id = start + n;
            if id >= SHEET_TILES {
                break;
            }
            tiles.push(b.leaf(
                Style { size: ui::size(8.0, 8.0), ..Default::default() },
                Content::Sprite {
                    id: id as i32,
                    scale: 1,
                    w: 1,
                    h: 1,
                    outline: (id == self.selected_tile).then_some(11),
                },
                Decoration::default(),
                Some(EditorKey::Tile(id)),
            ));
        }
        let grid = b.container(
            Style { size: ui::width(PALETTE_COLS as f32 * 8.0), ..ui::wrap_row(0.0) },
            Decoration::fill(1),
            None,
            &tiles,
        );
        rows.push(grid);
    }

    fn build_objects(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>, map: &MapInfo) {
        let warps = self.tool == EditorTool::Warps;
        rows.push(b.leaf(
            Style { size: ui::full_width(8.0), ..Default::default() },
            Content::Text {
                text: if warps { "WARPS:" } else { "INTERACTS:" }.to_string(),
                color: 13,
                center: false,
                small: false,
            },
            Decoration::default(),
            None,
        ));
        let new = b.leaf(
            Style { size: ui::size(PANEL_W / 2.0 - 1.0, 7.0), ..Default::default() },
            Content::Text { text: "+new".to_string(), color: 11, center: true, small: true },
            Decoration::outlined(0, 11),
            Some(EditorKey::NewObject),
        );
        let del = b.leaf(
            Style { size: ui::size(PANEL_W / 2.0 - 1.0, 7.0), ..Default::default() },
            Content::Text { text: "-del".to_string(), color: 8, center: true, small: true },
            Decoration::outlined(0, 8),
            Some(EditorKey::DeleteObject),
        );
        rows.push(b.container(ui::row(2.0), Decoration::default(), None, &[new, del]));

        let count = if warps { map.warps.len() } else { map.interactables.len() };
        for i in 0..count {
            let label = if warps {
                let dest = map.warps[i].map.map(|m| m.0 as i32).unwrap_or(-1);
                format!("{i}: ->m{dest}")
            } else {
                match &map.interactables[i].interaction {
                    Interaction::Dialogue(k) => format!("{i}: {k}"),
                    Interaction::Func(_) => format!("{i}: <fn>"),
                    Interaction::None => format!("{i}: <->"),
                }
            };
            rows.push(b.leaf(
                Style { size: ui::full_width(7.0), ..Default::default() },
                Content::Text { text: label, color: 12, center: false, small: true },
                if Some(i) == self.selected { Decoration::fill(15) } else { Decoration::default() },
                Some(EditorKey::Object(i)),
            ));
        }

        if let Some(i) = self.selected {
            rows.push(spacer(b, 2.0));
            if warps {
                if let Some(w) = map.warps.get(i) {
                    let dest = w.map.map(|m| m.0.to_string()).unwrap_or_else(|| "-".to_string());
                    self.field_row(b, rows, EditField::ToMap, "map", &dest);
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
        rows.push(b.leaf(
            Style { size: ui::full_width(7.0), ..Default::default() },
            Content::Text { text, color: if editing { 0 } else { 12 }, center: false, small: true },
            if editing { Decoration::fill(14) } else { Decoration::default() },
            Some(EditorKey::Field(field)),
        ));
    }

    fn cycle_row(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        field: CycleField,
        label: &str,
        value: &str,
    ) {
        rows.push(b.leaf(
            Style { size: ui::full_width(7.0), ..Default::default() },
            Content::Text { text: format!("{label}:{value}"), color: 12, center: false, small: true },
            Decoration::default(),
            Some(EditorKey::Cycle(field)),
        ));
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
        if self.fg { map.fg_layers.len() } else { map.layers.len() }
    }

    /// The layer the paint tool writes into (selected in the Layers tool).
    fn active_layer<'a>(&self, map: &'a MapInfo) -> Option<&'a LayerInfo> {
        let layers = if self.fg { &map.fg_layers } else { &map.layers };
        layers.get(self.layer_index)
    }

    /// Index of the interactable/warp whose hitbox contains `world` (px).
    fn object_at(&self, map: &MapInfo, world: Vec2) -> Option<usize> {
        if self.tool == EditorTool::Warps {
            map.warps.iter().position(|w| w.hitbox().touches_point(world))
        } else {
            map.interactables.iter().position(|it| it.hitbox.touches_point(world))
        }
    }

    /// Top-left (px) of object `i`'s hitbox for the active object tool.
    fn object_origin(&self, map: &MapInfo, i: usize) -> Vec2 {
        if self.tool == EditorTool::Warps {
            map.warps.get(i).map(|w| w.from.0).unwrap_or(Vec2::new(0, 0))
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

    // --- Step (input) ---------------------------------------------------------

    pub fn step_map_viewer(&mut self, system: &mut impl ConsoleApi, map: &mut MapInfo, camera_pos: Vec2) {
        if self.editing.is_some() {
            self.step_text_entry(system, map);
        }

        let panel_hit = self.build_ui(map).hit(system.mouse().pos());
        let mouse = system.mouse();
        match panel_hit {
            Some(key) => self.handle_panel(system, map, key, &mouse, camera_pos),
            None => self.handle_canvas(system, map, camera_pos, &mouse),
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

    fn handle_panel(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        key: EditorKey,
        mouse: &MouseInput,
        camera_pos: Vec2,
    ) {
        let click = just_pressed(mouse.left);
        match key {
            EditorKey::Tool(tool) => {
                if click {
                    self.tool = tool;
                    self.selected = None;
                    self.editing = None;
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
                    let max = SHEET_TILES.div_ceil(PALETTE_COLS).saturating_sub(PALETTE_ROWS);
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
                    self.new_object(map, camera_pos);
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
            EditorKey::Save => {
                if click {
                    system.write_map(map);
                }
            }
        }
    }

    fn handle_canvas(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        camera_pos: Vec2,
        mouse: &MouseInput,
    ) {
        match self.tool {
            EditorTool::Paint => {
                if pressed(mouse.left) || pressed(mouse.right) {
                    let (tx, ty) = world_tile(mouse, camera_pos);
                    if tx >= 0 && ty >= 0 {
                        let value = if pressed(mouse.right) { 0 } else { self.selected_tile };
                        if let Some((bank, src)) =
                            self.active_layer(map).map(|l| (map.bank, l.source_layer))
                        {
                            system.map_set(bank, src, tx, ty, value);
                        }
                    }
                }
            }
            EditorTool::Interactables | EditorTool::Warps => {
                let world = Vec2::new(mouse.pos().x + camera_pos.x, mouse.pos().y + camera_pos.y);
                if just_pressed(mouse.left) {
                    if let Some(i) = self.object_at(map, world) {
                        // Grab the object under the cursor to drag it around.
                        self.selected = Some(i);
                        self.editing = None;
                        self.drag = None;
                        self.moving = Some(world - self.object_origin(map, i));
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
                    self.moving = None;
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

    fn new_object(&mut self, map: &mut MapInfo, camera_pos: Vec2) {
        let x = camera_pos.x + crate::system::WIDTH as i16 / 2;
        let y = camera_pos.y + crate::system::HEIGHT as i16 / 2;
        self.create_object(map, Hitbox::new(x, y, 16, 16));
    }

    fn create_object(&mut self, map: &mut MapInfo, hitbox: Hitbox) {
        if self.tool == EditorTool::Warps {
            let to = Vec2::new(hitbox.x, hitbox.y);
            map.warps.push(Warp::new(hitbox, None, to));
            self.selected = Some(map.warps.len() - 1);
        } else {
            map.interactables.push(Interactable::dialogue(hitbox, "new_key"));
            self.selected = Some(map.interactables.len() - 1);
        }
        self.editing = None;
    }

    fn delete_object(&mut self, map: &mut MapInfo) {
        if let Some(i) = self.selected {
            if self.tool == EditorTool::Warps {
                if i < map.warps.len() {
                    map.warps.remove(i);
                }
            } else if i < map.interactables.len() {
                map.interactables.remove(i);
            }
            self.selected = None;
            self.editing = None;
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
                .and_then(|w| w.map)
                .map(|m| m.0.to_string())
                .unwrap_or_default(),
            (Some(i), EditField::ToX) => {
                map.warps.get(i).map(|w| w.to.x.to_string()).unwrap_or_default()
            }
            (Some(i), EditField::ToY) => {
                map.warps.get(i).map(|w| w.to.y.to_string()).unwrap_or_default()
            }
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

    fn commit_edit(&mut self, map: &mut MapInfo) {
        let (Some(i), Some(field)) = (self.selected, self.editing) else {
            return;
        };
        match field {
            EditField::Key => {
                if let Some(it) = map.interactables.get_mut(i) {
                    it.interaction = Interaction::Dialogue(self.buffer.trim().to_string());
                }
            }
            EditField::ToMap => {
                if let Some(w) = map.warps.get_mut(i) {
                    w.map = self.buffer.trim().parse::<usize>().ok().map(MapIndex);
                }
            }
            EditField::ToX => {
                if let (Some(w), Ok(x)) = (map.warps.get_mut(i), self.buffer.trim().parse()) {
                    w.to.x = x;
                }
            }
            EditField::ToY => {
                if let (Some(w), Ok(y)) = (map.warps.get_mut(i), self.buffer.trim().parse()) {
                    w.to.y = y;
                }
            }
        }
    }

    fn cycle(&mut self, map: &mut MapInfo, field: CycleField) {
        let Some(i) = self.selected else { return };
        let Some(w) = map.warps.get_mut(i) else { return };
        match field {
            CycleField::Flip => w.flip = cycle_flip(&w.flip),
            CycleField::Mode => w.mode = cycle_mode(&w.mode),
            CycleField::Sound => w.sound = cycle_sound(&w.sound),
        }
    }

    // --- Draw -----------------------------------------------------------------

    pub fn draw_map_viewer(
        &self,
        draw_state: &mut DrawState,
        system: &mut impl ConsoleApi,
        walkaround: &WalkaroundState,
    ) {
        if !self.focused {
            return;
        }
        let map = &walkaround.current_map;
        self.draw_canvas_overlay(draw_state, system, map, walkaround.camera.pos);
        self.build_ui(map).draw(draw_state, system, LayerId::BG);
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
                let (tx, ty) = world_tile(&system.mouse(), camera_pos);
                let colour = draw_state.colour(11);
                draw_state
                    .rgba(LayerId::BG)
                    .stroke_rect(tx * 8 - cx, ty * 8 - cy, 8, 8, colour);
            }
            EditorTool::Interactables => {
                let base = draw_state.colour(14);
                let sel = draw_state.colour(11);
                let canvas = draw_state.rgba(LayerId::BG);
                for (i, it) in map.interactables.iter().enumerate() {
                    let colour = if Some(i) == self.selected { sel } else { base };
                    let h = it.hitbox;
                    canvas.stroke_rect(i32::from(h.x) - cx, i32::from(h.y) - cy, i32::from(h.w), i32::from(h.h), colour);
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
                    canvas.stroke_rect(i32::from(h.x) - cx, i32::from(h.y) - cy, i32::from(h.w), i32::from(h.h), colour);
                }
                self.draw_drag_preview(draw_state, system, camera_pos);
            }
            EditorTool::Layers => {}
        }
    }

    fn draw_drag_preview(&self, draw_state: &mut DrawState, system: &mut impl ConsoleApi, camera_pos: Vec2) {
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

fn spacer(b: &mut UiBuilder<EditorKey>, height: f32) -> NodeId {
    b.leaf(
        Style { size: ui::full_width(height), ..Default::default() },
        Content::None,
        Decoration::default(),
        None,
    )
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

//! Immediate-mode panel builders: one `build_*` per dock panel (layers, paint,
//! objects, maps, setup, dialog, …) plus the shared row primitives and the
//! object-key / gate-flag autocomplete dropdown.

use super::*;

impl MapViewer {
    /// The known vocabulary a text field autocompletes against, or `None` for a
    /// free-form / numeric field (no dropdown). An interaction's dialogue key and
    /// a warp's pre-warp narration key both complete against the script's declared
    /// dialogue keys ([`dialogue_keys`](Self::dialogue_keys)); the gate fields
    /// (`if` / `unless` / `sets`) against the declared `#flag` vocabulary
    /// ([`flag_names`](Self::flag_names)) — the same list the gate `?` marker
    /// checks. Both lists are refreshed each focused step and arrive sorted.
    pub(super) fn autocomplete_vocab(&self, field: EditField) -> Option<&[String]> {
        match field {
            EditField::Key | EditField::Narration => Some(&self.dialogue_keys),
            EditField::CondIf | EditField::CondUnless | EditField::Sets => Some(&self.flag_names),
            _ => None,
        }
    }

    /// The live autocomplete suggestions for the field being edited: the top few
    /// vocabulary entries that prefix-match the current buffer. Empty when no
    /// field is focused, the field takes free-form / numeric input, or nothing
    /// matches. The panel renders these under the field; the first is the Tab
    /// target.
    pub(super) fn autocomplete_suggestions(&self) -> Vec<String> {
        let Some(edit) = self.editing.as_ref() else {
            return Vec::new();
        };
        let Some(vocab) = self.autocomplete_vocab(edit.field) else {
            return Vec::new();
        };
        autocomplete_matches(vocab, edit.buffer.text())
    }

    /// Accept autocomplete suggestion `i` for the field being edited: replace the
    /// edit buffer with that whole vocabulary entry, commit it to the object, and
    /// close the field — the click / Tab counterpart to typing the full key and
    /// pressing Return. A no-op when no field is focused or the index is stale.
    pub(super) fn accept_suggestion(&mut self, map: &mut MapInfo, maps: &mut MapStore, i: usize) {
        let Some(pick) = self.autocomplete_suggestions().into_iter().nth(i) else {
            return;
        };
        let Some(edit) = self.editing.as_mut() else {
            return;
        };
        edit.buffer = TextField::new(pick);
        self.commit_edit(map, maps);
        self.stop_editing();
    }

    /// Build one panel — a title-bar chrome row plus the panel kind's body —
    /// laid out at the origin and sized to fill its placed `rect`. The dock
    /// translates it to `rect`'s screen position when hit-testing
    /// ([`Ui::hit_at`]) and drawing ([`Ui::draw_at`]).
    /// The Presets panel: every creature preset by name; clicking one opens
    /// the fullscreen walk-sprite editor on it. The list is the engine-pushed
    /// [`preset_defs`](Self::preset_defs) snapshot (the editor can't see the
    /// live registry itself).
    fn build_presets(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>) {
        if self.preset_defs.is_empty() {
            rows.push(
                b.text("(no presets pushed)")
                    .small(true)
                    .color(13)
                    .full_width(7.0)
                    .id(),
            );
            return;
        }
        rows.push(
            b.text("click to edit walk sprites")
                .small(true)
                .color(13)
                .full_width(7.0)
                .id(),
        );
        for (i, (name, _)) in self.preset_defs.iter().enumerate() {
            rows.push(
                b.text(name)
                    .small(true)
                    .color(12)
                    .full_width(7.0)
                    .key(EditorKey::PresetRow(i))
                    .id(),
            );
        }
    }

    pub(super) fn build_panel(
        &self,
        idx: usize,
        rect: Rect,
        map: &MapInfo,
        maps: &MapStore,
    ) -> Ui<EditorKey> {
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
            .key(EditorKey::Dock(idx))
            .id();

        match kind {
            PanelKind::Layers => self.build_layers(&mut b, &mut rows, map, maps),
            PanelKind::Paint => {
                self.build_paint_tabs(&mut b, &mut rows);
                if self.tool == EditorTool::Select {
                    self.build_select(&mut b, &mut rows);
                } else {
                    self.build_paint(&mut b, &mut rows, map);
                }
            }
            PanelKind::Objects => {
                self.build_obj_tabs(&mut b, &mut rows);
                self.build_objects(&mut b, &mut rows, map, rect);
            }
            PanelKind::Maps => self.build_maps(&mut b, &mut rows, rect, maps),
            PanelKind::Map => self.build_setup(&mut b, &mut rows, map, maps),
            PanelKind::Dialogue => self.build_dialogue(&mut b, &mut rows),
            PanelKind::Presets => self.build_presets(&mut b, &mut rows),
        }

        let size = (rect.w as f32, rect.h as f32);
        // A scrollable kind keeps its body at natural height (no_shrink) so it
        // overflows the panel and can be scrolled+clipped; other kinds stay in one
        // column that shrinks to fit, exactly as before.
        let root = if Self::is_scroll_kind(kind) {
            // The body keeps natural (content) height so it overflows + scrolls.
            // It carries its own bg fill spanning that full height: the root's
            // fill is only panel-tall, so once scrolled it stops short of the
            // body's bottom — the body's own fill keeps the scrolled region opaque.
            let body = b.column(0.0, rows).no_shrink().fill(0).id();
            b.column(0.0, [title, body])
                .size(size.0, size.1)
                .fill(0)
                .id()
        } else {
            let mut all = Vec::with_capacity(rows.len() + 1);
            all.push(title);
            all.extend(rows);
            b.column(0.0, all).size(size.0, size.1).fill(0).id()
        };
        b.finish(root, size)
    }

    /// A small always-on toolbar — undo / redo / save — pinned to the world's
    /// top-left. The global editor controls, independent of any panel (so they
    /// survive whatever the user does with the tool panels).
    pub(super) fn build_global_bar(&self) -> Ui<EditorKey> {
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
                    .fill(if open { 11 } else { 0 })
                    .outline(12)
                    .key(EditorKey::TogglePanel(kind))
                    .id(),
            );
        }
        let root = b.row(1.0, buttons).fill(0).pad(1.0).id();
        b.finish(root, (GLOBAL_BAR_W, GLOBAL_BAR_H))
    }

    /// The centred modal for the active map dialog (new / rename / delete). Pure
    /// display — driven entirely by the keyboard in [`step_maps_dialog`].
    pub(super) fn build_dialog(&self) -> Ui<EditorKey> {
        let (sw, sh) = self.dock.solved.screen;
        let mut b = UiBuilder::new();
        let (title, body, hint) = match &self.maps_dialog {
            MapsDialog::New { name, w, h, focus } => {
                // The focused field shows the caret at the cursor; the others show
                // their plain text.
                let shown = |field: &TextField, i: u8| {
                    if *focus == i {
                        field.display()
                    } else {
                        field.text().to_string()
                    }
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
                    if *focus == i {
                        field.display()
                    } else {
                        field.text().to_string()
                    }
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
    pub(super) fn build_obj_tabs(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>) {
        // A tool tab is just a toggle button keyed by the tool it selects.
        let mk = |b: &mut UiBuilder<EditorKey>, tool: EditorTool, label: &str, sel: bool| {
            Self::toggle_button(b, label, sel, EditorKey::Tool(tool))
        };
        let it = mk(
            b,
            EditorTool::Interactables,
            "Intr",
            self.tool == EditorTool::Interactables,
        );
        let wp = mk(b, EditorTool::Warps, "Warp", self.tool == EditorTool::Warps);
        rows.push(b.row(1.0, [it, wp]).id());
    }

    /// The Maps browser: a paged grid of map cells. A thumbnail is blitted over
    /// each cell in `draw`; the name labels it. A first click selects a map, a
    /// second click on the selected map opens it (via `pending_open`).
    pub(super) fn build_maps(
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
        let new = Self::action_button(b, "+", 11, true, EditorKey::MapNew);
        let dup = Self::action_button(b, "dup", 12, has_sel, EditorKey::MapDup);
        let ren = Self::action_button(b, "ren", 12, has_sel, EditorKey::MapRename);
        let del = Self::action_button(b, "del", 8, has_sel, EditorKey::MapDelete);
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
    pub(super) fn build_setup(
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
        self.header_row(b, rows, "CAMERA:", 8.0);
        let cam = match stick {
            Some((x, y)) => format!("stick {x},{y}"),
            None => "auto".to_string(),
        };
        rows.push(b.text(cam).small(true).color(12).full_width(7.0).id());
        let auto = Self::toggle_button(b, "auto", stick.is_none(), EditorKey::CamAuto);
        let pin = Self::toggle_button(b, "pin", stick.is_some(), EditorKey::CamPin);
        rows.push(b.row(1.0, [auto, pin]).id());

        // Background colour: 16 palette swatches, the current one ringed.
        self.header_row(b, rows, format!("BG: {bg}"), 8.0);
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
        self.header_row(b, rows, format!("SIZE: {w}x{h}"), 8.0);
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
        self.header_row(b, rows, format!("MUSIC: {}", truncate(music, 11)), 8.0);
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
    pub(super) fn modern_names(&self, maps: &MapStore) -> Vec<String> {
        maps.names()
            .into_iter()
            .filter(|n| maps.is_modern(n))
            .map(str::to_string)
            .collect()
    }

    /// How many `(cols, rows)` of map cells fit the Maps panel `rect`.
    pub(super) fn maps_grid(&self, rect: Rect) -> (usize, usize) {
        let cols = (((rect.w as i32) - 2) / (THUMB_W as i32 + 1)).max(1) as usize;
        let rows = (((rect.h as i32) - 16) / (THUMB_H as i32 + 7)).max(1) as usize;
        (cols, rows)
    }

    /// The panel kind that currently owns the canvas (drives the active-panel
    /// highlight), derived from the active [`EditorTool`].
    pub(super) fn active_kind(&self) -> Option<PanelKind> {
        Some(match self.tool {
            EditorTool::Layers => PanelKind::Layers,
            EditorTool::Paint | EditorTool::Select => PanelKind::Paint,
            EditorTool::Interactables | EditorTool::Warps => PanelKind::Objects,
        })
    }

    /// The tool a panel of `kind` should activate, given the `current` tool (so
    /// re-activating the Objects panel keeps its Interact/Warp sub-tab). `None`
    /// for panels (Maps) that don't own the canvas.
    pub(super) fn panel_tool(kind: PanelKind, current: EditorTool) -> Option<EditorTool> {
        match kind {
            // The Layers panel only edits shared layer state (selection /
            // visibility / order); picking a layer must not steal the canvas tool
            // from Paint. (`Layers` stays reachable as a neutral tool via key 1.)
            PanelKind::Layers => None,
            // The Paint panel hosts both Paint and its Select sub-mode; keep
            // whichever is current so clicking the panel body doesn't flip it.
            PanelKind::Paint => Some(
                if matches!(current, EditorTool::Paint | EditorTool::Select) {
                    current
                } else {
                    EditorTool::Paint
                },
            ),
            PanelKind::Objects => Some(
                if matches!(current, EditorTool::Interactables | EditorTool::Warps) {
                    current
                } else {
                    EditorTool::Interactables
                },
            ),
            // Map settings, the Maps browser, the Dialog editor and the Critters
            // list don't own the canvas tool.
            PanelKind::Maps | PanelKind::Map | PanelKind::Dialogue | PanelKind::Presets => None,
        }
    }

    pub(super) fn build_layers(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        map: &MapInfo,
        maps: &MapStore,
    ) {
        rows.push(b.text("LAYERS:").color(13).full_width(8.0).id());
        // Plane filters (bg / spr / fg): a chip click solos its plane, further
        // clicks add planes back in (see [`LayerFilter::click`]). All on by
        // default — a filter narrows the map-ordered list below, it never moves
        // the selection. Replaces the old title-click plane paging.
        let f_bg = Self::toggle_button(b, "bg", self.filter.bg, EditorKey::LayerFilter(Plane::Bg));
        let f_spr = Self::toggle_button(
            b,
            "spr",
            self.filter.sprite,
            EditorKey::LayerFilter(Plane::Sprite),
        );
        let f_fg = Self::toggle_button(b, "fg", self.filter.fg, EditorKey::LayerFilter(Plane::Fg));
        rows.push(b.row(1.0, [f_bg, f_spr, f_fg]).id());

        // Toolbar (two rows): add / duplicate / delete; then move up / down /
        // rename / cycle plane. The collision layer (first tile layer) is
        // protected from delete / move / rename / plane-cycle — a non-bg plane
        // would move it out of the collision slot. Its protection keys off the
        // store index, so hiding rows with a filter can't mis-target it.
        let collision_src = self.collision_src(maps, &map.source);
        let collision = collision_src.is_some() && Some(self.layer_index) == collision_src;
        let add = Self::action_button(b, "+L", 11, true, EditorKey::LayerAdd);
        let dup = Self::action_button(b, "dup", 12, !collision, EditorKey::LayerDup);
        let del = Self::action_button(b, "del", 8, !collision, EditorKey::LayerDel);
        rows.push(b.row(1.0, [add, dup, del]).id());
        let up = Self::action_button(b, "^", 12, !collision, EditorKey::LayerUp);
        let dn = Self::action_button(b, "v", 12, !collision, EditorKey::LayerDown);
        let ren = Self::action_button(b, "ren", 12, !collision, EditorKey::LayerRename);
        // `pln` cycles the selected layer BG → Sprite → FG (writes the `plane`
        // property, no rename); its label is the selected layer's current plane
        // and it highlights (11) on the non-default planes.
        let sel_plane = self.selected_plane(map);
        let plane = Self::action_button(
            b,
            plane_short(sel_plane),
            if sel_plane == Plane::Bg { 12 } else { 11 },
            !collision,
            EditorKey::LayerPlane,
        );
        rows.push(b.row(1.0, [up, dn, ren, plane]).id());

        // A layer drag in progress recolours the grabbed row (grey) and the row
        // it would drop onto (green); see `reorder_drag`. `from`/`at` are store
        // indices (rows are keyed by `source_layer`), so they match `src` below.
        let drag = self.reorder_drag(ReorderList::Layers);
        let store = maps.get(&map.source);
        // Every layer in map order, one row each; filtered planes are skipped at
        // render time only — selection, keys and protection all use the store
        // index, so a hidden row never shifts what the visible ones mean.
        for (plane, layer) in self.layers_in_order(map) {
            let src = layer.source_layer;
            if !self.filter.shows(plane) {
                continue;
            }
            let is_collision = Some(src) == collision_src;
            // Eye toggles visibility; the name selects the layer (sticky, by
            // click). The colour flags the kind: red = the protected collision
            // layer, grey = an image layer (never a paint target), else a plain
            // tile layer.
            let eye = b
                .text(if layer.visible { "O" } else { "-" })
                .small(true)
                .center()
                .color(if layer.visible { 11 } else { 13 })
                .size(7.0, 7.0)
                .key(EditorKey::LayerVis(src))
                .id();
            let renaming = matches!(
                &self.editing,
                Some(e) if e.field == EditField::LayerName && e.target == src
            );
            let src_name = store.and_then(|tm| tm.layer_name(src)).unwrap_or("");
            let label = if renaming {
                format!(
                    "{}_",
                    self.editing.as_ref().map(|e| e.buffer.text()).unwrap_or("")
                )
            } else if is_collision {
                "collision".to_string()
            } else if src_name.is_empty() {
                format!("Layer {src}")
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
                .fill_if(!renaming && src == self.layer_index, 15)
                .fill_if(drag.is_some_and(|(from, _)| src == from), 13)
                .fill_if(drag.is_some_and(|(from, at)| src == at && at != from), 11)
                .key(EditorKey::Layer(src))
                .id();
            // Compact plane tag (bg dim / spr green / fg white) so the map-ordered
            // list reads each row's plane at a glance.
            let tag = b
                .text(plane_short(plane))
                .small(true)
                .center()
                .color(match plane {
                    Plane::Bg => 13,
                    Plane::Sprite => 11,
                    Plane::Fg => 12,
                })
                .size(15.0, 7.0)
                .id();
            rows.push(b.row(1.0, [eye, name, tag]).id());
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
    pub(super) fn build_paint(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        map: &MapInfo,
    ) {
        let target = plane_short(self.selected_plane(map)).to_uppercase();
        let (bw, bh) = self.brush_size();
        let info = if bw > 1 || bh > 1 {
            format!(
                "T{} {bw}x{bh} {target}{}",
                self.selected_tile, self.layer_index
            )
        } else {
            format!("Tile {} {target}{}", self.selected_tile, self.layer_index)
        };
        rows.push(b.text(info).small(true).color(13).full_width(8.0).id());

        // Eraser: paints the empty tile (0). Highlights when it's the brush.
        let erasing = self.selected_tile == 0;
        let eraser = b
            .text("eraser")
            .small(true)
            .center()
            .color(if erasing { 0 } else { 12 })
            .full_width(7.0)
            .key(EditorKey::Eraser);
        let eraser = if erasing {
            eraser.fill(8)
        } else {
            eraser.outlined(0, 8)
        };
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
    pub(super) fn build_paint_tabs(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>) {
        // A tool tab is just a toggle button keyed by the tool it selects.
        let mk = |b: &mut UiBuilder<EditorKey>, tool: EditorTool, label: &str, sel: bool| {
            Self::toggle_button(b, label, sel, EditorKey::Tool(tool))
        };
        let pt = mk(
            b,
            EditorTool::Paint,
            "Paint",
            self.tool == EditorTool::Paint,
        );
        let sl = mk(
            b,
            EditorTool::Select,
            "Sel",
            self.tool == EditorTool::Select,
        );
        rows.push(b.row(1.0, [pt, sl]).id());
    }

    /// The Select tool's body: the marquee/clipboard sizes and the ops that act
    /// on them (copy / cut / paste / delete / clear). Disabled ops grey out.
    pub(super) fn build_select(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>) {
        let sel = match self.selection {
            Some(s) => format!("sel {}x{}", s.w, s.h),
            None => "drag to select".to_string(),
        };
        let clip = match &self.clipboard {
            Some(c) => format!("clip {}x{}", c.w, c.h),
            None => "clip -".to_string(),
        };
        self.header_row(b, rows, sel, 8.0);
        self.header_row(b, rows, clip, 8.0);

        let has_sel = self.selection.is_some();
        let has_clip = self.clipboard.is_some();
        let copy = Self::action_button(b, "copy", 12, has_sel, EditorKey::SelCopy);
        let cut = Self::action_button(b, "cut", 12, has_sel, EditorKey::SelCut);
        rows.push(b.row(1.0, [copy, cut]).id());
        let paste = Self::action_button(b, "paste", 11, has_sel && has_clip, EditorKey::SelPaste);
        let del = Self::action_button(b, "del", 8, has_sel, EditorKey::SelDelete);
        rows.push(b.row(1.0, [paste, del]).id());
        let clear = Self::action_button(b, "clear", 12, has_sel, EditorKey::SelClear);
        rows.push(b.row(1.0, [clear]).id());
    }

    /// Whether `object` (on `map`) is a pickup already collected in this save —
    /// its `<map>#<id>` key is in the cached [`taken`](Self::taken) snapshot. Only
    /// a removable object with a stable id can be taken; everything else reads
    /// `false`. Mirrors [`WalkaroundState::object_taken`], the use-time skip this
    /// badges (so the panel shows exactly what gameplay hides).
    pub(super) fn is_object_taken(&self, map: &MapInfo, object: &MapObject) -> bool {
        object.removable
            && object
                .id
                .is_some_and(|id| self.taken.contains(&SaveData::taken_key(&map.source, id)))
    }

    pub(super) fn build_objects(
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
        let new = Self::action_button(b, "+new", 11, true, EditorKey::NewObject);
        let dup = Self::action_button(b, "dup", 12, has_sel, EditorKey::DupObject);
        let del = Self::action_button(b, "-del", 8, has_sel, EditorKey::DeleteObject);
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
            let mut label = match &object.effect {
                ObjectEffect::Warp(w) => {
                    let dest = w.map.as_deref().unwrap_or("-");
                    format!("{row}: ->{dest}")
                }
                ObjectEffect::Interact(Interaction::Dialogue(k)) => format!("{row}: {k}"),
                ObjectEffect::Interact(Interaction::Cutscene(n)) => format!("{row}: ~{n}"),
                ObjectEffect::Interact(Interaction::Func(_)) => format!("{row}: <fn>"),
                ObjectEffect::Interact(Interaction::None) => format!("{row}: <->"),
            };
            // A collected pickup stays listed (it's still in the map data) but
            // reads as "gone in this save": marked and dimmed, distinct from the
            // white selection fill.
            let taken = self.is_object_taken(map, object);
            if taken {
                label.push_str(" [taken]");
            }
            rows.push(
                b.text(label)
                    .small(true)
                    .color(if taken { 8 } else { 12 })
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
            self.header_row(b, rows, "box:", 7.0);
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
                    // Open the destination map in the editor, centred on the
                    // landing point. Disabled for a same-map warp (no `map`).
                    let open =
                        Self::action_button(b, "open", 11, w.map.is_some(), EditorKey::OpenWarpDest);
                    // Fullscreen 1:1 placement overlay (works for same-map warps too).
                    let place =
                        Self::action_button(b, "place", 11, true, EditorKey::WarpPreviewOpen);
                    rows.push(b.row(2.0, [open, place]).id());
                    self.field_row(b, rows, EditField::ToX, "x", &w.to.x.to_string());
                    self.field_row(b, rows, EditField::ToY, "y", &w.to.y.to_string());
                    // Click-to-place destination preview, grouped right under the
                    // destination map/coords + open/place buttons above it: a
                    // rendered map of the warp target with the player at the landing
                    // point. Drawn over this box (see `draw_warp_preview`); clicks
                    // land here.
                    self.header_row(b, rows, "land:", 7.0);
                    rows.push(
                        b.boxed([])
                            .size((rect.w as f32 - 2.0).max(THUMB_W), WARP_PREVIEW_H)
                            .fill(0)
                            .outline(13)
                            .key(EditorKey::WarpPreview)
                            .id(),
                    );
                    // Warp behaviour, below the destination group.
                    self.cycle_row(b, rows, CycleField::Flip, "flip", axis_label(&w.flip));
                    self.cycle_row(b, rows, CycleField::Mode, "mode", mode_label(&w.mode));
                    self.cycle_row(b, rows, CycleField::Sound, "snd", sound_label(&w.sound));
                    self.cycle_row(b, rows, CycleField::Trigger, "trig", object.trigger.name());
                    let narr = w.narration.as_deref().unwrap_or("-");
                    self.field_row(b, rows, EditField::Narration, "narr", narr);
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
                        Interaction::Cutscene(name) => {
                            self.field_row(b, rows, EditField::Scene, "name", name)
                        }
                        Interaction::Func(InteractFn::Note(p)) => {
                            self.field_row(b, rows, EditField::Pitch, "pitch", &p.to_string())
                        }
                        Interaction::Func(InteractFn::AddCreatures(c)) => {
                            self.field_row(b, rows, EditField::Count, "count", &c.to_string())
                        }
                        Interaction::Func(InteractFn::GiveItem(key)) => {
                            self.field_row(b, rows, EditField::Item, "item", key)
                        }
                        // None / toggle_dog / piano have no editable param.
                        _ => {}
                    }
                    self.cycle_row(b, rows, CycleField::Trigger, "trig", object.trigger.name());
                    // Consume-on-interact: does this interaction pick up / vanish?
                    // (Authoring: *whether* it's a pickup — not this save's state.)
                    self.cycle_row(
                        b,
                        rows,
                        CycleField::Removable,
                        "take",
                        removable_label(object.removable),
                    );
                    // Only a removable object with a stable id can be collected;
                    // for those, show this save's collected state and a test toggle
                    // to un-take / re-take it (distinct from the "take" authoring
                    // toggle above). Fires through `pending_taken_toggle`.
                    if object.removable && object.id.is_some() {
                        let taken = self.is_object_taken(map, object);
                        self.header_row(
                            b,
                            rows,
                            format!("collected: {}", if taken { "yes" } else { "no" }),
                            7.0,
                        );
                        let toggle = Self::action_button(
                            b,
                            if taken { "un-take" } else { "re-take" },
                            14,
                            true,
                            EditorKey::TakenToggle,
                        );
                        rows.push(b.row(2.0, [toggle]).id());
                    }
                }
            }
            // The flag gate (`if` / `unless` / `sets`) is common to every object
            // kind — shown once below the per-kind params, above the sprite.
            self.build_gate(b, rows, object);
            self.build_sprite_frames(b, rows, object);
        }
    }

    /// The selected object's flag [`Gate`](crate::world::map::Gate): the `if` /
    /// `unless` conditions and the `sets` one-shot latch. Each is a free-text
    /// story-flag name (empty ⇒ that condition is unset). A name not in the loaded
    /// `#flag` vocabulary ([`flag_names`](MapViewer::flag_names)) is marked with a
    /// trailing `?`, so a typo — which would silently make the object never fire —
    /// is visible while authoring. Common to every object kind.
    pub(super) fn build_gate(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        object: &MapObject,
    ) {
        rows.push(b.spacer(2.0).id());
        self.header_row(b, rows, "gate:", 7.0);
        self.gate_field(b, rows, EditField::CondIf, "if", object.gate.if_flag.as_deref());
        self.gate_field(
            b,
            rows,
            EditField::CondUnless,
            "unless",
            object.gate.unless_flag.as_deref(),
        );
        self.gate_field(b, rows, EditField::Sets, "sets", object.gate.sets.as_deref());
    }

    /// One gate field row: `-` when unset, the flag name otherwise, with a
    /// trailing `?` when that name isn't in the declared `#flag` vocabulary. The
    /// marker is display-only (the edit buffer, and so what commits, is untouched).
    pub(super) fn gate_field(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        field: EditField,
        label: &str,
        flag: Option<&str>,
    ) {
        let undeclared =
            flag.is_some_and(|f| !f.is_empty() && !self.flag_names.iter().any(|n| n == f));
        let value = match flag {
            Some(f) if !f.is_empty() && undeclared => format!("{f} ?"),
            Some(f) if !f.is_empty() => f.to_string(),
            _ => "-".to_string(),
        };
        self.field_row(b, rows, field, label, &value);
    }

    /// The selected object's animated-sprite controls: a row per frame (tile id +
    /// duration, the active one highlighted), add / remove buttons, and — when a
    /// frame is selected — its editable tile / duration fields plus a button that
    /// stamps the current palette brush tile into it.
    pub(super) fn build_sprite_frames(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        object: &MapObject,
    ) {
        rows.push(b.spacer(2.0).id());
        self.header_row(b, rows, "sprite:", 7.0);
        let frames = object.sprite.as_deref().unwrap_or(&[]);
        // A live animated preview of the frames (painted over in `draw_at`).
        if !frames.is_empty() {
            rows.push(
                b.boxed([])
                    .size(SPRITE_PREVIEW_PX, SPRITE_PREVIEW_PX)
                    .fill(0)
                    .outline(13)
                    .key(EditorKey::SpritePreview)
                    .id(),
            );
        }
        let sel = self.sprite_frame.min(frames.len().saturating_sub(1));
        // A frame drag in progress recolours the grabbed row (grey) and the row
        // it would drop onto (green); see `reorder_drag`.
        let drag = self.reorder_drag(ReorderList::SpriteFrames);
        for (fi, frame) in frames.iter().enumerate() {
            rows.push(
                b.text(format!("{fi}: t{} d{}", frame.spr_id, frame.duration))
                    .small(true)
                    .full_width(7.0)
                    .fill_if(fi == sel, 15)
                    .fill_if(drag.is_some_and(|(from, _)| fi == from), 13)
                    .fill_if(drag.is_some_and(|(from, at)| fi == at && at != from), 11)
                    .key(EditorKey::SpriteFrame(fi))
                    .id(),
            );
        }
        // Add / remove, then move the selected frame earlier / later — the
        // click counterpart to dragging a frame row (reorder needs >1 frame).
        let multi = frames.len() > 1;
        let add = Self::action_button(b, "+frm", 11, true, EditorKey::SpriteAddFrame);
        let del = Self::action_button(b, "-frm", 8, !frames.is_empty(), EditorKey::SpriteDelFrame);
        let up = Self::action_button(b, "^", 12, multi, EditorKey::SpriteFrameUp);
        let dn = Self::action_button(b, "v", 12, multi, EditorKey::SpriteFrameDown);
        rows.push(b.row(1.0, [add, del, up, dn]).id());
        if let Some(frame) = frames.get(sel) {
            self.field_row(
                b,
                rows,
                EditField::FrameTile,
                "tile",
                &frame.spr_id.to_string(),
            );
            self.field_row(
                b,
                rows,
                EditField::FrameDuration,
                "dur",
                &frame.duration.to_string(),
            );
            self.field_row(
                b,
                rows,
                EditField::FrameOffX,
                "offx",
                &frame.pos.x.to_string(),
            );
            self.field_row(
                b,
                rows,
                EditField::FrameOffY,
                "offy",
                &frame.pos.y.to_string(),
            );
            self.field_row(
                b,
                rows,
                EditField::FrameW,
                "w",
                &frame.options.w.to_string(),
            );
            self.field_row(
                b,
                rows,
                EditField::FrameH,
                "h",
                &frame.options.h.to_string(),
            );
            self.field_row(
                b,
                rows,
                EditField::FrameScale,
                "scale",
                &frame.options.scale.to_string(),
            );
            self.cycle_row(
                b,
                rows,
                CycleField::FrameFlip,
                "flip",
                flip_label(&frame.options.flip),
            );
            self.cycle_row(
                b,
                rows,
                CycleField::FrameRotate,
                "rot",
                rotate_label(&frame.options.rotate),
            );
            self.field_row(
                b,
                rows,
                EditField::FramePaletteRot,
                "pal",
                &frame.palette_rotate.to_string(),
            );
            // `Option<u8>`: `-` is the absent state (cleared by an empty buffer).
            let trans = frame
                .options
                .transparent
                .map_or_else(|| "-".to_string(), |t| t.to_string());
            let outline = frame
                .outline_colour
                .map_or_else(|| "-".to_string(), |o| o.to_string());
            self.field_row(b, rows, EditField::FrameTransparent, "trans", &trans);
            self.field_row(b, rows, EditField::FrameOutline, "outl", &outline);
            let grab =
                Self::action_button(b, "set from brush", 12, true, EditorKey::SpriteFromBrush);
            rows.push(b.row(1.0, [grab]).id());
        }
    }

    /// The Dialog panel: a faithful **preview** of the dialogue an object
    /// triggers, a **browser** to assign a key, and a link to **edit** it in the
    /// text editor. Editing moved to the text editor (F2) — the one canonical,
    /// lossless route — so this panel previews and assigns only. The preview box
    /// is painted over in `draw_at`.
    pub(super) fn build_dialogue(&self, b: &mut UiBuilder<EditorKey>, rows: &mut Vec<NodeId>) {
        let msg_count = self.dialogue_preview.len();
        match &self.dialogue_key {
            None => {
                self.header_row(b, rows, "pick a key below,", 7.0);
                self.header_row(b, rows, "or select an object", 7.0);
            }
            Some(key) => {
                self.header_row(b, rows, format!("key: {}", truncate(key, 18)), 7.0);
                let shown = if msg_count == 0 {
                    0
                } else {
                    self.dialogue_msg + 1
                };
                self.header_row(b, rows, format!("msg {shown}/{msg_count}"), 7.0);
                let prev =
                    Self::action_button(b, "<", 12, self.dialogue_msg > 0, EditorKey::DlgMsgPrev);
                let next = Self::action_button(
                    b,
                    ">",
                    12,
                    self.dialogue_msg + 1 < msg_count,
                    EditorKey::DlgMsgNext,
                );
                rows.push(b.row(1.0, [prev, next]).id());
                let edit =
                    Self::action_button(b, "edit in text editor", 11, true, EditorKey::DlgOpenText);
                rows.push(b.row(1.0, [edit]).id());
            }
        }

        rows.push(b.spacer(2.0).id());
        self.header_row(b, rows, "keys:", 7.0);
        let current = self.dialogue_key.as_deref();
        for (i, key) in self.dialogue_keys.iter().enumerate() {
            let sel = current == Some(key.as_str());
            rows.push(
                b.text(truncate(key, 16))
                    .small(true)
                    .color(if sel { 0 } else { 12 })
                    .full_width(7.0)
                    .fill_if(sel, 15)
                    .key(EditorKey::DlgPick(i))
                    .id(),
            );
        }
    }

    /// A toolbar action button: shows `colour` when `on`, dim grey (13) when
    /// disabled; outlined, full-width and growing to share its row evenly. The
    /// one body behind the Maps / Layers / Select / Objects toolbars.
    pub(super) fn action_button(
        b: &mut UiBuilder<EditorKey>,
        label: &str,
        colour: u8,
        on: bool,
        key: EditorKey,
    ) -> NodeId {
        b.text(label)
            .small(true)
            .center()
            .color(if on { colour } else { 13 })
            .full_width(7.0)
            .grow(1.0)
            .outlined(0, if on { colour } else { 13 })
            .key(key)
            .id()
    }

    /// A toggle/tab button: a filled (11) highlight with dark text (0) when `on`,
    /// grey (12) when off. Shared by the tool tabs and the Setup camera toggles.
    pub(super) fn toggle_button(
        b: &mut UiBuilder<EditorKey>,
        label: &str,
        on: bool,
        key: EditorKey,
    ) -> NodeId {
        b.text(label)
            .small(true)
            .center()
            .color(if on { 0 } else { 12 })
            .full_width(7.0)
            .grow(1.0)
            .fill(if on { 11 } else { 0 })
            .outline(12)
            .key(key)
            .id()
    }

    pub(super) fn field_row(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        field: EditField,
        label: &str,
        value: &str,
    ) {
        let editing = self.editing_field() == Some(field);
        let text = match &self.editing {
            Some(e) if e.field == field => format!("{label}:{}", e.buffer.display()),
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
        // While a vocabulary field (an interaction key or gate flag) is being
        // edited, list the top prefix matches from its known vocabulary as
        // clickable rows directly under it. The first is the Tab target, filled
        // to mark it. Empty for numeric / free-form fields, so those add nothing.
        if editing {
            for (i, suggestion) in self.autocomplete_suggestions().into_iter().enumerate() {
                let top = i == 0;
                rows.push(
                    b.text(truncate(&suggestion, 16))
                        .small(true)
                        .color(if top { 0 } else { 12 })
                        .full_width(7.0)
                        .fill(if top { 11 } else { 0 })
                        .outline(13)
                        .key(EditorKey::Suggest(i))
                        .id(),
                );
            }
        }
    }

    pub(super) fn cycle_row(
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

    /// A static, non-interactive panel label row in the dim (13) small font —
    /// section headers and read-only readouts. `h` is the row height (the panels
    /// mix 8px header rows with 7px readouts).
    pub(super) fn header_row(
        &self,
        b: &mut UiBuilder<EditorKey>,
        rows: &mut Vec<NodeId>,
        text: impl Into<String>,
        h: f32,
    ) {
        rows.push(b.text(text).small(true).color(13).full_width(h).id());
    }

    // --- Helpers --------------------------------------------------------------
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::script::eggtext;

    fn vocab(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Stepping then drawing with the Dialog panel open, an object selected and a
    /// matching script entry: the panel follows the selection into its key,
    /// resolves the faithful preview, and the box draws without panicking. (The
    /// "edit in text editor" link, not the panel, edits the dialogue.)
    #[test]
    fn dialogue_panel_follows_selection_and_draws() {
        use crate::platform::test_console::TestConsole;

        let mut console = TestConsole::new();
        let mut draw = DrawState::default();
        let mut store = MapStore::default();
        let screen = (240.0, 136.0);

        let mut script = Script::default();
        script.set_base(eggtext::parse("#dialogue greet\n    Hello there!").unwrap());
        let save = SaveData::default();

        let mut map = MapInfo::default();
        map.objects
            .push(MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "greet"));

        let mut viewer = MapViewer {
            focused: true,
            ..Default::default()
        };
        viewer.dock.toggle_panel(PanelKind::Dialogue);
        viewer.selected = Some(0);
        viewer.step_map_viewer_at(
            &mut console,
            &EggInput::new(),
            &mut map,
            &mut store,
            Vec2::new(0, 0),
            screen,
            (0, 0),
            &script,
            &save,
        );

        assert_eq!(
            viewer.dialogue_key.as_deref(),
            Some("greet"),
            "panel follows the selected object's key"
        );
        assert_eq!(viewer.dialogue_preview.len(), 1, "one previewed message");

        viewer.draw_at(&mut draw, &EggInput::new(), &Font::blank(), &map, &store, Vec2::new(0, 0));
    }

    /// The key/flag autocomplete matcher: a prefix filter that preserves the
    /// vocabulary's (sorted) order, caps at [`AUTOCOMPLETE_MAX`], drops an exact
    /// match, and treats an empty prefix as "show the first few".
    #[test]
    fn autocomplete_matches_filters_caps_and_orders() {
        let words = vocab(&[
            "door_open", "door_shut", "gate", "gate_two", "greet", "wave",
        ]);
        // Prefix filter, keeping input order.
        assert_eq!(
            autocomplete_matches(&words, "door"),
            vocab(&["door_open", "door_shut"])
        );
        // Case-sensitive: an upper-case prefix matches nothing here.
        assert!(autocomplete_matches(&words, "Door").is_empty());
        // A fully-typed name has nothing left to complete — the exact entry is
        // dropped, but longer entries sharing it as a prefix stay.
        assert_eq!(autocomplete_matches(&words, "gate"), vocab(&["gate_two"]));
        // No match ⇒ empty.
        assert!(autocomplete_matches(&words, "zzz").is_empty());
        // Empty prefix offers the first few, in order.
        assert_eq!(
            autocomplete_matches(&words, ""),
            vocab(&["door_open", "door_shut", "gate", "gate_two", "greet"]),
            "capped at AUTOCOMPLETE_MAX (5), preserving order"
        );
        // The cap holds when many entries share the prefix.
        let many: Vec<String> = (0..9).map(|n| format!("flag_{n}")).collect();
        let hits = autocomplete_matches(&many, "flag_");
        assert_eq!(hits.len(), AUTOCOMPLETE_MAX);
        assert_eq!(hits, vocab(&["flag_0", "flag_1", "flag_2", "flag_3", "flag_4"]));
    }

    /// `autocomplete_suggestions` routes each editable field to the right
    /// vocabulary — dialogue keys for an interaction/narration key, the `#flag`
    /// vocabulary for the gate fields — reads the live buffer as the prefix, and
    /// stays empty for a numeric field (no dropdown).
    #[test]
    fn autocomplete_suggestions_pick_vocab_per_field() {
        let mut v = MapViewer {
            dialogue_keys: vocab(&["greet_dog", "greet_egg", "wave"]),
            flag_names: vocab(&["met_dog", "met_egg"]),
            ..Default::default()
        };
        // No field focused ⇒ nothing to suggest.
        assert!(v.autocomplete_suggestions().is_empty());
        // A dialogue key field completes against the dialogue keys.
        v.editing = Some(TextEdit {
            field: EditField::Key,
            buffer: TextField::new("greet"),
            target: 0,
        });
        assert_eq!(
            v.autocomplete_suggestions(),
            vocab(&["greet_dog", "greet_egg"])
        );
        // A warp's narration key shares that vocabulary.
        v.editing.as_mut().unwrap().field = EditField::Narration;
        assert_eq!(
            v.autocomplete_suggestions(),
            vocab(&["greet_dog", "greet_egg"])
        );
        // A gate flag field completes against the `#flag` vocabulary.
        v.editing = Some(TextEdit {
            field: EditField::CondIf,
            buffer: TextField::new("met"),
            target: 0,
        });
        assert_eq!(v.autocomplete_suggestions(), vocab(&["met_dog", "met_egg"]));
        // A numeric field has no vocabulary ⇒ no dropdown.
        v.editing.as_mut().unwrap().field = EditField::HitX;
        assert!(v.autocomplete_suggestions().is_empty());
    }
}

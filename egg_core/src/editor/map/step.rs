//! Per-frame driver + input routing for the map editor: the `step_map_viewer`
//! entry points, the pointer / shortcut / dock-click dispatch, and the editor
//! lifecycle (open, layout persistence, per-map reset).

use super::*;

impl MapViewer {
    /// True while a text field is capturing keyboard input — the host suppresses
    /// its global debug hotkeys so typed dialogue keys don't trigger them.
    pub fn is_typing(&self) -> bool {
        self.editing.is_some()
            || self.maps_dialog.is_typing()
            || self.warp_preview.is_some()
            || self.path_recorder.is_some()
            || self.scene_picker.is_some()
            || self.walk_editor.is_some()
    }

    /// The field currently focused for text entry, if any.
    pub(super) fn editing_field(&self) -> Option<EditField> {
        self.editing.as_ref().map(|e| e.field)
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
    pub(super) fn load_layout(&mut self, system: &mut impl ConsoleApi) {
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
    pub(super) fn save_layout(&mut self, system: &mut impl ConsoleApi) {
        self.dock.dirty = false;
        let layout = DockLayout {
            panels: self.dock.panels.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&layout) {
            system.write_file(LAYOUT_PATH, json.as_bytes());
        }
    }

    // --- Layout (rebuilt each frame for both hit-testing and drawing) ---------

    #[allow(clippy::too_many_arguments)]
    pub fn step_map_viewer(
        &mut self,
        system: &mut impl ConsoleApi,
        input: &EggInput,
        map: &mut MapInfo,
        maps: &mut MapStore,
        camera_pos: Vec2,
        sheet: (usize, usize),
        script: &Script,
        save: &SaveData,
    ) {
        let screen = (system.width() as f32, system.height() as f32);
        self.step_map_viewer_at(system, input, map, maps, camera_pos, screen, sheet, script, save);
    }

    /// Like [`step_map_viewer`](Self::step_map_viewer) but with an explicit
    /// `screen` size for the panel layout/hit-testing. An extra view's
    /// framebuffer can be any size, while `system.width()/height()` is always
    /// the *main* window's framebuffer.
    #[allow(clippy::too_many_arguments)]
    pub fn step_map_viewer_at(
        &mut self,
        system: &mut impl ConsoleApi,
        input: &EggInput,
        map: &mut MapInfo,
        maps: &mut MapStore,
        camera_pos: Vec2,
        screen: (f32, f32),
        sheet: (usize, usize),
        script: &Script,
        save: &SaveData,
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
        self.advance_sprite_preview(map);
        // Follow the selection into the Dialog panel and resolve its faithful
        // preview, so both the hit pass and draw pass read cached state.
        self.sync_dialogue(map, script, save);
        // Cache the save's taken set so the objects panel can badge collected
        // pickups in the draw pass (which has no `SaveData`).
        self.taken = save.taken.clone();
        // Cache the declared flag vocabulary so the objects panel can flag an
        // undeclared name in an object's gate field (a typo that never fires).
        self.flag_names = script.flags().into_iter().collect();

        // Restore the saved dock layout once, lazily, on first focus (primary).
        if self.persist && !self.dock.loaded {
            self.load_layout(system);
        }

        // The live path recorder is fully modal (like warp placement).
        if self.path_recorder.is_some() {
            self.dock.recompute(screen);
            self.step_path_recorder(system, input, map, maps, screen);
            return;
        }
        // A fullscreen warp-destination placement session is fully modal: it draws
        // over the editor and captures all input until confirmed or cancelled.
        if self.warp_preview.is_some() {
            self.dock.recompute(screen);
            self.step_warp_preview(input, map, maps, screen);
            return;
        }
        // The scene picker is fully modal (like the recorder/warp placement).
        if self.scene_picker.is_some() {
            self.dock.recompute(screen);
            self.step_scene_picker(input);
            return;
        }
        // The walk-sprite editor is fully modal (like the rest).
        if self.walk_editor.is_some() {
            self.dock.recompute(screen);
            self.step_walk_editor(system, input);
            return;
        }
        // `R` opens the path recorder — but not while a text field or the maps
        // dialog is capturing keys (else typing an `r` would abort the edit).
        if self.editing.is_none()
            && !self.maps_dialog.is_active()
            && input.keyp(ScanCode::R)
        {
            self.dock.recompute(screen);
            self.open_path_recorder(map, maps, camera_pos);
            return;
        }
        // `P` opens the scene picker — pick any saved cutscene to replay in the
        // scrubber (same guards as `R`). The engine, which owns the registry,
        // pushes the names in via `scene_names`.
        if self.editing.is_none()
            && !self.maps_dialog.is_active()
            && input.keyp(ScanCode::P)
        {
            self.dock.recompute(screen);
            self.open_scene_picker();
            return;
        }

        if self.maps_dialog.is_active() {
            // A modal map dialog (new / rename / delete) captures all input.
            self.step_maps_dialog(system, input, maps);
        } else if self.editing.is_some() {
            // While a text field is focused all keys feed the buffer — don't let
            // editor shortcuts (incl. a typed "z") fire.
            self.step_text_entry(input, map, maps);
        } else {
            self.handle_shortcuts(system, input, map, maps);
        }

        // Tile the panels once; both this hit pass and the later draw pass read
        // the same `self.dock.solved`, so they can't disagree about geometry.
        self.dock.recompute(screen);
        // Cache the Paint palette's viewport rect for the pan/pick + draw math.
        let pal_rect = self
            .dock
            .open_panel(PanelKind::Paint)
            .and_then(|(idx, rect)| {
                self.build_panel(idx, rect, map, maps).rect_at(
                    rect.x,
                    rect.y,
                    EditorKey::PaletteView,
                )
            });
        self.pal_rect = pal_rect.unwrap_or_default();
        // A modal map dialog (new / rename / delete) swallows all mouse
        // interaction with the panels and world; otherwise hit-test it.
        if !self.maps_dialog.is_active() {
            self.step_mouse_input(system, input, map, maps, camera_pos, screen);
        }

        // Controller fallback for the Layers tool: up/down walk the visible rows
        // in map order, A toggles visibility, B cycles the selected layer's plane
        // (the `pln` button — collision layer refused inside `cycle_layer_plane`).
        if self.tool == EditorTool::Layers {
            let pad = input.controller();
            if just_pressed(pad.up) {
                self.move_selection(map, false);
            }
            if just_pressed(pad.down) {
                self.move_selection(map, true);
            }
            if just_pressed(pad.a) {
                self.toggle_layer(map);
            }
            if just_pressed(pad.b)
                && let Some(src) = self.selected_source_layer(map)
                && self.collision_src(maps, &map.source) != Some(src)
            {
                self.cycle_layer_plane(map, maps, src);
            }
        }

        // Debounced layout save: only after a committed dock change (the drag/
        // resize/toggle handlers set `dirty`), and only the primary editor.
        if self.persist && self.dock.dirty {
            self.save_layout(system);
        }
    }

    /// Keep the Dialog panel current: refresh the pick list, follow the selected
    /// object's dialogue (or warp-narration) key, and resolve the faithful preview
    /// for this frame. Caches everything on `self` so the hit pass, panel build
    /// and draw all read the same state. Editing happens in the text editor, so
    /// this only tracks and previews the key.
    pub(super) fn sync_dialogue(&mut self, map: &MapInfo, script: &Script, save: &SaveData) {
        self.dialogue_keys = script.dialogue_keys();
        self.dialogue_small_text = save.small_text_on;

        let selected_key = self
            .selected
            .and_then(|i| map.objects.get(i))
            .and_then(|o| match &o.effect {
                ObjectEffect::Interact(Interaction::Dialogue(k)) => Some(k.clone()),
                ObjectEffect::Warp(w) => w.narration.clone(),
                _ => None,
            });

        // A browser pick wins (the input handler can't load it — it lacks the
        // `Script`). Otherwise auto-follow the selection when its key changes, so
        // clicking an object shows its dialogue.
        if let Some(key) = self.dialogue_pick.take() {
            self.set_dialogue_key(key);
        } else if let Some(key) = selected_key
            && self.dialogue_key.as_deref() != Some(key.as_str())
        {
            self.set_dialogue_key(key);
        }

        // Resolve the live script against the save, so advanced dialogue and `#if`
        // branches preview exactly as they'd play.
        let preview = match &self.dialogue_key {
            Some(key) => script.get_dialogue(key, save),
            None => Vec::new(),
        };
        self.dialogue_msg = self.dialogue_msg.min(preview.len().saturating_sub(1));
        self.dialogue_preview = preview;
    }

    /// Hit-test and dispatch one frame of mouse input across the panels and the
    /// world view, in priority order: an in-progress drag (scroll bar / palette /
    /// panel) owns the mouse, then the global bar, then a front-to-back panel
    /// pick, then the leftover world view. Gated out by the caller while a modal
    /// map dialog owns input.
    pub(super) fn step_mouse_input(
        &mut self,
        system: &mut impl ConsoleApi,
        input: &EggInput,
        map: &mut MapInfo,
        maps: &mut MapStore,
        camera_pos: Vec2,
        screen: (f32, f32),
    ) {
        let mouse = input.mouse;
        let cursor = mouse.pos();
        if matches!(self.canvas_drag, CanvasDrag::Scrollbar { .. }) {
            // A panel scroll-bar drag owns the mouse.
            self.step_scroll_drag(&mouse, map, maps);
        } else if matches!(self.canvas_drag, CanvasDrag::Palette(_)) {
            // A palette drag (pan / tile-pick / scroll bar) owns the mouse.
            self.step_palette_drag(&mouse);
        } else if matches!(self.canvas_drag, CanvasDrag::Reorder { .. }) {
            // A list drag-reorder (layers / sprite frames) owns the mouse.
            self.step_reorder_drag(map, maps, cursor, released(mouse.left));
        } else if self.step_drag(&mouse, screen) {
            // A panel drag (move / tear-off / resize) owns the mouse this frame —
            // suppress panel and canvas input so it can't paint or re-select.
        } else if let Some(key) = self.global_bar_hit(cursor) {
            // The always-on undo/redo/save bar wins over the world beneath it.
            self.handle_panel(system, input, map, maps, usize::MAX, key, camera_pos);
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
                Some((idx, key)) => self.handle_panel(system, input, map, maps, idx, key, camera_pos),
                // World gate: canvas tools fire only over the leftover world view
                // (not behind a docked strip) and only when nothing is dragging.
                None if self.dock.solved.world.contains(cursor) => {
                    self.handle_canvas(input, map, maps, camera_pos, &mouse)
                }
                None => {}
            }
        }
    }

    /// Advance (or start) a panel drag — splitter resize, float move/tear-off, or
    /// float resize. Returns `true` while a drag is active so the caller
    /// suppresses panel/canvas input. Mutations re-solve immediately, so the draw
    /// pass shows the panel under the cursor this frame (no one-frame lag).
    pub(super) fn step_drag(&mut self, mouse: &MouseInput, screen: (f32, f32)) -> bool {
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
            DragState::MovePanel {
                idx,
                grab_dx,
                grab_dy,
                arming,
            } => {
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
                        self.dock.set_float(
                            idx,
                            Vec2::new(p.x - grab_dx, p.y - grab_dy),
                            rect.w,
                            rect.h,
                        );
                        self.dock.drag = DragState::MovePanel {
                            idx,
                            grab_dx,
                            grab_dy,
                            arming: false,
                        };
                        self.dock.recompute(screen);
                    }
                    return true;
                }
                // Following the cursor: move, flag the drop edge, then re-solve so
                // draw places it under the cursor and shows the drop highlight.
                self.dock
                    .move_float(idx, Vec2::new(p.x - grab_dx, p.y - grab_dy));
                self.dock.recompute(screen);
                self.dock.solved.hot_edge = self.dock.edge_near(p, screen);
                true
            }
        }
    }

    /// Pick the always-on global bar (pinned to the world's top-left), if the
    /// cursor is over one of its buttons.
    pub(super) fn global_bar_hit(&self, cursor: Vec2) -> Option<EditorKey> {
        let world = self.dock.solved.world;
        self.build_global_bar()
            .hit_at(world.x + 1, world.y + 1, cursor)
    }

    /// Make panel `idx` the active one: switch the canvas tool to match its kind
    /// (so its content + the world interaction line up) and raise it to the
    /// front. A no-op `idx` (the global bar's `usize::MAX`) just returns.
    pub(super) fn activate_panel(&mut self, idx: usize) {
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
    pub(super) fn handle_shortcuts(
        &mut self,
        system: &mut impl ConsoleApi,
        input: &EggInput,
        map: &mut MapInfo,
        maps: &mut MapStore,
    ) {
        let ctrl = input.key(ScanCode::Ctrl);
        let shift = input.key(ScanCode::Shift);
        if ctrl {
            if input.keyp(ScanCode::Z) {
                if shift {
                    self.redo(system, map, maps);
                } else {
                    self.undo(system, map, maps);
                }
            }
            if input.keyp(ScanCode::Y) {
                self.redo(system, map, maps);
            }
            if input.keyp(ScanCode::S) {
                self.save(system, map, maps);
            }
            // Select-tool clipboard ops (Ctrl+C/X/V) on the active layer.
            if self.tool == EditorTool::Select {
                if input.keyp(ScanCode::C) {
                    self.selection_copy(maps, map);
                }
                if input.keyp(ScanCode::X) {
                    self.selection_cut(maps, map);
                }
                if input.keyp(ScanCode::V) {
                    self.selection_paste(maps, map);
                }
            }
            // Ctrl-chorded: don't also treat the digit as a tool switch.
            return;
        }

        // Delete: removes the selected object, or clears the Select marquee.
        if input.keyp(ScanCode::Delete) {
            if matches!(self.tool, EditorTool::Interactables | EditorTool::Warps) {
                self.delete_object(map);
            } else if self.tool == EditorTool::Select {
                self.selection_delete(maps, map);
            }
        }
        // Escape drops the Select marquee.
        if input.keyp(ScanCode::Escape) && self.tool == EditorTool::Select {
            self.selection = None;
        }
        // G toggles the tile-grid + coordinate overlay.
        if input.keyp(ScanCode::G) {
            self.show_grid = !self.show_grid;
        }
        // Arrow keys nudge the selected object's hitbox (8px with Shift), each
        // press one undo step — the keyboard companion to the x/y/w/h fields.
        if matches!(self.tool, EditorTool::Interactables | EditorTool::Warps) {
            let step = if shift { 8 } else { 1 };
            let (mut dx, mut dy) = (0i16, 0i16);
            if input.keyp(ScanCode::Left) {
                dx -= step;
            }
            if input.keyp(ScanCode::Right) {
                dx += step;
            }
            if input.keyp(ScanCode::Up) {
                dy -= step;
            }
            if input.keyp(ScanCode::Down) {
                dy += step;
            }
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
        let tool = if input.keyp(ScanCode::Digit1) {
            Some(EditorTool::Layers)
        } else if input.keyp(ScanCode::Digit2) {
            Some(EditorTool::Paint)
        } else if input.keyp(ScanCode::Digit3) {
            Some(EditorTool::Interactables)
        } else if input.keyp(ScanCode::Digit4) {
            Some(EditorTool::Warps)
        } else if input.keyp(ScanCode::Digit5) {
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
    pub(super) fn switch_tool(&mut self, tool: EditorTool) {
        self.tool = tool;
        self.selected = None;
        self.stop_editing();
        self.drag = None;
        self.stroke = None;
        self.moving = None;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_panel(
        &mut self,
        system: &mut impl ConsoleApi,
        input: &EggInput,
        map: &mut MapInfo,
        maps: &mut MapStore,
        idx: usize,
        key: EditorKey,
        camera_pos: Vec2,
    ) {
        let mouse = input.mouse;
        let click = just_pressed(mouse.left);
        // Clicking anywhere in a panel makes it the active canvas tool (the
        // global bar passes `usize::MAX`, which `activate_panel` ignores).
        if click {
            self.activate_panel(idx);
        }
        match key {
            // Title-bar press begins a move: a float follows the cursor at once;
            // a docked panel arms a tear-off (a still click just focuses, via
            // `activate_panel` above). Resize handles are picked geometrically
            // (see `step_drag`), so they never arrive here as a key.
            EditorKey::Dock(pidx) => {
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
            // Narrow the (map-ordered) list to a plane — solo on the first
            // click, build-up on later ones (see [`LayerFilter::click`]). The
            // selection is untouched, so a click here never changes what paints.
            EditorKey::LayerFilter(plane) => {
                if click {
                    self.filter.click(plane);
                }
            }
            // Sticky select: a click sets the active layer by store index (and
            // stays — no hover-select, and the canvas tool isn't changed; see
            // `panel_tool`). The press also arms a drag-reorder (except on the
            // protected collision layer, which can't move); a release in place is
            // just the select, a drag onto another row reorders (see
            // `step_reorder_drag`).
            EditorKey::Layer(i) => {
                if click {
                    self.layer_index = i;
                    // The protected collision layer can't be dragged.
                    if Some(i) != self.collision_src(maps, &map.source) {
                        self.canvas_drag = CanvasDrag::Reorder {
                            list: ReorderList::Layers,
                            from: i,
                            at: i,
                        };
                    }
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
                if click && let Some(tm) = maps.get_mut(&map.source) {
                    let name = format!("Layer {}", tm.layers.len());
                    let index = tm.add_tile_layer(&name);
                    let layer = Box::new(tm.layers[index].clone());
                    self.record(EditAction::LayerInsert {
                        source: map.source.clone(),
                        index,
                        layer,
                    });
                    // Select the new layer so an add → `pln` (make it sprite/fg)
                    // flow needs no hunting — its store index is where it landed.
                    self.layer_index = index;
                    self.pending_reload = true;
                }
            }
            EditorKey::LayerDel => {
                if click
                    && let Some(src) = self.selected_source_layer(map)
                    && let Some(layer) = maps
                        .get_mut(&map.source)
                        .and_then(|tm| tm.remove_layer_at(src))
                {
                    self.record(EditAction::LayerRemove {
                        source: map.source.clone(),
                        index: src,
                        layer: Box::new(layer),
                    });
                    // Layers above `src` slide down one; step onto the layer just
                    // before the hole (the re-derive settles a stale index).
                    self.layer_index = self.layer_index.saturating_sub(1);
                    self.pending_reload = true;
                }
            }
            EditorKey::LayerUp => {
                if click
                    && let Some(src) = self.selected_source_layer(map)
                    && let Some((a, b)) = maps
                        .get_mut(&map.source)
                        .and_then(|tm| tm.move_layer(src, true))
                {
                    self.record(EditAction::LayerSwap {
                        source: map.source.clone(),
                        a,
                        b,
                    });
                    // Follow the moved layer to its new store slot.
                    self.layer_index = b;
                    self.pending_reload = true;
                }
            }
            EditorKey::LayerDown => {
                if click
                    && let Some(src) = self.selected_source_layer(map)
                    && let Some((a, b)) = maps
                        .get_mut(&map.source)
                        .and_then(|tm| tm.move_layer(src, false))
                {
                    self.record(EditAction::LayerSwap {
                        source: map.source.clone(),
                        a,
                        b,
                    });
                    // Follow the moved layer to its new store slot.
                    self.layer_index = b;
                    self.pending_reload = true;
                }
            }
            EditorKey::LayerDup => {
                if click
                    && let Some(src) = self.selected_source_layer(map)
                    && self.collision_src(maps, &map.source) != Some(src)
                    && let Some(tm) = maps.get_mut(&map.source)
                {
                    let index = src + 1;
                    let dup = tm.layers[src].clone();
                    tm.insert_layer(index, dup);
                    let layer = Box::new(tm.layers[index].clone());
                    self.record(EditAction::LayerInsert {
                        source: map.source.clone(),
                        index,
                        layer,
                    });
                    // Select the duplicate (it landed at `index`).
                    self.layer_index = index;
                    self.pending_reload = true;
                }
            }
            EditorKey::LayerRename => {
                if click
                    && let Some(src) = self.selected_source_layer(map)
                    && self.collision_src(maps, &map.source) != Some(src)
                {
                    self.begin_layer_rename(maps, &map.source, src);
                }
            }
            EditorKey::LayerPlane => {
                if click
                    && let Some(src) = self.selected_source_layer(map)
                    && self.collision_src(maps, &map.source) != Some(src)
                {
                    self.cycle_layer_plane(map, maps, src);
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
                        let thumb_top =
                            thumb_pos(i32::from(v.y), travel, self.pal_row as i32, max_r as i32);
                        let grab = grab_offset(i32::from(p.y), thumb_top, th) as i16;
                        self.canvas_drag = CanvasDrag::Palette(PalDrag::ScrollV { grab });
                        self.scroll_palette_bar(true, p, grab);
                    } else if max_c > 0 && p.y >= v.y + v.h - PALETTE_BAR_GRAB {
                        let (tw, travel) = self.palette_thumb_h();
                        let thumb_left =
                            thumb_pos(i32::from(v.x), travel, self.pal_col as i32, max_c as i32);
                        let grab = grab_offset(i32::from(p.x), thumb_left, tw) as i16;
                        self.canvas_drag = CanvasDrag::Palette(PalDrag::ScrollH { grab });
                        self.scroll_palette_bar(false, p, grab);
                    } else {
                        // Start a brush box-select (a click stays 1×1).
                        let (c, r) = self.palette_tile_at(p);
                        self.set_brush_box(c, r, c, r);
                        self.canvas_drag = CanvasDrag::Palette(PalDrag::Select {
                            anchor_col: c,
                            anchor_row: r,
                        });
                    }
                }
            }
            EditorKey::Object(i) => {
                if click {
                    self.selected = Some(i);
                    self.sprite_frame = 0;
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
            // Un-take / re-take the selected pickup: park its `<map>#<id>` key for
            // the host to flip in `save.taken` (the editor never holds the save).
            EditorKey::TakenToggle => {
                if click
                    && let Some(id) = self.selected.and_then(|i| map.objects.get(i)).and_then(|o| o.id)
                {
                    self.pending_taken_toggle = Some(SaveData::taken_key(&map.source, id));
                }
            }
            // Select the frame for editing, and arm a drag-reorder (a release in
            // place is just the select; a drag onto another frame row reorders).
            EditorKey::SpriteFrame(fi) => {
                if click {
                    self.sprite_frame = fi;
                    self.stop_editing();
                    self.canvas_drag = CanvasDrag::Reorder {
                        list: ReorderList::SpriteFrames,
                        from: fi,
                        at: fi,
                    };
                }
            }
            EditorKey::SpriteAddFrame => {
                if click {
                    self.add_sprite_frame(map);
                }
            }
            EditorKey::SpriteDelFrame => {
                if click {
                    self.del_sprite_frame(map);
                }
            }
            EditorKey::SpriteFrameUp => {
                if click {
                    self.move_sprite_frame(map, true);
                }
            }
            EditorKey::SpriteFrameDown => {
                if click {
                    self.move_sprite_frame(map, false);
                }
            }
            EditorKey::SpriteFromBrush => {
                if click {
                    self.set_frame_from_brush(map);
                }
            }
            // A display-only box; the live frame is painted over it in `draw_at`.
            EditorKey::SpritePreview => {}
            // A click in the warp destination preview sets the landing point to
            // the clicked map pixel (clamped to the target's bounds).
            EditorKey::WarpPreview => {
                if click && let Some(rect) = self.dock.solved.rect_of(idx) {
                    let ui = self.build_panel(idx, rect, map, maps);
                    let (scroll, _) = self.panel_scroll(idx, rect, ui.content_height());
                    if let Some(box_rect) =
                        ui.rect_at(rect.x, rect.y - scroll, EditorKey::WarpPreview)
                    {
                        self.place_warp_from_preview(map, maps, box_rect, mouse.pos());
                    }
                }
            }
            // Open the selected warp's destination map, centred on its landing
            // point (the host drains `pending_open`). No-op for a same-map warp.
            EditorKey::OpenWarpDest => {
                if click {
                    self.open_selected_warp_dest(&map.objects);
                }
            }
            EditorKey::WarpPreviewOpen => {
                if click {
                    self.open_warp_preview(map, maps);
                }
            }
            // A Presets-panel row opens the walk-sprite editor on that preset.
            EditorKey::PresetRow(row) => {
                if click {
                    self.open_walk_editor(row);
                }
            }
            // The overlay's confirm/cancel are hit-tested inside `step_warp_preview`
            // (it's modal), so they never arrive through the normal panel dispatch.
            // The recorder's own controls (save/cancel/name/actor/canvas) are the
            // same — hit-tested inside `step_path_recorder`, never here.
            EditorKey::WarpPreviewOk
            | EditorKey::WarpPreviewCancel
            | EditorKey::WalkEdOk
            | EditorKey::WalkEdCancel
            | EditorKey::PathRecOk
            | EditorKey::PathRecCancel
            | EditorKey::PathRecName
            | EditorKey::PathRecActor
            | EditorKey::PathRecCanvas => {}
            // Press on a panel's scroll bar: begin a thumb drag, capturing the grab
            // offset so the thumb tracks the cursor rather than snapping under it.
            EditorKey::PanelScroll(pidx) => {
                if click && let Some(rect) = self.dock.solved.rect_of(pidx) {
                    let content_h = self.build_panel(pidx, rect, map, maps).content_height();
                    let body = Self::panel_body(rect);
                    let (scroll, _) = self.panel_scroll(pidx, rect, content_h);
                    let (top, th) = Self::scroll_thumb(body, scroll, content_h);
                    let grab =
                        grab_offset(i32::from(mouse.pos().y), i32::from(top), i32::from(th)) as i16;
                    self.canvas_drag = CanvasDrag::Scrollbar { idx: pidx, grab };
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
                    self.undo(system, map, maps);
                }
            }
            EditorKey::Redo => {
                if click {
                    self.redo(system, map, maps);
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
                            self.pending_open = Some((name, None));
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
                if click && let Some(sel) = self.maps_selected.clone() {
                    self.duplicate_map(system, maps, &sel);
                }
            }
            EditorKey::MapRename => {
                if click && let Some(sel) = self.maps_selected.clone() {
                    self.maps_dialog = MapsDialog::Rename {
                        from: sel.clone(),
                        name: TextField::new(sel),
                    };
                }
            }
            EditorKey::MapDelete => {
                if click && let Some(sel) = self.maps_selected.clone() {
                    self.maps_dialog = MapsDialog::ConfirmDelete(sel);
                }
            }
            // Setup panel. These map-level settings aren't on the undo stack —
            // they just mutate the stored map and ask for a re-derive.
            EditorKey::BgColour(c) => {
                if click {
                    if let Some(tm) = maps.get_mut(&map.source) {
                        tm.set_bg_colour(BgColour::Index(c));
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
            // Dialog browser pick: assign the key to the selected object (so the
            // panel and the object agree) and queue it for load by `sync_dialogue`.
            EditorKey::DlgPick(i) => {
                if click && let Some(key) = self.dialogue_keys.get(i).cloned() {
                    self.assign_dialogue_key(map, &key);
                    self.dialogue_pick = Some(key);
                }
            }
            // A picked autocomplete suggestion commits that whole vocabulary entry
            // into the field being edited (the click counterpart to Tab).
            EditorKey::Suggest(i) => {
                if click {
                    self.accept_suggestion(map, maps, i);
                }
            }
            EditorKey::DlgMsgPrev => {
                if click {
                    self.dialogue_msg = self.dialogue_msg.saturating_sub(1);
                }
            }
            EditorKey::DlgMsgNext => {
                if click {
                    let max = self.dialogue_preview.len().saturating_sub(1);
                    self.dialogue_msg = (self.dialogue_msg + 1).min(max);
                }
            }
            // Hand the current dialogue off to the text editor (the canonical edit
            // route): park a request the host drains to open `en.eggtext` at this
            // `#dialogue` block. Editing the source there reloads live on save.
            EditorKey::DlgOpenText => {
                if click && let Some(key) = self.dialogue_key.clone() {
                    self.pending_text_open = Some(TextOpenReq {
                        path: SCRIPT_PATH.to_string(),
                        anchor: TextAnchor::Tag(key),
                    });
                }
            }
        }
    }

    /// Clear text-entry focus: drop the whole [`TextEdit`] session (field, buffer
    /// and layer target together) so [`is_typing`](Self::is_typing) stays in step.
    pub(super) fn stop_editing(&mut self) {
        self.editing = None;
    }

    /// Forget all per-map editor state: undo/redo history, text-entry focus,
    /// object selection, and any in-progress drag/stroke. Deliberately keeps
    /// [`SaveStatus`]: tile paints land in the shared [`MapStore`], so
    /// unsaved-ness genuinely survives a map switch. (Tile undo entries are
    /// source-tagged and would replay correctly across maps, but object entries
    /// index into the replaced objects list — so the whole history goes.)
    pub(super) fn reset_for_new_map(&mut self) {
        self.history.clear();
        self.stop_editing();
        // A placement / recording / picking session belongs to the old map; drop it.
        self.warp_preview = None;
        self.path_recorder = None;
        self.scene_picker = None;
        self.selected = None;
        self.sprite_frame = 0;
        self.preview_frame = 0;
        self.preview_tick = 0;
        self.drag = None;
        self.stroke = None;
        self.moving = None;
        // The new map may have fewer layers; a stale index would dangle past its
        // layer list, leaving `active_layer` `None` (Paint silently no-ops).
        self.layer_index = 0;
        // The marquee indexes the old map's tiles; drop it. (The clipboard
        // survives, so a block can be pasted into a different map.)
        self.selection = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        use crate::platform::test_console::TestConsole;

        let mut console = TestConsole::new();
        let mut store = MapStore::default();
        let screen = (240.0, 136.0);

        let mut viewer = MapViewer::default();
        let mut map_a = MapInfo {
            source: "a".to_string(),
            ..MapInfo::default()
        };
        viewer.step_map_viewer_at(
            &mut console,
            &EggInput::new(),
            &mut map_a,
            &mut store,
            Vec2::new(0, 0),
            screen,
            (0, 0),
            &Script::default(),
            &SaveData::default(),
        );

        // Seed per-map state on map "a".
        viewer.record(tiles(vec![(0, 0, 1, 2)]));
        viewer.selected = Some(0);
        viewer.editing = Some(TextEdit {
            field: EditField::Key,
            buffer: TextField::new("x"),
            target: 0,
        });
        viewer.layer_index = 3;

        // Stepping the same map keeps it all.
        viewer.step_map_viewer_at(
            &mut console,
            &EggInput::new(),
            &mut map_a,
            &mut store,
            Vec2::new(0, 0),
            screen,
            (0, 0),
            &Script::default(),
            &SaveData::default(),
        );
        assert!(viewer.history.can_undo());
        assert!(viewer.is_typing());
        assert_eq!(viewer.selected, Some(0));

        // Stepping a different map drops it.
        let mut map_b = MapInfo {
            source: "b".to_string(),
            ..MapInfo::default()
        };
        viewer.step_map_viewer_at(
            &mut console,
            &EggInput::new(),
            &mut map_b,
            &mut store,
            Vec2::new(0, 0),
            screen,
            (0, 0),
            &Script::default(),
            &SaveData::default(),
        );
        assert!(!viewer.history.can_undo(), "object undo entries went stale");
        assert!(!viewer.is_typing(), "text focus dropped");
        assert_eq!(viewer.selected, None, "selection index went stale");
        assert_eq!(
            viewer.layer_index, 0,
            "layer index reset for the new (maybe shorter) map"
        );
    }

    /// A primary editor saves its dock arrangement and a fresh primary restores
    /// it; a view editor (non-persistent) is gated off.
    #[test]
    fn layout_persists_round_trip() {
        use crate::platform::test_console::TestConsole;

        let mut console = TestConsole::new();
        let mut a = MapViewer::primary();
        a.dock.set_side_thickness(Side::Left, 50);
        a.dock.toggle_panel(PanelKind::Maps); // open Maps (closed by default)
        a.save_layout(&mut console);
        assert!(console.files.contains_key(LAYOUT_PATH));

        let mut b = MapViewer::primary();
        b.load_layout(&mut console);
        assert!(
            b.dock
                .panels
                .iter()
                .any(|p| p.kind == PanelKind::Maps && p.open),
            "Maps stays open after reload",
        );
        assert!(
            b.dock.panels.iter().any(|p| matches!(
                p.place,
                Placement::Dock {
                    side: Side::Left,
                    size: 50
                }
            )),
            "left dock thickness restored",
        );

        // View editors never persist.
        assert!(!MapViewer::default().persist);
        assert!(MapViewer::primary().persist);
    }
}

//! Layer authoring + map-store management: select / toggle / reorder / rename
//! layers and their properties, cycle map music, and the map browser's
//! new / duplicate / rename / delete dialog.

use super::*;

impl MapViewer {
    /// Every layer in map (store) order, each tagged with the draw [`Plane`] it
    /// currently belongs to (by which derived list holds it — the collision layer
    /// and plain/`bg` layers are [`Plane::Bg`], sprite/fg layers their own plane).
    /// This is the authoring view the Layers panel lists: one row per tile/image
    /// layer, sorted by `source_layer` so file order shows regardless of plane.
    /// Object layers carry no [`LayerInfo`], so they're absent (leaving gaps in
    /// the store-index space, which is fine — nothing iterates it densely).
    pub(super) fn layers_in_order<'a>(&self, map: &'a MapInfo) -> Vec<(Plane, &'a LayerInfo)> {
        let mut all: Vec<(Plane, &LayerInfo)> = map
            .layers
            .iter()
            .map(|l| (Plane::Bg, l))
            .chain(map.sprite_layers.iter().map(|l| (Plane::Sprite, l)))
            .chain(map.fg_layers.iter().map(|l| (Plane::Fg, l)))
            .collect();
        all.sort_by_key(|(_, l)| l.source_layer);
        all
    }

    /// The selected layer — the [`LayerInfo`] whose `source_layer` matches
    /// [`layer_index`](Self::layer_index) — searched across all three plane lists
    /// (`source_layer` is unique, so at most one matches). `None` if the index is
    /// stale (e.g. after a delete) — Paint then no-ops.
    pub(super) fn selected_layer<'a>(&self, map: &'a MapInfo) -> Option<&'a LayerInfo> {
        map.layers
            .iter()
            .chain(map.sprite_layers.iter())
            .chain(map.fg_layers.iter())
            .find(|l| l.source_layer == self.layer_index)
    }

    /// The draw plane of the selected layer (defaulting to [`Plane::Bg`] when the
    /// selection is empty) — for the Paint readout and sprite-reload check.
    pub(super) fn selected_plane(&self, map: &MapInfo) -> Plane {
        self.layers_in_order(map)
            .into_iter()
            .find(|(_, l)| l.source_layer == self.layer_index)
            .map(|(plane, _)| plane)
            .unwrap_or(Plane::Bg)
    }

    /// Whether store layer `src` is a [`Plane::Sprite`] layer — its tile edits
    /// must re-derive the flood-fill components (cached at load, not read live).
    pub(super) fn is_sprite_layer(map: &MapInfo, src: usize) -> bool {
        map.sprite_layers.iter().any(|l| l.source_layer == src)
    }

    /// The store index of `source`'s collision (first tile) layer, the one the
    /// editor protects from delete / move / rename / plane-cycle. `None` for a
    /// pure-painted map (no tile layer).
    pub(super) fn collision_src(&self, maps: &MapStore, source: &str) -> Option<usize> {
        maps.get(source).and_then(|tm| tm.collision_layer())
    }

    /// Move the selection to the previous / next **visible** (filtered) row in
    /// map order — the controller's up/down. If the current selection is hidden
    /// (or stale), it snaps to the first visible row.
    pub(super) fn move_selection(&mut self, map: &MapInfo, forward: bool) {
        let rows: Vec<usize> = self
            .layers_in_order(map)
            .into_iter()
            .filter(|(plane, _)| self.filter.shows(*plane))
            .map(|(_, l)| l.source_layer)
            .collect();
        if rows.is_empty() {
            return;
        }
        let here = rows.iter().position(|&s| s == self.layer_index);
        let next = match (here, forward) {
            (Some(i), true) => (i + 1).min(rows.len() - 1),
            (Some(i), false) => i.saturating_sub(1),
            (None, _) => 0,
        };
        self.layer_index = rows[next];
    }

    /// Toggle the visibility of the currently selected layer (found by store
    /// index across all plane lists).
    pub(super) fn toggle_layer(&self, map: &mut MapInfo) {
        let index = self.layer_index;
        if let Some(layer) = map
            .layers
            .iter_mut()
            .chain(map.sprite_layers.iter_mut())
            .chain(map.fg_layers.iter_mut())
            .find(|l| l.source_layer == index)
        {
            layer.visible = !layer.visible;
        }
    }

    /// The store index of the currently-selected layer, if it still exists.
    /// Because [`layer_index`](Self::layer_index) *is* the store index, this just
    /// validates the selection against the current layer set.
    pub(super) fn selected_source_layer(&self, map: &MapInfo) -> Option<usize> {
        self.selected_layer(map).map(|l| l.source_layer)
    }

    /// Open the rename text field on the layer at store `index`, seeded with its
    /// current name. The commit ([`commit_layer_rename`](Self::commit_layer_rename))
    /// writes it back to the store and records the undo step.
    pub(super) fn begin_layer_rename(&mut self, maps: &MapStore, source: &str, index: usize) {
        let name = maps
            .get(source)
            .and_then(|tm| tm.layer_name(index))
            .unwrap_or("")
            .to_string();
        self.stop_editing();
        self.editing = Some(TextEdit {
            field: EditField::LayerName,
            buffer: TextField::new(name),
            target: index,
        });
    }

    /// Open a numeric text field on the selected tile layer's offset / palette
    /// rotation, seeded from the store and targeting `index` (the captured layer
    /// for store-backed text edits, held in [`TextEdit::target`]).
    pub(super) fn begin_layer_field(
        &mut self,
        maps: &MapStore,
        source: &str,
        index: usize,
        field: EditField,
    ) {
        let tm = maps.get(source);
        let value = match field {
            EditField::LayerOffX => tm
                .and_then(|t| t.layer_offset(index))
                .map(|(x, _)| x.to_string())
                .unwrap_or_default(),
            EditField::LayerOffY => tm
                .and_then(|t| t.layer_offset(index))
                .map(|(_, y)| y.to_string())
                .unwrap_or_default(),
            EditField::LayerRotate => tm
                .map(|t| t.layer_palette_rotate(index))
                .unwrap_or(0)
                .to_string(),
            _ => String::new(),
        };
        self.stop_editing();
        self.editing = Some(TextEdit {
            field,
            buffer: TextField::new(value),
            target: index,
        });
    }

    /// Cycle the tile layer at store `index` through the BG → Sprite → FG draw
    /// planes, writing its `plane` custom property (no rename), recorded as one
    /// undoable step. Cycling doesn't reorder the store, so the layer keeps its
    /// `source_layer` — the selection stays on it and the panel view doesn't move
    /// (the re-derive next frame just re-tags its row's plane).
    pub(super) fn cycle_layer_plane(&mut self, map: &MapInfo, maps: &mut MapStore, index: usize) {
        let Some(tm) = maps.get_mut(&map.source) else {
            return;
        };
        // Only tile layers carry a plane; an image layer stays on its name
        // convention (its `set_layer_plane` no-ops).
        if !matches!(tm.layers.get(index), Some(TiledMapLayer::TileLayer(_))) {
            return;
        }
        let before = tm.layer_plane(index);
        let after = before.cycle();
        tm.set_layer_plane(index, after);
        self.layer_index = index;
        self.record(EditAction::LayerPlane {
            source: map.source.clone(),
            index,
            before,
            after,
        });
        self.pending_reload = true;
    }

    /// The layer the paint tool writes into (the selected layer, by store index).
    pub(super) fn active_layer<'a>(&self, map: &'a MapInfo) -> Option<&'a LayerInfo> {
        self.selected_layer(map)
    }

    /// Step the map's `music` property through `[none] + tracks`, by name — the
    /// same string-indexed model as a warp's `to_map`. `tracks` are the music
    /// directory's file stems (from [`ConsoleApi::music_tracks`]). Stored on the
    /// map (saved + resolved at load); not on the undo stack, like the panel's
    /// other map settings, and it takes effect on the next map load.
    pub(super) fn cycle_music(&mut self, map: &MapInfo, maps: &mut MapStore, tracks: &[String]) {
        let Some(tm) = maps.get_mut(&map.source) else {
            return;
        };
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
    pub(super) fn cycle_music_speed(&mut self, map: &MapInfo, maps: &mut MapStore) {
        let Some(tm) = maps.get_mut(&map.source) else {
            return;
        };
        let speed = tm.music_speed();
        // Snap to the nearest preset, then advance one (wrapping).
        let current = MUSIC_SPEEDS
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (*a - speed).abs().total_cmp(&(*b - speed).abs()))
            .map_or(0, |(i, _)| i);
        let next = (current + 1) % MUSIC_SPEEDS.len();
        tm.set_music_speed(MUSIC_SPEEDS[next]);
        self.status.edited();
    }

    /// Apply a finished layer rename to the store and record it for undo. Empty
    /// names are ignored (a layer must stay identifiable). A rename can move the
    /// layer between the bg/fg draw lists, so it flags a re-derive.
    pub(super) fn commit_layer_rename(
        &mut self,
        map: &mut MapInfo,
        maps: &mut MapStore,
        name: &str,
    ) {
        let Some(index) = self.editing.as_ref().map(|e| e.target) else {
            return;
        };
        if name.is_empty() {
            return;
        }
        let Some(before) = maps
            .get(&map.source)
            .and_then(|tm| tm.layer_name(index))
            .map(str::to_string)
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
    pub(super) fn commit_layer_prop(
        &mut self,
        map: &MapInfo,
        maps: &mut MapStore,
        prop: LayerProp,
        value: f64,
    ) {
        let Some(index) = self.editing.as_ref().map(|e| e.target) else {
            return;
        };
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

    /// Persist the map and start the save-confirmation toast. A map only writes
    /// back when it's in the store as a modern map; anything else (e.g. the
    /// empty default map, source `""`) has no `.tmj` to save to and just logs.
    pub(super) fn save(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &MapInfo,
        maps: &mut MapStore,
    ) {
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
    pub(super) fn step_maps_dialog(
        &mut self,
        system: &mut impl ConsoleApi,
        input: &EggInput,
        maps: &mut MapStore,
    ) {
        let action = match &mut self.maps_dialog {
            MapsDialog::None => DialogAction::Keep,
            MapsDialog::New { name, w, h, focus } => {
                if input.keyp(ScanCode::Escape) {
                    DialogAction::Close
                } else if input.keyp(ScanCode::Return) {
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
                    for c in input.key_chars() {
                        let allowed = !c.is_control() && (!digits_only || c.is_ascii_digit());
                        if allowed {
                            field.edit(TextOp::Push(*c));
                        }
                    }
                    field.edit_keys(input);
                    DialogAction::Keep
                }
            }
            MapsDialog::Rename { from, name } => match name.step(input) {
                TextEvent::Commit => {
                    DialogAction::Rename(from.clone(), name.text().trim().to_string())
                }
                TextEvent::Cancel => DialogAction::Close,
                TextEvent::Active => DialogAction::Keep,
            },
            MapsDialog::ConfirmDelete(name) => {
                if input.keyp(ScanCode::Return) {
                    DialogAction::Delete(name.clone())
                } else if input.keyp(ScanCode::Escape) {
                    DialogAction::Close
                } else {
                    DialogAction::Keep
                }
            }
            MapsDialog::Resize {
                source,
                w,
                h,
                focus,
            } => {
                if input.keyp(ScanCode::Escape) {
                    DialogAction::Close
                } else if input.keyp(ScanCode::Return) {
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
                    for c in input.key_chars() {
                        if c.is_ascii_digit() {
                            field.edit(TextOp::Push(*c));
                        }
                    }
                    field.edit_keys(input);
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
    pub(super) fn create_map(
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
    pub(super) fn duplicate_map(
        &mut self,
        system: &mut impl ConsoleApi,
        maps: &mut MapStore,
        src: &str,
    ) {
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
    pub(super) fn rename_map(
        &mut self,
        system: &mut impl ConsoleApi,
        maps: &mut MapStore,
        from: &str,
        to: &str,
    ) {
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
    pub(super) fn delete_map(
        &mut self,
        system: &mut impl ConsoleApi,
        maps: &mut MapStore,
        name: &str,
    ) {
        maps.remove(name);
        self.manifest_mutate(system, maps, |m| m.maps.retain(|n| n != name));
        if self.maps_selected.as_deref() == Some(name) {
            self.maps_selected = None;
        }
    }

    /// Read-modify-write the asset manifest. Falls back to the store's current
    /// names if no manifest file is present, so a fresh manifest is still correct.
    pub(super) fn manifest_mutate(
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use egg_platform::null_console::NullConsole;

    /// Create → duplicate → rename → delete a map, checking the store and the
    /// written manifest stay consistent at each step (native file path).
    #[test]
    fn map_crud_round_trip() {
        use egg_platform::test_console::TestConsole;

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
        assert_eq!(
            maps.get("newmap").map(|m| (m.width, m.height)),
            Some((20, 15))
        );
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

    /// Adding a layer is undoable (and redoable), and deleting a layer restores
    /// its tile content on undo.
    #[test]
    fn layer_ops_are_undoable() {
        use egg_world::data::tiled::TiledMap;
        let mut maps = MapStore::default();
        maps.insert("m", TiledMap::blank_modern(4, 4));
        let n0 = maps.get("m").unwrap().layers.len(); // collision + Layer 1 + objects
        let mut map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo {
                    source_layer: 0,
                    ..LayerInfo::DEFAULT_LAYER
                },
                LayerInfo {
                    source_layer: 1,
                    ..LayerInfo::DEFAULT_LAYER
                },
            ],
            ..MapInfo::default()
        };
        let mut v = MapViewer::default();

        // Add (mirror the handler): record the insert.
        let index = maps.get_mut("m").unwrap().add_tile_layer("Layer 2");
        let layer = Box::new(maps.get_mut("m").unwrap().layers[index].clone());
        v.record(EditAction::LayerInsert {
            source: "m".to_string(),
            index,
            layer,
        });
        assert_eq!(maps.get("m").unwrap().layers.len(), n0 + 1);

        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layers.len(), n0);
        assert!(v.pending_reload, "layer undo re-derives");
        v.redo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layers.len(), n0 + 1);

        // Delete a tile layer holding content; undo brings it (and the tile) back.
        maps.get_mut("m").unwrap().set(1, 0, 0, 7);
        let removed = maps.get_mut("m").unwrap().remove_layer_at(1).unwrap();
        v.record(EditAction::LayerRemove {
            source: "m".to_string(),
            index: 1,
            layer: Box::new(removed),
        });
        assert_eq!(maps.get("m").unwrap().layers.len(), n0); // back down one
        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(
            maps.get("m").unwrap().get(1, 0, 0),
            Some(7),
            "content restored"
        );

        // The collision layer (index 0) is protected — remove returns None.
        assert!(maps.get_mut("m").unwrap().remove_layer_at(0).is_none());
    }

    /// Renaming a layer commits to the store and is undoable; the plane cycle
    /// writes the `plane` property (BG → Sprite → FG) without renaming, itself
    /// undoable.
    #[test]
    fn layer_rename_and_plane_cycle() {
        use egg_world::data::tiled::TiledMap;
        let mut maps = MapStore::default();
        maps.insert("m", TiledMap::blank_modern(4, 4));
        let mut map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo {
                    source_layer: 0,
                    ..LayerInfo::DEFAULT_LAYER
                },
                LayerInfo {
                    source_layer: 1,
                    ..LayerInfo::DEFAULT_LAYER
                },
            ],
            ..MapInfo::default()
        };
        let mut v = MapViewer {
            layer_index: 1,
            ..Default::default()
        };

        // Rename "Layer 1" -> "water" via the begin/commit flow.
        v.begin_layer_rename(&maps, "m", 1);
        assert_eq!(v.editing_field(), Some(EditField::LayerName));
        v.editing.as_mut().unwrap().buffer = TextField::new("water");
        v.commit_edit(&mut map, &mut maps);
        v.stop_editing();
        assert_eq!(maps.get("m").unwrap().layer_name(1), Some("water"));
        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layer_name(1), Some("Layer 1"));
        v.redo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layer_name(1), Some("water"));

        // Plane cycle writes the `plane` property (no rename): BG -> Sprite ->
        // FG, each an undoable step; the layer name is untouched throughout.
        assert_eq!(maps.get("m").unwrap().layer_plane(1), Plane::Bg);
        v.cycle_layer_plane(&map, &mut maps, 1);
        assert_eq!(maps.get("m").unwrap().layer_plane(1), Plane::Sprite);
        // Cycling doesn't reorder the store, so the selection stays on the layer
        // (its store index is unchanged) — the panel view no longer jumps planes.
        assert_eq!(v.layer_index, 1, "selection stays on the cycled layer");
        v.cycle_layer_plane(&map, &mut maps, 1);
        assert_eq!(maps.get("m").unwrap().layer_plane(1), Plane::Fg);
        assert_eq!(maps.get("m").unwrap().layer_name(1), Some("water"), "no rename");
        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layer_plane(1), Plane::Sprite);
        v.redo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layer_plane(1), Plane::Fg);

        // An empty rename is ignored (a layer stays identifiable).
        v.editing = Some(TextEdit {
            field: EditField::LayerName,
            buffer: TextField::new("   "),
            target: 1,
        });
        v.commit_edit(&mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layer_name(1), Some("water"));
    }

    /// Drag-reordering a layer: rows are keyed by store index, so the move goes
    /// straight to the store's layer indices, records one undoable `LayerMove`,
    /// follows the dropped layer, protects the collision layer, and round-trips
    /// through undo / redo.
    #[test]
    fn layer_drag_reorder_translates_and_undoes() {
        use egg_world::data::tiled::TiledMap;
        let mut maps = MapStore::default();
        let mut tm = TiledMap::blank_modern(4, 4);
        tm.add_tile_layer("a");
        tm.add_tile_layer("b");
        maps.insert("m", tm);
        // Store (map) order: 0=collision, 1="Layer 1", 2="a", 3="b".
        let mut map = MapInfo {
            source: "m".to_string(),
            layers: (0..4)
                .map(|src| LayerInfo {
                    source_layer: src,
                    ..LayerInfo::DEFAULT_LAYER
                })
                .collect(),
            ..MapInfo::default()
        };
        let mut v = MapViewer::default();
        let order = |maps: &MapStore| {
            (0..4)
                .map(|i| maps.get("m").unwrap().layer_name(i).unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(order(&maps), ["collision", "Layer 1", "a", "b"]);

        // Drag store layer 3 ("b") up to index 1: the layers between slide down.
        v.reorder_layer_to(&mut map, &mut maps, 3, 1);
        assert_eq!(order(&maps), ["collision", "b", "Layer 1", "a"]);
        assert_eq!(v.layer_index, 1, "selection follows the dropped layer");
        assert!(v.pending_reload, "a reorder re-derives the display list");
        assert!(v.history.can_undo(), "the drag is one undo step");

        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(order(&maps), ["collision", "Layer 1", "a", "b"], "undone");
        v.redo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(order(&maps), ["collision", "b", "Layer 1", "a"], "redone");

        // Dragging the protected collision layer (store 0) is refused — no change.
        v.reorder_layer_to(&mut map, &mut maps, 0, 2);
        assert_eq!(order(&maps), ["collision", "b", "Layer 1", "a"], "collision stays put");
        // ...and it records nothing: the next undo reverts the earlier reorder
        // rather than a no-op the refused drag would have stacked on top.
        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(
            order(&maps),
            ["collision", "Layer 1", "a", "b"],
            "undo skips the refused move"
        );
    }

    /// Regression for the "only one sprite layer" bug: with the map-ordered,
    /// store-indexed model, several [`Plane::Sprite`] layers are each individually
    /// selectable and editable. The old single-plane view bucketed layers by plane
    /// and indexed the filtered bucket, so a second sprite layer was unreachable
    /// (the "+L" add landed in the hidden bg bucket).
    #[test]
    fn multiple_sprite_layers_select_and_edit() {
        use egg_world::data::tiled::TiledMap;
        let mut maps = MapStore::default();
        let mut tm = TiledMap::blank_modern(4, 4);
        let s1 = tm.add_tile_layer("s1");
        let s2 = tm.add_tile_layer("s2");
        tm.set_layer_plane(s1, Plane::Sprite);
        tm.set_layer_plane(s2, Plane::Sprite);
        maps.insert("m", tm);
        // Two sprite layers derived into `sprite_layers` (as `modern_map_info`
        // would), the collision layer alone in `layers`.
        let map = MapInfo {
            source: "m".to_string(),
            layers: vec![LayerInfo {
                source_layer: 0,
                ..LayerInfo::DEFAULT_LAYER
            }],
            sprite_layers: vec![
                LayerInfo {
                    source_layer: s1,
                    ..LayerInfo::DEFAULT_LAYER
                },
                LayerInfo {
                    source_layer: s2,
                    ..LayerInfo::DEFAULT_LAYER
                },
            ],
            ..MapInfo::default()
        };
        let mut v = MapViewer::default();

        // Every layer lists in map order, each tagged with its plane.
        assert_eq!(
            v.layers_in_order(&map)
                .iter()
                .map(|(p, l)| (*p, l.source_layer))
                .collect::<Vec<_>>(),
            [(Plane::Bg, 0), (Plane::Sprite, s1), (Plane::Sprite, s2)],
        );

        // Selecting either sprite layer resolves to its own store layer — the
        // paint target, sprite-reload flag and plane readout all follow it.
        v.layer_index = s1;
        assert_eq!(v.selected_source_layer(&map), Some(s1));
        assert_eq!(v.active_layer(&map).map(|l| l.source_layer), Some(s1));
        assert_eq!(v.selected_plane(&map), Plane::Sprite);
        assert!(MapViewer::is_sprite_layer(&map, s1));

        v.layer_index = s2;
        assert_eq!(v.active_layer(&map).map(|l| l.source_layer), Some(s2));
        assert!(MapViewer::is_sprite_layer(&map, s2));

        // An edit routed through the selection lands on `s2`, leaving `s1` empty:
        // the two sprite layers are distinct paint targets.
        let target = v.active_layer(&map).unwrap().source_layer;
        maps.get_mut("m").unwrap().set(target, 0, 0, 7);
        assert_eq!(maps.get("m").unwrap().get(s2, 0, 0), Some(7));
        assert_eq!(maps.get("m").unwrap().get(s1, 0, 0), Some(0), "s1 untouched");
    }

    /// A plane filter hides its rows without disturbing the selection (a hidden
    /// layer still paints), and collision protection keys off the store index —
    /// so hiding rows can never mis-target it.
    #[test]
    fn layer_filters_hide_rows_but_keep_selection_and_protection() {
        use egg_world::data::tiled::TiledMap;
        let mut maps = MapStore::default();
        let mut tm = TiledMap::blank_modern(4, 4);
        tm.add_tile_layer("a");
        tm.add_tile_layer("b");
        maps.insert("m", tm);
        // Store: collision(0), "Layer 1"(1), a(2), b(3). Route them across planes
        // in the MapInfo so each filter has a row to hide.
        let map_of = || MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo {
                    source_layer: 0,
                    ..LayerInfo::DEFAULT_LAYER
                },
                LayerInfo {
                    source_layer: 1,
                    ..LayerInfo::DEFAULT_LAYER
                },
            ],
            sprite_layers: vec![LayerInfo {
                source_layer: 2,
                ..LayerInfo::DEFAULT_LAYER
            }],
            fg_layers: vec![LayerInfo {
                source_layer: 3,
                ..LayerInfo::DEFAULT_LAYER
            }],
            ..MapInfo::default()
        };
        let mut map = map_of();
        let mut v = MapViewer::default();
        assert!(
            v.filter.bg && v.filter.sprite && v.filter.fg,
            "all planes show by default"
        );

        // Select the sprite layer, then hide the sprite plane: the selection and
        // paint target hold even though the row is gone.
        v.layer_index = 2;
        v.filter.toggle(Plane::Sprite);
        assert!(!v.filter.shows(Plane::Sprite));
        assert_eq!(
            v.selected_source_layer(&map),
            Some(2),
            "hidden layer stays selected"
        );
        assert_eq!(
            v.active_layer(&map).map(|l| l.source_layer),
            Some(2),
            "and still paints"
        );

        // Hiding the bg plane can't unprotect the collision layer (store index 0).
        v.filter.toggle(Plane::Bg);
        assert_eq!(v.collision_src(&maps, "m"), Some(0));
        v.layer_index = 0;
        v.reorder_layer_to(&mut map, &mut maps, 0, 2);
        assert_eq!(
            maps.get("m").unwrap().layer_name(0),
            Some("collision"),
            "collision stays first"
        );
        assert!(!v.history.can_undo(), "the refused move recorded nothing");
    }

    /// A chip click means "show me this plane": from all-on it solos, later
    /// clicks build the set back up, and emptying the set snaps to all-on — the
    /// panel can never show a blank list ("my layers got erased").
    #[test]
    fn filter_chip_click_solos_then_builds_up() {
        let mut f = LayerFilter::default();

        // All on → clicking spr solos the sprite plane.
        f.click(Plane::Sprite);
        assert!(f.sprite && !f.bg && !f.fg, "first click solos");

        // A second plane clicks in alongside it.
        f.click(Plane::Fg);
        assert!(f.sprite && f.fg && !f.bg, "later clicks add planes");

        // Clicking shown planes off, down to none, restores all-on.
        f.click(Plane::Fg);
        assert!(f.sprite && !f.fg && !f.bg);
        f.click(Plane::Sprite);
        assert!(
            f.bg && f.sprite && f.fg,
            "emptying the set snaps back to all-on"
        );
    }

    /// Controller up/down walk only the **visible** rows in map order: a hidden
    /// plane is skipped, and a selection on a now-hidden row snaps to a visible one.
    #[test]
    fn move_selection_walks_visible_rows() {
        let map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo {
                    source_layer: 0,
                    ..LayerInfo::DEFAULT_LAYER
                },
                LayerInfo {
                    source_layer: 1,
                    ..LayerInfo::DEFAULT_LAYER
                },
            ],
            sprite_layers: vec![LayerInfo {
                source_layer: 2,
                ..LayerInfo::DEFAULT_LAYER
            }],
            fg_layers: vec![LayerInfo {
                source_layer: 3,
                ..LayerInfo::DEFAULT_LAYER
            }],
            ..MapInfo::default()
        };
        let mut v = MapViewer::default();

        // All planes on: walk 0 → 1 → 2 → 3, clamping at the ends.
        v.layer_index = 0;
        v.move_selection(&map, true);
        assert_eq!(v.layer_index, 1);
        v.move_selection(&map, true);
        assert_eq!(v.layer_index, 2);
        v.move_selection(&map, true);
        assert_eq!(v.layer_index, 3);
        v.move_selection(&map, true);
        assert_eq!(v.layer_index, 3, "clamps at the last visible row");

        // Hide the sprite plane: stepping down from row 1 skips store index 2.
        v.filter.toggle(Plane::Sprite);
        v.layer_index = 1;
        v.move_selection(&map, true);
        assert_eq!(v.layer_index, 3, "sprite row skipped");
        v.move_selection(&map, false);
        assert_eq!(v.layer_index, 1, "and back");

        // A selection stranded on the hidden row snaps to the first visible one.
        v.layer_index = 2;
        v.move_selection(&map, true);
        assert_eq!(v.layer_index, 0, "hidden selection snaps to first visible");
    }

    /// The Layers panel builds one row per layer in map order (with per-row plane
    /// tags / collision styling — no panic), and a filter drops exactly that
    /// plane's rows with nothing else shifting.
    #[test]
    fn layers_panel_lists_all_planes_and_filters_rows() {
        use egg_world::data::tiled::TiledMap;
        let mut maps = MapStore::default();
        let mut tm = TiledMap::blank_modern(4, 4);
        tm.add_tile_layer("spr1");
        tm.add_tile_layer("fg1");
        tm.set_layer_plane(2, Plane::Sprite);
        tm.set_layer_plane(3, Plane::Fg);
        maps.insert("m", tm);
        let map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo {
                    source_layer: 0,
                    ..LayerInfo::DEFAULT_LAYER
                },
                LayerInfo {
                    source_layer: 1,
                    ..LayerInfo::DEFAULT_LAYER
                },
            ],
            sprite_layers: vec![LayerInfo {
                source_layer: 2,
                ..LayerInfo::DEFAULT_LAYER
            }],
            fg_layers: vec![LayerInfo {
                source_layer: 3,
                ..LayerInfo::DEFAULT_LAYER
            }],
            ..MapInfo::default()
        };
        let mut v = MapViewer::default();
        v.layer_index = 0; // hold the selection constant across both builds

        let count = |v: &MapViewer| {
            let mut b = UiBuilder::new();
            let mut rows: Vec<NodeId> = Vec::new();
            v.build_layers(&mut b, &mut rows, &map, &maps);
            rows.len()
        };
        let all = count(&v);
        v.filter.toggle(Plane::Sprite);
        let hidden = count(&v);
        assert_eq!(hidden, all - 1, "hiding the sprite plane drops its one row");
    }

    /// A tile layer's offset / palette-rotation fields edit the store and are
    /// undoable, one step each.
    #[test]
    fn layer_offset_and_rotate_edit_and_undo() {
        use egg_world::data::tiled::TiledMap;
        let mut maps = MapStore::default();
        maps.insert("m", TiledMap::blank_modern(4, 4));
        let mut map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo {
                    source_layer: 0,
                    ..LayerInfo::DEFAULT_LAYER
                },
                LayerInfo {
                    source_layer: 1,
                    ..LayerInfo::DEFAULT_LAYER
                },
            ],
            ..MapInfo::default()
        };
        let mut v = MapViewer {
            layer_index: 1,
            ..Default::default()
        };
        let edit = |v: &mut MapViewer,
                    map: &mut MapInfo,
                    maps: &mut MapStore,
                    field: EditField,
                    text: &str| {
            v.editing = Some(TextEdit {
                field,
                buffer: TextField::new(text),
                target: 1,
            });
            v.commit_edit(map, maps);
            v.stop_editing();
        };
        edit(&mut v, &mut map, &mut maps, EditField::LayerOffX, "3");
        edit(&mut v, &mut map, &mut maps, EditField::LayerOffY, "-2");
        edit(&mut v, &mut map, &mut maps, EditField::LayerRotate, "5");
        assert_eq!(maps.get("m").unwrap().layer_offset(1), Some((3.0, -2.0)));
        assert_eq!(maps.get("m").unwrap().layer_palette_rotate(1), 5);

        v.undo(&mut NullConsole::new(), &mut map, &mut maps); // rotation
        assert_eq!(maps.get("m").unwrap().layer_palette_rotate(1), 0);
        v.undo(&mut NullConsole::new(), &mut map, &mut maps); // y offset
        assert_eq!(maps.get("m").unwrap().layer_offset(1), Some((3.0, 0.0)));
        v.redo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(maps.get("m").unwrap().layer_offset(1), Some((3.0, -2.0)));
    }

    /// Rotation edits normalise mod-16 (so revert is exact even for a
    /// hand-authored value > 15), and a non-finite offset is rejected.
    #[test]
    fn layer_prop_edits_normalise_and_reject_bad_input() {
        use egg_world::data::tiled::TiledMap;
        let mut maps = MapStore::default();
        maps.insert("m", TiledMap::blank_modern(4, 4));
        maps.get_mut("m").unwrap().set_layer_palette_rotate(1, 20); // out of range, as if hand-authored
        let mut map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo {
                    source_layer: 0,
                    ..LayerInfo::DEFAULT_LAYER
                },
                LayerInfo {
                    source_layer: 1,
                    ..LayerInfo::DEFAULT_LAYER
                },
            ],
            ..MapInfo::default()
        };
        let mut v = MapViewer {
            layer_index: 1,
            ..Default::default()
        };
        let commit = |v: &mut MapViewer,
                      map: &mut MapInfo,
                      maps: &mut MapStore,
                      field: EditField,
                      text: &str| {
            v.editing = Some(TextEdit {
                field,
                buffer: TextField::new(text),
                target: 1,
            });
            v.commit_edit(map, maps);
            v.stop_editing();
        };

        commit(&mut v, &mut map, &mut maps, EditField::LayerRotate, "5");
        assert_eq!(maps.get("m").unwrap().layer_palette_rotate(1), 5);
        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
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
        use egg_world::data::tiled::TiledMap;
        let mut maps = MapStore::default();
        maps.insert("m", TiledMap::blank_modern(2, 2));
        let map = MapInfo {
            source: "m".to_string(),
            ..MapInfo::default()
        };
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

    /// A layer's `plane` property overrides the `fg` name fallback and cycles
    /// through all three planes; choosing the plane the name already implies
    /// leaves the layer clean (no property), so the fallback still applies.
    #[test]
    fn layer_plane_property_and_name_fallback() {
        use egg_world::data::tiled::TiledMap;
        let mut tm = TiledMap::blank_modern(2, 2);
        // "Layer 1" — no `fg` prefix, so the name fallback is Bg.
        assert_eq!(tm.layer_plane(1), Plane::Bg);
        tm.set_layer_plane(1, Plane::Sprite);
        assert_eq!(tm.layer_plane(1), Plane::Sprite);
        tm.set_layer_plane(1, Plane::Fg);
        assert_eq!(tm.layer_plane(1), Plane::Fg);
        // Setting the plane the name already implies (Bg) drops the property.
        tm.set_layer_plane(1, Plane::Bg);
        assert_eq!(tm.layer_plane(1), Plane::Bg);
        // An `fg`-prefixed name falls back to Fg; an explicit `plane` still wins.
        tm.set_layer_name(1, "fg roof");
        assert_eq!(tm.layer_plane(1), Plane::Fg);
        tm.set_layer_plane(1, Plane::Bg);
        assert_eq!(tm.layer_plane(1), Plane::Bg);
    }

    /// A name collision, a path-separator name and an empty name are all rejected.
    #[test]
    fn new_map_name_validation() {
        let mut maps = MapStore::default();
        maps.insert("town", egg_world::data::tiled::TiledMap::blank_modern(4, 4));
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

    /// Saving writes the `.tmj` *and* re-syncs the store, so leaving and
    /// re-entering the map sees the edited objects — without the sync, the disk
    /// file was right but `map_by_name` rebuilt from the stale pre-edit object
    /// layer until a restart. Attached image pixels survive the swap (they
    /// aren't serialised, so the sync carries them over by path).
    #[test]
    fn save_syncs_the_store() {
        use egg_world::data::tiled::{
            ImageLayer, ObjectLayer, TileLayer, TiledMap, TiledMapLayer, Tileset,
        };
        use egg_render::image::RgbaImage;
        use egg_platform::test_console::TestConsole;

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
            synced
                .layers
                .iter()
                .any(|l| matches!(l, TiledMapLayer::ImageLayer(i) if i.pixels.is_some())),
            "attached pixels survive the sync"
        );
    }
}

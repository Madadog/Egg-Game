//! Bounded undo/redo for the map editor: object/tile snapshots and the
//! `revert` / `reapply` engine that plays an [`EditAction`] backwards or forwards.

use super::*;

impl MapViewer {
    /// Clone object `i` into a snapshot, if it exists.
    pub(super) fn snapshot(map: &MapInfo, i: usize) -> Option<ObjSnapshot> {
        map.objects.get(i).cloned()
    }

    // --- History --------------------------------------------------------------

    /// Record an action onto the undo stack and flag the map as unsaved. Every
    /// mutating editor operation funnels through here so dirty-tracking and
    /// history stay in lock-step.
    pub(super) fn record(&mut self, action: EditAction) {
        self.history.push(action);
        self.status.edited();
    }

    /// Undo the most recent edit (Ctrl+Z). Object indices may shift on
    /// add/remove, so undo restores list shape as well as contents. The action is
    /// cloned out of the history before reverting because `revert` needs `&mut
    /// self`, which can't coexist with a borrow into `self.history`.
    pub(super) fn undo(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        maps: &mut MapStore,
    ) {
        if let Some(action) = self.history.undo().cloned() {
            self.revert(system, map, maps, &action);
            self.status.edited();
        }
    }

    /// Redo the most recently undone edit (Ctrl+Y / Ctrl+Shift+Z).
    pub(super) fn redo(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        maps: &mut MapStore,
    ) {
        if let Some(action) = self.history.redo().cloned() {
            self.reapply(system, map, maps, &action);
            self.status.edited();
        }
    }

    /// Flag a re-derive if tile edits on `layer` feed derived runtime state:
    /// the collision (first tile) layer's art is what `Collider::from_sprite`
    /// derives from, and a sprite-plane layer's flood-fill components (shape +
    /// baselines) are cached at load, not read live like bg/fg tiles. Forward
    /// tile edits flag it inline (see `handle_paint` / `selection_layer`); this
    /// keeps undo/redo in step — without it an undone sprite-plane stroke
    /// restores the tile data but the stale components keep drawing until the
    /// next forward edit on that layer rebuilds them.
    pub(super) fn flag_derived_reload(&mut self, map: &MapInfo, layer: usize) {
        let is_collision = map.layers.first().map(|l| l.source_layer) == Some(layer);
        if is_collision || Self::is_sprite_layer(map, layer) {
            self.pending_reload = true;
        }
    }

    /// Reverse an action's effect (the undo direction). `system` is only used by
    /// the scene edit (which re-installs a file); the map/object/layer actions
    /// ignore it.
    pub(super) fn revert(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        maps: &mut MapStore,
        action: &EditAction,
    ) {
        match action {
            EditAction::Tiles {
                source,
                layer,
                cells,
            } => {
                if let Some(tiles) = maps.get_mut(source) {
                    for &(x, y, old, _new) in cells {
                        tiles.set(*layer, x as usize, y as usize, old);
                    }
                }
                self.flag_derived_reload(map, *layer);
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
            EditAction::LayerRemove {
                source,
                index,
                layer,
            } => {
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
            // Undo a move by sliding the layer back from `to` to `from`. The
            // stored indices are already collision-clamped, so this re-applies
            // exactly with no further clamping.
            EditAction::LayerMove { source, from, to } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.reorder_layer(*to, *from);
                }
                self.pending_reload = true;
            }
            EditAction::LayerRename {
                source,
                index,
                before,
                ..
            } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.set_layer_name(*index, before);
                }
                self.pending_reload = true;
            }
            EditAction::LayerPlane {
                source,
                index,
                before,
                ..
            } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.set_layer_plane(*index, *before);
                }
                self.pending_reload = true;
            }
            EditAction::LayerSetProp {
                source,
                index,
                prop,
                before,
                ..
            } => {
                apply_layer_prop(maps, source, *index, *prop, *before);
                self.pending_reload = true;
            }
            // Undo a scene edit by re-installing the file as it was before it.
            EditAction::SceneEdit { before, .. } => {
                self.install_scene_source(system, before.clone());
            }
        }
    }

    /// Re-perform an action's effect (the redo direction). `system` is only used by
    /// the scene edit; the other actions ignore it.
    pub(super) fn reapply(
        &mut self,
        system: &mut impl ConsoleApi,
        map: &mut MapInfo,
        maps: &mut MapStore,
        action: &EditAction,
    ) {
        match action {
            EditAction::Tiles {
                source,
                layer,
                cells,
            } => {
                if let Some(tiles) = maps.get_mut(source) {
                    for &(x, y, _old, new) in cells {
                        tiles.set(*layer, x as usize, y as usize, new);
                    }
                }
                self.flag_derived_reload(map, *layer);
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
            EditAction::LayerInsert {
                source,
                index,
                layer,
            } => {
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
            EditAction::LayerMove { source, from, to } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.reorder_layer(*from, *to);
                }
                self.pending_reload = true;
            }
            EditAction::LayerRename {
                source,
                index,
                after,
                ..
            } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.set_layer_name(*index, after);
                }
                self.pending_reload = true;
            }
            EditAction::LayerPlane {
                source,
                index,
                after,
                ..
            } => {
                if let Some(tm) = maps.get_mut(source) {
                    tm.set_layer_plane(*index, *after);
                }
                self.pending_reload = true;
            }
            EditAction::LayerSetProp {
                source,
                index,
                prop,
                after,
                ..
            } => {
                apply_layer_prop(maps, source, *index, *prop, *after);
                self.pending_reload = true;
            }
            // Redo a scene edit by re-installing the post-edit file.
            EditAction::SceneEdit { after, .. } => {
                self.install_scene_source(system, after.clone());
            }
        }
    }

    // --- Step (input) ---------------------------------------------------------
}

#[cfg(test)]
mod tests {
    use super::*;
    use egg_platform::NullConsole;

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
        let narrated = MapObject::warp(
            Hitbox::new(0, 0, 8, 8),
            Warp::new(None, Vec2::new(0, 0)).with_narration("creak"),
        );
        assert!(!snapshot_eq(&warp, &narrated), "narration edit detected");
        // Same narration compares equal (no spurious undo entry).
        let narrated2 = MapObject::warp(
            Hitbox::new(0, 0, 8, 8),
            Warp::new(None, Vec2::new(0, 0)).with_narration("creak"),
        );
        assert!(snapshot_eq(&narrated, &narrated2));
    }

    /// A Note pitch / AddCreatures count edit changes only the `InteractFn`
    /// payload (same kind, same box); `snapshot_eq` must still detect it, or the
    /// editor never records the edit as an undo step (it would be unmancellable).
    #[test]
    fn snapshot_eq_detects_func_payload_edits() {
        let note0 = MapObject::func(Hitbox::new(0, 0, 8, 8), InteractFn::Note(0));
        let note7 = MapObject::func(Hitbox::new(0, 0, 8, 8), InteractFn::Note(7));
        assert!(!snapshot_eq(&note0, &note7), "pitch edit detected");
        // Identical payload compares equal (no spurious undo entry).
        let note0_again = MapObject::func(Hitbox::new(0, 0, 8, 8), InteractFn::Note(0));
        assert!(snapshot_eq(&note0, &note0_again));
        // The same holds for the AddCreatures count.
        let few = MapObject::func(Hitbox::new(0, 0, 8, 8), InteractFn::AddCreatures(0));
        let many = MapObject::func(Hitbox::new(0, 0, 8, 8), InteractFn::AddCreatures(3));
        assert!(!snapshot_eq(&few, &many), "count edit detected");
    }

    /// `snapshot_eq` compares the animated sprite, so adding a frame or changing a
    /// frame's tile is recorded as an undo step (identical sprites stay equal).
    #[test]
    fn snapshot_eq_detects_sprite_edits() {
        let base = MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "k");
        let with = base.clone().with_sprite(vec![AnimFrame {
            spr_id: 5,
            ..AnimFrame::default()
        }]);
        assert!(!snapshot_eq(&base, &with), "adding a sprite detected");
        let with_b = base.clone().with_sprite(vec![AnimFrame {
            spr_id: 9,
            ..AnimFrame::default()
        }]);
        assert!(!snapshot_eq(&with, &with_b), "frame tile edit detected");
        // Identical sprites compare equal (no spurious undo entry).
        let with_again = base.with_sprite(vec![AnimFrame {
            spr_id: 5,
            ..AnimFrame::default()
        }]);
        assert!(snapshot_eq(&with, &with_again));
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
        assert_eq!(
            dialogue_key(&map.objects[0]),
            "a",
            "re-inserted at original index"
        );

        // A past-the-end insert clamps to a push rather than panicking.
        let extra = MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "c");
        insert_object(&mut map, 99, extra);
        assert_eq!(map.objects.len(), 3);
    }

    /// Undoing (or redoing) a tile stroke on a sprite-plane layer must flag a
    /// re-derive: its flood-fill components are cached at load, so without the
    /// flag the restored tiles keep drawing the stale shapes until the next
    /// forward edit on that layer rebuilds them. Live-read layers stay unflagged.
    #[test]
    fn sprite_plane_tile_undo_redo_rederives() {
        let mut maps = MapStore::default();
        maps.insert("m", egg_world::data::tiled::TiledMap::blank_modern(4, 4));
        let mut map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo {
                    source_layer: 0,
                    ..LayerInfo::DEFAULT_LAYER
                }, // collision
                LayerInfo {
                    source_layer: 1,
                    ..LayerInfo::DEFAULT_LAYER
                }, // drawable, sprite plane (below)
            ],
            sprite_layers: vec![LayerInfo {
                source_layer: 1,
                ..LayerInfo::DEFAULT_LAYER
            }],
            ..MapInfo::default()
        };
        let mut viewer = MapViewer::default();

        // A recorded stroke on the sprite layer: cell (0,0) painted 0 → 7.
        maps.get_mut("m").unwrap().set(1, 0, 0, 7);
        viewer.record(EditAction::Tiles {
            source: "m".to_string(),
            layer: 1,
            cells: vec![(0, 0, 0, 7)],
        });

        viewer.pending_reload = false;
        viewer.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(
            maps.get("m").unwrap().get(1, 0, 0),
            Some(0),
            "undo restores the tile"
        );
        assert!(
            viewer.pending_reload,
            "sprite-plane undo re-derives the flood-fill components"
        );

        viewer.pending_reload = false;
        viewer.redo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(
            maps.get("m").unwrap().get(1, 0, 0),
            Some(7),
            "redo reapplies the tile"
        );
        assert!(viewer.pending_reload, "sprite-plane redo re-derives too");

        // A plain bg/fg layer is read live each frame — editing it derives
        // nothing, so undo/redo there must not schedule a reload.
        viewer.pending_reload = false;
        viewer.flag_derived_reload(&map, 2);
        assert!(!viewer.pending_reload, "live-read layers don't flag");
    }
}

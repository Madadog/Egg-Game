//! The canvas editing tools — tile Paint and marquee Select — and the tile
//! palette / brush geometry that feeds them.

use super::*;

impl MapViewer {
    /// Visible palette dimensions `(cols, rows)` in tiles, from the cached
    /// viewport rect.
    pub(super) fn palette_visible(&self) -> (usize, usize) {
        (
            (self.pal_rect.w as usize / 8).max(1),
            (self.pal_rect.h as usize / 8).max(1),
        )
    }

    /// Vertical scroll-thumb metrics in px: `(thumb height, travel)`, where
    /// `travel` is the track length the thumb's top moves over as `pal_row` runs
    /// `0..=max_r`. Shared by [`draw_palette`](Self::draw_palette) and the drag
    /// math so the thumb the user grabs is exactly the thumb they move.
    pub(super) fn palette_thumb_v(&self) -> (i32, i32) {
        let v = self.pal_rect;
        let (_, vr) = self.palette_visible();
        let total_rows = self.sheet_tiles().div_ceil(self.sheet_cols()).max(1);
        let th = ((v.h as usize * vr) / total_rows).max(2) as i32;
        (th, (v.h as i32 - th).max(1))
    }

    /// Horizontal counterpart of [`palette_thumb_v`](Self::palette_thumb_v):
    /// `(thumb width, travel)`.
    pub(super) fn palette_thumb_h(&self) -> (i32, i32) {
        let v = self.pal_rect;
        let (vc, _) = self.palette_visible();
        let cols = self.sheet_cols().max(1);
        let tw = ((v.w as usize * vc) / cols).max(2) as i32;
        (tw, (v.w as i32 - tw).max(1))
    }

    /// The maximum scroll `(col, row)` so the last column/row can reach the edge.
    pub(super) fn palette_scroll_max(&self) -> (usize, usize) {
        let (vc, vr) = self.palette_visible();
        let total_rows = self.sheet_tiles().div_ceil(self.sheet_cols());
        (
            self.sheet_cols().saturating_sub(vc),
            total_rows.saturating_sub(vr),
        )
    }

    /// Advance an in-progress palette drag. A `Pan` that barely moved picks the
    /// tile under the press on release; a larger one pans (content follows the
    /// cursor). A scroll-bar drag maps the cursor to the scroll position. Started
    /// by a `PaletteView` press in `handle_panel`.
    pub(super) fn step_palette_drag(&mut self, mouse: &MouseInput) {
        let CanvasDrag::Palette(drag) = self.canvas_drag else {
            return;
        };
        let p = mouse.pos();
        let up = released(mouse.left);
        match drag {
            // Extend the brush box from the anchor to the tile under the cursor.
            PalDrag::Select {
                anchor_col,
                anchor_row,
            } => {
                let (c, r) = self.palette_tile_at(p);
                self.set_brush_box(anchor_col, anchor_row, c, r);
                if up {
                    self.canvas_drag = CanvasDrag::None;
                }
            }
            PalDrag::ScrollV { grab } => {
                if up {
                    self.canvas_drag = CanvasDrag::None;
                } else {
                    self.scroll_palette_bar(true, p, grab);
                }
            }
            PalDrag::ScrollH { grab } => {
                if up {
                    self.canvas_drag = CanvasDrag::None;
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
    pub(super) fn scroll_palette_bar(&mut self, vertical: bool, p: Vec2, grab: i16) {
        let (max_c, max_r) = self.palette_scroll_max();
        if vertical && max_r > 0 {
            let (_, travel) = self.palette_thumb_v();
            self.pal_row = scroll_from_drag(
                i32::from(p.y),
                i32::from(grab),
                i32::from(self.pal_rect.y),
                travel,
                max_r as i32,
            ) as usize;
        } else if !vertical && max_c > 0 {
            let (_, travel) = self.palette_thumb_h();
            self.pal_col = scroll_from_drag(
                i32::from(p.x),
                i32::from(grab),
                i32::from(self.pal_rect.x),
                travel,
                max_c as i32,
            ) as usize;
        }
    }

    /// Scroll the palette by a mouse-wheel delta (vertical by default; the
    /// horizontal wheel/touchpad axis scrolls columns). Up scrolls toward row 0.
    pub(super) fn palette_wheel(&mut self, sx: i8, sy: i8) {
        let (max_c, max_r) = self.palette_scroll_max();
        self.pal_row = (self.pal_row as i32 - sy as i32).clamp(0, max_r as i32) as usize;
        self.pal_col = (self.pal_col as i32 - sx as i32).clamp(0, max_c as i32) as usize;
    }

    /// The brush size in tiles, treating an unset `0` as `1`.
    pub(super) fn brush_size(&self) -> (usize, usize) {
        (self.brush_w.max(1), self.brush_h.max(1))
    }

    /// Live sprite-sheet width in tiles (the palette's column count), from the
    /// draw-cached size, falling back to the current sheet until the first draw.
    pub(super) fn sheet_cols(&self) -> usize {
        if self.sheet.0 == 0 {
            SHEET_COLS_DEFAULT
        } else {
            self.sheet.0
        }
    }

    /// Live sprite-sheet height in tiles.
    pub(super) fn sheet_rows(&self) -> usize {
        if self.sheet.1 == 0 {
            SHEET_ROWS_DEFAULT
        } else {
            self.sheet.1
        }
    }

    /// Total tiles in the live sheet — every one is selectable in the palette.
    pub(super) fn sheet_tiles(&self) -> usize {
        self.sheet_cols() * self.sheet_rows()
    }

    /// The sheet `(col, row)` under `point`, clamped into the visible viewport and
    /// the sheet bounds — so a drag that runs off the edge sticks to the last
    /// visible tile rather than wrapping.
    pub(super) fn palette_tile_at(&self, point: Vec2) -> (usize, usize) {
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
    pub(super) fn set_brush_box(&mut self, ac: usize, ar: usize, cc: usize, cr: usize) {
        let (c0, c1) = (ac.min(cc), ac.max(cc));
        let (r0, r1) = (ar.min(cr), ar.max(cr));
        self.selected_tile = r0 * self.sheet_cols() + c0;
        self.brush_w = c1 - c0 + 1;
        self.brush_h = r1 - r0 + 1;
    }

    pub(super) fn handle_canvas(
        &mut self,
        input: &EggInput,
        map: &mut MapInfo,
        maps: &mut MapStore,
        camera_pos: Vec2,
        mouse: &MouseInput,
    ) {
        match self.tool {
            EditorTool::Paint => self.handle_paint(input, map, maps, camera_pos, mouse),
            EditorTool::Select => self.handle_select(camera_pos, mouse),
            EditorTool::Interactables | EditorTool::Warps => {
                let world = Vec2::new(mouse.pos().x + camera_pos.x, mouse.pos().y + camera_pos.y);
                if just_pressed(mouse.left) {
                    if let Some(i) = self.object_at(map, world) {
                        // Grab the object under the cursor to drag it around. Note
                        // the start origin so a completed drag records one undo
                        // step (start → drop), not a step per moved frame.
                        self.selected = Some(i);
                        self.sprite_frame = 0;
                        self.stop_editing();
                        self.drag = None;
                        let origin = self.object_origin(map, i);
                        self.moving = Some(ObjectDrag {
                            grab_offset: world - origin,
                            from: origin,
                        });
                    } else {
                        // Empty space: drag out a box for a new object.
                        self.drag = Some(world);
                        self.moving = None;
                    }
                }
                // Drag the grabbed object's hitbox to follow the cursor.
                if pressed(mouse.left)
                    && let (Some(i), Some(drag)) = (self.selected, self.moving)
                {
                    self.set_object_origin(map, i, world - drag.grab_offset);
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
    pub(super) fn handle_paint(
        &mut self,
        input: &EggInput,
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
        // then takes effect immediately, without a map reload. A sprite-plane
        // paint likewise needs a re-derive: its flood-fill components (shape +
        // baselines) are cached at load, not read live like bg/fg tiles.
        let is_collision = map.layers.first().map(|l| l.source_layer) == Some(layer);
        let needs_reload = is_collision || Self::is_sprite_layer(map, layer);
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

        let rect_mode = input.key(ScanCode::Shift);
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
                if needs_reload {
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
                if needs_reload {
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
    pub(super) fn paint_brush(
        &mut self,
        maps: &mut MapStore,
        source: &str,
        layer: usize,
        tx: i32,
        ty: i32,
        erase: bool,
    ) {
        let (bw, bh) = self.brush_size();
        let (bc, br) = (
            self.selected_tile % self.sheet_cols(),
            self.selected_tile / self.sheet_cols(),
        );
        for dy in 0..bh {
            for dx in 0..bw {
                if bc + dx >= self.sheet_cols() {
                    continue; // don't wrap past the sheet's right edge
                }
                let value = if erase {
                    0
                } else {
                    (br + dy) * self.sheet_cols() + (bc + dx)
                };
                self.paint_cell(maps, source, layer, tx + dx as i32, ty + dy as i32, value);
            }
        }
    }

    /// Set one tile, recording its `(old, new)` into the in-progress stroke.
    /// Skips no-op writes so an undo step only holds cells that actually changed.
    pub(super) fn paint_cell(
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
    pub(super) fn flush_stroke(&mut self) {
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
    pub(super) fn fill_rect(
        &mut self,
        maps: &mut MapStore,
        source: &str,
        layer: usize,
        a: Vec2,
        b: Vec2,
    ) {
        let (x0, y0, x1, y1) = tile_bounds(a, b);
        let (bw, bh) = self.brush_size();
        let (bc, br) = (
            self.selected_tile % self.sheet_cols(),
            self.selected_tile / self.sheet_cols(),
        );
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
    pub(super) fn handle_select(&mut self, camera_pos: Vec2, mouse: &MouseInput) {
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
    /// needs_reload)`, or `None` if the active layer isn't an editable tile
    /// layer (an image layer carries a bitmap, not cells). Mirrors the
    /// [`handle_paint`](Self::handle_paint) target guard; `needs_reload` is set
    /// when editing the layer must re-derive runtime state — the collision layer
    /// (colliders) or a sprite-plane layer (flood-fill components).
    pub(super) fn selection_layer(&self, map: &MapInfo) -> Option<(String, usize, bool)> {
        let (source, layer) = self
            .active_layer(map)
            .filter(|l| l.kind == LayerKind::Tiles)
            .map(|l| (map.source.clone(), l.source_layer))?;
        let is_collision = map.layers.first().map(|l| l.source_layer) == Some(layer);
        let needs_reload = is_collision || Self::is_sprite_layer(map, layer);
        Some((source, layer, needs_reload))
    }

    /// Copy the active layer's tiles under the marquee into the clipboard (cells
    /// off the layer read as empty). Non-destructive — no undo entry.
    pub(super) fn selection_copy(&mut self, maps: &MapStore, map: &MapInfo) {
        let (Some(sel), Some((source, layer, _))) = (self.selection, self.selection_layer(map))
        else {
            return;
        };
        let Some(tiles) = maps.get(&source) else {
            return;
        };
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
        self.clipboard = Some(Clipboard {
            w: sel.w,
            h: sel.h,
            tiles: buf,
        });
    }

    /// Copy the marquee, then clear it to empty — a single undo step.
    pub(super) fn selection_cut(&mut self, maps: &mut MapStore, map: &MapInfo) {
        self.selection_copy(maps, map);
        self.selection_delete(maps, map);
    }

    /// Clear every cell under the marquee to the empty tile, as one undo step.
    pub(super) fn selection_delete(&mut self, maps: &mut MapStore, map: &MapInfo) {
        let (Some(sel), Some((source, layer, needs_reload))) =
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
                self.paint_cell(
                    maps,
                    &source,
                    layer,
                    sel.x + dx as i32,
                    sel.y + dy as i32,
                    0,
                );
            }
        }
        self.flush_stroke();
        if needs_reload {
            self.pending_reload = true;
        }
    }

    /// Stamp the clipboard with its top-left at the marquee's origin, as one undo
    /// step (cells off the layer are skipped). Click to drop a 1×1 marquee where
    /// you want the paste to land.
    pub(super) fn selection_paste(&mut self, maps: &mut MapStore, map: &MapInfo) {
        let (Some(sel), Some(clip)) = (self.selection, self.clipboard.clone()) else {
            return;
        };
        let Some((source, layer, needs_reload)) = self.selection_layer(map) else {
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
                self.paint_cell(
                    maps,
                    &source,
                    layer,
                    sel.x + dx as i32,
                    sel.y + dy as i32,
                    value,
                );
            }
        }
        self.flush_stroke();
        if needs_reload {
            self.pending_reload = true;
        }
    }

    /// Settle a finished object drag: if the origin actually changed, record a
    /// single move as one undo step.
    pub(super) fn finish_move(&mut self, map: &mut MapInfo) {
        let Some(drag) = self.moving.take() else {
            return;
        };
        let Some(i) = self.selected else { return };
        let from = drag.from;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use egg_platform::null_console::NullConsole;

    /// The Select tool's clipboard ops: copy lifts the marquee's tiles, paste
    /// stamps them at a new origin as one undo step, cut clears the source while
    /// keeping the buffer, and a collision-layer edit flags an immediate re-derive.
    #[test]
    fn select_copy_cut_paste_and_delete() {
        let mut maps = MapStore::default();
        maps.insert("m", egg_world::data::tiled::TiledMap::blank_modern(6, 4));
        // A 2×2 block of tile 5 at the origin on the drawable layer (source 1).
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            maps.get_mut("m").unwrap().set(1, x, y, 5);
        }
        let map = MapInfo {
            source: "m".to_string(),
            layers: vec![
                LayerInfo {
                    source_layer: 0,
                    ..LayerInfo::DEFAULT_LAYER
                }, // collision
                LayerInfo {
                    source_layer: 1,
                    ..LayerInfo::DEFAULT_LAYER
                }, // drawable
            ],
            ..MapInfo::default()
        };
        let mut viewer = MapViewer {
            tool: EditorTool::Select,
            layer_index: 1,
            ..Default::default()
        };

        // Copy the 2×2 block.
        viewer.selection = Some(TileSelection {
            x: 0,
            y: 0,
            w: 2,
            h: 2,
        });
        viewer.selection_copy(&maps, &map);
        assert_eq!(viewer.clipboard.as_ref().map(|c| (c.w, c.h)), Some((2, 2)));
        assert_eq!(viewer.clipboard.as_ref().unwrap().tiles, vec![5, 5, 5, 5]);

        // Paste at (3,1): the block lands there as one undo step.
        viewer.selection = Some(TileSelection {
            x: 3,
            y: 1,
            w: 1,
            h: 1,
        });
        viewer.selection_paste(&mut maps, &map);
        let m = maps.get("m").unwrap();
        assert_eq!(
            [
                m.get(1, 3, 1),
                m.get(1, 4, 1),
                m.get(1, 3, 2),
                m.get(1, 4, 2)
            ],
            [Some(5), Some(5), Some(5), Some(5)]
        );
        assert!(viewer.history.can_undo(), "paste records an undo step");

        // Cut the original: it clears, but the clipboard still holds the block.
        viewer.selection = Some(TileSelection {
            x: 0,
            y: 0,
            w: 2,
            h: 2,
        });
        viewer.selection_cut(&mut maps, &map);
        let m = maps.get("m").unwrap();
        assert_eq!([m.get(1, 0, 0), m.get(1, 1, 1)], [Some(0), Some(0)]);
        assert_eq!(viewer.clipboard.as_ref().unwrap().tiles, vec![5, 5, 5, 5]);

        // Deleting on the collision layer (index 0) clears + flags a re-derive.
        maps.get_mut("m").unwrap().set(0, 0, 0, 9);
        viewer.layer_index = 0;
        viewer.selection = Some(TileSelection {
            x: 0,
            y: 0,
            w: 2,
            h: 2,
        });
        viewer.pending_reload = false;
        viewer.selection_delete(&mut maps, &map);
        assert_eq!(maps.get("m").unwrap().get(0, 0, 0), Some(0));
        assert!(viewer.pending_reload, "collision edits re-derive colliders");

        // Undoing that collision edit must also re-derive (restore tile + flag).
        let mut map = map; // undo/redo take &mut MapInfo
        viewer.pending_reload = false;
        viewer.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(
            maps.get("m").unwrap().get(0, 0, 0),
            Some(9),
            "undo restores the tile"
        );
        assert!(
            viewer.pending_reload,
            "undoing a collision edit re-derives too"
        );
    }

    /// The fixed-32 palette maps a cursor to the right sheet tile, box-selects a
    /// brush, clamps a drag that runs off the viewport, and bounds scroll so the
    /// last column/row can reach the edge.
    #[test]
    fn palette_box_select_and_scroll_bounds() {
        let mut v = MapViewer {
            pal_rect: Rect {
                x: 4,
                y: 20,
                w: 80,
                h: 64,
            }, // 10 cols x 8 rows visible
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
        assert_eq!(
            v.palette_tile_at(Vec2::new(500, 500)),
            (5 + 10 - 1, 2 + 8 - 1)
        );
        // Scroll bounds: 10 of 32 cols, 8 of the sheet's 128 rows visible.
        assert_eq!(v.palette_scroll_max(), (32 - 10, 128 - 8));
    }

    /// A scroll-bar drag preserves the grab offset: the thumb tracks the cursor
    /// instead of snapping its top under it. (A bigger grab offset at the same
    /// cursor scrolls less, and the old behaviour ignored grab entirely.)
    #[test]
    fn scroll_bar_drag_preserves_grab_offset() {
        let base = MapViewer {
            pal_rect: Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 80,
            }, // 10x10 visible
            sheet: (10, 30), // 10 cols, 30 rows -> max_r = 20
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
            pal_rect: Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 64,
            }, // 10 x 8 tiles visible
            ..Default::default()
        };
        assert_eq!((v.sheet_cols(), v.sheet_rows()), (32, 128)); // fallback
        v.sheet = (40, 200); // a bigger sheet after an art update
        assert_eq!(v.sheet_cols(), 40);
        assert_eq!(v.sheet_tiles(), 8000);
        assert_eq!(v.palette_scroll_max(), (40 - 10, 200 - 8));
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
}

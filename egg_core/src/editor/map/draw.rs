//! The map editor's draw pass: the world overlay, the panels, the tile
//! palette, the sprite / dialogue / warp previews, and the tool overlays.

use super::*;

impl MapViewer {
    pub fn draw_map_viewer(
        &self,
        draw_state: &mut DrawState,
        input: &EggInput,
        font: &Font,
        maps: &MapStore,
        walkaround: &WalkaroundState,
    ) {
        self.draw_at(
            draw_state,
            input,
            font,
            &walkaround.current_map,
            maps,
            walkaround.camera.pos,
        );
    }

    /// Draw the dock resize bars (the inner-edge splitter band per occupied dock
    /// side). Drawn between the docked panels and the floats so a floating window
    /// sits on top of any bar it overlaps.
    pub(super) fn draw_splitters(&self, draw_state: &mut DrawState) {
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
    pub(super) fn draw_grid(
        &self,
        draw_state: &mut DrawState,
        input: &EggInput,
        font: &Font,
        map: &MapInfo,
        maps: &MapStore,
        camera_pos: Vec2,
    ) {
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

        // Frame the map's extent: the tile area is world `(0,0)..(w*8, h*8)`, which
        // maps to screen `world - camera`. Draw the outline one pixel *outside* the
        // map so it brackets the edge tiles, each side clamped to the canvas view
        // and skipped when it falls off-screen (so a map larger than the view
        // doesn't draw a misleading frame at the viewport edge).
        if let Some((mw, mh)) = maps
            .get(&map.source)
            .map(|t| (t.width as i32 * 8, t.height as i32 * 8))
        {
            let border = draw_state.colour(11);
            let (fx0, fy0) = (-cx - 1, -cy - 1);
            let (fx1, fy1) = (mw - cx, mh - cy);
            // A vertical / horizontal 1px edge, clamped to the canvas viewport and
            // skipped when the edge's own axis is outside it.
            let vline = |ds: &mut DrawState, x: i32, ya: i32, yb: i32| {
                if x < wx0 || x >= wx1 {
                    return;
                }
                let (y0, y1) = (ya.max(wy0), yb.min(wy1));
                if y1 > y0 {
                    ds.rgba(LayerId::BG).fill_rect(x, y0, 1, y1 - y0, border);
                }
            };
            let hline = |ds: &mut DrawState, y: i32, xa: i32, xb: i32| {
                if y < wy0 || y >= wy1 {
                    return;
                }
                let (x0, x1) = (xa.max(wx0), xb.min(wx1));
                if x1 > x0 {
                    ds.rgba(LayerId::BG).fill_rect(x0, y, x1 - x0, 1, border);
                }
            };
            hline(draw_state, fy0, fx0, fx1 + 1);
            hline(draw_state, fy1, fx0, fx1 + 1);
            vline(draw_state, fx0, fy0, fy1 + 1);
            vline(draw_state, fx1, fy0, fy1 + 1);
        }
        // Cursor tile-coordinate readout, bottom-right — clear of the global
        // undo/redo/save bar at the world's top-left (which draws on top), even
        // when the world is only a little wider than that bar.
        if world.contains(input.mouse.pos()) {
            let (tx, ty) = world_tile(&input.mouse, camera_pos);
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
                font,
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
        input: &EggInput,
        font: &Font,
        map: &MapInfo,
        maps: &MapStore,
        camera_pos: Vec2,
    ) {
        if !self.focused {
            return;
        }
        // The path recorder draws over everything.
        if self.path_recorder.is_some() {
            self.draw_path_recorder_fullscreen(draw_state, font, map, maps);
            return;
        }
        // A fullscreen warp-destination placement session draws over everything.
        if self.warp_preview.is_some() {
            self.draw_warp_preview_fullscreen(draw_state, font, maps);
            return;
        }
        // The scene picker draws over everything.
        if self.scene_picker.is_some() {
            self.draw_scene_picker_fullscreen(draw_state, font);
            return;
        }
        self.draw_hidden_active_layer(draw_state, map, maps, camera_pos);
        self.draw_grid(draw_state, input, font, map, maps, camera_pos);
        self.draw_canvas_overlay(draw_state, input, map, camera_pos);
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
                let title_clip = Rect {
                    x: rect.x,
                    y: rect.y,
                    w: rect.w,
                    h: PANEL_TITLE_H,
                };
                ui.draw_at_clipped(
                    rect.x,
                    rect.y - scroll,
                    body,
                    draw_state,
                    font,
                    LayerId::BG,
                );
                ui.draw_at_clipped(rect.x, rect.y, title_clip, draw_state, font, LayerId::BG);
                self.draw_panel_scrollbar(rect, scroll, ui.content_height(), draw_state);
            } else {
                ui.draw_at(rect.x, rect.y, draw_state, font, LayerId::BG);
            }
            match self.dock.panels[idx].kind {
                PanelKind::Maps => self.draw_map_thumbnails(&ui, rect, maps, draw_state),
                PanelKind::Paint => self.draw_palette(draw_state),
                PanelKind::Objects => {
                    self.draw_warp_preview(&ui, rect, idx, map, maps, draw_state);
                    self.draw_sprite_preview(&ui, rect, idx, map, draw_state);
                }
                PanelKind::Dialogue => self.draw_dialogue_preview(draw_state, font),
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
            .draw_at(world.x + 1, world.y + 1, draw_state, font, LayerId::BG);
        // A modal map dialog, centred over everything.
        if self.maps_dialog.is_active() {
            self.build_dialog()
                .draw_at(0, 0, draw_state, font, LayerId::BG);
        }
    }

    /// While painting a *hidden* layer (e.g. the collision layer), ghost its
    /// tiles over the world — checkerboard-dithered — so you can see what you're
    /// editing without un-hiding it.
    pub(super) fn draw_hidden_active_layer(
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
        ghost.map_draw_indexed(
            tl,
            &draw_state.indexed_sprites,
            &draw_state.palettes[0],
            &pmap,
            opts,
        );
        knockout_checker(&mut ghost);
        blit_ghost(
            draw_state.rgba(LayerId::BG),
            -(camera_pos.x as i32),
            -(camera_pos.y as i32),
            &ghost,
        );
    }

    /// Blit the Paint palette into its viewport: the sheet's 32-wide tile grid,
    /// scrolled to `(pal_col, pal_row)`, the selected tile outlined, plus thin
    /// scroll bars on the overflowing edges.
    pub(super) fn draw_palette(&self, draw_state: &mut DrawState) {
        let v = self.pal_rect;
        if v.w <= 0 || v.h <= 0 {
            return;
        }
        let (vc, vr) = self.palette_visible();
        let (bw, bh) = self.brush_size();
        let (bc, br) = (
            self.selected_tile % self.sheet_cols(),
            self.selected_tile / self.sheet_cols(),
        );
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
                let opts = SpriteOptions {
                    transparent: Some(0),
                    ..Default::default()
                };
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
        if max_r > 0 {
            let bx = v.x + v.w - 2;
            let (th, travel) = self.palette_thumb_v();
            let ty = thumb_pos(i32::from(v.y), travel, self.pal_row as i32, max_r as i32);
            self.draw_scrollbar(
                draw_state,
                Rect {
                    x: bx,
                    y: v.y,
                    w: 2,
                    h: v.h,
                },
                Rect {
                    x: bx,
                    y: ty as i16,
                    w: 2,
                    h: th as i16,
                },
            );
        }
        if max_c > 0 {
            let by = v.y + v.h - 2;
            let (tw, travel) = self.palette_thumb_h();
            let tx = thumb_pos(i32::from(v.x), travel, self.pal_col as i32, max_c as i32);
            self.draw_scrollbar(
                draw_state,
                Rect {
                    x: v.x,
                    y: by,
                    w: v.w,
                    h: 2,
                },
                Rect {
                    x: tx as i16,
                    y: by,
                    w: tw as i16,
                    h: 2,
                },
            );
        }
    }

    /// Blit a rendered preview of each visible map over its browser cell. Drawn
    /// after the panel UI so the thumbnail lands on top of the cell's outline.
    pub(super) fn draw_map_thumbnails(
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

    /// Blit a rendered preview of the selected warp's destination map over its
    /// preview box (after the panel UI, so it lands on top of the box outline),
    /// with the player drawn at the landing point and a crosshair pinpointing it.
    /// A no-op unless a warp is selected (the box is only emitted then). An
    /// unknown target (a free-typed name with no map) gets an `X` instead.
    ///
    /// `scroll` shifts the box with the panel body and `clip` (the body region,
    /// when the panel scrolls) crops the blit/marks so a half-scrolled preview
    /// stays inside the panel.
    /// Paint the selected object's current animation frame (the live-preview
    /// cursor) into its [`SpritePreview`](EditorKey::SpritePreview) box — centred,
    /// clipped to the panel body — so the sprite animates as you edit its frames.
    pub(super) fn draw_sprite_preview(
        &self,
        ui: &Ui<EditorKey>,
        rect: Rect,
        idx: usize,
        map: &MapInfo,
        draw_state: &mut DrawState,
    ) {
        let Some(frames) = self
            .selected
            .and_then(|i| map.objects.get(i))
            .and_then(|o| o.sprite.as_deref())
            .filter(|frames| !frames.is_empty())
        else {
            return;
        };
        let frame = &frames[self.preview_frame % frames.len()];
        let (scroll, scrolling) = self.panel_scroll(idx, rect, ui.content_height());
        let clip = scrolling.then(|| Self::panel_body(rect));
        let Some(box_rect) = ui.rect_at(rect.x, rect.y - scroll, EditorKey::SpritePreview) else {
            return;
        };
        // Skip if the box scrolled entirely out of the panel body.
        if clip.is_some_and(|c| clamp_to(box_rect, c).is_none()) {
            return;
        }
        // Render the frame centred into a box-sized image, so it clips to the box
        // (the sprite may be larger than the box), then blit clipped to the body.
        let mut img = RgbaImage::new(box_rect.w as u32, box_rect.h as u32);
        let scale = frame.options.scale.max(1);
        let sw = frame.options.w.max(1) * 8 * scale;
        let sh = frame.options.h.max(1) * 8 * scale;
        let ox = (box_rect.w as i32 - sw) / 2;
        let oy = (box_rect.h as i32 - sh) / 2;
        let (sprites, palette) = (&draw_state.indexed_sprites, &draw_state.palettes[0]);
        let pal_map = palette_map_rotate(frame.palette_rotate as usize);
        let id = i32::from(frame.spr_id);
        if let Some(outline) = frame.outline_colour {
            img.spr_outline(sprites, palette, id, ox, oy, frame.options.clone(), outline);
        }
        img.spr_indexed(
            sprites,
            palette,
            &pal_map,
            id,
            ox,
            oy,
            frame.options.clone(),
        );
        blit_clipped(draw_state, box_rect, &img, clip);
    }

    /// Paint the faithful in-game dialogue box for the previewed message — the
    /// real [`Dialogue`] renderer, fully revealed, at its usual bottom-anchored
    /// spot. Screen-anchored, so it's independent of the Dialog panel's rect; the
    /// cached `dialogue_preview` already holds the resolved message to show.
    pub(super) fn draw_dialogue_preview(&self, draw_state: &mut DrawState, font: &Font) {
        let len = self.dialogue_preview.len();
        if len == 0 {
            return;
        }
        let message = &self.dialogue_preview[self.dialogue_msg.min(len - 1)];
        let small = self.dialogue_small_text;
        let dialogue = Dialogue {
            portrait: message.portrait.clone(),
            flip_portrait: message.flip_portrait,
            ..Dialogue::default()
        };
        // Wrap to the box width exactly as in-game, then draw fully revealed. The
        // box re-centres on the render target, so it lands at the bottom-middle of
        // this view and keeps its margin across resizes — faithful to gameplay.
        let text = dialogue.fit_text(font, small, &message.to_plain_string());
        dialogue.draw_dialogue_box(draw_state, LayerId::BG, font, small, &text, false);
    }

    pub(super) fn draw_warp_preview(
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
        let (dest, landing) = match self
            .selected
            .and_then(|i| map.objects.get(i))
            .map(|o| &o.effect)
        {
            Some(ObjectEffect::Warp(w)) => {
                (w.map.clone().unwrap_or_else(|| map.source.clone()), w.to)
            }
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
            let (x1, y1) = (
                (box_rect.x + box_rect.w) as i32 - 1,
                (box_rect.y + box_rect.h) as i32 - 1,
            );
            let layer = draw_state.rgba(LayerId::BG);
            layer.line(x0, y0, x1, y1, bad);
            layer.line(x0, y1, x1, y0, bad);
            return;
        };

        // Draw the player at the spawn point (its feet on the landing pixel, as in
        // the live game), outlined so it reads even once downscaled.
        let (sprite, _) = Shell::default().sprite_options();
        let opts = SpriteOptions {
            transparent: Some(0),
            ..sprite
        };
        let (px, py) = (
            landing.x as i32 - opts.x_offset,
            landing.y as i32 - opts.y_offset,
        );
        let (sprites, palette) = (&draw_state.indexed_sprites, &draw_state.palettes[0]);
        full.spr_outline(sprites, palette, opts.id, px, py, opts.clone(), 0);
        full.spr_indexed(
            sprites,
            palette,
            &PALETTE_MAP_IDENTITY,
            opts.id,
            px,
            py,
            opts,
        );

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
            let (x1, y1) = (
                (bound.x + bound.w) as i32 - 1,
                (bound.y + bound.h) as i32 - 1,
            );
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
    pub(super) fn draw_canvas_overlay(
        &self,
        draw_state: &mut DrawState,
        input: &EggInput,
        map: &MapInfo,
        camera_pos: Vec2,
    ) {
        match self.tool {
            EditorTool::Paint => self.draw_paint_overlay(draw_state, input, camera_pos),
            EditorTool::Select => self.draw_select_overlay(draw_state, camera_pos),
            EditorTool::Interactables | EditorTool::Warps => {
                self.draw_object_overlay(draw_state, input, map, camera_pos)
            }
            EditorTool::Layers => {}
        }
    }

    /// Paint tool overlay: a Shift+drag rectangle-fill outline, or a dithered
    /// ghost of the brush footprint under the cursor with its outline.
    pub(super) fn draw_paint_overlay(
        &self,
        draw_state: &mut DrawState,
        input: &EggInput,
        camera_pos: Vec2,
    ) {
        let cx = i32::from(camera_pos.x);
        let cy = i32::from(camera_pos.y);
        let colour = draw_state.colour(11);
        if let Some(start) = self.drag {
            // Shift+drag rectangle fill: outline the tile-snapped region.
            let m = input.mouse;
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
            let (tx, ty) = world_tile(&input.mouse, camera_pos);
            let (px, py) = (tx * 8 - cx, ty * 8 - cy);
            let (bw, bh) = self.brush_size();
            let (bc, br) = (
                self.selected_tile % self.sheet_cols(),
                self.selected_tile / self.sheet_cols(),
            );
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
                        SpriteOptions {
                            transparent: Some(0),
                            ..Default::default()
                        },
                    );
                }
            }
            // Knock out a checkerboard so it reads as a preview, not paint.
            knockout_checker(&mut ghost);
            blit_ghost(draw_state.rgba(LayerId::BG), px, py, &ghost);
            draw_state.rgba(LayerId::BG).stroke_rect(
                px,
                py,
                (bw * 8) as i32,
                (bh * 8) as i32,
                colour,
            );
        }
    }

    /// Select tool overlay: the marquee outline, plus a paste preview ghost of
    /// the clipboard at the marquee origin where `SelPaste` would stamp it.
    pub(super) fn draw_select_overlay(&self, draw_state: &mut DrawState, camera_pos: Vec2) {
        let cx = i32::from(camera_pos.x);
        let cy = i32::from(camera_pos.y);
        let Some(sel) = self.selection else {
            return;
        };
        let outline = draw_state.colour(11);
        let (px, py) = (sel.x * 8 - cx, sel.y * 8 - cy);
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
                        SpriteOptions {
                            transparent: Some(0),
                            ..Default::default()
                        },
                    );
                }
            }
            knockout_checker(&mut ghost);
            blit_ghost(draw_state.rgba(LayerId::BG), px, py, &ghost);
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

    /// Object tools (Interactables / Warps) overlay: every object of the active
    /// tab's kind outlined (warps 12, interactions 14, the selected one 11),
    /// plus the in-progress new-object drag box.
    pub(super) fn draw_object_overlay(
        &self,
        draw_state: &mut DrawState,
        input: &EggInput,
        map: &MapInfo,
        camera_pos: Vec2,
    ) {
        let cx = i32::from(camera_pos.x);
        let cy = i32::from(camera_pos.y);
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
        self.draw_drag_preview(draw_state, input, camera_pos);
    }

    pub(super) fn draw_drag_preview(
        &self,
        draw_state: &mut DrawState,
        input: &EggInput,
        camera_pos: Vec2,
    ) {
        if let Some(start) = self.drag {
            let m = input.mouse;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Stepping then drawing a layout that exercises docked panels, a floating
    /// panel, the global bar, splitters and a drop-zone highlight must not panic —
    /// coverage for `build_panel`/`draw_at` across the dock features.
    #[test]
    fn draw_across_dock_features_does_not_panic() {
        use crate::platform::test_console::TestConsole;

        let mut console = TestConsole::new();
        let mut draw = DrawState::default();
        let mut store = MapStore::default();
        let screen = (240.0, 136.0);
        let mut map = MapInfo::default();

        let mut viewer = MapViewer {
            focused: true,
            show_grid: true,
            ..Default::default()
        };
        viewer.dock.toggle_panel(PanelKind::Maps); // open the Maps panel too
        viewer.dock.toggle_panel(PanelKind::Map); // and the Setup panel
        viewer.dock.set_float(1, Vec2::new(100, 30), 80, 60); // float the Paint panel
        viewer.step_map_viewer_at(
            &mut console,
            &EggInput::new(),
            &mut map,
            &mut store,
            Vec2::new(0, 0),
            screen,
            (0, 0),
            &Script::default(),
            &SaveData::default(),
        );
        // Force the drop-zone highlight branch.
        viewer.dock.solved.hot_edge = Some(Side::Right);
        viewer.draw_at(&mut draw, &EggInput::new(), &Font::blank(), &map, &store, Vec2::new(0, 0));
    }

    /// Rendering a thumbnail for a blank modern map produces an image that fits
    /// the cell — panic coverage for the P3 preview render path.
    #[test]
    fn thumbnail_renders_within_the_cell() {
        // A real sheet so the modern-map collider derivation has art to read.
        let draw = DrawState {
            indexed_sprites: crate::render::image::IndexedImage::new(256, 256),
            ..Default::default()
        };
        let mut maps = MapStore::default();
        maps.insert("m", crate::data::tiled::TiledMap::blank_modern(10, 8));

        let thumb = render_map_thumbnail("m", &maps, &draw, 40, 22).expect("thumbnail");
        assert!(thumb.width() <= 40 && thumb.height() <= 22);
        assert!(thumb.width() >= 1 && thumb.height() >= 1);
    }
}

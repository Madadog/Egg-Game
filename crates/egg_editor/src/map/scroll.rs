//! Panel chrome geometry: scroll bars, drag-reorder of layer/frame rows, and
//! the panel hit/wheel plumbing — the interactive scaffolding the dock panels
//! sit in.

use super::*;

impl MapViewer {
    /// Whether a panel of `kind` scrolls when its content overflows. The Maps
    /// browser pages instead, and the Paint palette scrolls its own viewport, so
    /// both opt out.
    pub(super) fn is_scroll_kind(kind: PanelKind) -> bool {
        !matches!(kind, PanelKind::Maps | PanelKind::Paint)
    }

    /// This panel's scroll state for the frame: the clamped scroll offset and
    /// whether it is actually scrolling (a scroll-kind whose content overflows
    /// `rect`). `content_h` comes from the built [`Ui::content_height`].
    pub(super) fn panel_scroll(&self, idx: usize, rect: Rect, content_h: i16) -> (i16, bool) {
        let kind = self.dock.panels[idx].kind;
        let overflow = content_h - rect.h;
        if !Self::is_scroll_kind(kind) || overflow <= 0 {
            return (0, false);
        }
        (self.dock.scroll(idx).clamp(0, overflow), true)
    }

    /// The body region of a scrolling panel (below the pinned title), where the
    /// scrolled content is clipped to.
    pub(super) fn panel_body(rect: Rect) -> Rect {
        Rect {
            x: rect.x,
            y: rect.y + PANEL_TITLE_H,
            w: rect.w,
            h: (rect.h - PANEL_TITLE_H).max(0),
        }
    }

    /// The scroll bar's grab band — a thin strip down the body's right edge.
    pub(super) fn scrollbar_zone(rect: Rect) -> Rect {
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
    pub(super) fn scroll_thumb(body: Rect, scroll: i16, content_h: i16) -> (i16, i16) {
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

    /// Fill a scroll bar: the `track` rect in dim colour 0, the `thumb` rect in
    /// the brighter colour 13. Shared by the panel scroll bar and both palette bars.
    pub(super) fn draw_scrollbar(&self, draw_state: &mut DrawState, track: Rect, thumb: Rect) {
        let track_c = draw_state.colour(0);
        draw_state.rgba(LayerId::BG).fill_rect(
            track.x.into(),
            track.y.into(),
            track.w.into(),
            track.h.into(),
            track_c,
        );
        let thumb_c = draw_state.colour(13);
        draw_state.rgba(LayerId::BG).fill_rect(
            thumb.x.into(),
            thumb.y.into(),
            thumb.w.into(),
            thumb.h.into(),
            thumb_c,
        );
    }

    /// Draw a panel's vertical scroll bar down the right edge of its body: a dim
    /// track with a brighter thumb sized/positioned by the scroll fraction.
    pub(super) fn draw_panel_scrollbar(
        &self,
        rect: Rect,
        scroll: i16,
        content_h: i16,
        draw_state: &mut DrawState,
    ) {
        let body = Self::panel_body(rect);
        if body.h <= 0 {
            return;
        }
        let bx = body.x + body.w - SCROLLBAR_W;
        let (top, thumb_h) = Self::scroll_thumb(body, scroll, content_h);
        self.draw_scrollbar(
            draw_state,
            Rect {
                x: bx,
                y: body.y,
                w: SCROLLBAR_W,
                h: body.h,
            },
            Rect {
                x: bx,
                y: top,
                w: SCROLLBAR_W,
                h: thumb_h,
            },
        );
    }

    /// Front-to-back pick across one panel, scroll-aware: a non-scrolling panel
    /// hits normally; a scrolling one hits its scroll-bar grab band, then its
    /// pinned title, then its scrolled (clipped) body.
    pub(super) fn hit_panel(
        &self,
        idx: usize,
        rect: Rect,
        ui: &Ui<EditorKey>,
        cursor: Vec2,
    ) -> Option<EditorKey> {
        let (scroll, scrolling) = self.panel_scroll(idx, rect, ui.content_height());
        if !scrolling {
            return ui.hit_at(rect.x, rect.y, cursor);
        }
        if Self::scrollbar_zone(rect).contains(cursor) {
            return Some(EditorKey::PanelScroll(idx));
        }
        let title_clip = Rect {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: PANEL_TITLE_H,
        };
        let body = Self::panel_body(rect);
        ui.hit_at_clipped(rect.x, rect.y, title_clip, cursor)
            .or_else(|| ui.hit_at_clipped(rect.x, rect.y - scroll, body, cursor))
    }

    /// A mouse-wheel notch over a scrollable panel scrolls its body (independent
    /// of any keyed row beneath the cursor). Topmost panel under the cursor wins.
    pub(super) fn handle_panel_wheel(
        &mut self,
        mouse: &MouseInput,
        map: &MapInfo,
        maps: &MapStore,
    ) {
        let sy = mouse.scroll_y[0];
        if sy == 0 {
            return;
        }
        let cursor = mouse.pos();
        let Some(&(idx, rect)) = self
            .dock
            .solved
            .rects
            .iter()
            .rev()
            .find(|(_, r)| r.contains(cursor))
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
        self.dock.set_scroll(
            idx,
            (cur - sy.signum() as i16 * SCROLL_STEP).clamp(0, overflow),
        );
    }

    /// Advance an active scroll-bar drag: map the cursor (less the grab offset)
    /// onto the panel's scroll range. Release ends it. Returns `true` while it
    /// owns the mouse, so the caller suppresses other panel/canvas input.
    pub(super) fn step_scroll_drag(
        &mut self,
        mouse: &MouseInput,
        map: &MapInfo,
        maps: &MapStore,
    ) -> bool {
        let CanvasDrag::Scrollbar { idx, grab } = self.canvas_drag else {
            return false;
        };
        if released(mouse.left) {
            self.canvas_drag = CanvasDrag::None;
            return true;
        }
        let Some(rect) = self.dock.solved.rect_of(idx) else {
            self.canvas_drag = CanvasDrag::None;
            return true;
        };
        let content_h = self.build_panel(idx, rect, map, maps).content_height();
        let body = Self::panel_body(rect);
        let overflow = (content_h - rect.h).max(1);
        let (_, thumb_h) = Self::scroll_thumb(body, 0, content_h);
        let travel = (body.h - thumb_h).max(1);
        let scroll = scroll_from_drag(
            i32::from(mouse.pos().y),
            i32::from(grab),
            i32::from(body.y),
            i32::from(travel),
            i32::from(overflow),
        ) as i16;
        self.dock.set_scroll(idx, scroll.clamp(0, overflow));
        true
    }

    /// Advance an active list drag-reorder (layers or sprite frames). Re-reads
    /// the row under the cursor each frame so the drop target tracks the cursor
    /// (sticky to the last valid row when hovering a gap or the toolbar), and on
    /// release commits the move — a no-op if dropped back on the grabbed row, so
    /// a plain click stays a select.
    pub(super) fn step_reorder_drag(
        &mut self,
        map: &mut MapInfo,
        maps: &mut MapStore,
        cursor: Vec2,
        up: bool,
    ) {
        let CanvasDrag::Reorder { list, from, at } = self.canvas_drag else {
            return;
        };
        // Borrow ends before the mutation below.
        let at = self.hovered_reorder_index(cursor, map, maps, list).unwrap_or(at);
        if up {
            self.canvas_drag = CanvasDrag::None;
            if at != from {
                match list {
                    ReorderList::Layers => self.reorder_layer_to(map, maps, from, at),
                    ReorderList::SpriteFrames => self.reorder_sprite_frame_to(map, from, at),
                }
            }
        } else {
            self.canvas_drag = CanvasDrag::Reorder { list, from, at };
        }
    }

    /// The index of the `list` row under `cursor`, by the same front-to-back
    /// panel pick `step_mouse_input` uses — so a drop reads whichever row the
    /// cursor is over regardless of panel placement / scroll. `None` when the
    /// cursor is off the list (a gap, the toolbar, another panel), which the
    /// caller treats as "hold the last target".
    pub(super) fn hovered_reorder_index(
        &self,
        cursor: Vec2,
        map: &MapInfo,
        maps: &MapStore,
        list: ReorderList,
    ) -> Option<usize> {
        for &(idx, rect) in self.dock.solved.rects.iter().rev() {
            let ui = self.build_panel(idx, rect, map, maps);
            if let Some(key) = self.hit_panel(idx, rect, &ui, cursor) {
                return match (list, key) {
                    // The eye column shares a layer row, so it counts as a target.
                    (ReorderList::Layers, EditorKey::Layer(i) | EditorKey::LayerVis(i)) => Some(i),
                    (ReorderList::SpriteFrames, EditorKey::SpriteFrame(i)) => Some(i),
                    _ => None,
                };
            }
        }
        None
    }

    /// Drag-reorder a layer: slide the layer at store index `from` to store index
    /// `to`, recording one [`EditAction::LayerMove`]. Rows are keyed by store
    /// index (map order), so `from`/`to` are already `source_layer`s — no
    /// per-plane translation. The collision layer is protected inside
    /// [`TiledMap::reorder_layer`], which also clamps `to` (returned as `b`).
    pub(super) fn reorder_layer_to(
        &mut self,
        map: &mut MapInfo,
        maps: &mut MapStore,
        from: usize,
        to: usize,
    ) {
        if let Some((a, b)) = maps
            .get_mut(&map.source)
            .and_then(|tm| tm.reorder_layer(from, to))
        {
            self.record(EditAction::LayerMove {
                source: map.source.clone(),
                from: a,
                to: b,
            });
            // Follow the dropped layer to its new store slot (the applied `to`).
            self.layer_index = b;
            self.pending_reload = true;
        }
    }

    /// Drag- or button-reorder the selected object's sprite frames: move the
    /// frame at `from` to index `to`, recorded as one object [`EditAction::Modify`]
    /// (via [`modify_object`](Self::modify_object)). The frame selection follows.
    pub(super) fn reorder_sprite_frame_to(&mut self, map: &mut MapInfo, from: usize, to: usize) {
        self.modify_object(map, |map, i| {
            if let Some(frames) = map.objects.get_mut(i).and_then(|o| o.sprite.as_mut())
                && from < frames.len()
                && to < frames.len()
            {
                let frame = frames.remove(from);
                frames.insert(to, frame);
            }
        });
        self.sprite_frame = to.min(self.sprite_frame_count(map).saturating_sub(1));
    }

    /// Move the selected sprite frame one step earlier (`up`) or later in the
    /// animation — the click/keyboard counterpart to dragging a frame row.
    pub(super) fn move_sprite_frame(&mut self, map: &mut MapInfo, up: bool) {
        let Some(cur) = self.current_frame(map) else {
            return;
        };
        let n = self.sprite_frame_count(map);
        let to = if up {
            cur.checked_sub(1)
        } else {
            (cur + 1 < n).then_some(cur + 1)
        };
        if let Some(to) = to {
            self.reorder_sprite_frame_to(map, cur, to);
        }
    }

    /// The active drag-reorder's `(from, at)` rows if it's rearranging `list`,
    /// else `None` — read by the list builders to recolour the grabbed and
    /// drop-target rows.
    pub(super) fn reorder_drag(&self, list: ReorderList) -> Option<(usize, usize)> {
        match self.canvas_drag {
            CanvasDrag::Reorder {
                list: l,
                from,
                at,
            } if l == list => Some((from, at)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scroll-kind panel whose content overflows scrolls, with the offset
    /// clamped to the overflow; panels that page/scroll their own viewport (Maps,
    /// Paint) never panel-scroll.
    #[test]
    fn panel_scroll_clamps_and_skips_non_scroll_kinds() {
        let mut v = MapViewer::default();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 84,
            h: 40,
        };
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
}

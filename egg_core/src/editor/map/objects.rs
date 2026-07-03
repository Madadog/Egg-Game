//! Map-object authoring: create / duplicate / delete, the animated-sprite
//! frames, and the text-entry commit path for an object's editable fields.

use super::*;

impl MapViewer {
    /// Real `map.objects` index of the active tab's object whose hitbox contains
    /// `world` (px) — so clicking in the Warps tab only grabs warps, and the
    /// returned index is the vec index selection works in.
    pub(super) fn object_at(&self, map: &MapInfo, world: Vec2) -> Option<usize> {
        let kind = self.obj_kind();
        map.objects
            .iter()
            .position(|o| kind.matches(o) && o.hitbox.touches_point(world))
    }

    /// Top-left (px) of object `i`'s hitbox.
    pub(super) fn object_origin(&self, map: &MapInfo, i: usize) -> Vec2 {
        map.objects
            .get(i)
            .map(|o| Vec2::new(o.hitbox.x, o.hitbox.y))
            .unwrap_or(Vec2::new(0, 0))
    }

    /// Move object `i`'s hitbox top-left to `pos`, keeping its size.
    pub(super) fn set_object_origin(&self, map: &mut MapInfo, i: usize, pos: Vec2) {
        if let Some(o) = map.objects.get_mut(i) {
            o.hitbox.x = pos.x;
            o.hitbox.y = pos.y;
        }
    }

    /// The object kind the active object tool creates / filters its view to.
    pub(super) fn obj_kind(&self) -> ObjKind {
        if self.tool == EditorTool::Warps {
            ObjKind::Warp
        } else {
            ObjKind::Interactable
        }
    }

    /// Point the panel at `key` (preview + browser highlight + the text-editor
    /// link target), resetting to the first message.
    pub(super) fn set_dialogue_key(&mut self, key: String) {
        self.dialogue_msg = 0;
        self.dialogue_key = Some(key);
    }

    /// Point the selected object at dialogue `key` (undoably): an interaction's
    /// dialogue key, or a warp's pre-warp narration. No selection ⇒ no-op (the
    /// pick still loads into the panel for preview/authoring).
    pub(super) fn assign_dialogue_key(&mut self, map: &mut MapInfo, key: &str) {
        let Some(i) = self.selected else { return };
        let is_warp = matches!(
            map.objects.get(i).map(|o| &o.effect),
            Some(ObjectEffect::Warp(_))
        );
        if is_warp {
            self.modify_warp(map, |w| w.narration = Some(key.to_string()));
        } else {
            self.modify_object(map, |map, i| {
                if let Some(ObjectEffect::Interact(interaction)) =
                    map.objects.get_mut(i).map(|o| &mut o.effect)
                {
                    *interaction = Interaction::Dialogue(key.to_string());
                }
            });
        }
    }

    pub(super) fn new_object(&mut self, map: &mut MapInfo, camera_pos: Vec2, w: i16, h: i16) {
        let x = camera_pos.x + w / 2;
        let y = camera_pos.y + h / 2;
        self.create_object(map, Hitbox::new(x, y, 16, 16));
    }

    pub(super) fn create_object(&mut self, map: &mut MapInfo, hitbox: Hitbox) {
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

    pub(super) fn delete_object(&mut self, map: &mut MapInfo) {
        let Some(i) = self.selected else { return };
        let before = Self::snapshot(map, i);
        remove_object(map, i);
        self.selected = None;
        self.stop_editing();
        if let Some(before) = before {
            self.record(EditAction::Remove { index: i, before });
        }
    }

    /// Duplicate the selected object, nudged a tile down-right so the copy is
    /// visible, select it, and record the append as one undo step.
    pub(super) fn duplicate_object(&mut self, map: &mut MapInfo) {
        let Some(i) = self.selected else { return };
        let Some(mut copy) = map.objects.get(i).cloned() else {
            return;
        };
        // A disk-loaded object carries a stable Tiled `id`; cloning it would make
        // the duplicate share that id, and `to_tmj` only mints fresh ids for
        // `None`. Two same-id objects collapse in the save's `taken` set (keyed by
        // id), so collecting one removable pickup would filter out both on load.
        // Clear it so the duplicate gets its own id on save.
        copy.id = None;
        copy.hitbox.x += 8;
        copy.hitbox.y += 8;
        map.objects.push(copy);
        let index = map.objects.len() - 1;
        self.selected = Some(index);
        self.sprite_frame = 0;
        self.stop_editing();
        if let Some(after) = Self::snapshot(map, index) {
            self.record(EditAction::Add { index, after });
        }
    }

    /// The selected object's sprite frame count, or `0` if it has no sprite.
    pub(super) fn sprite_frame_count(&self, map: &MapInfo) -> usize {
        self.selected
            .and_then(|i| map.objects.get(i))
            .and_then(|o| o.sprite.as_ref())
            .map_or(0, Vec::len)
    }

    /// The selected frame's index, clamped to the selected object's frame count,
    /// or `None` if it has no sprite — heals a [`sprite_frame`](Self::sprite_frame)
    /// left stale by an undo/redo or a selection change.
    pub(super) fn current_frame(&self, map: &MapInfo) -> Option<usize> {
        let count = self.sprite_frame_count(map);
        (count > 0).then(|| self.sprite_frame.min(count - 1))
    }

    /// The frame [`sprite_frame`](Self::sprite_frame) points at within `object`'s
    /// sprite (clamped to the frame count), or `None` if it has no sprite.
    pub(super) fn selected_frame<'a>(
        &self,
        object: Option<&'a MapObject>,
    ) -> Option<&'a AnimFrame> {
        let frames = object.and_then(|o| o.sprite.as_ref())?;
        frames.get(self.sprite_frame.min(frames.len().saturating_sub(1)))
    }

    /// Advance the live-preview playback cursor one tick against the selected
    /// object's frames (mirrors [`Animation::advance`]). A no-op without a sprite.
    pub(super) fn advance_sprite_preview(&mut self, map: &MapInfo) {
        let count = self.sprite_frame_count(map);
        if count == 0 {
            self.preview_frame = 0;
            self.preview_tick = 0;
            return;
        }
        self.preview_frame %= count;
        let dur = self
            .selected
            .and_then(|i| map.objects.get(i))
            .and_then(|o| o.sprite.as_ref())
            .and_then(|frames| frames.get(self.preview_frame))
            .map_or(1, |frame| frame.duration.max(1));
        if self.preview_tick >= dur {
            self.preview_frame = (self.preview_frame + 1) % count;
            self.preview_tick = 0;
        } else {
            self.preview_tick += 1;
        }
    }

    /// Append a frame to the selected object's sprite (creating the sprite if it
    /// had none), seeded with the current palette brush tile, and select it. One
    /// undo step.
    pub(super) fn add_sprite_frame(&mut self, map: &mut MapInfo) {
        let tile = self.selected_tile as u16;
        let (bw, bh) = self.brush_size();
        self.modify_object(map, |map, i| {
            if let Some(o) = map.objects.get_mut(i) {
                // A multi-tile brush seeds a multi-tile frame: its top-left tile is
                // the `spr_id`, its box the sprite's `w`×`h` footprint.
                let mut frame = AnimFrame {
                    spr_id: tile,
                    ..AnimFrame::default()
                };
                frame.options.w = bw as i32;
                frame.options.h = bh as i32;
                match &mut o.sprite {
                    Some(frames) => frames.push(frame),
                    None => o.sprite = Some(vec![frame]),
                }
            }
        });
        self.sprite_frame = self.sprite_frame_count(map).saturating_sub(1);
    }

    /// Remove the selected frame from the selected object's sprite (dropping the
    /// whole sprite if it was the last frame), then clamp the selection. One undo
    /// step.
    pub(super) fn del_sprite_frame(&mut self, map: &mut MapInfo) {
        let Some(frame) = self.current_frame(map) else {
            return;
        };
        self.sprite_frame = frame;
        self.modify_object(map, |map, i| {
            if let Some(o) = map.objects.get_mut(i)
                && let Some(frames) = &mut o.sprite
            {
                frames.remove(frame);
                if frames.is_empty() {
                    o.sprite = None;
                }
            }
        });
        self.sprite_frame = self
            .sprite_frame
            .min(self.sprite_frame_count(map).saturating_sub(1));
    }

    /// Stamp the current palette brush into the selected frame: its top-left tile
    /// becomes the frame's `spr_id`, and the brush's `w`×`h` box becomes the
    /// sprite's multi-tile footprint — so a box-selected brush grabs the whole
    /// block in one click. Leaves the frame's other render settings (scale, flip,
    /// transparent, …) untouched. One undo step.
    pub(super) fn set_frame_from_brush(&mut self, map: &mut MapInfo) {
        let Some(frame) = self.current_frame(map) else {
            return;
        };
        let tile = self.selected_tile as u16;
        let (bw, bh) = self.brush_size();
        self.modify_object(map, |map, i| {
            if let Some(f) = frame_mut(map, i, frame) {
                f.spr_id = tile;
                f.options.w = bw as i32;
                f.options.h = bh as i32;
            }
        });
    }

    pub(super) fn begin_edit(&mut self, field: EditField, map: &MapInfo) {
        let object = self.selected.and_then(|i| map.objects.get(i));
        let effect = object.map(|o| &o.effect);
        let value = match (effect, field) {
            (Some(ObjectEffect::Interact(Interaction::Dialogue(k))), EditField::Key) => k.clone(),
            (Some(ObjectEffect::Interact(Interaction::Cutscene(n))), EditField::Scene) => n.clone(),
            (Some(ObjectEffect::Warp(w)), EditField::ToMap) => w.map.clone().unwrap_or_default(),
            (Some(ObjectEffect::Warp(w)), EditField::ToX) => w.to.x.to_string(),
            (Some(ObjectEffect::Warp(w)), EditField::ToY) => w.to.y.to_string(),
            (Some(ObjectEffect::Warp(w)), EditField::Narration) => {
                w.narration.clone().unwrap_or_default()
            }
            (
                Some(ObjectEffect::Interact(Interaction::Func(InteractFn::Note(p)))),
                EditField::Pitch,
            ) => p.to_string(),
            (
                Some(ObjectEffect::Interact(Interaction::Func(InteractFn::AddCreatures(c)))),
                EditField::Count,
            ) => c.to_string(),
            (
                Some(ObjectEffect::Interact(Interaction::Func(InteractFn::GiveItem(key)))),
                EditField::Item,
            ) => key.clone(),
            // The flag gate lives on the object itself, not the effect (common to
            // every kind). An unset condition seeds empty, so leaving the field
            // blank commits back to `None`.
            (_, EditField::CondIf) => object
                .and_then(|o| o.gate.if_flag.clone())
                .unwrap_or_default(),
            (_, EditField::CondUnless) => object
                .and_then(|o| o.gate.unless_flag.clone())
                .unwrap_or_default(),
            (_, EditField::Sets) => object
                .and_then(|o| o.gate.sets.clone())
                .unwrap_or_default(),
            // Hitbox geometry lives on the object itself, not the effect.
            (_, EditField::HitX) => object.map(|o| o.hitbox.x.to_string()).unwrap_or_default(),
            (_, EditField::HitY) => object.map(|o| o.hitbox.y.to_string()).unwrap_or_default(),
            (_, EditField::HitW) => object.map(|o| o.hitbox.w.to_string()).unwrap_or_default(),
            (_, EditField::HitH) => object.map(|o| o.hitbox.h.to_string()).unwrap_or_default(),
            // Sprite frame fields read the selected frame of the object's sprite.
            // The two `Option` fields seed empty when absent (an empty buffer
            // commits back to `None`).
            (_, EditField::FrameTile) => self
                .selected_frame(object)
                .map(|f| f.spr_id.to_string())
                .unwrap_or_default(),
            (_, EditField::FrameDuration) => self
                .selected_frame(object)
                .map(|f| f.duration.to_string())
                .unwrap_or_default(),
            (_, EditField::FrameOffX) => self
                .selected_frame(object)
                .map(|f| f.pos.x.to_string())
                .unwrap_or_default(),
            (_, EditField::FrameOffY) => self
                .selected_frame(object)
                .map(|f| f.pos.y.to_string())
                .unwrap_or_default(),
            (_, EditField::FrameW) => self
                .selected_frame(object)
                .map(|f| f.options.w.to_string())
                .unwrap_or_default(),
            (_, EditField::FrameH) => self
                .selected_frame(object)
                .map(|f| f.options.h.to_string())
                .unwrap_or_default(),
            (_, EditField::FrameScale) => self
                .selected_frame(object)
                .map(|f| f.options.scale.to_string())
                .unwrap_or_default(),
            (_, EditField::FramePaletteRot) => self
                .selected_frame(object)
                .map(|f| f.palette_rotate.to_string())
                .unwrap_or_default(),
            (_, EditField::FrameTransparent) => self
                .selected_frame(object)
                .and_then(|f| f.options.transparent)
                .map(|t| t.to_string())
                .unwrap_or_default(),
            (_, EditField::FrameOutline) => self
                .selected_frame(object)
                .and_then(|f| f.outline_colour)
                .map(|o| o.to_string())
                .unwrap_or_default(),
            _ => String::new(),
        };
        self.editing = Some(TextEdit {
            field,
            buffer: TextField::new(value),
            target: 0,
        });
    }

    pub(super) fn step_text_entry(
        &mut self,
        input: &EggInput,
        map: &mut MapInfo,
        maps: &mut MapStore,
    ) {
        if self.editing.is_none() {
            return;
        }
        // Tab accepts the top autocomplete match (when the field offers one) —
        // committing that whole vocabulary entry, the keyboard counterpart to
        // clicking the highlighted suggestion. Tab is inert in the buffer itself
        // (a control char), so it's free to mean "accept" here.
        if input.keyp(ScanCode::Tab) && !self.autocomplete_suggestions().is_empty() {
            self.accept_suggestion(map, maps, 0);
            return;
        }
        let Some(edit) = self.editing.as_mut() else {
            return;
        };
        match edit.buffer.step(input) {
            TextEvent::Active => {}
            TextEvent::Commit => {
                self.commit_edit(map, maps);
                self.stop_editing();
            }
            TextEvent::Cancel => self.stop_editing(),
        }
    }

    /// Snapshot the selected object, run `f` to mutate it, then record a single
    /// [`EditAction::Modify`] if it actually changed. The before/after snapshots
    /// make every field edit undoable without per-field bookkeeping.
    pub(super) fn modify_object(&mut self, map: &mut MapInfo, f: impl FnOnce(&mut MapInfo, usize)) {
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
    pub(super) fn modify_warp(&mut self, map: &mut MapInfo, f: impl FnOnce(&mut Warp)) {
        self.modify_object(map, |map, i| {
            if let Some(ObjectEffect::Warp(w)) = map.objects.get_mut(i).map(|o| &mut o.effect) {
                f(w);
            }
        });
    }

    pub(super) fn commit_edit(&mut self, map: &mut MapInfo, maps: &mut MapStore) {
        let Some(edit) = self.editing.as_ref() else {
            return;
        };
        let field = edit.field;
        let buffer = edit.buffer.text().trim().to_string();
        // Layer text edits target the store, not the selected object — handle
        // them up front (no object selection required).
        match field {
            EditField::LayerName => {
                self.commit_layer_rename(map, maps, &buffer);
                return;
            }
            // `f64::parse` accepts "NaN"/"inf"; reject them — a non-finite offset
            // serialises to `null` and breaks the next reload.
            EditField::LayerOffX => {
                if let Ok(v) = buffer.parse::<f64>()
                    && v.is_finite()
                {
                    self.commit_layer_prop(map, maps, LayerProp::OffsetX, v);
                }
                return;
            }
            EditField::LayerOffY => {
                if let Ok(v) = buffer.parse::<f64>()
                    && v.is_finite()
                {
                    self.commit_layer_prop(map, maps, LayerProp::OffsetY, v);
                }
                return;
            }
            EditField::LayerRotate => {
                if let Ok(v) = buffer.parse::<u8>() {
                    self.commit_layer_prop(map, maps, LayerProp::Rotate, f64::from(v % 16));
                }
                return;
            }
            _ => {}
        }
        if self.selected.is_none() {
            return;
        }
        match field {
            EditField::Key => self.modify_object(map, |map, i| {
                if let Some(ObjectEffect::Interact(interaction)) =
                    map.objects.get_mut(i).map(|o| &mut o.effect)
                {
                    *interaction = Interaction::Dialogue(buffer.clone());
                }
            }),
            EditField::Scene => self.modify_object(map, |map, i| {
                // The cutscene name is stored verbatim; it's resolved against the
                // loaded cutscene registry when the object fires.
                if let Some(ObjectEffect::Interact(interaction)) =
                    map.objects.get_mut(i).map(|o| &mut o.effect)
                {
                    *interaction = Interaction::Cutscene(buffer.clone());
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
            // Gate fields: the flag name is stored verbatim (empty buffer clears
            // that condition to `None`). Validated against the `#flag` vocabulary
            // only for display (the `?` marker), not on commit — so an author can
            // type a name before declaring it in the script.
            EditField::CondIf => self.modify_object(map, |map, i| {
                if let Some(object) = map.objects.get_mut(i) {
                    object.gate.if_flag = (!buffer.is_empty()).then(|| buffer.clone());
                }
            }),
            EditField::CondUnless => self.modify_object(map, |map, i| {
                if let Some(object) = map.objects.get_mut(i) {
                    object.gate.unless_flag = (!buffer.is_empty()).then(|| buffer.clone());
                }
            }),
            EditField::Sets => self.modify_object(map, |map, i| {
                if let Some(object) = map.objects.get_mut(i) {
                    object.gate.sets = (!buffer.is_empty()).then(|| buffer.clone());
                }
            }),
            EditField::Pitch => {
                if let Ok(pitch) = buffer.parse::<i32>() {
                    self.modify_object(map, |map, i| {
                        if let Some(ObjectEffect::Interact(Interaction::Func(InteractFn::Note(
                            p,
                        )))) = map.objects.get_mut(i).map(|o| &mut o.effect)
                        {
                            *p = pitch;
                        }
                    });
                }
            }
            EditField::Count => {
                if let Ok(count) = buffer.parse::<usize>() {
                    self.modify_object(map, |map, i| {
                        if let Some(ObjectEffect::Interact(Interaction::Func(
                            InteractFn::AddCreatures(c),
                        ))) = map.objects.get_mut(i).map(|o| &mut o.effect)
                        {
                            *c = count;
                        }
                    });
                }
            }
            // The item key is stored verbatim (a free-text registry key, empty
            // until typed); it's resolved against the item registry when the
            // object fires. A dropdown of known keys is a future autocomplete.
            EditField::Item => self.modify_object(map, |map, i| {
                if let Some(ObjectEffect::Interact(Interaction::Func(InteractFn::GiveItem(key)))) =
                    map.objects.get_mut(i).map(|o| &mut o.effect)
                {
                    *key = buffer.clone();
                }
            }),
            // Hitbox geometry: width/height keep a 1px floor so a box stays usable.
            // (X/Y deliberately have no floor — an object may sit at a negative
            // offset.) The field is selected inside the closure, where `o` exists.
            EditField::HitX | EditField::HitY | EditField::HitW | EditField::HitH => {
                if let Ok(v) = buffer.parse::<i16>() {
                    self.modify_object(map, |map, i| {
                        if let Some(o) = map.objects.get_mut(i) {
                            match field {
                                EditField::HitX => o.hitbox.x = v,
                                EditField::HitY => o.hitbox.y = v,
                                EditField::HitW => o.hitbox.w = v.max(1),
                                EditField::HitH => o.hitbox.h = v.max(1),
                                _ => unreachable!("outer arm guards the four hitbox fields"),
                            }
                        }
                    });
                }
            }
            // Sprite frame fields: write the parsed value into the selected frame
            // (duration floored to 1, never zero). `sprite_frame` is the editor's
            // current frame; `get_mut` clamps a stale index to a no-op.
            EditField::FrameTile => {
                if let (Ok(id), Some(frame)) = (buffer.parse::<u16>(), self.current_frame(map)) {
                    self.modify_object(map, |map, i| {
                        if let Some(f) = frame_mut(map, i, frame) {
                            f.spr_id = id;
                        }
                    });
                }
            }
            EditField::FrameDuration => {
                if let (Ok(d), Some(frame)) = (buffer.parse::<u16>(), self.current_frame(map)) {
                    self.modify_object(map, |map, i| {
                        if let Some(f) = frame_mut(map, i, frame) {
                            f.duration = d.max(1);
                        }
                    });
                }
            }
            EditField::FrameOffX => {
                if let (Ok(v), Some(frame)) = (buffer.parse::<i16>(), self.current_frame(map)) {
                    self.modify_object(map, |map, i| {
                        if let Some(f) = frame_mut(map, i, frame) {
                            f.pos.x = v;
                        }
                    });
                }
            }
            EditField::FrameOffY => {
                if let (Ok(v), Some(frame)) = (buffer.parse::<i16>(), self.current_frame(map)) {
                    self.modify_object(map, |map, i| {
                        if let Some(f) = frame_mut(map, i, frame) {
                            f.pos.y = v;
                        }
                    });
                }
            }
            // Multi-tile span and pixel scale keep a 1 floor (a 0 draws nothing).
            EditField::FrameW => {
                if let (Ok(v), Some(frame)) = (buffer.parse::<i32>(), self.current_frame(map)) {
                    self.modify_object(map, |map, i| {
                        if let Some(f) = frame_mut(map, i, frame) {
                            f.options.w = v.max(1);
                        }
                    });
                }
            }
            EditField::FrameH => {
                if let (Ok(v), Some(frame)) = (buffer.parse::<i32>(), self.current_frame(map)) {
                    self.modify_object(map, |map, i| {
                        if let Some(f) = frame_mut(map, i, frame) {
                            f.options.h = v.max(1);
                        }
                    });
                }
            }
            EditField::FrameScale => {
                if let (Ok(v), Some(frame)) = (buffer.parse::<i32>(), self.current_frame(map)) {
                    self.modify_object(map, |map, i| {
                        if let Some(f) = frame_mut(map, i, frame) {
                            f.options.scale = v.max(1);
                        }
                    });
                }
            }
            EditField::FramePaletteRot => {
                if let (Ok(v), Some(frame)) = (buffer.parse::<u8>(), self.current_frame(map)) {
                    self.modify_object(map, |map, i| {
                        if let Some(f) = frame_mut(map, i, frame) {
                            f.palette_rotate = v % 16;
                        }
                    });
                }
            }
            // Transparent / outline are `Option<u8>`: an empty buffer clears them,
            // a valid index sets them, a malformed non-empty buffer is ignored.
            EditField::FrameTransparent => {
                if let (Some(value), Some(frame)) =
                    (parse_optional_index(&buffer), self.current_frame(map))
                {
                    self.modify_object(map, |map, i| {
                        if let Some(f) = frame_mut(map, i, frame) {
                            f.options.transparent = value;
                        }
                    });
                }
            }
            EditField::FrameOutline => {
                if let (Some(value), Some(frame)) =
                    (parse_optional_index(&buffer), self.current_frame(map))
                {
                    self.modify_object(map, |map, i| {
                        if let Some(f) = frame_mut(map, i, frame) {
                            f.outline_colour = value;
                        }
                    });
                }
            }
            // Layer fields are handled by the early return above (they target the
            // store, not an object).
            EditField::LayerName
            | EditField::LayerOffX
            | EditField::LayerOffY
            | EditField::LayerRotate => {}
        }
    }

    pub(super) fn cycle(&mut self, map: &mut MapInfo, field: CycleField) {
        // Trigger lives on the MapObject (both kinds), so it cycles through
        // `modify_object`; the warp-only fields go through `modify_warp`.
        match field {
            CycleField::Trigger => self.modify_object(map, |map, i| {
                if let Some(object) = map.objects.get_mut(i) {
                    object.trigger = cycle_trigger(object.trigger);
                }
            }),
            CycleField::Removable => self.modify_object(map, |map, i| {
                if let Some(object) = map.objects.get_mut(i) {
                    object.removable = !object.removable;
                }
            }),
            CycleField::Flip => self.modify_warp(map, |w| w.flip = cycle_flip(&w.flip)),
            CycleField::Mode => self.modify_warp(map, |w| w.mode = cycle_mode(&w.mode)),
            CycleField::Sound => self.modify_warp(map, |w| w.sound = cycle_sound(&w.sound)),
            // Advance the interaction kind, rebuilding the effect in place and
            // carrying a sensible default param (piano's origin = the hitbox).
            CycleField::IntKind => self.modify_object(map, |map, i| {
                if let Some(object) = map.objects.get_mut(i)
                    && let ObjectEffect::Interact(interaction) = &object.effect
                {
                    let origin = Vec2::new(object.hitbox.x, object.hitbox.y);
                    let next = cycle_interaction(interaction, origin);
                    object.effect = ObjectEffect::Interact(next);
                }
            }),
            // The selected sprite frame's mirror / rotation, cycled in place.
            CycleField::FrameFlip => {
                if let Some(frame) = self.current_frame(map) {
                    self.modify_object(map, |map, i| {
                        if let Some(f) = frame_mut(map, i, frame) {
                            f.options.flip = cycle_anim_flip(&f.options.flip);
                        }
                    });
                }
            }
            CycleField::FrameRotate => {
                if let Some(frame) = self.current_frame(map) {
                    self.modify_object(map, |map, i| {
                        if let Some(f) = frame_mut(map, i, frame) {
                            f.options.rotate = cycle_rotate(&f.options.rotate);
                        }
                    });
                }
            }
            // Handled in `handle_panel` (it needs the map store) — see
            // [`cycle_warp_target`](Self::cycle_warp_target).
            CycleField::WarpTarget => {}
        }
    }

    /// Step the selected warp's destination through `[same-map] + the existing
    /// modern maps`, so a target is picked from real maps rather than typed (and
    /// can't become a dangling name). Recorded as one undo step.
    pub(super) fn cycle_warp_target(&mut self, map: &mut MapInfo, maps: &MapStore) {
        let names = self.modern_names(maps);
        self.modify_warp(map, move |w| {
            // Options are indexed 0 = same-map (None), then each name at +1.
            let current = match w.map.as_deref() {
                None => 0,
                Some(c) => names.iter().position(|n| n == c).map_or(0, |i| i + 1),
            };
            let next = (current + 1) % (names.len() + 1);
            w.map = (next > 0).then(|| names[next - 1].clone());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::null_console::NullConsole;

    /// Editing an object's hitbox x/y/w/h commits to the box and is undoable;
    /// w/h keep a 1px floor.
    #[test]
    fn object_hitbox_fields_edit_and_undo() {
        let mut maps = MapStore::default();
        let mut map = MapInfo {
            objects: vec![MapObject::dialogue(Hitbox::new(10, 10, 16, 16), "k")],
            ..MapInfo::default()
        };
        let mut v = MapViewer {
            selected: Some(0),
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
                target: 0,
            });
            v.commit_edit(map, maps);
            v.stop_editing();
        };
        edit(&mut v, &mut map, &mut maps, EditField::HitX, "40");
        edit(&mut v, &mut map, &mut maps, EditField::HitY, "24");
        edit(&mut v, &mut map, &mut maps, EditField::HitW, "8");
        edit(&mut v, &mut map, &mut maps, EditField::HitH, "0"); // floored to 1
        let hb = map.objects[0].hitbox;
        assert_eq!((hb.x, hb.y, hb.w, hb.h), (40, 24, 8, 1));

        // Each edit is one undo step; undoing the last reverts just the height.
        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(map.objects[0].hitbox.h, 16);
        assert_eq!(map.objects[0].hitbox.w, 8);
    }

    /// The object panel's animated-sprite controls: add a frame from the brush,
    /// edit its tile / duration (duration floored to 1), add and auto-select a
    /// second frame, and undo/remove — each an undo step, with a stale frame
    /// index healed on use.
    #[test]
    fn sprite_frames_add_edit_remove_and_undo() {
        let mut maps = MapStore::default();
        let mut map = MapInfo {
            objects: vec![MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "k")],
            ..MapInfo::default()
        };
        let mut v = MapViewer {
            selected: Some(0),
            selected_tile: 12,
            ..Default::default()
        };
        let frames = |map: &MapInfo| map.objects[0].sprite.clone().unwrap_or_default();

        assert!(map.objects[0].sprite.is_none(), "starts spriteless");

        // +frm seeds a frame from the brush tile (12) and selects it.
        v.add_sprite_frame(&mut map);
        assert_eq!(frames(&map).len(), 1);
        assert_eq!(frames(&map)[0].spr_id, 12);
        assert_eq!(v.sprite_frame, 0);

        // Edit the frame's tile and duration via the text fields.
        let edit = |v: &mut MapViewer,
                    map: &mut MapInfo,
                    maps: &mut MapStore,
                    field: EditField,
                    text: &str| {
            v.editing = Some(TextEdit {
                field,
                buffer: TextField::new(text),
                target: 0,
            });
            v.commit_edit(map, maps);
            v.stop_editing();
        };
        edit(&mut v, &mut map, &mut maps, EditField::FrameTile, "30");
        edit(&mut v, &mut map, &mut maps, EditField::FrameDuration, "5");
        edit(&mut v, &mut map, &mut maps, EditField::FrameDuration, "0"); // floored to 1
        assert_eq!(frames(&map)[0].spr_id, 30);
        assert_eq!(frames(&map)[0].duration, 1, "duration floored to 1");

        // A second frame from a different brush tile, auto-selected.
        v.selected_tile = 7;
        v.add_sprite_frame(&mut map);
        assert_eq!(frames(&map).len(), 2);
        assert_eq!(v.sprite_frame, 1);
        assert_eq!(frames(&map)[1].spr_id, 7);

        // Undo the second add (leaves `sprite_frame` stale at 1).
        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(frames(&map).len(), 1, "second frame undone");

        // -frm heals the stale index to the last frame and removes it; the whole
        // sprite drops when the last frame goes.
        v.del_sprite_frame(&mut map);
        assert!(
            map.objects[0].sprite.is_none(),
            "removing the last frame drops the sprite"
        );

        // Undo the removal.
        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(frames(&map).len(), 1, "removal undone");
        assert_eq!(frames(&map)[0].spr_id, 30);
    }

    /// Sprite frames reorder by button (^/v) and by the drag-commit path, each one
    /// undo step (an object `Modify`), with the frame selection following the move.
    #[test]
    fn sprite_frames_reorder_and_undo() {
        let mut maps = MapStore::default();
        let mut map = MapInfo {
            objects: vec![MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "k")],
            ..MapInfo::default()
        };
        let mut v = MapViewer {
            selected: Some(0),
            ..Default::default()
        };
        let tiles = |map: &MapInfo| {
            map.objects[0]
                .sprite
                .clone()
                .unwrap_or_default()
                .iter()
                .map(|f| f.spr_id)
                .collect::<Vec<_>>()
        };
        // Three frames: tiles 10, 20, 30.
        for t in [10u16, 20, 30] {
            v.selected_tile = t as usize;
            v.add_sprite_frame(&mut map);
        }
        assert_eq!(tiles(&map), vec![10, 20, 30]);
        assert_eq!(v.sprite_frame, 2, "the last add stays selected");

        // ^ moves the selected frame (30, index 2) one earlier; selection follows.
        v.move_sprite_frame(&mut map, true);
        assert_eq!(tiles(&map), vec![10, 30, 20]);
        assert_eq!(v.sprite_frame, 1);
        // ^ at the top edge would underflow — it's a no-op.
        v.sprite_frame = 0;
        v.move_sprite_frame(&mut map, true);
        assert_eq!(tiles(&map), vec![10, 30, 20], "no-op past the top");

        // The drag-commit path moves frame 0 to index 2 (the layers between slide).
        v.reorder_sprite_frame_to(&mut map, 0, 2);
        assert_eq!(tiles(&map), vec![30, 20, 10]);
        assert_eq!(v.sprite_frame, 2, "selection follows the dropped frame");

        // Each reorder is a single undo step.
        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(tiles(&map), vec![10, 30, 20], "drag move undone");
        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(tiles(&map), vec![10, 20, 30], "button move undone");
        v.redo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(tiles(&map), vec![10, 30, 20], "button move redone");
    }

    /// The Interacts tab's `take` toggle flips [`MapObject::removable`] through
    /// the undo machinery (no ⇄ yes), one undo step per toggle.
    #[test]
    fn cycle_toggles_removable() {
        let mut maps = MapStore::default();
        let mut map = MapInfo {
            objects: vec![MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "k")],
            ..MapInfo::default()
        };
        let mut v = MapViewer {
            selected: Some(0),
            ..Default::default()
        };
        assert!(!map.objects[0].removable);
        v.cycle(&mut map, CycleField::Removable);
        assert!(map.objects[0].removable, "toggled on");
        v.cycle(&mut map, CycleField::Removable);
        assert!(!map.objects[0].removable, "toggled off");
        // Each toggle is one undo step.
        v.cycle(&mut map, CycleField::Removable);
        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert!(!map.objects[0].removable, "toggle undone");
        v.redo(&mut NullConsole::new(), &mut map, &mut maps);
        assert!(map.objects[0].removable, "toggle redone");
    }

    /// `is_object_taken` reads the cached save snapshot: a removable object whose
    /// `<map>#<id>` key is in `taken` reads taken (badged + skipped-preview);
    /// a not-yet-collected sibling, or a non-removable object, never does.
    #[test]
    fn is_object_taken_reads_cached_taken_set() {
        let map = MapInfo {
            source: "town".to_string(),
            objects: vec![
                MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "k")
                    .with_id(Some(5))
                    .with_removable(true),
                MapObject::dialogue(Hitbox::new(8, 0, 8, 8), "s")
                    .with_id(Some(6))
                    .with_removable(true),
                MapObject::dialogue(Hitbox::new(16, 0, 8, 8), "n").with_id(Some(7)),
            ],
            ..MapInfo::default()
        };
        let mut v = MapViewer::default();
        v.taken = BTreeSet::from([SaveData::taken_key("town", 5)]);
        assert!(v.is_object_taken(&map, &map.objects[0]), "id 5 is collected");
        assert!(
            !v.is_object_taken(&map, &map.objects[1]),
            "id 6 is not in the taken set"
        );
        assert!(
            !v.is_object_taken(&map, &map.objects[2]),
            "a non-removable object is never taken"
        );
    }

    /// The Interacts tab's un-take / re-take toggle parks the selected pickup's
    /// `<map>#<id>` key for the host to flip in `save.taken` (the editor never
    /// holds `&mut SaveData`) — distinct from the `removable` authoring toggle. An
    /// id-less object can't be recorded, so it parks nothing.
    #[test]
    fn taken_toggle_parks_selected_pickup_key() {
        let mut console = crate::platform::test_console::TestConsole::new();
        let mut input = crate::platform::EggInput::new();
        input.mouse.left = [true, false]; // a just-pressed click edge
        let mut maps = MapStore::default();
        let mut map = MapInfo {
            source: "town".to_string(),
            objects: vec![
                MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "k")
                    .with_id(Some(5))
                    .with_removable(true),
            ],
            ..MapInfo::default()
        };
        let mut v = MapViewer {
            selected: Some(0),
            ..Default::default()
        };
        v.handle_panel(
            &mut console,
            &input,
            &mut map,
            &mut maps,
            usize::MAX,
            EditorKey::TakenToggle,
            Vec2::new(0, 0),
        );
        assert_eq!(
            v.pending_taken_toggle.as_deref(),
            Some("town#5"),
            "the toggle parks the selected pickup's key"
        );

        // An id-less object has no durable key to record under: park nothing.
        v.pending_taken_toggle = None;
        map.objects[0].id = None;
        v.handle_panel(
            &mut console,
            &input,
            &mut map,
            &mut maps,
            usize::MAX,
            EditorKey::TakenToggle,
            Vec2::new(0, 0),
        );
        assert_eq!(v.pending_taken_toggle, None, "id-less object parks nothing");
    }

    /// The feature-complete frame fields: offset / size / scale / palette-rotate
    /// (size & scale floored to 1, palette mod-16), flip + rotate cycles, and the
    /// `Option<u8>` transparent / outline (a number sets it, an empty buffer
    /// clears it to `None`). Each routes through the undo machinery.
    #[test]
    fn sprite_frame_full_field_edits() {
        let mut maps = MapStore::default();
        let mut map = MapInfo {
            objects: vec![MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "k")],
            ..MapInfo::default()
        };
        let mut v = MapViewer {
            selected: Some(0),
            selected_tile: 1,
            ..Default::default()
        };
        v.add_sprite_frame(&mut map);
        let edit = |v: &mut MapViewer,
                    map: &mut MapInfo,
                    maps: &mut MapStore,
                    field: EditField,
                    text: &str| {
            v.editing = Some(TextEdit {
                field,
                buffer: TextField::new(text),
                target: 0,
            });
            v.commit_edit(map, maps);
            v.stop_editing();
        };

        edit(&mut v, &mut map, &mut maps, EditField::FrameOffX, "3");
        edit(&mut v, &mut map, &mut maps, EditField::FrameOffY, "-4");
        edit(&mut v, &mut map, &mut maps, EditField::FrameW, "2");
        edit(&mut v, &mut map, &mut maps, EditField::FrameH, "0"); // floored to 1
        edit(&mut v, &mut map, &mut maps, EditField::FrameScale, "3");
        edit(
            &mut v,
            &mut map,
            &mut maps,
            EditField::FramePaletteRot,
            "20",
        ); // mod 16 -> 4
        {
            let f = &map.objects[0].sprite.as_ref().unwrap()[0];
            assert_eq!((f.pos.x, f.pos.y), (3, -4));
            assert_eq!((f.options.w, f.options.h), (2, 1));
            assert_eq!(f.options.scale, 3);
            assert_eq!(f.palette_rotate, 4);
        }

        // Flip cycles none -> horiz; rotate 0 -> 90.
        v.cycle(&mut map, CycleField::FrameFlip);
        v.cycle(&mut map, CycleField::FrameRotate);
        {
            let f = &map.objects[0].sprite.as_ref().unwrap()[0];
            assert_eq!(f.options.flip, Flip::Horizontal);
            assert_eq!(f.options.rotate, Rotate::By90);
        }

        // Outline / transparent: a number sets `Some`, an empty buffer clears to
        // `None` (transparent starts at the default `Some(0)`).
        edit(&mut v, &mut map, &mut maps, EditField::FrameOutline, "7");
        edit(&mut v, &mut map, &mut maps, EditField::FrameTransparent, "");
        {
            let f = &map.objects[0].sprite.as_ref().unwrap()[0];
            assert_eq!(f.outline_colour, Some(7));
            assert_eq!(f.options.transparent, None);
        }
        // The clear is one undo step.
        v.undo(&mut NullConsole::new(), &mut map, &mut maps);
        assert_eq!(
            map.objects[0].sprite.as_ref().unwrap()[0]
                .options
                .transparent,
            Some(0),
            "transparent restored"
        );
    }

    /// A box-selected palette brush grabs a multi-tile block: its top-left tile is
    /// the frame's `spr_id` and the box becomes the sprite's `w`×`h` footprint, on
    /// both `+frm` (add) and `set from brush` (re-grab). A 1×1 brush grabs 1×1.
    #[test]
    fn sprite_frame_grabs_multi_tile_brush() {
        let mut map = MapInfo {
            objects: vec![MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "k")],
            ..MapInfo::default()
        };
        let mut v = MapViewer {
            selected: Some(0),
            ..Default::default()
        };
        let frame = |map: &MapInfo| map.objects[0].sprite.as_ref().unwrap()[0].clone();

        // A 2-wide × 3-tall box (cols 7..=8, rows 2..=4) → top-left tile + 2×3.
        v.set_brush_box(7, 2, 8, 4);
        let top_left = v.selected_tile as u16;
        v.add_sprite_frame(&mut map);
        assert_eq!(
            frame(&map).spr_id,
            top_left,
            "+frm grabs the box's top-left tile"
        );
        assert_eq!(
            (frame(&map).options.w, frame(&map).options.h),
            (2, 3),
            "+frm grabs the box size"
        );

        // Re-grab from a 1×1 brush: spr_id + footprint collapse back to one tile,
        // leaving the rest of the frame's render settings alone.
        v.set_brush_box(3, 1, 3, 1);
        v.set_frame_from_brush(&mut map);
        assert_eq!(frame(&map).spr_id, v.selected_tile as u16);
        assert_eq!(
            (frame(&map).options.w, frame(&map).options.h),
            (1, 1),
            "set-from-brush re-grabs 1×1"
        );
    }

    /// The warp-target picker steps through `[same-map] + existing maps` and
    /// wraps, recording each step for undo.
    #[test]
    fn warp_target_cycles_through_maps() {
        use crate::data::tiled::TiledMap;
        let mut maps = MapStore::default();
        maps.insert("a", TiledMap::blank_modern(4, 4));
        maps.insert("b", TiledMap::blank_modern(4, 4));
        let mut map = MapInfo {
            objects: vec![MapObject::warp(
                Hitbox::new(0, 0, 8, 8),
                Warp::new(None, Vec2::new(0, 0)),
            )],
            ..MapInfo::default()
        };
        let mut v = MapViewer {
            selected: Some(0),
            ..Default::default()
        };
        let target = |map: &MapInfo| match &map.objects[0].effect {
            ObjectEffect::Warp(w) => w.map.clone(),
            _ => panic!("the object is a warp"),
        };

        assert_eq!(target(&map), None); // same-map
        v.cycle_warp_target(&mut map, &maps);
        assert_eq!(target(&map).as_deref(), Some("a"));
        v.cycle_warp_target(&mut map, &maps);
        assert_eq!(target(&map).as_deref(), Some("b"));
        v.cycle_warp_target(&mut map, &maps); // wraps back to same-map
        assert_eq!(target(&map), None);
        assert!(v.history.can_undo(), "each pick is an undo step");
    }

    /// The text field's caret: arrow motion, insert/delete at the cursor, word
    /// motion over whitespace, and ctrl-backspace clearing the buffer. `display`
    /// shows the caret as `_` at its position.
    #[test]
    fn text_field_cursor_editing() {
        let mut f = TextField::new("cat");
        assert_eq!(f.display(), "cat_", "caret starts at the end");
        f.apply(TextOp::Left);
        f.apply(TextOp::Left);
        assert_eq!(f.display(), "c_at");
        f.apply(TextOp::Push('X'));
        assert_eq!(f.text(), "cXat");
        assert_eq!(f.display(), "cX_at", "insert lands at the caret");
        f.apply(TextOp::Pop);
        assert_eq!(f.text(), "cat", "backspace deletes before the caret");
        assert_eq!(f.display(), "c_at");

        // Word motion skips a run of whitespace then a run of word characters.
        let mut g = TextField::new("foo bar baz");
        g.apply(TextOp::WordLeft);
        assert_eq!(g.display(), "foo bar _baz");
        g.apply(TextOp::WordLeft);
        assert_eq!(g.display(), "foo _bar baz");
        g.apply(TextOp::WordRight);
        assert_eq!(g.display(), "foo bar_ baz");
        // Ctrl+Backspace deletes the word before the cursor.
        g.apply(TextOp::DeleteWordBack);
        assert_eq!((g.text(), g.display().as_str()), ("foo  baz", "foo _ baz"));
    }

    /// Cycling an interaction reaches every authorable kind — the GUI's way to
    /// place Func interactions (toggle_dog / piano / note / add_creatures /
    /// give_item).
    #[test]
    fn interaction_kind_cycles_through_func_variants() {
        let o = Vec2::new(5, 7);
        let mut i = Interaction::None;
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::Dialogue(_)));
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::Func(InteractFn::ToggleDog)));
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::Func(InteractFn::Piano(p)) if p == o));
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::Func(InteractFn::Note(0))));
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::Func(InteractFn::AddCreatures(0))));
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::Func(InteractFn::GiveItem(ref k)) if k.is_empty()));
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::Cutscene(_)));
        i = cycle_interaction(&i, o);
        assert!(matches!(i, Interaction::None));
        assert_eq!(
            interaction_kind_label(&Interaction::Func(InteractFn::Note(3))),
            "note"
        );
        assert_eq!(interaction_kind_label(&Interaction::None), "none");
        assert_eq!(
            interaction_kind_label(&Interaction::Cutscene(String::new())),
            "scene"
        );
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
}

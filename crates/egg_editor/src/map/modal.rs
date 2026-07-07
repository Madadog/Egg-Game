//! The editor's fullscreen modal sessions: warp-destination placement, the
//! live path recorder, and the scene picker — each captures all input until
//! confirmed or cancelled.

use super::*;

impl MapViewer {
    /// Queue the selected warp's destination map to open in the editor, centred
    /// on its landing point (the host drains [`pending_open`](Self::pending_open)
    /// and frames the landing). A no-op if no warp is selected or it's a same-map
    /// warp (`map = None`) — there's nothing to open.
    pub(super) fn open_selected_warp_dest(&mut self, objects: &[MapObject]) {
        if let Some(ObjectEffect::Warp(w)) =
            self.selected.and_then(|i| objects.get(i)).map(|o| &o.effect)
            && let Some(dest) = w.map.clone()
        {
            self.pending_open = Some((dest, Some(w.to)));
        }
    }

    /// Open the fullscreen warp-destination placement overlay for the selected
    /// warp: render its destination map 1:1, centred on the current landing, so
    /// you pan and click to place the landing against the real geometry. A no-op
    /// if no warp is selected or its destination map can't be resolved.
    pub(super) fn open_warp_preview(&mut self, map: &MapInfo, maps: &MapStore) {
        let Some(i) = self.selected else { return };
        let Some(ObjectEffect::Warp(w)) = map.objects.get(i).map(|o| &o.effect) else {
            return;
        };
        // A same-map warp previews the current map; otherwise the named target.
        let dest = w.map.clone().unwrap_or_else(|| map.source.clone());
        let Some(tiled) = maps.get(&dest) else { return };
        let (fw, fh) = ((tiled.width as i16 * 8).max(1), (tiled.height as i16 * 8).max(1));
        let (sw, sh) = self.dock.solved.screen;
        // Centre on the landing, then clamp so the map stays framed.
        let camera = Vec2::new(
            clamp_camera(w.to.x - sw / 2, fw, sw),
            clamp_camera(w.to.y - sh / 2, fh, sh),
        );
        self.warp_preview = Some(WarpPreview {
            object: i,
            dest,
            point: w.to,
            camera,
            armed: false,
            pan_anchor: None,
        });
    }

    /// Step the fullscreen placement overlay (fully modal). Arrows or right-drag
    /// pan; left-click/drag sets the landing; Z/Enter confirm, X/Esc cancel. The
    /// warp is untouched until confirm. Self-closes if the warp or its destination
    /// map vanishes mid-session.
    pub(super) fn step_warp_preview(
        &mut self,
        input: &EggInput,
        map: &mut MapInfo,
        maps: &mut MapStore,
        screen: (f32, f32),
    ) {
        let Some(mut pv) = self.warp_preview.clone() else { return };
        // Self-heal: still a warp, and its destination map still resolves.
        let still_warp = matches!(
            map.objects.get(pv.object).map(|o| &o.effect),
            Some(ObjectEffect::Warp(_))
        );
        let dims = maps
            .get(&pv.dest)
            .filter(|_| still_warp)
            .map(|t| ((t.width as i16 * 8).max(1), (t.height as i16 * 8).max(1)));
        let Some((fw, fh)) = dims else {
            self.warp_preview = None;
            return;
        };
        let (sw, sh) = (screen.0 as i16, screen.1 as i16);

        // Z/Enter commit the working point; X/Esc discard (warp untouched).
        if input.keyp(ScanCode::Escape) || input.keyp(ScanCode::X) {
            self.warp_preview = None;
            return;
        }
        if input.keyp(ScanCode::Return) || input.keyp(ScanCode::Z) {
            self.commit_warp_preview(&pv, map);
            return;
        }

        let mouse = input.mouse;
        let cursor = mouse.pos();
        // The Confirm/Cancel buttons win a click over the map beneath them.
        if just_pressed(mouse.left) {
            match self.build_warp_preview_ui().hit_at(0, 0, cursor) {
                Some(EditorKey::WarpPreviewOk) => {
                    self.commit_warp_preview(&pv, map);
                    return;
                }
                Some(EditorKey::WarpPreviewCancel) => {
                    self.warp_preview = None;
                    return;
                }
                _ => {}
            }
        }

        // Arrow keys pan (held = continuous; Shift = faster).
        let step = if input.key(ScanCode::Shift) {
            WARP_CAM_PAN_FAST
        } else {
            WARP_CAM_PAN
        };
        if input.key(ScanCode::Left) { pv.camera.x -= step; }
        if input.key(ScanCode::Right) { pv.camera.x += step; }
        if input.key(ScanCode::Up) { pv.camera.y -= step; }
        if input.key(ScanCode::Down) { pv.camera.y += step; }
        // Right-drag grabs and pans the map (left stays for placement).
        if just_pressed(mouse.right) {
            pv.pan_anchor = Some((cursor, pv.camera));
        }
        if let Some((anchor_cursor, anchor_cam)) = pv.pan_anchor {
            if mouse.right[0] {
                pv.camera.x = anchor_cam.x + (anchor_cursor.x - cursor.x);
                pv.camera.y = anchor_cam.y + (anchor_cursor.y - cursor.y);
            } else {
                pv.pan_anchor = None;
            }
        }
        pv.camera.x = clamp_camera(pv.camera.x, fw, sw);
        pv.camera.y = clamp_camera(pv.camera.y, fh, sh);

        // The click that opened the overlay stays held into these first frames;
        // arm placement only once it has released, so the opening click can't drop
        // the landing under the "place" button.
        pv.armed |= !mouse.left[0];
        // Left-click/drag on the map sets the landing: screen px + camera = map px
        // (the inverse of the 1:1 render), clamped to bounds. Not while pan-dragging.
        if pv.armed && mouse.left[0] && pv.pan_anchor.is_none() {
            pv.point.x = (cursor.x + pv.camera.x).clamp(0, fw - 1);
            pv.point.y = (cursor.y + pv.camera.y).clamp(0, fh - 1);
        }
        self.warp_preview = Some(pv);
    }

    /// Commit the placement: write the working landing to the warp (one undo step)
    /// and close the overlay.
    pub(super) fn commit_warp_preview(&mut self, pv: &WarpPreview, map: &mut MapInfo) {
        let point = pv.point;
        self.modify_warp(map, |w| w.to = point);
        self.warp_preview = None;
    }

    /// The overlay chrome: a top `WARP PREVIEW` banner and a bottom bar (hint +
    /// confirm/cancel buttons). Built once and shared by the hit pass (in
    /// [`step_warp_preview`]) and the draw pass so the buttons can't disagree.
    pub(super) fn build_warp_preview_ui(&self) -> Ui<EditorKey> {
        let (sw, sh) = self.dock.solved.screen;
        let mut b = UiBuilder::new();
        let banner = b
            .text("WARP PREVIEW")
            .small(true)
            .center()
            .color(0)
            .full_width(8.0)
            .fill(8)
            .id();
        let hint = b
            .text("drag = place   arrows/right-drag = pan   Z = ok   X = cancel")
            .small(true)
            .color(13)
            .grow(1.0)
            .id();
        let ok = b
            .text("confirm")
            .small(true)
            .center()
            .color(0)
            .full_width(7.0)
            .outlined(11, 11)
            .key(EditorKey::WarpPreviewOk)
            .id();
        let cancel = b
            .text("cancel")
            .small(true)
            .center()
            .color(0)
            .full_width(7.0)
            .outlined(8, 8)
            .key(EditorKey::WarpPreviewCancel)
            .id();
        let bottom = b.row(2.0, [hint, ok, cancel]).pad(1.0).id();
        let spacer = b.spacer(0.0).grow(1.0).id();
        let root = b
            .column(0.0, [banner, spacer, bottom])
            .size(sw as f32, sh as f32)
            .id();
        b.finish(root, (sw as f32, sh as f32))
    }

    /// Draw the fullscreen placement overlay: the destination map rendered 1:1 (the
    /// live-world layer path) at the pan camera, its objects as static frame-0
    /// sprites + kind-coloured hitbox markers, the landing avatar + crosshair, and
    /// the chrome on top.
    pub(super) fn draw_warp_preview_fullscreen(
        &self,
        draw_state: &mut DrawState,
        font: &Font,
        maps: &MapStore,
    ) {
        let Some(pv) = self.warp_preview.as_ref() else { return };
        let cam = pv.camera;
        let Some(tiled) = maps.get(&pv.dest) else { return };
        // `info` borrows the sprite sheet, so resolve it before the mutable draws.
        let Some(info) = map_by_name(&draw_state.indexed_sprites, &pv.dest, maps) else {
            return;
        };

        // Live-world draw order at native scale, offset by the camera.
        let bg = draw_state.resolve(tiled.bg_colour().unwrap_or_default());
        draw_state.rgba(LayerId::BG).fill(bg);
        info.draw_bg_indexed(draw_state, LayerId::BG, tiled, cam, false);
        // Static preview: sprite-plane layers draw flat (no live entities here).
        info.draw_sprite_indexed(draw_state, LayerId::BG, tiled, cam, false);

        // The destination's objects, so you place against real geometry: each
        // object's static frame-0 sprite (if any) + a kind-coloured hitbox marker
        // (warps 12, interactables 14).
        for object in &info.objects {
            if let Some(frame) = object.sprite.as_ref().and_then(|f| f.first()) {
                DrawParams::new(
                    frame.spr_id.into(),
                    i32::from(frame.pos.x) + i32::from(object.hitbox.x) - i32::from(cam.x),
                    i32::from(frame.pos.y) + i32::from(object.hitbox.y) - i32::from(cam.y),
                    frame.options.clone(),
                    frame.outline_colour,
                    frame.palette_rotate,
                )
                .draw_to(draw_state, LayerId::BG);
            }
            let marker = if matches!(object.effect, ObjectEffect::Warp(_)) { 12 } else { 14 };
            draw_state.stroke_hitbox(
                LayerId::BG,
                object.hitbox.offset_xy(-cam.x, -cam.y),
                marker,
            );
        }

        // The landing avatar via the live sprite path, then fg layers, then the
        // avatar's collision hitbox.
        let player = Shell::default().with_pos(pv.point);
        player.draw_params(cam).draw_to(draw_state, LayerId::BG);
        info.draw_fg_indexed(draw_state, LayerId::BG, tiled, cam, false);
        draw_state.stroke_hitbox(
            LayerId::BG,
            player.hitbox().offset_xy(-cam.x, -cam.y),
            11,
        );

        // Crosshair on the exact landing pixel.
        let mark = draw_state.colour(8);
        let (cx, cy) = (i32::from(pv.point.x - cam.x), i32::from(pv.point.y - cam.y));
        let arm = 4;
        let layer = draw_state.rgba(LayerId::BG);
        layer.line(cx - arm, cy, cx + arm, cy, mark);
        layer.line(cx, cy - arm, cx, cy + arm, mark);

        self.build_warp_preview_ui()
            .draw_at(0, 0, draw_state, font, LayerId::BG);
    }

    /// Open the live path recorder over the current map: a puppet (player sprites)
    /// at the centre of the current view, with an empty recording. The camera
    /// tracks it as the dpad drives.
    /// Open the scene picker over the editor — a snapshot of the engine-pushed
    /// cutscene names, highlighted from the top. Empty is fine: the list shows a
    /// hint, so the modal can't trap you with nothing to pick.
    pub(super) fn open_scene_picker(&mut self) {
        self.scene_picker = Some(ScenePicker {
            names: self.scene_names.clone(),
            selected: 0,
        });
    }

    /// Step the modal scene picker: up/down move the highlight, Enter replays the
    /// chosen scene (parks a [`ScrubRequest`] the engine opens), Esc/X cancels.
    pub(super) fn step_scene_picker(&mut self, input: &EggInput) {
        let Some(mut picker) = self.scene_picker.clone() else {
            return;
        };
        if input.keyp(ScanCode::Escape) || input.keyp(ScanCode::X) {
            self.scene_picker = None;
            return;
        }
        let len = picker.names.len();
        if len > 0 {
            if input.keyp(ScanCode::Up) {
                picker.selected = picker.selected.saturating_sub(1);
            }
            if input.keyp(ScanCode::Down) {
                picker.selected = (picker.selected + 1).min(len - 1);
            }
            if input.keyp(ScanCode::Return) {
                let name = picker.names[picker.selected].clone();
                self.pending_scrub = Some(ScrubRequest::ByName(name));
                self.scene_picker = None;
                return;
            }
        }
        self.scene_picker = Some(picker);
    }

    /// Draw the fullscreen scene picker: a dark backdrop, the cutscene list with
    /// the highlighted row, and the key hints.
    pub(super) fn draw_scene_picker_fullscreen(&self, draw_state: &mut DrawState, font: &Font) {
        let Some(picker) = self.scene_picker.as_ref() else {
            return;
        };
        let (_, sh) = self.dock.solved.screen;
        let backdrop = draw_state.colour(0);
        draw_state.rgba(LayerId::BG).fill(backdrop);

        let title = draw_state.colour(11);
        let row = draw_state.colour(13);
        let canvas = draw_state.rgba(LayerId::BG);
        let opts = PrintOptions::default();
        print_to_with_font(font, canvas, "SCRUB WHICH SCENE?", 6, 6, title, opts.clone());
        if picker.names.is_empty() {
            print_to_with_font(
                font,
                canvas,
                "(no saved cutscenes - record a path with R)",
                6,
                20,
                row,
                opts.clone(),
            );
        } else {
            for (i, name) in picker.names.iter().enumerate() {
                let y = 20 + i as i32 * 8;
                let (mark, col) = if i == picker.selected {
                    ("> ", title)
                } else {
                    ("  ", row)
                };
                print_to_with_font(font, canvas, &format!("{mark}{name}"), 6, y, col, opts.clone());
            }
        }
        print_to_with_font(
            font,
            canvas,
            "[up/down] choose   [enter] scrub   [X] cancel",
            6,
            sh as i32 - 10,
            row,
            opts,
        );
    }

    pub(super) fn open_path_recorder(&mut self, map: &MapInfo, maps: &MapStore, camera_pos: Vec2) {
        let (sw, sh) = self.dock.solved.screen;
        let view_centre = Vec2::new(camera_pos.x + sw / 2, camera_pos.y + sh / 2);
        // The host pushes the live actors each focused frame (the primary and
        // every extra view); fall back to the player at the view centre if the
        // list ever arrives empty (belt-and-braces).
        let actors = if self.recorder_actors.is_empty() {
            vec![("player".to_string(), view_centre)]
        } else {
            self.recorder_actors.clone()
        };
        // Seed the puppet on the first actor so clicked waypoints land relative to
        // where it really is; centre the camera on it (clamped to the map).
        let start = actors[0].1;
        let (fw, fh) = Self::map_px_dims(map, maps, (sw, sh));
        let camera = Vec2::new(
            clamp_camera(start.x - sw / 2, fw, sw),
            clamp_camera(start.y - sh / 2, fh, sh),
        );
        self.path_recorder = Some(PathRecorder {
            puppet: Shell::default().with_pos(start),
            runs: Vec::new(),
            instructions: Vec::new(),
            path: vec![start],
            camera,
            noclip: false,
            name: format!("{}_path", map.source),
            actors,
            actor: 0,
            naming: None,
            status: None,
        });
    }

    /// Map pixel dimensions (clamped ≥1), for camera clamping.
    pub(super) fn map_px_dims(map: &MapInfo, maps: &MapStore, fallback: (i16, i16)) -> (i16, i16) {
        maps.get(&map.source)
            .map(|t| ((t.width as i16 * 8).max(1), (t.height as i16 * 8).max(1)))
            .unwrap_or(fallback)
    }

    /// Step the recorder (fully modal): the dpad drives the puppet via
    /// `Shell::walk` (collision unless `noclip`), capturing the **commanded**
    /// heading each frame as an RLE run; the camera follows. `N` toggles noclip;
    /// Z/Enter saves the path as a cutscene, X/Esc discards.
    pub(super) fn step_path_recorder(
        &mut self,
        system: &mut impl ConsoleApi,
        input: &EggInput,
        map: &mut MapInfo,
        maps: &mut MapStore,
        screen: (f32, f32),
    ) {
        let Some(mut pr) = self.path_recorder.clone() else { return };
        let (sw, sh) = (screen.0 as i16, screen.1 as i16);
        let (fw, fh) = Self::map_px_dims(map, maps, (sw, sh));

        // The name field, while focused, swallows every key — so typing a name
        // can't drive the puppet, toggle noclip, or trip the save/cancel keys.
        if pr.naming.is_some() {
            self.step_recorder_naming(&mut pr, input);
            self.path_recorder = Some(pr);
            return;
        }

        if input.keyp(ScanCode::Escape) || input.keyp(ScanCode::X) {
            self.path_recorder = None;
            return;
        }
        if input.keyp(ScanCode::Return) || input.keyp(ScanCode::Z) {
            self.path_recorder = Some(pr);
            self.commit_path_recorder(system);
            return;
        }
        // `Tab` cycles which actor is recorded (player / companions / map creatures).
        if input.keyp(ScanCode::Tab) {
            pr.select_actor(pr.actor + 1);
            pr.follow_camera(sw, sh, fw, fh);
            self.path_recorder = Some(pr);
            return;
        }
        // `N` toggles noclip — but only before any movement is recorded, since the
        // flag is saved once for the whole replay (toggling mid-record would make
        // the replay re-collide segments differently than they were driven, and it
        // also governs whether clicked waypoints emit `walk` or `noclip`).
        if input.keyp(ScanCode::N) && !pr.has_recorded() {
            pr.noclip = !pr.noclip;
        }

        let mouse = input.mouse;
        let cursor = mouse.pos();
        if just_pressed(mouse.left) {
            match self.build_path_recorder_ui().hit_at(0, 0, cursor) {
                Some(EditorKey::PathRecOk) => {
                    self.path_recorder = Some(pr);
                    self.commit_path_recorder(system);
                    return;
                }
                Some(EditorKey::PathRecCancel) => {
                    self.path_recorder = None;
                    return;
                }
                Some(EditorKey::PathRecName) => {
                    // Focus the name field, primed with the current name.
                    pr.naming = Some(TextField::new(pr.name.clone()));
                    pr.status = None;
                    self.path_recorder = Some(pr);
                    return;
                }
                Some(EditorKey::PathRecActor) => {
                    pr.select_actor(pr.actor + 1);
                    pr.follow_camera(sw, sh, fw, fh);
                    self.path_recorder = Some(pr);
                    return;
                }
                Some(EditorKey::PathRecCanvas) => {
                    // A click on the map area drops a `MoveToPoint` waypoint at the
                    // clicked map pixel — authoring a path without walking it.
                    let point = Vec2::new(
                        (cursor.x + pr.camera.x).clamp(0, fw - 1),
                        (cursor.y + pr.camera.y).clamp(0, fh - 1),
                    );
                    pr.place_waypoint(point);
                    pr.follow_camera(sw, sh, fw, fh);
                    self.path_recorder = Some(pr);
                    return;
                }
                _ => {}
            }
        }

        // Drive the puppet one frame; record the COMMANDED heading (collision is
        // re-applied at replay, so storing the clamped delta would double-clip).
        // Read raw arrow keys — the reliable editor-input source (the warp-preview
        // pan reads the same): the host only folds arrows onto the controller dpad
        // for the player-driving *primary* window, so a focused F8 view's dpad is
        // empty. Sum with the gamepad dpad and clamp so either source drives a step
        // (and holding an arrow on the primary, where both fire, isn't doubled).
        let kx = input.key(ScanCode::Right) as i16 - input.key(ScanCode::Left) as i16;
        let ky = input.key(ScanCode::Down) as i16 - input.key(ScanCode::Up) as i16;
        let pad = input.controller();
        let (pdx, pdy) = dpad_delta(&pad, pressed);
        let dx0 = (kx + pdx).clamp(-1, 1);
        let dy0 = (ky + pdy).clamp(-1, 1);
        let tiles = maps.get(&map.source);
        let (mdx, mdy) = pr.puppet.walk(system, dx0, dy0, pr.noclip, map, tiles);
        pr.puppet.apply_motion(mdx, mdy);
        // Keep the puppet on the map so it can't be driven (noclip) off-frame and
        // lost — the recorded heading is the *commanded* one, unaffected by this.
        pr.puppet.pos.x = pr.puppet.pos.x.clamp(0, fw - 1);
        pr.puppet.pos.y = pr.puppet.pos.y.clamp(0, fh - 1);
        let dir = (dx0 as i8, dy0 as i8);
        match pr.runs.last_mut() {
            Some((d, n)) if *d == dir => *n = n.saturating_add(1),
            _ => pr.runs.push((dir, 1)),
        }
        pr.path.push(pr.puppet.pos);

        // Camera follows the puppet, clamped to the map.
        pr.follow_camera(sw, sh, fw, fh);

        self.path_recorder = Some(pr);
    }

    /// Step the focused scene-name field: feed it the frame's keys, and on Return
    /// validate the typed name. A valid identifier is accepted (flagged "replaces
    /// existing" when it collides with a saved scene, since re-recording under a
    /// name overwrites it); anything else keeps the field open with a hint. Escape
    /// abandons the edit, leaving the previous name.
    pub(super) fn step_recorder_naming(&mut self, pr: &mut PathRecorder, input: &EggInput) {
        let Some(mut field) = pr.naming.take() else {
            return;
        };
        match field.step(input) {
            TextEvent::Active => pr.naming = Some(field),
            TextEvent::Cancel => pr.status = None,
            TextEvent::Commit => {
                let candidate = field.text().trim().to_string();
                if scene::is_identifier_name(&candidate) {
                    let replaces = self.scene_names.iter().any(|n| n == &candidate);
                    pr.name = candidate;
                    pr.status = replaces.then(|| format!("replaces existing '{}'", pr.name));
                } else {
                    pr.status = Some("name: letters, digits, _ only".into());
                    pr.naming = Some(field);
                }
            }
        }
    }

    /// Save the recording as a one-actor cutscene (the chosen actor's chain), under
    /// the recorder's name, merged into the on-disk `.eggscene` registry (so other
    /// cutscenes survive) and recorded as one undoable [`EditAction::SceneEdit`],
    /// then signal the host to re-parse + live-reload it. The editor writes the
    /// file itself (it holds the console); only the install is host-deferred.
    pub(super) fn commit_path_recorder(&mut self, system: &mut impl ConsoleApi) {
        // Read the on-disk registry as RAW TEXT and merge into it textually
        // ([`merge_cutscene_source`]) — replacing just this scene's block and
        // leaving every other line verbatim. Parsing the file into a `SceneFile`
        // and re-emitting it would discard every hand-authored comment and blank
        // line, silently eating the file's documentation each time you record.
        // Still validate FIRST: a file that exists but won't parse must NOT be
        // clobbered — abort and keep the recording so it can be fixed in F2 and
        // retried. A missing/empty file is a fresh start.
        let raw = match system.read_file(SCENE_PATH) {
            None => String::new(),
            Some(bytes) => match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => {
                    log::warn!("path recorder: `{SCENE_PATH}` isn't valid UTF-8 — not saving");
                    return;
                }
            },
        };
        if !raw.trim().is_empty() && scene::parse(&raw).is_err() {
            log::warn!(
                "path recorder: `{SCENE_PATH}` is unparseable — not saving (fix it in F2 first)"
            );
            return;
        }
        let Some(mut pr) = self.path_recorder.take() else {
            return;
        };
        // A name should already be a valid identifier (the field validates it, and
        // the `<map>_path` default is always valid), but guard defensively: never
        // emit a header the file can't re-parse. Keep the recording to retry.
        if !scene::is_identifier_name(&pr.name) {
            pr.status = Some("name: letters, digits, _ only".into());
            self.path_recorder = Some(pr);
            return;
        }
        // Fold the trailing walked buffer into the instruction list (each segment
        // is edge-trimmed as it's folded — see `fold_walk_run`). Nothing recorded
        // ⇒ nothing to save.
        pr.fold_walk_run();
        if pr.instructions.is_empty() {
            return;
        }
        let actor = pr.actor_token().to_string();
        // A non-reserved actor is a map creature referenced by id; bind it with a
        // `find` init so the chain resolves it explicitly (and fail-safely if it's
        // gone). `player`/`companion N` resolve without any binding.
        let init = if scene::is_reserved_actor(&actor) {
            Vec::new()
        } else {
            vec![scene::GetEntity::GetOrIgnore {
                name: actor.clone(),
            }]
        };
        let chain = Chain {
            actor,
            instructions: pr.instructions.clone(),
        };
        let def = CutsceneDef {
            init,
            content: vec![CutsceneContent::Move(vec![chain])],
            ..Default::default()
        };
        let block = scene::emit_cutscene(&pr.name, &def);
        let merged = merge_cutscene_source(&raw, &pr.name, &block);
        debug_assert!(
            scene::parse(&merged).is_ok(),
            "merged source must re-parse: {merged}"
        );
        // Route the write through the History as one undoable edit (before = the
        // file as read, after = the merge), so Ctrl+Z reverts the whole recording.
        self.install_scene_source(system, merged.clone());
        self.record(EditAction::SceneEdit {
            before: raw,
            after: merged,
        });
        // Save-and-play: drop straight into the scrubber on what was just
        // recorded — replayed directly from `def`, so it needn't wait for the
        // on-disk scene to live-reload back into the registry.
        self.pending_scrub = Some(ScrubRequest::Recorded(pr.name.clone(), def));
    }

    /// Write an `.eggscene` source to disk and stage it for the host's live-reload
    /// (re-parse → `set_scenes`). The single seam the recorder's save and its
    /// undo/redo share — the editor owns the file; only the registry install is
    /// host-deferred (see [`pending_scene`](Self::pending_scene)).
    pub(super) fn install_scene_source(&mut self, system: &mut impl ConsoleApi, source: String) {
        system.write_file(SCENE_PATH, source.as_bytes());
        self.pending_scene = Some(source);
    }

    /// The recorder chrome: a `PATH RECORDER` banner, a clickable name + actor row,
    /// an optional status line, and a bottom bar (hint + save/cancel). The central
    /// spacer is the map canvas — a click there drops a waypoint. Shared by the hit
    /// pass and the draw pass.
    pub(super) fn build_path_recorder_ui(&self) -> Ui<EditorKey> {
        let (sw, sh) = self.dock.solved.screen;
        let pr = self.path_recorder.as_ref();
        let naming = pr.is_some_and(|pr| pr.naming.is_some());
        let mut b = UiBuilder::new();
        let banner = b
            .text("PATH RECORDER")
            .small(true)
            .center()
            .color(0)
            .full_width(8.0)
            .fill(11)
            .id();
        // Name: shows the live edit buffer (with caret) while focused, else the
        // committed name; clicking it focuses the field.
        let name_text = match pr {
            Some(pr) if pr.naming.is_some() => {
                format!("name: {}", pr.naming.as_ref().unwrap().display())
            }
            Some(pr) => format!("name: {}", pr.name),
            None => "name:".to_string(),
        };
        let name_row = b
            .text(name_text)
            .small(true)
            .color(if naming { 0 } else { 12 })
            .full_width(7.0)
            .fill(if naming { 14 } else { 0 })
            .key(EditorKey::PathRecName)
            .id();
        let actor_row = b
            .text(format!(
                "actor: {}   [Tab]",
                pr.map_or("player", |pr| pr.actor_token())
            ))
            .small(true)
            .color(12)
            .full_width(7.0)
            .key(EditorKey::PathRecActor)
            .id();
        let mut top = vec![banner, name_row, actor_row];
        if let Some(status) = pr.and_then(|pr| pr.status.as_ref()) {
            top.push(
                b.text(status.clone())
                    .small(true)
                    .color(9)
                    .full_width(7.0)
                    .id(),
            );
        }
        let hint = b
            .text("dpad/click = path   Tab = actor   N = noclip   Z = save   X = cancel")
            .small(true)
            .color(13)
            .grow(1.0)
            .id();
        let ok = b
            .text("save")
            .small(true)
            .center()
            .color(0)
            .full_width(7.0)
            .outlined(11, 11)
            .key(EditorKey::PathRecOk)
            .id();
        let cancel = b
            .text("cancel")
            .small(true)
            .center()
            .color(0)
            .full_width(7.0)
            .outlined(8, 8)
            .key(EditorKey::PathRecCancel)
            .id();
        let bottom = b.row(2.0, [hint, ok, cancel]).pad(1.0).id();
        // The central spacer is the clickable map canvas (waypoint placement).
        let canvas = b.spacer(0.0).grow(1.0).key(EditorKey::PathRecCanvas).id();
        let header = b.column(0.0, top).id();
        let root = b
            .column(0.0, [header, canvas, bottom])
            .size(sw as f32, sh as f32)
            .id();
        b.finish(root, (sw as f32, sh as f32))
    }

    /// Draw the fullscreen recorder: the current map rendered 1:1 at the follow
    /// camera, the recorded path as a polyline, the puppet ghost, and the chrome.
    pub(super) fn draw_path_recorder_fullscreen(
        &self,
        draw_state: &mut DrawState,
        font: &Font,
        map: &MapInfo,
        maps: &MapStore,
    ) {
        let Some(pr) = self.path_recorder.as_ref() else { return };
        let cam = pr.camera;
        let Some(tiled) = maps.get(&map.source) else { return };

        let bg = draw_state.resolve(tiled.bg_colour().unwrap_or_default());
        draw_state.rgba(LayerId::BG).fill(bg);
        map.draw_bg_indexed(draw_state, LayerId::BG, tiled, cam, false);
        // Static preview: sprite-plane layers draw flat (no live entities here).
        map.draw_sprite_indexed(draw_state, LayerId::BG, tiled, cam, false);
        map.draw_fg_indexed(draw_state, LayerId::BG, tiled, cam, false);

        // The recorded path as a polyline (colour 11), under the puppet.
        let line_col = draw_state.colour(11);
        {
            let layer = draw_state.rgba(LayerId::BG);
            for seg in pr.path.windows(2) {
                layer.line(
                    i32::from(seg[0].x - cam.x),
                    i32::from(seg[0].y - cam.y),
                    i32::from(seg[1].x - cam.x),
                    i32::from(seg[1].y - cam.y),
                    line_col,
                );
            }
        }

        // The puppet ghost on top, then the chrome.
        pr.puppet.draw_params(cam).draw_to(draw_state, LayerId::BG);
        self.build_path_recorder_ui()
            .draw_at(0, 0, draw_state, font, LayerId::BG);
    }

    /// Set the selected warp's landing point from a click in its destination
    /// preview box: invert the same letterbox fit the draw used to recover the
    /// clicked map pixel, clamped to the target's bounds. A click in the
    /// letterbox margin (outside the rendered map) is ignored. One undo step.
    pub(super) fn place_warp_from_preview(
        &mut self,
        map: &mut MapInfo,
        maps: &MapStore,
        box_rect: Rect,
        cursor: Vec2,
    ) {
        let dest = match self
            .selected
            .and_then(|i| map.objects.get(i))
            .map(|o| &o.effect)
        {
            Some(ObjectEffect::Warp(w)) => w.map.clone().unwrap_or_else(|| map.source.clone()),
            _ => return,
        };
        let Some(tiled) = maps.get(&dest) else {
            return;
        };
        let (fw, fh) = (
            (tiled.width as u32 * 8).max(1),
            (tiled.height as u32 * 8).max(1),
        );
        let (inner, s) = fit_preview(box_rect, fw, fh);
        if s <= 0.0 || !inner.contains(cursor) {
            return;
        }
        let mx = (((cursor.x - inner.x) as f32) / s).clamp(0.0, fw as f32 - 1.0) as i16;
        let my = (((cursor.y - inner.y) as f32) / s).clamp(0.0, fh as f32 - 1.0) as i16;
        self.modify_warp(map, |w| {
            w.to.x = mx;
            w.to.y = my;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clicking a warp's "open" queues its destination map plus the landing point,
    /// which the host loads and frames the camera on. A same-map warp (`map =
    /// None`) queues nothing — there's nowhere to open.
    #[test]
    fn open_warp_dest_queues_destination_and_landing() {
        let mut ed = MapViewer::primary();
        let warp = |dest: Option<&str>, to| {
            MapObject::new(
                Hitbox::new(0, 0, 8, 8),
                ObjectEffect::Warp(Warp::new(dest, to)),
                None,
            )
        };

        let objects = vec![warp(Some("town"), Vec2::new(40, 24))];
        ed.selected = Some(0);
        ed.open_selected_warp_dest(&objects);
        assert_eq!(
            ed.pending_open,
            Some(("town".to_string(), Some(Vec2::new(40, 24)))),
            "opens the destination map, carrying the landing point as the focus"
        );

        // A same-map warp has no destination to load: nothing queued.
        ed.pending_open = None;
        let same_map = vec![warp(None, Vec2::new(1, 1))];
        ed.open_selected_warp_dest(&same_map);
        assert_eq!(ed.pending_open, None, "same-map warp queues no open");
    }

    /// `clamp_camera` keeps a 1:1 map framed: a map larger than the view pans
    /// within `[0, content - view]`; a smaller one pins to a centred negative
    /// offset (so it sits in the middle, not the corner).
    #[test]
    fn clamp_camera_pans_large_and_centres_small() {
        // Larger than the view: clamp into bounds.
        assert_eq!(clamp_camera(-5, 200, 100), 0);
        assert_eq!(clamp_camera(150, 200, 100), 100); // content - view
        assert_eq!(clamp_camera(40, 200, 100), 40); // within bounds, untouched
        // Smaller than the view: pinned centred, ignoring the requested value.
        assert_eq!(clamp_camera(999, 60, 100), -20); // -((100 - 60) / 2)
    }

    /// Opening seeds the placement session from the selected warp; committing
    /// writes the working landing to the warp and closes; cancelling (dropping the
    /// session) leaves the warp untouched — nothing is written until commit.
    #[test]
    fn warp_preview_opens_commits_and_cancels() {
        let mut store = MapStore::default();
        store.insert("dest", TiledMap::blank_modern(20, 15)); // 160×120 px
        let mut map = MapInfo {
            source: "home".to_string(),
            ..MapInfo::default()
        };
        map.objects.push(MapObject::warp(
            Hitbox::new(0, 0, 8, 8),
            Warp::new(Some("dest"), Vec2::new(40, 24)),
        ));
        let mut ed = MapViewer::primary();
        ed.selected = Some(0);

        ed.open_warp_preview(&map, &store);
        let pv = ed.warp_preview.clone().expect("opened a session");
        assert_eq!(pv.dest, "dest");
        assert_eq!(pv.point, Vec2::new(40, 24), "seeded from the warp's landing");

        // Commit a moved point: writes it to the warp and closes.
        let mut moved = pv;
        moved.point = Vec2::new(50, 30);
        ed.commit_warp_preview(&moved, &mut map);
        assert!(ed.warp_preview.is_none(), "commit closes the session");
        let ObjectEffect::Warp(w) = &map.objects[0].effect else {
            panic!("still a warp");
        };
        assert_eq!(w.to, Vec2::new(50, 30), "committed the moved landing");

        // Re-open then cancel (drop the session): the warp keeps its value.
        ed.open_warp_preview(&map, &store);
        ed.warp_preview = None;
        let ObjectEffect::Warp(w) = &map.objects[0].effect else {
            panic!("still a warp");
        };
        assert_eq!(w.to, Vec2::new(50, 30), "cancel leaves the warp untouched");
    }

    /// Committing a recording emits a one-actor `player` `Record` cutscene, merges
    /// it into the on-disk registry (existing scenes survive), writes the file, and
    /// stages the source for the host's live-reload.
    #[test]
    fn path_recorder_commit_emits_and_merges_a_record_cutscene() {
        use egg_platform::test_console::TestConsole;
        let mut ed = MapViewer::primary();
        ed.path_recorder = Some(PathRecorder::test(
            vec![((1, 0), 5), ((0, 1), 3)],
            "town_path",
        ));
        let mut console = TestConsole::new();
        // A pre-existing cutscene on disk must survive the merge.
        console.write_file(SCENE_PATH, b"#cutscene other\n    wait 5");

        ed.commit_path_recorder(&mut console);
        assert!(ed.path_recorder.is_none(), "commit closes the session");
        let src = ed.pending_scene.clone().expect("staged for live-reload");
        assert_eq!(
            console.read_file(SCENE_PATH),
            Some(src.clone().into_bytes()),
            "wrote the same source to disk",
        );

        let file = scene::parse(&src).expect("emitted source re-parses");
        assert!(file.get_cutscene("other").is_some(), "existing scene survives");
        let def = file.get_cutscene("town_path").expect("new scene added");
        let CutsceneContent::Move(chains) = &def.content[0] else {
            panic!("first content is a move");
        };
        assert_eq!(chains[0].actor, "player");
        assert!(matches!(
            chains[0].instructions[0].motion,
            Motion::Record { noclip: false, .. }
        ));
    }

    /// Saving merges textually, so hand-authored comments and other scenes in the
    /// file survive untouched (a parse-then-re-emit would silently eat them).
    #[test]
    fn path_recorder_save_preserves_comments_and_other_scenes() {
        use egg_platform::test_console::TestConsole;
        let mut ed = MapViewer::primary();
        ed.path_recorder = Some(PathRecorder::test(vec![((1, 0), 5)], "town_path"));
        let mut console = TestConsole::new();
        let original = "// a hand-authored header\n// keep me\n#cutscene other\n    wait 5\n";
        console.write_file(SCENE_PATH, original.as_bytes());

        ed.commit_path_recorder(&mut console);
        let saved = String::from_utf8(console.read_file(SCENE_PATH).unwrap()).unwrap();
        assert!(saved.contains("// a hand-authored header"), "header kept: {saved}");
        assert!(saved.contains("// keep me"), "all comments kept: {saved}");
        assert!(saved.contains("#cutscene other"), "other scene kept: {saved}");
        // And the result still parses to both scenes.
        let file = scene::parse(&saved).expect("merged source re-parses");
        assert!(file.get_cutscene("other").is_some(), "other survives the merge");
        assert!(file.get_cutscene("town_path").is_some(), "new scene added");
    }

    /// Re-recording a scene replaces its block in place (no duplicate header),
    /// leaving the other scenes alone.
    #[test]
    fn path_recorder_save_replaces_an_existing_same_name_block() {
        use egg_platform::test_console::TestConsole;
        let mut ed = MapViewer::primary();
        ed.path_recorder = Some(PathRecorder::test(vec![((1, 0), 5)], "town_path"));
        let mut console = TestConsole::new();
        // An old town_path (a bare wait) plus a neighbour to preserve.
        let original = "#cutscene town_path\n    wait 99\n\n#cutscene other\n    wait 5\n";
        console.write_file(SCENE_PATH, original.as_bytes());

        ed.commit_path_recorder(&mut console);
        let saved = String::from_utf8(console.read_file(SCENE_PATH).unwrap()).unwrap();
        let file = scene::parse(&saved).expect("merged source re-parses");
        assert_eq!(file.cutscenes.len(), 2, "no duplicate town_path: {saved}");
        assert!(file.get_cutscene("other").is_some(), "neighbour survives");
        // The new town_path is the recorded move, not the old `wait 99`.
        let def = file.get_cutscene("town_path").unwrap();
        assert!(
            matches!(def.content.first(), Some(CutsceneContent::Move(_))),
            "town_path replaced with the recording: {saved}"
        );
    }

    /// Holding a direction drives the puppet: the step reads the controller dpad,
    /// walks the puppet, and records the commanded heading. (Isolates the step
    /// logic from host input — injects the controller directly.)
    #[test]
    fn path_recorder_drive_moves_the_puppet_when_a_direction_is_held() {
        use egg_platform::test_console::TestConsole;
        let mut store = MapStore::default();
        let mut map = MapInfo::default();
        let mut ed = MapViewer::primary();
        ed.open_path_recorder(&map, &store, Vec2::new(0, 0));
        let start = ed.path_recorder.as_ref().unwrap().puppet.pos;

        let mut console = TestConsole::new();
        let mut input = EggInput::new();
        input.controllers[0].right = [true, false]; // hold right this frame
        ed.step_path_recorder(&mut console, &input, &mut map, &mut store, (200.0, 150.0));

        let after = ed.path_recorder.as_ref().unwrap().puppet.pos;
        assert!(
            after.x > start.x,
            "held-right should move the puppet: {start:?} -> {after:?}"
        );
    }

    /// `P` opens the scene picker over the engine-pushed name list, highlighting
    /// the top. The names come from `scene_names`, which the engine refreshes
    /// each focused frame from the cutscene registry.
    #[test]
    fn scene_picker_opens_over_the_pushed_names() {
        let mut ed = MapViewer::primary();
        ed.scene_names = vec!["backyard_path".into(), "pet_dog".into()];
        ed.open_scene_picker();
        let p = ed.scene_picker.as_ref().expect("picker opened");
        assert_eq!(
            p.names,
            vec!["backyard_path".to_string(), "pet_dog".to_string()]
        );
        assert_eq!(p.selected, 0, "highlight starts at the top");
    }

    /// A recording with no real movement (all idle) saves nothing.
    #[test]
    fn path_recorder_ignores_an_empty_recording() {
        use egg_platform::test_console::TestConsole;
        let mut ed = MapViewer::primary();
        ed.path_recorder = Some(PathRecorder::test(vec![((0, 0), 30)], "town_path"));
        let mut console = TestConsole::new();
        ed.commit_path_recorder(&mut console);
        assert!(ed.pending_scene.is_none(), "nothing recorded, nothing saved");
    }

    /// A `main.eggscene` that exists but won't parse is left untouched (no silent
    /// wipe of hand-authored cutscenes); the recording is kept to retry.
    #[test]
    fn path_recorder_does_not_clobber_an_unparseable_scene_file() {
        use egg_platform::test_console::TestConsole;
        let mut ed = MapViewer::primary();
        ed.path_recorder = Some(PathRecorder::test(vec![((1, 0), 5)], "town_path"));
        let mut console = TestConsole::new();
        let bad = b"#cutscene broken\n    bogusverb 1 2";
        console.write_file(SCENE_PATH, bad);
        ed.commit_path_recorder(&mut console);
        assert_eq!(console.read_file(SCENE_PATH), Some(bad.to_vec()), "file untouched");
        assert!(ed.pending_scene.is_none(), "nothing staged");
        assert!(ed.path_recorder.is_some(), "recording kept to retry");
    }

    /// Leading + trailing idle (reaction-time pauses) are trimmed; interior idle
    /// (intentional timing) is preserved.
    #[test]
    fn path_recorder_trims_outer_idle_keeps_inner() {
        use egg_platform::test_console::TestConsole;
        let mut ed = MapViewer::primary();
        ed.path_recorder = Some(PathRecorder::test(
            vec![
                ((0, 0), 10),
                ((1, 0), 5),
                ((0, 0), 4),
                ((0, 1), 3),
                ((0, 0), 20),
            ],
            "p",
        ));
        let mut console = TestConsole::new();
        ed.commit_path_recorder(&mut console);
        let file = scene::parse(&ed.pending_scene.unwrap()).unwrap();
        let CutsceneContent::Move(chains) = &file.get_cutscene("p").unwrap().content[0] else {
            panic!("move");
        };
        let Motion::Record { runs, .. } = &chains[0].instructions[0].motion else {
            panic!("record");
        };
        assert_eq!(runs, &vec![((1, 0), 5), ((0, 0), 4), ((0, 1), 3)]);
    }

    /// The move of a chosen map creature: the emitted scene binds it with a `find`
    /// init and its chain names the creature's id (as a hand-authored scene does).
    #[test]
    fn path_recorder_records_a_chosen_map_creature() {
        use egg_platform::test_console::TestConsole;
        let mut ed = MapViewer::primary();
        let mut pr = PathRecorder::test(vec![((1, 0), 4)], "dog_path");
        pr.actors = vec![
            ("player".to_string(), Vec2::new(0, 0)),
            ("dog".to_string(), Vec2::new(50, 50)),
        ];
        pr.actor = 1; // the dog
        ed.path_recorder = Some(pr);
        let mut console = TestConsole::new();
        ed.commit_path_recorder(&mut console);

        let file = scene::parse(&ed.pending_scene.unwrap()).unwrap();
        let def = file.get_cutscene("dog_path").unwrap();
        assert_eq!(
            def.init,
            vec![scene::GetEntity::GetOrIgnore { name: "dog".into() }],
            "a map creature is bound with a `find` init"
        );
        let CutsceneContent::Move(chains) = &def.content[0] else {
            panic!("move");
        };
        assert_eq!(chains[0].actor, "dog", "the chain names the creature");
    }

    /// The player needs no init binding — `player` resolves without one.
    #[test]
    fn path_recorder_player_actor_emits_no_init() {
        use egg_platform::test_console::TestConsole;
        let mut ed = MapViewer::primary();
        ed.path_recorder = Some(PathRecorder::test(vec![((1, 0), 4)], "p_path"));
        let mut console = TestConsole::new();
        ed.commit_path_recorder(&mut console);
        let file = scene::parse(&ed.pending_scene.unwrap()).unwrap();
        assert!(
            file.get_cutscene("p_path").unwrap().init.is_empty(),
            "the player is a reserved actor — no binding"
        );
    }

    /// A committed recording is one undoable edit: undo re-installs the file as it
    /// was before, redo puts the recording back (both write disk + stage a reload).
    #[test]
    fn path_recorder_commit_is_undoable() {
        use egg_platform::test_console::TestConsole;
        let mut ed = MapViewer::primary();
        let mut map = MapInfo::default();
        let mut maps = MapStore::default();
        ed.path_recorder = Some(PathRecorder::test(vec![((1, 0), 5)], "town_path"));
        let mut console = TestConsole::new();
        let before = "#cutscene other\n    wait 5\n";
        console.write_file(SCENE_PATH, before.as_bytes());

        ed.commit_path_recorder(&mut console);
        let after = ed.pending_scene.clone().expect("staged");
        assert!(after.contains("#cutscene town_path"), "recorded: {after}");

        // Undo restores the pre-recording file and stages it for reload.
        ed.undo(&mut console, &mut map, &mut maps);
        let on_disk = String::from_utf8(console.read_file(SCENE_PATH).unwrap()).unwrap();
        assert_eq!(on_disk, before, "undo re-installs the old file");
        assert_eq!(ed.pending_scene.as_deref(), Some(before), "and stages it");

        // Redo re-installs the recording.
        ed.redo(&mut console, &mut map, &mut maps);
        let on_disk = String::from_utf8(console.read_file(SCENE_PATH).unwrap()).unwrap();
        assert_eq!(on_disk, after, "redo re-installs the recording");
        assert_eq!(ed.pending_scene.as_ref(), Some(&after), "and stages it");
    }

    /// The name field validates on commit: a valid identifier is taken, a
    /// collision is taken but flagged "replaces", and a bad name keeps the field
    /// open with the old name intact.
    #[test]
    fn recorder_naming_validates() {
        let mut ed = MapViewer::primary();
        ed.scene_names = vec!["existing".into()];
        let mut pr = PathRecorder::test(vec![], "town_path");
        let mut enter = EggInput::new();
        enter.press_key(ScanCode::Return);

        // A valid new name is accepted and closes the field.
        pr.naming = Some(TextField::new("fresh_path"));
        ed.step_recorder_naming(&mut pr, &enter);
        assert_eq!(pr.name, "fresh_path");
        assert!(pr.naming.is_none());
        assert!(pr.status.is_none(), "a fresh name is not a replace");

        // A collision is accepted but flagged as a replace.
        pr.naming = Some(TextField::new("existing"));
        ed.step_recorder_naming(&mut pr, &enter);
        assert_eq!(pr.name, "existing");
        assert!(
            pr.status.as_deref().unwrap().contains("replaces"),
            "a duplicate name is flagged: {:?}",
            pr.status
        );

        // A bad name (whitespace) is rejected — field stays open, name unchanged.
        pr.naming = Some(TextField::new("two words"));
        ed.step_recorder_naming(&mut pr, &enter);
        assert!(pr.naming.is_some(), "an invalid name keeps the field open");
        assert_eq!(pr.name, "existing", "the name is unchanged");
    }

    /// A clicked waypoint appends a `MoveToPoint`; several compose in click order,
    /// emitting `walk X Y` motions.
    #[test]
    fn path_recorder_waypoints_emit_walk_motions() {
        use egg_platform::test_console::TestConsole;
        let mut ed = MapViewer::primary();
        let mut pr = PathRecorder::test(vec![], "wp_path");
        pr.place_waypoint(Vec2::new(30, 40));
        pr.place_waypoint(Vec2::new(60, 20));
        assert_eq!(
            pr.puppet.pos,
            Vec2::new(60, 20),
            "the puppet follows the last"
        );
        ed.path_recorder = Some(pr);
        let mut console = TestConsole::new();
        ed.commit_path_recorder(&mut console);

        let file = scene::parse(&ed.pending_scene.unwrap()).unwrap();
        let CutsceneContent::Move(chains) = &file.get_cutscene("wp_path").unwrap().content[0]
        else {
            panic!("move");
        };
        let motions: Vec<&Motion> = chains[0].instructions.iter().map(|i| &i.motion).collect();
        assert_eq!(
            motions,
            vec![
                &Motion::MoveToPoint(Vec2::new(30, 40)),
                &Motion::MoveToPoint(Vec2::new(60, 20)),
            ]
        );
    }

    /// Walked runs and clicked waypoints interleave in author order: a buffered
    /// walk, a waypoint, then another buffered walk emit Record, walk, Record.
    #[test]
    fn path_recorder_interleaves_walks_and_waypoints() {
        use egg_platform::test_console::TestConsole;
        let mut ed = MapViewer::primary();
        let mut pr = PathRecorder::test(vec![((1, 0), 3)], "mix_path");
        pr.place_waypoint(Vec2::new(80, 10)); // folds the walk, then the waypoint
        pr.runs = vec![((0, 1), 2)]; // a second walked segment, folded at commit
        ed.path_recorder = Some(pr);
        let mut console = TestConsole::new();
        ed.commit_path_recorder(&mut console);

        let file = scene::parse(&ed.pending_scene.unwrap()).unwrap();
        let CutsceneContent::Move(chains) = &file.get_cutscene("mix_path").unwrap().content[0]
        else {
            panic!("move");
        };
        let kinds: Vec<_> = chains[0]
            .instructions
            .iter()
            .map(|i| match &i.motion {
                Motion::Record { .. } => "record",
                Motion::MoveToPoint(_) => "walk",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, vec!["record", "walk", "record"]);
    }

    /// A canvas click through the modal step drops a waypoint at the clicked map
    /// point (exercising the hit-test → `place_waypoint` wiring).
    #[test]
    fn path_recorder_canvas_click_drops_a_waypoint() {
        use egg_platform::test_console::TestConsole;
        let mut store = MapStore::default();
        let mut map = MapInfo::default();
        let mut ed = MapViewer::primary();
        ed.dock.recompute((200.0, 150.0));
        ed.path_recorder = Some(PathRecorder::test(vec![], "click_path"));
        let mut console = TestConsole::new();

        // Click in the middle of the screen — inside the map-canvas region.
        let mut input = EggInput::new();
        input.mouse.x = [100, 100];
        input.mouse.y = [75, 75];
        input.mouse.left = [true, false]; // a fresh press this frame
        ed.step_path_recorder(&mut console, &input, &mut map, &mut store, (200.0, 150.0));

        let pr = ed.path_recorder.as_ref().expect("still recording");
        assert_eq!(pr.instructions.len(), 1, "one waypoint was placed");
        assert_eq!(
            pr.instructions[0].motion,
            Motion::MoveToPoint(Vec2::new(100, 75))
        );
    }

    /// Switching actor discards the in-progress path and re-seats the puppet on the
    /// new actor's start position.
    #[test]
    fn path_recorder_select_actor_resets_the_path() {
        let mut pr = PathRecorder::test(vec![((1, 0), 6)], "x");
        pr.instructions
            .push(Instruction::new(Motion::MoveToPoint(Vec2::new(1, 1)), 0));
        pr.actors = vec![
            ("player".to_string(), Vec2::new(0, 0)),
            ("dog".to_string(), Vec2::new(90, 30)),
        ];
        pr.select_actor(1);
        assert_eq!(pr.actor, 1);
        assert_eq!(pr.actor_token(), "dog");
        assert_eq!(
            pr.puppet.pos,
            Vec2::new(90, 30),
            "puppet re-seated on the dog"
        );
        assert!(
            pr.runs.is_empty() && pr.instructions.is_empty(),
            "path cleared"
        );
        assert_eq!(pr.path, vec![Vec2::new(90, 30)]);
    }

    /// The modal step self-closes if the destination map vanishes mid-session, so
    /// a deleted target can't strand the editor in a placement view.
    #[test]
    fn warp_preview_self_closes_when_target_missing() {
        let mut store = MapStore::default();
        store.insert("dest", TiledMap::blank_modern(20, 15));
        let mut map = MapInfo::default();
        map.objects.push(MapObject::warp(
            Hitbox::new(0, 0, 8, 8),
            Warp::new(Some("dest"), Vec2::new(1, 1)),
        ));
        let mut ed = MapViewer::primary();
        ed.selected = Some(0);
        ed.open_warp_preview(&map, &store);
        assert!(ed.warp_preview.is_some());

        // Step with a store that no longer has the destination: the session closes.
        let mut empty = MapStore::default();
        ed.step_warp_preview(&EggInput::new(), &mut map, &mut empty, (200.0, 150.0));
        assert!(
            ed.warp_preview.is_none(),
            "self-closes when the destination map is missing"
        );
    }

    /// The warp preview's letterbox fit and its inverse agree: a click at the
    /// centre of a placed map pixel round-trips back to that pixel. This is the
    /// contract the draw (where to blit / mark) and the click handler (how to
    /// invert a click to a coordinate) both depend on.
    #[test]
    fn warp_preview_fit_inverts() {
        // A 240×136 map letterboxed into an 82×64 box: downscales, centres.
        let outer = Rect {
            x: 10,
            y: 20,
            w: 82,
            h: 64,
        };
        let (fw, fh) = (240u32, 136u32);
        let (inner, s) = fit_preview(outer, fw, fh);
        assert!(s > 0.0 && s < 1.0, "a large map downscales: {s}");
        // The inner map sits inside the box, centred (letterboxed).
        assert!(inner.w <= outer.w && inner.h <= outer.h);
        assert!(inner.x >= outer.x && inner.y >= outer.y);

        // Click the middle of where map pixel (100, 50) renders → recover (100,50).
        let (mx, my) = (100i16, 50i16);
        let cursor = Vec2::new(
            inner.x + (mx as f32 * s) as i16,
            inner.y + (my as f32 * s) as i16,
        );
        let inv_x = (((cursor.x - inner.x) as f32) / s) as i16;
        let inv_y = (((cursor.y - inner.y) as f32) / s) as i16;
        // Within one source pixel (the scale's quantisation).
        assert!((inv_x - mx).abs() <= 1, "x round-trips: {inv_x} vs {mx}");
        assert!((inv_y - my).abs() <= 1, "y round-trips: {inv_y} vs {my}");

        // A tiny map (smaller than the box) is shown 1:1, not upscaled, and centred.
        let (inner, s) = fit_preview(outer, 16, 16);
        assert_eq!(s, 1.0, "downscale only — a small map stays 1:1");
        assert_eq!((inner.w, inner.h), (16, 16));
        assert_eq!(inner.x, outer.x + (outer.w - 16) / 2);
    }
}

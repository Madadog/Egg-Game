//! The walk-sprite authoring GUI: a fullscreen, modal editor for a creature
//! preset's nine-cell walk grid (`[presets.<name>.walk]` in `data.toml`). Opened
//! from the Presets panel; edits a working copy (deferred commit) and saves by
//! splicing the re-emitted preset back into `data.toml` — comments and every
//! other section untouched (see [`eggdata::splice_preset`]) — then flags
//! [`MapViewer::pending_data_reload`] so the host re-installs the live
//! [`Presets`](crate::data::eggdata::Presets) registry.

use crate::data::eggdata::{self, PresetDef};
use crate::world::player::{FacingPolicy, LoopMode};

use super::*;

/// A fullscreen, modal walk-sprite authoring session. Fully modal like the warp
/// placement / path recorder: it owns all editor input and paints over the
/// whole editor. `def` is a working copy — nothing touches the live registry or
/// the file until Save.
#[derive(Debug, Clone)]
pub(super) struct WalkEditor {
    /// The preset's `data.toml` key (`presets.<name>`).
    pub(super) name: String,
    /// The working copy every edit lands in. Committed on save, dropped on
    /// cancel.
    pub(super) def: PresetDef,
    /// Selected grid cell, `0..9` row-major (4 = idle).
    pub(super) cell: usize,
    /// Selected frame within the selected cell's animation.
    pub(super) frame: usize,
    /// Preview clock: every cell's animation plays live off this.
    tick: usize,
    /// `Some` while the sprite-sheet picker overlay is up, holding its top-left
    /// visible sheet cell `(col, row)`.
    pub(super) picking: Option<(usize, usize)>,
}

/// Grid geometry — shared by the hit pass and the draw pass so a click can't
/// disagree with what was painted.
const GRID_X: i16 = 8;
const GRID_Y: i16 = 20;
const CELL: i16 = 32;
const GAP: i16 = 2;
/// Frame-strip geometry (right column).
const STRIP_X: i16 = 122;
const STRIP_Y: i16 = 42;
const FBOX: i16 = 20;
const STRIP_COLS: usize = 5;
/// The picker's sheet view starts under a one-line banner.
const PICK_Y: i16 = 10;

/// The screen rect of grid cell `i` (`0..9`).
fn cell_rect(i: usize) -> Rect {
    Rect {
        x: GRID_X + (i % 3) as i16 * (CELL + GAP),
        y: GRID_Y + (i / 3) as i16 * (CELL + GAP),
        w: CELL,
        h: CELL,
    }
}
/// The screen rect of frame-strip slot `j`.
fn frame_rect(j: usize) -> Rect {
    Rect {
        x: STRIP_X + (j % STRIP_COLS) as i16 * (FBOX + 1),
        y: STRIP_Y + (j / STRIP_COLS) as i16 * (FBOX + 1),
        w: FBOX,
        h: FBOX,
    }
}

impl MapViewer {
    /// Open the walk-sprite editor on row `row` of the Presets panel (an index
    /// into the [`preset_defs`](Self::preset_defs) snapshot). A no-op on a stale
    /// row. Starts on the idle (centre) cell, first frame.
    pub(super) fn open_walk_editor(&mut self, row: usize) {
        let Some((name, def)) = self.preset_defs.get(row) else {
            return;
        };
        self.walk_editor = Some(WalkEditor {
            name: name.clone(),
            def: def.clone(),
            cell: 4,
            frame: 0,
            tick: 0,
            picking: None,
        });
    }

    /// Step the modal walk-sprite editor. Keyboard-first (the help bar documents
    /// the bindings); the mouse selects cells/frames, drives the Save / Cancel
    /// buttons, and picks a sheet sprite in the picker overlay.
    pub(super) fn step_walk_editor(&mut self, system: &mut impl ConsoleApi, input: &EggInput) {
        let Some(mut we) = self.walk_editor.clone() else {
            return;
        };
        we.tick = we.tick.wrapping_add(1);
        let mouse = input.mouse;
        let cursor = mouse.pos();
        let (sw, sh) = self.dock.solved.screen;

        // --- The sprite-sheet picker overlay owns input while it is up. ------
        if let Some((mut pcol, mut prow)) = we.picking {
            let cols = self.sheet_cols();
            let rows = self.sheet_tiles().div_ceil(cols.max(1));
            // `.max(0)` before the usize cast: a not-yet-solved (zero-size)
            // screen must clamp to "nothing visible", not wrap negative.
            let (vis_cols, vis_rows) = (
                (sw.max(0) / 8) as usize,
                ((sh - PICK_Y - 10).max(0) / 8) as usize,
            );
            if input.keyp(ScanCode::Escape) || input.keyp(ScanCode::X) || input.keyp(ScanCode::P) {
                we.picking = None;
                self.walk_editor = Some(we);
                return;
            }
            // Arrows + wheel scroll the sheet view; clamp to keep it in range.
            if input.keyp(ScanCode::Left) {
                pcol = pcol.saturating_sub(1);
            }
            if input.keyp(ScanCode::Right) {
                pcol += 1;
            }
            if input.keyp(ScanCode::Up) {
                prow = prow.saturating_sub(1);
            }
            if input.keyp(ScanCode::Down) {
                prow += 1;
            }
            prow = (prow as i32 - mouse.scroll_y[0] as i32).max(0) as usize;
            pcol = pcol.min(cols.saturating_sub(vis_cols));
            prow = prow.min(rows.saturating_sub(vis_rows));
            we.picking = Some((pcol, prow));

            // Click a visible sheet cell: assign its id to the selected frame
            // and drop back to the editor.
            if just_pressed(mouse.left) && cursor.y >= PICK_Y {
                let (cx, cy) = (
                    (cursor.x / 8) as usize + pcol,
                    ((cursor.y - PICK_Y) / 8) as usize + prow,
                );
                let id = cy * cols + cx;
                if cursor.x >= 0 && cx < cols && id < self.sheet_tiles() {
                    if let Some(frame) = we.def.walk.cell_mut(we.cell).frames_mut().get_mut(we.frame)
                    {
                        frame.id = id as i32;
                    }
                    we.picking = None;
                }
            }
            self.walk_editor = Some(we);
            return;
        }

        // --- Editor proper. ---------------------------------------------------
        if input.keyp(ScanCode::Escape) || input.keyp(ScanCode::X) {
            self.walk_editor = None;
            return;
        }
        if input.keyp(ScanCode::Return) || input.keyp(ScanCode::Z) {
            self.walk_editor = Some(we);
            self.save_walk_editor(system);
            return;
        }
        // Save / Cancel buttons (hit-built identically to the draw pass).
        if just_pressed(mouse.left) {
            match self.build_walk_editor_ui(&we).hit_at(0, 0, cursor) {
                Some(EditorKey::WalkEdOk) => {
                    self.walk_editor = Some(we);
                    self.save_walk_editor(system);
                    return;
                }
                Some(EditorKey::WalkEdCancel) => {
                    self.walk_editor = None;
                    return;
                }
                _ => {}
            }
        }

        let shift = input.key(ScanCode::Shift);
        let frames_len = we.def.walk.cells()[we.cell].frames().len();
        // Arrows: move the cell selection; with Shift, nudge the selected
        // frame's pixel offset instead (art alignment).
        let (mut dx, mut dy) = (0i32, 0i32);
        if input.keyp(ScanCode::Left) {
            dx -= 1;
        }
        if input.keyp(ScanCode::Right) {
            dx += 1;
        }
        if input.keyp(ScanCode::Up) {
            dy -= 1;
        }
        if input.keyp(ScanCode::Down) {
            dy += 1;
        }
        if shift {
            if let Some(frame) = we.def.walk.cell_mut(we.cell).frames_mut().get_mut(we.frame) {
                frame.x_offset += dx;
                frame.y_offset += dy;
            }
        } else {
            let (col, row) = ((we.cell % 3) as i32, (we.cell / 3) as i32);
            let (col, row) = ((col + dx).clamp(0, 2), (row + dy).clamp(0, 2));
            let cell = (row * 3 + col) as usize;
            if cell != we.cell {
                we.cell = cell;
                we.frame = 0;
            }
        }
        // Comma / Period: select the previous / next frame.
        if input.keyp(ScanCode::Comma) {
            we.frame = we.frame.saturating_sub(1);
        }
        if input.keyp(ScanCode::Period) {
            we.frame = (we.frame + 1).min(frames_len.saturating_sub(1));
        }
        // N duplicates the selected frame after itself; Delete/Backspace removes
        // it (an animation always keeps at least one frame).
        if input.keyp(ScanCode::N) {
            let frames = we.def.walk.cell_mut(we.cell).frames_mut();
            if let Some(cur) = frames.get(we.frame).cloned() {
                frames.insert(we.frame + 1, cur);
                we.frame += 1;
            }
        }
        if (input.keyp(ScanCode::Delete) || input.keyp(ScanCode::Backspace)) && frames_len > 1 {
            let frames = we.def.walk.cell_mut(we.cell).frames_mut();
            frames.remove(we.frame);
            we.frame = we.frame.min(frames.len() - 1);
        }
        // P opens the sheet picker on the selected frame's sprite.
        if input.keyp(ScanCode::P) {
            we.picking = Some(self.picker_view_for(&we));
        }
        // Per-frame properties: F cycles flip, W/E cycle the tile footprint.
        if let Some(frame) = we.def.walk.cell_mut(we.cell).frames_mut().get_mut(we.frame) {
            if input.keyp(ScanCode::F) {
                frame.flip = match frame.flip {
                    Flip::None => Flip::Horizontal,
                    Flip::Horizontal => Flip::Vertical,
                    Flip::Vertical => Flip::Both,
                    Flip::Both => Flip::None,
                };
            }
            if input.keyp(ScanCode::W) {
                frame.w = frame.w % 4 + 1;
            }
            if input.keyp(ScanCode::E) {
                frame.h = frame.h % 4 + 1;
            }
        }
        // Cell/preset-level properties: G toggles facing, L cycles loopmode.
        if input.keyp(ScanCode::G) {
            let next = match we.def.walk.facing() {
                FacingPolicy::PerAxis => FacingPolicy::Committed,
                FacingPolicy::Committed => FacingPolicy::PerAxis,
            };
            we.def.walk.set_facing(next);
        }
        if input.keyp(ScanCode::L) {
            let anim = we.def.walk.cell_mut(we.cell);
            let next = match anim.loopmode() {
                LoopMode::Loop => LoopMode::Hold,
                _ => LoopMode::Loop,
            };
            anim.set_loopmode(next);
        }

        // Mouse selection: a grid cell, or a frame slot (clicking the selected
        // frame again opens the picker — the strip's "edit this one" gesture).
        if just_pressed(mouse.left) {
            for i in 0..9 {
                if cell_rect(i).contains(cursor) && we.cell != i {
                    we.cell = i;
                    we.frame = 0;
                }
            }
            for j in 0..we.def.walk.cells()[we.cell].frames().len() {
                if frame_rect(j).contains(cursor) {
                    if we.frame == j {
                        we.picking = Some(self.picker_view_for(&we));
                    } else {
                        we.frame = j;
                    }
                }
            }
        }
        self.walk_editor = Some(we);
    }

    /// The picker's initial view: scrolled so the selected frame's sprite is
    /// visible (its sheet row at the top, column origin left).
    fn picker_view_for(&self, we: &WalkEditor) -> (usize, usize) {
        let cols = self.sheet_cols().max(1);
        let id = we.def.walk.cells()[we.cell]
            .frames()
            .get(we.frame)
            .map_or(0, |f| f.id.max(0) as usize);
        (0, (id / cols).saturating_sub(2))
    }

    /// Commit the session: re-emit the preset, splice it into `data.toml`
    /// (falling back to the embedded shipped source if the store has no runtime
    /// copy yet, so the write is always a complete file), write it back, flag
    /// the data reload for the host, and refresh the panel snapshot so the save
    /// shows immediately. Closes the session.
    pub(super) fn save_walk_editor(&mut self, system: &mut impl ConsoleApi) {
        let Some(we) = self.walk_editor.take() else {
            return;
        };
        let emitted = match eggdata::emit_preset(&we.name, &we.def) {
            Ok(emitted) => emitted,
            Err(e) => {
                log::error!("walk editor: preset {} failed to emit: {e}", we.name);
                return;
            }
        };
        let src = system
            .read_file(eggdata::DATA_PATH)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_else(|| eggdata::shipped_source().to_string());
        let out = eggdata::splice_preset(&src, &we.name, &emitted);
        system.write_file(eggdata::DATA_PATH, out.as_bytes());
        self.pending_data_reload = true;
        if let Some(row) = self.preset_defs.iter_mut().find(|(n, _)| n == &we.name) {
            row.1 = we.def;
        }
    }

    /// The overlay chrome: a banner naming the preset and its grid-level state,
    /// and a bottom bar with the key hints and Save / Cancel buttons. Built
    /// identically by the hit pass and the draw pass.
    fn build_walk_editor_ui(&self, we: &WalkEditor) -> Ui<EditorKey> {
        let (sw, sh) = self.dock.solved.screen;
        let facing = match we.def.walk.facing() {
            FacingPolicy::PerAxis => "per_axis",
            FacingPolicy::Committed => "committed",
        };
        let mut b = UiBuilder::new();
        let banner = b
            .text(format!("WALK SPRITES - {}   facing: {facing}", we.name))
            .small(true)
            .center()
            .color(0)
            .full_width(8.0)
            .fill(11)
            .id();
        let hint = b
            .text("arrows cell  ,. frame  n/del +/-  p pick  f flip  w/e size  g facing  l loop")
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
            .key(EditorKey::WalkEdOk)
            .id();
        let cancel = b
            .text("cancel")
            .small(true)
            .center()
            .color(0)
            .full_width(7.0)
            .outlined(8, 8)
            .key(EditorKey::WalkEdCancel)
            .id();
        let bottom = b.row(2.0, [hint, ok, cancel]).pad(1.0).id();
        let spacer = b.spacer(0.0).grow(1.0).id();
        let root = b
            .column(0.0, [banner, spacer, bottom])
            .size(sw as f32, sh as f32)
            .id();
        b.finish(root, (sw as f32, sh as f32))
    }

    /// Paint the fullscreen walk-sprite editor: the 3×3 heading grid with every
    /// cell's animation playing live, the selected cell's frame strip and
    /// per-frame fields, and the chrome — or, while picking, the sprite sheet
    /// with the selected frame's sprite highlighted.
    pub(super) fn draw_walk_editor_fullscreen(
        &self,
        draw_state: &mut DrawState,
        font: &Font,
    ) {
        let Some(we) = self.walk_editor.as_ref() else {
            return;
        };
        let (sw, sh) = self.dock.solved.screen;
        let backdrop = draw_state.colour(0);
        draw_state.rgba(LayerId::BG).fill(backdrop);
        let dim = draw_state.colour(13);
        let hot = draw_state.colour(11);
        let opts = PrintOptions {
            small_text: true,
            ..PrintOptions::default()
        };

        // --- Picker overlay: the sheet, scrolled, with the pick highlighted. --
        if let Some((pcol, prow)) = we.picking {
            let cols = self.sheet_cols();
            let (vis_cols, vis_rows) = (
                (sw.max(0) / 8) as usize,
                ((sh - PICK_Y - 10).max(0) / 8) as usize,
            );
            let selected = we.def.walk.cells()[we.cell]
                .frames()
                .get(we.frame)
                .map(|f| (f.id, f.w.max(1), f.h.max(1)));
            for r in 0..vis_rows {
                for c in 0..vis_cols {
                    let (col, row) = (pcol + c, prow + r);
                    if col >= cols {
                        continue;
                    }
                    let id = row * cols + col;
                    if id >= self.sheet_tiles() {
                        continue;
                    }
                    let (x, y) = (c as i32 * 8, PICK_Y as i32 + r as i32 * 8);
                    draw_state.spr(
                        LayerId::BG,
                        &PALETTE_MAP_IDENTITY,
                        id as i32,
                        x,
                        y,
                        SpriteOptions {
                            transparent: Some(0),
                            ..SpriteOptions::default()
                        },
                    );
                }
            }
            // Outline the currently-assigned sprite's w×h footprint if visible.
            if let Some((id, w, h)) = selected
                && id >= 0
            {
                let (col, row) = (id as usize % cols, id as usize / cols);
                if col >= pcol && row >= prow {
                    let x = ((col - pcol) * 8) as i32;
                    let y = PICK_Y as i32 + ((row - prow) * 8) as i32;
                    draw_state.rgba(LayerId::BG).stroke_rect(
                        x - 1,
                        y - 1,
                        w * 8 + 2,
                        h * 8 + 2,
                        hot,
                    );
                }
            }
            let canvas = draw_state.rgba(LayerId::BG);
            print_to_with_font(font, canvas, "PICK A SPRITE", 2, 1, hot, opts.clone());
            print_to_with_font(
                font,
                canvas,
                "click = assign   arrows/wheel = scroll   esc = back",
                2,
                (sh - 8) as i32,
                dim,
                opts,
            );
            return;
        }

        // --- The 3×3 heading grid, every cell animating live. ----------------
        let preview = we.tick / 10;
        for i in 0..9 {
            let r = cell_rect(i);
            let outline = if i == we.cell {
                hot
            } else if i == 4 {
                draw_state.colour(2)
            } else {
                dim
            };
            draw_state.rgba(LayerId::BG).stroke_rect(
                r.x as i32,
                r.y as i32,
                r.w as i32,
                r.h as i32,
                outline,
            );
            let anim = &we.def.walk.cells()[i];
            if anim.frames().is_empty() {
                continue;
            }
            let frame = anim.get_frame(preview);
            let (fw, fh) = (frame.w.max(1) * 8, frame.h.max(1) * 8);
            let x = r.x as i32 + ((r.w as i32 - fw).max(0)) / 2;
            let y = r.y as i32 + ((r.h as i32 - fh).max(0)) / 2;
            let draw = SpriteOptions {
                x_offset: 0,
                y_offset: 0,
                scale: 1,
                transparent: Some(0),
                ..frame.clone()
            };
            draw_state.spr(LayerId::BG, &PALETTE_MAP_IDENTITY, draw.id, x, y, draw);
        }

        // --- The selected cell's frame strip + per-frame fields. --------------
        let anim = &we.def.walk.cells()[we.cell];
        let loopmode = match anim.loopmode() {
            LoopMode::Loop => "loop".to_string(),
            LoopMode::Hold => "hold".to_string(),
            LoopMode::LoopRange(a, b) => format!("loop {a}-{b}"),
        };
        {
            let canvas = draw_state.rgba(LayerId::BG);
            let cell_name = [
                "up-left", "up", "up-right", "left", "idle", "right", "down-left", "down",
                "down-right",
            ][we.cell];
            print_to_with_font(
                font,
                canvas,
                &format!("{cell_name}   {loopmode}"),
                STRIP_X as i32,
                (STRIP_Y - 18) as i32,
                hot,
                opts.clone(),
            );
            print_to_with_font(
                font,
                canvas,
                &format!("frame {}/{}", we.frame + 1, anim.frames().len()),
                STRIP_X as i32,
                (STRIP_Y - 10) as i32,
                dim,
                opts.clone(),
            );
        }
        for (j, frame) in anim.frames().iter().enumerate().take(STRIP_COLS * 2) {
            let r = frame_rect(j);
            let outline = if j == we.frame { hot } else { dim };
            draw_state.rgba(LayerId::BG).stroke_rect(
                r.x as i32,
                r.y as i32,
                r.w as i32,
                r.h as i32,
                outline,
            );
            let (fw, fh) = (frame.w.max(1) * 8, frame.h.max(1) * 8);
            let x = r.x as i32 + ((r.w as i32 - fw).max(0)) / 2;
            let y = r.y as i32 + ((r.h as i32 - fh).max(0)) / 2;
            let draw = SpriteOptions {
                x_offset: 0,
                y_offset: 0,
                scale: 1,
                transparent: Some(0),
                ..frame.clone()
            };
            draw_state.spr(LayerId::BG, &PALETTE_MAP_IDENTITY, draw.id, x, y, draw);
        }
        if let Some(frame) = anim.frames().get(we.frame) {
            let flip = match frame.flip {
                Flip::None => "-",
                Flip::Horizontal => "x",
                Flip::Vertical => "y",
                Flip::Both => "xy",
            };
            let canvas = draw_state.rgba(LayerId::BG);
            print_to_with_font(
                font,
                canvas,
                &format!(
                    "id {}  {}x{}  flip {}  off {},{}",
                    frame.id, frame.w, frame.h, flip, frame.x_offset, frame.y_offset
                ),
                STRIP_X as i32,
                (STRIP_Y + 2 * (FBOX + 1) + 4) as i32,
                dim,
                opts,
            );
        }

        // Chrome (banner + hints + buttons) on top.
        self.build_walk_editor_ui(we)
            .draw_at(0, 0, draw_state, font, LayerId::BG);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::test_console::TestConsole;

    /// A viewer with the shipped presets as its snapshot (as the engine would
    /// push), plus a console whose store holds the shipped `data.toml`.
    fn harness() -> (MapViewer, TestConsole) {
        let mut ed = MapViewer {
            sheet: (32, 64),
            ..Default::default()
        };
        ed.preset_defs = eggdata::Presets::builtin().named_defs();
        let mut console = TestConsole::new();
        console.files.insert(
            eggdata::DATA_PATH.to_string(),
            eggdata::shipped_source().as_bytes().to_vec(),
        );
        (ed, console)
    }

    fn row_of(ed: &MapViewer, name: &str) -> usize {
        ed.preset_defs
            .iter()
            .position(|(n, _)| n == name)
            .expect("preset in snapshot")
    }

    /// Open seeds a working copy on the idle cell; editing that copy touches
    /// neither the snapshot nor the file until save; Esc discards it all.
    #[test]
    fn walk_editor_opens_edits_a_working_copy_and_esc_discards() {
        let (mut ed, mut console) = harness();
        let row = row_of(&ed, "critter");
        ed.open_walk_editor(row);
        let we = ed.walk_editor.as_ref().expect("session open");
        assert_eq!((we.name.as_str(), we.cell, we.frame), ("critter", 4, 0));

        // Retile the idle frame in the working copy.
        ed.walk_editor
            .as_mut()
            .unwrap()
            .def
            .walk
            .cell_mut(4)
            .frames_mut()[0]
            .id = 999;
        assert_ne!(
            ed.preset_defs[row].1.walk.cells()[4].frames()[0].id,
            999,
            "snapshot untouched while editing"
        );

        // Esc: the session (and the edit) is gone; nothing was written.
        let mut esc = EggInput::new();
        esc.press_key(ScanCode::Escape);
        ed.step_walk_editor(&mut console, &esc);
        assert!(ed.walk_editor.is_none(), "esc closes");
        assert!(!ed.pending_data_reload, "no reload without a save");
        let on_disk = console.files.get(eggdata::DATA_PATH).unwrap();
        assert_eq!(
            on_disk,
            &eggdata::shipped_source().as_bytes().to_vec(),
            "cancel writes nothing"
        );
    }

    /// Save re-emits the preset, splices it into the stored `data.toml`
    /// (comments intact), flags the host reload, and refreshes the snapshot.
    #[test]
    fn walk_editor_save_splices_flags_and_refreshes() {
        let (mut ed, mut console) = harness();
        let row = row_of(&ed, "critter");
        ed.open_walk_editor(row);
        ed.walk_editor
            .as_mut()
            .unwrap()
            .def
            .walk
            .cell_mut(4)
            .frames_mut()[0]
            .id = 777;
        let edited = ed.walk_editor.as_ref().unwrap().def.clone();

        let mut save = EggInput::new();
        save.press_key(ScanCode::Return);
        ed.step_walk_editor(&mut console, &save);
        assert!(ed.walk_editor.is_none(), "save closes the session");
        assert!(ed.pending_data_reload, "save schedules the data reload");
        assert_eq!(ed.preset_defs[row].1, edited, "snapshot refreshed");

        let out = String::from_utf8(console.files.get(eggdata::DATA_PATH).unwrap().clone()).unwrap();
        let reparsed = eggdata::parse(&out).unwrap();
        assert_eq!(reparsed.presets["critter"], edited, "file took the edit");
        assert!(
            out.contains("# --- dialogue portraits ---"),
            "comments survive the save"
        );
    }

    /// The frame-strip editing invariants: duplicate inserts after the selected
    /// frame, delete floors at one frame, and the frame cursor never dangles.
    #[test]
    fn walk_editor_frame_add_remove_invariants() {
        let (mut ed, mut console) = harness();
        ed.open_walk_editor(row_of(&ed, "critter"));
        let frames = |ed: &MapViewer| {
            let we = ed.walk_editor.as_ref().unwrap();
            we.def.walk.cells()[we.cell].frames().len()
        };
        let start = frames(&ed);

        let mut dup = EggInput::new();
        dup.press_key(ScanCode::N);
        ed.step_walk_editor(&mut console, &dup);
        assert_eq!(frames(&ed), start + 1, "N duplicates");
        assert_eq!(ed.walk_editor.as_ref().unwrap().frame, 1, "selects the copy");

        let mut del = EggInput::new();
        del.press_key(ScanCode::Delete);
        for _ in 0..(start + 3) {
            ed.step_walk_editor(&mut console, &del);
        }
        assert_eq!(frames(&ed), 1, "delete floors at one frame");
        assert_eq!(ed.walk_editor.as_ref().unwrap().frame, 0, "cursor clamped");
    }

    /// Cell navigation clamps to the 3×3 grid and resets the frame cursor; the
    /// picker assigns a clicked sheet sprite to the selected frame.
    #[test]
    fn walk_editor_navigation_and_picker_assign() {
        let (mut ed, mut console) = harness();
        ed.open_walk_editor(row_of(&ed, "critter"));
        // Idle (4) → left (3), then clamp at the left edge.
        let mut left = EggInput::new();
        left.press_key(ScanCode::Left);
        ed.step_walk_editor(&mut console, &left);
        assert_eq!(ed.walk_editor.as_ref().unwrap().cell, 3);
        ed.step_walk_editor(&mut console, &left);
        assert_eq!(ed.walk_editor.as_ref().unwrap().cell, 3, "clamped at edge");

        // Open the picker and click sheet cell (2, 1) of the visible view →
        // id = (prow + 1) * cols + (pcol + 2).
        let mut pick = EggInput::new();
        pick.press_key(ScanCode::P);
        ed.step_walk_editor(&mut console, &pick);
        let (pcol, prow) = ed.walk_editor.as_ref().unwrap().picking.expect("picking");

        let mut click = EggInput::new();
        click.mouse.left = [true, false];
        click.mouse.x = [2 * 8 + 1, 2 * 8 + 1];
        click.mouse.y = [PICK_Y + 8 + 1, PICK_Y + 8 + 1];
        ed.step_walk_editor(&mut console, &click);
        let we = ed.walk_editor.as_ref().unwrap();
        assert!(we.picking.is_none(), "assign closes the picker");
        let expect = ((prow + 1) * ed.sheet_cols() + pcol + 2) as i32;
        assert_eq!(
            we.def.walk.cells()[we.cell].frames()[we.frame].id,
            expect,
            "clicked sheet cell assigned"
        );
    }
}

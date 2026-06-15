//! A full-window raw text editor for the script DSL files (`script/en.eggtext`
//! and `script/main.eggscene`), hosted per extra view (toggled with F2; F1
//! returns to the walkaround/map editor — see the frontend's multi-window views).
//!
//! It edits the real source bytes — no structured re-emit — so comments and
//! ordering are preserved, and it covers both DSLs uniformly. Saving (Ctrl+S)
//! writes the file (the host backs it up to `*.bak`) and, if the source still
//! parses, parks it in [`pending_script`](TextEditor::pending_script) /
//! [`pending_scene`](TextEditor::pending_scene) for the host to reinstall live; a
//! parse error is reported in the status line and *not* installed, so the running
//! game keeps the last good version.
//!
//! An **outline** of the file's column-0 tags (`#dialogue`/`#list`/`#flag`, or
//! `#cutscene`, plus eggtext labels) is shown in a sidebar; clicking one jumps the
//! caret to that block. The caret/word-motion comes from the shared
//! [`TextField`](super::text_field::TextField); this module adds the multi-line
//! navigation, file I/O, outline and rendering on top.

use super::text_field::{TextField, TextOp};
use crate::data::{eggscene, eggtext};
use crate::drawstate::{DrawState, LayerId};
use crate::system::drawing::Canvas;
use crate::system::{ConsoleApi, ConsoleHelper, PrintOptions, ScanCode, just_pressed};

/// The English dialogue/text source and the cutscene source — the editor's two
/// known files (matching the startup asset loads). No host directory enumeration
/// exists, so the file switch (Ctrl+O) toggles between exactly these.
const EGGTEXT_PATH: &str = "script/en.eggtext";
const EGGSCENE_PATH: &str = "script/main.eggscene";

/// Row pitch / caret height in framebuffer px. The bitmap font is 8 px tall; 7
/// keeps lines tight without glyphs touching.
const LINE_H: i32 = 7;
/// A little breathing room from panel edges.
const PAD: i32 = 2;
/// Tab inserts this many spaces (the script files indent with spaces).
const TAB_WIDTH: usize = 2;

// Palette indices — the dock's known-good editor colours.
const C_BG: u8 = 0;
const C_TEXT: u8 = 12;
const C_DIM: u8 = 13;
const C_TAG: u8 = 14;
const C_HILITE: u8 = 11;

/// Small (condensed) text, so a cramped view framebuffer still fits a useful
/// number of columns and rows. Measuring and drawing share these options so the
/// caret lands where the glyphs do.
fn print_opts() -> PrintOptions {
    PrintOptions {
        color: 0,
        fixed: false,
        scale: 1,
        small_text: true,
    }
}

/// Which script DSL the open file is, by extension — picks the parser used to
/// validate on save and the tags surfaced in the outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptKind {
    EggText,
    EggScene,
}

/// One navigable header in the open file: a column-0 `#tag` (or eggtext label)
/// and the 0-based line it sits on.
#[derive(Debug, Clone)]
struct OutlineEntry {
    line: usize,
    label: String,
    /// The header's key (second token of a `#tag key`, or a label's name), used
    /// to resolve a [`TextAnchor::Tag`] jump.
    key: Option<String>,
}

/// Where to place the caret when the host opens a file — the Dialogue panel's
/// "edit in text editor" link jumps to a dialogue key's block.
#[derive(Debug, Clone)]
pub enum TextAnchor {
    Top,
    Line(usize),
    Tag(String),
}

/// A request — parked on the map editor's `pending_text_open` — for the host to
/// open `path` in a text view at `anchor`. Drained by the frontend's
/// `poll_text_open`, which reuses or spawns a text-mode view.
#[derive(Debug, Clone)]
pub struct TextOpenReq {
    pub path: String,
    pub anchor: TextAnchor,
}

/// A multi-line raw editor over one script file. Engine-agnostic: driven by a
/// `&mut impl ConsoleApi` and drawn into a [`DrawState`], exactly like the map
/// editor, so a host owns one per view and pumps `step`/`draw`.
#[derive(Debug, Default)]
pub struct TextEditor {
    /// The open file's path, or `None` until the first `step` lazy-loads the
    /// eggtext file.
    path: Option<String>,
    /// The text buffer + caret (a flat `String` with `'\n'`s embedded).
    field: TextField,
    /// First visible text line (vertical scroll of the body).
    scroll: usize,
    /// First visible outline entry (the sidebar scrolls independently).
    outline_scroll: usize,
    outline: Vec<OutlineEntry>,
    /// The last save / parse result, shown in the status bar.
    status: String,
    /// Unsaved edits since the last load/save.
    dirty: bool,
    /// Set after a clean eggtext save: the new source, drained by the host
    /// (parse → `Script::set_base`) so the edit reloads live.
    pub pending_script: Option<String>,
    /// Set after a clean eggscene save: the new source, drained by the host
    /// (parse → `EggState::set_scenes`).
    pub pending_scene: Option<String>,
}

impl TextEditor {
    /// Load `path` into the buffer and jump to `anchor`. Missing/invalid files
    /// open empty (the editor can still create them). Used both on first entry
    /// and by the Dialogue panel's link (via the host's `poll_text_open`).
    pub fn open(&mut self, system: &mut impl ConsoleApi, path: &str, anchor: TextAnchor) {
        let text = system
            .read_file(path)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_default();
        self.field = TextField::new(text);
        self.path = Some(path.to_string());
        self.scroll = 0;
        self.outline_scroll = 0;
        self.dirty = false;
        self.status = path.to_string();
        self.rebuild_outline();
        match anchor {
            TextAnchor::Top => self.field.move_to_line_col(0, 0),
            TextAnchor::Line(line) => self.jump_to_line(line),
            TextAnchor::Tag(key) => {
                if let Some(entry) = self.outline.iter().find(|e| e.key.as_deref() == Some(&key)) {
                    let line = entry.line;
                    self.jump_to_line(line);
                } else {
                    self.status = format!("{path}: '{key}' not found");
                }
            }
        }
    }

    /// Advance one frame: route this view's already-mapped mouse + keyboard into
    /// the buffer. `fb_w`/`fb_h` are the view's framebuffer size (cursor space),
    /// so the click regions match what [`draw`](Self::draw) lays out.
    pub fn step(&mut self, system: &mut impl ConsoleApi, fb_w: i32, fb_h: i32) {
        if self.path.is_none() {
            self.open(system, EGGTEXT_PATH, TextAnchor::Top);
        }
        let r = Regions::of(fb_w, fb_h);
        let ctrl = system.key(ScanCode::Ctrl);

        // Mouse: click to place the caret / jump via the outline; wheel scrolls
        // whichever column the cursor is over.
        let mouse = system.mouse();
        let p = mouse.pos();
        let (mx, my) = (i32::from(p.x), i32::from(p.y));
        if just_pressed(mouse.left) && my >= PAD && my < r.status_y {
            let row_in_view = ((my - PAD) / LINE_H).max(0) as usize;
            if mx < r.sidebar_w {
                if let Some(entry) = self.outline.get(self.outline_scroll + row_in_view) {
                    let line = entry.line;
                    self.jump_to_line(line);
                }
            } else if mx >= r.text_x {
                let row = self.scroll + row_in_view;
                let col = self.column_at_x(row, mx - r.text_x, system);
                self.field.move_to_line_col(row, col);
            }
        }
        let wheel = i32::from(mouse.scroll_y[0]);
        if wheel != 0 {
            if mx < r.sidebar_w {
                let max = self.outline.len().saturating_sub(1) as i32;
                self.outline_scroll =
                    (self.outline_scroll as i32 - wheel * 2).clamp(0, max) as usize;
            } else {
                let max = self.line_count().saturating_sub(1) as i32;
                self.scroll = (self.scroll as i32 - wheel * 3).clamp(0, max) as usize;
            }
        }

        // Keyboard. Ctrl+S / Ctrl+O are commands; otherwise edit.
        let mut changed = false;
        if ctrl && system.keyp(ScanCode::S) {
            self.save_and_reload(system);
        } else if ctrl && system.keyp(ScanCode::O) {
            self.switch_file(system);
        } else {
            for c in system.key_chars() {
                if !c.is_control() {
                    self.field.apply(TextOp::Push(*c));
                    changed = true;
                }
            }
            if system.keyp(ScanCode::Return) {
                self.field.apply(TextOp::Push('\n'));
                changed = true;
            }
            if system.keyp(ScanCode::Tab) {
                for _ in 0..TAB_WIDTH {
                    self.field.apply(TextOp::Push(' '));
                }
                changed = true;
            }
            // Backspace / Ctrl+Backspace / Left / Right / Ctrl+word, shared with
            // the map editor's fields; a length change means a delete happened.
            let len_before = self.field.text().len();
            self.field.edit_keys(system);
            changed |= self.field.text().len() != len_before;
            if system.keyp(ScanCode::Up) {
                self.field.apply(TextOp::Up);
            }
            if system.keyp(ScanCode::Down) {
                self.field.apply(TextOp::Down);
            }
            if system.keyp(ScanCode::Home) {
                self.field.apply(TextOp::Home);
            }
            if system.keyp(ScanCode::End) {
                self.field.apply(TextOp::End);
            }
            let page = r.visible_rows.saturating_sub(1);
            if system.keyp(ScanCode::PageUp) {
                for _ in 0..page {
                    self.field.apply(TextOp::Up);
                }
            }
            if system.keyp(ScanCode::PageDown) {
                for _ in 0..page {
                    self.field.apply(TextOp::Down);
                }
            }
        }

        if changed {
            self.dirty = true;
            self.rebuild_outline();
        }
        self.ensure_caret_visible(r.visible_rows);
    }

    /// Paint the editor into the view's BG layer (which `composite_into` blits to
    /// the framebuffer). Fills opaque first, so switching from walkaround leaves
    /// no stale world pixels behind.
    pub fn draw(&self, draw_state: &mut DrawState, system: &impl ConsoleApi) {
        let (fb_w, fb_h) = draw_state.size();
        let r = Regions::of(fb_w, fb_h);
        let opts = print_opts();

        // Resolve every palette colour before the mutable canvas borrow.
        let dim = draw_state.colour(C_DIM);
        let text_col = draw_state.colour(C_TEXT);
        let tag_col = draw_state.colour(C_TAG);
        let hilite = draw_state.colour(C_HILITE);

        draw_state.cls(LayerId::BG, C_BG);
        let canvas = draw_state.rgba(LayerId::BG);

        // Column dividers.
        canvas.fill_rect(r.sidebar_w, 0, 1, r.status_y, dim);
        canvas.fill_rect(r.text_x - 1, 0, 1, r.status_y, dim);
        canvas.fill_rect(0, r.status_y - 1, fb_w, 1, dim);

        let (cur_line, cur_col) = self.field.line_col();
        let active = self.outline.iter().rposition(|e| e.line <= cur_line);

        // Outline sidebar.
        for (idx, entry) in self
            .outline
            .iter()
            .enumerate()
            .skip(self.outline_scroll)
            .take(r.visible_rows)
        {
            let y = PAD + (idx - self.outline_scroll) as i32 * LINE_H;
            let colour = if Some(idx) == active {
                hilite
            } else {
                text_col
            };
            let label = truncate_to_width(&entry.label, r.sidebar_w - PAD * 2, system, &opts);
            system.print_to(canvas, &label, PAD, y, colour, opts.clone());
        }

        // Gutter line numbers + body text.
        let body = self.field.text();
        for (row, line) in body
            .split('\n')
            .enumerate()
            .skip(self.scroll)
            .take(r.visible_rows)
        {
            let y = PAD + (row - self.scroll) as i32 * LINE_H;
            let num = format!("{}", row + 1);
            system.print_to(canvas, &num, r.sidebar_w + PAD, y, dim, opts.clone());
            let colour = if line.starts_with('#') {
                tag_col
            } else {
                text_col
            };
            system.print_to(canvas, line, r.text_x, y, colour, opts.clone());
        }

        // Caret, when its line is on screen.
        if cur_line >= self.scroll && cur_line < self.scroll + r.visible_rows {
            let line = body.split('\n').nth(cur_line).unwrap_or("");
            let end = line
                .char_indices()
                .nth(cur_col)
                .map_or(line.len(), |(i, _)| i);
            let cx = r.text_x + system.text_width(&line[..end], opts.clone());
            let cy = PAD + (cur_line - self.scroll) as i32 * LINE_H;
            canvas.fill_rect(cx, cy, 1, LINE_H, hilite);
        }

        // Status bar.
        let path = self.path.as_deref().unwrap_or("");
        let mark = if self.dirty { "*" } else { " " };
        let bar = format!("{mark}{path}   {}   ^S save  ^O switch", self.status);
        let bar = truncate_to_width(&bar, fb_w - PAD * 2, system, &opts);
        system.print_to(canvas, &bar, PAD, r.status_y, text_col, opts);
    }

    /// Write the buffer to disk and, if it still parses, hand the new source to
    /// the host for a live reload. A parse error is surfaced in the status line
    /// and the running game keeps the last good version.
    fn save_and_reload(&mut self, system: &mut impl ConsoleApi) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let src = self.field.text().to_string();
        system.write_file(&path, src.as_bytes());
        self.dirty = false;
        match self.kind() {
            ScriptKind::EggText => match eggtext::parse(&src) {
                Ok(_) => {
                    self.pending_script = Some(src);
                    self.status = "saved & reloaded".into();
                }
                Err(e) => self.status = format!("saved — line {}: {}", e.line, e.message),
            },
            ScriptKind::EggScene => match eggscene::parse(&src) {
                Ok(_) => {
                    self.pending_scene = Some(src);
                    self.status = "saved & reloaded".into();
                }
                Err(e) => self.status = format!("saved — line {}: {}", e.line, e.message),
            },
        }
    }

    /// Toggle between the eggtext and eggscene files. Refuses while dirty so an
    /// unsaved edit can't be silently dropped.
    fn switch_file(&mut self, system: &mut impl ConsoleApi) {
        if self.dirty {
            self.status = "unsaved — Ctrl+S before switching".into();
            return;
        }
        let next = match self.kind() {
            ScriptKind::EggText => EGGSCENE_PATH,
            ScriptKind::EggScene => EGGTEXT_PATH,
        };
        self.open(system, next, TextAnchor::Top);
    }

    fn kind(&self) -> ScriptKind {
        match self.path.as_deref() {
            Some(p) if p.ends_with(".eggscene") => ScriptKind::EggScene,
            _ => ScriptKind::EggText,
        }
    }

    fn line_count(&self) -> usize {
        self.field.text().split('\n').count()
    }

    /// Move the caret to `line` and scroll it a few rows below the top for
    /// context (clamped later by [`ensure_caret_visible`](Self::ensure_caret_visible)).
    fn jump_to_line(&mut self, line: usize) {
        self.field.move_to_line_col(line, 0);
        self.scroll = line.saturating_sub(3);
    }

    /// Keep the caret's line within the visible body after an edit or jump.
    fn ensure_caret_visible(&mut self, visible_rows: usize) {
        let (line, _) = self.field.line_col();
        if line < self.scroll {
            self.scroll = line;
        } else if visible_rows > 0 && line >= self.scroll + visible_rows {
            self.scroll = line + 1 - visible_rows;
        }
    }

    /// The column whose glyph boundary is nearest `target_x` (framebuffer px from
    /// the text origin) on `row` — click-to-place-caret. O(n²) in the line length,
    /// but only run per click on a short line.
    fn column_at_x(&self, row: usize, target_x: i32, system: &impl ConsoleApi) -> usize {
        let text = self.field.text();
        let line = text.split('\n').nth(row).unwrap_or("");
        let opts = print_opts();
        let mut best = 0;
        let mut best_dist = i32::MAX;
        for (col, end) in line
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(line.len()))
            .enumerate()
        {
            let dist = (system.text_width(&line[..end], opts.clone()) - target_x).abs();
            if dist < best_dist {
                best_dist = dist;
                best = col;
            }
        }
        best
    }

    /// Rescan the buffer for column-0 headers (and eggtext labels) to drive the
    /// outline. The parsers guarantee a `#` only starts a block at column 0, so a
    /// raw line scan is reliable — no parser line-table needed.
    fn rebuild_outline(&mut self) {
        let kind = self.kind();
        let mut outline = Vec::new();
        for (i, raw) in self.field.text().split('\n').enumerate() {
            if let Some(rest) = raw.strip_prefix('#') {
                let mut words = rest.split_whitespace();
                let tag = words.next().unwrap_or("");
                let key = words.next().map(str::to_string);
                let relevant = match kind {
                    ScriptKind::EggText => matches!(tag, "dialogue" | "list" | "flag"),
                    ScriptKind::EggScene => tag == "cutscene",
                };
                if relevant {
                    let label = match &key {
                        Some(k) => format!("#{tag} {k}"),
                        None => format!("#{tag}"),
                    };
                    outline.push(OutlineEntry {
                        line: i,
                        label,
                        key,
                    });
                }
            } else if kind == ScriptKind::EggText
                && !raw.starts_with([' ', '\t'])
                && !raw.trim_start().starts_with("//")
                && let Some(eq) = raw.find('=')
            {
                let name = raw[..eq].trim();
                if !name.is_empty() {
                    outline.push(OutlineEntry {
                        line: i,
                        label: name.to_string(),
                        key: Some(name.to_string()),
                    });
                }
            }
        }
        self.outline = outline;
    }
}

/// The editor's screen split, derived once from the framebuffer size so `step`'s
/// hit-testing and `draw`'s layout stay in lock-step.
struct Regions {
    sidebar_w: i32,
    text_x: i32,
    status_y: i32,
    visible_rows: usize,
}

impl Regions {
    fn of(fb_w: i32, fb_h: i32) -> Self {
        let sidebar_w = (fb_w / 4).clamp(36, 96);
        let gutter_w = 18; // ~3 digits + padding
        let text_x = sidebar_w + gutter_w;
        let status_y = fb_h - LINE_H;
        let visible_rows = ((status_y - PAD) / LINE_H).max(0) as usize;
        Self {
            sidebar_w,
            text_x,
            status_y,
            visible_rows,
        }
    }
}

/// Drop trailing chars until `s` fits `max_w` px (used for the sidebar labels and
/// the status bar). Cheap: the strings are short.
fn truncate_to_width(s: &str, max_w: i32, system: &impl ConsoleApi, opts: &PrintOptions) -> String {
    if system.text_width(s, opts.clone()) <= max_w {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        let mut candidate = out.clone();
        candidate.push(c);
        if system.text_width(&candidate, opts.clone()) > max_w {
            break;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor_with(path: &str, src: &str) -> TextEditor {
        TextEditor {
            path: Some(path.to_string()),
            field: TextField::new(src),
            ..TextEditor::default()
        }
    }

    /// The eggtext outline surfaces `#dialogue`/`#list`/`#flag` headers and
    /// column-0 labels, skips indented `#` (mid-block) and comments, and records
    /// the right line numbers + keys.
    #[test]
    fn outline_eggtext_tags_and_labels() {
        let src = "\
// a comment
title = Hello
#flag seen_intro

#dialogue lamp
  It glows.
  #pic none

#list names
  one
  two";
        let mut ed = editor_with("script/en.eggtext", src);
        ed.rebuild_outline();
        let got: Vec<(usize, &str, Option<&str>)> = ed
            .outline
            .iter()
            .map(|e| (e.line, e.label.as_str(), e.key.as_deref()))
            .collect();
        assert_eq!(
            got,
            vec![
                (1, "title", Some("title")),
                (2, "#flag seen_intro", Some("seen_intro")),
                (4, "#dialogue lamp", Some("lamp")),
                (8, "#list names", Some("names")),
            ],
            "indented `#pic` (line 6) is mid-block, not an outline entry"
        );
    }

    /// The eggscene outline lists only `#cutscene` headers (and never treats a
    /// `key = value` line as a label — that's eggtext-only).
    #[test]
    fn outline_eggscene_cutscenes_only() {
        let src = "\
#cutscene intro
  wait 30
  dialogue lamp

#cutscene outro
  music none";
        let mut ed = editor_with("script/main.eggscene", src);
        ed.rebuild_outline();
        let got: Vec<(usize, Option<&str>)> = ed
            .outline
            .iter()
            .map(|e| (e.line, e.key.as_deref()))
            .collect();
        assert_eq!(got, vec![(0, Some("intro")), (4, Some("outro"))]);
    }

    /// A `Tag` anchor parks the caret on the matching header line.
    #[test]
    fn tag_anchor_jumps_via_outline() {
        let src = "#dialogue a\n  one\n#dialogue b\n  two";
        let mut ed = editor_with("script/en.eggtext", src);
        ed.rebuild_outline();
        // Resolve the jump the way `open` does, without a console.
        let line = ed
            .outline
            .iter()
            .find(|e| e.key.as_deref() == Some("b"))
            .map(|e| e.line)
            .expect("key b in outline");
        ed.jump_to_line(line);
        assert_eq!(ed.field.line_col(), (2, 0));
    }

    /// `step` then `draw` against a real framebuffer-sized canvas must not panic —
    /// exercises the caret-prefix slicing, the scroll / visible-row math and the
    /// outline + gutter rendering on a multi-line buffer. (No GUI here; this guards
    /// the hot paths the outline unit tests don't reach.)
    #[test]
    fn step_and_draw_dont_panic() {
        use crate::system::test_console::TestConsole;

        let mut console = TestConsole::new();
        let mut draw = DrawState::default();
        draw.resize(240, 136);

        let mut ed = editor_with(
            "script/en.eggtext",
            "#dialogue greet\n  Hello there!\n  A second line.\n\n#list names\n  one\n  two\n",
        );
        ed.rebuild_outline();

        // Idle step (mouse still, no keys) then a draw with the caret at the top.
        ed.step(&mut console, 240, 136);
        ed.draw(&mut draw, &console);

        // Drive the caret past the last line and to a far column, scroll to follow,
        // then draw — the caret-on-screen branch and the end-of-buffer clamps.
        for _ in 0..20 {
            ed.field.apply(TextOp::Down);
        }
        ed.field.apply(TextOp::End);
        ed.ensure_caret_visible(Regions::of(240, 136).visible_rows);
        ed.draw(&mut draw, &console);

        // The minimum framebuffer (a very narrow text column) is also safe to draw.
        let mut small = DrawState::default();
        small.resize(64, 48);
        ed.draw(&mut small, &console);
    }
}

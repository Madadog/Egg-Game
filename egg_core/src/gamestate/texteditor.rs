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

use super::text_field::{REPEAT_DELAY, REPEAT_RATE, TextEvent, TextField, TextOp};
use crate::data::{eggscene, eggtext};
use crate::drawstate::{DrawState, LayerId};
use crate::system::drawing::Canvas;
use crate::system::{ConsoleApi, ConsoleHelper, PrintOptions, ScanCode, just_pressed, pressed};

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
/// Selection background — a dark blue (Sweetie-16 #8) that white body text still
/// reads clearly over, kept distinct from the cyan caret/active-outline hilite.
const C_SEL: u8 = 8;

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

/// Which one-line prompt is open over the editor — the shared
/// [`TextField`](super::text_field::TextField) input is read the same way for
/// both; only what Enter does differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptKind {
    /// Incremental case-insensitive search (Ctrl+F); Enter = next, Shift+Enter
    /// = previous, both wrapping.
    Find,
    /// Jump to a 1-based line number (Ctrl+G), clamped to the file.
    GoTo,
}

/// A modal one-line prompt (find / go-to-line) layered over the editor. While
/// one is open it swallows all keystrokes via its `input` field; Enter acts,
/// Escape closes.
#[derive(Debug)]
struct Prompt {
    kind: PromptKind,
    input: TextField,
    /// The caret byte when the prompt opened — incremental find searches from
    /// here so the first match is the nearest one ahead of where you were.
    origin: usize,
    /// The query the last search ran on, to fire an incremental search only when
    /// the text actually changes.
    last_query: String,
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
    /// Horizontal scroll of the body, in character columns: long lines are drawn
    /// starting from this column so the caret stays visible past the right edge.
    /// The gutter and sidebar don't move. Driven by caret-follow and Shift+wheel.
    h_scroll: usize,
    /// True while the left mouse button is held after a press that began in the
    /// text body, so motion extends the selection (drag-select). Cleared on
    /// release.
    dragging: bool,
    /// First visible outline entry (the sidebar scrolls independently).
    outline_scroll: usize,
    outline: Vec<OutlineEntry>,
    /// The last save / parse result, shown in the status bar.
    status: String,
    /// Unsaved edits since the last load/save.
    dirty: bool,
    /// Undo / redo stacks of `(text, cursor)` snapshots. `mid_edit` is true while
    /// inside a coalescing edit group, so a run of typing/deleting collapses to a
    /// single undo step; navigation / whitespace / a save close the group.
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    mid_edit: bool,
    /// Set after a clean eggtext save: the new source, drained by the host
    /// (parse → `Script::set_base`) so the edit reloads live.
    pub pending_script: Option<String>,
    /// Set after a clean eggscene save: the new source, drained by the host
    /// (parse → `EggState::set_scenes`).
    pub pending_scene: Option<String>,
    /// The open find / go-to-line prompt, if any. While `Some`, it intercepts
    /// all keyboard input (the editor body is read-only until it closes).
    prompt: Option<Prompt>,
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
        self.h_scroll = 0;
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

    const UNDO_CAP: usize = 256;

    /// `(text, cursor)` snapshot of the buffer.
    fn snapshot(&self) -> (String, usize) {
        (self.field.text().to_string(), self.field.cursor())
    }

    /// Open a coalescing undo group unless one is already open: push the pre-edit
    /// snapshot and clear redo. Call right before each buffer edit so a run shares
    /// one undo step; whitespace / navigation / a save end the group.
    fn checkpoint(&mut self) {
        if self.mid_edit {
            return;
        }
        self.undo.push(self.snapshot());
        if self.undo.len() > Self::UNDO_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.mid_edit = true;
    }

    fn restore(&mut self, snap: (String, usize)) {
        let (text, cursor) = snap;
        self.field = TextField::new(text);
        self.field.set_cursor(cursor);
        self.dirty = true;
        self.mid_edit = false;
        self.rebuild_outline();
    }

    /// Undo the last edit group (Ctrl+Z); the current state goes onto the redo
    /// stack so it can be reapplied.
    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(prev);
        }
    }

    /// Redo (Ctrl+Y / Ctrl+Shift+Z).
    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(next);
        }
    }

    /// Snapshot the buffer as its own undo step, closing any open coalescing
    /// group first — for a discrete command (cut / paste / a line op) that should
    /// never merge into a neighbouring run of typing.
    fn checkpoint_discrete(&mut self) {
        self.mid_edit = false;
        self.checkpoint();
    }

    /// Copy the selection to the clipboard, or — with no selection — the whole
    /// current line plus a trailing newline (so a bare Ctrl+C grabs the line and
    /// pasting it re-inserts a full line).
    fn copy(&mut self, system: &mut impl ConsoleApi) {
        let text = if self.field.selection().is_some() {
            self.field.selected_text().to_string()
        } else {
            let (s, e) = self.field.current_line_span();
            format!("{}\n", &self.field.text()[s..e])
        };
        system.clipboard_set(&text);
    }

    /// Cut: copy, then delete. With a selection that's the selection; with none
    /// it's the whole current line including a newline, removed cleanly (the
    /// trailing newline, or the preceding one on the last line) so no blank line
    /// is left behind. The caller checkpoints first.
    fn cut(&mut self, system: &mut impl ConsoleApi) {
        if self.field.selection().is_some() {
            system.clipboard_set(self.field.selected_text());
            self.field.delete_selection();
        } else {
            let (s, e) = self.field.current_line_span();
            system.clipboard_set(&format!("{}\n", &self.field.text()[s..e]));
            let len = self.field.text().len();
            if e < len {
                self.field.delete_range(s, e + 1); // line + its trailing newline
            } else if s > 0 {
                self.field.delete_range(s - 1, e); // last line: take the newline before
            } else {
                self.field.delete_range(s, e); // the only line
            }
        }
    }

    /// Paste the clipboard at the caret, replacing any selection. The caller
    /// checkpoints first. No-op when the clipboard is empty.
    fn paste(&mut self, system: &mut impl ConsoleApi) {
        if let Some(text) = system.clipboard_get() {
            self.field.delete_selection();
            self.field.insert_str(&text);
        }
    }

    /// Open a find / go-to-line prompt. Find seeds its query with the current
    /// selection (so "select a word, Ctrl+F" searches for it), anchored at the
    /// caret so the first incremental match is the nearest one ahead.
    fn open_prompt(&mut self, kind: PromptKind) {
        let seed = match kind {
            PromptKind::Find => self.field.selected_text().to_string(),
            PromptKind::GoTo => String::new(),
        };
        self.prompt = Some(Prompt {
            kind,
            input: TextField::new(seed),
            origin: self.field.cursor(),
            last_query: String::new(),
        });
    }

    /// Drive the open prompt for one frame: feed the keystroke into its input
    /// field and act on the result. Find searches incrementally as the query
    /// changes (Enter / Shift+Enter step to the next / previous match, wrapping);
    /// go-to jumps on Enter. Escape closes either. `shift` is this frame's Shift.
    fn step_prompt(&mut self, system: &mut impl ConsoleApi, shift: bool) {
        let Some(mut prompt) = self.prompt.take() else {
            return;
        };
        let event = prompt.input.step(system);
        let query = prompt.input.text().to_string();
        match prompt.kind {
            PromptKind::Find => match event {
                TextEvent::Cancel => {} // dropped prompt stays closed
                TextEvent::Commit => {
                    // Enter = next match, Shift+Enter = previous; the bar stays open.
                    if !query.is_empty() {
                        let from = match (shift, self.field.selection()) {
                            (true, Some((s, _))) => s, // search back from the match start
                            (true, None) => self.field.cursor(),
                            (false, _) => self.field.cursor(), // forward from the match end
                        };
                        if let Some((s, e)) = self.search(&query, from, shift) {
                            self.field.select(s, e);
                        }
                    }
                    prompt.last_query = query;
                    self.prompt = Some(prompt);
                }
                TextEvent::Active => {
                    if query != prompt.last_query {
                        match self.search(&query, prompt.origin, false) {
                            Some((s, e)) => self.field.select(s, e),
                            None => self.field.clear_selection(),
                        }
                        prompt.last_query = query;
                    }
                    self.prompt = Some(prompt);
                }
            },
            PromptKind::GoTo => match event {
                TextEvent::Cancel => {}
                TextEvent::Commit => {
                    if let Some(line) = self.goto_target(&query) {
                        self.jump_to_line(line);
                    }
                }
                TextEvent::Active => self.prompt = Some(prompt),
            },
        }
    }

    /// The 0-based line a go-to-line query resolves to: parse its 1-based number
    /// and clamp into the file. `None` when it doesn't parse.
    fn goto_target(&self, query: &str) -> Option<usize> {
        query
            .trim()
            .parse::<usize>()
            .ok()
            .map(|n| n.saturating_sub(1).min(self.line_count().saturating_sub(1)))
    }

    /// Find `query` case-insensitively, returning the matched byte range. Forward
    /// search takes the first match at/after `from`; reverse takes the last match
    /// before `from`; both wrap once around the buffer if nothing is found on the
    /// first pass. (Lowercasing is ASCII-exact for the script files; the bounds
    /// are snapped when applied via [`TextField::select`], so an exotic-Unicode
    /// length shift can't panic.)
    fn search(&self, query: &str, from: usize, reverse: bool) -> Option<(usize, usize)> {
        let hay = self.field.text();
        if query.is_empty() || hay.is_empty() {
            return None;
        }
        let hay = hay.to_lowercase();
        let needle = query.to_lowercase();
        let from = from.min(hay.len());
        if reverse {
            hay[..from]
                .rfind(&needle)
                .or_else(|| hay.rfind(&needle))
                .map(|i| (i, i + needle.len()))
        } else {
            match hay[from..].find(&needle) {
                Some(i) => Some((from + i, from + i + needle.len())),
                None => hay.find(&needle).map(|i| (i, i + needle.len())),
            }
        }
    }

    /// 0-based line index containing byte `byte`.
    fn line_of_byte(&self, byte: usize) -> usize {
        let text = self.field.text();
        text[..byte.min(text.len())].matches('\n').count()
    }

    /// Byte offset of the start of line `line` (clamped to the buffer).
    fn line_start_byte(&self, line: usize) -> usize {
        self.field
            .text()
            .split('\n')
            .take(line)
            .map(|l| l.len() + 1)
            .sum()
    }

    /// Byte offset of the end of line `line` (just before its `'\n'`, or the
    /// buffer end on the last line).
    fn line_end_byte(&self, line: usize) -> usize {
        let start = self.line_start_byte(line);
        let text = self.field.text();
        text[start..].find('\n').map_or(text.len(), |i| start + i)
    }

    /// The leading whitespace of the caret's line — what Enter carries onto the
    /// new line (auto-indent).
    fn current_indent(&self) -> String {
        let (s, e) = self.field.current_line_span();
        self.field.text()[s..e]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }

    /// Insert a newline that carries the current line's leading whitespace onto
    /// the new line (auto-indent), replacing any selection first. The caller
    /// checkpoints.
    fn newline_autoindent(&mut self) {
        self.field.delete_selection();
        let indent = self.current_indent();
        self.field.apply(TextOp::Push('\n'));
        self.field.insert_str(&indent);
    }

    /// Smart Home: jump to the first non-whitespace character of the line, or —
    /// when already there — to column 0, toggling between the two. `extend`
    /// carries a Shift-selection along.
    fn smart_home(&mut self, extend: bool) {
        let (s, e) = self.field.current_line_span();
        let indent: usize = self.field.text()[s..e]
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(char::len_utf8)
            .sum();
        let first_non_ws = s + indent;
        if self.field.cursor() == first_non_ws {
            // Already at the first non-blank — toggle to column 0 (plain Home).
            self.field.move_caret(TextOp::Home, extend);
        } else {
            self.field.move_to_byte(first_non_ws, extend);
        }
    }

    /// The inclusive 0-based line range a Tab/Shift+Tab affects: the selection's
    /// span, or the caret's line when there's no selection. A selection ending
    /// exactly at a line's column 0 doesn't pull that line in.
    fn indent_line_range(&self) -> (usize, usize) {
        let Some((s, e)) = self.field.selection() else {
            let l = self.field.line_col().0;
            return (l, l);
        };
        let first = self.line_of_byte(s);
        let mut last = self.line_of_byte(e);
        if last > first && e == self.line_start_byte(last) {
            last -= 1;
        }
        (first, last)
    }

    /// Indent each line in `[first, last]` by `TAB_WIDTH` spaces (skipping blank
    /// lines so none gain trailing whitespace), then re-select the block so Tab
    /// can be pressed repeatedly. Processes bottom-up so earlier offsets hold.
    fn indent_lines(&mut self, first: usize, last: usize) {
        let pad = " ".repeat(TAB_WIDTH);
        for line in (first..=last).rev() {
            let start = self.line_start_byte(line);
            if self.line_end_byte(line) > start {
                self.field.set_cursor(start);
                self.field.insert_str(&pad);
            }
        }
        self.reselect_lines(first, last);
    }

    /// Dedent each line in `[first, last]`: strip a leading tab, or up to
    /// `TAB_WIDTH` leading spaces. Bottom-up, then re-select the block.
    fn dedent_lines(&mut self, first: usize, last: usize) {
        for line in (first..=last).rev() {
            let start = self.line_start_byte(line);
            let w = self.dedent_width_at(start);
            if w > 0 {
                self.field.delete_range(start, start + w);
            }
        }
        self.reselect_lines(first, last);
    }

    /// How many leading bytes a dedent strips at line-start byte `start`: a single
    /// tab, else up to `TAB_WIDTH` spaces.
    fn dedent_width_at(&self, start: usize) -> usize {
        let rest = &self.field.text()[start..];
        if rest.starts_with('\t') {
            1
        } else {
            rest.chars()
                .take(TAB_WIDTH)
                .take_while(|c| *c == ' ')
                .count()
        }
    }

    /// Select whole lines `[first, last]` after an indent/dedent, so the block
    /// stays highlighted for a repeated Tab.
    fn reselect_lines(&mut self, first: usize, last: usize) {
        self.field
            .select(self.line_start_byte(first), self.line_end_byte(last));
    }

    /// Dedent just the caret's line (no-selection Shift+Tab), shifting the caret
    /// left by however much indentation was removed (never past the line start).
    fn dedent_current_line(&mut self) {
        let line = self.field.line_col().0;
        let start = self.line_start_byte(line);
        let w = self.dedent_width_at(start);
        if w > 0 {
            let cursor = self.field.cursor();
            self.field.delete_range(start, start + w);
            self.field.set_cursor(cursor.saturating_sub(w).max(start));
        }
    }

    /// Duplicate the caret's line, inserting the copy just below it and moving the
    /// caret onto the copy at the same column (Ctrl+D).
    fn duplicate_line(&mut self) {
        self.field.clear_selection();
        let (s, e) = self.field.current_line_span();
        let col_off = self.field.cursor() - s;
        let line = self.field.text()[s..e].to_string();
        self.field.set_cursor(e);
        self.field.insert_str(&format!("\n{line}"));
        self.field.set_cursor(e + 1 + col_off); // same column, on the copy
    }

    /// Delete the caret's line and one newline — the trailing one, or the
    /// preceding one on the last line — so no blank line is left (Ctrl+Shift+K).
    /// The caret keeps its column on whatever line takes the slot.
    fn delete_line(&mut self) {
        self.field.clear_selection();
        let (s, e) = self.field.current_line_span();
        let col_off = self.field.cursor() - s;
        let len = self.field.text().len();
        let landing = if e < len { s } else { s.saturating_sub(1) };
        if e < len {
            self.field.delete_range(s, e + 1); // line + trailing newline
        } else if s > 0 {
            self.field.delete_range(s - 1, e); // last line: preceding newline
        } else {
            self.field.delete_range(s, e); // the only line → empty buffer
        }
        let line_end = self.line_end_byte(self.line_of_byte(landing));
        self.field.set_cursor((landing + col_off).min(line_end));
    }

    /// Swap the caret's line with its neighbour (Alt+Up / Alt+Down), carrying the
    /// caret to the moved line at the same column. A no-op at the top/bottom edge.
    fn move_line(&mut self, down: bool) {
        let (line, col) = self.field.line_col();
        let count = self.line_count();
        let target = if down { line + 1 } else { line.wrapping_sub(1) };
        if (down && line + 1 >= count) || (!down && line == 0) {
            return;
        }
        let new_text = {
            let mut lines: Vec<&str> = self.field.text().split('\n').collect();
            lines.swap(line, target);
            lines.join("\n")
        };
        self.field = TextField::new(new_text);
        self.field.move_to_line_col(target, col);
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
        // Shift extends a selection (Shift+arrow / Shift+click); read once for the
        // mouse and keyboard handling below.
        let shift = system.key(ScanCode::Shift);

        // A modal find / go-to-line prompt swallows all input until it closes.
        if self.prompt.is_some() {
            self.step_prompt(system, shift);
            self.ensure_caret_visible(r.visible_rows);
            self.ensure_caret_visible_h(system, fb_w - r.text_x - PAD);
            return;
        }

        // Closes the current coalescing undo group so a run of typing/deleting is
        // one undo step: set by navigation (click / arrows / paging) or by a
        // whitespace insert.
        let mut boundary = false;

        // Mouse: a click places the caret (Shift-click / drag extends the
        // selection); the outline jumps; the wheel scrolls whichever column the
        // cursor is over.
        let mouse = system.mouse();
        let p = mouse.pos();
        let (mx, my) = (i32::from(p.x), i32::from(p.y));
        if just_pressed(mouse.left) && my >= PAD && my < r.status_y {
            boundary = true;
            let row_in_view = ((my - PAD) / LINE_H).max(0) as usize;
            if mx < r.sidebar_w {
                if let Some(entry) = self.outline.get(self.outline_scroll + row_in_view) {
                    let line = entry.line;
                    self.jump_to_line(line);
                }
            } else if mx >= r.text_x {
                let row = self.scroll + row_in_view;
                let col = self.column_at_x(row, mx - r.text_x, system);
                if shift {
                    self.field.extend_to_line_col(row, col);
                } else {
                    self.field.move_to_line_col(row, col);
                    self.field.anchor_here();
                }
                self.dragging = true; // a body press may become a drag-select
            }
        } else if self.dragging && pressed(mouse.left) {
            // Drag in progress: extend the selection to the mouse, clamped to the
            // body's rows/columns when it strays past an edge.
            let row_in_view = ((my - PAD).max(0) / LINE_H) as usize;
            let row = (self.scroll + row_in_view).min(self.line_count().saturating_sub(1));
            let col = self.column_at_x(row, (mx - r.text_x).max(0), system);
            self.field.extend_to_line_col(row, col);
        }
        if !pressed(mouse.left) {
            self.dragging = false;
        }
        let wheel = i32::from(mouse.scroll_y[0]);
        if wheel != 0 {
            if mx < r.sidebar_w {
                let max = self.outline.len().saturating_sub(1) as i32;
                self.outline_scroll =
                    (self.outline_scroll as i32 - wheel * 2).clamp(0, max) as usize;
            } else if shift {
                // Shift+wheel scrolls the body horizontally (the caret-follow can
                // pull it back, exactly as a vertical wheel interacts with the caret).
                let max = self.max_line_cols() as i32;
                self.h_scroll = (self.h_scroll as i32 - wheel * 3).clamp(0, max) as usize;
            } else {
                let max = self.line_count().saturating_sub(1) as i32;
                self.scroll = (self.scroll as i32 - wheel * 3).clamp(0, max) as usize;
            }
        }

        // Keyboard. Ctrl-chords are commands (clipboard, undo, save, …); otherwise
        // typed text / navigation. Selection-aware: typing and a delete key
        // replace any selection, and Shift+motion extends it. Alt+Up/Down move the
        // current line.
        let alt = system.key(ScanCode::Alt);
        let mut changed = false;
        if ctrl && system.keyp(ScanCode::S) {
            self.save_and_reload(system);
            self.mid_edit = false; // a save closes the current undo group
        } else if ctrl && system.keyp(ScanCode::O) {
            self.switch_file(system);
        } else if ctrl && system.keyp(ScanCode::F) {
            self.open_prompt(PromptKind::Find);
        } else if ctrl && system.keyp(ScanCode::G) {
            self.open_prompt(PromptKind::GoTo);
        } else if ctrl && system.keyp(ScanCode::A) {
            self.field.select_all();
            boundary = true;
        } else if ctrl && system.keyp(ScanCode::C) {
            self.copy(system);
            boundary = true;
        } else if ctrl && system.keyp(ScanCode::X) {
            self.checkpoint_discrete();
            self.cut(system);
            changed = true;
            boundary = true;
        } else if ctrl && system.keyp(ScanCode::V) {
            self.checkpoint_discrete();
            self.paste(system);
            changed = true;
            boundary = true;
        } else if ctrl && shift && system.keyp(ScanCode::K) {
            self.checkpoint_discrete();
            self.delete_line();
            changed = true;
            boundary = true;
        } else if ctrl && system.keyp(ScanCode::D) {
            self.checkpoint_discrete();
            self.duplicate_line();
            changed = true;
            boundary = true;
        } else if ctrl && system.key_repeat(ScanCode::Z, REPEAT_DELAY, REPEAT_RATE) {
            // Ctrl+Z undo, Ctrl+Shift+Z redo (both repeat while held).
            if shift {
                self.redo();
            } else {
                self.undo();
            }
        } else if ctrl && system.key_repeat(ScanCode::Y, REPEAT_DELAY, REPEAT_RATE) {
            self.redo();
        } else {
            // Typed text — replaces any selection; a whitespace insert closes the
            // undo group, so each word is its own undo step.
            for c in system.key_chars() {
                if !c.is_control() {
                    self.checkpoint();
                    self.field.edit(TextOp::Push(*c));
                    changed = true;
                    boundary |= c.is_whitespace();
                }
            }
            // Navigation + edits auto-repeat while held (newlines, indents, caret
            // glide, paging); the cadence is the shared text-entry one.
            if system.key_repeat(ScanCode::Return, REPEAT_DELAY, REPEAT_RATE) {
                self.checkpoint();
                self.newline_autoindent(); // carries the line's leading whitespace
                changed = true;
                boundary = true;
            }
            if system.key_repeat(ScanCode::Tab, REPEAT_DELAY, REPEAT_RATE) {
                self.checkpoint();
                if self.field.selection().is_some() {
                    // A selection indents / dedents every line it covers.
                    let (first, last) = self.indent_line_range();
                    if shift {
                        self.dedent_lines(first, last);
                    } else {
                        self.indent_lines(first, last);
                    }
                } else if shift {
                    // Shift+Tab with no selection dedents the current line.
                    self.dedent_current_line();
                } else {
                    // Tab with no selection inserts spaces at the caret.
                    for _ in 0..TAB_WIDTH {
                        self.field.apply(TextOp::Push(' '));
                    }
                }
                changed = true;
            }
            // Backspace / Delete / Ctrl+word-delete + Left / Right (shared with the
            // map editor's fields, and selection-aware). Checkpoint before a
            // delete; a length change means one happened.
            if system.key_repeat(ScanCode::Backspace, REPEAT_DELAY, REPEAT_RATE)
                || system.key_repeat(ScanCode::Delete, REPEAT_DELAY, REPEAT_RATE)
            {
                self.checkpoint();
            }
            let len_before = self.field.text().len();
            self.field.edit_keys(system);
            changed |= self.field.text().len() != len_before;
            if system.key_repeat(ScanCode::Up, REPEAT_DELAY, REPEAT_RATE) {
                if alt {
                    self.checkpoint_discrete();
                    self.move_line(false);
                    changed = true;
                } else {
                    self.field.move_caret(TextOp::Up, shift);
                }
                boundary = true;
            }
            if system.key_repeat(ScanCode::Down, REPEAT_DELAY, REPEAT_RATE) {
                if alt {
                    self.checkpoint_discrete();
                    self.move_line(true);
                    changed = true;
                } else {
                    self.field.move_caret(TextOp::Down, shift);
                }
                boundary = true;
            }
            if system.key_repeat(ScanCode::Home, REPEAT_DELAY, REPEAT_RATE) {
                self.smart_home(shift);
                boundary = true;
            }
            if system.key_repeat(ScanCode::End, REPEAT_DELAY, REPEAT_RATE) {
                self.field.move_caret(TextOp::End, shift);
                boundary = true;
            }
            let page = r.visible_rows.saturating_sub(1);
            if system.key_repeat(ScanCode::PageUp, REPEAT_DELAY, REPEAT_RATE) {
                for _ in 0..page {
                    self.field.move_caret(TextOp::Up, shift);
                }
                boundary = true;
            }
            if system.key_repeat(ScanCode::PageDown, REPEAT_DELAY, REPEAT_RATE) {
                for _ in 0..page {
                    self.field.move_caret(TextOp::Down, shift);
                }
                boundary = true;
            }
        }

        if changed {
            self.dirty = true;
            self.rebuild_outline();
        }
        if boundary {
            self.mid_edit = false; // close the coalescing group at a boundary
        }
        self.ensure_caret_visible(r.visible_rows);
        self.ensure_caret_visible_h(system, fb_w - r.text_x - PAD);
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
        let sel_col = draw_state.colour(C_SEL);

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

        // Gutter line numbers + body text, scrolled horizontally by `h_scroll`
        // (each line is drawn from that column on; the gutter stays fixed), with
        // the selection highlighted behind the glyphs on every row it covers.
        // TODO: word-wrap — long lines run off the right edge and are reached by
        // horizontal scrolling for now; wrapping them is a future option.
        let body = self.field.text();
        let sel = self.field.selection();
        // Byte offset of the first visible line, then advanced one line at a time
        // (each line is its text plus the `'\n'` that follows it).
        let mut byte = body
            .split('\n')
            .take(self.scroll)
            .map(|l| l.len() + 1)
            .sum::<usize>();
        for (row, line) in body
            .split('\n')
            .enumerate()
            .skip(self.scroll)
            .take(r.visible_rows)
        {
            let y = PAD + (row - self.scroll) as i32 * LINE_H;
            let num = format!("{}", row + 1);
            system.print_to(canvas, &num, r.sidebar_w + PAD, y, dim, opts.clone());

            // First visible byte of this line under the horizontal scroll.
            let hb = byte_at_col(line, self.h_scroll);

            // Selection: intersect it with this line's byte span (its trailing
            // newline included, so a selection through it shows a short tail past
            // the text — a blank selected line still reads as selected). Offsets
            // are measured from `hb`, and anything scrolled off to the left clips.
            if let Some((s, e)) = sel {
                let line_end = byte + line.len();
                let lo = s.clamp(byte, line_end);
                let hi = e.clamp(byte, line_end + 1);
                if lo < hi {
                    let left = (lo - byte).max(hb);
                    let right = (hi.min(line_end) - byte).max(hb);
                    let x0 = r.text_x + system.text_width(&line[hb..left], opts.clone());
                    let x1 = r.text_x + system.text_width(&line[hb..right], opts.clone());
                    let tail = if hi > line_end { 3 } else { 0 };
                    let w = x1 - x0 + tail;
                    if w > 0 {
                        canvas.fill_rect(x0, y, w, LINE_H, sel_col);
                    }
                }
            }

            let colour = if line.starts_with('#') {
                tag_col
            } else {
                text_col
            };
            system.print_to(canvas, &line[hb..], r.text_x, y, colour, opts.clone());
            byte += line.len() + 1;
        }

        // Caret, when its line is on screen and not scrolled off to the left.
        if cur_line >= self.scroll
            && cur_line < self.scroll + r.visible_rows
            && cur_col >= self.h_scroll
        {
            let line = body.split('\n').nth(cur_line).unwrap_or("");
            let hb = byte_at_col(line, self.h_scroll);
            let end = byte_at_col(line, cur_col);
            let cx = r.text_x + system.text_width(&line[hb..end], opts.clone());
            let cy = PAD + (cur_line - self.scroll) as i32 * LINE_H;
            canvas.fill_rect(cx, cy, 1, LINE_H, hilite);
        }

        // Find / go-to-line prompt bar, one row above the status bar when open —
        // a filled strip so it reads as a modal input over the body text.
        if let Some(prompt) = &self.prompt {
            let py = r.status_y - LINE_H;
            canvas.fill_rect(0, py, fb_w, LINE_H, sel_col);
            let label = match prompt.kind {
                PromptKind::Find => "Find",
                PromptKind::GoTo => "Go to line",
            };
            let bar = format!("{label}: {}", prompt.input.display());
            let bar = truncate_to_width(&bar, fb_w - PAD * 2, system, &opts);
            system.print_to(canvas, &bar, PAD, py, text_col, opts.clone());
        }

        // Status bar.
        let path = self.path.as_deref().unwrap_or("");
        let mark = if self.dirty { "*" } else { " " };
        let hint = if self.prompt.is_some() {
            "Enter find  Esc close"
        } else {
            "^S save  ^O switch  ^F find  ^G goto"
        };
        let bar = format!("{mark}{path}   {}   {hint}", self.status);
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

    /// Keep the caret's column within the body horizontally: scroll left if it's
    /// behind `h_scroll`, or right until its measured x fits `text_area_w` px.
    /// `text_area_w` is the body's pixel width (framebuffer minus the gutter).
    fn ensure_caret_visible_h(&mut self, system: &impl ConsoleApi, text_area_w: i32) {
        let (line_idx, col) = self.field.line_col();
        if col <= self.h_scroll {
            self.h_scroll = col;
            return;
        }
        let opts = print_opts();
        // Owned copy so the loop can mutate `self.h_scroll` without borrowing.
        let line = self
            .field
            .text()
            .split('\n')
            .nth(line_idx)
            .unwrap_or("")
            .to_string();
        let cb = byte_at_col(&line, col);
        while self.h_scroll < col {
            let hb = byte_at_col(&line, self.h_scroll);
            if system.text_width(&line[hb..cb], opts.clone()) <= text_area_w {
                break;
            }
            self.h_scroll += 1;
        }
    }

    /// The longest line's length in characters — the horizontal scroll bound.
    fn max_line_cols(&self) -> usize {
        self.field
            .text()
            .split('\n')
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0)
    }

    /// The column whose glyph boundary is nearest `target_x` (px from the text
    /// origin) on `row` — click-to-place-caret. Measured from the first visible
    /// column, so it's correct under horizontal scroll. O(n²) in the line length,
    /// but only run per click on a short line.
    fn column_at_x(&self, row: usize, target_x: i32, system: &impl ConsoleApi) -> usize {
        let text = self.field.text();
        let line = text.split('\n').nth(row).unwrap_or("");
        let opts = print_opts();
        let hb = byte_at_col(line, self.h_scroll);
        let mut best = self.h_scroll;
        let mut best_dist = i32::MAX;
        for (offset, end) in line[hb..]
            .char_indices()
            .map(|(i, _)| hb + i)
            .chain(std::iter::once(line.len()))
            .enumerate()
        {
            let dist = (system.text_width(&line[hb..end], opts.clone()) - target_x).abs();
            if dist < best_dist {
                best_dist = dist;
                best = self.h_scroll + offset;
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
/// The byte offset of character column `col` within `line` (the line end when
/// `col` runs past it). Maps a horizontal scroll / caret column to a byte index
/// for slicing a visible line prefix.
fn byte_at_col(line: &str, col: usize) -> usize {
    line.char_indices().nth(col).map_or(line.len(), |(i, _)| i)
}

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

        // The content includes non-ASCII whose low byte is ≥ 128 (`→` U+2192,
        // `é` U+00E9) — these used to overflow the small-text glyph map and panic
        // when drawn (the Ctrl+O-to-`main.eggscene` crash).
        let mut ed = editor_with(
            "script/en.eggtext",
            "#dialogue greet\n  Hello there! café\n  registry → build →\n\n#list names\n  one\n  two\n",
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

        // With a find prompt open, the prompt-bar strip also draws cleanly.
        ed.open_prompt(PromptKind::Find);
        ed.draw(&mut draw, &console);
        ed.draw(&mut small, &console);

        // Horizontally scrolled (long lines sliced from a mid-line column) draws
        // without slicing panics, both with and without a selection.
        ed.prompt = None;
        ed.h_scroll = 4;
        ed.field.select(0, ed.field.text().len());
        ed.draw(&mut draw, &console);
        ed.draw(&mut small, &console);
    }

    /// Undo/redo coalesces a run of typing into one step, broken at whitespace, and
    /// redo replays it. (`checkpoint`/`mid_edit` are what `step` drives per frame;
    /// here we drive them directly — a space closes the group.)
    #[test]
    fn undo_redo_coalesces_words() {
        let mut ed = TextEditor::default();
        for c in "ab".chars() {
            ed.checkpoint();
            ed.field.apply(TextOp::Push(c));
        }
        ed.mid_edit = false;
        ed.checkpoint();
        ed.field.apply(TextOp::Push(' '));
        ed.mid_edit = false;
        for c in "cd".chars() {
            ed.checkpoint();
            ed.field.apply(TextOp::Push(c));
        }
        assert_eq!(ed.field.text(), "ab cd");

        ed.undo();
        assert_eq!(ed.field.text(), "ab ");
        ed.undo();
        assert_eq!(ed.field.text(), "ab");
        ed.undo();
        assert_eq!(ed.field.text(), "");
        ed.undo();
        assert_eq!(ed.field.text(), "", "nothing left to undo is a no-op");

        ed.redo();
        ed.redo();
        ed.redo();
        assert_eq!(ed.field.text(), "ab cd", "redo replays every group");
    }

    /// Copy/cut/paste round-trip through the console clipboard: a selection is
    /// copied verbatim, paste drops it at the caret (replacing any selection).
    #[test]
    fn clipboard_copy_paste_selection() {
        use crate::system::test_console::TestConsole;
        let mut console = TestConsole::new();

        let mut ed = editor_with("script/en.eggtext", "alpha beta gamma");
        ed.field.select(6, 10); // "beta"
        ed.copy(&mut console);
        assert_eq!(console.clipboard.as_deref(), Some("beta"));

        // Paste over the same selection round-trips to the original text.
        ed.paste(&mut console);
        assert_eq!(ed.field.text(), "alpha beta gamma");

        // With no selection, paste inserts at the caret.
        ed.field.apply(TextOp::End);
        ed.field.clear_selection();
        ed.paste(&mut console);
        assert_eq!(ed.field.text(), "alpha beta gammabeta");
    }

    /// With no selection, Ctrl+C copies the whole current line *with* its newline,
    /// and Ctrl+X removes the line cleanly — taking the trailing newline, or the
    /// preceding one on the last line, so no blank line is left behind.
    #[test]
    fn clipboard_current_line_copy_and_cut() {
        use crate::system::test_console::TestConsole;
        let mut console = TestConsole::new();

        let mut ed = editor_with("script/en.eggtext", "one\ntwo\nthree");
        ed.field.move_to_line_col(1, 1); // on "two"
        ed.copy(&mut console);
        assert_eq!(console.clipboard.as_deref(), Some("two\n"));

        ed.cut(&mut console);
        assert_eq!(
            ed.field.text(),
            "one\nthree",
            "cut removes the line + its newline"
        );

        // Cutting the last (newline-less) line takes the preceding newline.
        let mut last = editor_with("script/en.eggtext", "a\nb");
        last.field.move_to_line_col(1, 0); // on "b"
        last.cut(&mut console);
        assert_eq!(last.field.text(), "a");
    }

    /// Case-insensitive search: forward takes the first match at/after `from`,
    /// reverse the last before it, and both wrap once around the buffer.
    #[test]
    fn find_search_next_prev_wrap_caseless() {
        // bytes: F0 o1 o2 ' '3 b4 a5 r6 ' '7 f8 o9 o10 ' '11 B12 A13 R14 ' '15 f16 o17 o18
        let ed = editor_with("script/en.eggtext", "Foo bar foo BAR foo");
        assert_eq!(ed.search("foo", 0, false), Some((0, 3)), "first match");
        assert_eq!(ed.search("foo", 1, false), Some((8, 11)), "next after 1");
        assert_eq!(ed.search("foo", 17, false), Some((0, 3)), "forward wraps");
        assert_eq!(ed.search("foo", 8, true), Some((0, 3)), "previous before 8");
        assert_eq!(ed.search("foo", 0, true), Some((16, 19)), "reverse wraps");
        assert_eq!(
            ed.search("bar", 5, false),
            Some((12, 15)),
            "matches BAR caselessly"
        );
        assert_eq!(ed.search("zzz", 0, false), None, "no match");
    }

    /// Go-to-line parses a 1-based number, trims it, and clamps into the file;
    /// a non-number yields nothing.
    #[test]
    fn goto_target_parses_and_clamps() {
        let ed = editor_with("script/en.eggtext", "a\nb\nc\nd\ne"); // 5 lines
        assert_eq!(ed.goto_target("1"), Some(0));
        assert_eq!(ed.goto_target("3"), Some(2));
        assert_eq!(ed.goto_target("99"), Some(4), "clamped to the last line");
        assert_eq!(
            ed.goto_target("0"),
            Some(0),
            "0 saturates to the first line"
        );
        assert_eq!(ed.goto_target("  2 "), Some(1), "surrounding space trimmed");
        assert_eq!(ed.goto_target("x"), None);
    }

    /// Enter carries the current line's leading whitespace onto the new line.
    #[test]
    fn autoindent_carries_leading_whitespace() {
        let mut ed = editor_with("script/en.eggtext", "    hello"); // caret at end
        ed.newline_autoindent();
        assert_eq!(ed.field.text(), "    hello\n    ");
        assert_eq!(
            ed.field.line_col(),
            (1, 4),
            "caret sits after the carried indent"
        );
    }

    /// Smart Home toggles between the first non-whitespace column and column 0.
    #[test]
    fn smart_home_toggles_indent_and_column_zero() {
        let mut ed = editor_with("script/en.eggtext", "    foo"); // caret at col 7
        ed.smart_home(false);
        assert_eq!(ed.field.line_col(), (0, 4), "first stop: first non-blank");
        ed.smart_home(false);
        assert_eq!(ed.field.line_col(), (0, 0), "second stop: column 0");
        ed.smart_home(false);
        assert_eq!(ed.field.line_col(), (0, 4), "toggles back");
    }

    /// Tab indents and Shift+Tab dedents every line a selection covers, by
    /// `TAB_WIDTH` spaces, and the block stays selected for a repeat.
    #[test]
    fn indent_dedent_multiline_selection() {
        let mut ed = editor_with("script/en.eggtext", "one\ntwo\nthree");
        ed.field.select(0, ed.field.text().len()); // whole buffer
        let (first, last) = ed.indent_line_range();
        assert_eq!((first, last), (0, 2));

        ed.indent_lines(first, last);
        assert_eq!(ed.field.text(), "  one\n  two\n  three");

        let (f2, l2) = ed.indent_line_range();
        ed.dedent_lines(f2, l2);
        assert_eq!(
            ed.field.text(),
            "one\ntwo\nthree",
            "dedent undoes the indent"
        );
    }

    /// Ctrl+D duplicates the caret's line below it, the caret moving to the copy.
    #[test]
    fn duplicate_line_copies_below() {
        let mut ed = editor_with("script/en.eggtext", "one\ntwo\nthree");
        ed.field.move_to_line_col(1, 1); // on "two"
        ed.duplicate_line();
        assert_eq!(ed.field.text(), "one\ntwo\ntwo\nthree");
        assert_eq!(
            ed.field.line_col(),
            (2, 1),
            "caret on the copy, same column"
        );
    }

    /// Ctrl+Shift+K deletes the caret's line (with one newline); the caret keeps
    /// its column on the line that takes the slot, and the last line takes the
    /// preceding newline.
    #[test]
    fn delete_line_removes_and_repositions() {
        let mut ed = editor_with("script/en.eggtext", "one\ntwo\nthree");
        ed.field.move_to_line_col(1, 2); // on "two"
        ed.delete_line();
        assert_eq!(ed.field.text(), "one\nthree");
        assert_eq!(
            ed.field.line_col(),
            (1, 2),
            "caret keeps its column on 'three'"
        );

        let mut last = editor_with("script/en.eggtext", "a\nbb");
        last.field.move_to_line_col(1, 1); // on "bb" (last line)
        last.delete_line();
        assert_eq!(last.field.text(), "a");
        assert_eq!(last.field.line_col(), (0, 1));
    }

    /// Alt+Up / Alt+Down swap the caret's line with its neighbour, carrying the
    /// caret; the top/bottom edge is a no-op.
    #[test]
    fn move_line_swaps_with_neighbour() {
        let mut ed = editor_with("script/en.eggtext", "one\ntwo\nthree");
        ed.field.move_to_line_col(1, 2); // on "two"
        ed.move_line(false); // up
        assert_eq!(ed.field.text(), "two\none\nthree");
        assert_eq!(ed.field.line_col(), (0, 2), "caret follows the line up");

        ed.move_line(true); // back down
        assert_eq!(ed.field.text(), "one\ntwo\nthree");
        assert_eq!(ed.field.line_col(), (1, 2));

        ed.field.move_to_line_col(0, 0);
        ed.move_line(false); // top line up: no-op
        assert_eq!(ed.field.text(), "one\ntwo\nthree");
    }

    /// Horizontal caret-follow: scroll left to reveal a caret behind `h_scroll`,
    /// and right (bounded by the caret column) until it fits the text width.
    #[test]
    fn h_scroll_follows_the_caret() {
        use crate::system::test_console::TestConsole;
        let console = TestConsole::new();
        let mut ed = editor_with("script/en.eggtext", "0123456789abcdef");

        // Caret behind the scroll snaps h_scroll back to it.
        ed.h_scroll = 8;
        ed.field.move_to_line_col(0, 2);
        ed.ensure_caret_visible_h(&console, 100);
        assert_eq!(ed.h_scroll, 2, "scrolls left to the caret");

        // A text width nothing fits in scrolls right up to (but not past) the
        // caret's column.
        ed.h_scroll = 0;
        ed.field.move_to_line_col(0, 6);
        ed.ensure_caret_visible_h(&console, -1);
        assert_eq!(ed.h_scroll, 6, "scrolls right, bounded by the caret column");
    }
}

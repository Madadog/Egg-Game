//! A full-window raw text editor for the script DSL files (`script/en.eggtext`
//! and `data/main.eggscene`), hosted per extra view (toggled with F2; F1
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
//! [`TextField`](egg_ui::text_field::TextField); this module adds the multi-line
//! navigation, file I/O, outline and rendering on top.

use std::collections::HashSet;

// Reuse the map editor's shared dock primitives for the outline's own dock —
// the `Side` enum and the resize-size constants. (The multi-panel `DockManager`
// is map-editor-specific, so the outline runs a focused single-panel dock.)
use super::map::dock::{DEFAULT_DOCK, MIN_DOCK, MIN_WORLD, Side};
use egg_ui::text_field::{REPEAT_DELAY, REPEAT_RATE, TextEvent, TextField, TextOp};
use egg_world::data::portraits::{Portrait, Portraits};
use egg_world::data::save::SaveData;
use egg_world::data::scene;
use egg_world::data::script::Script;
use egg_world::data::script::eggtext;
use egg_world::data::script::message::{Message, TextContent};
use egg_world::data::sound::music::MusicTrack;
use egg_world::draw_state::{DrawState, LayerId};
use egg_platform::{ConsoleApi, EggInput, ScanCode, SfxOptions, just_pressed, pressed};
use egg_render::image::{Rgba, RgbaImage};
use egg_render::{
    Canvas, EdgePolicy, Font, PrintOptions, Transform, print_to_centered_with_font,
    print_to_with_font, text_width,
};
use egg_ui::dialogue::Dialogue;

/// The English dialogue/text source and the cutscene source — the editor's two
/// known files (matching the startup asset loads). No host directory enumeration
/// exists, so the file switch (Ctrl+O) toggles between exactly these.
const EGGTEXT_PATH: &str = "script/en.eggtext";
const EGGSCENE_PATH: &str = "data/main.eggscene";

/// Row pitch / caret height in framebuffer px. The bitmap font is 8 px tall; 7
/// keeps lines tight without glyphs touching.
const LINE_H: i32 = 7;
/// A little breathing room from panel edges.
const PAD: i32 = 2;
/// Tab inserts this many spaces (the script files indent with spaces).
const TAB_WIDTH: usize = 2;
/// Floor on a wrapped row's pixel budget, so a deeply-indented line in a narrow
/// view still advances at least one character per row instead of looping.
const MIN_WRAP_W: i32 = 8;
/// Gutter space reserved left of the line number for the fold `+`/`-` glyph, px.
const FOLD_W: i32 = 6;

// Palette indices — the dock's known-good editor colours.
const C_BG: u8 = 0;
const C_TEXT: u8 = 12;
const C_DIM: u8 = 13;
const C_HILITE: u8 = 11;
/// Selection background — the bright blue (Sweetie-16 #9). The darker blue #8
/// was tried first and is near-indistinguishable from the #1a1c2c editor
/// background; #9 reads unmistakably as a selection while white body text (and
/// the syntax-role colours) stay legible over it, and it remains distinct from
/// the cyan caret/active-outline hilite.
const C_SEL: u8 = 9;

// Syntax-highlight role colours (Sweetie-16 indices), resolved into `role_cols`
// in draw and indexed by `HiRole as usize`.
const C_COMMENT: u8 = 13; // muted grey-blue
const C_KEYWORD: u8 = 3; // orange — `#` directives / headers / eggscene verbs
const C_NAME: u8 = 4; // yellow — identifiers, label keys, arguments
const C_STRING: u8 = 5; // light green — quoted strings
const C_NUMBER: u8 = 10; // light blue
const C_BOOL: u8 = 2; // maroon — true / false
const C_OP: u8 = 13; // dim — the `=` of a label
const C_ESCAPE: u8 = 11; // cyan — `\n`-style escapes inside strings

/// A syntax-highlight role for a span of a body line. `Text` (0) is the default
/// colour drawn under everything; the rest overdraw their spans. `repr(usize)`
/// so a role indexes the resolved colour table directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
enum HiRole {
    Text = 0,
    Comment,
    Keyword,
    Name,
    Str,
    Number,
    Bool,
    Operator,
    Escape,
}

/// The eggscene verb keywords — the first word of an indented cutscene line.
const EGGSCENE_VERBS: &[&str] = &[
    "wait", "dialogue", "set", "sound", "music", "walk", "move", "face", "camera", "shake",
    "over",
];

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
    /// The full header text (e.g. `#dialogue lamp`), kept as the stable fold key.
    label: String,
    /// The header's key (second token of a `#tag key`, or a label's name) — the
    /// short name shown under its category, and what a [`TextAnchor::Tag`] matches.
    key: Option<String>,
    /// Which category group this entry lists under in the outline panel.
    category: OutlineCat,
}

/// The category an outline entry groups under in the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutlineCat {
    Label,
    Flag,
    Dialogue,
    List,
    Cutscene,
}

impl OutlineCat {
    /// Display order of the groups; empty ones are skipped.
    const ORDER: [OutlineCat; 5] = [
        Self::Label,
        Self::Flag,
        Self::Dialogue,
        Self::List,
        Self::Cutscene,
    ];
    /// The uppercase group header drawn above the category's items.
    fn title(self) -> &'static str {
        match self {
            Self::Label => "LABELS",
            Self::Flag => "FLAGS",
            Self::Dialogue => "DIALOGUE",
            Self::List => "LISTS",
            Self::Cutscene => "CUTSCENES",
        }
    }
}

/// A rendered outline row: a category header, or an item (index into `outline`).
enum OutlineRow {
    Header(&'static str),
    Item(usize),
}

/// An editor dock panel. `Body` is the text-editing surface itself (never
/// hidden); `Outline` and `Preview` are the aux docks. One panel is the *main*
/// (centre) panel; the others tile off the edges. `repr(usize)` so a panel
/// indexes the per-panel rect table in [`TextEditor::regions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
enum TextPanel {
    Body = 0,
    Outline,
    Preview,
}

/// A dock panel's placement *when it isn't the main/centre panel*: which screen
/// edge it tiles off (`None` = hidden, for aux panels) and its size in px (width
/// for Left/Right, height for Top/Bottom), drag-resizable.
#[derive(Debug, Clone, Copy)]
struct Dock {
    side: Option<Side>,
    size: i32,
}

/// An absolute panel rectangle in framebuffer px.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PanelRect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl PanelRect {
    fn contains(&self, mx: i32, my: i32) -> bool {
        mx >= self.x && mx < self.x + self.w && my >= self.y && my < self.y + self.h
    }
}

/// One resolved conversation page: its fitted text and the portrait/flip in
/// effect — enough to redraw a dialogue box for that turn.
#[derive(Debug, Clone, Default)]
struct PageSnap {
    text: String,
    portrait: Option<Portrait>,
    flip: bool,
}

/// The live dialogue previewer. It parses the editor's *own* buffer into a
/// throwaway [`Script`] (so unsaved edits preview live), resolves the
/// `#dialogue` the caret is inside into per-turn [`PageSnap`]s, and draws a
/// stack of turns ending at the current page (the caret's turn / forward-back).
#[derive(Debug, Default)]
struct Preview {
    /// The buffer parsed into a throwaway script — `None` when it doesn't parse,
    /// or the open file isn't eggtext. Rebuilt on edit.
    script: Option<Script>,
    /// The dialogue key currently shown (the caret's `#dialogue` block).
    key: Option<String>,
    /// The conversation's turns, fitted to the box width; rebuilt on edit / key /
    /// font change.
    pages: Vec<PageSnap>,
    /// The current page (the bottom of the stack) — what forward / back / the
    /// caret select.
    page: usize,
    /// The caret turn the preview last synced to. Caret-follow only re-syncs
    /// `page` when the caret moves to a *different* turn (`!= followed`), so manual
    /// paging (the `<` / `>` buttons, Ctrl+, / .) isn't snapped back every frame.
    followed: usize,
    /// Typewriter progress (chars revealed) of the current page; `delay` paces it.
    chars: usize,
    delay: usize,
    /// Skip the typewriter — show each page's full text at once.
    skip: bool,
    /// Render the box with the small (condensed) font.
    small_font: bool,
}

/// A console that delegates everything to the real one but silences audio — so
/// loading / replaying a preview dialogue (which fits text, and would otherwise
/// play its sounds) stays quiet.
struct Muted<'a, C: ConsoleApi>(&'a mut C);

impl<C: ConsoleApi> ConsoleApi for Muted<'_, C> {
    fn exit(&mut self) {
        self.0.exit();
    }
    fn music(&mut self, _track: Option<&MusicTrack>) {}
    fn sfx(&mut self, _id: &str, _opts: SfxOptions) {}
    fn write_file(&mut self, path: &str, bytes: &[u8]) {
        self.0.write_file(path, bytes);
    }
    fn read_file(&mut self, path: &str) -> Option<Vec<u8>> {
        self.0.read_file(path)
    }
    fn output_image(&mut self) -> &mut RgbaImage {
        self.0.output_image()
    }
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
/// [`TextField`](egg_ui::text_field::TextField) input is read the same way for
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

/// One on-screen row produced by the layout: a slice `[start, end)` (byte
/// offsets within buffer line `line`) of that line's text, drawn at `indent_px`
/// from the text origin. A line that fits is one row (`start = 0`,
/// `indent_px = 0`); a wrapped line is several (continuation rows hang-indent
/// under the line's own indentation); a folded-away line yields none. `fold`
/// marks a foldable header's first row for the gutter glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VisualRow {
    line: usize,
    start: usize,
    end: usize,
    indent_px: i32,
    /// `None` = no fold glyph; `Some(true)` = a folded header; `Some(false)` = a
    /// foldable header that's currently open.
    fold: Option<bool>,
}

/// A multi-line raw editor over one script file. Engine-agnostic: driven by a
/// `&mut impl ConsoleApi` and drawn into a [`DrawState`], exactly like the map
/// editor, so a host owns one per view and pumps `step`/`draw`.
#[derive(Debug)]
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
    /// The caret byte as of the end of the previous `step` (or an external `open`).
    /// The end-of-step caret-follow runs only when the caret has moved since — so
    /// a jump / edit reveals itself, but a bare wheel scroll is free to leave the
    /// caret off screen instead of snapping straight back to it.
    last_caret: usize,
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
    /// Word-wrap: when on (the default), long lines break at word boundaries to
    /// fit the body width — continuation rows hang-indent under the line's own
    /// indentation — and horizontal scroll is disabled. Toggled with Alt+Z.
    wrap: bool,
    /// Collapsed outline sections, keyed by the header's outline label (so a fold
    /// survives the line shifts of editing elsewhere). A header whose label is in
    /// here hides its body lines until the next header.
    folded: HashSet<String>,
    /// The caret's target x (px from the text origin) held across a run of
    /// vertical moves, so Up/Down keep a column through short/wrapped rows.
    /// Cleared by any horizontal move or edit.
    goal_x: Option<i32>,
    /// The aux docks (Ctrl+Shift+O outline / Ctrl+Shift+P preview cycle their
    /// side / hidden) and the body's dock — where the body tiles *when it isn't
    /// the main panel*. Reuses the map editor's `Side`.
    outline_dock: Dock,
    preview_dock: Dock,
    body_dock: Dock,
    /// Which panel holds the centre (the "main" panel). The body by default;
    /// Ctrl+Shift+M cycles it, swapping the body into the vacated dock.
    main_panel: TextPanel,
    /// The panel whose resize splitter is currently being dragged, if any.
    resizing: Option<TextPanel>,
    /// The dialogue previewer's runtime (parsed buffer + the playing dialogue).
    preview: Preview,
}

impl Default for TextEditor {
    fn default() -> Self {
        Self {
            path: None,
            field: TextField::default(),
            scroll: 0,
            h_scroll: 0,
            last_caret: 0,
            dragging: false,
            outline_scroll: 0,
            outline: Vec::new(),
            status: String::new(),
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            mid_edit: false,
            pending_script: None,
            pending_scene: None,
            prompt: None,
            wrap: true, // word-wrap on by default
            folded: HashSet::new(),
            goal_x: None,
            outline_dock: Dock {
                side: Some(Side::Left),
                size: DEFAULT_DOCK as i32,
            },
            preview_dock: Dock {
                side: Some(Side::Bottom),
                size: 44,
            },
            body_dock: Dock {
                side: Some(Side::Bottom),
                size: 44,
            },
            main_panel: TextPanel::Body,
            resizing: None,
            preview: Preview::default(),
        }
    }
}

impl TextEditor {
    /// Load `path` into the buffer and jump to `anchor`. Missing/invalid files
    /// open empty (the editor can still create them). Used both on first entry
    /// and by the Dialogue panel's link (via the host's `poll_text_open`).
    pub fn open(&mut self, system: &mut impl ConsoleApi, path: &str, anchor: TextAnchor) {
        // A present-but-undecodable file is NOT the same as a missing one: only a
        // missing file opens blank (so the editor can still create it). Decoding a
        // non-UTF-8 file to "" and then saving would erase it — refuse instead and
        // leave the current buffer untouched.
        let text = match system.read_file(path) {
            None => String::new(),
            Some(bytes) => match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => {
                    self.status = format!("{path}: not valid UTF-8 — not opened");
                    return;
                }
            },
        };
        self.field = TextField::new(text);
        self.path = Some(path.to_string());
        self.scroll = 0;
        self.h_scroll = 0;
        self.outline_scroll = 0;
        self.dirty = false;
        // Per-file mutable state must not leak across a file switch. A stale undo
        // stack would restore the *previous* file's text into this buffer — and
        // the next Ctrl+S would write it to disk; folds are keyed to the old
        // outline, and the goal column / drag-select / find-prompt / live preview
        // all reference the old buffer.
        self.undo.clear();
        self.redo.clear();
        self.mid_edit = false;
        self.folded.clear();
        self.goal_x = None;
        self.dragging = false;
        self.prompt = None;
        self.last_caret = 0;
        self.preview = Preview::default();
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
    fn step_prompt(&mut self, input: &EggInput, shift: bool) {
        let Some(mut prompt) = self.prompt.take() else {
            return;
        };
        let event = prompt.input.step(input);
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
    /// so the click regions match what [`draw`](Self::draw) lays out. `portraits`
    /// is the live runtime registry (not the built-in default) so the dialogue
    /// previewer's `#pic` resolution matches what the game itself would show.
    pub fn step(
        &mut self,
        system: &mut impl ConsoleApi,
        input: &EggInput,
        font: &Font,
        fb_w: i32,
        fb_h: i32,
        portraits: &Portraits,
    ) {
        if self.path.is_none() {
            self.open(system, EGGTEXT_PATH, TextAnchor::Top);
        }
        let r = self.regions(font, fb_w, fb_h);
        let ctrl = input.key(ScanCode::Ctrl);
        // Shift extends a selection (Shift+arrow / Shift+click); read once for the
        // mouse and keyboard handling below.
        let shift = input.key(ScanCode::Shift);

        // A modal find / go-to-line prompt swallows all input until it closes.
        if self.prompt.is_some() {
            self.step_prompt(input, shift);
            let layout = self.layout(font, &r);
            // A find / go-to match moves the caret, so always reveal it here.
            self.ensure_caret_visible_rows(&layout, r.visible_rows, true);
            if !self.wrap {
                self.ensure_caret_visible_h(font, r.body_w);
            }
            self.last_caret = self.field.cursor();
            return;
        }

        // The visual-row layout for this frame: clicks and the caret map through
        // it. Recomputed after edits below for the caret-follow.
        let layout = self.layout(font, &r);

        // Closes the current coalescing undo group so a run of typing/deleting is
        // one undo step: set by navigation (click / arrows / paging) or by a
        // whitespace insert.
        let mut boundary = false;
        // Whether a vertical move ran this frame — if not, the goal column resets.
        let mut vertical = false;

        // Mouse: a click places the caret (Shift-click / drag extends the
        // selection); the outline jumps; the wheel scrolls whichever column the
        // cursor is over.
        let mouse = input.mouse;
        let p = mouse.pos();
        let (mx, my) = (i32::from(p.x), i32::from(p.y));
        if just_pressed(mouse.left)
            && let Some(panel) = r.splitter_at(mx, my)
        {
            self.resizing = Some(panel); // grab a dock's resize band
            boundary = true;
        } else if just_pressed(mouse.left) {
            boundary = true;
            if let Some(rect) = r.outline.filter(|rc| rc.contains(mx, my)) {
                let row = ((my - rect.y - PAD).max(0) / LINE_H) as usize;
                if let Some(line) = self.outline_jump_target(self.outline_scroll + row) {
                    self.jump_to_line(line);
                }
            } else if r.in_gutter(mx, my) {
                // Gutter click toggles a foldable header's fold.
                if let Some(vr) = layout.get(self.scroll + r.row_at_y(my))
                    && vr.fold.is_some()
                {
                    self.toggle_fold_at(vr.line);
                }
            } else if r.in_body(mx, my)
                && let Some(&row) = layout.get(self.scroll + r.row_at_y(my))
            {
                let within = self.byte_at_x_in_row(&row, mx - r.text_x, font);
                let global = self.line_start_byte(row.line) + within;
                if shift {
                    self.field.move_to_byte(global, true);
                } else {
                    self.field.move_to_byte(global, false);
                    self.field.anchor_here();
                }
                self.dragging = true; // a body press may become a drag-select
            } else if let Some(rect) = r.preview.filter(|rc| rc.contains(mx, my)) {
                // Preview control line (top): five equal cells — `<` · N/M ·
                // page/skip · reg/sm · `>` — sharing `preview_cell_w` with the
                // renderer so each label's click target sits directly under it.
                if my < rect.y + LINE_H + PAD {
                    match ((mx - rect.x) / Self::preview_cell_w(rect.w)).min(4) {
                        0 => self.preview_prev(),
                        2 => {
                            self.preview.skip = !self.preview.skip;
                            self.preview.chars = 0;
                        }
                        3 => self.toggle_preview_font(system, font, rect.w),
                        4 => self.preview_next(),
                        _ => {} // cell 1: the N/M counter, not a button
                    }
                }
            }
        }

        if let Some(panel) = self.resizing.filter(|_| pressed(mouse.left)) {
            // Drag a dock's splitter: its size follows the cursor relative to the
            // panel's outer edge (clamped in `regions`).
            let rect = match panel {
                TextPanel::Body => Some(r.body),
                TextPanel::Outline => r.outline,
                TextPanel::Preview => r.preview,
            };
            if let (Some(rect), Some(side)) = (rect, self.dock(panel).side) {
                self.dock_mut(panel).size = match side {
                    Side::Left => mx - rect.x,
                    Side::Right => rect.x + rect.w - mx,
                    Side::Top => my - rect.y,
                    Side::Bottom => rect.y + rect.h - my,
                };
            }
        } else if self.dragging && pressed(mouse.left) {
            // Drag in progress: extend the selection to the mouse, clamped to the
            // visible rows when it strays past an edge.
            let idx = (self.scroll + r.row_at_y(my)).min(layout.len().saturating_sub(1));
            if let Some(&row) = layout.get(idx) {
                let within = self.byte_at_x_in_row(&row, (mx - r.text_x).max(0), font);
                let global = self.line_start_byte(row.line) + within;
                self.field.move_to_byte(global, true);
            }
        }
        if !pressed(mouse.left) {
            self.dragging = false;
            self.resizing = None;
        }
        let wheel = i32::from(mouse.scroll_y[0]);
        if wheel != 0 {
            if r.outline.is_some_and(|rc| rc.contains(mx, my)) {
                let max = self.outline_rows().len().saturating_sub(1) as i32;
                self.outline_scroll =
                    (self.outline_scroll as i32 - wheel * 2).clamp(0, max) as usize;
            } else if shift && !self.wrap {
                // Shift+wheel scrolls a non-wrapped body horizontally (the
                // caret-follow can pull it back, like the vertical wheel does).
                let max = self.max_line_cols() as i32;
                self.h_scroll = (self.h_scroll as i32 - wheel * 3).clamp(0, max) as usize;
            } else {
                let max = layout.len().saturating_sub(1) as i32;
                self.scroll = (self.scroll as i32 - wheel * 3).clamp(0, max) as usize;
            }
        }

        // Keyboard. Ctrl-chords are commands (clipboard, undo, save, …); otherwise
        // typed text / navigation. Selection-aware: typing and a delete key
        // replace any selection, and Shift+motion extends it. Alt+Up/Down move the
        // current line.
        let alt = input.key(ScanCode::Alt);
        let mut changed = false;
        if ctrl && input.keyp(ScanCode::S) {
            self.save_and_reload(system);
            self.mid_edit = false; // a save closes the current undo group
        } else if ctrl && shift && input.keyp(ScanCode::O) {
            self.cycle_dock(TextPanel::Outline); // Ctrl+Shift+O cycles the outline
            boundary = true;
        } else if ctrl && shift && input.keyp(ScanCode::P) {
            self.cycle_dock(TextPanel::Preview); // Ctrl+Shift+P cycles the preview
            boundary = true;
        } else if ctrl && shift && input.keyp(ScanCode::M) {
            self.cycle_main(); // Ctrl+Shift+M cycles which panel is the centre
            boundary = true;
        } else if ctrl && input.keyp(ScanCode::O) {
            self.switch_file(system);
        } else if ctrl && input.keyp(ScanCode::F) {
            self.open_prompt(PromptKind::Find);
        } else if ctrl && input.keyp(ScanCode::G) {
            self.open_prompt(PromptKind::GoTo);
        } else if ctrl && input.keyp(ScanCode::A) {
            self.field.select_all();
            boundary = true;
        } else if ctrl && input.keyp(ScanCode::C) {
            self.copy(system);
            boundary = true;
        } else if ctrl && input.keyp(ScanCode::X) {
            self.checkpoint_discrete();
            self.cut(system);
            changed = true;
            boundary = true;
        } else if ctrl && input.keyp(ScanCode::V) {
            self.checkpoint_discrete();
            self.paste(system);
            changed = true;
            boundary = true;
        } else if ctrl && shift && input.keyp(ScanCode::K) {
            self.checkpoint_discrete();
            self.delete_line();
            changed = true;
            boundary = true;
        } else if ctrl && input.keyp(ScanCode::D) {
            self.checkpoint_discrete();
            self.duplicate_line();
            changed = true;
            boundary = true;
        } else if ctrl && input.key_repeat(ScanCode::Z, REPEAT_DELAY, REPEAT_RATE) {
            // Ctrl+Z undo, Ctrl+Shift+Z redo (both repeat while held).
            if shift {
                self.redo();
            } else {
                self.undo();
            }
        } else if ctrl && input.key_repeat(ScanCode::Y, REPEAT_DELAY, REPEAT_RATE) {
            self.redo();
        } else if ctrl && shift && input.keyp(ScanCode::LeftBracket) {
            self.toggle_fold_at_caret(); // Ctrl+Shift+[ folds/unfolds the caret's section
            boundary = true;
        } else if alt && input.keyp(ScanCode::Z) {
            self.wrap = !self.wrap; // Alt+Z toggles word-wrap
            if self.wrap {
                self.h_scroll = 0;
            }
            self.goal_x = None;
            boundary = true;
        } else if ctrl && input.keyp(ScanCode::Period) {
            self.preview_next(); // Ctrl+. forward-skips the preview a turn
            boundary = true;
        } else if ctrl && input.keyp(ScanCode::Comma) {
            self.preview_prev(); // Ctrl+, back a turn
            boundary = true;
        } else {
            // Typed text — replaces any selection; a whitespace insert closes the
            // undo group, so each word is its own undo step.
            for c in input.key_chars() {
                if !c.is_control() {
                    self.checkpoint();
                    self.field.edit(TextOp::Push(*c));
                    changed = true;
                    boundary |= c.is_whitespace();
                }
            }
            // Navigation + edits auto-repeat while held (newlines, indents, caret
            // glide, paging); the cadence is the shared text-entry one.
            if input.key_repeat(ScanCode::Return, REPEAT_DELAY, REPEAT_RATE) {
                self.checkpoint();
                self.newline_autoindent(); // carries the line's leading whitespace
                changed = true;
                boundary = true;
            }
            if input.key_repeat(ScanCode::Tab, REPEAT_DELAY, REPEAT_RATE) {
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
            if input.key_repeat(ScanCode::Backspace, REPEAT_DELAY, REPEAT_RATE)
                || input.key_repeat(ScanCode::Delete, REPEAT_DELAY, REPEAT_RATE)
            {
                self.checkpoint();
            }
            let len_before = self.field.text().len();
            self.field.edit_keys(input);
            changed |= self.field.text().len() != len_before;
            if input.key_repeat(ScanCode::Up, REPEAT_DELAY, REPEAT_RATE) {
                if alt {
                    self.checkpoint_discrete();
                    self.move_line(false);
                    changed = true;
                } else {
                    self.move_caret_visual(-1, shift, &layout, font);
                    vertical = true;
                }
                boundary = true;
            }
            if input.key_repeat(ScanCode::Down, REPEAT_DELAY, REPEAT_RATE) {
                if alt {
                    self.checkpoint_discrete();
                    self.move_line(true);
                    changed = true;
                } else {
                    self.move_caret_visual(1, shift, &layout, font);
                    vertical = true;
                }
                boundary = true;
            }
            if input.key_repeat(ScanCode::Home, REPEAT_DELAY, REPEAT_RATE) {
                self.smart_home(shift);
                boundary = true;
            }
            if input.key_repeat(ScanCode::End, REPEAT_DELAY, REPEAT_RATE) {
                self.field.move_caret(TextOp::End, shift);
                boundary = true;
            }
            let page = r.visible_rows.saturating_sub(1) as i32;
            if input.key_repeat(ScanCode::PageUp, REPEAT_DELAY, REPEAT_RATE) {
                self.move_caret_visual(-page, shift, &layout, font);
                vertical = true;
                boundary = true;
            }
            if input.key_repeat(ScanCode::PageDown, REPEAT_DELAY, REPEAT_RATE) {
                self.move_caret_visual(page, shift, &layout, font);
                vertical = true;
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
        if !vertical {
            self.goal_x = None; // a non-vertical action resets the goal column
        }
        // Recompute the layout post-edit. Follow the caret only if it moved (since
        // the previous step or an external jump) or the buffer changed this frame;
        // a bare wheel scroll keeps its position.
        let follow = changed || self.field.cursor() != self.last_caret;
        let layout = self.layout(font, &r);
        self.ensure_caret_visible_rows(&layout, r.visible_rows, follow);
        if follow && !self.wrap {
            self.ensure_caret_visible_h(font, r.body_w);
        }
        self.last_caret = self.field.cursor();
        // Drive the dialogue previewer (parse/follow-caret/tick) for this frame.
        self.update_preview(system, font, r.preview.map_or(0, |p| p.w), changed, portraits);
    }

    /// Paint the editor into the view's BG layer (which `composite_into` blits to
    /// the framebuffer). Fills opaque first, so switching from walkaround leaves
    /// no stale world pixels behind.
    pub fn draw(&self, draw_state: &mut DrawState, font: &Font) {
        let (fb_w, fb_h) = draw_state.size();
        let r = self.regions(font, fb_w, fb_h);
        let opts = print_opts();

        // Resolve every palette colour before the mutable canvas borrow.
        let dim = draw_state.colour(C_DIM);
        let text_col = draw_state.colour(C_TEXT);
        let hilite = draw_state.colour(C_HILITE);
        let sel_col = draw_state.colour(C_SEL);
        let cat_col = draw_state.colour(C_KEYWORD); // outline category headers
        // Syntax-highlight role colours, indexed by `HiRole as usize`.
        let role_cols = [
            text_col,
            draw_state.colour(C_COMMENT),
            draw_state.colour(C_KEYWORD),
            draw_state.colour(C_NAME),
            draw_state.colour(C_STRING),
            draw_state.colour(C_NUMBER),
            draw_state.colour(C_BOOL),
            draw_state.colour(C_OP),
            draw_state.colour(C_ESCAPE),
        ];
        let kind = self.kind();
        // Clone the sprite sheet + palette for the preview's offscreen box surface
        // (it draws portraits) before the canvas borrow below — only when shown.
        let preview_assets = r.preview.map(|_| {
            (
                draw_state.indexed_sprites.clone(),
                draw_state.palettes.clone(),
            )
        });

        draw_state.cls(LayerId::BG, C_BG);
        let canvas = draw_state.rgba(LayerId::BG);

        // Dividers: each open dock's resize band (lit while that panel resizes),
        // the gutter↔body edge, and the status bar.
        for &(panel, band) in &r.splitters {
            let lit = self.resizing == Some(panel);
            canvas.fill_rect(
                band.x,
                band.y,
                band.w,
                band.h,
                if lit { hilite } else { dim },
            );
        }
        canvas.fill_rect(r.text_x - 1, r.body_top, 1, r.body_bottom - r.body_top, dim);
        canvas.fill_rect(0, r.status_y - 1, fb_w, 1, dim);

        let cur_line = self.field.line_col().0;
        let active = self.outline.iter().rposition(|e| e.line <= cur_line);
        let layout = self.layout(font, &r);

        // Outline panel (when shown), at its dock rect: category headers, then each
        // item as `name … line-number` (number right-aligned).
        if let Some(rect) = r.outline {
            let max_rows = ((rect.h - PAD) / LINE_H).max(0) as usize;
            for (vi, row) in self
                .outline_rows()
                .iter()
                .enumerate()
                .skip(self.outline_scroll)
                .take(max_rows)
            {
                let y = rect.y + PAD + (vi - self.outline_scroll) as i32 * LINE_H;
                match *row {
                    OutlineRow::Header(title) => {
                        print_to_with_font(
                            font,
                            canvas,
                            title,
                            rect.x + PAD,
                            y,
                            cat_col,
                            opts.clone(),
                        );
                    }
                    OutlineRow::Item(i) => {
                        let entry = &self.outline[i];
                        let name = entry.key.as_deref().unwrap_or(&entry.label);
                        let colour = if Some(i) == active { hilite } else { text_col };
                        let num = format!("{}", entry.line + 1);
                        let num_x = rect.x + rect.w - PAD - text_width(font, &num, opts.clone());
                        let name_x = rect.x + PAD + 4; // indent items under the header
                        let name =
                            truncate_to_width(name, (num_x - PAD - name_x).max(0), font, &opts);
                        print_to_with_font(font, canvas, &name, name_x, y, colour, opts.clone());
                        print_to_with_font(font, canvas, &num, num_x, y, dim, opts.clone());
                    }
                }
            }
        }

        // Body: each visible visual row is a slice of a buffer line. The gutter
        // (line number, fold glyph) draws on a line's first row; continuation rows
        // hang-indent. The selection is highlighted behind the glyphs per row.
        let body = self.field.text();
        let sel = self.field.selection();
        for (vi, row) in layout
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(r.visible_rows)
        {
            let y = r.row_y(vi - self.scroll);
            let ls = self.line_start_byte(row.line);
            let line = &body[ls..ls + self.line_byte_len(row.line)];

            // Gutter: line number (right-aligned) + fold glyph, on the first row.
            if row.start == 0 {
                if let Some(folded) = row.fold {
                    let glyph = if folded { "+" } else { "-" };
                    print_to_with_font(font, canvas, glyph, r.gutter_x + 1, y, dim, opts.clone());
                }
                let num = format!("{}", row.line + 1);
                let nx = (r.text_x - PAD - text_width(font, &num, opts.clone()))
                    .max(r.gutter_x + FOLD_W);
                print_to_with_font(font, canvas, &num, nx, y, dim, opts.clone());
            }

            // Where this row's text starts (wrap rows from `start`; non-wrap rows
            // from the horizontal-scroll column) and its x origin.
            let (draw_start, x_off) = self.row_origin(row, line);
            let x_base = r.text_x + x_off;
            let row_lo = ls + row.start;
            let row_hi = ls + row.end;
            let last_row = row.end == self.line_byte_len(row.line);

            // Selection: intersect with this row's span (the trailing newline is
            // part of the line's last row, shown as a short tail).
            if let Some((s, e)) = sel {
                let lo = s.clamp(row_lo, row_hi);
                let hi = e.clamp(row_lo, if last_row { row_hi + 1 } else { row_hi });
                if lo < hi {
                    let from = lo.max(ls + draw_start) - ls;
                    let to = hi.min(row_hi) - ls;
                    if to >= from {
                        let x0 = x_base + text_width(font, &line[draw_start..from], opts.clone());
                        let x1 = x_base + text_width(font, &line[draw_start..to], opts.clone());
                        let tail = if last_row && e > row_hi { 3 } else { 0 };
                        let w = x1 - x0 + tail;
                        if w > 0 {
                            canvas.fill_rect(x0, y, w, LINE_H, sel_col);
                        }
                    }
                }
            }

            // Base text, then each syntax-highlight span overdrawn in its colour,
            // clipped to this row's visible slice.
            print_to_with_font(font, 
                canvas,
                &line[draw_start..row.end],
                x_base,
                y,
                role_cols[0],
                opts.clone(),
            );
            for (s, e, role) in highlight_line(line, kind) {
                let cs = s.max(draw_start);
                let ce = e.min(row.end);
                if cs < ce {
                    let x = x_base + text_width(font, &line[draw_start..cs], opts.clone());
                    print_to_with_font(font, 
                        canvas,
                        &line[cs..ce],
                        x,
                        y,
                        role_cols[role as usize],
                        opts.clone(),
                    );
                }
            }
        }

        // Caret, when its visual row is on screen and not scrolled off the left.
        let cr = self.caret_row(&layout);
        if cr >= self.scroll && cr < self.scroll + r.visible_rows {
            let row = layout[cr];
            let ls = self.line_start_byte(row.line);
            let line = &body[ls..ls + self.line_byte_len(row.line)];
            let (_, cb) = self.caret_line_byte();
            let (draw_start, x_off) = self.row_origin(&row, line);
            if cb >= draw_start {
                let cx = x_off
                    + r.text_x
                    + text_width(font, &line[draw_start..cb.min(row.end)], opts.clone());
                let cy = r.row_y(cr - self.scroll);
                canvas.fill_rect(cx, cy, 1, LINE_H, hilite);
            }
        }

        // Dialogue preview panel: a control line (`<  N/M  page  reg  >`) over a
        // stack of conversation turns ending at the current page. Each turn is
        // drawn into an offscreen box surface that carries the real sprite sheet
        // (so portraits render) and is blitted into the panel.
        if let Some(rect) = r.preview {
            let cy = rect.y + PAD;
            let total = self.preview.pages.len();
            // Five equal cells: `<` · N/M · page/skip · reg/sm · `>`. The cell math
            // is shared with the click hit-test (`preview_cell_w`) so each label and
            // its button coincide; every label is centred in its own cell.
            let cw = Self::preview_cell_w(rect.w);
            let cell_cx = |i: i32| rect.x + i * cw + cw / 2;
            print_to_centered_with_font(font, canvas, "<", cell_cx(0), cy, hilite, opts.clone());
            print_to_centered_with_font(font, canvas, ">", cell_cx(4), cy, hilite, opts.clone());
            match &self.preview.key {
                Some(_) if total > 0 => {
                    let counter = format!("{}/{total}", self.preview.page + 1);
                    let mode = if self.preview.skip { "skip" } else { "page" };
                    let font_label = if self.preview.small_font { "sm" } else { "reg" };
                    print_to_centered_with_font(
                        font,
                        canvas,
                        &counter,
                        cell_cx(1),
                        cy,
                        dim,
                        opts.clone(),
                    );
                    // The two toggles read as hilit (clickable), the counter as dim.
                    print_to_centered_with_font(
                        font,
                        canvas,
                        mode,
                        cell_cx(2),
                        cy,
                        hilite,
                        opts.clone(),
                    );
                    print_to_centered_with_font(
                        font,
                        canvas,
                        font_label,
                        cell_cx(3),
                        cy,
                        hilite,
                        opts.clone(),
                    );
                }
                Some(_) => {
                    print_to_centered_with_font(
                        font,
                        canvas,
                        "no dialogue",
                        cell_cx(2),
                        cy,
                        dim,
                        opts.clone(),
                    );
                }
                None => {
                    print_to_centered_with_font(
                        font,
                        canvas,
                        "caret not in a dialogue",
                        cell_cx(2),
                        cy,
                        dim,
                        opts.clone(),
                    );
                }
            }

            if total > 0
                && let Some((sheet, palettes)) = preview_assets
            {
                let turn_h = if self.preview.small_font { 26 } else { 30 };
                let visible = ((rect.h - LINE_H) / turn_h).max(1) as usize;
                let end = self.preview.page;
                let start = end + 1 - visible.min(end + 1); // window ends at `page`
                let box_w = (rect.w - 8).clamp(40, 220) as usize;
                let mut tmp = DrawState::default();
                tmp.resize(rect.w.max(1) as u32, turn_h as u32);
                tmp.indexed_sprites = sheet;
                tmp.palettes = palettes;
                for (slot, pi) in (start..=end).enumerate() {
                    let snap = &self.preview.pages[pi];
                    tmp.rgba(LayerId::BG).fill(Rgba::TRANSPARENT);
                    // Only the current (bottom) turn animates; earlier turns and
                    // skip-mode show full text.
                    let timer = pi == self.preview.page && !self.preview.skip;
                    let d = Dialogue {
                        current_text: Some(snap.text.clone()),
                        portrait: snap.portrait.clone(),
                        flip_portrait: snap.flip,
                        characters: self.preview.chars,
                        width: box_w,
                        ..Dialogue::default()
                    };
                    d.draw_dialogue_box(
                        &mut tmp,
                        LayerId::BG,
                        font,
                        self.preview.small_font,
                        &snap.text,
                        timer,
                    );
                    let y = rect.y + LINE_H + slot as i32 * turn_h;
                    canvas.blit(
                        rect.x,
                        y,
                        tmp.rgba(LayerId::BG),
                        EdgePolicy::Transparent,
                        Transform::default(),
                        |px| px.0[3] == 0,
                    );
                }
            }
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
            let bar = truncate_to_width(&bar, fb_w - PAD * 2, font, &opts);
            print_to_with_font(font, canvas, &bar, PAD, py, text_col, opts.clone());
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
        let bar = truncate_to_width(&bar, fb_w - PAD * 2, font, &opts);
        print_to_with_font(font, canvas, &bar, PAD, r.status_y, text_col, opts);
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
            ScriptKind::EggScene => match scene::parse(&src) {
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

    /// Reveal `line` (unfolding any section that hides it) and move the caret to
    /// its start; the scroll catches up via `ensure_caret_visible_rows`.
    fn jump_to_line(&mut self, line: usize) {
        self.reveal_line(line);
        self.field.move_to_line_col(line, 0);
        self.goal_x = None;
    }

    /// Keep the caret's column within the body horizontally: scroll left if it's
    /// behind `h_scroll`, or right until its measured x fits `text_area_w` px.
    /// `text_area_w` is the body's pixel width (framebuffer minus the gutter).
    fn ensure_caret_visible_h(&mut self, font: &Font, text_area_w: i32) {
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
            if text_width(font, &line[hb..cb], opts.clone()) <= text_area_w {
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

    /// Byte length of buffer line `line`.
    fn line_byte_len(&self, line: usize) -> usize {
        self.line_end_byte(line) - self.line_start_byte(line)
    }

    /// The line-number gutter's pixel width for the current file: room for a fold
    /// glyph, the widest line number, and padding either side. Grows so a 4-digit
    /// file's numbers aren't clipped.
    fn gutter_width(&self, font: &Font) -> i32 {
        let digits = format!("{}", self.line_count().max(1));
        FOLD_W + text_width(font, &digits, print_opts()) + PAD * 2
    }

    /// A panel's dock placement (copy).
    fn dock(&self, panel: TextPanel) -> Dock {
        match panel {
            TextPanel::Body => self.body_dock,
            TextPanel::Outline => self.outline_dock,
            TextPanel::Preview => self.preview_dock,
        }
    }

    /// Mutable access to a panel's dock placement.
    fn dock_mut(&mut self, panel: TextPanel) -> &mut Dock {
        match panel {
            TextPanel::Body => &mut self.body_dock,
            TextPanel::Outline => &mut self.outline_dock,
            TextPanel::Preview => &mut self.preview_dock,
        }
    }

    /// The non-main panel docked to `side`, if any (the cycles keep ≤ 1 per side).
    fn panel_on_side(&self, side: Side) -> Option<TextPanel> {
        [TextPanel::Body, TextPanel::Outline, TextPanel::Preview]
            .into_iter()
            .find(|&p| p != self.main_panel && self.dock(p).side == Some(side))
    }

    /// Resolve this frame's screen split: the non-main panels tile off the edges
    /// (Left/Right full height first, then Top/Bottom between), and the main panel
    /// takes the leftover centre. The body's rect (centre or docked) drives the
    /// gutter / text geometry; the gutter sizes to the file's line numbers.
    fn regions(&self, font: &Font, fb_w: i32, fb_h: i32) -> Regions {
        let status_y = fb_h - LINE_H;
        let gutter_w = self.gutter_width(font);

        let (mut wx, mut wy, mut ww, mut wh) = (0, 0, fb_w, status_y);
        let mut rects: [Option<PanelRect>; 3] = [None; 3];
        let mut splitters = Vec::new();
        for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
            let Some(panel) = self.panel_on_side(side) else {
                continue;
            };
            let horizontal = matches!(side, Side::Left | Side::Right);
            // Keep the leftover a minimum along the claimed axis (a body that ends
            // up there also needs room for its gutter).
            let avail = if horizontal { ww - gutter_w } else { wh };
            let lo = i32::from(MIN_DOCK);
            let thick = self
                .dock(panel)
                .size
                .clamp(lo, (avail - i32::from(MIN_WORLD)).max(lo));
            let (rect, band) = dock_strip(side, wx, wy, ww, wh, thick);
            match side {
                Side::Left => (wx, ww) = (wx + thick, ww - thick),
                Side::Right => ww -= thick,
                Side::Top => (wy, wh) = (wy + thick, wh - thick),
                Side::Bottom => wh -= thick,
            }
            rects[panel as usize] = Some(rect);
            splitters.push((panel, band));
        }
        // The main panel claims the leftover centre.
        rects[self.main_panel as usize] = Some(PanelRect {
            x: wx,
            y: wy,
            w: ww,
            h: wh,
        });

        // The body's rect (centre or docked) drives the text geometry.
        let body = rects[TextPanel::Body as usize].unwrap_or(PanelRect {
            x: 0,
            y: 0,
            w: fb_w,
            h: status_y,
        });
        let gutter_x = body.x;
        let text_x = gutter_x + gutter_w;
        let body_right = body.x + body.w;
        Regions {
            gutter_x,
            text_x,
            body_top: body.y,
            body_bottom: body.y + body.h,
            body_right,
            body_w: (body_right - text_x - PAD).max(1),
            visible_rows: ((body.h - PAD) / LINE_H).max(0) as usize,
            status_y,
            body,
            outline: rects[TextPanel::Outline as usize],
            preview: rects[TextPanel::Preview as usize],
            splitters,
        }
    }

    /// The outline as rendered: category headers interleaved with their items, in
    /// `OutlineCat::ORDER` (empty groups skipped). Items keep their file order.
    fn outline_rows(&self) -> Vec<OutlineRow> {
        let mut rows = Vec::new();
        for &cat in &OutlineCat::ORDER {
            let mut started = false;
            for (i, e) in self.outline.iter().enumerate() {
                if e.category == cat {
                    if !started {
                        rows.push(OutlineRow::Header(cat.title()));
                        started = true;
                    }
                    rows.push(OutlineRow::Item(i));
                }
            }
        }
        rows
    }

    /// The buffer line an outline display row jumps to, or `None` if that row
    /// isn't a jump target (a category header / out of range).
    fn outline_jump_target(&self, row: usize) -> Option<usize> {
        match self.outline_rows().get(row) {
            Some(OutlineRow::Item(i)) => self.outline.get(*i).map(|e| e.line),
            _ => None,
        }
    }

    /// Cycle an aux panel's dock side, skipping a side another visible panel
    /// occupies. A maximized aux drops back to a dock first. The outline cycles
    /// Left → Right → Hidden; the preview Bottom → Left → Right → Hidden. (The
    /// body is moved via [`cycle_main`](Self::cycle_main), never hidden.)
    fn cycle_dock(&mut self, panel: TextPanel) {
        if self.main_panel == panel {
            self.main_panel = TextPanel::Body;
        }
        let order: &[Option<Side>] = match panel {
            TextPanel::Outline => &[Some(Side::Left), Some(Side::Right), None],
            TextPanel::Preview => &[
                Some(Side::Bottom),
                Some(Side::Left),
                Some(Side::Right),
                None,
            ],
            TextPanel::Body => return,
        };
        let cur = self.dock(panel).side;
        let start = order.iter().position(|&s| s == cur).unwrap_or(0);
        for step in 1..=order.len() {
            let cand = order[(start + step) % order.len()];
            if cand.is_none() || !self.side_taken(cand, panel) {
                self.dock_mut(panel).side = cand;
                return;
            }
        }
    }

    /// Whether `side` is occupied by a docked, visible panel other than `except`.
    fn side_taken(&self, side: Option<Side>, except: TextPanel) -> bool {
        let Some(side) = side else { return false };
        [TextPanel::Body, TextPanel::Outline, TextPanel::Preview]
            .into_iter()
            .any(|p| p != except && p != self.main_panel && self.dock(p).side == Some(side))
    }

    /// Cycle which panel is the main (centre) one among the visible panels
    /// (Ctrl+Shift+M); the body swaps into the spot the new main panel vacated, so
    /// the layout stays collision-free.
    fn cycle_main(&mut self) {
        let order = [TextPanel::Body, TextPanel::Outline, TextPanel::Preview];
        let visible = |p: TextPanel| p == TextPanel::Body || self.dock(p).side.is_some();
        let cur = order
            .iter()
            .position(|&p| p == self.main_panel)
            .unwrap_or(0);
        for step in 1..=order.len() {
            let cand = order[(cur + step) % order.len()];
            if visible(cand) {
                if cand != TextPanel::Body {
                    self.body_dock = self.dock(cand); // body takes its vacated spot
                }
                self.main_panel = cand;
                return;
            }
        }
    }

    // ---- Dialogue preview ---------------------------------------------------

    /// The dialogue key the caret is inside: the nearest `#dialogue` outline entry
    /// at or above the caret line.
    fn caret_dialogue_key(&self) -> Option<String> {
        let line = self.field.line_col().0;
        self.outline
            .iter()
            .filter(|e| e.category == OutlineCat::Dialogue && e.line <= line)
            .max_by_key(|e| e.line)
            .and_then(|e| e.key.clone())
    }

    /// The 0-based turn the caret is on within its `#dialogue` block — the index
    /// of the blank-line-separated message group it sits in. Maps a caret move
    /// onto a preview page (clamped to the resolved page count by the caller).
    fn caret_dialogue_page(&self) -> usize {
        let caret = self.field.line_col().0;
        let Some(header) = self
            .outline
            .iter()
            .filter(|e| e.category == OutlineCat::Dialogue && e.line <= caret)
            .map(|e| e.line)
            .max()
        else {
            return 0;
        };
        let lines: Vec<&str> = self.field.text().split('\n').collect();
        let mut groups = 0usize;
        let mut prev_blank = true; // the header acts as a leading boundary
        for i in (header + 1)..=caret {
            let blank = lines.get(i).is_none_or(|l| l.trim().is_empty());
            if !blank && prev_blank {
                groups += 1; // a new message group begins
            }
            prev_blank = blank;
        }
        groups.saturating_sub(1)
    }

    /// Advance the previewer one frame: (re)parse the buffer on edit, follow the
    /// caret to its dialogue + turn, and tick the typewriter. `panel_w` is the
    /// preview dock's width (for text fitting); `changed` is whether the buffer
    /// changed; `portraits` is the live registry `#pic` names resolve against
    /// (the preview must match what the game would actually show, not the
    /// built-in default). A cheap no-op when the preview is hidden.
    fn update_preview(
        &mut self,
        system: &mut impl ConsoleApi,
        font: &Font,
        panel_w: i32,
        changed: bool,
        portraits: &Portraits,
    ) {
        if self.preview_dock.side.is_none() {
            return;
        }
        if changed || self.preview.script.is_none() {
            self.preview.script = (self.kind() == ScriptKind::EggText)
                .then(|| eggtext::parse(self.field.text()).ok())
                .flatten()
                .map(|file| {
                    let mut s = Script::new();
                    s.set_base(file, portraits);
                    s
                });
        }
        let key = self.caret_dialogue_key();
        let key_changed = key != self.preview.key;
        let target = self.caret_dialogue_page();
        if changed || key_changed {
            self.preview.key = key;
            self.preview.page = target;
            self.preview.followed = target;
            self.reload_preview(system, font, panel_w);
        } else if target != self.preview.followed {
            // The caret moved to a different turn of the same dialogue — follow it.
            // Compared against `followed` (the turn last synced to), not `page`, so
            // a manual page step away from the caret's turn isn't reverted here.
            self.preview.followed = target;
            self.preview.page = target.min(self.preview.pages.len().saturating_sub(1));
            self.preview.chars = 0;
            self.preview.delay = 0;
        }
        self.preview_tick();
    }

    /// Rebuild the conversation's per-turn [`PageSnap`]s from the parsed buffer +
    /// caret key, fitted to the box width at the current font. Clamps the page and
    /// resets the typewriter. Silent (loads through a [`Muted`] console).
    fn reload_preview(&mut self, system: &mut impl ConsoleApi, font: &Font, panel_w: i32) {
        let width = (panel_w - 8).clamp(40, 220) as usize;
        let messages = match (&self.preview.script, &self.preview.key) {
            (Some(script), Some(key)) => script.get_dialogue(key),
            _ => Vec::new(),
        };
        let small = self.preview.small_font;
        let mut m = Muted(system);
        self.preview.pages = extract_pages(&messages, width, small, &mut m, font);
        self.preview.page = self
            .preview
            .page
            .min(self.preview.pages.len().saturating_sub(1));
        self.preview.chars = 0;
        self.preview.delay = 0;
    }

    /// The preview control line is five equal cells — `<` · N/M · page/skip ·
    /// reg/sm · `>`. The renderer and the click hit-test both derive cell `i`'s
    /// span from this width (`rect.x + i*cw`), so labels and buttons stay aligned.
    fn preview_cell_w(w: i32) -> i32 {
        (w / 5).max(1)
    }

    /// Forward-skip the preview one turn (reveals the next stacked turn).
    fn preview_next(&mut self) {
        if self.preview.page + 1 < self.preview.pages.len() {
            self.preview.page += 1;
            self.preview.chars = 0;
            self.preview.delay = 0;
        }
    }

    /// Back-skip the preview one turn.
    fn preview_prev(&mut self) {
        self.preview.page = self.preview.page.saturating_sub(1);
        self.preview.chars = 0;
        self.preview.delay = 0;
    }

    /// Toggle the preview box between the small and regular font (re-fits the
    /// pages, which the wrap width depends on).
    fn toggle_preview_font(&mut self, system: &mut impl ConsoleApi, font: &Font, panel_w: i32) {
        self.preview.small_font = !self.preview.small_font;
        self.reload_preview(system, font, panel_w);
    }

    /// Tick the current page's typewriter (silent), pacing ~1 char / 2 frames with
    /// a beat on full stops; a no-op in skip mode or once the page is fully shown.
    fn preview_tick(&mut self) {
        if self.preview.skip {
            return;
        }
        let Some(page) = self.preview.pages.get(self.preview.page) else {
            return;
        };
        let full = page.text.chars().count();
        if self.preview.chars >= full {
            return;
        }
        if self.preview.delay > 0 {
            self.preview.delay -= 1;
            return;
        }
        if page.text.chars().nth(self.preview.chars) == Some('.') {
            self.preview.delay += 4;
        }
        self.preview.chars += 1;
        self.preview.delay += 1;
    }

    // ---- Visual-row layout (word-wrap + folding) ----------------------------
    //
    // Screen rows are decoupled from buffer lines: a line that fits is one row, a
    // wrapped line is several (continuation rows hang-indented), a folded-away
    // line is none. `layout` produces the rows; caret/click/scroll/Up-Down all
    // work in this visual-row space. `scroll` is the index of the top visible row.

    /// Break `line` into the on-screen row slices it occupies at body width
    /// `avail_w`, as `(start, end, indent_px)` byte ranges within the line. With
    /// wrap off (or a line that fits) it's the whole line as one row; otherwise
    /// continuation rows hang-indent under the line's leading whitespace and break
    /// at the last space that fits, hard-breaking only a word too long for a row.
    fn wrap_segments(
        &self,
        line: &str,
        font: &Font,
        avail_w: i32,
    ) -> Vec<(usize, usize, i32)> {
        let opts = print_opts();
        if !self.wrap || line.is_empty() {
            return vec![(0, line.len(), 0)];
        }
        let indent_bytes = line.len() - line.trim_start_matches([' ', '\t']).len();
        let indent_px = text_width(font, &line[..indent_bytes], opts.clone());
        let mut segs = Vec::new();
        let mut start = 0;
        while start < line.len() {
            let row_indent = if segs.is_empty() { 0 } else { indent_px };
            let budget = (avail_w - row_indent).max(MIN_WRAP_W);
            let mut end = start; // furthest fitting char boundary
            let mut last_space = None; // boundary just after a space
            for (off, ch) in line[start..].char_indices() {
                let next = start + off + ch.len_utf8();
                let over = text_width(font, &line[start..next], opts.clone()) > budget;
                if over && next > start + ch.len_utf8() {
                    break; // overflow, with at least one char already placed
                }
                end = next;
                if ch == ' ' {
                    last_space = Some(next);
                }
            }
            if end >= line.len() {
                segs.push((start, line.len(), row_indent));
                break;
            }
            // Break at the last fitting space, else hard-break at the fit boundary.
            let brk = last_space.filter(|&s| s > start).unwrap_or(end);
            segs.push((start, brk, row_indent));
            start = brk;
        }
        if segs.is_empty() {
            segs.push((0, line.len(), 0));
        }
        segs
    }

    /// The line index where the foldable region opened by header line `i` ends
    /// (exclusive) — the next outline header line, or the buffer end.
    fn fold_end(&self, i: usize) -> usize {
        self.outline
            .iter()
            .map(|e| e.line)
            .filter(|&l| l > i)
            .min()
            .unwrap_or_else(|| self.line_count())
    }

    /// The outline entry that starts at buffer line `i`, if any.
    fn outline_at(&self, i: usize) -> Option<&OutlineEntry> {
        self.outline.iter().find(|e| e.line == i)
    }

    /// The fold-glyph state for line `i`: `None` unless it's an outline header
    /// with a non-empty body, else `Some(is_folded)`.
    fn fold_marker(&self, i: usize) -> Option<bool> {
        let entry = self.outline_at(i)?;
        if self.fold_end(i) <= i + 1 {
            return None; // a header with no body line to hide isn't foldable
        }
        Some(self.folded.contains(&entry.label))
    }

    /// Build the body's visual rows at the current wrap/fold state and body width:
    /// folded sections collapse to their header row, and (with wrap on) long lines
    /// split into hang-indented continuation rows.
    fn layout(&self, font: &Font, r: &Regions) -> Vec<VisualRow> {
        let text = self.field.text();
        let lines: Vec<&str> = text.split('\n').collect();
        let mut rows = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let fold = self.fold_marker(i);
            for (start, end, indent_px) in self.wrap_segments(lines[i], font, r.body_w) {
                rows.push(VisualRow {
                    line: i,
                    start,
                    end,
                    indent_px,
                    fold: if start == 0 { fold } else { None },
                });
            }
            i = if fold == Some(true) {
                self.fold_end(i).max(i + 1) // skip the hidden body
            } else {
                i + 1
            };
        }
        rows
    }

    /// `(line, byte within that line)` of the caret.
    fn caret_line_byte(&self) -> (usize, usize) {
        let (line, _) = self.field.line_col();
        (line, self.field.cursor() - self.line_start_byte(line))
    }

    /// Index into `layout` of the visual row the caret sits on. A caret at a wrap
    /// boundary belongs to the start of the following row; one at a line's very
    /// end belongs to that line's last row.
    fn caret_row(&self, layout: &[VisualRow]) -> usize {
        let (cl, cb) = self.caret_line_byte();
        let mut last_on_line = None;
        let mut fallback = 0;
        for (idx, row) in layout.iter().enumerate() {
            if row.line < cl {
                fallback = idx;
            }
            if row.line == cl {
                last_on_line = Some(idx);
                if cb >= row.start && (cb < row.end || cb == self.line_byte_len(cl)) {
                    return idx;
                }
            }
        }
        last_on_line.unwrap_or(fallback)
    }

    /// Where visual `row` (whose buffer-line text is `line`) starts drawing and at
    /// what x offset from the text origin: wrapped rows render from `start` at
    /// their hang-indent; non-wrapped rows render from the horizontal-scroll
    /// column at no offset. The single source of truth shared by draw, clicks and
    /// caret math, so they stay in lock-step.
    fn row_origin(&self, row: &VisualRow, line: &str) -> (usize, i32) {
        if self.wrap {
            (row.start, row.indent_px)
        } else {
            (byte_at_col(line, self.h_scroll).max(row.start), 0)
        }
    }

    /// The caret's x in px from the text origin within its visual row.
    fn caret_x(&self, layout: &[VisualRow], font: &Font) -> i32 {
        let row = layout[self.caret_row(layout)];
        let (_, cb) = self.caret_line_byte();
        let ls = self.line_start_byte(row.line);
        let text = self.field.text();
        let line = &text[ls..ls + self.line_byte_len(row.line)];
        let (draw_start, x_off) = self.row_origin(&row, line);
        let to = cb.clamp(draw_start, row.end);
        x_off + text_width(font, &line[draw_start..to], print_opts())
    }

    /// The byte (within its line) on visual `row` whose x is closest to `goal_x`
    /// (px from the text origin) — lands the caret under the goal column on a
    /// vertical move or a click.
    fn byte_at_x_in_row(&self, row: &VisualRow, goal_x: i32, font: &Font) -> usize {
        let text = self.field.text();
        let ls = self.line_start_byte(row.line);
        let line = &text[ls..ls + self.line_byte_len(row.line)];
        let (draw_start, x_off) = self.row_origin(row, line);
        let slice = &line[draw_start..row.end];
        let opts = print_opts();
        let target = (goal_x - x_off).max(0);
        let mut best = draw_start;
        let mut best_dist = i32::MAX;
        for end in slice
            .char_indices()
            .map(|(o, _)| o)
            .chain(std::iter::once(slice.len()))
        {
            let dist = (text_width(font, &slice[..end], opts.clone()) - target).abs();
            if dist < best_dist {
                best_dist = dist;
                best = draw_start + end;
            }
        }
        best
    }

    /// Move the caret `delta` visual rows (negative = up), keeping its goal x
    /// column, extending the selection when `extend`. Clamps at the ends.
    fn move_caret_visual(
        &mut self,
        delta: i32,
        extend: bool,
        layout: &[VisualRow],
        font: &Font,
    ) {
        if layout.is_empty() {
            return;
        }
        let cur = self.caret_row(layout) as i32;
        let goal = match self.goal_x {
            Some(g) => g,
            None => {
                let g = self.caret_x(layout, font);
                self.goal_x = Some(g);
                g
            }
        };
        let target = (cur + delta).clamp(0, layout.len() as i32 - 1) as usize;
        let row = layout[target];
        let within = self.byte_at_x_in_row(&row, goal, font);
        self.field
            .move_to_byte(self.line_start_byte(row.line) + within, extend);
    }

    /// Keep the scroll in range, and — when `follow` — pull the caret's visual row
    /// back on screen. `follow` is set after a caret move / edit (so a jump reveals
    /// itself) but cleared after a bare wheel scroll, which is then free to leave
    /// the caret off screen rather than snapping straight back to it.
    fn ensure_caret_visible_rows(&mut self, layout: &[VisualRow], visible_rows: usize, follow: bool) {
        if follow {
            let cr = self.caret_row(layout);
            if cr < self.scroll {
                self.scroll = cr;
            } else if visible_rows > 0 && cr >= self.scroll + visible_rows {
                self.scroll = cr + 1 - visible_rows;
            }
        }
        self.scroll = self.scroll.min(layout.len().saturating_sub(1));
    }

    /// Toggle the fold of the outline section whose header is buffer line
    /// `header_line`. Collapsing a section that holds the caret lifts the caret to
    /// the header so it never lands on a hidden line.
    fn toggle_fold_at(&mut self, header_line: usize) {
        let Some(entry) = self.outline_at(header_line) else {
            return;
        };
        let label = entry.label.clone();
        if !self.folded.remove(&label) {
            self.folded.insert(label);
            let caret_line = self.field.line_col().0;
            if caret_line > header_line && caret_line < self.fold_end(header_line) {
                self.field.move_to_line_col(header_line, 0);
                self.goal_x = None;
            }
        }
    }

    /// Toggle the fold of the section the caret is in (Ctrl+Shift+[): the nearest
    /// foldable header at or above the caret's line.
    fn toggle_fold_at_caret(&mut self) {
        let caret_line = self.field.line_col().0;
        let header = self
            .outline
            .iter()
            .map(|e| e.line)
            .filter(|&l| l <= caret_line && self.fold_marker(l).is_some())
            .max();
        if let Some(h) = header {
            self.toggle_fold_at(h);
        }
    }

    /// Unfold whatever section currently hides buffer `line`, so a jump (find /
    /// go-to / outline click) can land there.
    fn reveal_line(&mut self, line: usize) {
        let open: Vec<String> = self
            .outline
            .iter()
            .filter(|e| {
                self.folded.contains(&e.label) && line > e.line && line < self.fold_end(e.line)
            })
            .map(|e| e.label.clone())
            .collect();
        for label in open {
            self.folded.remove(&label);
        }
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
                let category = match (kind, tag) {
                    (ScriptKind::EggText, "dialogue") => Some(OutlineCat::Dialogue),
                    (ScriptKind::EggText, "list") => Some(OutlineCat::List),
                    (ScriptKind::EggText, "flag") => Some(OutlineCat::Flag),
                    (ScriptKind::EggScene, "cutscene") => Some(OutlineCat::Cutscene),
                    _ => None,
                };
                if let Some(category) = category {
                    let label = match &key {
                        Some(k) => format!("#{tag} {k}"),
                        None => format!("#{tag}"),
                    };
                    outline.push(OutlineEntry {
                        line: i,
                        label,
                        key,
                        category,
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
                        category: OutlineCat::Label,
                    });
                }
            }
        }
        self.outline = outline;
    }
}

/// The editor's screen split, derived once per frame (by [`TextEditor::regions`])
/// so `step`'s hit-testing and `draw`'s layout stay in lock-step. The docks tile
/// off the edges; the body is the centre, with a gutter sized to the file's line
/// numbers.
struct Regions {
    /// Gutter (fold glyphs + line numbers): `[gutter_x, text_x)`.
    gutter_x: i32,
    /// Left edge of the body text.
    text_x: i32,
    /// Body text rect edges (top inclusive, right/bottom exclusive).
    body_top: i32,
    body_bottom: i32,
    body_right: i32,
    /// Body text pixel width — the word-wrap budget.
    body_w: i32,
    /// Body rows that fit between `body_top` and `body_bottom`.
    visible_rows: usize,
    status_y: i32,
    /// The body panel's full rect (gutter + text), wherever it's placed — for
    /// resizing the body when it's docked.
    body: PanelRect,
    /// Each aux dock's absolute rect (when shown).
    outline: Option<PanelRect>,
    preview: Option<PanelRect>,
    /// A resize grab band per docked (non-centre) panel — drag to resize it.
    splitters: Vec<(TextPanel, PanelRect)>,
}

impl Regions {
    /// Top y of the first body row.
    fn body_origin_y(&self) -> i32 {
        self.body_top + PAD
    }
    /// Screen y of visible body row `row_in_view` (0 = first visible).
    fn row_y(&self, row_in_view: usize) -> i32 {
        self.body_origin_y() + row_in_view as i32 * LINE_H
    }
    /// The visible body row index under screen y `my` (floored at 0).
    fn row_at_y(&self, my: i32) -> usize {
        ((my - self.body_origin_y()).max(0) / LINE_H) as usize
    }
    /// Is `(mx, my)` over the body text rect?
    fn in_body(&self, mx: i32, my: i32) -> bool {
        mx >= self.text_x && mx < self.body_right && my >= self.body_top && my < self.body_bottom
    }
    /// Is `(mx, my)` over the gutter (fold glyphs / line numbers)?
    fn in_gutter(&self, mx: i32, my: i32) -> bool {
        mx >= self.gutter_x && mx < self.text_x && my >= self.body_top && my < self.body_bottom
    }
    /// The dock panel whose resize grab band contains `(mx, my)`, if any.
    fn splitter_at(&self, mx: i32, my: i32) -> Option<TextPanel> {
        self.splitters
            .iter()
            .find(|(_, b)| b.contains(mx, my))
            .map(|(p, _)| *p)
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

/// The rect a dock of thickness `thick` on `side` claims off the area
/// `(ax, ay, aw, ah)`, plus its inner-edge (3px) resize grab band.
fn dock_strip(
    side: Side,
    ax: i32,
    ay: i32,
    aw: i32,
    ah: i32,
    thick: i32,
) -> (PanelRect, PanelRect) {
    match side {
        Side::Left => (
            PanelRect {
                x: ax,
                y: ay,
                w: thick,
                h: ah,
            },
            PanelRect {
                x: ax + thick - 1,
                y: ay,
                w: 3,
                h: ah,
            },
        ),
        Side::Right => (
            PanelRect {
                x: ax + aw - thick,
                y: ay,
                w: thick,
                h: ah,
            },
            PanelRect {
                x: ax + aw - thick - 1,
                y: ay,
                w: 3,
                h: ah,
            },
        ),
        Side::Top => (
            PanelRect {
                x: ax,
                y: ay,
                w: aw,
                h: thick,
            },
            PanelRect {
                x: ax,
                y: ay + thick - 1,
                w: aw,
                h: 3,
            },
        ),
        Side::Bottom => (
            PanelRect {
                x: ax,
                y: ay + ah - thick,
                w: aw,
                h: thick,
            },
            PanelRect {
                x: ax,
                y: ay + ah - thick - 1,
                w: aw,
                h: 3,
            },
        ),
    }
}

/// Advance a preview dialogue past a pause to the next page, returning whether a
/// new page is shown. Loops `next_text` until the displayed text changes (a lone
/// `Pause` is consumed without changing it) or the queue empties.
fn advance_dialogue(
    d: &mut Dialogue,
    console: &mut impl ConsoleApi,
    font: &Font,
    save: &mut SaveData,
) -> bool {
    let before = d.current_text.clone();
    loop {
        if !d.next_text(console, font, save, true) {
            return false;
        }
        if d.current_text != before {
            return true;
        }
    }
}

/// Resolve a conversation into its per-turn snapshots (text fitted to `width` at
/// `small_text`, plus the portrait/flip in effect on each turn), by silently
/// playing it through to the end. `console` should be a [`Muted`] wrapper.
fn extract_pages(
    messages: &[Message],
    width: usize,
    small_text: bool,
    console: &mut impl ConsoleApi,
    font: &Font,
) -> Vec<PageSnap> {
    let mut save = SaveData {
        small_text_on: small_text,
        ..SaveData::default()
    };
    let mut d = Dialogue::default().with_width(width);
    d.set_messages(console, font, &mut save, messages);
    let mut pages = Vec::new();
    while d.current_text.is_some() {
        // A message's text always appends onto its own page (see
        // `TextContent::Text`), so absorb every queued append into this box
        // before snapshotting it — otherwise a piecemeal reveal would show as
        // several stacked turns instead of one.
        while next_is_append(&d) {
            d.finish_line();
            if !advance_dialogue(&mut d, console, font, &mut save) {
                break;
            }
        }
        pages.push(PageSnap {
            text: d.current_text.clone().unwrap_or_default(),
            portrait: d.portrait.clone(),
            flip: d.flip_portrait,
        });
        // Mark the page done so the next text opens a fresh box rather than
        // appending (mirrors `TextContent::Clear`, which is what actually
        // clears the live widget's `current_text` between messages).
        d.finish_line();
        if !advance_dialogue(&mut d, console, font, &mut save) {
            break;
        }
    }
    pages
}

/// Whether the next text the dialogue will display *appends* to the current
/// box rather than opening a fresh one — decided by whether a
/// [`TextContent::Clear`] (the page-break `lower_messages` inserts before
/// every message) sits between here and it, not by that text's own `#delay`
/// (which no longer signals a page boundary — see the module doc of
/// `egg_world::data::script::eggtext`). The queue holds play order reversed,
/// so `rev()` walks it forwards; anything else in between (`Sound`/
/// `Portrait`/`Flip`/`SetFlag`/`Shake`/`Speed`) fires silently and doesn't
/// affect the answer. An unconsumed `If` carrier can't appear here in
/// practice — `set_messages`/`advance_dialogue` always resolve straight
/// through one via `next_text`'s `is_skip` recursion before returning — so,
/// like the old delay-based version, this doesn't need to look inside one.
fn next_is_append(d: &Dialogue) -> bool {
    for item in d.next_text.iter().rev() {
        match item {
            TextContent::Clear => return false,
            TextContent::Text { .. } => return true,
            _ => continue,
        }
    }
    false
}

/// Scan a quoted string starting at the opening quote `start`, returning the byte
/// just past the closing quote (or the line end) and the `\x` escape sub-spans
/// inside it (drawn over the string in the escape colour).
fn scan_string(line: &str, start: usize) -> (usize, Vec<(usize, usize)>) {
    let mut i = start + 1;
    let mut escapes = Vec::new();
    while i < line.len() {
        let ch = line[i..].chars().next().unwrap();
        if ch == '\\' {
            let mut e = i + 1;
            if let Some(n) = line[e..].chars().next() {
                e += n.len_utf8();
            }
            escapes.push((i, e));
            i = e;
            continue;
        }
        i += ch.len_utf8();
        if ch == '"' {
            break;
        }
    }
    (i, escapes)
}

/// Tokenize one body `line` of script `kind` into coloured byte spans (only the
/// non-default ones; gaps render in the base text colour). Best-effort and
/// per-line: it colours structure — comments, `#` directives/headers, eggscene
/// verbs, `key =` labels, strings, numbers, booleans, string escapes — without
/// validating it (the parser owns correctness).
fn highlight_line(line: &str, kind: ScriptKind) -> Vec<(usize, usize, HiRole)> {
    let mut out = Vec::new();
    let indent = line.len() - line.trim_start().len();
    let body = &line[indent..];
    if body.starts_with("//") {
        out.push((indent, line.len(), HiRole::Comment));
        return out;
    }
    if body.is_empty() {
        return out;
    }
    let col0 = indent == 0;
    let starts_hash = body.starts_with('#');
    let first_word = body.split_whitespace().next().unwrap_or("");
    let is_label = col0 && kind == ScriptKind::EggText && !starts_hash && body.contains('=');
    let is_verb = !col0 && kind == ScriptKind::EggScene && EGGSCENE_VERBS.contains(&first_word);
    // A "structured" line colours typed args; a free-text dialogue/list line only
    // gets its strings and any `#`-directive coloured (via the rules below).
    let structured = starts_hash || is_label || is_verb;

    let mut i = indent;
    let mut word_index = 0usize;
    let mut seen_eq = false;
    let mut prev_keyword = false;
    while i < line.len() {
        let c = line[i..].chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        if c == '"' {
            let (end, escapes) = scan_string(line, i);
            out.push((i, end, HiRole::Str));
            out.extend(escapes.into_iter().map(|(s, e)| (s, e, HiRole::Escape)));
            i = end;
            word_index += 1;
            prev_keyword = false;
            continue;
        }
        if c == '=' && is_label && !seen_eq {
            out.push((i, i + 1, HiRole::Operator));
            seen_eq = true;
            i += 1;
            prev_keyword = false;
            continue;
        }
        // A word: an optional leading `#`, then identifier characters.
        let start = i;
        if c == '#' {
            i += 1;
        }
        while let Some(ch) = line[i..].chars().next() {
            if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                i += ch.len_utf8();
            } else {
                break;
            }
        }
        if i == start {
            i += c.len_utf8(); // lone punctuation → base text
            continue;
        }
        let word = &line[start..i];
        let digits = word.strip_prefix('-').unwrap_or(word);
        let is_num = !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit());
        let role = if word.starts_with('#') {
            HiRole::Keyword
        } else if is_num && (structured || prev_keyword) {
            HiRole::Number
        } else if (word == "true" || word == "false") && (structured || prev_keyword) {
            HiRole::Bool
        } else if is_label {
            if word_index == 0 {
                HiRole::Name // the label key
            } else {
                HiRole::Text // a bare value
            }
        } else if structured && word_index == 0 {
            if is_verb {
                HiRole::Keyword
            } else {
                HiRole::Text
            }
        } else if structured || prev_keyword {
            HiRole::Name // an argument identifier
        } else {
            HiRole::Text
        };
        if role != HiRole::Text {
            out.push((start, i, role));
        }
        prev_keyword = role == HiRole::Keyword;
        word_index += 1;
    }
    out
}

fn truncate_to_width(s: &str, max_w: i32, font: &Font, opts: &PrintOptions) -> String {
    if text_width(font, s, opts.clone()) <= max_w {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        let mut candidate = out.clone();
        candidate.push(c);
        if text_width(font, &candidate, opts.clone()) > max_w {
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
        let mut ed = editor_with("data/main.eggscene", src);
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
        use egg_platform::test_console::TestConsole;

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
        ed.step(&mut console, &EggInput::new(), &Font::blank(), 240, 136, &Portraits::builtin());
        ed.draw(&mut draw, &Font::blank());

        // Drive the caret to the buffer end, let the visual caret-follow move the
        // scroll, then draw — the caret-on-screen branch and the end clamps.
        ed.field.set_cursor(ed.field.text().len());
        let r = ed.regions(&Font::blank(), 240, 136);
        let layout = ed.layout(&Font::blank(), &r);
        ed.ensure_caret_visible_rows(&layout, r.visible_rows, true);
        ed.draw(&mut draw, &Font::blank());

        // The minimum framebuffer (a very narrow text column) is also safe to draw
        // — exercises the wrap layout at a tiny body width.
        let mut small = DrawState::default();
        small.resize(64, 48);
        ed.draw(&mut small, &Font::blank());

        // With a find prompt open, the prompt-bar strip also draws cleanly.
        ed.open_prompt(PromptKind::Find);
        ed.draw(&mut draw, &Font::blank());
        ed.draw(&mut small, &Font::blank());

        // Non-wrapped + horizontally scrolled (long lines sliced from a mid-line
        // column) draws without slicing panics, both with and without a selection.
        ed.prompt = None;
        ed.wrap = false;
        ed.h_scroll = 4;
        ed.field.select(0, ed.field.text().len());
        ed.draw(&mut draw, &Font::blank());
        ed.draw(&mut small, &Font::blank());
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
        use egg_platform::test_console::TestConsole;
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
        use egg_platform::test_console::TestConsole;
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
        let mut ed = editor_with("script/en.eggtext", "0123456789abcdef");

        // Caret behind the scroll snaps h_scroll back to it.
        ed.h_scroll = 8;
        ed.field.move_to_line_col(0, 2);
        ed.ensure_caret_visible_h(&Font::blank(), 100);
        assert_eq!(ed.h_scroll, 2, "scrolls left to the caret");

        // A text width nothing fits in scrolls right up to (but not past) the
        // caret's column.
        ed.h_scroll = 0;
        ed.field.move_to_line_col(0, 6);
        ed.ensure_caret_visible_h(&Font::blank(), -1);
        assert_eq!(ed.h_scroll, 6, "scrolls right, bounded by the caret column");
    }

    // The test console's blank font measures 1px per non-space glyph and 3px per
    // space (small text), which is deterministic enough to drive the wrap layout.

    /// A long line wraps into rows that tile it exactly; the first row has no
    /// hang indent.
    #[test]
    fn wrap_segments_breaks_and_tiles() {
        let ed = editor_with("script/en.eggtext", "");
        let line = "aa bb cc dd ee ff";
        let segs = ed.wrap_segments(line, &Font::blank(), 8);
        assert!(segs.len() > 1, "wraps into multiple rows");
        let joined: String = segs.iter().map(|&(s, e, _)| &line[s..e]).collect();
        assert_eq!(joined, line, "rows tile the line exactly");
        assert_eq!(segs[0].2, 0, "first row has no hang indent");
    }

    /// Wrapped continuation rows hang-indent under the line's leading whitespace.
    #[test]
    fn wrap_segments_hang_indent_continuations() {
        let ed = editor_with("script/en.eggtext", "");
        let line = "    aa bb cc dd ee ff"; // 4-space indent
        let segs = ed.wrap_segments(line, &Font::blank(), 16);
        assert!(segs.len() > 1);
        let indent_px = text_width(&Font::blank(), "    ", print_opts());
        assert_eq!(segs[0].2, 0, "first row flush left");
        assert!(
            segs[1..].iter().all(|&(_, _, ind)| ind == indent_px),
            "continuation rows hang-indent by the leading whitespace width"
        );
    }

    /// With wrap off, a line is always a single full-width row.
    #[test]
    fn wrap_off_is_one_segment() {
        let mut ed = editor_with("script/en.eggtext", "");
        ed.wrap = false;
        let line = "a very long line that would otherwise wrap";
        assert_eq!(
            ed.wrap_segments(line, &Font::blank(), 4),
            vec![(0, line.len(), 0)]
        );
    }

    fn folding_fixture() -> TextEditor {
        let mut ed = editor_with(
            "script/en.eggtext",
            "#dialogue a\n  one\n  two\n#dialogue b\n  three",
        );
        ed.rebuild_outline();
        ed
    }

    /// Folding a section drops its body rows from the layout but keeps the header
    /// (now marked folded); the next section still shows.
    #[test]
    fn folding_hides_body_rows() {
        let mut ed = folding_fixture();
        let r = ed.regions(&Font::blank(), 240, 136);
        let before = ed.layout(&Font::blank(), &r).len();
        ed.toggle_fold_at(0);
        let after = ed.layout(&Font::blank(), &r);
        assert!(after.len() < before, "fewer rows once folded");
        assert!(
            after
                .iter()
                .any(|row| row.line == 0 && row.fold == Some(true)),
            "header row marked folded"
        );
        assert!(
            !after.iter().any(|row| row.line == 1 || row.line == 2),
            "body lines hidden"
        );
        assert!(
            after.iter().any(|row| row.line == 3),
            "the next section still shows"
        );
    }

    /// Collapsing the section the caret is in lifts the caret to the header;
    /// `reveal_line` reopens a section hiding a target line.
    #[test]
    fn folding_lifts_caret_and_reveal_reopens() {
        let mut ed = folding_fixture();
        ed.field.move_to_line_col(2, 1); // caret inside section a's body
        ed.toggle_fold_at(0);
        assert_eq!(ed.field.line_col().0, 0, "caret lifted to the header");
        assert!(ed.folded.contains("#dialogue a"));

        ed.reveal_line(2); // line 2 is inside the folded section
        assert!(!ed.folded.contains("#dialogue a"), "reveal reopened it");
    }

    /// Vertical motion is by visual row: Down advances one row, and steps over a
    /// folded section's hidden body to the next visible row.
    #[test]
    fn visual_down_moves_and_skips_folds() {

        let mut ed = editor_with("script/en.eggtext", "one\ntwo\nthree");
        ed.field.move_to_line_col(0, 1);
        let r = ed.regions(&Font::blank(), 240, 136);
        let layout = ed.layout(&Font::blank(), &r);
        assert_eq!(ed.caret_row(&layout), 0);
        ed.move_caret_visual(1, false, &layout, &Font::blank());
        assert_eq!(ed.field.line_col().0, 1, "down moves one row");

        let mut folded = folding_fixture();
        folded.toggle_fold_at(0); // hide lines 1, 2
        folded.field.move_to_line_col(0, 0); // on header a
        let r = folded.regions(&Font::blank(), 240, 136);
        let layout = folded.layout(&Font::blank(), &r);
        folded.move_caret_visual(1, false, &layout, &Font::blank());
        assert_eq!(
            folded.field.line_col().0,
            3,
            "down from a folded header skips the hidden body"
        );
    }

    /// The eggtext tokenizer colours comments, `#` headers/directives, `key =`
    /// labels, strings (with escapes), numbers and booleans — and leaves free
    /// dialogue text default, save for a trailing `#delay`.
    #[test]
    fn highlight_eggtext_roles() {
        use HiRole::*;
        let roles = |line: &str| highlight_line(line, ScriptKind::EggText);
        assert_eq!(roles("// hi"), vec![(0, 5, Comment)]);
        assert_eq!(roles("  // indented"), vec![(2, 13, Comment)]);
        assert_eq!(
            roles("#dialogue lamp"),
            vec![(0, 9, Keyword), (10, 14, Name)]
        );
        assert_eq!(
            roles("game_title = \"EGG\""),
            vec![(0, 10, Name), (11, 12, Operator), (13, 18, Str)]
        );
        assert_eq!(roles("  #flip false"), vec![(2, 7, Keyword), (8, 13, Bool)]);
        assert_eq!(roles("  #delay 10"), vec![(2, 8, Keyword), (9, 11, Number)]);
        assert_eq!(
            roles("  You can't sleep."),
            vec![],
            "free text stays default"
        );
        assert_eq!(
            roles("  Bye. #delay 30"),
            vec![(7, 13, Keyword), (14, 16, Number)],
            "a trailing directive still colours"
        );
        // A `#choice` block: the header and each `#option` colour as directives
        // (the option's display text falls to the generic argument-name role).
        assert_eq!(roles("  #choice"), vec![(2, 9, Keyword)]);
        assert_eq!(
            roles("  #option Tea"),
            vec![(2, 9, Keyword), (10, 13, Name)]
        );
        // A string escape overlays the string span.
        assert_eq!(
            roles("x = \"a\\nb\""),
            vec![(0, 1, Name), (2, 3, Operator), (4, 10, Str), (6, 8, Escape)]
        );
    }

    /// The eggscene tokenizer colours `#cutscene` headers and verb lines (verb
    /// keyword + typed args), and leaves a non-verb line default.
    #[test]
    fn highlight_eggscene_roles() {
        use HiRole::*;
        let roles = |line: &str| highlight_line(line, ScriptKind::EggScene);
        assert_eq!(
            roles("#cutscene pet_dog"),
            vec![(0, 9, Keyword), (10, 17, Name)]
        );
        assert_eq!(
            roles("  walk 120 64"),
            vec![(2, 6, Keyword), (7, 10, Number), (11, 13, Number)]
        );
        assert_eq!(
            roles("  set seen true"),
            vec![(2, 5, Keyword), (6, 10, Name), (11, 15, Bool)]
        );
        assert_eq!(
            roles("  hello world"),
            vec![],
            "an unknown verb isn't coloured"
        );
    }

    /// The outline docks left / right / hidden, and the panel rect, gutter and
    /// body edges fall out of `regions` accordingly.
    #[test]
    fn regions_dock_left_right_hidden() {
        use crate::map::dock::Side;
        let mut ed = editor_with("script/en.eggtext", "a\nb\nc");
        ed.preview_dock.side = None; // isolate the outline
        ed.outline_dock.size = 60;

        ed.outline_dock.side = Some(Side::Left);
        let r = ed.regions(&Font::blank(), 240, 136);
        let o = r.outline.expect("outline shown");
        assert_eq!((o.x, o.w, r.gutter_x), (0, 60, 60));
        assert_eq!(r.body_right, 240);
        assert!(
            r.text_x > r.gutter_x,
            "gutter sits between outline and body"
        );
        assert_eq!(r.splitters.len(), 1);

        ed.outline_dock.side = Some(Side::Right);
        let r = ed.regions(&Font::blank(), 240, 136);
        let o = r.outline.expect("outline shown");
        assert_eq!((o.x, r.gutter_x, r.body_right), (180, 0, 180));

        ed.outline_dock.side = None;
        let r = ed.regions(&Font::blank(), 240, 136);
        assert!(r.outline.is_none());
        assert_eq!((r.gutter_x, r.body_right), (0, 240));
        assert!(r.splitters.is_empty());
    }

    /// A bottom dock shrinks the body upward — the body ends where the preview
    /// begins, which sits just above the status bar.
    #[test]
    fn regions_bottom_dock_shrinks_body() {
        use crate::map::dock::Side;
        let mut ed = editor_with("script/en.eggtext", "a");
        ed.outline_dock.side = None;
        ed.preview_dock.side = Some(Side::Bottom);
        ed.preview_dock.size = 40;
        let r = ed.regions(&Font::blank(), 240, 136);
        let p = r.preview.expect("preview shown");
        assert_eq!((p.x, p.w), (0, 240), "spans the full width");
        assert_eq!(p.y + p.h, r.status_y, "sits just above the status bar");
        assert_eq!(r.body_bottom, p.y, "body ends where the preview begins");
        assert_eq!(r.body_top, 0);
    }

    /// The line-number gutter widens for a file with more digits in its numbers.
    #[test]
    fn gutter_width_grows_with_line_count() {
        let few = editor_with("script/en.eggtext", "a\nb"); // up to "2"
        let many = editor_with("script/en.eggtext", &"x\n".repeat(150)); // up to "151"
        assert!(
            many.gutter_width(&Font::blank()) > few.gutter_width(&Font::blank()),
            "more digits → wider gutter"
        );
    }

    /// Ctrl+Shift+O cycles the outline Left → Right → Hidden → Left.
    #[test]
    fn cycle_outline_dock_steps() {
        use crate::map::dock::Side;
        let mut ed = editor_with("script/en.eggtext", "");
        ed.preview_dock.side = None; // so the outline cycle doesn't skip-collide
        assert_eq!(ed.outline_dock.side, Some(Side::Left));
        ed.cycle_dock(TextPanel::Outline);
        assert_eq!(ed.outline_dock.side, Some(Side::Right));
        ed.cycle_dock(TextPanel::Outline);
        assert_eq!(ed.outline_dock.side, None);
        ed.cycle_dock(TextPanel::Outline);
        assert_eq!(ed.outline_dock.side, Some(Side::Left));
    }

    /// The previewer's dialogue key follows the caret's `#dialogue` block; a caret
    /// above any dialogue has none.
    #[test]
    fn caret_dialogue_key_follows_caret() {
        let src = "title = Hi\n#dialogue greet\n  Hello.\n#dialogue bye\n  Later.";
        let mut ed = editor_with("script/en.eggtext", src);
        ed.rebuild_outline();
        ed.field.move_to_line_col(0, 0); // on the title label, above any dialogue
        assert_eq!(ed.caret_dialogue_key(), None);
        ed.field.move_to_line_col(2, 0); // inside greet's body
        assert_eq!(ed.caret_dialogue_key().as_deref(), Some("greet"));
        ed.field.move_to_line_col(3, 0); // on bye's header
        assert_eq!(ed.caret_dialogue_key().as_deref(), Some("bye"));
    }

    /// The previewer parses the buffer, resolves the caret's dialogue into turns,
    /// and steps forward / back (clamped).
    #[test]
    fn preview_loads_and_steps_pages() {
        use egg_platform::test_console::TestConsole;
        let mut console = TestConsole::new();
        let src = "#dialogue talk\n  First page.\n\n  Second page.\n\n  Third page.";
        let mut ed = editor_with("script/en.eggtext", src);
        ed.rebuild_outline();
        ed.field.move_to_line_col(1, 0); // inside the dialogue, first turn

        ed.update_preview(&mut console, &Font::blank(), 200, true, &Portraits::builtin()); // parse + resolve pages
        assert_eq!(ed.preview.key.as_deref(), Some("talk"));
        assert_eq!(ed.preview.pages.len(), 3, "three turns resolved");
        assert_eq!(ed.preview.page, 0);

        ed.preview_next();
        ed.preview_next();
        assert_eq!(ed.preview.page, 2, "forward steps through the turns");
        ed.preview_next();
        assert_eq!(ed.preview.page, 2, "clamped at the last turn");
        ed.preview_prev();
        assert_eq!(ed.preview.page, 1, "back steps a turn");
    }

    /// The caret's turn within a `#dialogue` block maps to a preview page (by
    /// blank-line-separated message group); the preview follows it.

    #[test]
    fn caret_dialogue_page_maps_turns() {
        use egg_platform::test_console::TestConsole;
        let mut console = TestConsole::new();
        let src = "#dialogue talk\n  One.\n\n  Two.\n\n  Three.";
        let mut ed = editor_with("script/en.eggtext", src);
        ed.rebuild_outline();
        ed.field.move_to_line_col(1, 0); // "One." → turn 0
        assert_eq!(ed.caret_dialogue_page(), 0);
        ed.field.move_to_line_col(3, 0); // "Two." → turn 1
        assert_eq!(ed.caret_dialogue_page(), 1);
        ed.field.move_to_line_col(5, 0); // "Three." → turn 2
        assert_eq!(ed.caret_dialogue_page(), 2);

        // Moving the caret (no edit) makes the preview follow to that turn.
        ed.field.move_to_line_col(1, 0);
        ed.update_preview(&mut console, &Font::blank(), 200, true, &Portraits::builtin());
        assert_eq!(ed.preview.page, 0);
        ed.field.move_to_line_col(5, 0);
        ed.update_preview(&mut console, &Font::blank(), 200, false, &Portraits::builtin());
        assert_eq!(ed.preview.page, 2, "preview followed the caret to turn 3");
    }

    /// Manual paging (the `<` / `>` buttons, Ctrl+, / .) sticks: a later
    /// caret-follow frame with the caret unmoved doesn't snap the page back to the
    /// caret's turn. Only a genuine caret move resumes following.
    #[test]
    fn manual_paging_survives_caret_follow() {
        use egg_platform::test_console::TestConsole;
        let mut console = TestConsole::new();
        let src = "#dialogue talk\n  One.\n\n  Two.\n\n  Three.";
        let mut ed = editor_with("script/en.eggtext", src);
        ed.rebuild_outline();
        ed.field.move_to_line_col(1, 0); // first turn
        ed.update_preview(&mut console, &Font::blank(), 200, true, &Portraits::builtin());
        assert_eq!(ed.preview.page, 0);

        ed.preview_next(); // step forward a turn via the button
        assert_eq!(ed.preview.page, 1);
        // A follow frame with the caret still on turn 0 must not revert it.
        ed.update_preview(&mut console, &Font::blank(), 200, false, &Portraits::builtin());
        assert_eq!(ed.preview.page, 1, "manual page step persists");

        // Moving the caret to a different turn resumes following.
        ed.field.move_to_line_col(5, 0); // third turn
        ed.update_preview(&mut console, &Font::blank(), 200, false, &Portraits::builtin());
        assert_eq!(ed.preview.page, 2, "a caret move resumes follow");
    }

    /// A `#delay` reveal appends to the current box, so the previewer resolves the
    /// clauses as one turn (the built-up text), not a stack of partial messages.
    #[test]
    fn delay_reveals_collapse_into_one_turn() {
        use egg_platform::test_console::TestConsole;
        let mut console = TestConsole::new();
        // One message (no blank line): "Hi", then two appended clauses; then a
        // second, separate message.
        let src = "#dialogue talk\n  Hi\n  \" there\" #delay 5\n  \"!\" #delay 5\n\n  Next.";
        let mut ed = editor_with("script/en.eggtext", src);
        ed.rebuild_outline();
        ed.field.move_to_line_col(1, 0);
        ed.update_preview(&mut console, &Font::blank(), 200, true, &Portraits::builtin());
        assert_eq!(ed.preview.pages.len(), 2, "delayed clauses stay one turn");
        let first = &ed.preview.pages[0].text;
        assert!(
            first.contains("there") && first.contains('!'),
            "the turn shows the fully built-up text, got {first:?}"
        );
    }

    /// The control line's rendered label centres and its click hit-test both come
    /// from `preview_cell_w`, so each label is hit-tested as its own cell (`<`=0,
    /// page/skip=2, reg/sm=3, `>`=4; the counter, cell 1, is inert).
    #[test]
    fn preview_control_cells_align() {
        for w in [120, 200, 240, 317] {
            let cw = TextEditor::preview_cell_w(w);
            for i in 0..5 {
                let centre = i * cw + cw / 2; // where the renderer draws label `i`
                let cell = (centre / cw).min(4); // how a click there is bucketed
                assert_eq!(cell, i, "w={w}: label cell {i} click-maps to itself");
            }
        }
    }

    /// `ensure_caret_visible_rows` re-centres on the caret only when asked to
    /// `follow`; a bare wheel scroll (`follow = false`) keeps its position so the
    /// viewport can move off the caret, but the scroll is still clamped in range.
    #[test]
    fn scroll_follows_caret_only_when_asked() {
        let src = "x\n".repeat(200);
        let mut ed = editor_with("script/en.eggtext", &src);
        ed.field.move_to_line_col(0, 0); // caret at the top
        let r = ed.regions(&Font::blank(), 240, 136);
        let layout = ed.layout(&Font::blank(), &r);

        ed.scroll = 50; // a wheel scrolled well past the caret
        ed.ensure_caret_visible_rows(&layout, r.visible_rows, false);
        assert_eq!(ed.scroll, 50, "a bare scroll is left where it is");

        ed.ensure_caret_visible_rows(&layout, r.visible_rows, true);
        assert_eq!(ed.scroll, 0, "following snaps back to the caret");

        // The clamp still applies with no follow: scroll can't exceed the layout.
        ed.scroll = 10_000;
        ed.ensure_caret_visible_rows(&layout, r.visible_rows, false);
        assert_eq!(ed.scroll, layout.len() - 1, "scroll stays clamped in range");
    }

    /// Opening at a deep anchor (the map editor's "edit in text editor" jump, run
    /// from a separate host system before the view steps) still reveals the caret
    /// on the next step — the follow keys off the caret moving since last step, not
    /// just within it.
    #[test]
    fn open_anchor_scrolls_into_view_next_step() {
        use egg_platform::test_console::TestConsole;
        let mut console = TestConsole::new();
        console
            .files
            .insert("script/en.eggtext".to_string(), "x\n".repeat(200).into_bytes());
        let mut ed = TextEditor::default();
        ed.open(&mut console, "script/en.eggtext", TextAnchor::Line(150));
        assert_eq!(ed.scroll, 0, "open resets the scroll");
        ed.step(&mut console, &EggInput::new(), &Font::blank(), 240, 136, &Portraits::builtin());
        assert!(ed.scroll > 0, "the step scrolled the deep caret into view");
    }

    /// Ctrl+Shift+M maximizes an aux to the centre, swapping the body into the
    /// spot it vacated (so the body becomes a dockable side panel).
    #[test]
    fn cycle_main_swaps_body_into_panel() {
        use crate::map::dock::Side;
        let mut ed = editor_with("script/en.eggtext", "a\nb");
        assert_eq!(ed.main_panel, TextPanel::Body);
        let body0 = ed.regions(&Font::blank(), 240, 136).body;

        ed.cycle_main(); // Body → Outline (the first visible aux)
        assert_eq!(ed.main_panel, TextPanel::Outline);
        assert_eq!(
            ed.body_dock.side,
            Some(Side::Left),
            "body took the outline's spot"
        );

        let r = ed.regions(&Font::blank(), 240, 136);
        let outline = r.outline.expect("outline shown (now centre)");
        assert!(
            outline.w > r.body.w,
            "outline maximized; body shrank to its dock"
        );
        assert!(
            r.body.w < body0.w,
            "body is smaller than when it was the centre"
        );
        assert!(
            r.splitters.iter().any(|(p, _)| *p == TextPanel::Body),
            "the docked body now has a resize splitter"
        );
    }

    /// Cycling a panel skips a side the other panel already occupies.
    #[test]
    fn cycle_dock_skips_occupied_side() {
        use crate::map::dock::Side;
        let mut ed = editor_with("script/en.eggtext", "");
        ed.outline_dock.side = Some(Side::Left);
        ed.preview_dock.side = Some(Side::Bottom);
        // Preview order Bottom→Left→Right→Hidden: Left is the outline's, so skip it.
        ed.cycle_dock(TextPanel::Preview);
        assert_eq!(
            ed.preview_dock.side,
            Some(Side::Right),
            "skips the outline's Left"
        );
    }

    /// The outline groups entries under category headers in a fixed order, items
    /// keeping their file order; a click resolves to the item's line and a header
    /// is inert.
    #[test]
    fn outline_groups_by_category_with_line_numbers() {
        let src = "title = Hi\n#flag seen\n#dialogue lamp\n  glows\n#list names\n  one";
        let mut ed = editor_with("script/en.eggtext", src);
        ed.rebuild_outline();
        let rendered: Vec<String> = ed
            .outline_rows()
            .iter()
            .map(|row| match *row {
                OutlineRow::Header(t) => format!("[{t}]"),
                OutlineRow::Item(i) => {
                    let e = &ed.outline[i];
                    format!("{}:{}", e.key.as_deref().unwrap_or(""), e.line + 1)
                }
            })
            .collect();
        assert_eq!(
            rendered,
            vec![
                "[LABELS]",
                "title:1",
                "[FLAGS]",
                "seen:2",
                "[DIALOGUE]",
                "lamp:3",
                "[LISTS]",
                "names:5",
            ]
        );

        // Headers (rows 0, 2, 4, 6) are inert; items resolve to their lines.
        assert_eq!(
            ed.outline_jump_target(0),
            None,
            "a header isn't a jump target"
        );
        assert_eq!(ed.outline_jump_target(1), Some(0), "title → line 0");
        assert_eq!(ed.outline_jump_target(5), Some(2), "lamp → line 2");
        assert_eq!(ed.outline_jump_target(99), None);
    }
}

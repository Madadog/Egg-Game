//! A reusable line-editing buffer: the string a focused field accumulates plus
//! the keystroke handling that grows and finishes it. Lifted out of the map
//! editor (which used to keep a bare `buffer: String` and hand-roll the per-frame
//! key reads) so the editing logic is one reusable, testable widget-state type —
//! shared by the map editor's single-line property fields and the multi-line
//! [text editor](super::texteditor).
//!
//! [`TextField::step`] reads the shared console and decodes it into [`TextOp`]s;
//! [`TextField::apply`] performs one op on the buffer. Tests drive `apply`
//! directly to exercise push/backspace/motion without a console. The field tracks
//! only the text — *which* field is being edited and what to do with a committed
//! value stay with the caller. The buffer is a flat `String`; a `'\n'` is just
//! another character, so the same type backs both single-line fields (which never
//! emit [`TextOp::Up`]/etc.) and the multi-line editor.

use crate::system::{ConsoleApi, ScanCode};

/// Held-key repeat cadence for text entry, in fixed steps (the sim runs at
/// 64 Hz): `REPEAT_DELAY` before the first repeat, then one every `REPEAT_RATE`.
/// The single place to tune how Backspace / arrows / etc. auto-repeat across the
/// text editor and the map editor's fields. (`EggInput::key_repeat` itself is
/// cadence-agnostic — these are this use case's numbers.)
pub(crate) const REPEAT_DELAY: u16 = 20;
pub(crate) const REPEAT_RATE: u16 = 2;

/// A single character-level edit to a [`TextField`], the pure unit its console
/// input decodes into. Splitting the keyboard read (which needs a
/// [`ConsoleApi`]) from the buffer mutation (which doesn't) is what lets the
/// field's behaviour be unit-tested without a live console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextOp {
    /// Insert a character at the cursor (a typed, non-control key; the multi-line
    /// editor also pushes a `'\n'` here for Return).
    Push(char),
    /// Delete the character before the cursor (Backspace).
    Pop,
    /// Empty the whole buffer (Ctrl+Backspace).
    Clear,
    /// Move the cursor one character left / right (Arrow keys).
    Left,
    Right,
    /// Move the cursor one word left / right (Ctrl+Arrow).
    WordLeft,
    WordRight,
    /// Move the cursor up / down one line, preserving the column (multi-line
    /// editor only; clamps to the target line's length).
    Up,
    Down,
    /// Move the cursor to the start / end of the current line (Home / End).
    Home,
    End,
    /// Finish editing, keeping the buffer (Return).
    Commit,
    /// Finish editing, discarding the buffer (Escape).
    Cancel,
}

/// How a [`TextField`] resolved this frame: still editing, or finished one way or
/// the other. The caller maps [`Commit`](Self::Commit)/[`Cancel`](Self::Cancel)
/// onto its own "apply the buffer" / "abandon" handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextEvent {
    /// The field absorbed input but is still being edited.
    Active,
    /// Return was pressed — commit the (trimmed-by-the-caller) buffer.
    Commit,
    /// Escape was pressed — drop the edit.
    Cancel,
}

/// A line-editing buffer plus the per-frame keystroke handling that grows it.
#[derive(Debug, Clone, Default)]
pub(crate) struct TextField {
    buffer: String,
    /// Caret position as a byte index into `buffer`, always on a char boundary.
    /// Inserts/deletes happen here, and the arrow keys move it.
    cursor: usize,
}

impl TextField {
    /// A field primed with `initial` as its starting contents (e.g. the existing
    /// value of the property being edited), with the caret at the end.
    pub(crate) fn new(initial: impl Into<String>) -> Self {
        let buffer = initial.into();
        let cursor = buffer.len();
        Self { buffer, cursor }
    }

    /// The current buffer contents (the committed/parsed value).
    pub(crate) fn text(&self) -> &str {
        &self.buffer
    }

    /// The buffer with a caret marker inserted at the cursor — what a focused
    /// single-line field renders, so the arrow keys' position is visible.
    pub(crate) fn display(&self) -> String {
        let mut s = self.buffer.clone();
        s.insert(self.cursor, '_');
        s
    }

    /// The cursor's `(line, column)`, both 0-based, column counted in chars —
    /// what the multi-line editor renders the caret at.
    pub(crate) fn line_col(&self) -> (usize, usize) {
        let line = self.buffer[..self.cursor].matches('\n').count();
        let ls = self.line_start_at(self.cursor);
        let col = self.buffer[ls..self.cursor].chars().count();
        (line, col)
    }

    /// Move the cursor to `(line, col)` (0-based), clamped to the text and to the
    /// target line's length — the multi-line editor's click-to-place-caret.
    pub(crate) fn move_to_line_col(&mut self, line: usize, col: usize) {
        let mut start = 0;
        for _ in 0..line {
            match self.buffer[start..].find('\n') {
                Some(i) => start += i + 1,
                // Fewer lines than `line`: clamp to the last line's start (not the
                // buffer end, which would land on the last line's *end*).
                None => break,
            }
        }
        let end = self.line_end_at(start);
        self.cursor = self.pos_at_column(start, end, col);
    }

    /// Byte index of the start of the line containing `pos` (just after the
    /// preceding `'\n'`, or 0).
    fn line_start_at(&self, pos: usize) -> usize {
        self.buffer[..pos].rfind('\n').map_or(0, |i| i + 1)
    }

    /// Byte index of the end of the line containing `pos` (at the next `'\n'`, or
    /// the buffer end).
    fn line_end_at(&self, pos: usize) -> usize {
        self.buffer[pos..]
            .find('\n')
            .map_or(self.buffer.len(), |i| pos + i)
    }

    /// The byte index `col` chars into the line spanning `[line_start, line_end)`,
    /// clamped to `line_end` when the column runs past the line.
    fn pos_at_column(&self, line_start: usize, line_end: usize, col: usize) -> usize {
        self.buffer[line_start..line_end]
            .char_indices()
            .nth(col)
            .map_or(line_end, |(off, _)| line_start + off)
    }

    /// The byte index of the char boundary just before `cursor`.
    fn prev_boundary(&self) -> usize {
        self.buffer[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    /// The byte index of the char boundary just after `cursor`.
    fn next_boundary(&self) -> usize {
        self.buffer[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |c| self.cursor + c.len_utf8())
    }

    /// Step left over a run of whitespace then a run of word characters — the
    /// start of the word before the cursor.
    fn word_left(&self) -> usize {
        let s = &self.buffer;
        let mut i = self.cursor;
        let is_ws = |i: usize| s[..i].chars().next_back().is_some_and(char::is_whitespace);
        while i > 0 && is_ws(i) {
            i = s[..i].char_indices().next_back().map_or(0, |(j, _)| j);
        }
        while i > 0 && !is_ws(i) {
            i = s[..i].char_indices().next_back().map_or(0, |(j, _)| j);
        }
        i
    }

    /// Step right over a run of whitespace then a run of word characters — the
    /// end of the word after the cursor.
    fn word_right(&self) -> usize {
        let s = &self.buffer;
        let mut i = self.cursor;
        let next = |i: usize| s[i..].chars().next();
        while let Some(c) = next(i).filter(|c| c.is_whitespace()) {
            i += c.len_utf8();
        }
        while let Some(c) = next(i).filter(|c| !c.is_whitespace()) {
            i += c.len_utf8();
        }
        i
    }

    /// Apply one editing op, returning how the field resolved. Edits mutate the
    /// buffer/cursor and stay [`Active`](TextEvent::Active); commit/cancel leave
    /// the buffer untouched and report the terminal event for the caller.
    pub(crate) fn apply(&mut self, op: TextOp) -> TextEvent {
        match op {
            TextOp::Push(c) => {
                self.buffer.insert(self.cursor, c);
                self.cursor += c.len_utf8();
            }
            TextOp::Pop => {
                if self.cursor > 0 {
                    let prev = self.prev_boundary();
                    self.buffer.replace_range(prev..self.cursor, "");
                    self.cursor = prev;
                }
            }
            TextOp::Clear => {
                self.buffer.clear();
                self.cursor = 0;
            }
            TextOp::Left => self.cursor = self.prev_boundary(),
            TextOp::Right => self.cursor = self.next_boundary(),
            TextOp::WordLeft => self.cursor = self.word_left(),
            TextOp::WordRight => self.cursor = self.word_right(),
            TextOp::Up => {
                let ls = self.line_start_at(self.cursor);
                if ls == 0 {
                    self.cursor = 0;
                } else {
                    let col = self.buffer[ls..self.cursor].chars().count();
                    let prev_end = ls - 1; // the '\n' that ends the previous line
                    let prev_start = self.line_start_at(prev_end);
                    self.cursor = self.pos_at_column(prev_start, prev_end, col);
                }
            }
            TextOp::Down => {
                let le = self.line_end_at(self.cursor);
                if le == self.buffer.len() {
                    self.cursor = le;
                } else {
                    let ls = self.line_start_at(self.cursor);
                    let col = self.buffer[ls..self.cursor].chars().count();
                    let next_start = le + 1; // just past the '\n'
                    let next_end = self.line_end_at(next_start);
                    self.cursor = self.pos_at_column(next_start, next_end, col);
                }
            }
            TextOp::Home => self.cursor = self.line_start_at(self.cursor),
            TextOp::End => self.cursor = self.line_end_at(self.cursor),
            TextOp::Commit => return TextEvent::Commit,
            TextOp::Cancel => return TextEvent::Cancel,
        }
        TextEvent::Active
    }

    /// Apply this frame's cursor-editing keys: Backspace (Ctrl ⇒ clear all), and
    /// Left/Right caret motion (Ctrl ⇒ by word). Shared by [`step`](Self::step),
    /// the map dialogs, and the multi-line editor (which adds Up/Down/Home/End and
    /// its own typed-char / newline handling on top), so they all get the same
    /// caret behaviour.
    pub(crate) fn edit_keys(&mut self, system: &impl ConsoleApi) {
        let ctrl = system.key(ScanCode::Ctrl);
        // These auto-repeat while held (Backspace to delete a run, arrows to glide
        // the caret); the Ctrl variants repeat by word.
        if system.key_repeat(ScanCode::Backspace, REPEAT_DELAY, REPEAT_RATE) {
            self.apply(if ctrl { TextOp::Clear } else { TextOp::Pop });
        }
        if system.key_repeat(ScanCode::Left, REPEAT_DELAY, REPEAT_RATE) {
            self.apply(if ctrl { TextOp::WordLeft } else { TextOp::Left });
        }
        if system.key_repeat(ScanCode::Right, REPEAT_DELAY, REPEAT_RATE) {
            self.apply(if ctrl {
                TextOp::WordRight
            } else {
                TextOp::Right
            });
        }
    }

    /// Consume this frame's keyboard input from `system` and fold it into the
    /// buffer, returning whether the field is still active or finished.
    ///
    /// Typed non-control characters insert at the caret; the arrow keys (and
    /// Ctrl+arrow / Ctrl+Backspace) edit via [`edit_keys`](Self::edit_keys);
    /// Escape cancels and Return commits. Escape takes priority over Return when
    /// (improbably) both fire in one frame.
    pub(crate) fn step(&mut self, system: &impl ConsoleApi) -> TextEvent {
        for c in system.key_chars() {
            if !c.is_control() {
                self.apply(TextOp::Push(*c));
            }
        }
        self.edit_keys(system);
        if system.keyp(ScanCode::Escape) {
            self.apply(TextOp::Cancel)
        } else if system.keyp(ScanCode::Return) {
            self.apply(TextOp::Commit)
        } else {
            TextEvent::Active
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A [`TextField`] grows with `Push`, shrinks with `Pop`, and reports the
    /// terminal `Commit`/`Cancel` without otherwise touching its buffer — the pure
    /// char-level operations the console `step` decodes into.
    #[test]
    fn text_field_pure_ops() {
        let mut f = TextField::new("ab");
        assert_eq!(f.text(), "ab");
        // Push appends and stays active.
        assert_eq!(f.apply(TextOp::Push('c')), TextEvent::Active);
        assert_eq!(f.text(), "abc");
        // Pop deletes the last char.
        assert_eq!(f.apply(TextOp::Pop), TextEvent::Active);
        assert_eq!(f.text(), "ab");
        // Pop past empty is harmless.
        let mut empty = TextField::default();
        assert_eq!(empty.apply(TextOp::Pop), TextEvent::Active);
        assert_eq!(empty.text(), "");
        // Commit/Cancel are terminal and leave the buffer for the caller to read.
        assert_eq!(f.apply(TextOp::Commit), TextEvent::Commit);
        assert_eq!(f.text(), "ab", "commit doesn't alter the buffer");
        assert_eq!(f.apply(TextOp::Cancel), TextEvent::Cancel);
        assert_eq!(f.text(), "ab", "cancel doesn't alter the buffer");
    }

    /// Build a field with the caret placed at the first `|` in `marked` (the bar
    /// is removed). Lets the multi-line tests state a caret position inline.
    fn at_caret(marked: &str) -> TextField {
        let cursor = marked.find('|').expect("caret marker");
        let buffer = marked.replace('|', "");
        TextField { buffer, cursor }
    }

    /// Home/End snap to the bounds of the current line, not the whole buffer.
    #[test]
    fn home_end_are_per_line() {
        let mut f = at_caret("one\ntw|o\nthree");
        f.apply(TextOp::Home);
        assert_eq!(f.line_col(), (1, 0));
        f.apply(TextOp::End);
        assert_eq!(f.line_col(), (1, 3), "End stops before the next newline");
    }

    /// Up/Down keep the column and clamp to a shorter target line; stepping off
    /// the top/bottom parks at the buffer ends.
    #[test]
    fn up_down_preserve_and_clamp_column() {
        // Column 4 on the middle line; the line above is shorter ("hi"), so Up
        // clamps to its end (column 2).
        let mut f = at_caret("hi\nfour|s\nbottom");
        f.apply(TextOp::Up);
        assert_eq!(f.line_col(), (0, 2));
        // Back down restores column 4 on a long-enough line.
        f.apply(TextOp::Down);
        f.apply(TextOp::Down);
        assert_eq!(f.line_col(), (2, 2), "column carried from the clamp above");

        // Down on the last line parks at the very end; Up on the first parks at 0.
        let mut g = at_caret("ab\nc|d");
        g.apply(TextOp::Down);
        assert_eq!(g.text(), "ab\ncd", "motion never alters the buffer");
        assert_eq!(g.line_col(), (1, 2));
        let mut h = at_caret("ab|c\nd");
        h.apply(TextOp::Up);
        assert_eq!(h.line_col(), (0, 0));
    }

    /// Clicking maps to `move_to_line_col`, which clamps a past-the-end column to
    /// the target line and a past-the-end line to the buffer's last line.
    #[test]
    fn move_to_line_col_clamps() {
        let mut f = TextField::new("alpha\nbe\ngamma");
        f.move_to_line_col(1, 9); // column past the short middle line
        assert_eq!(f.line_col(), (1, 2));
        f.move_to_line_col(9, 0); // line past the end → last line, column 0
        assert_eq!(f.line_col(), (2, 0));
    }

    /// A `'\n'` pushed at the caret splits the line; the column resets on the new
    /// line — the multi-line editor's Return.
    #[test]
    fn newline_push_splits_the_line() {
        let mut f = at_caret("ab|cd");
        f.apply(TextOp::Push('\n'));
        assert_eq!(f.text(), "ab\ncd");
        assert_eq!(f.line_col(), (1, 0));
    }
}

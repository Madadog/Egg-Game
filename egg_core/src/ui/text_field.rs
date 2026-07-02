//! A reusable line-editing buffer: the string a focused field accumulates plus
//! the keystroke handling that grows and finishes it. Lifted out of the map
//! editor (which used to keep a bare `buffer: String` and hand-roll the per-frame
//! key reads) so the editing logic is one reusable, testable widget-state type —
//! shared by the map editor's single-line property fields and the multi-line
//! [text editor](super::text).
//!
//! [`TextField::step`] reads the frame's [`EggInput`](crate::platform::EggInput)
//! and decodes it into [`TextOp`]s; [`TextField::apply`] performs one op on the
//! buffer. Tests drive `apply` directly to exercise push/backspace/motion without
//! any input. The field tracks
//! only the text — *which* field is being edited and what to do with a committed
//! value stay with the caller. The buffer is a flat `String`; a `'\n'` is just
//! another character, so the same type backs both single-line fields (which never
//! emit [`TextOp::Up`]/etc.) and the multi-line editor.

use crate::platform::{EggInput, ScanCode};

/// Held-key repeat cadence for text entry, in fixed steps (the sim runs at
/// 64 Hz): `REPEAT_DELAY` before the first repeat, then one every `REPEAT_RATE`.
/// The single place to tune how Backspace / arrows / etc. auto-repeat across the
/// text editor and the map editor's fields. (`EggInput::key_repeat` itself is
/// cadence-agnostic — these are this use case's numbers.)
pub(crate) const REPEAT_DELAY: u16 = 20;
pub(crate) const REPEAT_RATE: u16 = 2;

/// A single character-level edit to a [`TextField`], the pure unit its input
/// decodes into. Splitting the keyboard read (which needs an
/// [`EggInput`](crate::platform::EggInput)) from the buffer mutation (which
/// doesn't) is what lets the field's behaviour be unit-tested without any input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextOp {
    /// Insert a character at the cursor (a typed, non-control key; the multi-line
    /// editor also pushes a `'\n'` here for Return).
    Push(char),
    /// Delete the character before the cursor (Backspace).
    Pop,
    /// Delete the character after the cursor (Delete).
    DeleteForward,
    /// Delete the word before the cursor (Ctrl+Backspace).
    DeleteWordBack,
    /// Delete the word after the cursor (Ctrl+Delete).
    DeleteWordForward,
    /// Move the cursor one character left / right (Arrow keys).
    Left,
    Right,
    /// Move the cursor one word left / right (Ctrl+Arrow).
    WordLeft,
    WordRight,
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
    /// The selection's fixed end (the *anchor*), as a byte index, set while a
    /// Shift-motion or drag is extending a selection; `cursor` is the moving end
    /// and the selected span runs between the two. `None` when nothing is
    /// selected. An unshifted motion clears it, as does consuming the selection
    /// (deleting / replacing it).
    anchor: Option<usize>,
}

impl TextField {
    /// A field primed with `initial` as its starting contents (e.g. the existing
    /// value of the property being edited), with the caret at the end.
    pub(crate) fn new(initial: impl Into<String>) -> Self {
        let buffer = initial.into();
        let cursor = buffer.len();
        Self {
            buffer,
            cursor,
            anchor: None,
        }
    }

    /// The current buffer contents (the committed/parsed value).
    pub(crate) fn text(&self) -> &str {
        &self.buffer
    }

    /// The cursor's byte index — for snapshotting (e.g. the editor's undo).
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    /// Place the cursor at `byte`, snapped down to a char boundary and clamped to
    /// the buffer — restores a snapshotted position.
    pub(crate) fn set_cursor(&mut self, byte: usize) {
        self.cursor = self.snap(byte);
    }

    /// Snap `byte` down to the nearest char boundary at or below it, clamped to
    /// the buffer length — so a byte index from a snapshot, a click, or a stale
    /// selection anchor is always safe to slice at.
    fn snap(&self, byte: usize) -> usize {
        let b = byte.min(self.buffer.len());
        (0..=b)
            .rev()
            .find(|&i| self.buffer.is_char_boundary(i))
            .unwrap_or(0)
    }

    /// The selected byte range as ordered `(start, end)`, or `None` when there is
    /// no selection — no anchor, or it coincides with the caret. The anchor is
    /// snapped defensively, so slicing the buffer at these bounds is always safe.
    pub(crate) fn selection(&self) -> Option<(usize, usize)> {
        let a = self.snap(self.anchor?);
        (a != self.cursor).then(|| (a.min(self.cursor), a.max(self.cursor)))
    }

    /// The currently selected text, or `""` when nothing is selected.
    pub(crate) fn selected_text(&self) -> &str {
        match self.selection() {
            Some((s, e)) => &self.buffer[s..e],
            None => "",
        }
    }

    /// Drop any selection, leaving the caret where it is.
    pub(crate) fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Select the byte range `[start, end)` (snapped + ordered), parking the caret
    /// at the high end — used by select-all and to highlight a find match.
    pub(crate) fn select(&mut self, start: usize, end: usize) {
        let lo = self.snap(start.min(end));
        let hi = self.snap(start.max(end));
        self.anchor = Some(lo);
        self.cursor = hi;
    }

    /// Delete the selected span if there is one, parking the caret where the
    /// selection began and clearing the anchor; returns whether it deleted.
    pub(crate) fn delete_selection(&mut self) -> bool {
        let had = if let Some((s, e)) = self.selection() {
            self.buffer.replace_range(s..e, "");
            self.cursor = s;
            true
        } else {
            false
        };
        self.anchor = None;
        had
    }

    /// Remove the bytes in `[start, end)` (snapped + ordered) and park the caret
    /// at the start. A raw primitive for the editor's line ops — it doesn't touch
    /// the selection anchor (the caller manages that).
    pub(crate) fn delete_range(&mut self, start: usize, end: usize) {
        let lo = self.snap(start.min(end));
        let hi = self.snap(start.max(end));
        self.buffer.replace_range(lo..hi, "");
        self.cursor = lo;
    }

    /// Insert `text` at the caret, advancing the caret past it. A raw primitive
    /// (paste / line ops); the caller deletes any selection first.
    pub(crate) fn insert_str(&mut self, text: &str) {
        self.buffer.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    /// Move the caret with `op`, extending or collapsing the selection: when
    /// `extend` is set, anchor at the current caret if not already anchored (so
    /// the far end stays put) and then move; otherwise drop any selection first.
    pub(crate) fn move_caret(&mut self, op: TextOp, extend: bool) -> TextEvent {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.apply(op)
    }

    /// Apply an editing `op` after first deleting any active selection, so typing
    /// or a delete key replaces the selection. A lone delete op is satisfied by
    /// that deletion alone (it doesn't then also remove a neighbouring char/word).
    pub(crate) fn edit(&mut self, op: TextOp) -> TextEvent {
        let deleted = self.delete_selection();
        let is_delete = matches!(
            op,
            TextOp::Pop
                | TextOp::DeleteForward
                | TextOp::DeleteWordBack
                | TextOp::DeleteWordForward
        );
        if deleted && is_delete {
            TextEvent::Active
        } else {
            self.apply(op)
        }
    }

    /// Select the whole buffer (Ctrl+A); the caret parks at the end.
    pub(crate) fn select_all(&mut self) {
        self.select(0, self.buffer.len());
    }

    /// The byte span `[start, end)` of the line the caret is on, excluding the
    /// trailing newline — for the editor's current-line copy and line ops.
    pub(crate) fn current_line_span(&self) -> (usize, usize) {
        (
            self.line_start_at(self.cursor),
            self.line_end_at(self.cursor),
        )
    }

    /// Anchor a selection at the current caret — a mouse press that may become a
    /// drag. With anchor == caret the selection is empty, so a bare click selects
    /// nothing; a following drag moves the caret and grows it.
    pub(crate) fn anchor_here(&mut self) {
        self.anchor = Some(self.cursor);
    }

    /// Move the caret to byte `target` (snapped), extending the selection when
    /// `extend` is set and collapsing it otherwise — the byte-addressed sibling of
    /// [`move_caret`](Self::move_caret), for the editor's smart-Home.
    pub(crate) fn move_to_byte(&mut self, target: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = self.snap(target);
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
            TextOp::DeleteForward => {
                if self.cursor < self.buffer.len() {
                    let next = self.next_boundary();
                    self.buffer.replace_range(self.cursor..next, "");
                }
            }
            TextOp::DeleteWordBack => {
                let start = self.word_left();
                self.buffer.replace_range(start..self.cursor, "");
                self.cursor = start;
            }
            TextOp::DeleteWordForward => {
                let end = self.word_right();
                self.buffer.replace_range(self.cursor..end, "");
            }
            TextOp::Left => self.cursor = self.prev_boundary(),
            TextOp::Right => self.cursor = self.next_boundary(),
            TextOp::WordLeft => self.cursor = self.word_left(),
            TextOp::WordRight => self.cursor = self.word_right(),
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
    pub(crate) fn edit_keys(&mut self, input: &EggInput) {
        let ctrl = input.key(ScanCode::Ctrl);
        let shift = input.key(ScanCode::Shift);
        // These auto-repeat while held (Backspace/Delete to delete a run, arrows to
        // glide the caret); the Ctrl variants act by word, Shift+arrow extends the
        // selection, and a delete with an active selection removes the selection.
        if input.key_repeat(ScanCode::Backspace, REPEAT_DELAY, REPEAT_RATE) {
            self.edit(if ctrl {
                TextOp::DeleteWordBack
            } else {
                TextOp::Pop
            });
        }
        if input.key_repeat(ScanCode::Delete, REPEAT_DELAY, REPEAT_RATE) {
            self.edit(if ctrl {
                TextOp::DeleteWordForward
            } else {
                TextOp::DeleteForward
            });
        }
        if input.key_repeat(ScanCode::Left, REPEAT_DELAY, REPEAT_RATE) {
            self.move_caret(if ctrl { TextOp::WordLeft } else { TextOp::Left }, shift);
        }
        if input.key_repeat(ScanCode::Right, REPEAT_DELAY, REPEAT_RATE) {
            self.move_caret(
                if ctrl {
                    TextOp::WordRight
                } else {
                    TextOp::Right
                },
                shift,
            );
        }
    }

    /// Consume this frame's keyboard input from `system` and fold it into the
    /// buffer, returning whether the field is still active or finished.
    ///
    /// Typed non-control characters insert at the caret; the arrow keys (and
    /// Ctrl+arrow / Ctrl+Backspace) edit via [`edit_keys`](Self::edit_keys);
    /// Escape cancels and Return commits. Escape takes priority over Return when
    /// (improbably) both fire in one frame.
    pub(crate) fn step(&mut self, input: &EggInput) -> TextEvent {
        for c in input.key_chars() {
            if !c.is_control() {
                self.edit(TextOp::Push(*c));
            }
        }
        self.edit_keys(input);
        if input.keyp(ScanCode::Escape) {
            self.apply(TextOp::Cancel)
        } else if input.keyp(ScanCode::Return) {
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
        TextField {
            buffer,
            cursor,
            anchor: None,
        }
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

    /// Forward delete (Delete) removes the char after the caret; the word variants
    /// (Ctrl+Delete / Ctrl+Backspace) remove a whole word in each direction.
    #[test]
    fn forward_and_word_deletes() {
        let mut f = at_caret("ab|cd");
        f.apply(TextOp::DeleteForward);
        assert_eq!(
            (f.text(), f.line_col()),
            ("abd", (0, 2)),
            "removed 'c', caret stays"
        );

        let mut g = at_caret("foo |bar baz");
        g.apply(TextOp::DeleteWordForward);
        assert_eq!(g.text(), "foo  baz", "removed the word after the caret");

        let mut h = at_caret("foo bar| baz");
        h.apply(TextOp::DeleteWordBack);
        assert_eq!(
            (h.text(), h.line_col()),
            ("foo  baz", (0, 4)),
            "removed the word before"
        );
    }

    /// A Shift-motion builds a selection from the anchor; an unshifted motion
    /// drops it. `selected_text` reflects the spanned range whichever way the
    /// caret moved relative to the anchor.
    #[test]
    fn selection_extends_and_collapses() {
        let mut f = at_caret("|hello");
        // Extend right twice: anchor stays at 0, caret moves to column 2.
        f.move_caret(TextOp::Right, true);
        f.move_caret(TextOp::Right, true);
        assert_eq!(f.selection(), Some((0, 2)));
        assert_eq!(f.selected_text(), "he");
        // An unshifted motion collapses the selection (anchor cleared).
        f.move_caret(TextOp::Right, false);
        assert_eq!(f.selection(), None);
        assert_eq!(f.selected_text(), "");

        // Selecting leftward orders the range the same way (anchor > caret).
        let mut g = at_caret("hello|");
        g.move_caret(TextOp::WordLeft, true);
        assert_eq!(g.selection(), Some((0, 5)));
        assert_eq!(g.selected_text(), "hello");
    }

    /// `edit` deletes an active selection first, so typing or a delete key
    /// replaces it; with no selection it falls through to plain `apply`.
    #[test]
    fn edit_replaces_selection() {
        let mut f = at_caret("|hello");
        f.move_caret(TextOp::Right, true);
        f.move_caret(TextOp::Right, true); // selects "he"
        f.edit(TextOp::Push('H')); // typing replaces it
        assert_eq!((f.text(), f.line_col()), ("Hllo", (0, 1)));
        assert!(f.selection().is_none(), "selection consumed by the edit");

        // A delete key with a selection removes only the selection (no extra char).
        let mut g = at_caret("a|bcd");
        g.move_caret(TextOp::Right, true);
        g.move_caret(TextOp::Right, true); // selects "bc"
        g.edit(TextOp::Pop);
        assert_eq!((g.text(), g.line_col()), ("ad", (0, 1)));
    }

    /// `select` highlights an explicit range (find / select-all), and
    /// `delete_selection` removes it and reports whether it did.
    #[test]
    fn select_and_delete_selection() {
        let mut f = TextField::new("alpha beta");
        f.select(6, 10); // "beta"
        assert_eq!(f.selected_text(), "beta");
        assert!(f.delete_selection());
        assert_eq!(f.text(), "alpha ");
        assert!(!f.delete_selection(), "nothing selected now");
    }
}

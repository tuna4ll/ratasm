//! An open file: text, cursor, selection, history and viewport.
//!
//! A [`Document`] is the unit the user thinks of as "a tab". It owns a
//! [`TextBuffer`] and a [`History`] and adds everything positional — where the
//! cursor is, what is selected, how far the view has scrolled.
//!
//! Nothing here draws. The viewport is tracked as plain numbers and
//! [`Document::scroll_into_view`] is called by the renderer with the height it
//! happens to have; the document never learns what a terminal is. That is what
//! makes every behaviour below testable without a screen.

use std::path::{Path, PathBuf};

use super::buffer::{Replacement, TextBuffer};
use super::history::{Edit, History};
use super::position::{Position, Range};

/// How far a cursor movement travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
    /// One character left, wrapping to the previous line.
    Left,
    /// One character right, wrapping to the next line.
    Right,
    /// One line up, preserving the desired column.
    Up,
    /// One line down, preserving the desired column.
    Down,
    /// To the first non-blank character, or to column zero if already there.
    LineStart,
    /// To the end of the current line.
    LineEnd,
    /// To the start of the previous word.
    WordLeft,
    /// To the start of the next word.
    WordRight,
    /// Up by `n` lines.
    PageUp(usize),
    /// Down by `n` lines.
    PageDown(usize),
    /// To the very start of the document.
    DocumentStart,
    /// To the very end of the document.
    DocumentEnd,
    /// To an explicit position, clamped into the buffer.
    To(Position),
}

/// Whether a movement extends the selection or collapses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// Collapse any selection and move the cursor.
    Collapse,
    /// Keep the anchor and extend the selection to the new cursor.
    Extend,
}

/// One open file with all of its editing state.
#[derive(Debug, Clone)]
pub struct Document {
    buffer: TextBuffer,
    history: History,
    cursor: Position,
    anchor: Option<Position>,
    /// Column the cursor "wants" during vertical movement.
    ///
    /// Moving down through a short line and back up must return to the
    /// original column; without a remembered target the cursor would be
    /// permanently pulled left by the shortest line it crossed.
    desired_column: Option<usize>,
    scroll_line: usize,
    scroll_column: usize,
    /// Number of spaces inserted by an indent operation.
    indent_width: usize,
}

impl Document {
    /// Creates an empty, untitled document.
    pub fn new() -> Self {
        Self {
            buffer: TextBuffer::new(),
            history: History::new(),
            cursor: Position::ORIGIN,
            anchor: None,
            desired_column: None,
            scroll_line: 0,
            scroll_column: 0,
            indent_width: 4,
        }
    }

    /// Creates a document from text loaded from `path`.
    pub fn from_file_contents(path: impl Into<PathBuf>, text: &str) -> Self {
        let mut document = Self::new();
        document.buffer = TextBuffer::from_file_contents(path, text);
        document
    }

    /// Creates a document from text with no associated file.
    pub fn from_text(text: &str) -> Self {
        let mut document = Self::new();
        document.buffer = TextBuffer::from_text(text);
        document
    }

    /// The text buffer.
    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    /// The undo history.
    pub fn history(&self) -> &History {
        &self.history
    }

    /// The cursor position.
    pub fn cursor(&self) -> Position {
        self.cursor
    }

    /// The path this document is associated with, if any.
    pub fn path(&self) -> Option<&Path> {
        self.buffer.path()
    }

    /// The name shown in the tab bar.
    pub fn display_name(&self) -> String {
        self.buffer.display_name()
    }

    /// Whether the document has unsaved changes.
    pub fn is_modified(&self) -> bool {
        self.history.is_modified()
    }

    /// The number of spaces one indent level occupies.
    pub fn indent_width(&self) -> usize {
        self.indent_width
    }

    /// Sets the indent width, clamped to a sane range.
    pub fn set_indent_width(&mut self, width: usize) {
        self.indent_width = width.clamp(1, 16);
    }

    /// The first visible line.
    pub fn scroll_line(&self) -> usize {
        self.scroll_line
    }

    /// The first visible display column.
    pub fn scroll_column(&self) -> usize {
        self.scroll_column
    }

    /// The current selection, or `None` when nothing is selected.
    ///
    /// An anchor equal to the cursor is reported as no selection, so a click
    /// that sets an anchor without dragging does not produce an empty
    /// highlight.
    pub fn selection(&self) -> Option<Range> {
        let anchor = self.anchor?;
        if anchor == self.cursor {
            None
        } else {
            Some(Range::new(anchor, self.cursor))
        }
    }

    /// The selected text, or an empty string when nothing is selected.
    pub fn selected_text(&self) -> String {
        self.selection()
            .map(|range| self.buffer.text_in_range(range))
            .unwrap_or_default()
    }

    /// Whether anything is selected.
    pub fn has_selection(&self) -> bool {
        self.selection().is_some()
    }

    /// Drops the selection, keeping the cursor where it is.
    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Selects the whole document.
    pub fn select_all(&mut self) {
        self.anchor = Some(Position::ORIGIN);
        self.cursor = self.buffer.end_position();
        self.desired_column = None;
    }

    /// Selects `range` and places the cursor at its end.
    pub fn select_range(&mut self, range: Range) {
        let range = self.buffer.clamp_range(range);
        self.anchor = Some(range.start);
        self.cursor = range.end;
        self.desired_column = None;
        self.history.seal();
    }

    /// Moves the cursor, optionally extending the selection.
    pub fn move_cursor(&mut self, movement: Movement, mode: SelectionMode) {
        // Moving ends a typing burst, so the next edit starts a new undo step.
        self.history.seal();

        match mode {
            SelectionMode::Extend => {
                if self.anchor.is_none() {
                    self.anchor = Some(self.cursor);
                }
            }
            SelectionMode::Collapse => self.anchor = None,
        }

        let vertical = matches!(
            movement,
            Movement::Up | Movement::Down | Movement::PageUp(_) | Movement::PageDown(_)
        );
        if !vertical {
            self.desired_column = None;
        }

        self.cursor = self.resolve(movement);
    }

    /// Computes the position a movement lands on.
    fn resolve(&mut self, movement: Movement) -> Position {
        let cursor = self.cursor;
        match movement {
            Movement::Left => self
                .buffer
                .position_before(cursor)
                .unwrap_or(Position::ORIGIN),
            Movement::Right => self
                .buffer
                .position_after(cursor)
                .unwrap_or_else(|| self.buffer.end_position()),
            Movement::Up => self.vertical(cursor.line.checked_sub(1)),
            Movement::Down => self.vertical(Some(cursor.line + 1)),
            Movement::PageUp(rows) => self.vertical(Some(cursor.line.saturating_sub(rows))),
            Movement::PageDown(rows) => self.vertical(Some(cursor.line + rows)),
            Movement::LineStart => {
                let indent = self.indentation_end(cursor.line);
                // Smart home: jump to the text, then to the true start.
                if cursor.column > indent {
                    Position::new(cursor.line, indent)
                } else {
                    Position::line_start(cursor.line)
                }
            }
            Movement::LineEnd => Position::new(cursor.line, self.buffer.line_len(cursor.line)),
            Movement::WordLeft => self.word_boundary_before(cursor),
            Movement::WordRight => self.word_boundary_after(cursor),
            Movement::DocumentStart => Position::ORIGIN,
            Movement::DocumentEnd => self.buffer.end_position(),
            Movement::To(position) => self.buffer.clamp(position),
        }
    }

    /// Moves to `line`, honouring the remembered desired column.
    fn vertical(&mut self, line: Option<usize>) -> Position {
        let target = self.desired_column.unwrap_or(self.cursor.column);
        self.desired_column = Some(target);
        let line = line.unwrap_or(0).min(self.buffer.line_count() - 1);
        Position::new(line, target.min(self.buffer.line_len(line)))
    }

    /// The column of the first non-blank character on `line`.
    fn indentation_end(&self, line: usize) -> usize {
        self.buffer
            .line_or_empty(line)
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .count()
    }

    /// Whether `ch` counts as part of a word for word-wise movement.
    fn is_word_char(ch: char) -> bool {
        ch.is_alphanumeric() || matches!(ch, '_' | '.' | '$' | '@' | '?')
    }

    fn word_boundary_before(&self, from: Position) -> Position {
        let mut position = from;
        // Step back over any run of separators, then over the word itself.
        loop {
            let Some(previous) = self.buffer.position_before(position) else {
                return Position::ORIGIN;
            };
            let ch = self.char_at(previous);
            if ch.is_some_and(Self::is_word_char) {
                break;
            }
            position = previous;
            if previous.line != from.line {
                return position;
            }
        }
        while let Some(previous) = self.buffer.position_before(position) {
            match self.char_at(previous) {
                Some(ch) if Self::is_word_char(ch) => position = previous,
                _ => break,
            }
        }
        position
    }

    fn word_boundary_after(&self, from: Position) -> Position {
        let mut position = from;
        let end = self.buffer.end_position();
        while position < end {
            match self.char_at(position) {
                Some(ch) if Self::is_word_char(ch) => break,
                _ => {}
            }
            let Some(next) = self.buffer.position_after(position) else {
                return end;
            };
            if next.line != position.line {
                return next;
            }
            position = next;
        }
        while position < end {
            match self.char_at(position) {
                Some(ch) if Self::is_word_char(ch) => {}
                _ => break,
            }
            let Some(next) = self.buffer.position_after(position) else {
                return end;
            };
            position = next;
        }
        position
    }

    /// The character at `position`, or `None` at a line end.
    fn char_at(&self, position: Position) -> Option<char> {
        self.buffer
            .line(position.line)?
            .chars()
            .nth(position.column)
    }

    /// Applies an edit, records it for undo and moves the cursor past it.
    fn apply(&mut self, range: Range, text: &str) -> Replacement {
        let range = self.buffer.clamp_range(range);
        let cursor_before = self.cursor;
        let result = self.buffer.replace_range(range, text);
        self.history.record(Edit {
            range,
            removed: result.removed.clone(),
            inserted: text.to_owned(),
            inserted_end: result.end,
            cursor_before,
            cursor_after: result.end,
        });
        self.cursor = result.end;
        self.anchor = None;
        self.desired_column = None;
        result
    }

    /// Deletes the selection if there is one, reporting whether it did.
    pub fn delete_selection(&mut self) -> bool {
        match self.selection() {
            Some(range) => {
                self.apply(range, "");
                true
            }
            None => false,
        }
    }

    /// Inserts text at the cursor, replacing the selection if any.
    pub fn insert(&mut self, text: &str) {
        let range = self.selection().unwrap_or(Range::empty(self.cursor));
        self.apply(range, text);
    }

    /// Inserts a single character at the cursor.
    pub fn insert_char(&mut self, ch: char) {
        let mut buffer = [0u8; 4];
        self.insert(ch.encode_utf8(&mut buffer));
    }

    /// Inserts a newline, carrying the current line's indentation with it.
    ///
    /// A line ending in `:` is a label, so the new line is indented one extra
    /// level — the layout assembly source almost always wants.
    pub fn insert_newline(&mut self) {
        let line = self.cursor.line;
        let text = self.buffer.line_or_empty(line);
        let indent: String = text.chars().take_while(|ch| ch.is_whitespace()).collect();

        let before_cursor: String = text.chars().take(self.cursor.column).collect();
        let opens_block =
            before_cursor.trim_end().ends_with(':') && !before_cursor.trim_start().starts_with(';');

        let mut inserted = String::with_capacity(indent.len() + 1);
        inserted.push('\n');
        if opens_block {
            // Indent relative to the label rather than the label's own indent.
            inserted.push_str(&indent);
            inserted.push_str(&" ".repeat(self.indent_width));
        } else {
            inserted.push_str(&indent);
        }
        self.insert(&inserted);
        self.history.seal();
    }

    /// Deletes the character before the cursor, or the selection.
    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if let Some(previous) = self.buffer.position_before(self.cursor) {
            self.apply(Range::new(previous, self.cursor), "");
        }
    }

    /// Deletes the character after the cursor, or the selection.
    pub fn delete_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        if let Some(next) = self.buffer.position_after(self.cursor) {
            self.apply(Range::new(self.cursor, next), "");
        }
    }

    /// Inserts one indent level, or indents every selected line.
    pub fn indent(&mut self) {
        let padding = " ".repeat(self.indent_width);
        match self.selection() {
            Some(range) if range.is_multiline() => {
                self.for_each_selected_line(|_, current| Some(format!("{padding}{current}")));
            }
            _ => self.insert(&padding),
        }
    }

    /// Removes one indent level from the current or selected lines.
    pub fn dedent(&mut self) {
        let width = self.indent_width;
        self.for_each_selected_line(move |_, current| {
            let removable = current
                .chars()
                .take(width)
                .take_while(|ch| *ch == ' ')
                .count();
            if removable == 0 {
                None
            } else {
                Some(current.chars().skip(removable).collect())
            }
        });
    }

    /// Rewrites each line touched by the selection, or the cursor's line.
    ///
    /// Lines are rewritten from the bottom up so that earlier edits do not
    /// invalidate the positions of later ones.
    fn for_each_selected_line<F>(&mut self, mut rewrite: F)
    where
        F: FnMut(usize, &str) -> Option<String>,
    {
        let span = self
            .selection()
            .map(|range| range.line_span())
            .unwrap_or(self.cursor.line..=self.cursor.line);
        let (first, last) = (*span.start(), *span.end());

        let cursor = self.cursor;
        let anchor = self.anchor;
        let mut changed = false;

        for line in (first..=last).rev() {
            let current = self.buffer.line_or_empty(line).to_owned();
            let Some(replacement) = rewrite(line, &current) else {
                continue;
            };
            if replacement == current {
                continue;
            }
            let range = Range::new(
                Position::line_start(line),
                Position::new(line, current.chars().count()),
            );
            let cursor_before = self.cursor;
            let result = self.buffer.replace_range(range, &replacement);
            self.history.record(Edit {
                range,
                removed: result.removed,
                inserted: replacement,
                inserted_end: result.end,
                cursor_before,
                cursor_after: result.end,
            });
            changed = true;
        }

        if changed {
            // Keep the cursor and selection on their original lines; columns
            // are clamped because the lines may have grown or shrunk.
            self.cursor = self.buffer.clamp(cursor);
            self.anchor = anchor.map(|position| self.buffer.clamp(position));
            self.history.seal();
        }
    }

    /// Replaces the text in `range`, leaving the cursor after the new text.
    pub fn replace_range(&mut self, range: Range, text: &str) {
        self.apply(range, text);
        self.history.seal();
    }

    /// Undoes the most recent change.
    ///
    /// Returns `true` when something was undone.
    pub fn undo(&mut self) -> bool {
        match self.history.undo(&mut self.buffer) {
            Some(restored) => {
                self.cursor = self.buffer.clamp(restored.cursor);
                self.anchor = None;
                self.desired_column = None;
                true
            }
            None => false,
        }
    }

    /// Redoes the most recently undone change.
    ///
    /// Returns `true` when something was redone.
    pub fn redo(&mut self) -> bool {
        match self.history.redo(&mut self.buffer) {
            Some(restored) => {
                self.cursor = self.buffer.clamp(restored.cursor);
                self.anchor = None;
                self.desired_column = None;
                true
            }
            None => false,
        }
    }

    /// Marks the document as saved at its current content.
    pub fn mark_saved(&mut self) {
        self.history.mark_saved();
    }

    /// Associates the document with a new path, as used by "save as".
    pub fn set_path(&mut self, path: impl Into<PathBuf>) {
        self.buffer.set_path(path);
    }

    /// Moves the cursor to a one-based line number, as typed by a user.
    ///
    /// Line 0 and lines past the end clamp to the nearest real line rather
    /// than being rejected, so "go to line 9999" lands at the end.
    pub fn go_to_line(&mut self, line_number: usize) {
        let line = line_number.saturating_sub(1);
        self.move_cursor(
            Movement::To(Position::line_start(line)),
            SelectionMode::Collapse,
        );
    }

    /// Scrolls the viewport so the cursor is visible in a window of `height`
    /// rows and `width` columns.
    ///
    /// Called by the renderer, which is the only part of the system that knows
    /// how large the window is.
    pub fn scroll_into_view(&mut self, height: usize, width: usize) {
        if height > 0 {
            if self.cursor.line < self.scroll_line {
                self.scroll_line = self.cursor.line;
            } else if self.cursor.line >= self.scroll_line + height {
                self.scroll_line = self.cursor.line + 1 - height;
            }
            let max_scroll = self.buffer.line_count().saturating_sub(1);
            self.scroll_line = self.scroll_line.min(max_scroll);
        }

        if width > 0 {
            let column = self
                .buffer
                .display_column(self.cursor.line, self.cursor.column);
            if column < self.scroll_column {
                self.scroll_column = column;
            } else if column >= self.scroll_column + width {
                self.scroll_column = column + 1 - width;
            }
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(text: &str) -> Document {
        Document::from_text(text)
    }

    fn at(document: &mut Document, line: usize, column: usize) {
        document.move_cursor(
            Movement::To(Position::new(line, column)),
            SelectionMode::Collapse,
        );
    }

    #[test]
    fn a_new_document_is_empty_and_unmodified() {
        let document = Document::new();
        assert_eq!(document.buffer().to_text(), "");
        assert!(!document.is_modified());
        assert_eq!(document.cursor(), Position::ORIGIN);
        assert!(!document.has_selection());
    }

    #[test]
    fn typing_inserts_text_and_marks_the_document_modified() {
        let mut document = Document::new();
        for ch in "mov".chars() {
            document.insert_char(ch);
        }
        assert_eq!(document.buffer().to_text(), "mov");
        assert_eq!(document.cursor(), Position::new(0, 3));
        assert!(document.is_modified());
    }

    #[test]
    fn a_typing_burst_undoes_as_one_step() {
        let mut document = Document::new();
        for ch in "syscall".chars() {
            document.insert_char(ch);
        }
        assert!(document.undo());
        assert_eq!(document.buffer().to_text(), "");
        assert!(!document.undo(), "burst should have been a single step");
    }

    #[test]
    fn moving_the_cursor_splits_the_undo_burst() {
        let mut document = Document::new();
        document.insert_char('a');
        document.move_cursor(Movement::Left, SelectionMode::Collapse);
        document.insert_char('b');
        document.undo();
        assert_eq!(document.buffer().to_text(), "a");
        document.undo();
        assert_eq!(document.buffer().to_text(), "");
    }

    #[test]
    fn backspace_at_the_start_of_a_line_joins_it_to_the_previous_one() {
        let mut document = document("mov\nret");
        at(&mut document, 1, 0);
        document.backspace();
        assert_eq!(document.buffer().to_text(), "movret");
        assert_eq!(document.cursor(), Position::new(0, 3));
    }

    #[test]
    fn backspace_at_the_very_start_does_nothing() {
        let mut document = document("ret");
        at(&mut document, 0, 0);
        document.backspace();
        assert_eq!(document.buffer().to_text(), "ret");
        assert!(!document.is_modified());
    }

    #[test]
    fn delete_forward_at_the_very_end_does_nothing() {
        let mut document = document("ret");
        document.move_cursor(Movement::DocumentEnd, SelectionMode::Collapse);
        document.delete_forward();
        assert_eq!(document.buffer().to_text(), "ret");
    }

    #[test]
    fn vertical_movement_remembers_the_desired_column() {
        // The behaviour a naive implementation gets wrong: passing through a
        // short line must not permanently shorten the cursor's column.
        let mut document = document("mov rax, 1\nret\nmov rbx, 2");
        at(&mut document, 0, 9);
        document.move_cursor(Movement::Down, SelectionMode::Collapse);
        assert_eq!(
            document.cursor(),
            Position::new(1, 3),
            "clamped to short line"
        );
        document.move_cursor(Movement::Down, SelectionMode::Collapse);
        assert_eq!(
            document.cursor(),
            Position::new(2, 9),
            "column must be restored on the longer line"
        );
    }

    #[test]
    fn horizontal_movement_clears_the_desired_column() {
        let mut document = document("mov rax, 1\nret\nmov rbx, 2");
        at(&mut document, 0, 9);
        document.move_cursor(Movement::Down, SelectionMode::Collapse);
        document.move_cursor(Movement::Left, SelectionMode::Collapse);
        document.move_cursor(Movement::Down, SelectionMode::Collapse);
        assert_eq!(document.cursor(), Position::new(2, 2));
    }

    #[test]
    fn left_and_right_wrap_across_lines() {
        let mut document = document("ab\ncd");
        at(&mut document, 1, 0);
        document.move_cursor(Movement::Left, SelectionMode::Collapse);
        assert_eq!(document.cursor(), Position::new(0, 2));
        document.move_cursor(Movement::Right, SelectionMode::Collapse);
        assert_eq!(document.cursor(), Position::new(1, 0));
    }

    #[test]
    fn movement_at_the_document_edges_is_clamped() {
        let mut document = document("ret");
        document.move_cursor(Movement::Up, SelectionMode::Collapse);
        assert_eq!(document.cursor(), Position::ORIGIN);
        document.move_cursor(Movement::PageDown(500), SelectionMode::Collapse);
        assert_eq!(document.cursor().line, 0);
        document.move_cursor(Movement::Left, SelectionMode::Collapse);
        assert_eq!(document.cursor(), Position::ORIGIN);
    }

    #[test]
    fn smart_home_toggles_between_text_and_column_zero() {
        let mut document = document("    mov rax, 1");
        at(&mut document, 0, 10);
        document.move_cursor(Movement::LineStart, SelectionMode::Collapse);
        assert_eq!(document.cursor(), Position::new(0, 4), "first to the text");
        document.move_cursor(Movement::LineStart, SelectionMode::Collapse);
        assert_eq!(
            document.cursor(),
            Position::new(0, 0),
            "then to column zero"
        );
    }

    #[test]
    fn word_movement_steps_over_identifiers() {
        let mut document = document("    mov rax, rbx");
        at(&mut document, 0, 0);
        document.move_cursor(Movement::WordRight, SelectionMode::Collapse);
        assert_eq!(document.cursor(), Position::new(0, 7));
        document.move_cursor(Movement::WordRight, SelectionMode::Collapse);
        assert_eq!(document.cursor(), Position::new(0, 11));
        document.move_cursor(Movement::WordLeft, SelectionMode::Collapse);
        assert_eq!(document.cursor(), Position::new(0, 8));
    }

    #[test]
    fn extending_a_selection_keeps_the_anchor() {
        let mut document = document("mov rax, 1");
        at(&mut document, 0, 4);
        for _ in 0..3 {
            document.move_cursor(Movement::Right, SelectionMode::Extend);
        }
        assert_eq!(document.selected_text(), "rax");
        assert_eq!(
            document.selection(),
            Some(Range::new(Position::new(0, 4), Position::new(0, 7)))
        );
    }

    #[test]
    fn a_collapsing_movement_drops_the_selection() {
        let mut document = document("mov rax, 1");
        document.select_all();
        assert!(document.has_selection());
        document.move_cursor(Movement::Right, SelectionMode::Collapse);
        assert!(!document.has_selection());
    }

    #[test]
    fn an_empty_selection_is_reported_as_no_selection() {
        let mut document = document("ret");
        at(&mut document, 0, 1);
        document.move_cursor(Movement::Right, SelectionMode::Extend);
        document.move_cursor(Movement::Left, SelectionMode::Extend);
        assert!(
            !document.has_selection(),
            "anchor equal to cursor is no selection"
        );
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut document = document("mov rax, 1");
        document.select_range(Range::new(Position::new(0, 4), Position::new(0, 7)));
        document.insert("rbx");
        assert_eq!(document.buffer().to_text(), "mov rbx, 1");
        assert!(!document.has_selection());
    }

    #[test]
    fn backspace_deletes_a_selection_rather_than_one_character() {
        let mut document = document("mov rax, 1");
        document.select_range(Range::new(Position::new(0, 3), Position::new(0, 10)));
        document.backspace();
        assert_eq!(document.buffer().to_text(), "mov");
    }

    #[test]
    fn select_all_covers_the_whole_document() {
        let mut document = document("one\ntwo\nthree");
        document.select_all();
        assert_eq!(document.selected_text(), "one\ntwo\nthree");
    }

    #[test]
    fn newline_carries_the_current_indentation() {
        let mut document = document("    mov rax, 1");
        document.move_cursor(Movement::LineEnd, SelectionMode::Collapse);
        document.insert_newline();
        assert_eq!(document.buffer().to_text(), "    mov rax, 1\n    ");
        assert_eq!(document.cursor(), Position::new(1, 4));
    }

    #[test]
    fn newline_after_a_label_adds_an_indent_level() {
        let mut document = document("_start:");
        document.move_cursor(Movement::LineEnd, SelectionMode::Collapse);
        document.insert_newline();
        assert_eq!(document.buffer().to_text(), "_start:\n    ");
    }

    #[test]
    fn newline_after_a_comment_ending_in_a_colon_does_not_indent() {
        let mut document = document("; note:");
        document.move_cursor(Movement::LineEnd, SelectionMode::Collapse);
        document.insert_newline();
        assert_eq!(document.buffer().to_text(), "; note:\n");
    }

    #[test]
    fn indent_inserts_spaces_at_the_cursor() {
        let mut document = document("ret");
        at(&mut document, 0, 0);
        document.indent();
        assert_eq!(document.buffer().to_text(), "    ret");
    }

    #[test]
    fn indent_shifts_every_selected_line() {
        let mut document = document("mov rax, 1\nret\nnop");
        document.select_range(Range::new(Position::new(0, 0), Position::new(2, 3)));
        document.indent();
        assert_eq!(
            document.buffer().to_text(),
            "    mov rax, 1\n    ret\n    nop"
        );
    }

    #[test]
    fn dedent_removes_one_level_and_stops_at_column_zero() {
        let mut document = document("        ret");
        document.dedent();
        assert_eq!(document.buffer().to_text(), "    ret");
        document.dedent();
        assert_eq!(document.buffer().to_text(), "ret");
        document.dedent();
        assert_eq!(
            document.buffer().to_text(),
            "ret",
            "dedent must not go negative"
        );
    }

    #[test]
    fn dedent_of_a_multiline_selection_undoes_as_one_step() {
        let mut document = document("    a\n    b\n    c");
        document.select_range(Range::new(Position::new(0, 0), Position::new(2, 5)));
        document.dedent();
        assert_eq!(document.buffer().to_text(), "a\nb\nc");
        document.undo();
        assert_eq!(document.buffer().to_text(), "    a\n    b\n    c");
    }

    #[test]
    fn go_to_line_is_one_based_and_clamps() {
        let mut document = document("one\ntwo\nthree");
        document.go_to_line(2);
        assert_eq!(document.cursor(), Position::new(1, 0));
        document.go_to_line(0);
        assert_eq!(document.cursor(), Position::new(0, 0));
        document.go_to_line(9999);
        assert_eq!(document.cursor().line, 2);
    }

    #[test]
    fn saving_clears_the_modified_flag() {
        let mut document = document("ret");
        document.insert_char('!');
        assert!(document.is_modified());
        document.mark_saved();
        assert!(!document.is_modified());
    }

    #[test]
    fn undo_restores_the_cursor_to_where_the_edit_began() {
        let mut document = document("mov rax, 1");
        at(&mut document, 0, 10);
        document.insert("\n    ret");
        document.undo();
        assert_eq!(document.buffer().to_text(), "mov rax, 1");
        assert_eq!(document.cursor(), Position::new(0, 10));
    }

    #[test]
    fn redo_reapplies_and_moves_the_cursor_forward() {
        let mut document = document("ret");
        document.move_cursor(Movement::DocumentEnd, SelectionMode::Collapse);
        document.insert("\nnop");
        document.undo();
        assert!(document.redo());
        assert_eq!(document.buffer().to_text(), "ret\nnop");
        assert_eq!(document.cursor(), Position::new(1, 3));
    }

    #[test]
    fn scrolling_follows_the_cursor_down_and_back_up() {
        let text = (0..100)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let mut document = document(&text);
        document.go_to_line(50);
        document.scroll_into_view(20, 80);
        assert!(document.scroll_line() <= 49);
        assert!(49 < document.scroll_line() + 20);

        document.go_to_line(1);
        document.scroll_into_view(20, 80);
        assert_eq!(document.scroll_line(), 0);
    }

    #[test]
    fn scrolling_with_a_zero_sized_viewport_does_not_panic() {
        let mut document = document("a\nb\nc");
        document.go_to_line(3);
        document.scroll_into_view(0, 0);
        assert_eq!(document.scroll_line(), 0);
    }

    #[test]
    fn horizontal_scrolling_tracks_wide_lines() {
        let mut document = document(&"x".repeat(200));
        at(&mut document, 0, 150);
        document.scroll_into_view(10, 40);
        assert!(document.scroll_column() > 0);
        assert!(150 < document.scroll_column() + 40);
    }

    #[test]
    fn indent_width_is_clamped_to_a_usable_range() {
        let mut document = Document::new();
        document.set_indent_width(0);
        assert_eq!(document.indent_width(), 1);
        document.set_indent_width(999);
        assert_eq!(document.indent_width(), 16);
    }

    #[test]
    fn editing_never_leaves_the_cursor_outside_the_buffer() {
        // A blunt check that no operation can strand the cursor.
        let mut document = document("mov rax, 1\nret\n");
        type Operation = Box<dyn Fn(&mut Document)>;
        let operations: Vec<Operation> = vec![
            Box::new(|d: &mut Document| d.select_all()),
            Box::new(|d: &mut Document| d.backspace()),
            Box::new(|d: &mut Document| d.insert("hello\nworld")),
            Box::new(|d: &mut Document| d.dedent()),
            Box::new(|d: &mut Document| d.indent()),
            Box::new(|d: &mut Document| {
                d.undo();
            }),
            Box::new(|d: &mut Document| d.delete_forward()),
            Box::new(|d: &mut Document| d.insert_newline()),
            Box::new(|d: &mut Document| {
                d.redo();
            }),
        ];
        for operation in &operations {
            operation(&mut document);
            let cursor = document.cursor();
            assert_eq!(
                cursor,
                document.buffer().clamp(cursor),
                "cursor escaped the buffer"
            );
        }
    }
}

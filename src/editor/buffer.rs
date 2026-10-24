//! The text storage underlying every open file.
//!
//! # Design
//!
//! Lines are stored as separate `String`s. Assembly source is line-oriented
//! and lines are short, so the cost of a line-vector splice is irrelevant next
//! to the clarity it buys: line indexing, per-line syntax highlighting and
//! diagnostic mapping all become direct lookups.
//!
//! Every mutation funnels through the single primitive
//! [`TextBuffer::replace_range`]. Insertion, deletion, paste and
//! search-and-replace are all expressed in terms of it. That matters because
//! undo is implemented as the inverse of exactly one operation: if edits could
//! reach the line vector by other routes, the history would silently drift out
//! of sync with the text.
//!
//! The buffer knows nothing about cursors, selections or rendering. Those live
//! in [`crate::editor::Document`], which keeps this type trivially testable.

use std::path::{Path, PathBuf};

use unicode_width::UnicodeWidthChar;

use super::position::{Position, Range};

/// Width in columns that a tab character advances to.
pub const TAB_WIDTH: usize = 8;

/// A line-oriented, UTF-8 text buffer.
///
/// The buffer always contains at least one line, so `lines()[0]` is never a
/// panic. An "empty" buffer is one line holding an empty string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBuffer {
    lines: Vec<String>,
    path: Option<PathBuf>,
}

impl TextBuffer {
    /// Creates an empty buffer with no associated path.
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            path: None,
        }
    }

    /// Creates a buffer from text, splitting on newlines.
    ///
    /// Both LF and CRLF line endings are accepted; carriage returns are
    /// stripped so the in-memory representation is always LF-separated.
    pub fn from_text(text: &str) -> Self {
        let mut lines: Vec<String> = text
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
            .collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self { lines, path: None }
    }

    /// Creates a buffer from text and records the path it came from.
    pub fn from_file_contents(path: impl Into<PathBuf>, text: &str) -> Self {
        let mut buffer = Self::from_text(text);
        buffer.path = Some(path.into());
        buffer
    }

    /// The path this buffer is associated with, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Associates the buffer with a path, as used by "save as".
    pub fn set_path(&mut self, path: impl Into<PathBuf>) {
        self.path = Some(path.into());
    }

    /// The display name: the file name, or `[untitled]`.
    pub fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "[untitled]".to_owned())
    }

    /// All lines in the buffer.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The number of lines, always at least one.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Returns line `index`, or `None` when out of bounds.
    pub fn line(&self, index: usize) -> Option<&str> {
        self.lines.get(index).map(String::as_str)
    }

    /// Returns line `index`, or an empty string when out of bounds.
    ///
    /// Convenient for rendering, where an out-of-range line simply draws
    /// nothing rather than being an error.
    pub fn line_or_empty(&self, index: usize) -> &str {
        self.line(index).unwrap_or("")
    }

    /// The number of characters on line `index`.
    pub fn line_len(&self, index: usize) -> usize {
        self.line(index).map_or(0, |line| line.chars().count())
    }

    /// The last valid position in the buffer.
    pub fn end_position(&self) -> Position {
        let line = self.lines.len().saturating_sub(1);
        Position::new(line, self.line_len(line))
    }

    /// Returns `true` when the buffer holds no text at all.
    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Renders the buffer back to a single string with LF endings.
    pub fn to_text(&self) -> String {
        self.lines.join("\n")
    }

    /// Clamps a position to one that actually exists in the buffer.
    ///
    /// Out-of-range lines clamp to the last line and out-of-range columns to
    /// the end of their line. Callers can therefore accept arbitrary
    /// user-supplied coordinates without risking a panic.
    pub fn clamp(&self, position: Position) -> Position {
        let line = position.line.min(self.lines.len().saturating_sub(1));
        let column = position.column.min(self.line_len(line));
        Position::new(line, column)
    }

    /// Converts a character column into a byte offset within `line`.
    ///
    /// Columns past the end of the line clamp to the line's byte length.
    fn byte_offset(&self, line: usize, column: usize) -> usize {
        let text = self.line_or_empty(line);
        text.char_indices()
            .nth(column)
            .map_or(text.len(), |(offset, _)| offset)
    }

    /// The rendered width, in terminal cells, of `line` up to `column`.
    ///
    /// Tabs advance to the next multiple of [`TAB_WIDTH`] and wide characters
    /// count as two cells, so this is the value a renderer needs to place a
    /// cursor.
    pub fn display_column(&self, line: usize, column: usize) -> usize {
        let mut width = 0usize;
        for ch in self.line_or_empty(line).chars().take(column) {
            width += char_display_width(ch, width);
        }
        width
    }

    /// The total rendered width of `line`.
    pub fn display_width(&self, line: usize) -> usize {
        self.display_column(line, self.line_len(line))
    }

    /// Returns the text covered by `range`.
    pub fn text_in_range(&self, range: Range) -> String {
        let range = self.clamp_range(range);
        if range.start.line == range.end.line {
            let text = self.line_or_empty(range.start.line);
            let start = self.byte_offset(range.start.line, range.start.column);
            let end = self.byte_offset(range.start.line, range.end.column);
            return text[start..end].to_owned();
        }

        let mut out = String::new();
        let first = self.line_or_empty(range.start.line);
        out.push_str(&first[self.byte_offset(range.start.line, range.start.column)..]);
        for index in (range.start.line + 1)..range.end.line {
            out.push('\n');
            out.push_str(self.line_or_empty(index));
        }
        out.push('\n');
        let last = self.line_or_empty(range.end.line);
        out.push_str(&last[..self.byte_offset(range.end.line, range.end.column)]);
        out
    }

    /// Clamps both endpoints of a range into the buffer.
    pub fn clamp_range(&self, range: Range) -> Range {
        Range::new(self.clamp(range.start), self.clamp(range.end))
    }

    /// Replaces the text covered by `range` with `text`.
    ///
    /// This is the only mutating primitive in the buffer. It returns the text
    /// that was removed together with the position just past the inserted
    /// text, which is exactly what the undo history and the cursor need.
    ///
    /// The range is clamped first, so out-of-range coordinates are corrected
    /// rather than producing a panic.
    pub fn replace_range(&mut self, range: Range, text: &str) -> Replacement {
        let range = self.clamp_range(range);
        let removed = self.text_in_range(range);

        let start_byte = self.byte_offset(range.start.line, range.start.column);
        let end_byte = self.byte_offset(range.end.line, range.end.column);

        let prefix = self.lines[range.start.line][..start_byte].to_owned();
        let suffix = self.lines[range.end.line][end_byte..].to_owned();

        let inserted: Vec<&str> = text.split('\n').collect();
        let end_position = if inserted.len() == 1 {
            Position::new(
                range.start.line,
                range.start.column + inserted[0].chars().count(),
            )
        } else {
            Position::new(
                range.start.line + inserted.len() - 1,
                inserted[inserted.len() - 1].chars().count(),
            )
        };

        let mut replacement: Vec<String> = Vec::with_capacity(inserted.len());
        for (index, chunk) in inserted.iter().enumerate() {
            let mut line = String::new();
            if index == 0 {
                line.push_str(&prefix);
            }
            line.push_str(chunk);
            if index == inserted.len() - 1 {
                line.push_str(&suffix);
            }
            replacement.push(line);
        }

        self.lines
            .splice(range.start.line..=range.end.line, replacement);

        Replacement {
            removed,
            end: end_position,
        }
    }

    /// Inserts `text` at `position`, returning the position just past it.
    pub fn insert(&mut self, position: Position, text: &str) -> Replacement {
        self.replace_range(Range::empty(position), text)
    }

    /// Deletes the text covered by `range`, returning what was removed.
    pub fn delete(&mut self, range: Range) -> Replacement {
        self.replace_range(range, "")
    }

    /// The position one character before `position`, or `None` at the origin.
    ///
    /// Moving back from column zero lands at the end of the previous line,
    /// which is what backspace needs in order to join lines.
    pub fn position_before(&self, position: Position) -> Option<Position> {
        let position = self.clamp(position);
        if position.column > 0 {
            Some(Position::new(position.line, position.column - 1))
        } else if position.line > 0 {
            let previous = position.line - 1;
            Some(Position::new(previous, self.line_len(previous)))
        } else {
            None
        }
    }

    /// The position one character after `position`, or `None` at the end.
    pub fn position_after(&self, position: Position) -> Option<Position> {
        let position = self.clamp(position);
        if position.column < self.line_len(position.line) {
            Some(Position::new(position.line, position.column + 1))
        } else if position.line + 1 < self.lines.len() {
            Some(Position::line_start(position.line + 1))
        } else {
            None
        }
    }
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// The outcome of a [`TextBuffer::replace_range`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    /// The text that was removed, used to build the inverse edit.
    pub removed: String,
    /// The position immediately after the inserted text.
    pub end: Position,
}

/// The number of terminal cells `ch` occupies when drawn at `column`.
///
/// Tabs are variable width, so the current column is required to compute the
/// distance to the next tab stop.
pub fn char_display_width(ch: char, column: usize) -> usize {
    if ch == '\t' {
        TAB_WIDTH - (column % TAB_WIDTH)
    } else {
        ch.width().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(text: &str) -> TextBuffer {
        TextBuffer::from_text(text)
    }

    #[test]
    fn empty_buffer_has_one_empty_line() {
        let buffer = TextBuffer::new();
        assert_eq!(buffer.line_count(), 1);
        assert_eq!(buffer.line(0), Some(""));
        assert!(buffer.is_empty());
    }

    #[test]
    fn crlf_endings_are_normalised_to_lf() {
        let buffer = buffer("mov rax, 1\r\nret\r\n");
        assert_eq!(buffer.lines(), ["mov rax, 1", "ret", ""]);
        assert_eq!(buffer.to_text(), "mov rax, 1\nret\n");
    }

    #[test]
    fn round_trips_text_unchanged() {
        let text = "section .text\nglobal _start\n\n_start:\n    ret\n";
        assert_eq!(buffer(text).to_text(), text);
    }

    #[test]
    fn insert_within_a_line() {
        let mut buffer = buffer("mov rax");
        let result = buffer.insert(Position::new(0, 7), ", 1");
        assert_eq!(buffer.to_text(), "mov rax, 1");
        assert_eq!(result.end, Position::new(0, 10));
        assert_eq!(result.removed, "");
    }

    #[test]
    fn insert_newline_splits_the_line() {
        let mut buffer = buffer("mov rax, 1");
        let result = buffer.insert(Position::new(0, 3), "\n   ");
        assert_eq!(buffer.to_text(), "mov\n    rax, 1");
        assert_eq!(result.end, Position::new(1, 3));
    }

    #[test]
    fn delete_across_lines_joins_them() {
        let mut buffer = buffer("mov rax, 1\nret\nnop");
        let range = Range::new(Position::new(0, 3), Position::new(2, 0));
        let result = buffer.delete(range);
        assert_eq!(buffer.to_text(), "movnop");
        assert_eq!(result.removed, " rax, 1\nret\n");
        assert_eq!(result.end, Position::new(0, 3));
    }

    #[test]
    fn replace_returns_the_text_it_removed() {
        let mut buffer = buffer("mov rax, 1");
        let range = Range::new(Position::new(0, 4), Position::new(0, 7));
        let result = buffer.replace_range(range, "rbx");
        assert_eq!(result.removed, "rax");
        assert_eq!(buffer.to_text(), "mov rbx, 1");
    }

    #[test]
    fn replacement_is_exactly_invertible() {
        // The property undo depends on: applying the inverse of a replacement
        // restores the original text byte for byte.
        let original = "section .text\n_start:\n    mov rax, 60\n    syscall\n";
        let mut buffer = buffer(original);
        let range = Range::new(Position::new(1, 0), Position::new(2, 8));
        let result = buffer.replace_range(range, "main:\n  xor");
        let inverse_end = result.end;
        buffer.replace_range(Range::new(range.start, inverse_end), &result.removed);
        assert_eq!(buffer.to_text(), original);
    }

    #[test]
    fn out_of_range_positions_clamp_instead_of_panicking() {
        let mut buffer = buffer("ret");
        let far = Position::new(999, 999);
        assert_eq!(buffer.clamp(far), Position::new(0, 3));
        let result = buffer.insert(far, "!");
        assert_eq!(buffer.to_text(), "ret!");
        assert_eq!(result.end, Position::new(0, 4));
    }

    #[test]
    fn multibyte_characters_are_not_split() {
        let mut buffer = buffer("; ölçüm değeri");
        // Column 3 is a character boundary even though it is not byte 3.
        let range = Range::new(Position::new(0, 2), Position::new(0, 7));
        assert_eq!(buffer.text_in_range(range), "ölçüm");
        buffer.replace_range(range, "value");
        assert_eq!(buffer.to_text(), "; value değeri");
    }

    #[test]
    fn text_in_range_spans_multiple_lines() {
        let buffer = buffer("one\ntwo\nthree");
        let range = Range::new(Position::new(0, 1), Position::new(2, 2));
        assert_eq!(buffer.text_in_range(range), "ne\ntwo\nth");
    }

    #[test]
    fn display_column_expands_tabs_to_stops() {
        let buffer = buffer("\tmov");
        assert_eq!(buffer.display_column(0, 0), 0);
        assert_eq!(buffer.display_column(0, 1), TAB_WIDTH);
        assert_eq!(buffer.display_column(0, 2), TAB_WIDTH + 1);
    }

    #[test]
    fn display_column_counts_wide_characters_as_two_cells() {
        let buffer = buffer("日本");
        assert_eq!(buffer.display_column(0, 1), 2);
        assert_eq!(buffer.display_width(0), 4);
    }

    #[test]
    fn position_before_crosses_the_line_boundary() {
        let buffer = buffer("ab\ncd");
        assert_eq!(
            buffer.position_before(Position::new(1, 0)),
            Some(Position::new(0, 2))
        );
        assert_eq!(buffer.position_before(Position::ORIGIN), None);
    }

    #[test]
    fn position_after_crosses_the_line_boundary() {
        let buffer = buffer("ab\ncd");
        assert_eq!(
            buffer.position_after(Position::new(0, 2)),
            Some(Position::new(1, 0))
        );
        assert_eq!(buffer.position_after(Position::new(1, 2)), None);
    }

    #[test]
    fn display_name_falls_back_when_there_is_no_path() {
        let mut buffer = TextBuffer::new();
        assert_eq!(buffer.display_name(), "[untitled]");
        buffer.set_path("/tmp/main.asm");
        assert_eq!(buffer.display_name(), "main.asm");
    }
}

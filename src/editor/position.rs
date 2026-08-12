//! Cursor positions and ranges within a text buffer.
//!
//! Positions are expressed as a zero-based line index plus a zero-based
//! *character* column, never a byte offset. Byte offsets leak UTF-8 encoding
//! details into every caller and make it easy to split a multi-byte character
//! by accident; character columns cannot.

use std::cmp::Ordering;

/// A zero-based cursor location in a buffer.
///
/// `column` counts characters, not bytes and not display cells. Use
/// [`crate::editor::buffer::TextBuffer::display_column`] when a rendered
/// x-offset is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Position {
    /// Zero-based line index.
    pub line: usize,
    /// Zero-based character offset within the line.
    pub column: usize,
}

impl Position {
    /// Creates a position from a line and character column.
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    /// The start of the buffer.
    pub const ORIGIN: Position = Position { line: 0, column: 0 };

    /// Returns the position at the start of `line`.
    pub const fn line_start(line: usize) -> Self {
        Self { line, column: 0 }
    }
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> Ordering {
        self.line
            .cmp(&other.line)
            .then_with(|| self.column.cmp(&other.column))
    }
}

/// An ordered pair of positions describing a span of text.
///
/// A range is always normalised so that `start <= end`, which removes the
/// "which end is the anchor" question from every consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Range {
    /// The earlier of the two endpoints.
    pub start: Position,
    /// The later of the two endpoints.
    pub end: Position,
}

impl Range {
    /// Creates a normalised range from two endpoints in any order.
    pub fn new(a: Position, b: Position) -> Self {
        if a <= b {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    /// Creates an empty range at `position`.
    pub const fn empty(position: Position) -> Self {
        Self {
            start: position,
            end: position,
        }
    }

    /// Returns `true` when the range covers no characters.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Returns `true` when the range spans more than one line.
    pub fn is_multiline(&self) -> bool {
        self.start.line != self.end.line
    }

    /// Returns `true` when `position` falls inside the range, end-exclusive.
    pub fn contains(&self, position: Position) -> bool {
        position >= self.start && position < self.end
    }

    /// The inclusive range of line indices the range touches.
    pub fn line_span(&self) -> std::ops::RangeInclusive<usize> {
        self.start.line..=self.end.line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_order_by_line_then_column() {
        assert!(Position::new(0, 9) < Position::new(1, 0));
        assert!(Position::new(2, 3) < Position::new(2, 4));
        assert_eq!(Position::new(1, 1), Position::new(1, 1));
    }

    #[test]
    fn range_normalises_reversed_endpoints() {
        let a = Position::new(4, 2);
        let b = Position::new(1, 7);
        let range = Range::new(a, b);
        assert_eq!(range.start, b);
        assert_eq!(range.end, a);
    }

    #[test]
    fn empty_range_contains_nothing() {
        let range = Range::empty(Position::new(3, 3));
        assert!(range.is_empty());
        assert!(!range.contains(Position::new(3, 3)));
    }

    #[test]
    fn contains_is_end_exclusive() {
        let range = Range::new(Position::new(0, 2), Position::new(0, 5));
        assert!(!range.contains(Position::new(0, 1)));
        assert!(range.contains(Position::new(0, 2)));
        assert!(range.contains(Position::new(0, 4)));
        assert!(!range.contains(Position::new(0, 5)));
    }

    #[test]
    fn line_span_covers_both_endpoints() {
        let range = Range::new(Position::new(2, 0), Position::new(5, 1));
        let lines: Vec<usize> = range.line_span().collect();
        assert_eq!(lines, vec![2, 3, 4, 5]);
        assert!(range.is_multiline());
    }
}

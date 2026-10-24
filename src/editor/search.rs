//! Incremental search and replace over a buffer.
//!
//! Matching is plain substring matching rather than regular expressions. For
//! assembly source the queries people actually type are register names, label
//! names and mnemonics, and the two options that matter for those — case
//! sensitivity and whole-word matching — are cheap to provide exactly. A regex
//! engine would add a dependency and a class of user-facing errors (invalid
//! patterns) for very little gain here.
//!
//! Searching never mutates the buffer; [`replace_all`] returns the edits to
//! perform so the caller can route them through the undo history rather than
//! writing to the buffer behind history's back.

use super::buffer::TextBuffer;
use super::position::{Position, Range};

/// How a query is matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOptions {
    /// Require an exact case match.
    pub case_sensitive: bool,
    /// Require the match to be bounded by non-word characters.
    pub whole_word: bool,
}

impl SearchOptions {
    /// Case-insensitive matching anywhere in a line, the default.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the options with case sensitivity set.
    pub fn case_sensitive(mut self, value: bool) -> Self {
        self.case_sensitive = value;
        self
    }

    /// Returns the options with whole-word matching set.
    pub fn whole_word(mut self, value: bool) -> Self {
        self.whole_word = value;
        self
    }
}

/// A located match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    /// The span the match covers.
    pub range: Range,
}

/// Whether `ch` counts as part of a word for whole-word matching.
///
/// Includes the characters NASM allows in identifiers, so searching for `rax`
/// with whole-word matching does not match inside `rax_backup`.
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '.' | '$' | '@' | '?')
}

/// Finds every match of `query` in `buffer`, in document order.
///
/// An empty query matches nothing, so an in-progress search box does not
/// select the entire file.
pub fn find_all(buffer: &TextBuffer, query: &str, options: SearchOptions) -> Vec<Match> {
    if query.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for (line_index, line) in buffer.lines().iter().enumerate() {
        matches.extend(find_in_line(line, line_index, query, options));
    }
    matches
}

/// Finds every match within a single line.
fn find_in_line(line: &str, line_index: usize, query: &str, options: SearchOptions) -> Vec<Match> {
    let haystack: Vec<char> = if options.case_sensitive {
        line.chars().collect()
    } else {
        line.chars().flat_map(char::to_lowercase).collect()
    };
    let needle: Vec<char> = if options.case_sensitive {
        query.chars().collect()
    } else {
        query.chars().flat_map(char::to_lowercase).collect()
    };

    // Case folding can change character counts (rare, but real: 'İ' folds to
    // two chars). When it does, column arithmetic would be wrong, so fall back
    // to case-sensitive matching rather than reporting a bad range.
    if !options.case_sensitive
        && (haystack.len() != line.chars().count() || needle.len() != query.chars().count())
    {
        return find_in_line(line, line_index, query, options.case_sensitive(true));
    }

    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut start = 0usize;
    while start + needle.len() <= haystack.len() {
        if haystack[start..start + needle.len()] == needle[..] {
            let end = start + needle.len();
            if !options.whole_word || is_word_bounded(&haystack, start, end) {
                matches.push(Match {
                    range: Range::new(
                        Position::new(line_index, start),
                        Position::new(line_index, end),
                    ),
                });
                // Advance past the match so overlapping hits are not reported
                // twice; searching for "aa" in "aaaa" yields two matches.
                start = end;
                continue;
            }
        }
        start += 1;
    }
    matches
}

/// Whether the span `start..end` is bounded by non-word characters.
fn is_word_bounded(haystack: &[char], start: usize, end: usize) -> bool {
    let before_ok = start == 0 || !is_word_char(haystack[start - 1]);
    let after_ok = end >= haystack.len() || !is_word_char(haystack[end]);
    before_ok && after_ok
}

/// The first match at or after `from`, wrapping to the start of the buffer.
///
/// Wrapping is what makes repeated "find next" usable; the caller can detect a
/// wrap by comparing the result's position against `from`.
pub fn find_next(
    buffer: &TextBuffer,
    query: &str,
    from: Position,
    options: SearchOptions,
) -> Option<Match> {
    let matches = find_all(buffer, query, options);
    matches
        .iter()
        .find(|found| found.range.start >= from)
        .or_else(|| matches.first())
        .copied()
}

/// The last match before `from`, wrapping to the end of the buffer.
pub fn find_previous(
    buffer: &TextBuffer,
    query: &str,
    from: Position,
    options: SearchOptions,
) -> Option<Match> {
    let matches = find_all(buffer, query, options);
    matches
        .iter()
        .rev()
        .find(|found| found.range.end <= from)
        .or_else(|| matches.last())
        .copied()
}

/// The replacements needed to replace every match of `query` with `replacement`.
///
/// Returned in reverse document order so a caller can apply them one after
/// another without earlier edits shifting the positions of later ones.
pub fn replace_all(
    buffer: &TextBuffer,
    query: &str,
    replacement: &str,
    options: SearchOptions,
) -> Vec<(Range, String)> {
    find_all(buffer, query, options)
        .into_iter()
        .rev()
        .map(|found| (found.range, replacement.to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(text: &str) -> TextBuffer {
        TextBuffer::from_text(text)
    }

    fn ranges(matches: &[Match]) -> Vec<(usize, usize, usize)> {
        matches
            .iter()
            .map(|m| (m.range.start.line, m.range.start.column, m.range.end.column))
            .collect()
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        let buffer = buffer("mov rax, 1");
        assert!(find_all(&buffer, "", SearchOptions::new()).is_empty());
    }

    #[test]
    fn finds_every_occurrence_in_document_order() {
        let buffer = buffer("mov rax, 1\nadd rax, rbx\nret");
        let found = find_all(&buffer, "rax", SearchOptions::new());
        assert_eq!(ranges(&found), vec![(0, 4, 7), (1, 4, 7)]);
    }

    #[test]
    fn matching_is_case_insensitive_by_default() {
        let buffer = buffer("MOV RAX, 1");
        let found = find_all(&buffer, "rax", SearchOptions::new());
        assert_eq!(ranges(&found), vec![(0, 4, 7)]);
    }

    #[test]
    fn case_sensitive_matching_respects_case() {
        let buffer = buffer("MOV RAX, 1\nmov rax, 2");
        let options = SearchOptions::new().case_sensitive(true);
        let found = find_all(&buffer, "rax", options);
        assert_eq!(ranges(&found), vec![(1, 4, 7)]);
    }

    #[test]
    fn whole_word_matching_rejects_substrings() {
        let buffer = buffer("mov rax, 1\nmov rax_backup, 2");
        let options = SearchOptions::new().whole_word(true);
        let found = find_all(&buffer, "rax", options);
        assert_eq!(ranges(&found), vec![(0, 4, 7)]);
    }

    #[test]
    fn whole_word_matching_allows_punctuation_boundaries() {
        let buffer = buffer("    mov [rax], 1");
        let options = SearchOptions::new().whole_word(true);
        assert_eq!(find_all(&buffer, "rax", options).len(), 1);
    }

    #[test]
    fn a_dot_is_part_of_a_word_so_local_labels_match_exactly() {
        // `.loop` and `loop` are different symbols in NASM.
        let buffer = buffer("    jmp .loop\n    loop .loop");
        let options = SearchOptions::new().whole_word(true);
        assert_eq!(find_all(&buffer, "loop", options).len(), 1);
        assert_eq!(find_all(&buffer, ".loop", options).len(), 2);
    }

    #[test]
    fn overlapping_matches_are_not_reported_twice() {
        let buffer = buffer("aaaa");
        let found = find_all(&buffer, "aa", SearchOptions::new());
        assert_eq!(ranges(&found), vec![(0, 0, 2), (0, 2, 4)]);
    }

    #[test]
    fn a_query_longer_than_the_line_matches_nothing() {
        let buffer = buffer("ab");
        assert!(find_all(&buffer, "abcdef", SearchOptions::new()).is_empty());
    }

    #[test]
    fn multibyte_text_yields_character_columns_not_byte_offsets() {
        let buffer = buffer("; ölçüm rax değeri");
        let found = find_all(&buffer, "rax", SearchOptions::new());
        // "; ölçüm " is 8 characters, so the match starts at column 8.
        assert_eq!(ranges(&found), vec![(0, 8, 11)]);
    }

    #[test]
    fn find_next_starts_at_the_cursor() {
        let buffer = buffer("rax\nrax\nrax");
        let found = find_next(&buffer, "rax", Position::new(1, 0), SearchOptions::new());
        assert_eq!(found.map(|m| m.range.start), Some(Position::new(1, 0)));
    }

    #[test]
    fn find_next_wraps_to_the_top() {
        let buffer = buffer("rax\nrbx\n");
        let found = find_next(&buffer, "rax", Position::new(2, 0), SearchOptions::new());
        assert_eq!(found.map(|m| m.range.start), Some(Position::new(0, 0)));
    }

    #[test]
    fn find_previous_wraps_to_the_bottom() {
        let buffer = buffer("rax\nrbx\nrax");
        let found = find_previous(&buffer, "rax", Position::ORIGIN, SearchOptions::new());
        assert_eq!(found.map(|m| m.range.start), Some(Position::new(2, 0)));
    }

    #[test]
    fn find_next_on_no_match_returns_nothing() {
        let buffer = buffer("mov rax, 1");
        assert!(find_next(&buffer, "zzz", Position::ORIGIN, SearchOptions::new()).is_none());
        assert!(find_previous(&buffer, "zzz", Position::ORIGIN, SearchOptions::new()).is_none());
    }

    #[test]
    fn replace_all_returns_edits_in_reverse_order() {
        // Reverse order is what lets a caller apply them sequentially without
        // recomputing positions after each edit.
        let buffer = buffer("mov rax, 1\nadd rax, rbx");
        let edits = replace_all(&buffer, "rax", "rcx", SearchOptions::new());
        assert_eq!(edits.len(), 2);
        assert!(edits[0].0.start > edits[1].0.start, "must be reverse order");
        assert_eq!(edits[0].1, "rcx");
    }

    #[test]
    fn applying_replacements_in_order_produces_the_expected_text() {
        let mut buffer = buffer("mov rax, rax\nadd rax, 1");
        for (range, text) in replace_all(&buffer, "rax", "r10", SearchOptions::new()) {
            buffer.replace_range(range, &text);
        }
        assert_eq!(buffer.to_text(), "mov r10, r10\nadd r10, 1");
    }

    #[test]
    fn replacing_with_a_longer_string_stays_correct() {
        let mut buffer = buffer("a a a");
        for (range, text) in replace_all(&buffer, "a", "bbbb", SearchOptions::new()) {
            buffer.replace_range(range, &text);
        }
        assert_eq!(buffer.to_text(), "bbbb bbbb bbbb");
    }

    #[test]
    fn replacing_with_an_empty_string_deletes_matches() {
        let mut buffer = buffer("mov rax, rax");
        for (range, text) in replace_all(&buffer, ", rax", "", SearchOptions::new()) {
            buffer.replace_range(range, &text);
        }
        assert_eq!(buffer.to_text(), "mov rax");
    }
}

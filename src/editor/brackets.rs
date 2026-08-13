//! Bracket matching across a buffer.
//!
//! Matching runs over the token stream rather than raw characters, so a
//! bracket inside a comment or a string literal is invisible to it. That is
//! the difference between a matcher that works on real source and one that
//! desynchronises the first time someone writes `db "]"`.

use super::buffer::TextBuffer;
use super::position::Position;
use super::syntax::{self, TokenKind};

/// The bracket pairs the editor matches.
const PAIRS: [(char, char); 3] = [('[', ']'), ('(', ')'), ('{', '}')];

/// The closing bracket for an opening one.
fn closing_for(ch: char) -> Option<char> {
    PAIRS
        .iter()
        .find(|(open, _)| *open == ch)
        .map(|(_, close)| *close)
}

/// The opening bracket for a closing one.
fn opening_for(ch: char) -> Option<char> {
    PAIRS
        .iter()
        .find(|(_, close)| *close == ch)
        .map(|(open, _)| *open)
}

/// Whether `ch` is a bracket of any kind.
pub fn is_bracket(ch: char) -> bool {
    closing_for(ch).is_some() || opening_for(ch).is_some()
}

/// One bracket found in the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bracket {
    position: Position,
    ch: char,
}

/// Collects every bracket in the buffer, in document order, skipping any that
/// appear inside comments or string literals.
fn collect(buffer: &TextBuffer) -> Vec<Bracket> {
    let mut brackets = Vec::new();
    for (line_index, line) in buffer.lines().iter().enumerate() {
        for token in syntax::tokenize(line) {
            if token.kind != TokenKind::Punctuation {
                continue;
            }
            let text = token.text(line);
            let Some(ch) = text.chars().next() else {
                continue;
            };
            if !is_bracket(ch) {
                continue;
            }
            // Punctuation tokens are one character, so the byte offset maps to
            // a character column by counting the characters before it.
            let column = line[..token.start].chars().count();
            brackets.push(Bracket {
                position: Position::new(line_index, column),
                ch,
            });
        }
    }
    brackets
}

/// Finds the bracket matching the one at or just before `position`.
///
/// Checking the character before the cursor as well as the one under it is
/// what makes matching feel right when the cursor sits just past a closing
/// bracket, which is where it lands after typing one.
pub fn matching_bracket(buffer: &TextBuffer, position: Position) -> Option<Position> {
    let brackets = collect(buffer);
    let before = position
        .column
        .checked_sub(1)
        .map(|column| Position::new(position.line, column));

    let index = brackets
        .iter()
        .position(|bracket| bracket.position == position)
        .or_else(|| {
            before.and_then(|target| {
                brackets
                    .iter()
                    .position(|bracket| bracket.position == target)
            })
        })?;

    let bracket = brackets[index];
    if let Some(close) = closing_for(bracket.ch) {
        scan_forward(&brackets, index, bracket.ch, close)
    } else {
        let open = opening_for(bracket.ch)?;
        scan_backward(&brackets, index, open, bracket.ch)
    }
}

/// Scans forward for the closing bracket that balances `open`.
fn scan_forward(brackets: &[Bracket], from: usize, open: char, close: char) -> Option<Position> {
    let mut depth = 0usize;
    for bracket in &brackets[from..] {
        if bracket.ch == open {
            depth += 1;
        } else if bracket.ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(bracket.position);
            }
        }
    }
    None
}

/// Scans backward for the opening bracket that balances `close`.
fn scan_backward(brackets: &[Bracket], from: usize, open: char, close: char) -> Option<Position> {
    let mut depth = 0usize;
    for bracket in brackets[..=from].iter().rev() {
        if bracket.ch == close {
            depth += 1;
        } else if bracket.ch == open {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(bracket.position);
            }
        }
    }
    None
}

/// Reports every unbalanced bracket in the buffer.
///
/// Used to warn about a missing `]` before the assembler does, which turns a
/// confusing NASM parse error into an obvious highlight.
pub fn unbalanced(buffer: &TextBuffer) -> Vec<Position> {
    let brackets = collect(buffer);
    let mut stack: Vec<Bracket> = Vec::new();
    let mut unmatched = Vec::new();

    for bracket in brackets {
        if closing_for(bracket.ch).is_some() {
            stack.push(bracket);
        } else if let Some(open) = opening_for(bracket.ch) {
            match stack.last() {
                Some(top) if top.ch == open => {
                    stack.pop();
                }
                _ => unmatched.push(bracket.position),
            }
        }
    }

    unmatched.extend(stack.into_iter().map(|bracket| bracket.position));
    unmatched.sort();
    unmatched
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(text: &str) -> TextBuffer {
        TextBuffer::from_text(text)
    }

    #[test]
    fn matches_a_simple_pair_in_both_directions() {
        let buffer = buffer("    mov rax, [rsp]");
        let open = Position::new(0, 13);
        let close = Position::new(0, 17);
        assert_eq!(matching_bracket(&buffer, open), Some(close));
        assert_eq!(matching_bracket(&buffer, close), Some(open));
    }

    #[test]
    fn matches_when_the_cursor_sits_just_past_a_bracket() {
        // Where the cursor lands after typing the closing bracket.
        let buffer = buffer("    mov rax, [rsp]");
        assert_eq!(
            matching_bracket(&buffer, Position::new(0, 18)),
            Some(Position::new(0, 13))
        );
    }

    #[test]
    fn matches_nested_brackets_at_the_right_depth() {
        let buffer = buffer("    mov rax, [rbx + (rcx * 2)]");
        assert_eq!(
            matching_bracket(&buffer, Position::new(0, 13)),
            Some(Position::new(0, 29)),
            "outer bracket must skip the inner pair"
        );
        assert_eq!(
            matching_bracket(&buffer, Position::new(0, 20)),
            Some(Position::new(0, 28))
        );
    }

    #[test]
    fn brackets_inside_strings_are_ignored() {
        // The case a character-scanning matcher gets wrong.
        let buffer = buffer("    db \"[not a bracket\", 0\n    mov rax, [rsp]");
        assert_eq!(
            matching_bracket(&buffer, Position::new(1, 13)),
            Some(Position::new(1, 17))
        );
        assert!(unbalanced(&buffer).is_empty());
    }

    #[test]
    fn brackets_inside_comments_are_ignored() {
        let buffer = buffer("    ret ; [unclosed\n");
        assert!(unbalanced(&buffer).is_empty());
    }

    #[test]
    fn matching_spans_lines() {
        let buffer = buffer("    mov rax, [rbx +\n                 rcx]");
        assert_eq!(
            matching_bracket(&buffer, Position::new(0, 13)),
            Some(Position::new(1, 20))
        );
    }

    #[test]
    fn a_position_with_no_bracket_matches_nothing() {
        let buffer = buffer("    mov rax, 1");
        assert_eq!(matching_bracket(&buffer, Position::new(0, 5)), None);
        assert_eq!(matching_bracket(&buffer, Position::ORIGIN), None);
    }

    #[test]
    fn an_unclosed_bracket_has_no_match() {
        let buffer = buffer("    mov rax, [rsp");
        assert_eq!(matching_bracket(&buffer, Position::new(0, 13)), None);
    }

    #[test]
    fn unbalanced_reports_an_unclosed_opening_bracket() {
        let buffer = buffer("    mov rax, [rsp\n    ret");
        assert_eq!(unbalanced(&buffer), vec![Position::new(0, 13)]);
    }

    #[test]
    fn unbalanced_reports_a_stray_closing_bracket() {
        let buffer = buffer("    mov rax, rsp]");
        assert_eq!(unbalanced(&buffer), vec![Position::new(0, 16)]);
    }

    #[test]
    fn unbalanced_reports_a_mismatched_pair() {
        let buffer = buffer("    mov rax, [rsp)");
        let found = unbalanced(&buffer);
        assert_eq!(found.len(), 2, "both the opener and the closer are wrong");
    }

    #[test]
    fn balanced_source_reports_nothing() {
        let buffer = buffer(
            "section .text\n_start:\n    mov rax, [rbx + (rcx * 8)]\n    lea rsi, [rel msg]\n",
        );
        assert!(unbalanced(&buffer).is_empty());
    }

    #[test]
    fn columns_are_characters_not_bytes() {
        let buffer = buffer("; ölçüm\n    mov rax, [rsp]");
        assert_eq!(
            matching_bracket(&buffer, Position::new(1, 13)),
            Some(Position::new(1, 17))
        );
    }

    #[test]
    fn an_empty_buffer_is_balanced() {
        assert!(unbalanced(&TextBuffer::new()).is_empty());
        assert_eq!(matching_bracket(&TextBuffer::new(), Position::ORIGIN), None);
    }
}

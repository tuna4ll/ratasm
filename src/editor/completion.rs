//! Context-aware completion for NASM source.
//!
//! Completion is filtered by *position*, not just by prefix. At the start of a
//! statement only instructions and directives can appear; in an operand only
//! registers, size specifiers and symbols can. Offering the whole vocabulary
//! everywhere would make the list useless exactly where it is longest, so the
//! context is narrowed first and the prefix applied second.
//!
//! Candidates are ranked so that an exact prefix match sorts above a
//! subsequence match, and shorter names sort above longer ones. That keeps
//! `rax` at the top when the user types `ra`, rather than burying it under
//! every symbol that happens to contain those letters.

use super::buffer::TextBuffer;
use super::position::Position;
use super::symbols::{self, SymbolKind};
use super::syntax::{self, TokenKind};
use crate::instruction::{mnemonics, registers};

/// What a completion candidate names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateKind {
    /// An instruction mnemonic.
    Instruction,
    /// An assembler directive.
    Directive,
    /// A register name.
    Register,
    /// A size or address specifier.
    SizeKeyword,
    /// A label, constant or macro defined in the file.
    Symbol,
}

impl CandidateKind {
    /// A short tag shown next to the candidate.
    pub const fn tag(self) -> &'static str {
        match self {
            CandidateKind::Instruction => "instr",
            CandidateKind::Directive => "dir",
            CandidateKind::Register => "reg",
            CandidateKind::SizeKeyword => "size",
            CandidateKind::Symbol => "sym",
        }
    }
}

/// One completion suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The text that would be inserted.
    pub text: String,
    /// What the candidate names.
    pub kind: CandidateKind,
    /// A one-line description shown beside the candidate.
    pub detail: String,
}

/// Where in a statement the cursor sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    /// At the start of a statement: an instruction or directive goes here.
    Mnemonic,
    /// Inside the operands of an instruction.
    Operand,
    /// After a branch instruction, where a label is the likely completion.
    BranchTarget,
    /// Somewhere no completion makes sense, such as inside a comment.
    None,
}

/// Determines what kind of token may appear at `position`.
pub fn context_at(buffer: &TextBuffer, position: Position) -> Context {
    let position = buffer.clamp(position);
    let line = buffer.line_or_empty(position.line);
    let byte_offset = byte_offset_of(line, position.column);
    let tokens = syntax::tokenize(line);

    // Inside a comment or a string, nothing should be suggested.
    if tokens.iter().any(|token| {
        matches!(token.kind, TokenKind::Comment | TokenKind::String)
            && byte_offset > token.start
            && byte_offset <= token.end
    }) {
        return Context::None;
    }

    // The token being typed is the one ending at the cursor; look at what
    // comes before it to decide the position in the statement.
    let preceding: Vec<_> = tokens
        .iter()
        .filter(|token| !token.kind.is_trivia() && token.end < byte_offset)
        .collect();

    match preceding.iter().find(|token| {
        matches!(
            token.kind,
            TokenKind::Instruction | TokenKind::Directive | TokenKind::Preprocessor
        )
    }) {
        Some(token) if token.kind == TokenKind::Instruction => {
            let mnemonic = token.text(line);
            if mnemonics::is_conditional_jump(mnemonic)
                || matches!(
                    mnemonic.to_ascii_lowercase().as_str(),
                    "jmp" | "call" | "loop" | "loope" | "loopne" | "jecxz" | "jrcxz"
                )
            {
                Context::BranchTarget
            } else {
                Context::Operand
            }
        }
        Some(_) => Context::Operand,
        None => Context::Mnemonic,
    }
}

/// Converts a character column to a byte offset within `line`.
fn byte_offset_of(line: &str, column: usize) -> usize {
    line.char_indices()
        .nth(column)
        .map_or(line.len(), |(offset, _)| offset)
}

/// The partial word immediately before `position`.
pub fn prefix_at(buffer: &TextBuffer, position: Position) -> String {
    let position = buffer.clamp(position);
    buffer
        .line_or_empty(position.line)
        .chars()
        .take(position.column)
        .collect::<String>()
        .chars()
        .rev()
        .take_while(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '.' | '$' | '@' | '?' | '%'))
        .collect::<Vec<char>>()
        .into_iter()
        .rev()
        .collect()
}

/// Produces ranked completion candidates for the cursor position.
pub fn candidates(buffer: &TextBuffer, position: Position) -> Vec<Candidate> {
    let position = buffer.clamp(position);
    let context = context_at(buffer, position);
    if context == Context::None {
        return Vec::new();
    }

    let prefix = prefix_at(buffer, position);
    let mut pool = Vec::new();

    match context {
        Context::Mnemonic => {
            pool.extend(mnemonics::all().into_iter().map(|text| Candidate {
                text,
                kind: CandidateKind::Instruction,
                detail: "instruction".to_owned(),
            }));
            pool.extend(directive_candidates());
        }
        Context::Operand | Context::BranchTarget => {
            pool.extend(symbol_candidates(buffer));
            if context == Context::Operand {
                pool.extend(register_candidates());
                pool.extend(size_keyword_candidates());
            }
        }
        Context::None => {}
    }

    rank(pool, &prefix)
}

fn directive_candidates() -> Vec<Candidate> {
    [
        "section", "global", "extern", "db", "dw", "dd", "dq", "resb", "resw", "resd", "resq",
        "equ", "times", "align", "bits", "default", "incbin", "struc", "endstruc",
    ]
    .iter()
    .map(|text| Candidate {
        text: (*text).to_owned(),
        kind: CandidateKind::Directive,
        detail: "directive".to_owned(),
    })
    .collect()
}

fn register_candidates() -> Vec<Candidate> {
    registers::all()
        .into_iter()
        .flat_map(|register| {
            register
                .aliases()
                .into_iter()
                .map(move |alias| Candidate {
                    text: alias.to_owned(),
                    kind: CandidateKind::Register,
                    detail: register.abi_role.description(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn size_keyword_candidates() -> Vec<Candidate> {
    [
        "byte", "word", "dword", "qword", "tword", "oword", "yword", "rel", "abs", "strict",
    ]
    .iter()
    .map(|text| Candidate {
        text: (*text).to_owned(),
        kind: CandidateKind::SizeKeyword,
        detail: "size specifier".to_owned(),
    })
    .collect()
}

fn symbol_candidates(buffer: &TextBuffer) -> Vec<Candidate> {
    symbols::extract(buffer)
        .into_iter()
        .filter(|symbol| symbol.kind != SymbolKind::Section)
        .map(|symbol| Candidate {
            text: symbol.name,
            kind: CandidateKind::Symbol,
            detail: symbol.kind.description().to_owned(),
        })
        .collect()
}

/// Filters and orders candidates against a prefix.
///
/// An empty prefix returns everything in the pool, ordered by kind then name,
/// which is what a user asking for "show me everything valid here" wants.
fn rank(pool: Vec<Candidate>, prefix: &str) -> Vec<Candidate> {
    let needle = prefix.to_ascii_lowercase();
    let mut scored: Vec<(u8, usize, Candidate)> = pool
        .into_iter()
        .filter_map(|candidate| {
            let haystack = candidate.text.to_ascii_lowercase();
            let score = if needle.is_empty() {
                2
            } else if haystack == needle {
                0
            } else if haystack.starts_with(&needle) {
                1
            } else if is_subsequence(&needle, &haystack) {
                3
            } else {
                return None;
            };
            Some((score, candidate.text.len(), candidate))
        })
        .collect();

    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.2.kind.cmp(&b.2.kind))
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.text.cmp(&b.2.text))
    });

    let mut seen = Vec::new();
    scored
        .into_iter()
        .map(|(_, _, candidate)| candidate)
        .filter(|candidate| {
            let key = (candidate.text.clone(), candidate.kind);
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        })
        .collect()
}

/// Whether `needle` appears in `haystack` as an ordered subsequence.
fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut chars = haystack.chars();
    needle
        .chars()
        .all(|wanted| chars.any(|actual| actual == wanted))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(text: &str) -> TextBuffer {
        TextBuffer::from_text(text)
    }

    fn texts(candidates: &[Candidate]) -> Vec<&str> {
        candidates.iter().map(|c| c.text.as_str()).collect()
    }

    #[test]
    fn prefix_is_the_partial_word_before_the_cursor() {
        let buffer = buffer("    mov ra");
        assert_eq!(prefix_at(&buffer, Position::new(0, 10)), "ra");
        assert_eq!(prefix_at(&buffer, Position::new(0, 8)), "");
        assert_eq!(prefix_at(&buffer, Position::new(0, 7)), "mov");
    }

    #[test]
    fn a_local_label_prefix_keeps_its_dot() {
        let buffer = buffer("    jmp .lo");
        assert_eq!(prefix_at(&buffer, Position::new(0, 11)), ".lo");
    }

    #[test]
    fn the_start_of_a_statement_is_mnemonic_context() {
        let buffer = buffer("    mo");
        assert_eq!(context_at(&buffer, Position::new(0, 6)), Context::Mnemonic);
    }

    #[test]
    fn after_an_instruction_is_operand_context() {
        let buffer = buffer("    mov ra");
        assert_eq!(context_at(&buffer, Position::new(0, 10)), Context::Operand);
    }

    #[test]
    fn after_a_branch_is_branch_target_context() {
        for line in ["    jmp ", "    je ", "    call ", "    jne .l"] {
            let buffer = buffer(line);
            let column = buffer.line_len(0);
            assert_eq!(
                context_at(&buffer, Position::new(0, column)),
                Context::BranchTarget,
                "failed for {line:?}"
            );
        }
    }

    #[test]
    fn inside_a_comment_nothing_is_suggested() {
        let buffer = buffer("    ret ; mo");
        assert_eq!(context_at(&buffer, Position::new(0, 12)), Context::None);
        assert!(candidates(&buffer, Position::new(0, 12)).is_empty());
    }

    #[test]
    fn inside_a_string_nothing_is_suggested() {
        let buffer = buffer("    db \"mo");
        assert_eq!(context_at(&buffer, Position::new(0, 10)), Context::None);
    }

    #[test]
    fn mnemonic_context_offers_instructions_not_registers() {
        let buffer = buffer("    mo");
        let found = candidates(&buffer, Position::new(0, 6));
        assert!(found.iter().any(|c| c.text == "mov"));
        assert!(
            !found.iter().any(|c| c.kind == CandidateKind::Register),
            "registers cannot appear in mnemonic position"
        );
    }

    #[test]
    fn operand_context_offers_registers_not_instructions() {
        let buffer = buffer("    mov ra");
        let found = candidates(&buffer, Position::new(0, 10));
        assert!(found.iter().any(|c| c.text == "rax"));
        assert!(
            !found.iter().any(|c| c.kind == CandidateKind::Instruction),
            "instructions cannot appear in operand position"
        );
    }

    #[test]
    fn a_prefix_match_ranks_first() {
        let buffer = buffer("    mov ra");
        let found = candidates(&buffer, Position::new(0, 10));
        assert_eq!(texts(&found).first(), Some(&"rax"));
    }

    #[test]
    fn a_subsequence_match_is_offered_when_no_prefix_matches() {
        // Nothing starts with "rx", but "rax" contains r, a, x in order.
        let buffer = buffer("    mov rx");
        let found = candidates(&buffer, Position::new(0, 10));
        assert!(
            found.iter().any(|c| c.text == "rax"),
            "expected a subsequence match, got {:?}",
            texts(&found)
        );
    }

    #[test]
    fn a_prefix_match_outranks_a_subsequence_match() {
        // "rax" matches "ra" as a prefix; "rbx_a" only as a subsequence
        // (r, then an a later on), so it must sort below.
        let register = |text: &str| Candidate {
            text: text.to_owned(),
            kind: CandidateKind::Register,
            detail: String::new(),
        };
        let ranked = rank(vec![register("rbx_a"), register("rax")], "ra");
        assert_eq!(texts(&ranked), vec!["rax", "rbx_a"]);
    }

    #[test]
    fn shorter_names_rank_above_longer_ones() {
        let buffer = buffer("    mov r");
        let found = candidates(&buffer, Position::new(0, 9));
        let names = texts(&found);
        let r8 = names.iter().position(|n| *n == "r8").expect("r8 offered");
        let r15b = names
            .iter()
            .position(|n| *n == "r15b")
            .expect("r15b offered");
        assert!(r8 < r15b, "shorter register names should sort first");
    }

    #[test]
    fn branch_targets_offer_labels_from_the_file() {
        let source = "_start:\n.loop:\n    dec rax\n    jnz .l";
        let buffer = buffer(source);
        let found = candidates(&buffer, Position::new(3, 10));
        assert!(found.iter().any(|c| c.text == ".loop"));
        assert!(
            !found.iter().any(|c| c.kind == CandidateKind::Register),
            "a branch target is never a register"
        );
    }

    #[test]
    fn symbols_defined_in_the_file_are_offered_in_operands() {
        let source = "message: db \"hi\", 0\n_start:\n    mov rsi, mes";
        let buffer = buffer(source);
        let found = candidates(&buffer, Position::new(2, 16));
        assert!(found.iter().any(|c| c.text == "message"));
    }

    #[test]
    fn size_keywords_are_offered_in_operands() {
        let buffer = buffer("    mov qw");
        let found = candidates(&buffer, Position::new(0, 10));
        assert!(found.iter().any(|c| c.text == "qword"));
    }

    #[test]
    fn an_empty_prefix_offers_the_whole_valid_vocabulary() {
        let buffer = buffer("    mov ");
        let found = candidates(&buffer, Position::new(0, 8));
        assert!(!found.is_empty());
        assert!(found.iter().any(|c| c.kind == CandidateKind::Register));
    }

    #[test]
    fn a_prefix_matching_nothing_yields_no_candidates() {
        let buffer = buffer("    mov zzzqqq");
        assert!(candidates(&buffer, Position::new(0, 14)).is_empty());
    }

    #[test]
    fn candidates_are_deduplicated() {
        let buffer = buffer("_start:\n    jmp _st");
        let found = candidates(&buffer, Position::new(1, 11));
        let names = texts(&found);
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "duplicates in {names:?}");
    }

    #[test]
    fn subsequence_matching_works() {
        assert!(is_subsequence("mv", "mov"));
        assert!(is_subsequence("", "anything"));
        assert!(!is_subsequence("vm", "mov"));
        assert!(!is_subsequence("movx", "mov"));
    }

    #[test]
    fn an_out_of_range_position_is_clamped_rather_than_rejected() {
        let buffer = buffer("    re");
        let clamped = candidates(&buffer, Position::new(99, 99));
        let direct = candidates(&buffer, Position::new(0, 6));
        assert_eq!(clamped, direct);
        assert!(clamped.iter().any(|c| c.text == "ret"));
    }
}

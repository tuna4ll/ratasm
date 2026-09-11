//! A NASM lexer used for highlighting, symbol extraction and completion.

use crate::instruction::{mnemonics, registers};

/// The lexical class of a token, used to pick a colour and to answer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// Runs of spaces and tabs.
    Whitespace,
    /// A `;` comment running to the end of the line.
    Comment,
    /// A quoted string or character literal.
    String,
    /// A numeric literal in any of NASM's bases.
    Number,
    /// A label being defined, such as `_start` in `_start:`.
    LabelDefinition,
    /// An instruction mnemonic.
    Instruction,
    /// An assembler directive such as `section` or `db`.
    Directive,
    /// A preprocessor directive such as `%define`.
    Preprocessor,
    /// A register name.
    Register,
    /// A size or address specifier such as `qword` or `rel`.
    SizeKeyword,
    /// Any other identifier: a label reference, a symbol, a macro name.
    Identifier,
    /// Operators, separators and brackets.
    Punctuation,
}

impl TokenKind {
    /// Whether tokens of this kind are ignorable when looking for structure.
    pub fn is_trivia(self) -> bool {
        matches!(self, TokenKind::Whitespace | TokenKind::Comment)
    }
}

/// A lexed token: a class plus the byte range it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// What kind of token this is.
    pub kind: TokenKind,
    /// Byte offset of the first character.
    pub start: usize,
    /// Byte offset one past the last character.
    pub end: usize,
}

impl Token {
    /// The token's text, taken from the line it was lexed from.
    pub fn text<'a>(&self, line: &'a str) -> &'a str {
        &line[self.start..self.end]
    }

    /// The token's length in bytes.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Whether the token covers no text.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Assembler directives NASM understands at the start of a statement.
#[rustfmt::skip]
const DIRECTIVES: &[&str] = &[
    "absolute", "align", "alignb", "at", "bits", "common", "cpu", "db", "dd", "default", "do",
    "dq", "dt", "dw", "dy", "dz", "endstruc", "equ", "extern", "float", "global", "group",
    "iend", "incbin", "istruc", "resb", "resd", "reso", "resq", "rest", "resw", "resy", "resz",
    "section", "segment", "static", "struc", "times", "use16", "use32", "use64",
];

/// Size, distance and relocation specifiers that may appear in operands.
#[rustfmt::skip]
const SIZE_KEYWORDS: &[&str] = &[
    "abs", "byte", "dword", "far", "long", "near", "nosplit", "oword", "ptr", "qword", "rel",
    "seg", "short", "strict", "tword", "word", "wrt", "yword", "zword",
];

/// Returns `true` when `word` is an assembler directive.
pub fn is_directive(word: &str) -> bool {
    let word = word.to_ascii_lowercase();
    DIRECTIVES.binary_search(&word.as_str()).is_ok()
}

/// Returns `true` when `word` is a size or address specifier.
pub fn is_size_keyword(word: &str) -> bool {
    let word = word.to_ascii_lowercase();
    SIZE_KEYWORDS.binary_search(&word.as_str()).is_ok()
}

/// Whether `ch` can start a NASM identifier.
fn is_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || matches!(ch, '_' | '.' | '?' | '@')
}

/// Whether `ch` can continue a NASM identifier.
fn is_identifier_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '?' | '@' | '$' | '#' | '~')
}

/// Splits one line of NASM source into tokens.
pub fn tokenize(line: &str) -> Vec<Token> {
    let mut tokens = scan(line);
    classify(line, &mut tokens);
    tokens
}

/// The lexical pass: splits text into tokens without deciding what identifiers
fn scan(line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let mut index = 0usize;

    while index < bytes.len() {
        let start = index;
        let ch = match line[index..].chars().next() {
            Some(ch) => ch,
            None => break,
        };

        let kind = if ch == ';' {
            index = bytes.len();
            TokenKind::Comment
        } else if ch == ' ' || ch == '\t' {
            while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
                index += 1;
            }
            TokenKind::Whitespace
        } else if ch == '\'' || ch == '"' || ch == '`' {
            index += ch.len_utf8();
            index = scan_string(line, index, ch);
            TokenKind::String
        } else if ch == '%' {
            index += 1;
            let is_preprocessor = line[index..]
                .chars()
                .next()
                .is_some_and(|next| is_identifier_start(next) || matches!(next, '%' | '$' | '!'));
            if is_preprocessor {
                while index < bytes.len() {
                    let next = match line[index..].chars().next() {
                        Some(next) => next,
                        None => break,
                    };
                    if is_identifier_continue(next) || matches!(next, '%' | '$' | '!') {
                        index += next.len_utf8();
                    } else {
                        break;
                    }
                }
                TokenKind::Preprocessor
            } else {
                TokenKind::Punctuation
            }
        } else if ch.is_ascii_digit() {
            index = scan_number(line, index);
            TokenKind::Number
        } else if is_identifier_start(ch) {
            index += ch.len_utf8();
            while index < bytes.len() {
                let next = match line[index..].chars().next() {
                    Some(next) => next,
                    None => break,
                };
                if is_identifier_continue(next) {
                    index += next.len_utf8();
                } else {
                    break;
                }
            }
            TokenKind::Identifier
        } else {
            index += ch.len_utf8();
            TokenKind::Punctuation
        };

        tokens.push(Token {
            kind,
            start,
            end: index,
        });
    }

    tokens
}

/// Consumes a string literal, returning the offset just past its closing quote.
fn scan_string(line: &str, mut index: usize, quote: char) -> usize {
    let bytes = line.as_bytes();
    while index < bytes.len() {
        let ch = match line[index..].chars().next() {
            Some(ch) => ch,
            None => break,
        };
        if quote == '`' && ch == '\\' && index + 1 < bytes.len() {
            index += 1;
            if let Some(escaped) = line[index..].chars().next() {
                index += escaped.len_utf8();
            }
            continue;
        }
        index += ch.len_utf8();
        if ch == quote {
            break;
        }
    }
    index
}

/// Consumes a numeric literal in any base NASM accepts.
fn scan_number(line: &str, mut index: usize) -> usize {
    let bytes = line.as_bytes();
    while index < bytes.len() {
        let ch = bytes[index] as char;
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            let is_exponent = matches!(ch, 'e' | 'E')
                && matches!(bytes.get(index + 1), Some(b'+' | b'-'))
                && bytes.get(index + 2).is_some_and(u8::is_ascii_digit);
            index += 1;
            if is_exponent {
                index += 1;
            }
        } else {
            break;
        }
    }
    index
}

/// The semantic pass: decides what each identifier means from its position.
fn classify(line: &str, tokens: &mut [Token]) {
    let mut seen_mnemonic = false;
    let mut is_first_word = true;

    for position in 0..tokens.len() {
        if tokens[position].kind != TokenKind::Identifier {
            if !tokens[position].kind.is_trivia() {
                is_first_word = false;
            }
            continue;
        }

        let token = tokens[position];
        let word = token.text(line);
        let followed_by_colon = next_significant(tokens, position)
            .map(|next| tokens[next])
            .is_some_and(|next| next.kind == TokenKind::Punctuation && next.text(line) == ":");

        let is_label = if followed_by_colon {
            true
        } else {
            is_first_word
                && !seen_mnemonic
                && token.start == 0
                && !mnemonics::is_mnemonic(word)
                && !is_directive(word)
        };

        tokens[position].kind = if is_label {
            TokenKind::LabelDefinition
        } else if !seen_mnemonic && is_directive(word) {
            seen_mnemonic = true;
            TokenKind::Directive
        } else if !seen_mnemonic && mnemonics::is_mnemonic(word) {
            seen_mnemonic = true;
            TokenKind::Instruction
        } else if registers::is_register(word) {
            TokenKind::Register
        } else if is_size_keyword(word) {
            TokenKind::SizeKeyword
        } else if is_directive(word) {
            TokenKind::Directive
        } else {
            TokenKind::Identifier
        };

        is_first_word = false;
    }
}

/// The index of the next token that is not whitespace or a comment.
fn next_significant(tokens: &[Token], from: usize) -> Option<usize> {
    tokens
        .iter()
        .enumerate()
        .skip(from + 1)
        .find(|(_, token)| !token.kind.is_trivia())
        .map(|(index, _)| index)
}

/// Extracts the label defined on `line`, if it defines one.
pub fn label_definition(line: &str) -> Option<String> {
    let tokens = tokenize(line);
    tokens
        .iter()
        .find(|token| token.kind == TokenKind::LabelDefinition)
        .map(|token| token.text(line).to_owned())
}

/// The file named by a `%include` line, if it is one.
pub fn include_target(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('%')?.trim_start();
    let rest = rest
        .strip_prefix("include")
        .or_else(|| rest.strip_prefix("INCLUDE"))?
        .trim_start();

    let quote = rest.chars().next()?;
    if !matches!(quote, '"' | '\'' | '`') {
        return None;
    }
    let rest = &rest[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end]).filter(|path| !path.is_empty())
}

/// Returns the identifier surrounding `byte_offset`, if there is one.
pub fn word_at(line: &str, byte_offset: usize) -> Option<(Token, &str)> {
    tokenize(line)
        .into_iter()
        .find(|token| {
            matches!(
                token.kind,
                TokenKind::Identifier
                    | TokenKind::Instruction
                    | TokenKind::Register
                    | TokenKind::Directive
                    | TokenKind::LabelDefinition
                    | TokenKind::SizeKeyword
            ) && byte_offset >= token.start
                && byte_offset <= token.end
        })
        .map(|token| (token, token.text(line)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(TokenKind, &str)> {
        tokenize(line)
            .into_iter()
            .filter(|token| token.kind != TokenKind::Whitespace)
            .map(|token| (token.kind, token.text(line)))
            .collect()
    }

    #[test]
    fn tokens_tile_the_line_exactly() {
        for line in [
            "",
            "    ",
            "_start:",
            "    mov rax, 60          ; exit",
            "%define SIZE 64",
            "msg: db `hi\\n`, 0",
            "    lea rdi, [rel msg]",
            "times 16 db 0",
            "; comment only",
        ] {
            let tokens = tokenize(line);
            let mut offset = 0;
            let mut rebuilt = String::new();
            for token in &tokens {
                assert_eq!(token.start, offset, "gap or overlap in {line:?}");
                assert!(token.end > token.start, "empty token in {line:?}");
                rebuilt.push_str(token.text(line));
                offset = token.end;
            }
            assert_eq!(offset, line.len(), "line {line:?} not fully covered");
            assert_eq!(rebuilt, line, "round trip failed for {line:?}");
        }
    }

    #[test]
    fn empty_line_produces_no_tokens() {
        assert!(tokenize("").is_empty());
    }

    #[test]
    fn a_labelled_instruction_is_split_correctly() {
        assert_eq!(
            kinds("_start: mov rax, 1"),
            vec![
                (TokenKind::LabelDefinition, "_start"),
                (TokenKind::Punctuation, ":"),
                (TokenKind::Instruction, "mov"),
                (TokenKind::Register, "rax"),
                (TokenKind::Punctuation, ","),
                (TokenKind::Number, "1"),
            ]
        );
    }

    #[test]
    fn a_colonless_label_in_column_zero_is_a_label() {
        assert_eq!(
            kinds("main mov rax, 1"),
            vec![
                (TokenKind::LabelDefinition, "main"),
                (TokenKind::Instruction, "mov"),
                (TokenKind::Register, "rax"),
                (TokenKind::Punctuation, ","),
                (TokenKind::Number, "1"),
            ]
        );
    }

    #[test]
    fn an_indented_instruction_is_not_read_as_a_label() {
        assert_eq!(
            kinds("    mov rax, 1"),
            vec![
                (TokenKind::Instruction, "mov"),
                (TokenKind::Register, "rax"),
                (TokenKind::Punctuation, ","),
                (TokenKind::Number, "1"),
            ]
        );
    }

    #[test]
    fn a_mnemonic_in_column_zero_is_still_an_instruction() {
        assert_eq!(kinds("ret")[0], (TokenKind::Instruction, "ret"));
        assert_eq!(kinds("syscall")[0], (TokenKind::Instruction, "syscall"));
    }

    #[test]
    fn local_labels_starting_with_a_dot_are_recognised() {
        assert_eq!(
            kinds(".loop:"),
            vec![
                (TokenKind::LabelDefinition, ".loop"),
                (TokenKind::Punctuation, ":"),
            ]
        );
    }

    #[test]
    fn comments_run_to_the_end_of_the_line() {
        let tokens = kinds("    ret ; return to caller: not a label");
        assert_eq!(tokens[0], (TokenKind::Instruction, "ret"));
        assert_eq!(
            tokens[1],
            (TokenKind::Comment, "; return to caller: not a label")
        );
        assert_eq!(tokens.len(), 2);
    }

    #[test]
    fn a_semicolon_inside_a_string_does_not_start_a_comment() {
        let tokens = kinds(r#"msg: db "a;b", 0"#);
        assert!(tokens
            .iter()
            .any(|(kind, text)| *kind == TokenKind::String && *text == "\"a;b\""));
        assert!(!tokens.iter().any(|(kind, _)| *kind == TokenKind::Comment));
    }

    #[test]
    fn all_three_quote_styles_are_recognised() {
        for line in ["db 'a'", "db \"a\"", "db `a`"] {
            let tokens = kinds(line);
            assert_eq!(tokens[1].0, TokenKind::String, "failed for {line}");
        }
    }

    #[test]
    fn backquoted_strings_honour_escapes() {
        let line = r"db `a\`b`, 0";
        let tokens = kinds(line);
        assert_eq!(tokens[1], (TokenKind::String, r"`a\`b`"));
    }

    #[test]
    fn single_quoted_strings_do_not_honour_escapes() {
        let line = r"db 'a\', 0";
        let tokens = kinds(line);
        assert_eq!(tokens[1], (TokenKind::String, r"'a\'"));
    }

    #[test]
    fn an_unterminated_string_runs_to_end_of_line_without_panicking() {
        let line = "db \"unfinished";
        let tokens = kinds(line);
        assert_eq!(tokens[1], (TokenKind::String, "\"unfinished"));
    }

    #[test]
    fn numbers_in_every_nasm_base_are_recognised() {
        for literal in [
            "1",
            "42",
            "0x1f",
            "0X1F",
            "1fh",
            "0b1010",
            "1010b",
            "0o777",
            "777q",
            "123d",
            "1.5",
            "1e10",
            "1e+9",
            "0xdeadbeef",
        ] {
            let line = format!("    mov rax, {literal}");
            let tokens = kinds(&line);
            assert_eq!(
                tokens[3],
                (TokenKind::Number, literal),
                "failed for {literal}"
            );
        }
    }

    #[test]
    fn arithmetic_signs_are_not_absorbed_into_numbers() {
        assert_eq!(
            kinds("    dq 1+2"),
            vec![
                (TokenKind::Directive, "dq"),
                (TokenKind::Number, "1"),
                (TokenKind::Punctuation, "+"),
                (TokenKind::Number, "2"),
            ]
        );
    }

    #[test]
    fn registers_of_every_width_are_recognised() {
        let tokens = kinds("    mov al, ah");
        assert_eq!(tokens[1], (TokenKind::Register, "al"));
        assert_eq!(tokens[3], (TokenKind::Register, "ah"));
        assert_eq!(kinds("    push r15")[1], (TokenKind::Register, "r15"));
        assert_eq!(kinds("    mov eax, edi")[1], (TokenKind::Register, "eax"));
    }

    #[test]
    fn directives_and_their_operands_are_classified() {
        assert_eq!(
            kinds("section .text"),
            vec![
                (TokenKind::Directive, "section"),
                (TokenKind::Identifier, ".text"),
            ]
        );
        assert_eq!(
            kinds("global _start"),
            vec![
                (TokenKind::Directive, "global"),
                (TokenKind::Identifier, "_start"),
            ]
        );
    }

    #[test]
    fn a_directive_may_follow_another_directive() {
        assert_eq!(
            kinds("times 16 db 0"),
            vec![
                (TokenKind::Directive, "times"),
                (TokenKind::Number, "16"),
                (TokenKind::Directive, "db"),
                (TokenKind::Number, "0"),
            ]
        );
    }

    #[test]
    fn size_keywords_are_distinguished_from_symbols() {
        assert_eq!(
            kinds("    mov qword [rsp], rax"),
            vec![
                (TokenKind::Instruction, "mov"),
                (TokenKind::SizeKeyword, "qword"),
                (TokenKind::Punctuation, "["),
                (TokenKind::Register, "rsp"),
                (TokenKind::Punctuation, "]"),
                (TokenKind::Punctuation, ","),
                (TokenKind::Register, "rax"),
            ]
        );
    }

    #[test]
    fn rip_relative_addressing_is_classified() {
        assert_eq!(
            kinds("    lea rdi, [rel message]"),
            vec![
                (TokenKind::Instruction, "lea"),
                (TokenKind::Register, "rdi"),
                (TokenKind::Punctuation, ","),
                (TokenKind::Punctuation, "["),
                (TokenKind::SizeKeyword, "rel"),
                (TokenKind::Identifier, "message"),
                (TokenKind::Punctuation, "]"),
            ]
        );
    }

    #[test]
    fn preprocessor_directives_are_recognised() {
        assert_eq!(
            kinds("%define BUFFER_SIZE 64")[0],
            (TokenKind::Preprocessor, "%define")
        );
        assert_eq!(
            kinds("%macro two 1")[0],
            (TokenKind::Preprocessor, "%macro")
        );
        assert_eq!(kinds("%%local:")[0], (TokenKind::Preprocessor, "%%local"));
    }

    #[test]
    fn a_bare_percent_is_an_operator_not_a_directive() {
        let tokens = kinds("    dq 7 % 2");
        assert_eq!(tokens[2], (TokenKind::Punctuation, "%"));
    }

    #[test]
    fn a_label_reference_in_an_operand_is_not_a_definition() {
        let tokens = kinds("    jmp .loop");
        assert_eq!(tokens[0], (TokenKind::Instruction, "jmp"));
        assert_eq!(tokens[1], (TokenKind::Identifier, ".loop"));
    }

    #[test]
    fn conditional_jumps_are_recognised_as_instructions() {
        for mnemonic in ["je", "jne", "jle", "setz", "cmovg"] {
            let line = format!("    {mnemonic} target");
            assert_eq!(
                kinds(&line)[0],
                (TokenKind::Instruction, mnemonic),
                "failed for {mnemonic}"
            );
        }
    }

    #[test]
    fn label_definition_is_extracted_from_a_line() {
        assert_eq!(label_definition("_start:"), Some("_start".to_owned()));
        assert_eq!(label_definition("main mov rax, 1"), Some("main".to_owned()));
        assert_eq!(label_definition("    mov rax, 1"), None);
        assert_eq!(label_definition("; nothing here"), None);
        assert_eq!(label_definition(""), None);
    }

    #[test]
    fn an_include_line_names_its_file() {
        assert_eq!(
            include_target("%include \"macros.inc\""),
            Some("macros.inc")
        );
        assert_eq!(include_target("  %include 'a/b.inc' "), Some("a/b.inc"));
        assert_eq!(include_target("% include `x.inc`"), Some("x.inc"));
    }

    #[test]
    fn a_line_that_is_not_an_include_names_nothing() {
        assert_eq!(include_target("    mov rax, 1"), None);
        assert_eq!(include_target("%define X 1"), None);
        assert_eq!(include_target("%include macros.inc"), None);
        assert_eq!(include_target("%include \"\""), None);
    }

    #[test]
    fn word_at_finds_the_identifier_under_the_cursor() {
        let line = "    jmp .loop";
        assert_eq!(word_at(line, 4).map(|(_, text)| text), Some("jmp"));
        assert_eq!(word_at(line, 6).map(|(_, text)| text), Some("jmp"));
        assert_eq!(word_at(line, 10).map(|(_, text)| text), Some(".loop"));
        assert_eq!(word_at(line, 1).map(|(_, text)| text), None);
    }

    #[test]
    fn word_at_handles_an_offset_past_the_end_of_the_line() {
        assert_eq!(word_at("ret", 99), None);
        assert_eq!(word_at("", 0), None);
    }

    #[test]
    fn directive_and_size_keyword_tables_are_sorted() {
        let mut sorted = DIRECTIVES.to_vec();
        sorted.sort_unstable();
        assert_eq!(DIRECTIVES, &sorted[..]);
        let mut sorted = SIZE_KEYWORDS.to_vec();
        sorted.sort_unstable();
        assert_eq!(SIZE_KEYWORDS, &sorted[..]);
    }

    #[test]
    fn classification_is_case_insensitive() {
        let tokens = kinds("    MOV RAX, QWORD [RSP]");
        assert_eq!(tokens[0].0, TokenKind::Instruction);
        assert_eq!(tokens[1].0, TokenKind::Register);
        assert_eq!(tokens[3].0, TokenKind::SizeKeyword);
    }

    #[test]
    fn non_ascii_text_in_comments_does_not_break_offsets() {
        let line = "    ret ; dönüş değeri";
        let tokens = tokenize(line);
        let total: usize = tokens.iter().map(Token::len).sum();
        assert_eq!(total, line.len());
        assert_eq!(tokens.last().map(|t| t.kind), Some(TokenKind::Comment));
    }

    #[test]
    fn a_full_program_lexes_without_panicking() {
        let source = "\
section .data
    message: db `Hello, world!\\n`, 0
    length:  equ $ - message

section .text
    global _start

_start:
    mov rax, 1
    mov rdi, 1
    lea rsi, [rel message]
    mov rdx, length
    syscall

.exit:
    xor edi, edi
    mov eax, 60
    syscall
";
        for line in source.lines() {
            let tokens = tokenize(line);
            let covered: usize = tokens.iter().map(Token::len).sum();
            assert_eq!(covered, line.len(), "coverage failed for {line:?}");
        }
    }
}

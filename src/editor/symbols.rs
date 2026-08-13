//! Symbol extraction: the labels, sections and constants a file defines.
//!
//! Powers the symbol explorer, "go to definition" and label completion. The
//! extractor reads the token stream from [`super::syntax`] rather than
//! matching raw text, so a label mentioned inside a comment or a string is
//! correctly ignored.
//!
//! # Local labels
//!
//! NASM scopes a label beginning with `.` to the most recent non-local label:
//! `.loop` under `_start` is really `_start.loop`, and a different `.loop`
//! under `main` is a distinct symbol. Both spellings are recorded, so jumping
//! to a definition works whether the user typed the short or the qualified
//! form, and two local labels with the same short name do not collide.

use super::buffer::TextBuffer;
use super::position::Position;
use super::syntax::{self, TokenKind};

/// What kind of thing a symbol names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    /// A top-level label such as `_start:`.
    Label,
    /// A label scoped to the preceding top-level label, such as `.loop:`.
    LocalLabel,
    /// A `section` or `segment` declaration.
    Section,
    /// A symbol exported with `global`.
    Global,
    /// A symbol imported with `extern`.
    Extern,
    /// A constant defined with `equ`.
    Constant,
    /// A preprocessor macro or define.
    Macro,
}

impl SymbolKind {
    /// A short label for display in the symbol list.
    pub const fn description(self) -> &'static str {
        match self {
            SymbolKind::Label => "label",
            SymbolKind::LocalLabel => "local label",
            SymbolKind::Section => "section",
            SymbolKind::Global => "global",
            SymbolKind::Extern => "extern",
            SymbolKind::Constant => "constant",
            SymbolKind::Macro => "macro",
        }
    }

    /// Whether the symbol is a jump target the user can navigate to.
    pub const fn is_jump_target(self) -> bool {
        matches!(self, SymbolKind::Label | SymbolKind::LocalLabel)
    }
}

/// A symbol defined somewhere in a buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The name as written, for example `.loop`.
    pub name: String,
    /// The fully qualified name, for example `_start.loop`.
    ///
    /// Equal to `name` for everything except local labels.
    pub qualified_name: String,
    /// What the symbol is.
    pub kind: SymbolKind,
    /// Where the definition starts.
    pub position: Position,
    /// Extra context shown next to the name, such as the section it sits in.
    pub detail: String,
}

impl Symbol {
    /// Whether `query` names this symbol, in either spelling.
    pub fn matches_name(&self, query: &str) -> bool {
        self.name == query || self.qualified_name == query
    }
}

/// Extracts every symbol defined in `buffer`, in source order.
pub fn extract(buffer: &TextBuffer) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut current_label: Option<String> = None;
    let mut current_section: Option<String> = None;

    for (line_index, line) in buffer.lines().iter().enumerate() {
        let tokens = syntax::tokenize(line);
        let significant: Vec<_> = tokens
            .iter()
            .filter(|token| !token.kind.is_trivia())
            .collect();
        let Some(first) = significant.first() else {
            continue;
        };

        match first.kind {
            TokenKind::LabelDefinition => {
                let name = first.text(line).to_owned();
                let is_local = name.starts_with('.');
                let qualified_name = match (&current_label, is_local) {
                    (Some(parent), true) => format!("{parent}{name}"),
                    _ => name.clone(),
                };
                if !is_local {
                    current_label = Some(name.clone());
                }

                // `name: equ 4` and `name: db 0` define a constant and a data
                // label respectively; both are more useful to show as such
                // than as bare labels.
                let kind = match significant
                    .iter()
                    .find(|token| token.kind == TokenKind::Directive)
                {
                    Some(directive) if directive.text(line).eq_ignore_ascii_case("equ") => {
                        SymbolKind::Constant
                    }
                    _ if is_local => SymbolKind::LocalLabel,
                    _ => SymbolKind::Label,
                };

                symbols.push(Symbol {
                    name,
                    qualified_name,
                    kind,
                    position: Position::new(line_index, 0),
                    detail: current_section.clone().unwrap_or_default(),
                });
            }
            TokenKind::Directive => {
                let directive = first.text(line).to_ascii_lowercase();
                match directive.as_str() {
                    "section" | "segment" => {
                        if let Some(operand) = significant.get(1) {
                            let name = operand.text(line).to_owned();
                            current_section = Some(name.clone());
                            symbols.push(Symbol {
                                name: name.clone(),
                                qualified_name: name,
                                kind: SymbolKind::Section,
                                position: Position::new(line_index, 0),
                                detail: String::new(),
                            });
                        }
                    }
                    "global" | "extern" | "common" => {
                        let kind = if directive == "extern" {
                            SymbolKind::Extern
                        } else {
                            SymbolKind::Global
                        };
                        for operand in significant.iter().skip(1) {
                            if operand.kind != TokenKind::Identifier {
                                continue;
                            }
                            let name = operand.text(line).to_owned();
                            symbols.push(Symbol {
                                name: name.clone(),
                                qualified_name: name,
                                kind,
                                position: Position::new(line_index, 0),
                                detail: directive.clone(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            TokenKind::Preprocessor => {
                let directive = first.text(line).to_ascii_lowercase();
                if matches!(
                    directive.as_str(),
                    "%define" | "%xdefine" | "%assign" | "%macro" | "%idefine"
                ) {
                    if let Some(operand) = significant.get(1) {
                        let name = operand.text(line).to_owned();
                        symbols.push(Symbol {
                            name: name.clone(),
                            qualified_name: name,
                            kind: SymbolKind::Macro,
                            position: Position::new(line_index, 0),
                            detail: directive.trim_start_matches('%').to_owned(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    symbols
}

/// Finds the definition of `name`, resolving local labels against `from`.
///
/// A bare `.loop` is ambiguous on its own; resolving it requires knowing which
/// top-level label the reference sits under, which `from` supplies.
pub fn find_definition<'a>(
    symbols: &'a [Symbol],
    buffer: &TextBuffer,
    name: &str,
    from: Position,
) -> Option<&'a Symbol> {
    if name.starts_with('.') {
        if let Some(parent) = enclosing_label(buffer, from) {
            let qualified = format!("{parent}{name}");
            if let Some(found) = symbols
                .iter()
                .find(|symbol| symbol.qualified_name == qualified)
            {
                return Some(found);
            }
        }
    }
    symbols.iter().find(|symbol| symbol.matches_name(name))
}

/// The most recent non-local label at or before `position`.
pub fn enclosing_label(buffer: &TextBuffer, position: Position) -> Option<String> {
    let last = position.line.min(buffer.line_count().saturating_sub(1));
    (0..=last).rev().find_map(|index| {
        let line = buffer.line_or_empty(index);
        syntax::label_definition(line).filter(|label| !label.starts_with('.'))
    })
}

/// Every label name defined in the buffer, for completion.
pub fn label_names(buffer: &TextBuffer) -> Vec<String> {
    let mut names: Vec<String> = extract(buffer)
        .into_iter()
        .filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Label
                    | SymbolKind::LocalLabel
                    | SymbolKind::Constant
                    | SymbolKind::Extern
                    | SymbolKind::Macro
            )
        })
        .map(|symbol| symbol.name)
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRAM: &str = "\
section .data
message: db `Hello`, 0
length:  equ $ - message

section .text
global _start
extern printf

%define BUFFER 64

_start:
    mov rax, 1
.loop:
    dec rax
    jnz .loop
    ret

main:
.loop:
    ret
";

    fn symbols() -> Vec<Symbol> {
        extract(&TextBuffer::from_text(PROGRAM))
    }

    /// Finds the symbol with this name and kind.
    ///
    /// Both are needed because one name legitimately produces two symbols:
    /// `global _start` declares it and `_start:` defines it.
    fn find(name: &str, kind: SymbolKind) -> Symbol {
        symbols()
            .into_iter()
            .find(|symbol| symbol.name == name && symbol.kind == kind)
            .unwrap_or_else(|| panic!("symbol {name} ({kind:?}) not found"))
    }

    #[test]
    fn top_level_labels_are_extracted() {
        for name in ["_start", "main", "message"] {
            let symbol = find(name, SymbolKind::Label);
            assert_eq!(symbol.qualified_name, name);
        }
    }

    #[test]
    fn sections_are_extracted() {
        let sections: Vec<String> = symbols()
            .into_iter()
            .filter(|symbol| symbol.kind == SymbolKind::Section)
            .map(|symbol| symbol.name)
            .collect();
        assert_eq!(sections, vec![".data", ".text"]);
    }

    #[test]
    fn globals_and_externs_are_distinguished() {
        assert_eq!(find("_start", SymbolKind::Global).detail, "global");
        assert_eq!(find("printf", SymbolKind::Extern).detail, "extern");
    }

    #[test]
    fn a_declared_and_defined_symbol_produces_both_entries() {
        // `global _start` and `_start:` are different facts about one name.
        let entries: Vec<SymbolKind> = symbols()
            .into_iter()
            .filter(|symbol| symbol.name == "_start")
            .map(|symbol| symbol.kind)
            .collect();
        assert_eq!(entries, vec![SymbolKind::Global, SymbolKind::Label]);
    }

    #[test]
    fn a_constant_defined_with_equ_is_not_a_plain_label() {
        assert_eq!(
            find("length", SymbolKind::Constant).kind,
            SymbolKind::Constant
        );
    }

    #[test]
    fn preprocessor_defines_are_extracted() {
        assert_eq!(find("BUFFER", SymbolKind::Macro).kind, SymbolKind::Macro);
    }

    #[test]
    fn symbols_carry_the_section_they_sit_in() {
        assert_eq!(find("message", SymbolKind::Label).detail, ".data");
        assert_eq!(find("_start", SymbolKind::Label).detail, ".text");
    }

    #[test]
    fn local_labels_are_qualified_by_their_parent() {
        let locals: Vec<(String, String)> = symbols()
            .into_iter()
            .filter(|symbol| symbol.kind == SymbolKind::LocalLabel)
            .map(|symbol| (symbol.name, symbol.qualified_name))
            .collect();
        assert_eq!(
            locals,
            vec![
                (".loop".to_owned(), "_start.loop".to_owned()),
                (".loop".to_owned(), "main.loop".to_owned()),
            ]
        );
    }

    #[test]
    fn a_local_label_resolves_against_the_enclosing_label() {
        // The distinction that makes local labels work: the same `.loop`
        // reference means different things in different functions.
        let buffer = TextBuffer::from_text(PROGRAM);
        let symbols = extract(&buffer);

        let from_start = find_definition(&symbols, &buffer, ".loop", Position::new(14, 8))
            .expect("resolves under _start");
        assert_eq!(from_start.qualified_name, "_start.loop");

        let from_main = find_definition(&symbols, &buffer, ".loop", Position::new(19, 4))
            .expect("resolves under main");
        assert_eq!(from_main.qualified_name, "main.loop");
    }

    #[test]
    fn a_qualified_name_resolves_directly() {
        let buffer = TextBuffer::from_text(PROGRAM);
        let symbols = extract(&buffer);
        let found = find_definition(&symbols, &buffer, "main.loop", Position::ORIGIN)
            .expect("qualified lookup");
        assert_eq!(found.qualified_name, "main.loop");
    }

    #[test]
    fn an_unknown_name_resolves_to_nothing() {
        let buffer = TextBuffer::from_text(PROGRAM);
        let symbols = extract(&buffer);
        assert!(find_definition(&symbols, &buffer, "nowhere", Position::ORIGIN).is_none());
    }

    #[test]
    fn enclosing_label_walks_backwards() {
        let buffer = TextBuffer::from_text(PROGRAM);
        assert_eq!(
            enclosing_label(&buffer, Position::new(14, 0)),
            Some("_start".to_owned())
        );
        assert_eq!(
            enclosing_label(&buffer, Position::new(19, 0)),
            Some("main".to_owned())
        );
    }

    #[test]
    fn enclosing_label_past_the_end_of_the_buffer_is_clamped() {
        let buffer = TextBuffer::from_text(PROGRAM);
        assert!(enclosing_label(&buffer, Position::new(9999, 0)).is_some());
    }

    #[test]
    fn labels_inside_comments_and_strings_are_ignored() {
        let buffer = TextBuffer::from_text(
            "; fake_label: not real\n    db \"other_label: also not real\", 0\nreal_label:\n",
        );
        let names: Vec<String> = extract(&buffer).into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["real_label"]);
    }

    #[test]
    fn an_empty_buffer_yields_no_symbols() {
        assert!(extract(&TextBuffer::new()).is_empty());
    }

    #[test]
    fn label_names_are_sorted_and_deduplicated() {
        let buffer = TextBuffer::from_text(PROGRAM);
        let names = label_names(&buffer);
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        assert_eq!(
            names.iter().filter(|name| *name == ".loop").count(),
            1,
            "duplicate short names collapse for completion"
        );
        assert!(names.contains(&"_start".to_owned()));
        assert!(names.contains(&"printf".to_owned()));
    }
}

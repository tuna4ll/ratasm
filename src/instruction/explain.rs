//! Turning a line of assembly into a plain-language explanation.
//!
//! Given `add rax, rbx`, this produces:
//!
//! ```text
//! Effect:  RAX ← RAX + RBX
//! Reads:   RAX, RBX
//! Writes:  RAX
//! Flags:   CF, PF, AF, ZF, SF, OF
//! ```
//!
//! Everything comes from [`super::database`]; nothing is inferred. An
//! instruction that is not in the database produces `None` rather than a
//! generic sentence, because a confident-sounding wrong explanation is worse
//! than no explanation at all.
//!
//! # Why the statement splitter lives here
//!
//! The editor has a full NASM lexer, but the editor depends on this module for
//! its mnemonic and register tables. Reaching back the other way would make
//! the dependency circular, so the small amount of parsing needed to find a
//! mnemonic and its operands is done here. It handles the cases that actually
//! change the answer — comments, string literals, bracketed memory operands
//! and instruction prefixes — and nothing more.

use super::database::{Database, InstructionInfo};
use super::flags::Flag;
use super::registers;

/// Instruction prefixes that precede the real mnemonic.
const PREFIXES: [&str; 7] = ["rep", "repe", "repz", "repne", "repnz", "lock", "bnd"];

/// A statement split into its parts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Statement {
    /// Any prefix, such as `rep`.
    pub prefix: Option<String>,
    /// The mnemonic, lowercase.
    pub mnemonic: String,
    /// The operands, in written order, with surrounding space trimmed.
    pub operands: Vec<String>,
}

/// Splits a line of assembly into a prefix, mnemonic and operands.
///
/// Returns `None` for a line that carries no instruction: a blank line, a
/// comment, a bare label, or a directive.
pub fn split_statement(line: &str) -> Option<Statement> {
    let code = strip_comment(line);
    let code = strip_label(code).trim();
    if code.is_empty() {
        return None;
    }

    let mut words = code.splitn(2, char::is_whitespace);
    let mut head = words.next()?.trim().to_ascii_lowercase();
    let mut rest = words.next().unwrap_or("").trim();

    let mut prefix = None;
    if PREFIXES.contains(&head.as_str()) && !rest.is_empty() {
        prefix = Some(head);
        let mut inner = rest.splitn(2, char::is_whitespace);
        head = inner.next().unwrap_or("").trim().to_ascii_lowercase();
        rest = inner.next().unwrap_or("").trim();
    }

    if head.is_empty() || head.ends_with(':') {
        return None;
    }

    Some(Statement {
        prefix,
        mnemonic: head,
        operands: split_operands(rest),
    })
}

/// Removes a trailing `;` comment, respecting string literals.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quote: Option<u8> = None;
    for (index, byte) in bytes.iter().enumerate() {
        match quote {
            Some(open) => {
                if *byte == open {
                    quote = None;
                }
            }
            None => match byte {
                b'\'' | b'"' | b'`' => quote = Some(*byte),
                b';' => return &line[..index],
                _ => {}
            },
        }
    }
    line
}

/// Removes a leading `label:` from a statement.
fn strip_label(line: &str) -> &str {
    let trimmed = line.trim_start();
    let Some(colon) = trimmed.find(':') else {
        return line;
    };
    // A colon inside brackets belongs to an operand, not a label.
    if trimmed[..colon].contains(['[', ' ', '\t', ',']) {
        return line;
    }
    &trimmed[colon + 1..]
}

/// Splits operands on commas that are not inside brackets or quotes.
fn split_operands(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    let mut operands = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut current = String::new();

    for ch in text.chars() {
        match quote {
            Some(open) => {
                current.push(ch);
                if ch == open {
                    quote = None;
                }
            }
            None => match ch {
                '\'' | '"' | '`' => {
                    quote = Some(ch);
                    current.push(ch);
                }
                '[' | '(' => {
                    depth += 1;
                    current.push(ch);
                }
                ']' | ')' => {
                    depth = depth.saturating_sub(1);
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    operands.push(current.trim().to_owned());
                    current.clear();
                }
                other => current.push(other),
            },
        }
    }

    if !current.trim().is_empty() {
        operands.push(current.trim().to_owned());
    }
    operands
}

/// A rendered explanation of one instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    /// The mnemonic as written, lowercase.
    pub mnemonic: String,
    /// Any instruction prefix that was present.
    pub prefix: Option<String>,
    /// A one-line description of the instruction.
    pub summary: String,
    /// The category it belongs to.
    pub group: String,
    /// What it does, with the actual operands substituted.
    pub effect: String,
    /// What it reads, as display names.
    pub reads: Vec<String>,
    /// What it writes, as display names.
    pub writes: Vec<String>,
    /// Flags it modifies.
    pub flags_modified: Vec<Flag>,
    /// Flags it consults.
    pub flags_read: Vec<Flag>,
    /// Flags it leaves architecturally undefined.
    pub flags_undefined: Vec<Flag>,
    /// Extra guidance.
    pub notes: Option<String>,
    /// Whether the effect is described only approximately.
    pub approximate: bool,
}

impl Explanation {
    /// Renders the explanation as the lines the panel displays.
    ///
    /// Sections with nothing to say are omitted rather than shown empty.
    pub fn lines(&self) -> Vec<(String, String)> {
        let mut lines = vec![("Effect".to_owned(), self.effect.clone())];

        if !self.reads.is_empty() {
            lines.push(("Reads".to_owned(), self.reads.join(", ")));
        }
        if !self.writes.is_empty() {
            lines.push(("Writes".to_owned(), self.writes.join(", ")));
        }
        if !self.flags_modified.is_empty() {
            lines.push(("Flags set".to_owned(), join_flags(&self.flags_modified)));
        }
        if !self.flags_read.is_empty() {
            lines.push(("Flags read".to_owned(), join_flags(&self.flags_read)));
        }
        if !self.flags_undefined.is_empty() {
            lines.push((
                "Flags undefined".to_owned(),
                join_flags(&self.flags_undefined),
            ));
        }
        lines
    }
}

/// Joins flag abbreviations for display.
fn join_flags(flags: &[Flag]) -> String {
    flags
        .iter()
        .map(|flag| flag.abbreviation())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Explains one line of assembly.
///
/// Returns `None` when the line holds no instruction, or when the instruction
/// is not in the database.
pub fn explain_line(database: &Database, line: &str) -> Option<Explanation> {
    let statement = split_statement(line)?;
    explain_statement(database, &statement)
}

/// Explains an already-split statement.
pub fn explain_statement(database: &Database, statement: &Statement) -> Option<Explanation> {
    let info = database.get(&statement.mnemonic)?;
    Some(build(info, statement))
}

/// Builds an explanation by substituting operands into the database entry.
fn build(info: &InstructionInfo, statement: &Statement) -> Explanation {
    let effect = substitute(&info.effect, info, statement);
    let reads = resolve_slots(&info.reads, info, statement);
    let writes = resolve_slots(&info.writes, info, statement);

    Explanation {
        mnemonic: statement.mnemonic.clone(),
        prefix: statement.prefix.clone(),
        summary: info.summary.clone(),
        group: info.group.clone(),
        effect,
        reads,
        writes,
        flags_modified: info.flags_modified(),
        flags_read: info.flags_consulted(),
        flags_undefined: info.flags_left_undefined(),
        notes: info.notes.clone(),
        approximate: info.is_approximate(),
    }
}

/// Replaces `{slot}` placeholders with the operands actually written.
///
/// A slot with no corresponding operand keeps its placeholder name in angle
/// brackets, so `add rax` renders as `RAX ← RAX + <src>` rather than silently
/// dropping the missing operand.
fn substitute(template: &str, info: &InstructionInfo, statement: &Statement) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let slot = &rest[start + 1..start + end];
        out.push_str(&operand_for(slot, info, statement).unwrap_or_else(|| format!("<{slot}>")));
        rest = &rest[start + end + 1..];
    }

    out.push_str(rest);
    out
}

/// The text written for an operand slot.
fn operand_for(slot: &str, info: &InstructionInfo, statement: &Statement) -> Option<String> {
    let index = info.operands.iter().position(|operand| operand == slot)?;
    statement.operands.get(index).map(|text| display(text))
}

/// Resolves a list of read or write slots into display names.
fn resolve_slots(slots: &[String], info: &InstructionInfo, statement: &Statement) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for slot in slots {
        let text = match operand_for(slot, info, statement) {
            Some(text) => text,
            None if info.operands.iter().any(|operand| operand == slot) => {
                // A declared operand that the user did not write; omit it
                // rather than inventing a name.
                continue;
            }
            None => display(slot),
        };
        if !out.contains(&text) {
            out.push(text);
        }
    }
    out
}

/// Renders an operand or register name for display.
///
/// Register names are upper-cased to match the register panel; everything else
/// — immediates, labels, memory expressions — is shown exactly as written, so
/// the user can match it against their own source.
fn display(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed == "memory" {
        return "memory".to_owned();
    }
    if registers::is_register(trimmed) || Flag::from_abbreviation(trimmed).is_some() {
        return trimmed.to_ascii_uppercase();
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Database {
        Database::load().expect("database")
    }

    fn explain(line: &str) -> Explanation {
        explain_line(&database(), line)
            .unwrap_or_else(|| panic!("{line:?} produced no explanation"))
    }

    #[test]
    fn the_documented_example_renders_correctly() {
        // The example from the specification.
        let explanation = explain("add rax, rbx");
        assert_eq!(explanation.effect, "RAX ← RAX + RBX");
        assert_eq!(explanation.reads, ["RAX", "RBX"]);
        assert_eq!(explanation.writes, ["RAX"]);
        assert_eq!(
            join_flags(&explanation.flags_modified),
            "CF, PF, AF, ZF, SF, OF"
        );
    }

    #[test]
    fn operands_are_substituted_in_order() {
        // The first operand fills {dst}, the second {src}, never the reverse.
        assert_eq!(explain("sub rcx, rdx").effect, "RCX ← RCX - RDX");
        assert_eq!(explain("sub rdx, rcx").effect, "RDX ← RDX - RCX");
    }

    #[test]
    fn immediates_are_shown_as_written() {
        let explanation = explain("mov rax, 60");
        assert_eq!(explanation.effect, "RAX ← 60");
        assert_eq!(explanation.reads, ["60"]);
        assert_eq!(explanation.writes, ["RAX"]);
    }

    #[test]
    fn memory_operands_survive_intact() {
        let explanation = explain("mov rax, qword [rbx + rcx*8 + 16]");
        assert_eq!(explanation.effect, "RAX ← qword [rbx + rcx*8 + 16]");
    }

    #[test]
    fn a_comma_inside_brackets_does_not_split_operands() {
        let statement = split_statement("    mov rax, [rbx + rcx]").expect("statement");
        assert_eq!(statement.operands, ["rax", "[rbx + rcx]"]);
    }

    #[test]
    fn a_leading_label_is_ignored() {
        let statement = split_statement("_start: mov rax, 1").expect("statement");
        assert_eq!(statement.mnemonic, "mov");
        assert_eq!(statement.operands, ["rax", "1"]);
    }

    #[test]
    fn a_trailing_comment_is_ignored() {
        let explanation = explain("    add rax, rbx    ; sum them");
        assert_eq!(explanation.effect, "RAX ← RAX + RBX");
    }

    #[test]
    fn a_semicolon_inside_a_string_is_not_a_comment() {
        let statement = split_statement(r#"    db "a;b", 0"#).expect("statement");
        assert_eq!(statement.operands, [r#""a;b""#, "0"]);
    }

    #[test]
    fn an_instruction_prefix_is_recognised() {
        let statement = split_statement("    rep movsb").expect("statement");
        assert_eq!(statement.prefix.as_deref(), Some("rep"));
        assert_eq!(statement.mnemonic, "movsb");

        let explanation = explain("    rep movsb");
        assert_eq!(explanation.prefix.as_deref(), Some("rep"));
        assert_eq!(explanation.mnemonic, "movsb");
    }

    #[test]
    fn a_lock_prefix_is_recognised() {
        let statement = split_statement("lock xadd [rbx], rax").expect("statement");
        assert_eq!(statement.prefix.as_deref(), Some("lock"));
        assert_eq!(statement.mnemonic, "xadd");
    }

    #[test]
    fn lines_with_no_instruction_produce_nothing() {
        let db = database();
        for line in ["", "   ", "; just a comment", "_start:", ".loop:"] {
            assert!(
                explain_line(&db, line).is_none(),
                "{line:?} should not explain"
            );
        }
    }

    #[test]
    fn an_unknown_instruction_produces_nothing_rather_than_a_guess() {
        // The honesty requirement: never invent semantics.
        let db = database();
        assert!(explain_line(&db, "    frobnicate rax, rbx").is_none());
    }

    #[test]
    fn a_missing_operand_is_marked_rather_than_dropped() {
        // `add rax` is invalid assembly; the explanation must not pretend the
        // second operand exists, nor silently omit it from the effect.
        let explanation = explain("add rax");
        assert_eq!(explanation.effect, "RAX ← RAX + <src>");
        assert_eq!(explanation.reads, ["RAX"], "the absent operand is not read");
    }

    #[test]
    fn case_is_normalised_for_lookup_but_registers_are_upper_cased() {
        let explanation = explain("    MOV RAX, RBX");
        assert_eq!(explanation.mnemonic, "mov");
        assert_eq!(explanation.effect, "RAX ← RBX");
    }

    #[test]
    fn a_conditional_jump_explains_its_condition() {
        let explanation = explain("    je .loop");
        assert_eq!(explanation.effect, "if ZF = 1 then RIP ← .loop");
        assert_eq!(join_flags(&explanation.flags_read), "ZF");
        assert!(explanation.flags_modified.is_empty());
        assert_eq!(explanation.group, "conditional-jump");
    }

    #[test]
    fn a_synonym_jump_explains_the_same_condition() {
        assert_eq!(explain("jz .loop").effect, explain("je .loop").effect);
    }

    #[test]
    fn signed_and_unsigned_jumps_are_explained_differently() {
        let signed = explain("jg .target");
        let unsigned = explain("ja .target");
        assert_ne!(signed.effect, unsigned.effect);
        assert!(signed.notes.unwrap_or_default().contains("Signed"));
        assert!(unsigned.notes.unwrap_or_default().contains("Unsigned"));
    }

    #[test]
    fn push_explains_its_effect_on_the_stack_pointer() {
        let explanation = explain("    push rbp");
        assert_eq!(explanation.effect, "RSP ← RSP - 8, [RSP] ← RBP");
        assert!(explanation.reads.contains(&"RBP".to_owned()));
        assert!(explanation.writes.contains(&"RSP".to_owned()));
        assert!(explanation.writes.contains(&"memory".to_owned()));
    }

    #[test]
    fn syscall_lists_the_registers_it_uses_and_destroys() {
        let explanation = explain("    syscall");
        assert!(explanation.reads.contains(&"RAX".to_owned()));
        assert!(explanation.reads.contains(&"R10".to_owned()));
        assert!(explanation.writes.contains(&"RCX".to_owned()));
        assert!(explanation.writes.contains(&"R11".to_owned()));
    }

    #[test]
    fn instructions_with_undefined_flags_say_so() {
        let explanation = explain("    div rbx");
        assert!(
            !explanation.flags_undefined.is_empty(),
            "div leaves flags undefined and must say so"
        );
        assert!(explanation.flags_modified.is_empty());
    }

    #[test]
    fn a_vector_instruction_is_marked_approximate() {
        let explanation = explain("    pxor xmm0, xmm1");
        assert!(explanation.approximate);
        assert!(explanation
            .notes
            .unwrap_or_default()
            .contains("does not model"));
    }

    #[test]
    fn rendered_lines_omit_empty_sections() {
        let explanation = explain("    nop");
        let labels: Vec<String> = explanation
            .lines()
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        assert_eq!(labels, ["Effect"], "nop reads and writes nothing");
    }

    #[test]
    fn rendered_lines_cover_every_section_when_present() {
        let explanation = explain("add rax, rbx");
        let labels: Vec<String> = explanation
            .lines()
            .iter()
            .map(|(label, _)| label.clone())
            .collect();
        assert_eq!(labels, ["Effect", "Reads", "Writes", "Flags set"]);
    }

    #[test]
    fn every_line_of_the_hello_world_template_is_explained_or_skipped() {
        // Nothing in the template should produce a wrong or missing answer.
        let db = database();
        for line in crate::project::template::HELLO_WORLD.lines() {
            let Some(statement) = split_statement(line) else {
                continue;
            };
            // Directives are not instructions and correctly have no entry.
            if matches!(
                statement.mnemonic.as_str(),
                "section" | "global" | "db" | "equ"
            ) {
                continue;
            }
            assert!(
                explain_statement(&db, &statement).is_some(),
                "no explanation for {:?} in {line:?}",
                statement.mnemonic
            );
        }
    }

    #[test]
    fn every_database_entry_can_be_explained_with_placeholder_operands() {
        // Guards against a template that cannot render.
        let db = database();
        for info in &db.instructions {
            let statement = Statement {
                prefix: None,
                mnemonic: info.mnemonic.clone(),
                operands: info.operands.iter().map(|_| "rax".to_owned()).collect(),
            };
            let explanation =
                explain_statement(&db, &statement).unwrap_or_else(|| panic!("{}", info.mnemonic));
            assert!(
                !explanation.effect.contains('{'),
                "{}: unsubstituted placeholder in {:?}",
                info.mnemonic,
                explanation.effect
            );
            // `<slot>` marks an operand the user did not write. A literal
            // `<` also appears in condition text such as "SF <> OF", so the
            // check looks for the declared slot names specifically.
            for slot in &info.operands {
                assert!(
                    !explanation.effect.contains(&format!("<{slot}>")),
                    "{}: operand {slot} was not substituted in {:?}",
                    info.mnemonic,
                    explanation.effect
                );
            }
        }
    }
}

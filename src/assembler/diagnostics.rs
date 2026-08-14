//! Parsing assembler and linker output into structured diagnostics.
//!
//! The point of this module is to turn a wall of tool output into something
//! the editor can act on: jump to the offending line, mark the gutter, show
//! the message in context. That requires real parsing, because the two tools
//! involved disagree about almost everything.
//!
//! # Formats handled
//!
//! NASM emits, depending on its `-X` option:
//!
//! ```text
//! main.asm:5: error: symbol `foo' undefined
//! main.asm:5:9: error: ...            (with column, newer releases)
//! main.asm(5) : error: ...            (Visual C style, -Xvc)
//! ```
//!
//! GNU `ld` emits several unrelated shapes, and only some carry a location:
//!
//! ```text
//! ld: cannot find -lc
//! ld: warning: cannot find entry symbol _start; defaulting to 0000000000401000
//! main.o: in function `_start':
//! main.asm:(.text+0x11): undefined reference to `printf'
//! ```
//!
//! Anything unrecognised is preserved as a diagnostic with no location rather
//! than dropped. Silently discarding a message the user needs to see would be
//! worse than showing it without a line number.

use std::path::PathBuf;

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Supplementary context attached to a nearby error.
    Note,
    /// Something suspicious that did not stop the build.
    Warning,
    /// Something that stopped the build.
    Error,
}

impl Severity {
    /// The lowercase name used in tool output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Note => "note",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }

    /// Parses a severity word, defaulting to [`Severity::Error`].
    ///
    /// Defaulting to error is deliberate: an unrecognised severity on a line
    /// the tool bothered to print is more likely to be a problem than not, and
    /// an over-reported error is more visible than an under-reported one.
    pub fn parse(word: &str) -> Self {
        match word.trim().to_ascii_lowercase().as_str() {
            "warning" | "warn" => Severity::Warning,
            "note" | "info" => Severity::Note,
            _ => Severity::Error,
        }
    }
}

/// Which tool produced a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Producer {
    /// The assembler.
    Assembler,
    /// The linker.
    Linker,
}

impl Producer {
    /// A short name for display.
    pub const fn as_str(self) -> &'static str {
        match self {
            Producer::Assembler => "assembler",
            Producer::Linker => "linker",
        }
    }
}

/// One parsed message from a build tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The file the message refers to, when it names one.
    pub file: Option<PathBuf>,
    /// The one-based line number, when the message carries one.
    pub line: Option<usize>,
    /// The one-based column, when the message carries one.
    pub column: Option<usize>,
    /// How serious the message is.
    pub severity: Severity,
    /// The message text, with the location prefix removed.
    pub message: String,
    /// Which tool produced it.
    pub producer: Producer,
    /// The original line, kept so the raw output can always be shown.
    pub raw: String,
}

impl Diagnostic {
    /// Whether the diagnostic can be navigated to.
    pub fn has_location(&self) -> bool {
        self.file.is_some() && self.line.is_some()
    }

    /// The zero-based line index, for addressing a buffer.
    ///
    /// Tools report one-based lines; buffers are zero-based. Converting in one
    /// place avoids an off-by-one at every call site.
    pub fn buffer_line(&self) -> Option<usize> {
        self.line.map(|line| line.saturating_sub(1))
    }

    /// The zero-based column index, for addressing a buffer.
    pub fn buffer_column(&self) -> usize {
        self.column.map_or(0, |column| column.saturating_sub(1))
    }

    /// A one-line rendering such as `main.asm:5: error: ...`.
    pub fn summary(&self) -> String {
        let mut out = String::new();
        if let Some(file) = &self.file {
            out.push_str(&file.display().to_string());
            if let Some(line) = self.line {
                out.push(':');
                out.push_str(&line.to_string());
                if let Some(column) = self.column {
                    out.push(':');
                    out.push_str(&column.to_string());
                }
            }
            out.push_str(": ");
        }
        out.push_str(self.severity.as_str());
        out.push_str(": ");
        out.push_str(&self.message);
        out
    }
}

/// Parses assembler output into diagnostics.
pub fn parse_assembler_output(output: &str) -> Vec<Diagnostic> {
    output
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(|line| parse_line(line, Producer::Assembler))
        .collect()
}

/// Parses linker output into diagnostics.
pub fn parse_linker_output(output: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    // `ld` prints "file: in function `name':" as context for the message that
    // follows. Carrying it forward turns two lines into one useful diagnostic.
    let mut pending_context: Option<String> = None;

    for line in output.lines().map(str::trim_end) {
        if line.trim().is_empty() {
            continue;
        }

        if let Some(context) = parse_linker_context(line) {
            pending_context = Some(context);
            continue;
        }

        let mut diagnostic = parse_line(line, Producer::Linker);
        if let Some(context) = pending_context.take() {
            diagnostic.message = format!("{} ({context})", diagnostic.message);
        }
        diagnostics.push(diagnostic);
    }

    diagnostics
}

/// Recognises `ld`'s "in function" context line.
fn parse_linker_context(line: &str) -> Option<String> {
    let trimmed = line.trim().trim_start_matches("ld: ");
    let position = trimmed.find(": in function ")?;
    let function = trimmed[position + ": in function ".len()..]
        .trim()
        .trim_end_matches(':')
        .trim_matches(|ch| ch == '`' || ch == '\'');
    Some(format!("in {function}"))
}

/// Parses a single output line into a diagnostic.
fn parse_line(line: &str, producer: Producer) -> Diagnostic {
    let raw = line.to_owned();
    let trimmed = line.trim();

    // `ld:` and `nasm:` prefixes carry no location; strip them so the rest of
    // the line can still be examined for one.
    let body = trimmed
        .strip_prefix("ld: ")
        .or_else(|| trimmed.strip_prefix("nasm: "))
        .unwrap_or(trimmed);

    if let Some(diagnostic) = parse_visual_c_style(body, producer, &raw) {
        return diagnostic;
    }
    if let Some(diagnostic) = parse_gnu_style(body, producer, &raw) {
        return diagnostic;
    }

    Diagnostic {
        file: None,
        line: None,
        column: None,
        severity: detect_severity(body),
        message: body.to_owned(),
        producer,
        raw,
    }
}

/// Parses `file(line) : severity: message`.
fn parse_visual_c_style(body: &str, producer: Producer, raw: &str) -> Option<Diagnostic> {
    let open = body.find('(')?;
    let close = body[open..].find(')')? + open;
    let line = body[open + 1..close].trim().parse::<usize>().ok()?;
    let rest = body[close + 1..].trim_start().strip_prefix(':')?.trim();
    let (severity, message) = split_severity(rest);

    Some(Diagnostic {
        file: Some(PathBuf::from(body[..open].trim())),
        line: Some(line),
        column: None,
        severity,
        message,
        producer,
        raw: raw.to_owned(),
    })
}

/// Parses `file:line[:column]: severity: message`.
///
/// The tricky part is telling a line number from the rest of the path on
/// Windows-style or colon-containing names, so a field only counts as a
/// location when it parses as a number.
fn parse_gnu_style(body: &str, producer: Producer, raw: &str) -> Option<Diagnostic> {
    let mut fields = body.splitn(4, ':');
    let file = fields.next()?.trim();
    if file.is_empty() {
        return None;
    }

    let second = fields.next()?.trim();
    // `ld` writes `main.asm:(.text+0x11): undefined reference to ...` where the
    // line number is absent. Keep the file, drop the missing line.
    if second.starts_with('(') {
        let rest = fields.collect::<Vec<_>>().join(":");
        let (severity, message) = split_severity(rest.trim());
        return Some(Diagnostic {
            file: Some(PathBuf::from(file)),
            line: None,
            column: None,
            severity,
            message: format!("{second}: {message}").trim().to_owned(),
            producer,
            raw: raw.to_owned(),
        });
    }

    let line = second.parse::<usize>().ok()?;
    let third = fields.next().unwrap_or("").trim();
    let remainder = fields.next().unwrap_or("").trim();

    // The third field is a column only when it is numeric; otherwise it is the
    // start of the message.
    let (column, rest) = match third.parse::<usize>() {
        Ok(column) => (Some(column), remainder.to_owned()),
        Err(_) => {
            let mut joined = third.to_owned();
            if !remainder.is_empty() {
                joined.push(':');
                joined.push_str(remainder);
            }
            (None, joined)
        }
    };

    let (severity, message) = split_severity(rest.trim());
    Some(Diagnostic {
        file: Some(PathBuf::from(file)),
        line: Some(line),
        column,
        severity,
        message,
        producer,
        raw: raw.to_owned(),
    })
}

/// Splits a leading `severity:` word off a message.
fn split_severity(text: &str) -> (Severity, String) {
    match text.split_once(':') {
        Some((word, rest))
            if matches!(
                word.trim().to_ascii_lowercase().as_str(),
                "error" | "warning" | "warn" | "note" | "info" | "fatal"
            ) =>
        {
            let severity = if word.trim().eq_ignore_ascii_case("fatal") {
                Severity::Error
            } else {
                Severity::parse(word)
            };
            (severity, rest.trim().to_owned())
        }
        _ => (detect_severity(text), text.trim().to_owned()),
    }
}

/// Guesses a severity from a message that has no explicit severity word.
fn detect_severity(text: &str) -> Severity {
    let lowered = text.to_ascii_lowercase();
    if lowered.contains("warning") {
        Severity::Warning
    } else if lowered.contains("note") {
        Severity::Note
    } else {
        Severity::Error
    }
}

/// The first diagnostic that can be navigated to.
///
/// Used to jump straight to the first real problem after a failed build.
pub fn first_navigable(diagnostics: &[Diagnostic]) -> Option<&Diagnostic> {
    diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity == Severity::Error && diagnostic.has_location())
        .or_else(|| diagnostics.iter().find(|d| d.has_location()))
}

/// Counts diagnostics by severity, for a build summary line.
pub fn counts(diagnostics: &[Diagnostic]) -> (usize, usize) {
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warnings = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();
    (errors, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real NASM output, reproduced verbatim.
    const NASM_OUTPUT: &str = "\
main.asm:5: error: symbol `undefined_label' undefined
main.asm:9: warning: label alone on a line without a colon might be in error
main.asm:12: error: invalid combination of opcode and operands
";

    /// Real GNU ld output for an undefined reference.
    const LD_OUTPUT: &str = "\
ld: warning: cannot find entry symbol _start; defaulting to 0000000000401000
ld: main.o: in function `main':
main.asm:(.text+0x11): undefined reference to `printf'
";

    #[test]
    fn nasm_errors_are_parsed_with_file_and_line() {
        let diagnostics = parse_assembler_output(NASM_OUTPUT);
        assert_eq!(diagnostics.len(), 3);

        let first = &diagnostics[0];
        assert_eq!(first.file, Some(PathBuf::from("main.asm")));
        assert_eq!(first.line, Some(5));
        assert_eq!(first.severity, Severity::Error);
        assert_eq!(first.message, "symbol `undefined_label' undefined");
        assert_eq!(first.producer, Producer::Assembler);
    }

    #[test]
    fn nasm_warnings_are_distinguished_from_errors() {
        let diagnostics = parse_assembler_output(NASM_OUTPUT);
        assert_eq!(diagnostics[1].severity, Severity::Warning);
        assert_eq!(counts(&diagnostics), (2, 1));
    }

    #[test]
    fn a_column_is_parsed_when_present() {
        let diagnostics = parse_assembler_output("main.asm:5:9: error: bad operand\n");
        assert_eq!(diagnostics[0].line, Some(5));
        assert_eq!(diagnostics[0].column, Some(9));
        assert_eq!(diagnostics[0].message, "bad operand");
    }

    #[test]
    fn a_message_containing_a_colon_is_not_mistaken_for_a_column() {
        // The failure mode this guards against: reading "error" as a column.
        let diagnostics = parse_assembler_output("main.asm:5: error: expected: comma\n");
        assert_eq!(diagnostics[0].column, None);
        assert_eq!(diagnostics[0].message, "expected: comma");
    }

    #[test]
    fn visual_c_style_output_is_parsed() {
        let diagnostics = parse_assembler_output("main.asm(5) : error: bad thing\n");
        assert_eq!(diagnostics[0].file, Some(PathBuf::from("main.asm")));
        assert_eq!(diagnostics[0].line, Some(5));
        assert_eq!(diagnostics[0].message, "bad thing");
    }

    #[test]
    fn a_fatal_error_is_reported_as_an_error() {
        let diagnostics = parse_assembler_output("main.asm:1: fatal: unable to open file\n");
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].message, "unable to open file");
    }

    #[test]
    fn linker_messages_without_a_location_are_kept() {
        // Dropping these would hide the actual cause of a failed link.
        let diagnostics = parse_linker_output("ld: cannot find -lc\n");
        assert_eq!(diagnostics.len(), 1);
        assert!(!diagnostics[0].has_location());
        assert_eq!(diagnostics[0].message, "cannot find -lc");
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].producer, Producer::Linker);
    }

    #[test]
    fn a_linker_warning_is_recognised_without_a_severity_prefix() {
        let diagnostics = parse_linker_output(LD_OUTPUT);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert!(diagnostics[0].message.contains("cannot find entry symbol"));
    }

    #[test]
    fn an_undefined_reference_keeps_its_file_and_section_offset() {
        let diagnostics = parse_linker_output(LD_OUTPUT);
        let undefined = diagnostics
            .iter()
            .find(|d| d.message.contains("undefined reference"))
            .expect("undefined reference diagnostic");
        assert_eq!(undefined.file, Some(PathBuf::from("main.asm")));
        assert!(undefined.message.contains("(.text+0x11)"));
        assert_eq!(undefined.severity, Severity::Error);
    }

    #[test]
    fn the_in_function_context_is_folded_into_the_next_message() {
        // Two lines of ld output describe one problem; joining them means the
        // user sees where the reference came from.
        let diagnostics = parse_linker_output(LD_OUTPUT);
        assert_eq!(
            diagnostics.len(),
            2,
            "the context line is not its own entry"
        );
        let undefined = &diagnostics[1];
        assert!(
            undefined.message.contains("in main"),
            "expected function context, got {:?}",
            undefined.message
        );
    }

    #[test]
    fn unparseable_output_is_preserved_rather_than_dropped() {
        let noise = "something entirely unexpected happened\n";
        let diagnostics = parse_assembler_output(noise);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, noise.trim());
        assert_eq!(diagnostics[0].raw, noise.trim());
        assert!(!diagnostics[0].has_location());
    }

    #[test]
    fn empty_output_yields_no_diagnostics() {
        assert!(parse_assembler_output("").is_empty());
        assert!(parse_assembler_output("\n\n  \n").is_empty());
        assert!(parse_linker_output("").is_empty());
    }

    #[test]
    fn line_numbers_convert_to_zero_based_buffer_indices() {
        let diagnostics = parse_assembler_output("main.asm:1:1: error: x\n");
        assert_eq!(diagnostics[0].buffer_line(), Some(0));
        assert_eq!(diagnostics[0].buffer_column(), 0);
    }

    #[test]
    fn a_missing_column_maps_to_the_start_of_the_line() {
        let diagnostics = parse_assembler_output("main.asm:7: error: x\n");
        assert_eq!(diagnostics[0].buffer_line(), Some(6));
        assert_eq!(diagnostics[0].buffer_column(), 0);
    }

    #[test]
    fn the_first_navigable_error_is_preferred_over_a_warning() {
        let mut diagnostics = parse_assembler_output(
            "main.asm:2: warning: something\nmain.asm:9: error: the real problem\n",
        );
        diagnostics.push(Diagnostic {
            file: None,
            line: None,
            column: None,
            severity: Severity::Error,
            message: "no location".to_owned(),
            producer: Producer::Linker,
            raw: String::new(),
        });

        let first = first_navigable(&diagnostics).expect("a navigable diagnostic");
        assert_eq!(first.line, Some(9));
    }

    #[test]
    fn first_navigable_falls_back_to_a_warning_when_there_is_no_error() {
        let diagnostics = parse_assembler_output("main.asm:3: warning: only this\n");
        assert_eq!(first_navigable(&diagnostics).and_then(|d| d.line), Some(3));
    }

    #[test]
    fn first_navigable_returns_nothing_when_no_message_has_a_location() {
        let diagnostics = parse_linker_output("ld: cannot find -lc\n");
        assert!(first_navigable(&diagnostics).is_none());
    }

    #[test]
    fn a_path_with_directories_is_preserved() {
        let diagnostics = parse_assembler_output("src/boot/main.asm:5: error: x\n");
        assert_eq!(
            diagnostics[0].file,
            Some(PathBuf::from("src/boot/main.asm"))
        );
    }

    #[test]
    fn summary_renders_the_original_shape() {
        let diagnostics = parse_assembler_output("main.asm:5:9: error: bad operand\n");
        assert_eq!(diagnostics[0].summary(), "main.asm:5:9: error: bad operand");

        let diagnostics = parse_assembler_output("main.asm:5: warning: odd\n");
        assert_eq!(diagnostics[0].summary(), "main.asm:5: warning: odd");
    }

    #[test]
    fn severity_ordering_puts_errors_last_for_sorting() {
        assert!(Severity::Note < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn parsing_is_not_confused_by_carriage_returns() {
        let diagnostics = parse_assembler_output("main.asm:5: error: bad\r\n");
        assert_eq!(diagnostics[0].message, "bad");
    }
}

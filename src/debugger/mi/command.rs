//! Building GDB/MI commands.
//!
//! Commands are constructed as a name plus a list of arguments and rendered
//! once, at the point of sending. Arguments that need quoting are quoted and
//! escaped here, so a path containing a space — or a source file whose name
//! contains a quote — cannot break the protocol framing.
//!
//! Every command carries a numeric token. GDB echoes it on the matching result
//! record, which is what lets a reply be identified when asynchronous events
//! are interleaved with it.

use std::fmt;
use std::path::Path;

/// A GDB/MI command awaiting a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    name: String,
    args: Vec<String>,
}

impl Command {
    /// Creates a command with the given MI operation name.
    ///
    /// The leading `-` is added if it is not already present.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let name = if name.starts_with('-') || name.is_empty() {
            name
        } else {
            format!("-{name}")
        };
        Self {
            name,
            args: Vec::new(),
        }
    }

    /// Appends one argument.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Appends several arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// The operation name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The arguments, unquoted.
    pub fn arguments(&self) -> &[String] {
        &self.args
    }

    /// Renders the command with `token`, without a trailing newline.
    pub fn render(&self, token: u32) -> String {
        let mut out = format!("{token}{}", self.name);
        for arg in &self.args {
            out.push(' ');
            out.push_str(&quote(arg));
        }
        out
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)?;
        for arg in &self.args {
            write!(f, " {}", quote(arg))?;
        }
        Ok(())
    }
}

/// Quotes an argument if the MI grammar requires it.
///
/// Arguments that are plain words are passed through unquoted, which keeps
/// commands readable in the protocol log. Anything containing whitespace, a
/// quote, a backslash or a control character is wrapped in a C-string.
pub fn quote(arg: &str) -> String {
    let needs_quoting = arg.is_empty()
        || arg
            .chars()
            .any(|ch| ch.is_whitespace() || ch == '"' || ch == '\\' || ch.is_control());

    if !needs_quoting {
        return arg.to_owned();
    }

    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    for ch in arg.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Loads an executable and its symbols.
pub fn file_exec_and_symbols(path: &Path) -> Command {
    Command::new("-file-exec-and-symbols").arg(path.display().to_string())
}

/// Sets the arguments the program will be started with.
pub fn exec_arguments<I, S>(args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    Command::new("-exec-arguments").args(args)
}

/// Sets the working directory for the program.
pub fn environment_cd(path: &Path) -> Command {
    Command::new("-environment-cd").arg(path.display().to_string())
}

/// Starts the program, stopping at its entry point.
///
/// `--start` is used rather than plain run so the debugger always gets an
/// initial stop, giving the user a chance to inspect the program before any of
/// it executes.
pub fn exec_run_to_start() -> Command {
    Command::new("-exec-run").arg("--start")
}

/// Starts the program without stopping at the entry point.
pub fn exec_run() -> Command {
    Command::new("-exec-run")
}

/// Resumes execution.
pub fn exec_continue() -> Command {
    Command::new("-exec-continue")
}

/// Interrupts a running program.
pub fn exec_interrupt() -> Command {
    Command::new("-exec-interrupt").arg("--all")
}

/// Executes one machine instruction, entering calls.
pub fn exec_step_instruction() -> Command {
    Command::new("-exec-step-instruction")
}

/// Executes one machine instruction, stepping over calls.
pub fn exec_next_instruction() -> Command {
    Command::new("-exec-next-instruction")
}

/// Executes one source line, entering calls.
pub fn exec_step() -> Command {
    Command::new("-exec-step")
}

/// Executes one source line, stepping over calls.
pub fn exec_next() -> Command {
    Command::new("-exec-next")
}

/// Runs until the current function returns.
pub fn exec_finish() -> Command {
    Command::new("-exec-finish")
}

/// Sets a breakpoint at a source location or symbol.
pub fn break_insert(location: &str) -> Command {
    Command::new("-break-insert").arg(location)
}

/// Sets a breakpoint at a file and line.
pub fn break_insert_at_line(file: &str, line: usize) -> Command {
    Command::new("-break-insert").arg(format!("{file}:{line}"))
}

/// Sets a breakpoint at a raw address.
///
/// The `*` prefix is MI's syntax for "this is an address, not a symbol".
pub fn break_insert_at_address(address: u64) -> Command {
    Command::new("-break-insert").arg(format!("*0x{address:x}"))
}

/// Removes a breakpoint by number.
pub fn break_delete(number: u32) -> Command {
    Command::new("-break-delete").arg(number.to_string())
}

/// Enables a breakpoint.
pub fn break_enable(number: u32) -> Command {
    Command::new("-break-enable").arg(number.to_string())
}

/// Disables a breakpoint without removing it.
pub fn break_disable(number: u32) -> Command {
    Command::new("-break-disable").arg(number.to_string())
}

/// Lists all breakpoints.
pub fn break_list() -> Command {
    Command::new("-break-list")
}

/// Lists the names of every register, in GDB's numbering order.
pub fn data_list_register_names() -> Command {
    Command::new("-data-list-register-names")
}

/// Reads every register as a hexadecimal value.
pub fn data_list_register_values() -> Command {
    Command::new("-data-list-register-values").arg("x")
}

/// Reads the registers that changed since the previous stop.
pub fn data_list_changed_registers() -> Command {
    Command::new("-data-list-changed-registers")
}

/// Reads `count` bytes of memory starting at `address`.
pub fn data_read_memory_bytes(address: u64, count: usize) -> Command {
    Command::new("-data-read-memory-bytes")
        .arg(format!("0x{address:x}"))
        .arg(count.to_string())
}

/// Writes a value into memory.
pub fn data_write_memory_bytes(address: u64, contents: &str) -> Command {
    Command::new("-data-write-memory-bytes")
        .arg(format!("0x{address:x}"))
        .arg(contents)
}

/// Disassembles the address range `[start, end)`.
///
/// Mode 4 asks for source lines and opcode bytes alongside the instructions,
/// which is what the side-by-side disassembly view needs. GDB falls back to
/// instructions alone when the program has no debug information.
pub fn data_disassemble_range(start: u64, end: u64) -> Command {
    Command::new("-data-disassemble")
        .arg("-s")
        .arg(format!("0x{start:x}"))
        .arg("-e")
        .arg(format!("0x{end:x}"))
        .arg("--")
        .arg("4")
}

/// Disassembles the range with instructions and opcodes but no source.
pub fn data_disassemble_range_plain(start: u64, end: u64) -> Command {
    Command::new("-data-disassemble")
        .arg("-s")
        .arg(format!("0x{start:x}"))
        .arg("-e")
        .arg(format!("0x{end:x}"))
        .arg("--")
        .arg("2")
}

/// Evaluates an expression in the current frame.
pub fn data_evaluate_expression(expression: &str) -> Command {
    Command::new("-data-evaluate-expression").arg(expression)
}

/// Lists the frames of the call stack.
pub fn stack_list_frames() -> Command {
    Command::new("-stack-list-frames")
}

/// Selects a stack frame by level.
pub fn stack_select_frame(level: u32) -> Command {
    Command::new("-stack-select-frame").arg(level.to_string())
}

/// Lists information about the current threads.
pub fn thread_info() -> Command {
    Command::new("-thread-info")
}

/// Ends the debugging session.
pub fn gdb_exit() -> Command {
    Command::new("-gdb-exit")
}

/// Sets a GDB parameter, such as `disassembly-flavor`.
pub fn gdb_set(name: &str, value: &str) -> Command {
    Command::new("-gdb-set").arg(name).arg(value)
}

/// Selects the disassembly flavour.
///
/// Intel syntax is the default because the editor targets NASM, where reading
/// disassembly in AT&T order would mean mentally reversing every operand.
pub fn set_disassembly_flavor(intel: bool) -> Command {
    gdb_set("disassembly-flavor", if intel { "intel" } else { "att" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_renders_with_its_token() {
        assert_eq!(Command::new("-exec-run").render(7), "7-exec-run");
    }

    #[test]
    fn a_missing_leading_dash_is_added() {
        assert_eq!(Command::new("exec-run").name(), "-exec-run");
        assert_eq!(Command::new("-exec-run").name(), "-exec-run");
    }

    #[test]
    fn plain_arguments_are_not_quoted() {
        // Readability in the protocol log matters when debugging the debugger.
        let command = Command::new("-break-insert").arg("_start");
        assert_eq!(command.render(1), "1-break-insert _start");
    }

    #[test]
    fn arguments_with_spaces_are_quoted() {
        let command = Command::new("-file-exec-and-symbols").arg("/tmp/my project/main");
        assert_eq!(
            command.render(2),
            "2-file-exec-and-symbols \"/tmp/my project/main\""
        );
    }

    #[test]
    fn quotes_and_backslashes_are_escaped() {
        // The property that keeps a hostile path from breaking framing.
        assert_eq!(quote(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote(r"a\b"), r#""a\\b""#);
        assert_eq!(quote("a\nb"), r#""a\nb""#);
        assert_eq!(quote("a\tb"), r#""a\tb""#);
    }

    #[test]
    fn an_empty_argument_is_quoted_so_it_is_not_lost() {
        assert_eq!(quote(""), r#""""#);
        assert_eq!(Command::new("-x").arg("").render(1), r#"1-x """#);
    }

    #[test]
    fn a_path_that_would_break_framing_is_neutralised() {
        let hostile = Path::new("/tmp/evil\" -exec-run \"/x");
        let rendered = file_exec_and_symbols(hostile).render(3);
        // The injected command text must be inside one quoted argument.
        assert_eq!(
            rendered,
            r#"3-file-exec-and-symbols "/tmp/evil\" -exec-run \"/x""#
        );
        assert_eq!(rendered.matches("-exec-run").count(), 1);
    }

    #[test]
    fn breakpoint_commands_render_correctly() {
        assert_eq!(break_insert("_start").render(1), "1-break-insert _start");
        assert_eq!(
            break_insert_at_line("main.asm", 12).render(2),
            "2-break-insert main.asm:12"
        );
        assert_eq!(break_delete(3).render(4), "4-break-delete 3");
        assert_eq!(break_enable(1).render(5), "5-break-enable 1");
        assert_eq!(break_disable(1).render(6), "6-break-disable 1");
        assert_eq!(break_list().render(7), "7-break-list");
    }

    #[test]
    fn an_address_breakpoint_uses_the_star_prefix() {
        // Without the star GDB reads the text as a symbol name.
        assert_eq!(
            break_insert_at_address(0x40_00b0).render(1),
            "1-break-insert *0x4000b0"
        );
    }

    #[test]
    fn a_file_with_a_space_is_quoted_in_a_line_breakpoint() {
        let command = break_insert_at_line("my source.asm", 5);
        assert_eq!(command.render(1), "1-break-insert \"my source.asm:5\"");
    }

    #[test]
    fn memory_commands_use_hexadecimal_addresses() {
        assert_eq!(
            data_read_memory_bytes(0x7fff_ffff_e000, 64).render(1),
            "1-data-read-memory-bytes 0x7fffffffe000 64"
        );
    }

    #[test]
    fn disassembly_requests_the_documented_mode() {
        assert_eq!(
            data_disassemble_range(0x4000, 0x4010).render(1),
            "1-data-disassemble -s 0x4000 -e 0x4010 -- 4"
        );
        assert_eq!(
            data_disassemble_range_plain(0x4000, 0x4010).render(2),
            "2-data-disassemble -s 0x4000 -e 0x4010 -- 2"
        );
    }

    #[test]
    fn execution_commands_render_correctly() {
        assert_eq!(exec_run_to_start().render(1), "1-exec-run --start");
        assert_eq!(exec_continue().render(2), "2-exec-continue");
        assert_eq!(exec_step_instruction().render(3), "3-exec-step-instruction");
        assert_eq!(exec_next_instruction().render(4), "4-exec-next-instruction");
        assert_eq!(exec_step().render(5), "5-exec-step");
        assert_eq!(exec_next().render(6), "6-exec-next");
        assert_eq!(exec_finish().render(7), "7-exec-finish");
        assert_eq!(exec_interrupt().render(8), "8-exec-interrupt --all");
    }

    #[test]
    fn register_commands_request_hexadecimal_values() {
        assert_eq!(
            data_list_register_values().render(1),
            "1-data-list-register-values x"
        );
        assert_eq!(
            data_list_register_names().render(2),
            "2-data-list-register-names"
        );
        assert_eq!(
            data_list_changed_registers().render(3),
            "3-data-list-changed-registers"
        );
    }

    #[test]
    fn the_disassembly_flavour_defaults_to_intel() {
        assert_eq!(
            set_disassembly_flavor(true).render(1),
            "1-gdb-set disassembly-flavor intel"
        );
        assert_eq!(
            set_disassembly_flavor(false).render(2),
            "2-gdb-set disassembly-flavor att"
        );
    }

    #[test]
    fn program_arguments_are_quoted_individually() {
        let command = exec_arguments(["--flag", "a value", "plain"]);
        assert_eq!(
            command.render(1),
            "1-exec-arguments --flag \"a value\" plain"
        );
    }

    #[test]
    fn display_omits_the_token() {
        // Used in the protocol log, where the token is shown separately.
        assert_eq!(break_insert("_start").to_string(), "-break-insert _start");
    }

    #[test]
    fn a_rendered_command_never_contains_a_newline() {
        // A newline inside a command would be read as a second command.
        let command = Command::new("-break-insert").arg("evil\n-gdb-exit");
        let rendered = command.render(1);
        assert!(!rendered.contains('\n'), "rendered: {rendered}");
        assert!(rendered.contains("\\n"));
    }

    #[test]
    fn arguments_are_preserved_unquoted_for_inspection() {
        let command = Command::new("-x").arg("a b").arg("c");
        assert_eq!(command.arguments(), ["a b", "c"]);
    }
}

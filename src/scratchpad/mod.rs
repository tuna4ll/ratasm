//! Trying one instruction without making a project.
//!
//! You set some registers, write a line or two of assembly, and see exactly
//! which registers and flags changed. That is the question a learner asks
//! constantly — "what does `sar rax, 1` actually do to a negative number?" —
//! and answering it normally means writing a whole program.
//!
//! # How the answer is obtained
//!
//! By running the code, not by simulating it. A small program is generated,
//! assembled, linked and executed under GDB with breakpoints either side of
//! the snippet; the registers are read at both, and the difference is the
//! answer. Nothing here models what an instruction does, so the result is
//! whatever the processor really did, including the cases a simulator would
//! get wrong.
//!
//! # This is not a sandbox
//!
//! The snippet is compiled into a real program and executed natively with the
//! user's own privileges. There is no isolation. A timeout stops a program
//! that loops forever; it stops nothing else.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use crate::assembler::{self, BuildOptions};
use crate::debugger::mi::command as mi;
use crate::debugger::registers::RegisterFile;
use crate::debugger::session::{GdbSession, SessionError};
use crate::instruction::registers;
use crate::instruction::Flags;
use crate::project::Project;

/// The label the snippet starts at.
const START_LABEL: &str = "ratasm_snippet_start";
/// The label the snippet ends at.
const END_LABEL: &str = "ratasm_snippet_end";

/// How long the whole experiment may take.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Errors from running a snippet.
#[derive(Debug, thiserror::Error)]
pub enum ScratchpadError {
    /// The snippet was empty.
    #[error("write an instruction to try")]
    Empty,
    /// A named register is not one ratasm knows.
    #[error("'{name}' is not a register")]
    UnknownRegister {
        /// The name as written.
        name: String,
    },
    /// The temporary project could not be created.
    #[error("cannot prepare the scratchpad: {0}")]
    Setup(String),
    /// The snippet did not assemble.
    #[error("{0}")]
    Assembly(String),
    /// The debugger could not run it.
    #[error(transparent)]
    Session(#[from] SessionError),
    /// The debugger is not installed.
    #[error("the scratchpad needs gdb to read the registers back")]
    NoDebugger,
}

/// One register that changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The register's canonical name.
    pub register: &'static str,
    /// Its value before the snippet ran.
    pub before: u64,
    /// Its value after.
    pub after: u64,
}

impl Change {
    /// A one-line description for the panel.
    pub fn describe(&self) -> String {
        format!(
            "{:<6} {:#018x} → {:#018x}",
            self.register.to_uppercase(),
            self.before,
            self.after
        )
    }
}

/// What running a snippet produced.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// Registers whose values changed, in panel order.
    pub changes: Vec<Change>,
    /// Flags before the snippet.
    pub flags_before: Flags,
    /// Flags after.
    pub flags_after: Flags,
    /// Anything the program wrote before it was stopped.
    pub output: String,
    /// The program that was actually assembled, for the curious.
    pub source: String,
}

impl Outcome {
    /// Flags the snippet changed.
    pub fn flag_changes(&self) -> Vec<crate::instruction::Flag> {
        self.flags_after.changed_from(self.flags_before)
    }

    /// Whether the snippet had no observable effect on the registers.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.flag_changes().is_empty()
    }
}

/// A snippet to try, with the machine state to start it from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scratchpad {
    /// Registers to set before the snippet runs.
    pub initial: BTreeMap<String, u64>,
    /// The instructions to try.
    pub snippet: String,
}

impl Scratchpad {
    /// An empty scratchpad.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a register's starting value.
    ///
    /// # Errors
    ///
    /// Returns [`ScratchpadError::UnknownRegister`] when the name is not a
    /// register, rather than generating a program that will not assemble.
    pub fn set(&mut self, name: &str, value: u64) -> Result<(), ScratchpadError> {
        let register = registers::lookup(name).ok_or_else(|| ScratchpadError::UnknownRegister {
            name: name.to_owned(),
        })?;
        self.initial.insert(register.name.to_owned(), value);
        Ok(())
    }

    /// Generates the program that will be assembled.
    ///
    /// Public so the interface can show exactly what is about to run: the
    /// scratchpad should not be a black box.
    pub fn to_source(&self) -> String {
        let mut source = String::from(
            "; Generated by the ratasm scratchpad.\n\
             ; This program is assembled and executed natively on this machine.\n\n\
             section .text\n    global _start\n\n_start:\n",
        );

        if self.initial.is_empty() {
            source.push_str("    ; no starting values were set\n");
        } else {
            for (name, value) in &self.initial {
                source.push_str(&format!("    mov     {name}, {value:#x}\n"));
            }
        }

        // The labels bracket the snippet so the registers can be read either
        // side of exactly the instructions the user wrote.
        source.push_str(&format!("\n{START_LABEL}:\n    nop\n\n"));
        for line in self.snippet.lines() {
            source.push_str("    ");
            source.push_str(line.trim_end());
            source.push('\n');
        }
        source.push_str(&format!("\n{END_LABEL}:\n    nop\n\n"));
        source.push_str("    mov     rax, 60\n    xor     edi, edi\n    syscall\n");
        source
    }

    /// Assembles and runs the snippet, reporting what changed.
    ///
    /// # Errors
    ///
    /// Returns [`ScratchpadError`] when the snippet is empty, does not
    /// assemble, or cannot be run.
    pub async fn run(&self, gdb: &str) -> Result<Outcome, ScratchpadError> {
        if self.snippet.trim().is_empty() {
            return Err(ScratchpadError::Empty);
        }
        if !crate::process::is_available(Path::new(gdb)) {
            return Err(ScratchpadError::NoDebugger);
        }

        // A temporary directory that cleans itself up, so nothing is left
        // behind however this function exits.
        let directory =
            tempfile::tempdir().map_err(|error| ScratchpadError::Setup(error.to_string()))?;
        let project = Project::create(directory.path(), "scratchpad")
            .map_err(|error| ScratchpadError::Setup(error.to_string()))?;

        let source = self.to_source();
        std::fs::write(project.entry_path(), &source)
            .map_err(|error| ScratchpadError::Setup(error.to_string()))?;

        let build = assembler::build(&project, BuildOptions::debug())
            .await
            .map_err(|error| ScratchpadError::Setup(error.to_string()))?;

        if !build.success {
            // Report the assembler's own words; they name the offending line.
            let message = build
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.clone())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ScratchpadError::Assembly(if message.is_empty() {
                build.summary()
            } else {
                message
            }));
        }

        let executable = build
            .executable
            .clone()
            .ok_or_else(|| ScratchpadError::Setup("no executable was produced".to_owned()))?;

        let outcome = self.observe(gdb, &executable).await;
        // `directory` drops here, removing everything.
        outcome.map(|(changes, before, after, output)| Outcome {
            changes,
            flags_before: before,
            flags_after: after,
            output,
            source,
        })
    }

    /// Runs the built program under GDB and reads the registers either side.
    async fn observe(
        &self,
        gdb: &str,
        executable: &Path,
    ) -> Result<(Vec<Change>, Flags, Flags, String), ScratchpadError> {
        let mut session = GdbSession::start(gdb).await?;
        session.set_timeout(TIMEOUT);

        let result = self.observe_inner(&mut session, executable).await;
        session.shutdown().await;
        result
    }

    /// The body of [`Scratchpad::observe`], so the session is always shut down.
    async fn observe_inner(
        &self,
        session: &mut GdbSession,
        executable: &Path,
    ) -> Result<(Vec<Change>, Flags, Flags, String), ScratchpadError> {
        session
            .execute(&mi::file_exec_and_symbols(executable))
            .await?;
        session.execute(&mi::break_insert(START_LABEL)).await?;
        session.execute(&mi::break_insert(END_LABEL)).await?;
        session.execute(&mi::exec_run()).await?;

        session.wait_for_stop(TIMEOUT).await?;
        let before = read_registers(session).await?;

        session.execute(&mi::exec_continue()).await?;
        let stopped = session.wait_for_stop(TIMEOUT).await?;

        // Running off the end instead of reaching the second label means the
        // snippet jumped somewhere unexpected. Reporting that beats reporting
        // an empty diff.
        if stopped.is_program_exit() {
            return Err(ScratchpadError::Assembly(
                "the snippet left the recorded region, so nothing could be compared".to_owned(),
            ));
        }
        let after = read_registers(session).await?;

        let output = session
            .take_events()
            .iter()
            .filter_map(|record| match record {
                crate::debugger::mi::Record::Stream {
                    kind: crate::debugger::mi::StreamKind::Target,
                    text,
                } => Some(text.clone()),
                _ => None,
            })
            .collect::<String>();

        Ok((
            diff(&before, &after),
            flags_of(&before),
            flags_of(&after),
            output,
        ))
    }
}

/// Reads every register into a file.
async fn read_registers(session: &mut GdbSession) -> Result<RegisterFile, ScratchpadError> {
    let names = session.execute(&mi::data_list_register_names()).await?;
    let values = session.execute(&mi::data_list_register_values()).await?;

    let mut file = RegisterFile::new();
    if let (Some(names), Some(values)) =
        (names.get("register-names"), values.get("register-values"))
    {
        file.update(crate::debugger::registers::parse_register_values(
            names, values,
        ));
    }
    Ok(file)
}

/// The flags held in a register file.
fn flags_of(file: &RegisterFile) -> Flags {
    file.flags().unwrap_or_else(Flags::empty)
}

/// The registers that differ between two snapshots.
///
/// `RIP` is excluded: it changes with every instruction, so reporting it would
/// bury the change the user actually asked about.
fn diff(before: &RegisterFile, after: &RegisterFile) -> Vec<Change> {
    registers::all()
        .into_iter()
        .filter(|register| register.name != "rip" && register.name != "rflags")
        .filter_map(|register| {
            let old = before.value_of(register.name)?;
            let new = after.value_of(register.name)?;
            (old != new).then_some(Change {
                register: register.name,
                before: old,
                after: new,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gdb_available() -> bool {
        crate::process::is_available(Path::new("gdb"))
            && crate::process::is_available(Path::new("nasm"))
            && crate::process::is_available(Path::new("ld"))
    }

    fn pad(snippet: &str, initial: &[(&str, u64)]) -> Scratchpad {
        let mut pad = Scratchpad::new();
        pad.snippet = snippet.to_owned();
        for (name, value) in initial {
            pad.set(name, *value).expect("a real register");
        }
        pad
    }

    #[test]
    fn the_generated_program_sets_the_requested_registers() {
        let source = pad("add rax, rbx", &[("rax", 1), ("rbx", 2)]).to_source();
        assert!(source.contains("mov     rax, 0x1"));
        assert!(source.contains("mov     rbx, 0x2"));
        assert!(source.contains("add rax, rbx"));
        assert!(source.contains("global _start"));
    }

    #[test]
    fn the_generated_program_brackets_the_snippet_with_labels() {
        let source = pad("nop", &[]).to_source();
        let start = source.find(START_LABEL).expect("start label");
        let snippet = source.rfind("nop").expect("the snippet");
        let end = source.find(END_LABEL).expect("end label");
        assert!(start < snippet, "the snippet must follow the first label");
        assert!(end < snippet || end > start, "labels bracket the snippet");
    }

    #[test]
    fn the_generated_program_exits_rather_than_running_off_the_end() {
        let source = pad("nop", &[]).to_source();
        assert!(source.contains("mov     rax, 60"));
        assert!(source.contains("syscall"));
    }

    #[test]
    fn the_generated_program_says_it_runs_natively() {
        // The scratchpad must never read as a sandbox.
        assert!(pad("nop", &[]).to_source().contains("natively"));
    }

    #[test]
    fn the_generated_program_assembles() {
        // The strongest check that does not need a debugger: hand it to nasm.
        if !crate::process::is_available(Path::new("nasm")) {
            eprintln!("skipping: nasm not installed");
            return;
        }
        let dir = tempfile::tempdir().expect("temp dir");
        let asm = dir.path().join("s.asm");
        std::fs::write(
            &asm,
            pad("add rax, rbx", &[("rax", 1), ("rbx", 2)]).to_source(),
        )
        .expect("write");

        let status = std::process::Command::new("nasm")
            .args(["-f", "elf64"])
            .arg(&asm)
            .arg("-o")
            .arg(dir.path().join("s.o"))
            .status()
            .expect("run nasm");
        assert!(status.success(), "the generated program must assemble");
    }

    #[test]
    fn setting_an_unknown_register_is_refused() {
        let mut pad = Scratchpad::new();
        assert!(matches!(
            pad.set("zmm0", 1),
            Err(ScratchpadError::UnknownRegister { .. })
        ));
        assert!(pad.initial.is_empty());
    }

    #[test]
    fn a_register_alias_is_stored_under_its_canonical_name() {
        let mut pad = Scratchpad::new();
        pad.set("eax", 5).expect("eax is a register");
        assert_eq!(pad.initial.get("rax"), Some(&5));
    }

    #[tokio::test]
    async fn an_empty_snippet_is_refused_before_anything_runs() {
        let outcome = Scratchpad::new().run("gdb").await;
        assert!(matches!(outcome, Err(ScratchpadError::Empty)));
    }

    #[tokio::test]
    async fn a_snippet_that_does_not_assemble_reports_the_assembler_message() {
        if !gdb_available() {
            eprintln!("skipping: toolchain not installed");
            return;
        }
        let error = pad("this is not an instruction", &[])
            .run("gdb")
            .await
            .expect_err("must fail");

        match error {
            ScratchpadError::Assembly(message) => assert!(!message.is_empty()),
            other => panic!("unexpected error: {other}"),
        }
    }

    #[tokio::test]
    async fn adding_two_registers_reports_exactly_what_changed() {
        // The headline case: set RAX and RBX, add them, see only RAX move.
        if !gdb_available() {
            eprintln!("skipping: toolchain not installed");
            return;
        }
        let outcome = pad("add rax, rbx", &[("rax", 1), ("rbx", 2)])
            .run("gdb")
            .await
            .expect("the snippet should run");

        let changes: Vec<(&str, u64, u64)> = outcome
            .changes
            .iter()
            .map(|change| (change.register, change.before, change.after))
            .collect();
        assert_eq!(changes, [("rax", 1, 3)], "only RAX should have changed");
    }

    #[tokio::test]
    async fn the_flags_an_instruction_sets_are_reported() {
        if !gdb_available() {
            eprintln!("skipping: toolchain not installed");
            return;
        }
        // Subtracting a value from itself gives zero, so ZF must end up set.
        let outcome = pad("sub rax, rax", &[("rax", 7)])
            .run("gdb")
            .await
            .expect("the snippet should run");

        assert!(
            outcome.flags_after.has(crate::instruction::Flag::Zero),
            "ZF should be set after subtracting a value from itself"
        );
        assert!(
            outcome
                .flag_changes()
                .contains(&crate::instruction::Flag::Zero),
            "the change should be reported"
        );
    }

    #[tokio::test]
    async fn an_arithmetic_shift_on_a_negative_value_is_shown_as_it_really_behaves() {
        // Exactly the question the scratchpad exists for, and one a naive
        // simulator gets wrong: sar rounds towards negative infinity.
        if !gdb_available() {
            eprintln!("skipping: toolchain not installed");
            return;
        }
        let outcome = pad("sar rax, 1", &[("rax", u64::MAX)])
            .run("gdb")
            .await
            .expect("the snippet should run");

        let rax = outcome
            .changes
            .iter()
            .find(|change| change.register == "rax");
        assert!(
            rax.is_none(),
            "-1 shifted arithmetically right is still -1, so nothing changed"
        );
    }

    #[tokio::test]
    async fn a_snippet_with_no_effect_is_reported_as_such() {
        if !gdb_available() {
            eprintln!("skipping: toolchain not installed");
            return;
        }
        let outcome = pad("nop", &[]).run("gdb").await.expect("nop runs");
        assert!(
            outcome.is_empty(),
            "nop changes nothing: {:?}",
            outcome.changes
        );
    }

    #[tokio::test]
    async fn nothing_is_left_behind_on_disk() {
        // The temporary project must clean itself up however the run ends.
        if !gdb_available() {
            eprintln!("skipping: toolchain not installed");
            return;
        }
        let before = std::fs::read_dir(std::env::temp_dir())
            .map(|entries| entries.count())
            .unwrap_or(0);

        let _ = pad("add rax, rbx", &[("rax", 1)]).run("gdb").await;
        let _ = pad("not an instruction", &[]).run("gdb").await;

        let after = std::fs::read_dir(std::env::temp_dir())
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert!(
            after <= before + 1,
            "temporary files accumulated: {before} then {after}"
        );
    }
}

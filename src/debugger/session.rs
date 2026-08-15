//! Driving a GDB process over its machine interface.
//!
//! # Concurrency model
//!
//! One background task owns GDB's standard output, parses each line into a
//! [`Record`] and forwards it on a channel. The session itself is driven from
//! a single place and issues one command at a time, waiting for the result
//! record carrying its token. Records that arrive meanwhile — the program
//! stopping, GDB announcing a new thread, output from the program — are
//! queued rather than discarded, and the caller drains them afterwards.
//!
//! Serialising commands this way removes an entire category of bug. A debugger
//! UI is inherently request/response: the user presses "step", the view
//! updates. Allowing several commands in flight would buy nothing and require
//! correlating a shared pending-map from multiple tasks.
//!
//! # Cleanup
//!
//! GDB must never be left running when ratasm exits, or it holds the
//! program's process group and the terminal alongside it. Three mechanisms
//! cover that: [`GdbSession::shutdown`] for the orderly path, `kill_on_drop`
//! for the panic and cancellation paths, and a bounded wait so a GDB that
//! refuses to exit is killed rather than waited on forever.

use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command as TokioCommand};
use tokio::sync::mpsc;

use super::mi::command::{self, Command};
use super::mi::record::{parse_line, Record, ResultClass, StreamKind};

/// How long to wait for a reply before giving up on a command.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long to wait for GDB to exit before killing it.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

/// Errors from running a debug session.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// GDB could not be started.
    #[error("cannot start {program}: {source}")]
    Spawn {
        /// The debugger executable.
        program: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// GDB is not installed.
    #[error("{program} not found; install it to use the debugger")]
    NotInstalled {
        /// The debugger executable that was looked for.
        program: String,
    },
    /// Writing a command failed.
    #[error("cannot send command to the debugger: {source}")]
    Write {
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// GDB exited or closed its output.
    #[error("the debugger exited unexpectedly")]
    Terminated,
    /// GDB did not answer within the timeout.
    #[error("the debugger did not respond to '{command}' within {}s", timeout.as_secs())]
    Timeout {
        /// The command that went unanswered.
        command: String,
        /// How long was waited.
        timeout: Duration,
    },
    /// GDB reported an error for a command.
    #[error("{message}")]
    CommandFailed {
        /// The command that failed.
        command: String,
        /// GDB's own message.
        message: String,
    },
}

/// A running GDB process spoken to over the machine interface.
pub struct GdbSession {
    child: Child,
    stdin: ChildStdin,
    records: mpsc::UnboundedReceiver<Record>,
    /// Async records received while waiting for a command result.
    queued: VecDeque<Record>,
    next_token: u32,
    timeout: Duration,
}

impl GdbSession {
    /// Starts GDB and prepares it for the machine interface.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::NotInstalled`] when the debugger is not on
    /// `PATH`, or [`SessionError::Spawn`] if it cannot be launched.
    pub async fn start(program: &str) -> Result<Self, SessionError> {
        if !crate::process::is_available(Path::new(program)) {
            return Err(SessionError::NotInstalled {
                program: program.to_owned(),
            });
        }

        let mut child = TokioCommand::new(program)
            // mi3 is the current machine interface revision. --nx skips the
            // user's .gdbinit, which could otherwise print output that
            // desynchronises the protocol or change settings we depend on.
            .args(["--interpreter=mi3", "--nx", "-q"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|source| SessionError::Spawn {
                program: program.to_owned(),
                source,
            })?;

        let stdin = child.stdin.take().ok_or(SessionError::Terminated)?;
        let stdout = child.stdout.take().ok_or(SessionError::Terminated)?;

        let (sender, records) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                match parse_line(&line) {
                    Ok(Some(record)) => {
                        if sender.send(record).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        // A line we cannot parse is still worth showing; it is
                        // more likely to be unusual GDB output than a bug, and
                        // hiding it would leave the user with no explanation.
                        tracing::warn!(%error, line, "unparsable GDB output");
                        let fallback = Record::Stream {
                            kind: StreamKind::Log,
                            text: line,
                        };
                        if sender.send(fallback).is_err() {
                            break;
                        }
                    }
                }
            }
            // Dropping the sender closes the channel, which the session reads
            // as "GDB has gone".
        });

        let mut session = Self {
            child,
            stdin,
            records,
            queued: VecDeque::new(),
            next_token: 1,
            timeout: DEFAULT_TIMEOUT,
        };

        // Consume the banner and initial notifications so the first real
        // command starts from a clean queue.
        session.drain_ready();
        Ok(session)
    }

    /// Sets how long to wait for a command to be answered.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// The configured command timeout.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Sends a command and waits for its result record.
    ///
    /// Asynchronous records that arrive first are queued for
    /// [`GdbSession::take_events`].
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::CommandFailed`] when GDB answers with `^error`,
    /// [`SessionError::Timeout`] when it does not answer, or
    /// [`SessionError::Terminated`] when the process has gone.
    pub async fn execute(&mut self, command: &Command) -> Result<Record, SessionError> {
        let token = self.next_token;
        self.next_token = self.next_token.wrapping_add(1).max(1);

        let line = format!("{}\n", command.render(token));
        tracing::debug!(command = %command, token, "sending MI command");

        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|source| SessionError::Write { source })?;
        self.stdin
            .flush()
            .await
            .map_err(|source| SessionError::Write { source })?;

        let deadline = tokio::time::Instant::now() + self.timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(SessionError::Timeout {
                    command: command.to_string(),
                    timeout: self.timeout,
                });
            }

            let received = tokio::time::timeout(remaining, self.records.recv()).await;
            let record = match received {
                Ok(Some(record)) => record,
                // The channel closed: the reader task saw end of file.
                Ok(None) => return Err(SessionError::Terminated),
                Err(_) => {
                    return Err(SessionError::Timeout {
                        command: command.to_string(),
                        timeout: self.timeout,
                    })
                }
            };

            match &record {
                Record::Result { token: got, .. } if *got == Some(token) => {
                    if let Some(message) = record.error_message() {
                        return Err(SessionError::CommandFailed {
                            command: command.to_string(),
                            message,
                        });
                    }
                    return Ok(record);
                }
                // A result for a different token cannot happen while commands
                // are serialised, but queueing it is safer than asserting.
                _ => self.queued.push_back(record),
            }
        }
    }

    /// Sends a command, returning `Ok` even when GDB reports an error.
    ///
    /// Used for operations whose failure is informative rather than fatal,
    /// such as removing a breakpoint that GDB has already discarded.
    ///
    /// # Errors
    ///
    /// Returns an error only for transport failures, not for `^error`.
    pub async fn try_execute(&mut self, command: &Command) -> Result<Record, SessionError> {
        match self.execute(command).await {
            Err(SessionError::CommandFailed { message, .. }) => {
                tracing::debug!(%message, "GDB reported an error, continuing");
                Ok(Record::Result {
                    token: None,
                    class: ResultClass::Error,
                    results: Vec::new(),
                })
            }
            other => other,
        }
    }

    /// Waits for the program to stop, returning the `*stopped` record.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::Timeout`] if no stop arrives in time, or
    /// [`SessionError::Terminated`] if GDB exits first.
    pub async fn wait_for_stop(&mut self, timeout: Duration) -> Result<Record, SessionError> {
        // A stop may already be queued from an earlier command.
        if let Some(index) = self.queued.iter().position(Record::is_stopped) {
            if let Some(record) = self.queued.remove(index) {
                return Ok(record);
            }
        }

        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(SessionError::Timeout {
                    command: "waiting for the program to stop".to_owned(),
                    timeout,
                });
            }

            match tokio::time::timeout(remaining, self.records.recv()).await {
                Ok(Some(record)) if record.is_stopped() => return Ok(record),
                Ok(Some(record)) => self.queued.push_back(record),
                Ok(None) => return Err(SessionError::Terminated),
                Err(_) => {
                    return Err(SessionError::Timeout {
                        command: "waiting for the program to stop".to_owned(),
                        timeout,
                    })
                }
            }
        }
    }

    /// Moves every record that has already arrived into the queue.
    ///
    /// Never blocks, so it is safe to call from the render loop.
    pub fn drain_ready(&mut self) {
        while let Ok(record) = self.records.try_recv() {
            self.queued.push_back(record);
        }
    }

    /// Takes the queued asynchronous records, leaving the queue empty.
    pub fn take_events(&mut self) -> Vec<Record> {
        self.drain_ready();
        self.queued.drain(..).collect()
    }

    /// The queued records without removing them.
    pub fn peek_events(&self) -> &VecDeque<Record> {
        &self.queued
    }

    /// Whether the GDB process is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Ends the session, killing GDB if it does not exit promptly.
    ///
    /// Errors are deliberately swallowed: shutdown runs on paths where there
    /// is nothing useful to do with a failure, and the process is killed
    /// regardless.
    pub async fn shutdown(mut self) {
        // Ask politely first so GDB can detach from the program cleanly.
        let exit = command::gdb_exit();
        let line = format!("{}\n", exit.render(0));
        let _ = self.stdin.write_all(line.as_bytes()).await;
        let _ = self.stdin.flush().await;

        match tokio::time::timeout(SHUTDOWN_GRACE, self.child.wait()).await {
            Ok(Ok(_)) => {}
            _ => {
                // Kill and reap: without the wait the process becomes a zombie.
                let _ = self.child.start_kill();
                let _ = self.child.wait().await;
            }
        }
    }
}

impl std::fmt::Debug for GdbSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GdbSession")
            .field("next_token", &self.next_token)
            .field("queued", &self.queued.len())
            .field("timeout", &self.timeout)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gdb_available() -> bool {
        crate::process::is_available(Path::new("gdb"))
    }

    /// Builds a small program to debug, or returns `None` when the toolchain
    /// is unavailable.
    async fn built_program() -> Option<(tempfile::TempDir, std::path::PathBuf)> {
        use crate::assembler::{build, BuildOptions};
        use crate::project::Project;

        if !crate::process::is_available(Path::new("nasm"))
            || !crate::process::is_available(Path::new("ld"))
        {
            return None;
        }

        let dir = tempfile::tempdir().ok()?;
        let project = Project::create(dir.path(), "debug-test").ok()?;
        std::fs::write(
            project.entry_path(),
            "\
section .text
    global _start
_start:
    mov rax, 1
    mov rbx, 2
    add rax, rbx
    mov rax, 60
    xor edi, edi
    syscall
",
        )
        .ok()?;

        let outcome = build(&project, BuildOptions::debug()).await.ok()?;
        let executable = outcome.executable.clone()?;
        Some((dir, executable))
    }

    #[tokio::test]
    async fn a_missing_debugger_is_reported_clearly() {
        let error = GdbSession::start("definitely-not-gdb-xyz")
            .await
            .expect_err("must fail");
        assert!(matches!(error, SessionError::NotInstalled { .. }));
        assert!(error.to_string().contains("install it"));
    }

    #[tokio::test]
    async fn a_session_starts_and_shuts_down_cleanly() {
        if !gdb_available() {
            eprintln!("skipping: gdb not installed");
            return;
        }
        let mut session = GdbSession::start("gdb").await.expect("start gdb");
        assert!(session.is_alive());
        session.shutdown().await;
    }

    #[tokio::test]
    async fn a_simple_command_gets_a_result_record() {
        if !gdb_available() {
            eprintln!("skipping: gdb not installed");
            return;
        }
        let mut session = GdbSession::start("gdb").await.expect("start gdb");
        let record = session
            .execute(&command::set_disassembly_flavor(true))
            .await
            .expect("command must succeed");
        assert!(record.is_result());
        session.shutdown().await;
    }

    #[tokio::test]
    async fn an_invalid_command_reports_gdbs_own_message() {
        if !gdb_available() {
            eprintln!("skipping: gdb not installed");
            return;
        }
        let mut session = GdbSession::start("gdb").await.expect("start gdb");
        let error = session
            .execute(&Command::new("-data-evaluate-expression").arg("no_such_symbol_xyz"))
            .await
            .expect_err("must fail");

        match error {
            SessionError::CommandFailed { message, .. } => {
                assert!(!message.is_empty(), "GDB's message must be preserved");
            }
            other => panic!("unexpected error: {other}"),
        }
        session.shutdown().await;
    }

    #[tokio::test]
    async fn try_execute_tolerates_a_command_error() {
        if !gdb_available() {
            eprintln!("skipping: gdb not installed");
            return;
        }
        let mut session = GdbSession::start("gdb").await.expect("start gdb");
        let record = session
            .try_execute(&command::break_delete(999))
            .await
            .expect("must not propagate the error");
        assert!(record.is_result());
        session.shutdown().await;
    }

    #[tokio::test]
    async fn a_program_can_be_loaded_and_run_to_its_entry_point() {
        if !gdb_available() {
            eprintln!("skipping: gdb not installed");
            return;
        }
        let Some((_dir, executable)) = built_program().await else {
            eprintln!("skipping: nasm or ld not installed");
            return;
        };

        let mut session = GdbSession::start("gdb").await.expect("start gdb");
        session
            .execute(&command::file_exec_and_symbols(&executable))
            .await
            .expect("load the program");
        session
            .execute(&command::break_insert("_start"))
            .await
            .expect("set a breakpoint");
        session
            .execute(&command::exec_run())
            .await
            .expect("run the program");

        let stopped = session
            .wait_for_stop(Duration::from_secs(10))
            .await
            .expect("the program must stop");
        assert_eq!(stopped.get_str("reason"), Some("breakpoint-hit"));

        session.shutdown().await;
    }

    #[tokio::test]
    async fn registers_can_be_read_at_a_breakpoint() {
        if !gdb_available() {
            eprintln!("skipping: gdb not installed");
            return;
        }
        let Some((_dir, executable)) = built_program().await else {
            eprintln!("skipping: nasm or ld not installed");
            return;
        };

        let mut session = GdbSession::start("gdb").await.expect("start gdb");
        session
            .execute(&command::file_exec_and_symbols(&executable))
            .await
            .expect("load");
        session
            .execute(&command::break_insert("_start"))
            .await
            .expect("breakpoint");
        session.execute(&command::exec_run()).await.expect("run");
        session
            .wait_for_stop(Duration::from_secs(10))
            .await
            .expect("stop");

        let names = session
            .execute(&command::data_list_register_names())
            .await
            .expect("register names");
        let names = names.get("register-names").expect("names present");
        let names = names.as_list().expect("a list");
        assert!(
            names.iter().any(|value| value.as_str() == Some("rax")),
            "rax must be among the register names"
        );

        let values = session
            .execute(&command::data_list_register_values())
            .await
            .expect("register values");
        let values = values
            .get("register-values")
            .and_then(|value| value.as_list())
            .expect("values present");
        assert!(!values.is_empty());

        session.shutdown().await;
    }

    #[tokio::test]
    async fn stepping_advances_the_program_counter() {
        if !gdb_available() {
            eprintln!("skipping: gdb not installed");
            return;
        }
        let Some((_dir, executable)) = built_program().await else {
            eprintln!("skipping: nasm or ld not installed");
            return;
        };

        let mut session = GdbSession::start("gdb").await.expect("start gdb");
        session
            .execute(&command::file_exec_and_symbols(&executable))
            .await
            .expect("load");
        session
            .execute(&command::break_insert("_start"))
            .await
            .expect("breakpoint");
        session.execute(&command::exec_run()).await.expect("run");
        let first = session
            .wait_for_stop(Duration::from_secs(10))
            .await
            .expect("stop");
        let first_address = first
            .get("frame")
            .and_then(|frame| frame.get_address("addr"))
            .expect("an address");

        session
            .execute(&command::exec_step_instruction())
            .await
            .expect("step");
        let second = session
            .wait_for_stop(Duration::from_secs(10))
            .await
            .expect("stop again");
        let second_address = second
            .get("frame")
            .and_then(|frame| frame.get_address("addr"))
            .expect("an address");

        assert!(
            second_address > first_address,
            "stepping must advance RIP: {first_address:#x} -> {second_address:#x}"
        );

        session.shutdown().await;
    }

    #[tokio::test]
    async fn memory_can_be_read_at_a_breakpoint() {
        if !gdb_available() {
            eprintln!("skipping: gdb not installed");
            return;
        }
        let Some((_dir, executable)) = built_program().await else {
            eprintln!("skipping: nasm or ld not installed");
            return;
        };

        let mut session = GdbSession::start("gdb").await.expect("start gdb");
        session
            .execute(&command::file_exec_and_symbols(&executable))
            .await
            .expect("load");
        session
            .execute(&command::break_insert("_start"))
            .await
            .expect("breakpoint");
        session.execute(&command::exec_run()).await.expect("run");
        let stopped = session
            .wait_for_stop(Duration::from_secs(10))
            .await
            .expect("stop");
        let address = stopped
            .get("frame")
            .and_then(|frame| frame.get_address("addr"))
            .expect("an address");

        let memory = session
            .execute(&command::data_read_memory_bytes(address, 16))
            .await
            .expect("read memory");
        assert!(memory.get("memory").is_some(), "memory payload expected");

        session.shutdown().await;
    }

    #[tokio::test]
    async fn the_program_running_to_completion_is_reported_as_an_exit() {
        if !gdb_available() {
            eprintln!("skipping: gdb not installed");
            return;
        }
        let Some((_dir, executable)) = built_program().await else {
            eprintln!("skipping: nasm or ld not installed");
            return;
        };

        let mut session = GdbSession::start("gdb").await.expect("start gdb");
        session
            .execute(&command::file_exec_and_symbols(&executable))
            .await
            .expect("load");
        session.execute(&command::exec_run()).await.expect("run");

        // With no breakpoint the program runs to completion. GDB reports that
        // as a *stopped record whose reason is an exit, not as a class of its
        // own, so wait_for_stop is the right thing to await.
        let stopped = session
            .wait_for_stop(Duration::from_secs(10))
            .await
            .expect("the exit must be reported");
        assert!(
            stopped.is_program_exit(),
            "expected an exit, got {:?}",
            stopped.stop_reason()
        );
        assert_eq!(stopped.exit_code(), Some(0));

        session.shutdown().await;
    }

    #[tokio::test]
    async fn a_dead_debugger_is_detected_rather_than_hanging() {
        if !gdb_available() {
            eprintln!("skipping: gdb not installed");
            return;
        }
        let mut session = GdbSession::start("gdb").await.expect("start gdb");
        // Asking GDB to exit closes its output; the next command must report
        // termination promptly instead of waiting out the timeout.
        let _ = session.execute(&command::gdb_exit()).await;

        let started = std::time::Instant::now();
        let result = session.execute(&command::break_list()).await;
        assert!(result.is_err(), "a command to a dead debugger must fail");
        assert!(
            started.elapsed() < DEFAULT_TIMEOUT,
            "must not wait for the full timeout"
        );
    }

    #[tokio::test]
    async fn a_short_timeout_is_reported_as_a_timeout() {
        if !gdb_available() {
            eprintln!("skipping: gdb not installed");
            return;
        }
        let Some((_dir, executable)) = built_program().await else {
            eprintln!("skipping: nasm or ld not installed");
            return;
        };

        let mut session = GdbSession::start("gdb").await.expect("start gdb");
        session
            .execute(&command::file_exec_and_symbols(&executable))
            .await
            .expect("load");

        // Nothing is running, so no stop can ever arrive.
        let error = session
            .wait_for_stop(Duration::from_millis(150))
            .await
            .expect_err("must time out");
        assert!(matches!(error, SessionError::Timeout { .. }));

        session.shutdown().await;
    }

    #[tokio::test]
    async fn tokens_increase_so_replies_can_be_matched() {
        if !gdb_available() {
            eprintln!("skipping: gdb not installed");
            return;
        }
        let mut session = GdbSession::start("gdb").await.expect("start gdb");
        let first = session.next_token;
        session
            .execute(&command::break_list())
            .await
            .expect("first command");
        let second = session.next_token;
        session
            .execute(&command::break_list())
            .await
            .expect("second command");
        assert!(second > first);
        assert!(session.next_token > second);
        session.shutdown().await;
    }
}

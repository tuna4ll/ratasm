//! Running external programs safely.
//!
//! Every external tool — the assembler, the linker, the user's own program,
//! GDB — is launched through this module. Four properties matter and each is
//! easy to get wrong:
//!
//! **No shell.** Arguments are passed as a vector to `execve`, never
//! interpolated into a command string. A project whose path contains a space,
//! a quote or a `;` is then merely a path, not an injection.
//!
//! **No deadlock.** `stdout` and `stderr` are drained concurrently by separate
//! tasks. Waiting for exit while a pipe buffer fills is the classic hang: the
//! child blocks writing, the parent blocks waiting, and neither moves again.
//!
//! **No zombies.** The child is always reaped. On timeout it is killed and
//! then waited on; `kill_on_drop` covers the paths where the future is
//! cancelled instead.
//!
//! **No lock-up.** A timeout bounds every run, so a program that never exits
//! cannot take the editor down with it.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Errors from launching or supervising a process.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    /// The program could not be found or executed.
    #[error("cannot run '{program}': {source}")]
    Spawn {
        /// The program that failed to start.
        program: String,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The working directory does not exist.
    #[error("working directory {path} does not exist")]
    MissingWorkingDirectory {
        /// The directory that was requested.
        path: PathBuf,
    },
    /// Reading the child's output failed.
    #[error("cannot read output of '{program}': {source}")]
    Io {
        /// The program whose output could not be read.
        program: String,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

/// How a process finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Exited normally with this status code.
    Exited(i32),
    /// Killed by this signal.
    ///
    /// The common cases in an assembly debugger are `SIGSEGV` (11) from a bad
    /// memory access and `SIGFPE` (8) from a division overflow, both of which
    /// are the *interesting* result rather than an error in the tool.
    Signalled(i32),
    /// Killed because it exceeded its timeout.
    TimedOut,
}

impl Outcome {
    /// Whether the process completed successfully.
    pub fn is_success(self) -> bool {
        matches!(self, Outcome::Exited(0))
    }

    /// The exit code, when the process exited normally.
    pub fn exit_code(self) -> Option<i32> {
        match self {
            Outcome::Exited(code) => Some(code),
            _ => None,
        }
    }

    /// The conventional name of the signal that killed the process.
    pub fn signal_name(self) -> Option<&'static str> {
        match self {
            Outcome::Signalled(signal) => Some(signal_name(signal)),
            _ => None,
        }
    }

    /// A human-readable description of how the process ended.
    pub fn description(self) -> String {
        match self {
            Outcome::Exited(0) => "exited successfully".to_owned(),
            Outcome::Exited(code) => format!("exited with code {code}"),
            Outcome::Signalled(signal) => {
                format!("killed by signal {signal} ({})", signal_name(signal))
            }
            Outcome::TimedOut => "timed out".to_owned(),
        }
    }
}

/// The conventional name for a Unix signal number.
pub fn signal_name(signal: i32) -> &'static str {
    match signal {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => "unknown",
    }
}

/// A program to run, with its arguments and environment.
///
/// Built with the builder methods rather than a constructor taking every
/// field, so a caller cannot accidentally swap two same-typed parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// The program to execute.
    pub program: PathBuf,
    /// Arguments, each passed separately and never shell-interpreted.
    pub args: Vec<String>,
    /// Directory to run in.
    pub working_directory: Option<PathBuf>,
    /// Maximum run time before the process is killed.
    pub timeout: Option<Duration>,
    /// Text written to the child's standard input.
    pub stdin: Option<String>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
}

impl CommandSpec {
    /// Creates a spec for `program` with no arguments.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            working_directory: None,
            timeout: None,
            stdin: None,
            env: Vec::new(),
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

    /// Sets the working directory.
    pub fn working_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(path.into());
        self
    }

    /// Sets the timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Supplies text on standard input.
    pub fn stdin(mut self, text: impl Into<String>) -> Self {
        self.stdin = Some(text.into());
        self
    }

    /// Adds an environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// A display string for logs and the build panel.
    ///
    /// For display only. It is never parsed back or handed to a shell, so
    /// quoting here is cosmetic rather than a security boundary.
    pub fn display(&self) -> String {
        let mut parts = vec![self.program.display().to_string()];
        parts.extend(self.args.iter().map(|arg| {
            if arg.contains(' ') {
                format!("\"{arg}\"")
            } else {
                arg.clone()
            }
        }));
        parts.join(" ")
    }
}

/// What a finished process produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    /// How the process ended.
    pub outcome: Outcome,
    /// Everything written to standard output.
    pub stdout: String,
    /// Everything written to standard error.
    pub stderr: String,
    /// How long the process ran.
    pub duration: Duration,
    /// The command that was run, for display.
    pub command: String,
}

impl ProcessOutput {
    /// Whether the process succeeded.
    pub fn is_success(&self) -> bool {
        self.outcome.is_success()
    }

    /// Standard output and standard error combined, in that order.
    pub fn combined(&self) -> String {
        let mut combined = self.stdout.clone();
        if !self.stderr.is_empty() {
            if !combined.is_empty() && !combined.ends_with('\n') {
                combined.push('\n');
            }
            combined.push_str(&self.stderr);
        }
        combined
    }
}

/// Runs a command to completion, honouring its timeout.
///
/// # Errors
///
/// Returns [`ProcessError::Spawn`] if the program cannot be started, or
/// [`ProcessError::MissingWorkingDirectory`] if the requested directory does
/// not exist. A program that runs and fails is *not* an error: it returns
/// `Ok` with a non-zero [`Outcome`], because a failed build is a normal
/// result that the caller wants to display rather than a fault to propagate.
pub async fn run(spec: &CommandSpec) -> Result<ProcessOutput, ProcessError> {
    let started = Instant::now();
    let program = spec.program.display().to_string();

    if let Some(directory) = &spec.working_directory {
        if !directory.is_dir() {
            return Err(ProcessError::MissingWorkingDirectory {
                path: directory.clone(),
            });
        }
    }

    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // If this future is dropped, the child must not outlive it.
        .kill_on_drop(true);

    if let Some(directory) = &spec.working_directory {
        command.current_dir(directory);
    }
    for (key, value) in &spec.env {
        command.env(key, value);
    }

    let mut child = command.spawn().map_err(|source| ProcessError::Spawn {
        program: program.clone(),
        source,
    })?;

    if let (Some(text), Some(mut pipe)) = (spec.stdin.as_ref(), child.stdin.take()) {
        use tokio::io::AsyncWriteExt;
        // A child that exits without reading stdin gives a broken pipe; that
        // is the child's prerogative, not an error in the runner.
        let _ = pipe.write_all(text.as_bytes()).await;
        let _ = pipe.shutdown().await;
    }

    // Drain both pipes concurrently. Waiting for exit first would deadlock as
    // soon as the child writes more than a pipe buffer holds.
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let stdout_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        if let Some(pipe) = stdout_pipe.as_mut() {
            let _ = pipe.read_to_end(&mut buffer).await;
        }
        buffer
    });
    let stderr_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        if let Some(pipe) = stderr_pipe.as_mut() {
            let _ = pipe.read_to_end(&mut buffer).await;
        }
        buffer
    });

    let (outcome, status) = match spec.timeout {
        Some(limit) => match tokio::time::timeout(limit, child.wait()).await {
            Ok(status) => (None, status),
            Err(_) => {
                // Kill, then wait: without the wait the child stays a zombie.
                let _ = child.start_kill();
                let status = child.wait().await;
                (Some(Outcome::TimedOut), status)
            }
        },
        None => (None, child.wait().await),
    };

    let status = status.map_err(|source| ProcessError::Io {
        program: program.clone(),
        source,
    })?;

    let outcome = outcome.unwrap_or_else(|| outcome_from_status(&status));
    let stdout = stdout_task.await.unwrap_or_default();
    let stderr = stderr_task.await.unwrap_or_default();

    Ok(ProcessOutput {
        outcome,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        duration: started.elapsed(),
        command: spec.display(),
    })
}

/// Converts an exit status into an [`Outcome`].
fn outcome_from_status(status: &std::process::ExitStatus) -> Outcome {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return Outcome::Signalled(signal);
        }
    }
    Outcome::Exited(status.code().unwrap_or(-1))
}

/// Whether `program` can be found and executed.
///
/// Used to report a missing `nasm` or `gdb` as a clear message at the point
/// the user asks for it, rather than as a spawn failure mid-build.
pub fn is_available(program: &Path) -> bool {
    if program.components().count() > 1 {
        return program.is_file();
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| directory.join(program).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program that is present on any Linux system running these tests.
    const TRUE: &str = "/usr/bin/true";
    const FALSE: &str = "/usr/bin/false";

    fn exists(path: &str) -> bool {
        Path::new(path).is_file()
    }

    #[tokio::test]
    async fn a_successful_command_reports_success() {
        if !exists(TRUE) {
            return;
        }
        let output = run(&CommandSpec::new(TRUE)).await.expect("run");
        assert_eq!(output.outcome, Outcome::Exited(0));
        assert!(output.is_success());
    }

    #[tokio::test]
    async fn a_failing_command_is_a_result_not_an_error() {
        // A non-zero exit is normal output from a build, not a fault.
        if !exists(FALSE) {
            return;
        }
        let output = run(&CommandSpec::new(FALSE)).await.expect("run");
        assert_eq!(output.outcome, Outcome::Exited(1));
        assert!(!output.is_success());
    }

    #[tokio::test]
    async fn standard_output_is_captured() {
        let spec = CommandSpec::new("/usr/bin/printf").arg("hello world");
        if !exists("/usr/bin/printf") {
            return;
        }
        let output = run(&spec).await.expect("run");
        assert_eq!(output.stdout, "hello world");
        assert!(output.stderr.is_empty());
    }

    #[tokio::test]
    async fn arguments_are_passed_without_shell_interpretation() {
        // The security property: shell metacharacters stay literal data.
        if !exists("/usr/bin/printf") {
            return;
        }
        let hostile = "%s\n";
        let spec = CommandSpec::new("/usr/bin/printf")
            .arg(hostile)
            .arg("; rm -rf / && echo pwned `id` $(whoami)");
        let output = run(&spec).await.expect("run");
        assert_eq!(output.stdout, "; rm -rf / && echo pwned `id` $(whoami)\n");
    }

    #[tokio::test]
    async fn a_missing_program_reports_a_spawn_error() {
        let spec = CommandSpec::new("/nonexistent/definitely-not-here");
        let error = run(&spec).await.expect_err("must fail");
        assert!(matches!(error, ProcessError::Spawn { .. }));
        assert!(error.to_string().contains("definitely-not-here"));
    }

    #[tokio::test]
    async fn a_missing_working_directory_is_reported_before_spawning() {
        if !exists(TRUE) {
            return;
        }
        let spec = CommandSpec::new(TRUE).working_directory("/nonexistent/directory");
        let error = run(&spec).await.expect_err("must fail");
        assert!(matches!(
            error,
            ProcessError::MissingWorkingDirectory { .. }
        ));
    }

    #[tokio::test]
    async fn a_runaway_process_is_killed_at_its_timeout() {
        if !exists("/usr/bin/sleep") {
            return;
        }
        let spec = CommandSpec::new("/usr/bin/sleep")
            .arg("30")
            .timeout(Duration::from_millis(150));
        let started = Instant::now();
        let output = run(&spec).await.expect("run");
        assert_eq!(output.outcome, Outcome::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the timeout must actually stop the process"
        );
    }

    #[tokio::test]
    async fn a_process_finishing_inside_its_timeout_is_not_killed() {
        if !exists(TRUE) {
            return;
        }
        let spec = CommandSpec::new(TRUE).timeout(Duration::from_secs(30));
        let output = run(&spec).await.expect("run");
        assert_eq!(output.outcome, Outcome::Exited(0));
    }

    #[tokio::test]
    async fn large_output_does_not_deadlock() {
        // Far more than one pipe buffer. A runner that waits for exit before
        // reading hangs here forever.
        if !exists("/usr/bin/yes") || !exists("/usr/bin/head") {
            return;
        }
        let spec = CommandSpec::new("/usr/bin/head")
            .args(["-c", "2000000", "/dev/zero"])
            .timeout(Duration::from_secs(20));
        let output = run(&spec).await.expect("run");
        assert_eq!(output.outcome, Outcome::Exited(0));
        assert_eq!(output.stdout.len(), 2_000_000);
    }

    #[tokio::test]
    async fn standard_input_is_delivered() {
        if !exists("/usr/bin/cat") {
            return;
        }
        let spec = CommandSpec::new("/usr/bin/cat").stdin("mov rax, 60\n");
        let output = run(&spec).await.expect("run");
        assert_eq!(output.stdout, "mov rax, 60\n");
    }

    #[tokio::test]
    async fn the_working_directory_is_honoured() {
        let dir = tempfile::tempdir().expect("temp dir");
        if !exists("/usr/bin/pwd") {
            return;
        }
        let spec = CommandSpec::new("/usr/bin/pwd").working_directory(dir.path());
        let output = run(&spec).await.expect("run");
        // The temporary directory may be reached through a symlink, so compare
        // the resolved forms rather than the literal strings.
        let reported = std::fs::canonicalize(output.stdout.trim()).expect("canonicalize");
        let expected = std::fs::canonicalize(dir.path()).expect("canonicalize");
        assert_eq!(reported, expected);
    }

    #[tokio::test]
    async fn environment_variables_are_passed_through() {
        if !exists("/usr/bin/env") {
            return;
        }
        let spec = CommandSpec::new("/usr/bin/env").env("RATASM_TEST_VALUE", "42");
        let output = run(&spec).await.expect("run");
        assert!(output.stdout.contains("RATASM_TEST_VALUE=42"));
    }

    #[tokio::test]
    async fn a_signalled_process_reports_its_signal() {
        if !exists("/usr/bin/sh") {
            return;
        }
        // `sh -c 'kill -SEGV $$'` is the least awkward way to produce a real
        // fatal signal; the argument is a fixed literal, not user input.
        let spec = CommandSpec::new("/usr/bin/sh").args(["-c", "kill -SEGV $$"]);
        let output = run(&spec).await.expect("run");
        assert_eq!(output.outcome, Outcome::Signalled(11));
        assert_eq!(output.outcome.signal_name(), Some("SIGSEGV"));
        assert!(output.outcome.description().contains("SIGSEGV"));
    }

    #[test]
    fn outcome_descriptions_are_readable() {
        assert_eq!(Outcome::Exited(0).description(), "exited successfully");
        assert_eq!(Outcome::Exited(2).description(), "exited with code 2");
        assert_eq!(Outcome::TimedOut.description(), "timed out");
        assert!(Outcome::Signalled(8).description().contains("SIGFPE"));
        assert_eq!(Outcome::Exited(3).exit_code(), Some(3));
        assert_eq!(Outcome::TimedOut.exit_code(), None);
    }

    #[test]
    fn the_display_string_is_readable_and_quotes_spaces() {
        let spec = CommandSpec::new("nasm")
            .args(["-f", "elf64"])
            .arg("my source.asm");
        assert_eq!(spec.display(), "nasm -f elf64 \"my source.asm\"");
    }

    #[test]
    fn availability_finds_programs_on_the_path() {
        assert!(is_available(Path::new("sh")) || is_available(Path::new("/bin/sh")));
        assert!(!is_available(Path::new(
            "definitely-not-a-real-program-xyz"
        )));
        assert!(!is_available(Path::new("/nonexistent/path/to/tool")));
    }

    #[test]
    fn combined_output_joins_both_streams() {
        let output = ProcessOutput {
            outcome: Outcome::Exited(1),
            stdout: "out".to_owned(),
            stderr: "err".to_owned(),
            duration: Duration::ZERO,
            command: String::new(),
        };
        assert_eq!(output.combined(), "out\nerr");
    }

    #[test]
    fn signal_names_cover_the_ones_a_debugger_sees() {
        assert_eq!(signal_name(11), "SIGSEGV");
        assert_eq!(signal_name(8), "SIGFPE");
        assert_eq!(signal_name(4), "SIGILL");
        assert_eq!(signal_name(5), "SIGTRAP");
        assert_eq!(signal_name(6), "SIGABRT");
        assert_eq!(signal_name(999), "unknown");
    }
}

//! Interactive process execution through a Linux pseudo-terminal.

use std::fs::File;
use std::future::pending;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt as _;
use std::process::Stdio;
use std::time::{Duration, Instant};

use nix::pty::{openpty, Winsize};
use nix::sys::signal::{killpg, Signal};
use nix::unistd::{setsid, ttyname, Pid};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use super::{outcome_from_status, CommandSpec, Outcome, ProcessError, ProcessOutput};

/// The drawable size of a pseudo-terminal, in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    /// Columns visible to the child.
    pub columns: u16,
    /// Rows visible to the child.
    pub rows: u16,
}

impl PtySize {
    /// Creates a non-empty terminal size.
    pub const fn new(columns: u16, rows: u16) -> Self {
        Self {
            columns: if columns == 0 { 1 } else { columns },
            rows: if rows == 0 { 1 } else { rows },
        }
    }

    fn winsize(self) -> Winsize {
        Winsize {
            ws_row: self.rows,
            ws_col: self.columns,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }
}

/// A request sent to an interactive program.
#[derive(Debug)]
pub enum PtyControl {
    /// Bytes typed or pasted by the user.
    Input(Vec<u8>),
    /// A new drawable size.
    Resize(PtySize),
    /// Stop the program and its process group.
    Stop,
}

/// An event produced by an interactive program.
#[derive(Debug)]
pub enum PtyEvent {
    /// Bytes read from the terminal as soon as they arrive.
    Output(Vec<u8>),
    /// The terminal has closed and the complete outcome is available.
    Finished(Result<ProcessOutput, ProcessError>),
    /// A debugger-owned terminal transport has flushed and closed.
    Closed,
}

/// One open pseudo-terminal before its slave has been assigned.
pub struct PtyPair {
    master: File,
    slave: OwnedFd,
}

impl PtyPair {
    /// Opens a master/slave pair at `size`.
    pub fn open(size: PtySize) -> Result<Self, ProcessError> {
        let pair = openpty(Some(&size.winsize()), None).map_err(|error| ProcessError::Pty {
            operation: "open a pseudo-terminal",
            source: io::Error::from_raw_os_error(error as i32),
        })?;
        Ok(Self {
            master: File::from(pair.master),
            slave: pair.slave,
        })
    }

    /// The filesystem path GDB can assign as an inferior terminal.
    pub fn slave_path(&self) -> Result<std::path::PathBuf, ProcessError> {
        ttyname(&self.slave).map_err(|error| ProcessError::Pty {
            operation: "find the pseudo-terminal slave",
            source: io::Error::from_raw_os_error(error as i32),
        })
    }

    /// Turns the pair into an endpoint for a debugger-managed inferior.
    pub fn into_endpoint(self) -> Result<(std::path::PathBuf, PtyEndpoint), ProcessError> {
        let path = self.slave_path()?;
        let reader = self
            .master
            .try_clone()
            .map_err(|source| ProcessError::Pty {
                operation: "duplicate the pseudo-terminal master",
                source,
            })?;
        let resizer = self
            .master
            .try_clone()
            .map_err(|source| ProcessError::Pty {
                operation: "duplicate the pseudo-terminal master",
                source,
            })?;
        Ok((
            path,
            PtyEndpoint {
                reader: tokio::fs::File::from_std(reader),
                writer: tokio::fs::File::from_std(self.master),
                resizer,
                slave: self.slave,
            },
        ))
    }

    /// Spawns `spec` with all three standard streams connected to the slave.
    fn spawn(self, spec: &CommandSpec) -> Result<PtyChild, ProcessError> {
        if let Some(directory) = &spec.working_directory {
            if !directory.is_dir() {
                return Err(ProcessError::MissingWorkingDirectory {
                    path: directory.clone(),
                });
            }
        }

        let program = spec.program.display().to_string();
        let stdin = self.slave.try_clone().map_err(|source| ProcessError::Pty {
            operation: "duplicate the pseudo-terminal slave",
            source,
        })?;
        let stdout = self.slave.try_clone().map_err(|source| ProcessError::Pty {
            operation: "duplicate the pseudo-terminal slave",
            source,
        })?;

        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(self.slave))
            .kill_on_drop(true);
        if let Some(directory) = &spec.working_directory {
            command.current_dir(directory);
        }
        for (key, value) in &spec.env {
            command.env(key, value);
        }

        // The standard streams have already been installed by the time this
        // callback runs. Starting a new session and claiming fd 0 as its
        // controlling terminal gives job control and terminal-generated
        // signals the same semantics as a shell-launched program.
        unsafe {
            command.as_std_mut().pre_exec(|| {
                setsid().map_err(errno_to_io)?;
                if nix::libc::ioctl(0, nix::libc::TIOCSCTTY, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let child = command
            .spawn()
            .map_err(|source| ProcessError::Spawn { program, source })?;
        let reader = self
            .master
            .try_clone()
            .map_err(|source| ProcessError::Pty {
                operation: "duplicate the pseudo-terminal master",
                source,
            })?;
        let resizer = self
            .master
            .try_clone()
            .map_err(|source| ProcessError::Pty {
                operation: "duplicate the pseudo-terminal master",
                source,
            })?;

        Ok(PtyChild {
            child,
            reader: tokio::fs::File::from_std(reader),
            writer: tokio::fs::File::from_std(self.master),
            resizer,
            started: Instant::now(),
            command: spec.display(),
            program: spec.program.display().to_string(),
        })
    }
}

/// The ratasm side of a PTY whose slave is opened by another process, such as GDB.
pub struct PtyEndpoint {
    reader: tokio::fs::File,
    writer: tokio::fs::File,
    resizer: File,
    // Keeping one slave descriptor open avoids a transient EIO before GDB
    // launches its inferior. It is dropped with the endpoint.
    slave: OwnedFd,
}

impl PtyEndpoint {
    /// Streams terminal bytes until the endpoint is stopped or disconnected.
    pub async fn run(
        self,
        mut controls: mpsc::UnboundedReceiver<PtyControl>,
        events: mpsc::UnboundedSender<PtyEvent>,
    ) {
        let Self {
            mut reader,
            mut writer,
            resizer,
            slave,
        } = self;
        let _keep_slave_open = slave;
        let mut buffer = vec![0; 8192];

        loop {
            tokio::select! {
                result = reader.read(&mut buffer) => match result {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let _ = events.send(PtyEvent::Output(buffer[..read].to_vec()));
                    }
                },
                control = controls.recv() => match control {
                    Some(PtyControl::Input(bytes)) => {
                        if writer.write_all(&bytes).await.is_err() {
                            break;
                        }
                        let _ = writer.flush().await;
                    }
                    Some(PtyControl::Resize(size)) => {
                        if resize(&resizer, size).is_err() {
                            break;
                        }
                    }
                    Some(PtyControl::Stop) | None => break,
                }
            }
        }

        // Output may become readable at the same instant as the stop request.
        // Give it a brief chance to reach the UI before declaring the endpoint closed.
        loop {
            let read =
                tokio::time::timeout(Duration::from_millis(10), reader.read(&mut buffer)).await;
            match read {
                Ok(Ok(count)) if count > 0 => {
                    let _ = events.send(PtyEvent::Output(buffer[..count].to_vec()));
                }
                _ => break,
            }
        }
        let _ = events.send(PtyEvent::Closed);
    }
}

struct PtyChild {
    child: Child,
    reader: tokio::fs::File,
    writer: tokio::fs::File,
    resizer: File,
    started: Instant,
    command: String,
    program: String,
}

impl PtyChild {
    async fn supervise(
        mut self,
        spec: CommandSpec,
        mut controls: mpsc::UnboundedReceiver<PtyControl>,
        events: mpsc::UnboundedSender<PtyEvent>,
    ) -> Result<ProcessOutput, ProcessError> {
        let output_events = events.clone();
        let mut reader = self.reader;
        let output_task = tokio::spawn(async move {
            let mut all = Vec::new();
            let mut buffer = vec![0; 8192];
            loop {
                match reader.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let chunk = buffer[..read].to_vec();
                        all.extend_from_slice(&chunk);
                        let _ = output_events.send(PtyEvent::Output(chunk));
                    }
                }
            }
            all
        });

        if let Some(initial) = &spec.stdin {
            let _ = self.writer.write_all(initial.as_bytes()).await;
        }

        let limit = async move {
            match spec.timeout {
                Some(duration) => tokio::time::sleep(duration).await,
                None => pending::<()>().await,
            }
        };
        tokio::pin!(limit);

        let mut forced = None;
        let mut controls_open = true;
        let status = loop {
            tokio::select! {
                status = self.child.wait() => break status,
                () = &mut limit => {
                    forced = Some(Outcome::TimedOut);
                    kill_process_group(&mut self.child);
                    break self.child.wait().await;
                }
                control = controls.recv(), if controls_open => match control {
                    Some(PtyControl::Input(bytes)) => {
                        let _ = self.writer.write_all(&bytes).await;
                        let _ = self.writer.flush().await;
                    }
                    Some(PtyControl::Resize(size)) => {
                        resize(&self.resizer, size)?;
                    }
                    Some(PtyControl::Stop) => {
                        forced = Some(Outcome::Stopped);
                        kill_process_group(&mut self.child);
                        break self.child.wait().await;
                    }
                    None => controls_open = false,
                }
            }
        }
        .map_err(|source| ProcessError::Io {
            program: self.program,
            source,
        })?;

        drop(self.writer);
        let output = match tokio::time::timeout(Duration::from_millis(250), output_task).await {
            Ok(Ok(bytes)) => bytes,
            _ => Vec::new(),
        };

        Ok(ProcessOutput {
            outcome: forced.unwrap_or_else(|| outcome_from_status(&status)),
            stdout: String::from_utf8_lossy(&output).into_owned(),
            stderr: String::new(),
            duration: self.started.elapsed(),
            command: self.command,
        })
    }
}

/// Runs `spec` on a PTY, streaming output and accepting input until it exits.
pub async fn run_interactive(
    spec: CommandSpec,
    size: PtySize,
    controls: mpsc::UnboundedReceiver<PtyControl>,
    events: mpsc::UnboundedSender<PtyEvent>,
) {
    let result = match PtyPair::open(size).and_then(|pair| pair.spawn(&spec)) {
        Ok(child) => child.supervise(spec, controls, events.clone()).await,
        Err(error) => Err(error),
    };
    let _ = events.send(PtyEvent::Finished(result));
}

fn resize(file: &File, size: PtySize) -> Result<(), ProcessError> {
    let winsize = size.winsize();
    let result = unsafe { nix::libc::ioctl(file.as_raw_fd(), nix::libc::TIOCSWINSZ, &winsize) };
    if result == -1 {
        return Err(ProcessError::Pty {
            operation: "resize the pseudo-terminal",
            source: io::Error::last_os_error(),
        });
    }
    Ok(())
}

fn kill_process_group(child: &mut Child) {
    if let Some(id) = child.id() {
        let _ = killpg(Pid::from_raw(id as i32), Signal::SIGKILL);
    }
    let _ = child.start_kill();
}

fn errno_to_io(error: nix::errno::Errno) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell() -> Option<&'static str> {
        ["/bin/sh", "/usr/bin/sh"]
            .into_iter()
            .find(|path| std::path::Path::new(path).is_file())
    }

    #[tokio::test]
    async fn a_program_can_read_and_answer_on_the_terminal() {
        let Some(shell) = shell() else { return };
        let spec = CommandSpec::new(shell)
            .arg("-c")
            .arg("printf 'Name: '; read name; printf 'Hello %s\\n' \"$name\"")
            .timeout(Duration::from_secs(2));
        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        tokio::spawn(run_interactive(
            spec,
            PtySize::new(80, 24),
            control_rx,
            event_tx,
        ));

        let mut seen = Vec::new();
        while let Some(event) = event_rx.recv().await {
            match event {
                PtyEvent::Output(bytes) => {
                    seen.extend_from_slice(&bytes);
                    if seen.windows(6).any(|window| window == b"Name: ") {
                        control_tx
                            .send(PtyControl::Input(b"Ada\r".to_vec()))
                            .expect("the program is running");
                    }
                }
                PtyEvent::Finished(result) => {
                    let output = result.expect("PTY run");
                    assert!(output.is_success(), "{:?}", output.outcome);
                    assert!(output.stdout.contains("Hello Ada"), "{}", output.stdout);
                    break;
                }
                PtyEvent::Closed => panic!("a child run is not an endpoint"),
            }
        }
    }

    #[tokio::test]
    async fn the_child_sees_a_terminal_and_its_size() {
        let Some(shell) = shell() else { return };
        let spec = CommandSpec::new(shell)
            .arg("-c")
            .arg("test -t 0 && stty size")
            .timeout(Duration::from_secs(2));
        let (_control_tx, control_rx) = mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        tokio::spawn(run_interactive(
            spec,
            PtySize::new(73, 19),
            control_rx,
            event_tx,
        ));

        while let Some(event) = event_rx.recv().await {
            match event {
                PtyEvent::Finished(result) => {
                    let output = result.expect("PTY run");
                    assert!(output.is_success(), "{:?}", output.outcome);
                    assert!(output.stdout.contains("19 73"), "{}", output.stdout);
                    break;
                }
                PtyEvent::Output(_) => {}
                PtyEvent::Closed => panic!("a child run is not an endpoint"),
            }
        }
    }
}

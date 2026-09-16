//! The loop that draws, reads input and performs effects.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt as _;

use crate::assembler::{self, BuildOptions};
use crate::command::Command;
use crate::debugger::mi::{command as mi, Record};
use crate::debugger::registers::parse_register_values;
use crate::debugger::session::GdbSession;
use crate::debugger::state::Transition;
use crate::disassembler;
use crate::editor::workspace;
use crate::process::pty::{PtyControl, PtyEvent, PtySize};
use crate::ui::clipboard;
use crate::ui::render;
use crate::ui::Tui;

use super::state::{Effect, StepKind};
use super::{App, Status};

/// How long to wait for the program to stop after a step.
const STOP_TIMEOUT: Duration = Duration::from_secs(15);

/// How often to redraw when nothing has happened.
const TICK: Duration = Duration::from_millis(250);

/// The result of work that ran off the event loop.
enum Background {
    /// The scratchpad snippet finished, or could not be run.
    Scratchpad(Result<crate::scratchpad::Outcome, String>),
}

/// Work in flight, so a second request can be refused and the first cancelled.
#[derive(Default)]
struct Running {
    /// The task, kept so stopping can abort it.
    handle: Option<tokio::task::JoinHandle<()>>,
    /// Commands for an interactive child, when the task owns a PTY.
    control: Option<tokio::sync::mpsc::UnboundedSender<PtyControl>>,
}

impl Running {
    /// Whether something is still running.
    fn is_busy(&self) -> bool {
        self.handle.as_ref().is_some_and(|task| !task.is_finished())
    }

    /// Aborts the task, if any.
    fn cancel(&mut self) -> bool {
        if let Some(control) = &self.control {
            if control.send(PtyControl::Stop).is_ok() {
                return true;
            }
        }
        match self.handle.take() {
            Some(task) if !task.is_finished() => {
                task.abort();
                true
            }
            _ => false,
        }
    }

    /// Sends bytes to the interactive child.
    fn input(&self, bytes: Vec<u8>) -> bool {
        self.control
            .as_ref()
            .is_some_and(|control| control.send(PtyControl::Input(bytes)).is_ok())
    }

    /// Resizes the interactive child's PTY.
    fn resize(&self, size: PtySize) {
        if let Some(control) = &self.control {
            let _ = control.send(PtyControl::Resize(size));
        }
    }
}

/// Runs the interface until the user quits.
pub async fn run(mut app: App, terminal: &mut Tui) -> Result<()> {
    let mut events = EventStream::new();
    let mut session: Option<GdbSession> = None;
    let (finished_tx, mut finished_rx) = tokio::sync::mpsc::unbounded_channel::<Background>();
    let (pty_tx, mut pty_rx) = tokio::sync::mpsc::unbounded_channel::<PtyEvent>();
    let mut running = Running::default();

    loop {
        let size = terminal.size().context("cannot read the terminal size")?;
        sync_terminal_size(&mut app, &running, size.width, size.height);
        render::sync_scroll(&mut app, size.width, size.height);
        terminal
            .draw(|frame| render::draw(frame, &app))
            .context("cannot draw to the terminal")?;

        if app.should_quit {
            break;
        }

        let effect = tokio::select! {
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) => handle_key_event(&mut app, &running, key),
                Some(Ok(Event::Paste(text))) => {
                    handle_terminal_paste(&app, &running, text);
                    Effect::None
                }
                Some(Ok(Event::Mouse(mouse))) => {
                    crate::event::handle_mouse(&mut app, mouse, size.width, size.height)
                }
                Some(Ok(Event::Resize(..))) => Effect::None,
                Some(Ok(_)) => Effect::None,
                Some(Err(_)) | None => break,
            },
            Some(done) = finished_rx.recv() => {
                running.handle = None;
                finish_background(&mut app, done);
                Effect::None
            },
            Some(event) = pty_rx.recv() => {
                finish_pty_event(&mut app, &mut running, event);
                Effect::None
            },
            () = tokio::time::sleep(TICK) => Effect::None,
        };

        perform(
            &mut app,
            &mut session,
            &finished_tx,
            &pty_tx,
            &mut running,
            (size.width, size.height),
            effect,
        )
        .await;
        drain_debugger_events(&mut app, &mut session).await;
    }

    running.cancel();

    if let Some(session) = session {
        session.shutdown().await;
    }
    Ok(())
}

/// Applies one key event.
fn handle_key_event(app: &mut App, running: &Running, key: KeyEvent) -> Effect {
    if app.terminal.is_some() && app.focus == super::Panel::Output && !app.mode.is_overlay() {
        if let Some(command) = app.keymap.command_for_event(key).cloned() {
            if terminal_reserved_command(&command) {
                return app.apply(&command);
            }
        }
        let application_cursor = app
            .terminal
            .as_ref()
            .is_some_and(|terminal| terminal.screen().application_cursor());
        if let Some(bytes) = terminal_key_bytes(key, application_cursor) {
            let _ = running.input(bytes);
        }
        return Effect::None;
    }
    crate::event::handle_key(app, key)
}

/// Commands that remain reachable while the Output panel sends ordinary keys to the child.
fn terminal_reserved_command(command: &Command) -> bool {
    matches!(
        command,
        Command::Stop
            | Command::Quit
            | Command::OpenPalette
            | Command::GoToPage(_)
            | Command::NextPage
            | Command::PreviousPage
            | Command::NextPanel
            | Command::PreviousPanel
            | Command::FocusPanel(_)
    )
}

/// Encodes a crossterm key as the byte sequence expected by a VT terminal.
fn terminal_key_bytes(key: KeyEvent, application_cursor: bool) -> Option<Vec<u8>> {
    if key.kind == KeyEventKind::Release {
        return None;
    }

    let sequence = match key.code {
        KeyCode::Char(character) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let upper = character.to_ascii_uppercase();
            let byte = match upper {
                '@' | ' ' => 0,
                'A'..='_' => (upper as u8) & 0x1f,
                '?' => 0x7f,
                _ => return None,
            };
            vec![byte]
        }
        KeyCode::Char(character) => {
            let mut bytes = Vec::new();
            if key.modifiers.contains(KeyModifiers::ALT) {
                bytes.push(0x1b);
            }
            let mut encoded = [0; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
            bytes
        }
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up if application_cursor => b"\x1bOA".to_vec(),
        KeyCode::Down if application_cursor => b"\x1bOB".to_vec(),
        KeyCode::Right if application_cursor => b"\x1bOC".to_vec(),
        KeyCode::Left if application_cursor => b"\x1bOD".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::F(number) => function_key(number)?.to_vec(),
        KeyCode::Null => vec![0],
        _ => return None,
    };
    Some(sequence)
}

fn function_key(number: u8) -> Option<&'static [u8]> {
    Some(match number {
        1 => b"\x1bOP",
        2 => b"\x1bOQ",
        3 => b"\x1bOR",
        4 => b"\x1bOS",
        5 => b"\x1b[15~",
        6 => b"\x1b[17~",
        7 => b"\x1b[18~",
        8 => b"\x1b[19~",
        9 => b"\x1b[20~",
        10 => b"\x1b[21~",
        11 => b"\x1b[23~",
        12 => b"\x1b[24~",
        _ => return None,
    })
}

fn handle_terminal_paste(app: &App, running: &Running, text: String) {
    if app.terminal.is_none() || app.focus != super::Panel::Output || app.mode.is_overlay() {
        return;
    }
    let bracketed = app
        .terminal
        .as_ref()
        .is_some_and(|terminal| terminal.screen().bracketed_paste());
    let bytes = if bracketed {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    };
    let _ = running.input(bytes);
}

/// Keeps both the parser and the kernel PTY aligned with the visible Output panel.
fn sync_terminal_size(app: &mut App, running: &Running, width: u16, height: u16) {
    let Some(size) = output_terminal_size(app, width, height) else {
        return;
    };
    let changed = app
        .terminal
        .as_ref()
        .is_some_and(|terminal| terminal.size() != size);
    if changed {
        if let Some(terminal) = &mut app.terminal {
            terminal.resize(size);
        }
        running.resize(size);
    }
}

fn output_terminal_size(app: &App, width: u16, height: u16) -> Option<PtySize> {
    let layout = crate::ui::layout::compute(
        ratatui::layout::Rect::new(0, 0, width, height),
        app.page,
        app.focus,
    );
    let area = layout.area_of(super::Panel::Output)?;
    Some(PtySize::new(
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    ))
}

/// Performs an effect, folding the result back into the application.
async fn perform(
    app: &mut App,
    session: &mut Option<GdbSession>,
    finished: &tokio::sync::mpsc::UnboundedSender<Background>,
    pty_events: &tokio::sync::mpsc::UnboundedSender<PtyEvent>,
    running: &mut Running,
    viewport: (u16, u16),
    effect: Effect,
) {
    match effect {
        Effect::None | Effect::Quit => {}

        Effect::Build { debug } => {
            build(app, debug).await;
        }

        Effect::Run => {
            if running.is_busy() {
                app.status = Status::warning("Something is already running");
            } else if build(app, false).await {
                start_program(app, pty_events, running, viewport.0, viewport.1);
            }
        }

        Effect::StopProgram => {
            if running.cancel() {
                app.status = Status::warning("Stopped");
            } else {
                app.status = Status::info("Nothing is running");
            }
        }

        Effect::SaveFile(path) => match app.workspace.save_active_as(&path) {
            Ok(path) => {
                app.status = Status::success(format!("Saved {}", path.display()));
                app.refresh_project_files();
            }
            Err(error) => app.status = Status::error(error.to_string()),
        },

        Effect::SaveAndClose(path) => match app.workspace.save_active_as(&path) {
            Ok(_) => {
                app.close_active_document();
                app.refresh_project_files();
            }
            Err(error) => app.status = Status::error(error.to_string()),
        },

        Effect::SaveProject => match app.project.save() {
            Ok(path) => {
                app.status = Status::success(format!(
                    "Added to {}; it is assembled with the project now",
                    crate::editor::workspace::display_path(&path)
                ));
                app.refresh_project_files();
            }
            Err(error) => app.status = Status::error(error.to_string()),
        },

        Effect::SaveAll => {
            app.status = match save_everything(app) {
                Ok(status) => status,
                Err(error) => Status::error(error.to_string()),
            };
        }

        Effect::OpenFile(path) => match app.workspace.open(&path) {
            Ok(_) => {
                app.workspace
                    .active_mut()
                    .set_indent_width(app.settings.indent_width());
                app.status = Status::success(format!("Opened {}", path.display()));
                app.focus = super::Panel::Editor;
                app.refresh_project_files();
            }
            Err(error) => app.status = Status::error(error.to_string()),
        },

        Effect::DebugStart => start_session(app, session).await,
        Effect::DebugStop => stop_session(app, session).await,

        Effect::DebugContinue => {
            resume(app, session, &mi::exec_continue(), "continuing").await;
        }
        Effect::Step(kind) => {
            let command = match kind {
                StepKind::Instruction => mi::exec_step_instruction(),
                StepKind::Over => mi::exec_next_instruction(),
                StepKind::Line => mi::exec_step(),
                StepKind::Out => mi::exec_finish(),
                StepKind::Back => mi::exec_step_instruction_reverse(),
                StepKind::BackOver => mi::exec_next_instruction_reverse(),
                StepKind::ReverseContinue => mi::exec_continue_reverse(),
            };
            resume(app, session, &command, "stepping").await;
        }

        Effect::DebugInterrupt => {
            let Some(active) = session.as_mut() else {
                return;
            };
            match active.execute(&mi::exec_interrupt()).await {
                Ok(_) => {
                    if let Ok(record) = active.wait_for_stop(STOP_TIMEOUT).await {
                        handle_stop(app, session, &record).await;
                    }
                }
                Err(error) => app.status = Status::error(error.to_string()),
            }
        }

        Effect::SyncBreakpoints => sync_breakpoints(app, session).await,

        Effect::ReadMemory(address) => read_memory(app, session, address).await,

        Effect::RunScratchpad => {
            if running.is_busy() {
                app.status = Status::warning("Something is already running");
            } else {
                start_snippet(app, finished, running);
            }
        }

        Effect::SetSystemClipboard(text) => match clipboard::set_sequence(&text) {
            Some(sequence) => {
                use std::io::Write as _;
                let mut out = std::io::stdout();
                if write!(out, "{sequence}")
                    .and_then(|()| out.flush())
                    .is_err()
                {
                    tracing::debug!("could not write the clipboard sequence");
                }
            }
            None => {
                app.status = Status::warning(format!(
                    "Copied within ratasm; too large for the terminal clipboard (over {} KiB)",
                    clipboard::MAX_BYTES / 1024
                ));
            }
        },
    }
}

/// Builds the project, reporting the outcome.
fn save_everything(app: &mut App) -> Result<Status, crate::editor::workspace::FileError> {
    let (written, skipped) = app.workspace.save_all()?;
    Ok(match (written, skipped) {
        (0, 0) => Status::info("Nothing to save"),
        (_, 0) => Status::success(format!("Saved {written} file(s)")),
        (_, _) => Status::warning(format!(
            "Saved {written} file(s); {skipped} buffer(s) have no file name yet"
        )),
    })
}

async fn build(app: &mut App, debug: bool) -> bool {
    if let Err(error) = app.workspace.save_all() {
        app.fail_build(error.to_string());
        return false;
    }

    let options = if debug {
        BuildOptions::debug()
    } else {
        BuildOptions::release()
    };

    if app.debugger.state().can_build() {
        let _ = app.debugger.apply(Transition::BuildStarted);
    }

    match assembler::build(&app.project, options).await {
        Ok(outcome) => {
            let executable = outcome.executable.clone();
            app.finish_build(outcome);
            if let Some(path) = &executable {
                load_static_disassembly(app, path);
            }
            executable.is_some()
        }
        Err(error) => {
            app.fail_build(error.to_string());
            false
        }
    }
}

/// Starts the built program on its own task.
fn start_program(
    app: &mut App,
    events: &tokio::sync::mpsc::UnboundedSender<PtyEvent>,
    running: &mut Running,
    width: u16,
    height: u16,
) {
    let Some(executable) = app
        .build
        .as_ref()
        .and_then(|outcome| outcome.executable.clone())
    else {
        return;
    };

    app.focus_panel(super::Panel::Output);
    let size = output_terminal_size(app, width, height).unwrap_or(PtySize::new(80, 24));
    app.terminal = Some(super::terminal::TerminalScreen::new(size));
    app.output.clear();
    app.status = Status::info("Running… Type in Output; Ctrl+F5 stops");
    let spec = assembler::run_command(&app.project, &executable);
    let (control_tx, control_rx) = tokio::sync::mpsc::unbounded_channel();
    let events = events.clone();
    running.control = Some(control_tx);
    running.handle = Some(tokio::spawn(async move {
        crate::process::pty::run_interactive(spec, size, control_rx, events).await;
    }));
}

/// Starts the scratchpad snippet on its own task.
fn start_snippet(
    app: &mut App,
    finished: &tokio::sync::mpsc::UnboundedSender<Background>,
    running: &mut Running,
) {
    app.status = Status::info("Running the snippet…");
    let gdb = app.settings.debugger.gdb.clone();
    let scratchpad = app.scratchpad.clone();
    let finished = finished.clone();
    running.handle = Some(tokio::spawn(async move {
        let result = scratchpad
            .run(&gdb)
            .await
            .map_err(|error| error.to_string());
        let _ = finished.send(Background::Scratchpad(result));
    }));
}

/// Folds the result of a background task back into the application.
fn finish_background(app: &mut App, done: Background) {
    match done {
        Background::Scratchpad(Ok(outcome)) => {
            app.status = if outcome.is_empty() {
                Status::info("The snippet changed nothing")
            } else {
                Status::success(format!("{} register(s) changed", outcome.changes.len()))
            };
            app.scratchpad_result = Some(Ok(outcome));
        }
        Background::Scratchpad(Err(error)) => {
            app.status = Status::error(error.clone());
            app.scratchpad_result = Some(Err(error));
        }
    }
}

fn finish_pty_event(app: &mut App, running: &mut Running, event: PtyEvent) {
    match event {
        PtyEvent::Output(bytes) => {
            if let Some(terminal) = &mut app.terminal {
                terminal.process(&bytes);
            }
        }
        PtyEvent::Finished(result) => {
            running.handle = None;
            running.control = None;
            match result {
                Ok(output) => app.finish_run(output),
                Err(error) => {
                    app.terminal = None;
                    app.status = Status::error(error.to_string());
                }
            }
        }
    }
}

/// Disassembles the built executable so the panel has something to show before a session starts.
fn load_static_disassembly(app: &mut App, executable: &Path) {
    let Ok(image) = disassembler::ElfImage::load(executable) else {
        return;
    };
    let Some(text) = image.text_section() else {
        return;
    };

    let decoded = disassembler::decode(&text.data, text.address, app.disassembly_syntax);
    app.disassembly = disassembler::annotate_with_symbols(decoded, &image.symbols);
    if app.current_address.is_none() {
        app.current_address = Some(image.entry);
    }
}

/// Starts a debug session.
async fn start_session(app: &mut App, session: &mut Option<GdbSession>) {
    if !build(app, true).await {
        return;
    }
    let Some(executable) = app
        .build
        .as_ref()
        .and_then(|outcome| outcome.executable.clone())
    else {
        return;
    };

    if let Some(existing) = session.take() {
        existing.shutdown().await;
    }
    let _ = app.debugger.apply(Transition::LaunchRequested);
    app.status = Status::info("Starting the debugger…");

    let mut active = match GdbSession::start(&app.settings.debugger.gdb).await {
        Ok(active) => active,
        Err(error) => {
            let _ = app
                .debugger
                .fail(Transition::LaunchFailed, error.to_string());
            app.status = Status::error(error.to_string());
            return;
        }
    };
    active.set_timeout(app.settings.debugger_timeout());

    let setup = async {
        active
            .execute(&mi::file_exec_and_symbols(&executable))
            .await?;
        active
            .execute(&mi::environment_cd(&app.project.run_directory()))
            .await?;
        active
            .execute(&mi::set_disassembly_flavor(
                app.disassembly_syntax == disassembler::Syntax::Intel,
            ))
            .await?;
        if !app.project.config().run.args.is_empty() {
            active
                .execute(&mi::exec_arguments(app.project.config().run.args.clone()))
                .await?;
        }
        Ok::<(), crate::debugger::SessionError>(())
    };

    if let Err(error) = setup.await {
        let _ = app
            .debugger
            .fail(Transition::LaunchFailed, error.to_string());
        app.status = Status::error(error.to_string());
        active.shutdown().await;
        return;
    }

    app.breakpoints.detach();
    let mut placed = Vec::new();
    for location in app.pending_breakpoints() {
        match active
            .execute(&mi::break_insert(&location.to_gdb_location()))
            .await
        {
            Ok(record) => {
                if let Some(bkpt) = record.get("bkpt") {
                    placed.push((location, bkpt.clone()));
                }
            }
            Err(error) => {
                tracing::warn!(%error, "breakpoint rejected");
            }
        }
    }
    for (location, record) in placed {
        app.breakpoints.adopt(&location, &record);
    }

    match disassembler::ElfImage::load(&executable) {
        Ok(image) => {
            let _ = active
                .try_execute(&mi::break_insert_temporary_at_address(image.entry))
                .await;
        }
        Err(_) => {
            let _ = active
                .try_execute(&mi::break_insert_temporary("_start"))
                .await;
        }
    }

    if let Err(error) = active.execute(&mi::exec_run()).await {
        let _ = app
            .debugger
            .fail(Transition::LaunchFailed, error.to_string());
        app.status = Status::error(error.to_string());
        active.shutdown().await;
        return;
    }
    let _ = app.debugger.apply(Transition::LaunchSucceeded);

    match active.wait_for_stop(STOP_TIMEOUT).await {
        Ok(record) => {
            if app.settings.debugger.record {
                let recorded = active.execute(&mi::record_full()).await.is_ok();
                if !recorded {
                    tracing::warn!("execution recording unavailable");
                }
                app.recording = recorded;
            } else {
                app.recording = false;
            }

            *session = Some(active);
            handle_stop(app, session, &record).await;
        }
        Err(error) => {
            let _ = app.debugger.apply(Transition::SessionFailed);
            app.status = Status::error(error.to_string());
            active.shutdown().await;
        }
    }
}

/// Ends the session and clears the state it produced.
async fn stop_session(app: &mut App, session: &mut Option<GdbSession>) {
    if let Some(active) = session.take() {
        active.shutdown().await;
    }
    let _ = app.debugger.exited(None);
    let _ = app.debugger.apply(Transition::Reset);

    app.recording = false;
    app.registers.clear();
    app.breakpoints.detach();
    app.frames.clear();
    app.stack = Default::default();
    app.memory = Default::default();
    app.current_address = None;
    app.current_line = None;
    app.status = Status::info("Debug session ended");
}

/// Resumes the program and waits for it to stop again.
async fn resume(
    app: &mut App,
    session: &mut Option<GdbSession>,
    command: &crate::debugger::mi::Command,
    what: &str,
) {
    let Some(active) = session.as_mut() else {
        app.status = Status::warning("No debug session");
        return;
    };

    if let Err(error) = active.execute(command).await {
        app.status = Status::error(error.to_string());
        return;
    }
    let _ = app.debugger.apply(Transition::Resumed);

    match active.wait_for_stop(STOP_TIMEOUT).await {
        Ok(record) => handle_stop(app, session, &record).await,
        Err(error) => {
            app.status = Status::error(format!("{what}: {error}"));
            let _ = app.debugger.apply(Transition::SessionFailed);
        }
    }
}

/// Records a stop and refreshes everything that depends on it.
async fn handle_stop(app: &mut App, session: &mut Option<GdbSession>, record: &Record) {
    if record.is_program_exit() {
        let code = record.exit_code();
        let _ = app.debugger.exited(code);
        app.registers.clear();
        app.frames.clear();
        app.current_address = None;
        app.current_line = None;

        app.status = match code {
            Some(0) => Status::success("Program exited normally"),
            Some(code) => Status::warning(format!("Program exited with status {code}")),
            None => Status::warning("Program exited"),
        };

        if let Some(active) = session.take() {
            active.shutdown().await;
        }
        return;
    }

    let _ = app.debugger.apply(Transition::Stopped);

    if let Some(frame) = record.get("frame") {
        app.current_address = frame.get_address("addr");
        if let (Some(file), Some(line)) = (frame.get_str("file"), frame.get_int("line")) {
            if let Ok(line) = usize::try_from(line) {
                app.current_line = Some((std::path::PathBuf::from(file), line));
            }
        }
    }

    app.show_execution();

    if let Some(number) = record.get("bkptno").and_then(|value| value.as_str()) {
        if let Ok(number) = number.parse::<u32>() {
            app.breakpoints.record_hit(number);
        }
    }

    app.status = match record.stop_reason() {
        Some("breakpoint-hit") => Status::success("Stopped at a breakpoint"),
        Some("signal-received") => {
            let signal = record.get_str("signal-name").unwrap_or("a signal");
            match app.current_address {
                Some(address) => Status::error(format!("{signal} at 0x{address:x}")),
                None => Status::error(format!("Received {signal}")),
            }
        }
        Some("end-stepping-range") | Some("function-finished") => Status::info("Stepped"),
        Some(reason) => Status::info(format!("Stopped: {reason}")),
        None => Status::info("Stopped"),
    };

    refresh_state(app, session).await;
}

/// Reads registers, the stack and the disassembly after a stop.
async fn refresh_state(app: &mut App, session: &mut Option<GdbSession>) {
    let Some(active) = session.as_mut() else {
        return;
    };

    let names = active.execute(&mi::data_list_register_names()).await;
    let values = active.execute(&mi::data_list_register_values()).await;

    if let (Ok(names), Ok(values)) = (names, values) {
        if let (Some(names), Some(values)) =
            (names.get("register-names"), values.get("register-values"))
        {
            app.registers.update(parse_register_values(names, values));
        }
    }

    match active.execute(&mi::stack_list_frames()).await {
        Ok(record) => {
            app.frames = record
                .get("stack")
                .map(crate::debugger::frames::parse_frames)
                .unwrap_or_default();
            app.frame_selected = 0;
        }
        Err(_) => app.frames.clear(),
    }

    if let Some(rsp) = app.registers.rsp() {
        let depth = app.settings.stack_depth() * 8;
        if let Ok(record) = active
            .execute(&mi::data_read_memory_bytes(rsp, depth))
            .await
        {
            if let Some(block) = record
                .get("memory")
                .and_then(crate::debugger::memory::parse_memory_reply)
            {
                app.stack = block;
            }
        }
    }

    if let Some(rip) = app.registers.rip() {
        let window = 128u64;
        if let Ok(record) = active
            .execute(&mi::data_read_memory_bytes(rip, window as usize))
            .await
        {
            if let Some(memory) = record.get("memory") {
                if let Some(block) = crate::debugger::memory::parse_memory_reply(memory) {
                    let decoded =
                        disassembler::decode(&block.bytes, block.address, app.disassembly_syntax);
                    app.disassembly = decoded
                        .into_iter()
                        .map(disassembler::DisassemblyLine::bare)
                        .collect();
                }
            }
        }
    }

    if let Some(address) = app.memory_address {
        read_memory(app, session, address).await;
    }
}

/// Reads memory into the memory panel.
async fn read_memory(app: &mut App, session: &mut Option<GdbSession>, address: u64) {
    let Some(active) = session.as_mut() else {
        app.status = Status::warning("Start a debug session to read memory");
        return;
    };

    let count = app.settings.memory_window();
    match active
        .execute(&mi::data_read_memory_bytes(address, count))
        .await
    {
        Ok(record) => match record
            .get("memory")
            .and_then(crate::debugger::memory::parse_memory_reply)
        {
            Some(block) => {
                app.memory = block;
                app.memory_address = Some(address);
            }
            None => app.status = Status::error(format!("Cannot read memory at 0x{address:x}")),
        },
        Err(error) => app.status = Status::error(error.to_string()),
    }
}

/// Sends breakpoint changes to a running session.
async fn sync_breakpoints(app: &mut App, session: &mut Option<GdbSession>) {
    let Some(active) = session.as_mut() else {
        return;
    };

    let mut placed = Vec::new();
    for location in app.pending_breakpoints() {
        if let Ok(record) = active
            .execute(&mi::break_insert(&location.to_gdb_location()))
            .await
        {
            if let Some(bkpt) = record.get("bkpt") {
                placed.push((location, bkpt.clone()));
            }
        }
    }
    for (location, record) in placed {
        app.breakpoints.adopt(&location, &record);
    }
}

/// Notices asynchronous records that arrived while nothing was being asked.
async fn drain_debugger_events(app: &mut App, session: &mut Option<GdbSession>) {
    let Some(active) = session.as_mut() else {
        return;
    };

    if !active.is_alive() {
        app.debugger
            .force_failed("the debugger exited unexpectedly");
        app.status = Status::error("The debugger exited unexpectedly");
        *session = None;
        return;
    }

    let events = active.take_events();
    let stop = events.into_iter().find(Record::is_stopped);
    if let Some(record) = stop {
        handle_stop(app, session, &record).await;
    }
}

/// Loads the file the user asked for on the command line.
pub fn open_initial_file(app: &mut App, path: &Path) {
    if !path.is_file() {
        return;
    }
    match app.workspace.open(path) {
        Ok(_) => {
            app.workspace
                .active_mut()
                .set_indent_width(app.settings.indent_width());
        }
        Err(error) => app.status = Status::error(error.to_string()),
    }
}

/// Loads the project's entry file, so the editor is not empty on start-up.
pub fn open_project_entry(app: &mut App) {
    let entry = app.project.entry_path();
    if entry.is_file() {
        open_initial_file(app, &entry);
    } else {
        app.status = Status::warning(format!(
            "{} does not exist yet",
            workspace::display_path(&entry)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::project::Project;

    fn app_for(dir: &Path) -> App {
        let project = Project::create(dir, "runtest").expect("create");
        App::new(project, Settings::default()).expect("databases load")
    }

    #[test]
    fn the_project_entry_is_opened_at_start_up() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());

        open_project_entry(&mut app);
        assert_eq!(app.workspace.active().display_name(), "main.asm");
        assert!(app
            .workspace
            .active()
            .buffer()
            .to_text()
            .contains("global _start"));
    }

    #[test]
    fn a_missing_entry_file_is_reported_rather_than_silently_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        std::fs::remove_file(app.project.entry_path()).expect("remove");

        open_project_entry(&mut app);
        assert_eq!(app.status.severity, crate::app::Severity::Warning);
    }

    #[test]
    fn terminal_keys_use_vt_sequences_and_control_bytes() {
        let key = |code, modifiers| KeyEvent::new(code, modifiers);
        assert_eq!(
            terminal_key_bytes(key(KeyCode::Char('c'), KeyModifiers::CONTROL), false),
            Some(vec![3])
        );
        assert_eq!(
            terminal_key_bytes(key(KeyCode::Up, KeyModifiers::NONE), false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(key(KeyCode::Up, KeyModifiers::NONE), true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(key(KeyCode::Char('ş'), KeyModifiers::ALT), false),
            Some("\x1bş".as_bytes().to_vec())
        );
    }

    #[test]
    fn release_events_are_not_sent_to_the_child() {
        let mut key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        key.kind = KeyEventKind::Release;
        assert_eq!(terminal_key_bytes(key, false), None);
    }

    /// Performs one effect the way the run loop does, then waits for the work it started.
    async fn settle(app: &mut App, session: &mut Option<GdbSession>, effect: Effect) {
        let (finished, mut incoming) = tokio::sync::mpsc::unbounded_channel();
        let (pty_events, mut pty_incoming) = tokio::sync::mpsc::unbounded_channel();
        let mut running = Running::default();
        perform(
            app,
            session,
            &finished,
            &pty_events,
            &mut running,
            (160, 48),
            effect,
        )
        .await;
        while running.handle.is_some() {
            tokio::select! {
                Some(done) = incoming.recv() => {
                    running.handle = None;
                    finish_background(app, done);
                }
                Some(event) = pty_incoming.recv() => {
                    finish_pty_event(app, &mut running, event);
                }
            }
        }
    }

    #[tokio::test]
    async fn building_writes_every_edited_buffer_first() {
        if !crate::process::is_available(Path::new("nasm"))
            || !crate::process::is_available(Path::new("ld"))
        {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        let entry = app.project.entry_path();

        open_initial_file(&mut app, &entry);
        app.workspace.active_mut().insert("; edited\n");
        assert!(app.workspace.has_unsaved_changes());

        assert!(build(&mut app, false).await, "the build should succeed");
        assert!(
            !app.workspace.has_unsaved_changes(),
            "an unsaved buffer would have been assembled from its old contents"
        );
        assert!(std::fs::read_to_string(&entry)
            .expect("read")
            .contains("; edited"));
    }

    #[tokio::test]
    async fn saving_everything_reports_what_it_wrote() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        let mut session = None;

        settle(&mut app, &mut session, Effect::SaveAll).await;
        assert_eq!(app.status.severity, crate::app::Severity::Info);

        let entry = app.project.entry_path();
        open_initial_file(&mut app, &entry);
        app.workspace.active_mut().insert("x");
        settle(&mut app, &mut session, Effect::SaveAll).await;
        assert_eq!(app.status.severity, crate::app::Severity::Success);
        assert!(!app.workspace.has_unsaved_changes());
    }

    #[tokio::test]
    async fn building_a_working_project_produces_disassembly() {
        if !crate::process::is_available(Path::new("nasm"))
            || !crate::process::is_available(Path::new("ld"))
        {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());

        assert!(build(&mut app, false).await, "the build should succeed");
        assert_eq!(app.status.severity, crate::app::Severity::Success);
        assert!(
            !app.disassembly.is_empty(),
            "the executable should have been disassembled"
        );
        assert!(app.current_address.is_some(), "the entry point is known");
    }

    #[tokio::test]
    async fn a_broken_project_reports_the_error_and_produces_nothing() {
        if !crate::process::is_available(Path::new("nasm")) {
            eprintln!("skipping: nasm not installed");
            return;
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        std::fs::write(app.project.entry_path(), "    this is not assembly\n").expect("write");

        assert!(!build(&mut app, false).await);
        assert_eq!(app.status.severity, crate::app::Severity::Error);
        assert!(app.disassembly.is_empty());
        assert!(!app.diagnostics.is_empty(), "the error should be recorded");
    }

    #[tokio::test]
    async fn a_program_that_runs_reports_its_output() {
        if !crate::process::is_available(Path::new("nasm"))
            || !crate::process::is_available(Path::new("ld"))
        {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());

        settle(&mut app, &mut None, Effect::Run).await;

        assert!(
            app.output.iter().any(|line| line.contains("Hello, world!")),
            "program output missing: {:?}",
            app.output
        );
        assert_eq!(app.status.severity, crate::app::Severity::Success);
    }

    #[tokio::test]
    async fn reading_memory_without_a_session_says_so() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        let mut session = None;

        read_memory(&mut app, &mut session, 0x4000).await;
        assert_eq!(app.status.severity, crate::app::Severity::Warning);
        assert!(app.memory.is_empty());
    }

    #[tokio::test]
    async fn effects_without_a_session_do_not_panic() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        let mut session = None;

        for effect in [
            Effect::None,
            Effect::DebugContinue,
            Effect::Step(StepKind::Instruction),
            Effect::DebugInterrupt,
            Effect::DebugStop,
            Effect::SyncBreakpoints,
            Effect::ReadMemory(0x1000),
            Effect::StopProgram,
        ] {
            settle(&mut app, &mut session, effect).await;
        }
    }

    #[tokio::test]
    async fn a_running_program_can_be_stopped() {
        if !crate::process::is_available(Path::new("nasm"))
            || !crate::process::is_available(Path::new("ld"))
        {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        std::fs::write(
            dir.path().join("src").join("main.asm"),
            "section .text\n    global _start\n_start:\n    jmp _start\n",
        )
        .expect("write the source");

        let (finished, _incoming) = tokio::sync::mpsc::unbounded_channel();
        let (pty_events, mut pty_incoming) = tokio::sync::mpsc::unbounded_channel();
        let mut running = Running::default();
        let mut session = None;

        perform(
            &mut app,
            &mut session,
            &finished,
            &pty_events,
            &mut running,
            (160, 48),
            Effect::Run,
        )
        .await;
        assert!(running.is_busy(), "the program should still be looping");

        perform(
            &mut app,
            &mut session,
            &finished,
            &pty_events,
            &mut running,
            (160, 48),
            Effect::StopProgram,
        )
        .await;
        assert_eq!(app.status.severity, crate::app::Severity::Warning);
        while running.handle.is_some() {
            let event = tokio::time::timeout(Duration::from_secs(2), pty_incoming.recv())
                .await
                .expect("the stopped process should finish")
                .expect("PTY event channel");
            finish_pty_event(&mut app, &mut running, event);
        }
        assert!(!running.is_busy());
    }

    #[tokio::test]
    async fn stopping_with_nothing_running_says_so() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        let mut session = None;

        settle(&mut app, &mut session, Effect::StopProgram).await;
        assert_eq!(app.status.severity, crate::app::Severity::Info);
    }

    #[tokio::test]
    async fn saving_through_an_effect_writes_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        let mut session = None;

        app.workspace.active_mut().insert("ret\n");
        let path = dir.path().join("written.asm");
        settle(&mut app, &mut session, Effect::SaveFile(path.clone())).await;

        assert_eq!(std::fs::read_to_string(&path).expect("read"), "ret\n");
        assert_eq!(app.status.severity, crate::app::Severity::Success);
    }

    #[tokio::test]
    async fn opening_a_missing_file_reports_the_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        let mut session = None;

        settle(
            &mut app,
            &mut session,
            Effect::OpenFile(dir.path().join("absent.asm")),
        )
        .await;
        assert_eq!(app.status.severity, crate::app::Severity::Error);
    }

    #[tokio::test]
    async fn the_explanation_gains_real_values_once_the_program_stops() {
        for tool in ["nasm", "ld", "gdb"] {
            if !crate::process::is_available(Path::new(tool)) {
                eprintln!("skipping: {tool} not installed");
                return;
            }
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        std::fs::write(
            app.project.entry_path(),
            "section .text\n    global _start\n_start:\n    mov rax, 1\n    mov rbx, 2\n    \
             add rax, rbx\n    mov rax, 60\n    xor edi, edi\n    syscall\n",
        )
        .expect("write");
        open_project_entry(&mut app);

        let mut session = None;
        start_session(&mut app, &mut session).await;
        assert!(session.is_some(), "session: {}", app.status.text);

        settle(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;
        settle(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;

        assert_eq!(app.registers.value_of("rax"), Some(1));
        assert_eq!(app.registers.value_of("rbx"), Some(2));

        let explanation = app.current_explanation().expect("an explanation");
        assert_eq!(explanation.mnemonic, "add");
        assert_eq!(
            explanation.concrete.as_deref(),
            Some("0x1 ← 0x1 + 0x2"),
            "the explainer should be reading live registers"
        );

        settle(&mut app, &mut session, Effect::DebugStop).await;
    }

    #[tokio::test]
    async fn stepping_backwards_restores_the_previous_register_values() {
        for tool in ["nasm", "ld", "gdb"] {
            if !crate::process::is_available(Path::new(tool)) {
                eprintln!("skipping: {tool} not installed");
                return;
            }
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        std::fs::write(
            app.project.entry_path(),
            "section .text\n    global _start\n_start:\n    mov rax, 1\n    mov rax, 2\n    \
             mov rax, 60\n    xor edi, edi\n    syscall\n",
        )
        .expect("write");

        let mut session = None;
        start_session(&mut app, &mut session).await;
        assert!(session.is_some(), "session: {}", app.status.text);

        settle(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;
        assert_eq!(
            app.registers.value_of("rax"),
            Some(1),
            "after the first mov"
        );

        settle(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;
        assert_eq!(app.registers.value_of("rax"), Some(2), "after the second");

        settle(&mut app, &mut session, Effect::Step(StepKind::Back)).await;
        assert_eq!(
            app.registers.value_of("rax"),
            Some(1),
            "stepping back must undo the write, not just move RIP: {}",
            app.status.text
        );

        settle(&mut app, &mut session, Effect::DebugStop).await;
    }

    #[test]
    fn stepping_backwards_without_recording_says_why() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        app.settings.debugger.record = false;

        assert_eq!(app.apply(&crate::command::Command::StepBack), Effect::None);
        assert_eq!(app.status.severity, crate::app::Severity::Warning);
        assert!(
            app.status.text.contains("record"),
            "the message should name the setting: {}",
            app.status.text
        );
    }

    #[tokio::test]
    async fn the_flags_are_read_from_a_live_session() {
        for tool in ["nasm", "ld", "gdb"] {
            if !crate::process::is_available(Path::new(tool)) {
                eprintln!("skipping: {tool} not installed");
                return;
            }
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        std::fs::write(
            app.project.entry_path(),
            "section .text\n    global _start\n_start:\n    mov rax, 7\n    sub rax, rax\n    \
             mov rax, 60\n    xor edi, edi\n    syscall\n",
        )
        .expect("write");

        let mut session = None;
        start_session(&mut app, &mut session).await;
        assert!(session.is_some(), "session: {}", app.status.text);

        settle(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;
        settle(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;

        let flags = app.registers.flags().expect("the flags must be readable");
        assert!(
            flags.has(crate::instruction::Flag::Zero),
            "ZF should be set after sub rax, rax; got {}",
            flags.summary()
        );

        settle(&mut app, &mut session, Effect::DebugStop).await;
    }

    #[tokio::test]
    async fn the_call_stack_shows_the_chain_of_calls() {
        for tool in ["nasm", "ld", "gdb"] {
            if !crate::process::is_available(Path::new(tool)) {
                eprintln!("skipping: {tool} not installed");
                return;
            }
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        std::fs::write(
            app.project.entry_path(),
            "section .text\n    global _start\nhelper:\n    nop\n    ret\n_start:\n    \
             call helper\n    mov rax, 60\n    xor edi, edi\n    syscall\n",
        )
        .expect("write");

        app.breakpoints.add_symbol("helper");

        let mut session = None;
        start_session(&mut app, &mut session).await;
        assert!(session.is_some(), "session: {}", app.status.text);

        settle(&mut app, &mut session, Effect::DebugContinue).await;

        assert!(
            app.frames.len() >= 2,
            "expected a caller above the callee, got {:?}",
            app.frames.iter().map(|f| f.describe()).collect::<Vec<_>>()
        );
        assert_eq!(app.frames[0].function.as_deref(), Some("helper"));
        assert_eq!(app.frames[1].function.as_deref(), Some("_start"));
        assert!(app.frames[0].is_innermost());

        settle(&mut app, &mut session, Effect::DebugStop).await;
        assert!(
            app.frames.is_empty(),
            "frames must be cleared with the session"
        );
    }

    #[tokio::test]
    async fn a_full_debug_session_steps_and_updates_the_registers() {
        for tool in ["nasm", "ld", "gdb"] {
            if !crate::process::is_available(Path::new(tool)) {
                eprintln!("skipping: {tool} not installed");
                return;
            }
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        std::fs::write(
            app.project.entry_path(),
            "section .text\n    global _start\n_start:\n    mov rax, 1\n    mov rbx, 2\n    \
             add rax, rbx\n    mov rax, 60\n    xor edi, edi\n    syscall\n",
        )
        .expect("write");

        let mut session = None;
        start_session(&mut app, &mut session).await;

        assert!(
            session.is_some(),
            "a session should be running: {}",
            app.status.text
        );
        assert_eq!(
            app.debugger.state(),
            crate::debugger::DebuggerState::Paused,
            "{}",
            app.status.text
        );
        assert!(!app.registers.is_empty(), "registers should have been read");
        assert!(app.current_address.is_some());

        let first = app.registers.rip();
        settle(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;
        let second = app.registers.rip();

        assert_ne!(first, second, "stepping must advance the program counter");
        assert!(!app.stack.is_empty(), "the stack should have been read");

        settle(&mut app, &mut session, Effect::DebugStop).await;
        assert!(session.is_none(), "the session should be gone");
        assert!(app.registers.is_empty(), "state should have been cleared");
    }
}

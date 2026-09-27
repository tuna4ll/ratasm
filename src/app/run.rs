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
use crate::process::editor;
use crate::process::pty::{PtyControl, PtyEvent, PtySize};
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
    /// A resumed inferior stopped again, or the wait failed.
    DebugStopped {
        session: Box<GdbSession>,
        result: Result<Record, String>,
    },
}

/// Work in flight, so a second request can be refused and the first cancelled.
#[derive(Default)]
struct Running {
    /// The task, kept so stopping can abort it.
    handle: Option<tokio::task::JoinHandle<()>>,
    /// Commands for an interactive child, when the task owns a PTY.
    control: Option<tokio::sync::mpsc::UnboundedSender<PtyControl>>,
    /// Interrupt request for a GDB wait task.
    interrupt: Option<tokio::sync::mpsc::UnboundedSender<()>>,
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
                self.interrupt = None;
                true
            }
            _ => false,
        }
    }

    /// Asks a terminal transport to flush and close itself.
    fn request_stop(&self) -> bool {
        self.control
            .as_ref()
            .is_some_and(|control| control.send(PtyControl::Stop).is_ok())
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

    /// Stops and forgets a transport that has no child process of its own.
    fn abort(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.send(PtyControl::Stop);
        }
        if let Some(task) = self.handle.take() {
            task.abort();
        }
        self.interrupt = None;
    }
}

/// The ordinary background task, the debugger inferior's terminal transport
/// and `$EDITOR` in the Editor panel.
#[derive(Default)]
struct Runtimes {
    background: Running,
    debugger: Running,
    editor: Running,
    /// Where the editor's PTY reports, kept apart from the Output terminal's.
    editor_events: Option<tokio::sync::mpsc::UnboundedSender<PtyEvent>>,
}

impl Runtimes {
    fn terminal(&self) -> &Running {
        if self.background.control.is_some() {
            &self.background
        } else {
            &self.debugger
        }
    }

    fn resize(&self, size: PtySize) {
        self.background.resize(size);
        self.debugger.resize(size);
    }
}

/// Runs the interface until the user quits.
pub async fn run(mut app: App, terminal: &mut Tui) -> Result<()> {
    let mut events = EventStream::new();
    let mut session: Option<GdbSession> = None;
    let (finished_tx, mut finished_rx) = tokio::sync::mpsc::unbounded_channel::<Background>();
    let (pty_tx, mut pty_rx) = tokio::sync::mpsc::unbounded_channel::<PtyEvent>();
    let (editor_tx, mut editor_rx) = tokio::sync::mpsc::unbounded_channel::<PtyEvent>();
    let mut runtimes = Runtimes {
        editor_events: Some(editor_tx),
        ..Runtimes::default()
    };

    let size = terminal.size().context("cannot read the terminal size")?;
    if let Some(path) = app.workspace.active().path().map(Path::to_path_buf) {
        let line = 1;
        start_editor(
            &mut app,
            &mut runtimes,
            (size.width, size.height),
            path,
            line,
        );
    }

    loop {
        let size = terminal.size().context("cannot read the terminal size")?;
        sync_terminal_size(&mut app, &runtimes, size.width, size.height);
        sync_editor_size(&mut app, &runtimes, size.width, size.height);
        render::sync_scroll(&mut app, size.width, size.height);
        terminal
            .draw(|frame| render::draw(frame, &app))
            .context("cannot draw to the terminal")?;

        if app.should_quit {
            break;
        }

        let effect = tokio::select! {
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) => handle_key_event(&mut app, &runtimes, key),
                Some(Ok(Event::Paste(text))) => {
                    handle_paste(&app, &runtimes, text);
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
                runtimes.background.handle = None;
                runtimes.background.interrupt = None;
                finish_background(&mut app, &mut session, done).await;
                Effect::None
            },
            Some(event) = pty_rx.recv() => {
                finish_pty_event(&mut app, &mut runtimes, event);
                Effect::None
            },
            Some(event) = editor_rx.recv() => {
                finish_editor_event(&mut app, &mut runtimes, event);
                Effect::None
            },
            () = tokio::time::sleep(TICK) => Effect::None,
        };

        perform(
            &mut app,
            &mut session,
            &finished_tx,
            &pty_tx,
            &mut runtimes,
            (size.width, size.height),
            effect,
        )
        .await;
        drain_debugger_events(&mut app, &mut session).await;
        if session.is_none() && runtimes.debugger.is_busy() {
            let _ = runtimes.debugger.request_stop();
        }
    }

    runtimes.background.abort();
    runtimes.debugger.abort();
    runtimes.editor.abort();

    if let Some(session) = session {
        session.shutdown().await;
    }
    Ok(())
}

/// Applies one key event.
fn handle_key_event(app: &mut App, runtimes: &Runtimes, key: KeyEvent) -> Effect {
    if app.shows_editor() && app.focus == super::Panel::Editor && !app.mode.is_overlay() {
        if editor_reserved_key(key) {
            if let Some(command) = app.keymap.command_for_event(key).cloned() {
                return app.apply(&command);
            }
        }
        let application_cursor = app
            .editor_screen
            .as_ref()
            .is_some_and(|screen| screen.screen().application_cursor());
        if let Some(bytes) = terminal_key_bytes(key, application_cursor) {
            let _ = runtimes.editor.input(bytes);
        }
        return Effect::None;
    }
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
            let _ = runtimes.terminal().input(bytes);
        }
        return Effect::None;
    }
    crate::event::handle_key(app, key)
}

/// Keys ratasm keeps while `$EDITOR` has focus: function keys and Alt plus a
/// digit, which build, run, debug and change page. Everything else, Tab and
/// every Ctrl chord included, belongs to the editor.
fn editor_reserved_key(key: KeyEvent) -> bool {
    match key.code {
        KeyCode::F(_) => true,
        KeyCode::Char(ch) => key.modifiers.contains(KeyModifiers::ALT) && ch.is_ascii_digit(),
        _ => false,
    }
}

/// Commands that remain reachable while the Output panel sends ordinary keys to the child.
fn terminal_reserved_command(command: &Command) -> bool {
    matches!(
        command,
        Command::Stop
            | Command::Quit
            | Command::OpenPalette
            | Command::Run
            | Command::DebugContinue
            | Command::DebugInterrupt
            | Command::DebugStop
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

/// Sends pasted text to whichever embedded terminal has focus.
fn handle_paste(app: &App, runtimes: &Runtimes, text: String) {
    if app.mode.is_overlay() {
        return;
    }
    let (screen, running) = match app.focus {
        super::Panel::Output => (app.terminal.as_ref(), runtimes.terminal()),
        super::Panel::Editor if app.shows_editor() => {
            (app.editor_screen.as_ref(), &runtimes.editor)
        }
        _ => return,
    };
    let Some(screen) = screen else {
        return;
    };
    let bytes = if screen.screen().bracketed_paste() {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    };
    let _ = running.input(bytes);
}

/// Keeps both the parser and the kernel PTY aligned with the visible Output panel.
fn sync_terminal_size(app: &mut App, runtimes: &Runtimes, width: u16, height: u16) {
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
        runtimes.resize(size);
    }
}

/// Keeps `$EDITOR`'s screen and PTY the size of the Editor panel, when it is shown.
fn sync_editor_size(app: &mut App, runtimes: &Runtimes, width: u16, height: u16) {
    let Some(size) = panel_terminal_size(app, super::Panel::Editor, width, height) else {
        return;
    };
    if let Some(screen) = &mut app.editor_screen {
        if screen.size() != size {
            screen.resize(size);
            runtimes.editor.resize(size);
        }
    }
}

fn output_terminal_size(app: &App, width: u16, height: u16) -> Option<PtySize> {
    panel_terminal_size(app, super::Panel::Output, width, height)
}

/// The cells inside `panel`'s border on the current page, if the page shows it.
fn panel_terminal_size(app: &App, panel: super::Panel, width: u16, height: u16) -> Option<PtySize> {
    let layout = crate::ui::layout::compute(
        ratatui::layout::Rect::new(0, 0, width, height),
        app.page,
        app.focus,
    );
    let area = layout.area_of(panel)?;
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
    runtimes: &mut Runtimes,
    viewport: (u16, u16),
    effect: Effect,
) {
    match effect {
        Effect::None | Effect::Quit => {}

        Effect::Edit { path, line } => start_editor(app, runtimes, viewport, path, line),

        Effect::Build { debug } => {
            build(app, debug).await;
        }

        Effect::Run => {
            if runtimes.background.is_busy() || runtimes.debugger.is_busy() {
                app.status = Status::warning("Something is already running");
            } else if build(app, false).await {
                start_program(
                    app,
                    pty_events,
                    &mut runtimes.background,
                    viewport.0,
                    viewport.1,
                );
            }
        }

        Effect::StopProgram => {
            if app.debugger.state() == crate::debugger::DebuggerState::Running
                && session.is_none()
                && runtimes.background.cancel()
            {
                stop_session(app, session).await;
                let _ = runtimes.debugger.request_stop();
                app.status = Status::warning("Debug session stopped");
            } else if runtimes.background.cancel() {
                app.status = Status::warning("Stopped");
            } else {
                app.status = Status::info("Nothing is running");
            }
        }

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

        Effect::OpenFile(path) => match app.workspace.open(&path) {
            Ok(_) => {
                app.status = Status::success(format!("Opened {}", path.display()));
                app.focus = super::Panel::Editor;
                app.refresh_project_files();
                if app.editor_screen.is_none() {
                    start_editor(app, runtimes, viewport, path, 1);
                }
            }
            Err(error) => app.status = Status::error(error.to_string()),
        },

        Effect::DebugStart => {
            let size =
                output_terminal_size(app, viewport.0, viewport.1).unwrap_or(PtySize::new(80, 24));
            start_session(app, session, &mut runtimes.debugger, pty_events, size).await;
        }
        Effect::DebugStop => {
            if session.is_none() && app.debugger.state() == crate::debugger::DebuggerState::Running
            {
                runtimes.background.abort();
            }
            stop_session(app, session).await;
            let _ = runtimes.debugger.request_stop();
        }

        Effect::DebugContinue => {
            resume(
                app,
                session,
                finished,
                &mut runtimes.background,
                &mi::exec_continue(),
                "continuing",
            )
            .await;
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
            resume(
                app,
                session,
                finished,
                &mut runtimes.background,
                &command,
                "stepping",
            )
            .await;
        }

        Effect::DebugInterrupt => {
            if let Some(interrupt) = &runtimes.background.interrupt {
                if interrupt.send(()).is_ok() {
                    app.status = Status::info("Interrupting…");
                }
            } else {
                app.status = Status::warning("The program is not running");
            }
        }

        Effect::SyncBreakpoints => sync_breakpoints(app, session).await,

        Effect::ReadMemory(address) => read_memory(app, session, address).await,

        Effect::RunScratchpad => {
            if runtimes.background.is_busy() || runtimes.debugger.is_busy() {
                app.status = Status::warning("Something is already running");
            } else {
                start_snippet(app, finished, &mut runtimes.background);
            }
        }
    }
}

/// Starts `$EDITOR` on `path` inside the Editor panel, unless one is already open.
fn start_editor(
    app: &mut App,
    runtimes: &mut Runtimes,
    viewport: (u16, u16),
    path: std::path::PathBuf,
    line: usize,
) {
    if app.editor_screen.is_some() {
        app.focus_panel(super::Panel::Editor);
        return;
    }
    let Some(events) = runtimes.editor_events.clone() else {
        return;
    };

    app.focus_panel(super::Panel::Editor);
    let size = panel_terminal_size(app, super::Panel::Editor, viewport.0, viewport.1)
        .unwrap_or(PtySize::new(80, 24));
    let program = editor::from_env();
    app.editor_screen = Some(super::terminal::TerminalScreen::new(size));
    app.editing = Some(path.clone());
    app.status = Status::info(format!(
        "{program} is editing {}; F-keys and Alt+1-4 stay with ratasm",
        workspace::display_path(&path)
    ));

    let spec = editor::spec(&program, &path, line);
    let (control_tx, control_rx) = tokio::sync::mpsc::unbounded_channel();
    runtimes.editor.control = Some(control_tx);
    runtimes.editor.handle = Some(tokio::spawn(async move {
        crate::process::pty::run_interactive(spec, size, control_rx, events).await;
    }));
}

/// Folds output from `$EDITOR` into its screen, and reads the file back when it exits.
fn finish_editor_event(app: &mut App, runtimes: &mut Runtimes, event: PtyEvent) {
    match event {
        PtyEvent::Output(bytes) => {
            if let Some(screen) = &mut app.editor_screen {
                let replies = screen.process(&bytes);
                if !replies.is_empty() {
                    let _ = runtimes.editor.input(replies);
                }
            }
        }
        PtyEvent::Finished(result) => {
            runtimes.editor.handle = None;
            runtimes.editor.control = None;
            let last_screen = app.editor_screen.take();
            let program = editor::from_env();
            let failed = !matches!(&result, Ok(output) if output.outcome.is_success());
            if let (true, Some(screen)) = (failed, last_screen) {
                // Whatever the editor printed on its way out explains why.
                app.output = screen.into_lines();
            }
            app.status = match result {
                Ok(output) if output.outcome.is_success() => Status::default(),
                Ok(output) => Status::warning(match output.outcome.exit_code() {
                    Some(127) => format!("{program} was not found; set $EDITOR"),
                    Some(code) => format!("{program} exited with status {code}"),
                    None => format!("{program} was stopped"),
                }),
                Err(error) => Status::error(format!("cannot start {program}: {error}")),
            };
            if let Some(path) = app.editing.take() {
                app.finish_edit(&path);
            }
        }
        PtyEvent::Closed => {}
    }
}

/// Builds the project, reporting the outcome.
async fn build(app: &mut App, debug: bool) -> bool {
    if let Err(error) = app.workspace.reload() {
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
async fn finish_background(app: &mut App, session: &mut Option<GdbSession>, done: Background) {
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
        Background::DebugStopped {
            session: active,
            result,
        } => {
            // The task that owned this sender has completed.
            *session = Some(*active);
            match result {
                Ok(record) => handle_stop(app, session, &record).await,
                Err(error) => {
                    app.status = Status::error(error);
                    let _ = app.debugger.apply(Transition::SessionFailed);
                    if let Some(active) = session.take() {
                        active.shutdown().await;
                    }
                }
            }
        }
    }
}

fn finish_pty_event(app: &mut App, runtimes: &mut Runtimes, event: PtyEvent) {
    match event {
        PtyEvent::Output(bytes) => {
            if let Some(terminal) = &mut app.terminal {
                let replies = terminal.process(&bytes);
                if !replies.is_empty() {
                    let _ = runtimes.terminal().input(replies);
                }
            }
        }
        PtyEvent::Finished(result) => {
            runtimes.background.handle = None;
            runtimes.background.control = None;
            match result {
                Ok(output) => app.finish_run(output),
                Err(error) => {
                    app.terminal = None;
                    app.status = Status::error(error.to_string());
                }
            }
        }
        PtyEvent::Closed => {
            runtimes.debugger.handle = None;
            runtimes.debugger.control = None;
            app.finish_terminal();
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
async fn start_session(
    app: &mut App,
    session: &mut Option<GdbSession>,
    terminal_runtime: &mut Running,
    terminal_events: &tokio::sync::mpsc::UnboundedSender<PtyEvent>,
    terminal_size: PtySize,
) {
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

    terminal_runtime.abort();
    let (tty_path, endpoint) = match crate::process::pty::PtyPair::open(terminal_size)
        .and_then(crate::process::pty::PtyPair::into_endpoint)
    {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = app
                .debugger
                .fail(Transition::LaunchFailed, error.to_string());
            app.status = Status::error(error.to_string());
            active.shutdown().await;
            return;
        }
    };
    let (control_tx, control_rx) = tokio::sync::mpsc::unbounded_channel();
    let events = terminal_events.clone();
    terminal_runtime.control = Some(control_tx);
    terminal_runtime.handle = Some(tokio::spawn(endpoint.run(control_rx, events)));
    app.terminal = Some(super::terminal::TerminalScreen::new(terminal_size));
    app.output.clear();

    let setup = async {
        active
            .execute(&mi::file_exec_and_symbols(&executable))
            .await?;
        active
            .execute(&mi::environment_cd(&app.project.run_directory()))
            .await?;
        active.execute(&mi::inferior_tty_set(&tty_path)).await?;
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
    finished: &tokio::sync::mpsc::UnboundedSender<Background>,
    running: &mut Running,
    command: &crate::debugger::mi::Command,
    what: &str,
) {
    let Some(mut active) = session.take() else {
        app.status = Status::warning("No debug session");
        return;
    };

    if let Err(error) = active.execute(command).await {
        app.status = Status::error(error.to_string());
        *session = Some(active);
        return;
    }
    let _ = app.debugger.apply(Transition::Resumed);
    app.status = Status::info(format!("{what}…"));

    let finished = finished.clone();
    let what = what.to_owned();
    let (interrupt_tx, mut interrupt_rx) = tokio::sync::mpsc::unbounded_channel();
    running.interrupt = Some(interrupt_tx);
    running.handle = Some(tokio::spawn(async move {
        let stopped = tokio::select! {
            result = active.wait_for_stop(STOP_TIMEOUT) => result,
            request = interrupt_rx.recv() => {
                if request.is_none() {
                    Err(crate::debugger::SessionError::Terminated)
                } else {
                    match active.execute(&mi::exec_interrupt()).await {
                        Ok(_) => active.wait_for_stop(STOP_TIMEOUT).await,
                        Err(error) => Err(error),
                    }
                }
            }
        };
        let result = stopped.map_err(|error| format!("{what}: {error}"));
        let _ = finished.send(Background::DebugStopped {
            session: Box::new(active),
            result,
        });
    }));
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
        Ok(_) => {}
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
    fn only_function_keys_and_alt_digits_leave_the_editor() {
        let key = |code, modifiers| KeyEvent::new(code, modifiers);
        assert!(editor_reserved_key(key(KeyCode::F(6), KeyModifiers::NONE)));
        assert!(editor_reserved_key(key(
            KeyCode::Char('2'),
            KeyModifiers::ALT
        )));
        for kept in [
            key(KeyCode::Tab, KeyModifiers::NONE),
            key(KeyCode::Char('s'), KeyModifiers::CONTROL),
            key(KeyCode::Char('q'), KeyModifiers::CONTROL),
            key(KeyCode::Char('p'), KeyModifiers::CONTROL),
            key(KeyCode::Char('x'), KeyModifiers::ALT),
            key(KeyCode::Esc, KeyModifiers::NONE),
        ] {
            assert!(!editor_reserved_key(kept), "{kept:?} belongs to the editor");
        }
    }

    #[test]
    fn keys_reach_the_editor_instead_of_the_keymap() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        app.editor_screen = Some(super::super::terminal::TerminalScreen::new(PtySize::new(
            80, 24,
        )));
        let (control_tx, mut control_rx) = tokio::sync::mpsc::unbounded_channel();
        let runtimes = Runtimes {
            editor: Running {
                control: Some(control_tx),
                ..Running::default()
            },
            ..Runtimes::default()
        };

        let ctrl_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(handle_key_event(&mut app, &runtimes, ctrl_q), Effect::None);
        assert!(!app.should_quit);
        assert!(matches!(
            control_rx.try_recv(),
            Ok(PtyControl::Input(bytes)) if bytes == [0x11]
        ));

        let build = KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(&mut app, &runtimes, build),
            Effect::Build { debug: false }
        );
        assert!(control_rx.try_recv().is_err(), "F6 stays with ratasm");
    }

    #[tokio::test]
    async fn the_editor_runs_in_the_panel_and_its_file_is_read_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        let path = dir.path().join("main.asm");
        std::fs::write(&path, "ret\n").expect("write");
        app.workspace.open(&path).expect("open");

        let script = dir.path().join("fake-editor");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf 'EDITING %s' \"$1\"\nprintf 'nop\\nret\\n' > \"$1\"\n",
        )
        .expect("write");
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");

        let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut runtimes = Runtimes {
            editor_events: Some(events_tx),
            ..Runtimes::default()
        };
        let program = script.display().to_string();
        app.editing = Some(path.clone());
        app.editor_screen = Some(super::super::terminal::TerminalScreen::new(PtySize::new(
            80, 24,
        )));
        let spec = crate::process::editor::spec(&program, &path, 1);
        let (control_tx, control_rx) = tokio::sync::mpsc::unbounded_channel();
        runtimes.editor.control = Some(control_tx);
        let events = runtimes.editor_events.clone().expect("sender");
        tokio::spawn(crate::process::pty::run_interactive(
            spec,
            PtySize::new(80, 24),
            control_rx,
            events,
        ));

        let mut seen = String::new();
        while app.editor_screen.is_some() {
            let event = tokio::time::timeout(Duration::from_secs(5), events_rx.recv())
                .await
                .expect("the editor finishes")
                .expect("an event");
            if let PtyEvent::Output(bytes) = &event {
                seen.push_str(&String::from_utf8_lossy(bytes));
            }
            finish_editor_event(&mut app, &mut runtimes, event);
        }

        assert!(
            seen.contains("EDITING"),
            "the editor drew into the panel: {seen:?}"
        );
        assert!(app.editing.is_none());
        assert_eq!(app.workspace.active().buffer().to_text(), "nop\nret\n");
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
        let mut runtimes = Runtimes::default();
        perform(
            app,
            session,
            &finished,
            &pty_events,
            &mut runtimes,
            (160, 48),
            effect,
        )
        .await;
        while runtimes.background.handle.is_some() {
            tokio::select! {
                Some(done) = incoming.recv() => {
                    runtimes.background.handle = None;
                    runtimes.background.interrupt = None;
                    finish_background(app, session, done).await;
                }
                Some(event) = pty_incoming.recv() => {
                    finish_pty_event(app, &mut runtimes, event);
                }
            }
        }
    }

    async fn start_test_session(
        app: &mut App,
        session: &mut Option<GdbSession>,
    ) -> (Running, tokio::sync::mpsc::UnboundedReceiver<PtyEvent>) {
        let mut runtime = Running::default();
        let (events, incoming) = tokio::sync::mpsc::unbounded_channel();
        start_session(app, session, &mut runtime, &events, PtySize::new(80, 24)).await;
        (runtime, incoming)
    }

    #[tokio::test]
    async fn building_reads_what_the_editor_wrote_first() {
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
        let edited = format!(
            "; edited\n{}",
            std::fs::read_to_string(&entry).expect("read")
        );
        std::fs::write(&entry, &edited).expect("write");

        assert!(build(&mut app, false).await, "the build should succeed");
        assert_eq!(
            app.workspace.active().buffer().to_text(),
            edited,
            "the view must show the source that was assembled"
        );
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
        let mut runtimes = Runtimes::default();
        let mut session = None;

        perform(
            &mut app,
            &mut session,
            &finished,
            &pty_events,
            &mut runtimes,
            (160, 48),
            Effect::Run,
        )
        .await;
        assert!(
            runtimes.background.is_busy(),
            "the program should still be looping"
        );

        perform(
            &mut app,
            &mut session,
            &finished,
            &pty_events,
            &mut runtimes,
            (160, 48),
            Effect::StopProgram,
        )
        .await;
        assert_eq!(app.status.severity, crate::app::Severity::Warning);
        while runtimes.background.handle.is_some() {
            let event = tokio::time::timeout(Duration::from_secs(2), pty_incoming.recv())
                .await
                .expect("the stopped process should finish")
                .expect("PTY event channel");
            finish_pty_event(&mut app, &mut runtimes, event);
        }
        assert!(!runtimes.background.is_busy());
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
        let (_terminal_runtime, _terminal_events) =
            start_test_session(&mut app, &mut session).await;
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
        let (_terminal_runtime, _terminal_events) =
            start_test_session(&mut app, &mut session).await;
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
        let (_terminal_runtime, _terminal_events) =
            start_test_session(&mut app, &mut session).await;
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
        let (_terminal_runtime, _terminal_events) =
            start_test_session(&mut app, &mut session).await;
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
        let (_terminal_runtime, _terminal_events) =
            start_test_session(&mut app, &mut session).await;

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

    #[tokio::test]
    async fn a_debugged_program_can_read_from_its_terminal() {
        for tool in ["nasm", "ld", "gdb"] {
            if !crate::process::is_available(Path::new(tool)) {
                eprintln!("skipping: {tool} not installed");
                return;
            }
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        app.settings.debugger.record = false;
        std::fs::write(
            app.project.entry_path(),
            "section .data\n    label db 'Got: '\n    label_len equ $-label\n\
             section .bss\n    input resb 16\n\
             section .text\n    global _start\n_start:\n\
             mov eax, 0\n    mov edi, 0\n    mov rsi, input\n    mov edx, 16\n    syscall\n\
             mov r12, rax\n    mov eax, 1\n    mov edi, 1\n    mov rsi, label\n\
             mov edx, label_len\n    syscall\n    mov eax, 1\n    mov rsi, input\n\
             mov rdx, r12\n    syscall\n    mov eax, 60\n    xor edi, edi\n    syscall\n",
        )
        .expect("write");

        let mut session = None;
        let (mut terminal_runtime, mut terminal_events) =
            start_test_session(&mut app, &mut session).await;
        assert!(session.is_some(), "session: {}", app.status.text);
        let input = terminal_runtime
            .control
            .as_ref()
            .expect("terminal input")
            .clone();
        let send_input = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            input
                .send(PtyControl::Input(b"Ada\r".to_vec()))
                .expect("inferior terminal");
        });

        settle(&mut app, &mut session, Effect::DebugContinue).await;
        send_input.await.expect("input task");
        assert!(
            session.is_none(),
            "the program should have exited: {} ({})",
            app.status.text,
            app.debugger.state()
        );
        assert!(terminal_runtime.request_stop());

        let mut output = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), terminal_events.recv())
                .await
                .expect("terminal should close")
                .expect("terminal event");
            match event {
                PtyEvent::Output(bytes) => output.extend(bytes),
                PtyEvent::Closed => break,
                PtyEvent::Finished(_) => panic!("debug endpoints do not own the inferior"),
            }
        }
        terminal_runtime.abort();

        let output = String::from_utf8_lossy(&output);
        assert!(output.contains("Got: Ada"), "{output:?}");
    }
}

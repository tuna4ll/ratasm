//! The loop that draws, reads input and performs effects.
//!
//! This is the only place the three worlds meet: the terminal, the async
//! runtime, and the synchronous state in [`App`]. Everything it does is either
//! "draw the state" or "perform an effect and put the result back into the
//! state", which keeps the interesting logic in modules that need neither a
//! terminal nor a runtime to test.
//!
//! # Why GDB lives here and not in `App`
//!
//! A [`GdbSession`] owns a child process. Putting it in `App` would make the
//! state un-cloneable, un-testable and awkward to reason about; every test
//! would need a debugger. The loop owns the session instead and hands `App`
//! the *results* — registers, memory, a stop location — which are plain data.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream, KeyEvent};
use futures_util::StreamExt as _;

use crate::assembler::{self, BuildOptions};
use crate::debugger::mi::{command as mi, Record};
use crate::debugger::registers::parse_register_values;
use crate::debugger::session::GdbSession;
use crate::debugger::state::Transition;
use crate::disassembler;
use crate::editor::workspace;
use crate::ui::clipboard;
use crate::ui::render;
use crate::ui::Tui;

use super::state::{Effect, StepKind};
use super::{App, Status};

/// How long to wait for the program to stop after a step.
const STOP_TIMEOUT: Duration = Duration::from_secs(15);

/// How often to redraw when nothing has happened.
const TICK: Duration = Duration::from_millis(250);

/// Runs the interface until the user quits.
///
/// # Errors
///
/// Returns an error only for failures that make continuing impossible, such
/// as the terminal becoming unusable. Everything a user can cause — a failed
/// build, a missing debugger, a crashed program — is reported in the status
/// bar and the loop keeps going.
pub async fn run(mut app: App, terminal: &mut Tui) -> Result<()> {
    let mut events = EventStream::new();
    let mut session: Option<GdbSession> = None;

    loop {
        let size = terminal.size().context("cannot read the terminal size")?;
        render::sync_scroll(&mut app, size.width, size.height);
        terminal
            .draw(|frame| render::draw(frame, &app))
            .context("cannot draw to the terminal")?;

        if app.should_quit {
            break;
        }

        // Wake up either on input or on the tick, so asynchronous debugger
        // events are noticed even while the user is not typing.
        let effect = tokio::select! {
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) => handle_key_event(&mut app, key),
                Some(Ok(Event::Resize(..))) => Effect::None,
                Some(Ok(_)) => Effect::None,
                // The event stream ending means stdin closed; there is no
                // way to receive further input, so exiting is the only
                // sensible response.
                Some(Err(_)) | None => break,
            },
            () = tokio::time::sleep(TICK) => Effect::None,
        };

        perform(&mut app, &mut session, effect).await;
        drain_debugger_events(&mut app, &mut session).await;
    }

    // Never leave GDB holding the program's process group.
    if let Some(session) = session {
        session.shutdown().await;
    }
    Ok(())
}

/// Applies one key event.
fn handle_key_event(app: &mut App, key: KeyEvent) -> Effect {
    crate::event::handle_key(app, key)
}

/// Performs an effect, folding the result back into the application.
async fn perform(app: &mut App, session: &mut Option<GdbSession>, effect: Effect) {
    match effect {
        Effect::None | Effect::Quit => {}

        Effect::Build { debug } => {
            build(app, debug).await;
        }

        Effect::Run => {
            if build(app, false).await {
                run_program(app).await;
            }
        }

        Effect::StopProgram => {
            app.status = Status::info("Programs already run with a timeout; nothing to stop");
        }

        Effect::SaveFile(path) => match app.workspace.save_active_as(&path) {
            Ok(path) => app.status = Status::success(format!("Saved {}", path.display())),
            Err(error) => app.status = Status::error(error.to_string()),
        },

        Effect::OpenFile(path) => match app.workspace.open(&path) {
            Ok(_) => {
                app.status = Status::success(format!("Opened {}", path.display()));
                app.focus = super::Panel::Editor;
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
            app.status = Status::info("Running the snippet…");
            let gdb = app.settings.debugger.gdb.clone();
            match app.scratchpad.run(&gdb).await {
                Ok(outcome) => {
                    app.status = if outcome.is_empty() {
                        Status::info("The snippet changed nothing")
                    } else {
                        Status::success(format!("{} register(s) changed", outcome.changes.len()))
                    };
                    app.scratchpad_result = Some(Ok(outcome));
                }
                Err(error) => {
                    app.status = Status::error(error.to_string());
                    app.scratchpad_result = Some(Err(error.to_string()));
                }
            }
        }

        Effect::SetSystemClipboard(text) => match clipboard::set_sequence(&text) {
            // The sequence goes straight to the terminal rather than through
            // ratatui: it draws nothing, it asks the terminal emulator for
            // something.
            Some(sequence) => {
                use std::io::Write as _;
                let mut out = std::io::stdout();
                if write!(out, "{sequence}")
                    .and_then(|()| out.flush())
                    .is_err()
                {
                    // ratasm's own clipboard already holds the text, so
                    // pasting still works; only the system clipboard missed
                    // out, and saying so would be noise on every copy.
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
///
/// Returns whether an executable was produced.
async fn build(app: &mut App, debug: bool) -> bool {
    let options = if debug {
        BuildOptions::debug()
    } else {
        BuildOptions::release()
    };

    // Announce the build to the state machine. Without this the machine stays
    // where it was, and every transition that follows — build succeeded,
    // launch requested, stopped — is rejected as illegal from that state, so
    // the session silently never becomes Paused.
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

/// Runs the built program.
async fn run_program(app: &mut App) {
    let Some(executable) = app
        .build
        .as_ref()
        .and_then(|outcome| outcome.executable.clone())
    else {
        return;
    };

    app.status = Status::info("Running…");
    match assembler::run_executable(&app.project, &executable).await {
        Ok(output) => app.finish_run(output),
        Err(error) => app.status = Status::error(error.to_string()),
    }
}

/// Disassembles the built executable so the panel has something to show
/// before any debugging starts.
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
    // A debug session needs debug information, so always build for it.
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

    // Breakpoints the user set before the session existed are replayed now.
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

    // Stop at the entry point so the user can look before anything runs.
    //
    // `-exec-run --start` is the obvious way to do this and is wrong here: it
    // sets a temporary breakpoint on `main`, which a `_start`-only assembly
    // program does not have, so the program runs to completion instead of
    // stopping. Breaking on the ELF entry address always works, whatever the
    // entry symbol is called.
    match disassembler::ElfImage::load(&executable) {
        Ok(image) => {
            let _ = active
                .try_execute(&mi::break_insert_temporary_at_address(image.entry))
                .await;
        }
        Err(_) => {
            // Without the ELF the symbol name is the best guess available.
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
            // Recording needs a live process, so it can only start once the
            // program has stopped at the entry point — asking any earlier
            // gets "Target native does not support this command". Starting
            // here means everything from the entry onwards is replayable.
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
    // A program that has exited is not a program that is paused: stepping it
    // would wait forever.
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

    if let Some(number) = record.get("bkptno").and_then(|value| value.as_str()) {
        if let Ok(number) = number.parse::<u32>() {
            app.breakpoints.record_hit(number);
        }
    }

    app.status = match record.stop_reason() {
        Some("breakpoint-hit") => Status::success("Stopped at a breakpoint"),
        Some("signal-received") => {
            let signal = record.get_str("signal-name").unwrap_or("a signal");
            // A segmentation fault is the interesting case, not an error in
            // the tool, so the address it happened at is the useful part.
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

    // Registers first: the stack and memory reads depend on RSP.
    let names = active.execute(&mi::data_list_register_names()).await;
    let values = active.execute(&mi::data_list_register_values()).await;

    if let (Ok(names), Ok(values)) = (names, values) {
        if let (Some(names), Some(values)) =
            (names.get("register-names"), values.get("register-values"))
        {
            app.registers.update(parse_register_values(names, values));
        }
    }

    // The call stack: how execution reached here, as distinct from the bytes
    // around RSP that the stack memory panel shows.
    match active.execute(&mi::stack_list_frames()).await {
        Ok(record) => {
            app.frames = record
                .get("stack")
                .map(crate::debugger::frames::parse_frames)
                .unwrap_or_default();
            app.frame_selected = 0;
        }
        // GDB cannot always walk the stack — early in _start there is no frame
        // chain yet. That is normal, not a session failure.
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

    // Refresh whatever the memory panel is looking at, so it tracks the
    // program rather than showing a stale snapshot.
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
        // An unreadable address is a normal thing to ask for by accident, so
        // it is reported rather than treated as a session failure.
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

        build(&mut app, false).await;
        run_program(&mut app).await;

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
            perform(&mut app, &mut session, effect).await;
        }
    }

    #[tokio::test]
    async fn saving_through_an_effect_writes_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        let mut session = None;

        app.workspace.active_mut().insert("ret\n");
        let path = dir.path().join("written.asm");
        perform(&mut app, &mut session, Effect::SaveFile(path.clone())).await;

        assert_eq!(std::fs::read_to_string(&path).expect("read"), "ret\n");
        assert_eq!(app.status.severity, crate::app::Severity::Success);
    }

    #[tokio::test]
    async fn opening_a_missing_file_reports_the_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut app = app_for(dir.path());
        let mut session = None;

        perform(
            &mut app,
            &mut session,
            Effect::OpenFile(dir.path().join("absent.asm")),
        )
        .await;
        assert_eq!(app.status.severity, crate::app::Severity::Error);
    }

    #[tokio::test]
    async fn the_explanation_gains_real_values_once_the_program_stops() {
        // End to end: build, stop, and check the explainer is reading the
        // registers rather than only describing the instruction.
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

        // Step past the two movs so RAX and RBX hold known values.
        perform(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;
        perform(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;

        assert_eq!(app.registers.value_of("rax"), Some(1));
        assert_eq!(app.registers.value_of("rbx"), Some(2));

        // The cursor follows the program counter while stopped, so the
        // explanation is for `add rax, rbx`.
        let explanation = app.current_explanation().expect("an explanation");
        assert_eq!(explanation.mnemonic, "add");
        assert_eq!(
            explanation.concrete.as_deref(),
            Some("0x1 ← 0x1 + 0x2"),
            "the explainer should be reading live registers"
        );

        perform(&mut app, &mut session, Effect::DebugStop).await;
    }

    #[tokio::test]
    async fn stepping_backwards_restores_the_previous_register_values() {
        // The feature only means anything if the machine really goes back, so
        // the test checks a register's value, not just the program counter.
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

        perform(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;
        assert_eq!(
            app.registers.value_of("rax"),
            Some(1),
            "after the first mov"
        );

        perform(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;
        assert_eq!(app.registers.value_of("rax"), Some(2), "after the second");

        perform(&mut app, &mut session, Effect::Step(StepKind::Back)).await;
        assert_eq!(
            app.registers.value_of("rax"),
            Some(1),
            "stepping back must undo the write, not just move RIP: {}",
            app.status.text
        );

        perform(&mut app, &mut session, Effect::DebugStop).await;
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
        // A regression guard: GDB names the flags register `eflags`, and a
        // filter that only accepted canonical names silently blanked the flag
        // panel for every session.
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

        perform(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;
        perform(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;

        let flags = app.registers.flags().expect("the flags must be readable");
        assert!(
            flags.has(crate::instruction::Flag::Zero),
            "ZF should be set after sub rax, rax; got {}",
            flags.summary()
        );

        perform(&mut app, &mut session, Effect::DebugStop).await;
    }

    #[tokio::test]
    async fn the_call_stack_shows_the_chain_of_calls() {
        // The panel exists to answer "how did I get here?", so the test uses a
        // program that actually calls into another function.
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

        // Break inside the callee so there is a caller above it.
        app.breakpoints.add_symbol("helper");

        let mut session = None;
        start_session(&mut app, &mut session).await;
        assert!(session.is_some(), "session: {}", app.status.text);

        perform(&mut app, &mut session, Effect::DebugContinue).await;

        assert!(
            app.frames.len() >= 2,
            "expected a caller above the callee, got {:?}",
            app.frames.iter().map(|f| f.describe()).collect::<Vec<_>>()
        );
        assert_eq!(app.frames[0].function.as_deref(), Some("helper"));
        assert_eq!(app.frames[1].function.as_deref(), Some("_start"));
        assert!(app.frames[0].is_innermost());

        perform(&mut app, &mut session, Effect::DebugStop).await;
        assert!(
            app.frames.is_empty(),
            "frames must be cleared with the session"
        );
    }

    #[tokio::test]
    async fn a_full_debug_session_steps_and_updates_the_registers() {
        // The end-to-end path: build with debug info, start GDB, stop at the
        // entry, step, and see the registers change.
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
        perform(&mut app, &mut session, Effect::Step(StepKind::Instruction)).await;
        let second = app.registers.rip();

        assert_ne!(first, second, "stepping must advance the program counter");
        assert!(!app.stack.is_empty(), "the stack should have been read");

        perform(&mut app, &mut session, Effect::DebugStop).await;
        assert!(session.is_none(), "the session should be gone");
        assert!(app.registers.is_empty(), "state should have been cleared");
    }
}

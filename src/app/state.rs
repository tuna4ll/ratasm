//! Application state, and what each command does to it.
//!
//! # Synchronous state, asynchronous work
//!
//! [`App::apply`] is a pure state transition: it takes a [`Command`], changes
//! the state, and returns an [`Effect`] describing any I/O that has to happen.
//! Building, running and talking to GDB are *not* performed here.
//!
//! That split is what makes the application testable. Every command can be
//! exercised without a terminal, without a toolchain and without an async
//! runtime, and the tests below do exactly that. The run loop is then a thin
//! thing that performs effects and feeds results back.

use std::path::PathBuf;

use crate::assembler::{BuildOutcome, Diagnostic};
use crate::command::Command;
use crate::config::{Keymap, Settings};
use crate::debugger::breakpoints::{BreakpointSet, Location};
use crate::debugger::memory::MemoryBlock;
use crate::debugger::registers::{Format, RegisterFile};
use crate::debugger::state::{DebuggerState, StateMachine, Transition};
use crate::disassembler::{DisassemblyLine, Syntax};
use crate::editor::{Movement, Position, Range, SelectionMode, Workspace};
use crate::instruction::Database as InstructionDatabase;
use crate::process::ProcessOutput;
use crate::project::Project;
use crate::syscall::Database as SyscallDatabase;
use crate::ui::Theme;

use super::mode::{Mode, Palette, Prompt, PromptKind};
use super::panel::Panel;

/// Work the run loop must perform on the application's behalf.
///
/// Returned by [`App::apply`] rather than done inside it, so state changes stay
/// synchronous and testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Nothing to do.
    None,
    /// Leave the application.
    Quit,
    /// Assemble and link, optionally with debug information.
    Build {
        /// Whether to ask the assembler for debug information.
        debug: bool,
    },
    /// Build if needed, then run the program.
    Run,
    /// Stop a running program.
    StopProgram,
    /// Start a debug session.
    DebugStart,
    /// Resume a paused program.
    DebugContinue,
    /// Interrupt a running program.
    DebugInterrupt,
    /// End the debug session.
    DebugStop,
    /// Advance the program by one step.
    Step(StepKind),
    /// Send a breakpoint change to the debugger.
    SyncBreakpoints,
    /// Read memory at an address into the memory panel.
    ReadMemory(u64),
    /// Write the active buffer to disk.
    SaveFile(PathBuf),
    /// Read a file into a new buffer.
    OpenFile(PathBuf),
    /// Assemble and run the scratchpad snippet.
    RunScratchpad,
    /// Offer text to the terminal's clipboard.
    SetSystemClipboard(String),
}

/// How far a step goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// One machine instruction, entering calls.
    Instruction,
    /// One machine instruction, over calls.
    Over,
    /// One source line.
    Line,
    /// Until the current function returns.
    Out,
    /// One machine instruction backwards.
    Back,
    /// One machine instruction backwards, over calls.
    BackOver,
    /// Backwards until the previous breakpoint.
    ReverseContinue,
}

/// How serious a status message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Ordinary feedback.
    Info,
    /// Something worth noticing.
    Warning,
    /// Something went wrong.
    Error,
    /// Something worked.
    Success,
}

/// A message shown in the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The text.
    pub text: String,
    /// How serious it is.
    pub severity: Severity,
}

impl Status {
    /// An informational message.
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: Severity::Info,
        }
    }

    /// A success message.
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: Severity::Success,
        }
    }

    /// A warning.
    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: Severity::Warning,
        }
    }

    /// An error.
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: Severity::Error,
        }
    }
}

impl Default for Status {
    fn default() -> Self {
        Status::info("Ready. Ctrl+P for commands, F6 to build, F5 to run.")
    }
}

/// Everything the interface shows.
pub struct App {
    /// Open documents.
    pub workspace: Workspace,
    /// The project being worked on.
    pub project: Project,
    /// User settings.
    pub settings: Settings,
    /// Key bindings.
    pub keymap: Keymap,
    /// Colours and glyphs.
    pub theme: Theme,
    /// Which panel has focus.
    pub focus: Panel,
    /// What the interface is doing.
    pub mode: Mode,
    /// The status bar message.
    pub status: Status,

    /// The debug session's state.
    pub debugger: StateMachine,
    /// Register values at the last stop.
    pub registers: RegisterFile,
    /// Breakpoints the user has asked for.
    pub breakpoints: BreakpointSet,
    /// The memory currently on show.
    pub memory: MemoryBlock,
    /// The address the memory panel is looking at.
    pub memory_address: Option<u64>,
    /// Stack contents around the stack pointer.
    pub stack: MemoryBlock,
    /// Whether execution is being recorded, so it can be stepped backwards.
    ///
    /// Distinct from the setting that asks for it: GDB may refuse, and the
    /// interface must report what is actually true rather than what was
    /// requested.
    pub recording: bool,
    /// The call stack at the last stop, innermost frame first.
    pub frames: Vec<crate::debugger::frames::Frame>,
    /// Which frame the call stack panel highlights.
    pub frame_selected: usize,
    /// The disassembly on show.
    pub disassembly: Vec<DisassemblyLine>,
    /// Where execution is stopped.
    pub current_address: Option<u64>,
    /// The source line execution is stopped at.
    pub current_line: Option<(PathBuf, usize)>,

    /// The last build's result.
    pub build: Option<BuildOutcome>,
    /// The last program run's result.
    pub last_run: Option<ProcessOutput>,
    /// Lines shown in the output panel.
    pub output: Vec<String>,
    /// Diagnostics from the last build.
    pub diagnostics: Vec<Diagnostic>,

    /// How register values are shown.
    pub register_format: Format,
    /// Which operand order the disassembly uses.
    pub disassembly_syntax: Syntax,
    /// Whether the learning panel is showing.
    pub learning_mode: bool,
    /// The snippet being tried, and the values it starts from.
    pub scratchpad: crate::scratchpad::Scratchpad,
    /// What the last snippet produced, or why it could not run.
    pub scratchpad_result: Option<Result<crate::scratchpad::Outcome, String>>,
    /// Where the reader is in the material.
    pub learning: crate::learning::Progress,
    /// Text cut or copied, used by paste.
    ///
    /// ratasm keeps its own copy because the terminal will accept text for
    /// the system clipboard but will not reliably give any back.
    pub clipboard: String,

    /// Instruction semantics, loaded once.
    pub instructions: InstructionDatabase,
    /// The syscall table, loaded once.
    pub syscalls: SyscallDatabase,
    /// The syscall panel's query.
    pub syscall_query: String,
    /// Which syscall is highlighted.
    pub syscall_selected: usize,
    /// Which breakpoint is highlighted.
    pub breakpoint_selected: usize,

    /// The search query in effect.
    pub search_query: String,
    /// What a pending prompt will do with its answer.
    pending_prompt: Option<PromptKind>,
    /// Whether the application should exit.
    pub should_quit: bool,
}

impl App {
    /// Creates the application for a project.
    ///
    /// # Errors
    ///
    /// Returns an error only if the embedded databases fail to parse, which
    /// would mean a corrupt build rather than anything the user did.
    pub fn new(project: Project, settings: Settings) -> Result<Self, serde_json::Error> {
        let (keymap, _) = settings.keymap();
        let theme = settings.theme();

        Ok(Self {
            workspace: Workspace::new(),
            project,
            keymap,
            theme,
            focus: Panel::default(),
            mode: Mode::default(),
            status: Status::default(),

            debugger: StateMachine::new(),
            registers: RegisterFile::new(),
            breakpoints: BreakpointSet::new(),
            memory: MemoryBlock::default(),
            memory_address: None,
            stack: MemoryBlock::default(),
            recording: false,
            frames: Vec::new(),
            frame_selected: 0,
            disassembly: Vec::new(),
            current_address: None,
            current_line: None,

            build: None,
            last_run: None,
            output: Vec::new(),
            diagnostics: Vec::new(),

            register_format: Format::default(),
            disassembly_syntax: Syntax::default(),
            learning_mode: false,
            scratchpad: crate::scratchpad::Scratchpad::new(),
            scratchpad_result: None,
            learning: crate::learning::Progress::new(),
            clipboard: String::new(),

            instructions: InstructionDatabase::load()?,
            syscalls: SyscallDatabase::load()?,
            syscall_query: String::new(),
            syscall_selected: 0,
            breakpoint_selected: 0,

            search_query: String::new(),
            pending_prompt: None,
            should_quit: false,
            settings,
        })
    }

    /// Copies the selection, or the whole line when there is none.
    ///
    /// Copying the current line with nothing selected is what every editor
    /// does, and it is the common case: you want this instruction, and
    /// selecting it first is a step you should not have to take.
    fn copy(&mut self, remove: bool) -> Effect {
        let document = self.workspace.active_mut();
        let selected = document.has_selection();

        let text = if selected {
            document.selected_text()
        } else {
            let line = document.cursor().line;
            format!("{}\n", document.buffer().line_or_empty(line))
        };

        if text.trim().is_empty() && !selected {
            self.status = Status::info("The line is empty");
            return Effect::None;
        }

        // With nothing selected, cutting takes the line the copy took —
        // including its newline, so the lines below move up.
        if remove && !document.delete_selection() {
            let line = document.cursor().line;
            let buffer = document.buffer();
            let end = if line + 1 < buffer.line_count() {
                Position::new(line + 1, 0)
            } else {
                Position::new(line, buffer.line_len(line))
            };
            document.replace_range(Range::new(Position::new(line, 0), end), "");
        }

        self.clipboard = text.clone();
        self.status = Status::info(format!(
            "{} {}",
            if remove { "Cut" } else { "Copied" },
            describe(&text)
        ));
        Effect::SetSystemClipboard(text)
    }

    /// Applies a command, returning any I/O the run loop must perform.
    pub fn apply(&mut self, command: &Command) -> Effect {
        match command {
            // --- File ---
            Command::NewFile => {
                self.workspace.new_document();
                self.status = Status::info("New buffer");
                Effect::None
            }
            Command::OpenFile => self.open_prompt(PromptKind::OpenFile),
            Command::SaveFile => match self.workspace.active().path() {
                Some(path) => Effect::SaveFile(path.to_path_buf()),
                None => self.open_prompt(PromptKind::SaveAs),
            },
            Command::SaveFileAs => self.open_prompt(PromptKind::SaveAs),
            Command::CloseFile => {
                let index = self.workspace.active_index();
                match self.workspace.close(index) {
                    Ok(true) => self.status = Status::warning("Closed with unsaved changes"),
                    Ok(false) => self.status = Status::info("Closed"),
                    Err(error) => self.status = Status::error(error.to_string()),
                }
                Effect::None
            }
            Command::Quit => {
                if self.workspace.has_unsaved_changes() {
                    // Never discard work without asking.
                    self.mode = Mode::Prompt(Prompt::new(PromptKind::ConfirmQuit, ""));
                    self.pending_prompt = Some(PromptKind::ConfirmQuit);
                    Effect::None
                } else {
                    self.should_quit = true;
                    Effect::Quit
                }
            }

            // --- Edit ---
            Command::Undo => {
                if !self.workspace.active_mut().undo() {
                    self.status = Status::info("Nothing to undo");
                }
                Effect::None
            }
            Command::Redo => {
                if !self.workspace.active_mut().redo() {
                    self.status = Status::info("Nothing to redo");
                }
                Effect::None
            }
            Command::SelectAll => {
                self.workspace.active_mut().select_all();
                Effect::None
            }
            Command::Indent => {
                self.workspace.active_mut().indent();
                Effect::None
            }
            Command::Dedent => {
                self.workspace.active_mut().dedent();
                Effect::None
            }
            Command::Copy => self.copy(false),
            Command::Cut => self.copy(true),
            Command::Paste => {
                if self.clipboard.is_empty() {
                    self.status = Status::info("Nothing has been copied yet");
                    return Effect::None;
                }
                let text = self.clipboard.clone();
                self.workspace.active_mut().insert(&text);
                self.status = Status::info(format!("Pasted {}", describe(&text)));
                Effect::None
            }

            // --- Navigate ---
            Command::NextPanel => {
                self.focus = self.focus.next();
                Effect::None
            }
            Command::PreviousPanel => {
                self.focus = self.focus.previous();
                Effect::None
            }
            Command::FocusPanel(panel) => {
                self.focus = *panel;
                Effect::None
            }
            Command::NextDocument => {
                self.workspace.next_document();
                Effect::None
            }
            Command::PreviousDocument => {
                self.workspace.previous_document();
                Effect::None
            }
            Command::GoToLine => self.open_prompt(PromptKind::GoToLine),
            Command::GoToAddress => self.open_prompt(PromptKind::GoToAddress),
            Command::GoToDefinition => {
                self.go_to_definition();
                Effect::None
            }
            Command::GoToFirstError => {
                self.go_to_first_error();
                Effect::None
            }

            // --- Search ---
            Command::Search => self.open_prompt(PromptKind::Search),
            Command::Replace => self.open_prompt(PromptKind::Replace),
            Command::SearchNext => {
                self.find(true);
                Effect::None
            }
            Command::SearchPrevious => {
                self.find(false);
                Effect::None
            }

            // --- Build ---
            Command::Build => self.start_build(false),
            Command::BuildWithDebugInfo => self.start_build(true),
            Command::Run => {
                if self.debugger.state() == DebuggerState::Paused {
                    // F5 continues when a session is paused, which is what
                    // every other debugger does.
                    return self.apply(&Command::DebugContinue);
                }
                Effect::Run
            }
            Command::Stop => Effect::StopProgram,

            // --- Debug ---
            Command::DebugStart => {
                if self.debugger.state().can_launch() || self.debugger.state().can_build() {
                    Effect::DebugStart
                } else {
                    self.status = Status::warning(format!(
                        "Cannot start a session while {}",
                        self.debugger.state()
                    ));
                    Effect::None
                }
            }
            Command::DebugContinue => self.require_paused(Effect::DebugContinue),
            Command::DebugInterrupt => {
                if self.debugger.state().can_interrupt() {
                    Effect::DebugInterrupt
                } else {
                    self.status = Status::warning("The program is not running");
                    Effect::None
                }
            }
            Command::StepInstruction => self.require_paused(Effect::Step(StepKind::Instruction)),
            Command::StepOver => self.require_paused(Effect::Step(StepKind::Over)),
            Command::StepLine => self.require_paused(Effect::Step(StepKind::Line)),
            Command::StepOut => self.require_paused(Effect::Step(StepKind::Out)),
            Command::StepBack => self.require_recording(StepKind::Back),
            Command::StepBackOver => self.require_recording(StepKind::BackOver),
            Command::ReverseContinue => self.require_recording(StepKind::ReverseContinue),
            Command::DebugStop => {
                if self.debugger.state().can_stop() {
                    Effect::DebugStop
                } else {
                    self.status = Status::info("No debug session is running");
                    Effect::None
                }
            }
            Command::ToggleBreakpoint => self.toggle_breakpoint(),
            Command::ClearBreakpoints => {
                self.breakpoints.clear();
                self.breakpoint_selected = 0;
                self.status = Status::info("All breakpoints cleared");
                Effect::SyncBreakpoints
            }

            // --- View ---
            Command::CycleRegisterFormat => {
                self.register_format = self.register_format.next();
                self.status = Status::info(format!(
                    "Registers shown as {}",
                    self.register_format.label()
                ));
                Effect::None
            }
            Command::ToggleDisassemblySyntax => {
                self.disassembly_syntax = self.disassembly_syntax.toggled();
                self.status = Status::info(format!(
                    "Disassembly in {} syntax",
                    self.disassembly_syntax.label()
                ));
                Effect::None
            }
            Command::CycleTheme => {
                let next = self.theme.kind().next();
                self.theme.set_kind(next);
                self.settings.appearance.theme = next;
                self.status = Status::info(format!("Theme: {}", next.description()));
                Effect::None
            }
            Command::ToggleLearningMode => {
                self.learning_mode = !self.learning_mode;
                if self.learning_mode {
                    self.focus = Panel::Learn;
                }
                self.status = Status::info(if self.learning_mode {
                    "Learning mode on: ←→ moves through the material, ? jumps to the questions"
                } else {
                    "Learning mode off"
                });
                Effect::None
            }

            // --- Application ---
            Command::OpenPalette => {
                self.mode = Mode::Palette(Palette::open());
                Effect::None
            }
            Command::OpenSyscallFinder => {
                self.focus = Panel::Syscalls;
                Effect::None
            }
            Command::OpenScratchpad => {
                self.focus = Panel::Scratchpad;
                Effect::None
            }
            Command::ShowKeybindings => {
                self.focus = Panel::Output;
                self.output.clear();
                self.output.push("Keyboard shortcuts".to_owned());
                self.output.push(String::new());
                for (binding, command) in self.keymap.sorted_bindings() {
                    self.output
                        .push(format!("  {:<14} {}", binding.to_string(), command.title()));
                }
                Effect::None
            }
        }
    }

    /// Opens a prompt and remembers what to do with the answer.
    fn open_prompt(&mut self, kind: PromptKind) -> Effect {
        let prefill = match kind {
            PromptKind::Search => self.search_query.clone(),
            PromptKind::SaveAs => self
                .workspace
                .active()
                .path()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            _ => String::new(),
        };
        self.mode = Mode::Prompt(Prompt::new(kind, prefill));
        self.pending_prompt = Some(kind);
        Effect::None
    }

    /// Refuses an action that needs a paused program, explaining why.
    fn require_paused(&mut self, effect: Effect) -> Effect {
        if self.debugger.state().can_step() {
            effect
        } else {
            self.status = Status::warning(match self.debugger.state() {
                DebuggerState::Idle | DebuggerState::Ready => {
                    "Start a debug session first (F12)".to_owned()
                }
                DebuggerState::Running => "The program is running; interrupt it first".to_owned(),
                other => format!("Cannot step while {other}"),
            });
            Effect::None
        }
    }

    /// Refuses a reverse step when nothing was recorded, explaining why.
    ///
    /// Stepping backwards only works if execution was recorded, and recording
    /// is a setting. Saying so is far better than the command appearing to do
    /// nothing.
    fn require_recording(&mut self, kind: StepKind) -> Effect {
        if !self.settings.debugger.record {
            self.status =
                Status::warning("Stepping backwards needs recording; set debugger.record = true");
            return Effect::None;
        }
        self.require_paused(Effect::Step(kind))
    }

    /// Cancels whatever overlay is open.
    pub fn cancel_overlay(&mut self) {
        self.mode = Mode::Normal;
        self.pending_prompt = None;
    }

    /// Accepts the current prompt's answer.
    ///
    /// Returns the effect the answer implies. An answer that cannot be used —
    /// a line number that is not a number, an address that does not parse — is
    /// reported in the status bar and the prompt stays open, so the user can
    /// correct it rather than retyping from scratch.
    pub fn accept_prompt(&mut self) -> Effect {
        let Mode::Prompt(prompt) = &self.mode else {
            return Effect::None;
        };
        let text = prompt.text().trim().to_owned();
        let Some(kind) = self.pending_prompt else {
            self.cancel_overlay();
            return Effect::None;
        };

        match kind {
            PromptKind::ConfirmQuit => {
                self.cancel_overlay();
                if text.eq_ignore_ascii_case("y") || text.eq_ignore_ascii_case("yes") {
                    self.should_quit = true;
                    return Effect::Quit;
                }
                self.status = Status::info("Quit cancelled");
                Effect::None
            }
            PromptKind::GoToLine => match text.parse::<usize>() {
                Ok(line) => {
                    self.cancel_overlay();
                    self.workspace.active_mut().go_to_line(line);
                    self.focus = Panel::Editor;
                    Effect::None
                }
                Err(_) => {
                    self.status = Status::error(format!("'{text}' is not a line number"));
                    Effect::None
                }
            },
            PromptKind::GoToAddress => {
                match crate::debugger::memory::evaluate_address(&text, &self.registers) {
                    Ok(address) => {
                        self.cancel_overlay();
                        self.memory_address = Some(address);
                        self.focus = Panel::Memory;
                        Effect::ReadMemory(address)
                    }
                    Err(error) => {
                        self.status = Status::error(error.to_string());
                        Effect::None
                    }
                }
            }
            PromptKind::Search => {
                self.cancel_overlay();
                self.search_query = text;
                self.find(true);
                Effect::None
            }
            PromptKind::Replace => {
                self.cancel_overlay();
                self.replace_all(&text);
                Effect::None
            }
            PromptKind::OpenFile => {
                if text.is_empty() {
                    self.status = Status::error("Enter a file name");
                    return Effect::None;
                }
                self.cancel_overlay();
                Effect::OpenFile(PathBuf::from(text))
            }
            PromptKind::SaveAs => {
                if text.is_empty() {
                    self.status = Status::error("Enter a file name");
                    return Effect::None;
                }
                self.cancel_overlay();
                Effect::SaveFile(PathBuf::from(text))
            }
        }
    }

    /// Runs the highlighted palette command.
    pub fn accept_palette(&mut self) -> Effect {
        let Mode::Palette(palette) = &self.mode else {
            return Effect::None;
        };
        let Some(command) = palette.selected_command().cloned() else {
            self.cancel_overlay();
            return Effect::None;
        };
        self.cancel_overlay();
        self.apply(&command)
    }

    /// Starts a build, refusing while a session holds the executable.
    fn start_build(&mut self, debug: bool) -> Effect {
        if !self.debugger.state().can_build() {
            self.status = Status::warning("Stop the debug session before rebuilding");
            return Effect::None;
        }
        let _ = self.debugger.apply(Transition::BuildStarted);
        self.status = Status::info("Building…");
        Effect::Build { debug }
    }

    /// Records a finished build.
    pub fn finish_build(&mut self, outcome: BuildOutcome) {
        self.diagnostics.clone_from(&outcome.diagnostics);
        self.output = outcome.raw_output().lines().map(str::to_owned).collect();

        if outcome.success {
            let _ = self.debugger.apply(Transition::BuildSucceeded);
            self.status = Status::success(outcome.summary());
        } else {
            let _ = self
                .debugger
                .fail(Transition::BuildFailed, outcome.summary());
            self.status = Status::error(outcome.summary());
            self.focus = Panel::Output;
            self.go_to_first_error();
        }
        self.build = Some(outcome);
    }

    /// Records that a build could not be attempted.
    pub fn fail_build(&mut self, message: impl Into<String>) {
        let message = message.into();
        let _ = self.debugger.fail(Transition::BuildFailed, message.clone());
        self.output = vec![message.clone()];
        self.status = Status::error(message);
        self.focus = Panel::Output;
    }

    /// Records a finished program run.
    pub fn finish_run(&mut self, output: ProcessOutput) {
        self.output.clear();
        if !output.stdout.is_empty() {
            self.output.extend(output.stdout.lines().map(str::to_owned));
        }
        if !output.stderr.is_empty() {
            self.output.extend(output.stderr.lines().map(str::to_owned));
        }
        self.output.push(String::new());
        self.output.push(format!(
            "Program {} in {} ms",
            output.outcome.description(),
            output.duration.as_millis()
        ));

        self.status = if output.is_success() {
            Status::success(format!("Program {}", output.outcome.description()))
        } else {
            Status::error(format!("Program {}", output.outcome.description()))
        };
        self.focus = Panel::Output;
        self.last_run = Some(output);
    }

    /// Moves the cursor to the definition of the symbol under it.
    fn go_to_definition(&mut self) {
        use crate::editor::symbols;

        let document = self.workspace.active();
        let cursor = document.cursor();
        let line = document.buffer().line_or_empty(cursor.line);
        let byte_offset = line
            .char_indices()
            .nth(cursor.column)
            .map_or(line.len(), |(offset, _)| offset);

        let Some((_, word)) = crate::editor::syntax::word_at(line, byte_offset) else {
            self.status = Status::info("No symbol under the cursor");
            return;
        };
        let word = word.to_owned();

        let all = symbols::extract(document.buffer());
        match symbols::find_definition(&all, document.buffer(), &word, cursor) {
            Some(symbol) => {
                let position = symbol.position;
                let name = symbol.name.clone();
                self.workspace
                    .active_mut()
                    .move_cursor(Movement::To(position), SelectionMode::Collapse);
                self.focus = Panel::Editor;
                self.status = Status::info(format!("{name} defined on line {}", position.line + 1));
            }
            None => self.status = Status::warning(format!("'{word}' is not defined in this file")),
        }
    }

    /// Jumps to the first error of the last build.
    fn go_to_first_error(&mut self) {
        let Some(diagnostic) = crate::assembler::diagnostics::first_navigable(&self.diagnostics)
        else {
            self.status = Status::info("No errors to jump to");
            return;
        };
        let Some(line) = diagnostic.buffer_line() else {
            return;
        };
        let column = diagnostic.buffer_column();
        let message = diagnostic.message.clone();

        self.workspace.active_mut().move_cursor(
            Movement::To(crate::editor::Position::new(line, column)),
            SelectionMode::Collapse,
        );
        self.focus = Panel::Editor;
        self.status = Status::error(message);
    }

    /// Moves to the next or previous search match.
    fn find(&mut self, forward: bool) {
        use crate::editor::search::{self, SearchOptions};

        if self.search_query.is_empty() {
            self.status = Status::info("Nothing to search for");
            return;
        }

        let document = self.workspace.active();
        let cursor = document.cursor();
        let options = SearchOptions::new();
        let found = if forward {
            let from = document.buffer().position_after(cursor).unwrap_or(cursor);
            search::find_next(document.buffer(), &self.search_query, from, options)
        } else {
            search::find_previous(document.buffer(), &self.search_query, cursor, options)
        };

        match found {
            Some(hit) => {
                let range = hit.range;
                self.workspace.active_mut().select_range(range);
                self.focus = Panel::Editor;
                self.status = Status::info(format!(
                    "'{}' on line {}",
                    self.search_query,
                    range.start.line + 1
                ));
            }
            None => {
                self.status = Status::warning(format!("'{}' not found", self.search_query));
            }
        }
    }

    /// Replaces every match of the current search query.
    fn replace_all(&mut self, replacement: &str) {
        use crate::editor::search::{self, SearchOptions};

        if self.search_query.is_empty() {
            self.status = Status::info("Search for something first");
            return;
        }

        let edits = search::replace_all(
            self.workspace.active().buffer(),
            &self.search_query,
            replacement,
            SearchOptions::new(),
        );
        let count = edits.len();

        // Applied in the order given: replace_all returns them in reverse
        // document order so earlier edits cannot shift later ones.
        for (range, text) in edits {
            self.workspace.active_mut().replace_range(range, &text);
        }

        self.status = if count == 0 {
            Status::warning(format!("'{}' not found", self.search_query))
        } else {
            Status::success(format!("Replaced {count} occurrence(s)"))
        };
    }

    /// Adds or removes a breakpoint on the cursor's line.
    fn toggle_breakpoint(&mut self) -> Effect {
        if !self.debugger.state().can_edit_breakpoints() {
            self.status = Status::warning("Cannot change breakpoints while the program runs");
            return Effect::None;
        }

        let Some(path) = self.workspace.active().path().map(PathBuf::from) else {
            self.status = Status::warning("Save the file before setting a breakpoint");
            return Effect::None;
        };
        let line = self.workspace.active().cursor().line + 1;

        let added = self.breakpoints.toggle_line(&path, line);
        self.status = Status::info(if added {
            format!("Breakpoint set on line {line}")
        } else {
            format!("Breakpoint removed from line {line}")
        });
        Effect::SyncBreakpoints
    }

    /// The breakpoint locations that still need sending to the debugger.
    pub fn pending_breakpoints(&self) -> Vec<Location> {
        self.breakpoints
            .pending()
            .into_iter()
            .map(|breakpoint| breakpoint.location.clone())
            .collect()
    }

    /// The syscalls matching the current query.
    pub fn matching_syscalls(&self) -> Vec<&crate::syscall::Syscall> {
        self.syscalls.search(&self.syscall_query)
    }

    /// The explanation for the instruction currently in view.
    ///
    /// While the program is stopped this is the instruction at the program
    /// counter, because that is the one whose effects every other panel shows.
    /// The source is taken from the file GDB reported, not from whichever
    /// document happens to be active — otherwise looking at a second file
    /// would silently explain the wrong line. When that file is not open, the
    /// disassembled instruction is used instead, so the panel still works for
    /// code with no source to hand.
    pub fn current_explanation(&self) -> Option<crate::instruction::Explanation> {
        let explanation = self.explanation_source()?;
        Some(if self.debugger.state().can_inspect() {
            explanation.with_values(&self.registers)
        } else {
            explanation
        })
    }

    /// Finds the text to explain and explains it.
    fn explanation_source(&self) -> Option<crate::instruction::Explanation> {
        let explain = |line: &str| crate::instruction::explain_line(&self.instructions, line);

        if self.debugger.state().can_inspect() {
            if let Some((file, line)) = &self.current_line {
                if let Some(document) = self.document_for(file) {
                    let text = document.buffer().line_or_empty(line.saturating_sub(1));
                    return explain(text);
                }
            }

            // No source for the stop location; fall back to the machine code,
            // which is always available.
            if let Some(address) = self.current_address {
                if let Some(line) = self
                    .disassembly
                    .iter()
                    .find(|line| line.instruction.address == address)
                {
                    return explain(&line.instruction.text());
                }
            }
            return None;
        }

        let document = self.workspace.active();
        explain(document.buffer().line_or_empty(document.cursor().line))
    }

    /// The open document for a path GDB reported.
    ///
    /// GDB reports the file name it was given, which may be relative where the
    /// editor holds an absolute path, so the file names are compared when the
    /// full paths do not match.
    fn document_for(&self, path: &std::path::Path) -> Option<&crate::editor::Document> {
        self.workspace.documents().iter().find(|document| {
            document
                .path()
                .is_some_and(|open| open == path || open.file_name() == path.file_name())
        })
    }
}

/// Describes an amount of copied text for the status bar.
fn describe(text: &str) -> String {
    let lines = text.lines().count().max(1);
    if lines > 1 {
        format!("{lines} lines")
    } else {
        let characters = text.trim_end_matches('\n').chars().count();
        format!(
            "{characters} character{}",
            if characters == 1 { "" } else { "s" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = Project::for_file(&dir.path().join("main.asm"));
        // The directory is dropped here; nothing in these tests touches disk.
        App::new(project, Settings::default()).expect("databases load")
    }

    fn app_with_source(text: &str) -> App {
        let mut app = app();
        app.workspace.active_mut().insert(text);
        app.workspace
            .active_mut()
            .move_cursor(Movement::DocumentStart, SelectionMode::Collapse);
        app
    }

    #[test]
    fn a_new_app_starts_in_the_editor_doing_nothing() {
        let app = app();
        assert_eq!(app.focus, Panel::Editor);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.debugger.state(), DebuggerState::Idle);
        assert!(!app.should_quit);
        assert!(app.breakpoints.is_empty());
    }

    #[test]
    fn panel_focus_cycles_and_jumps() {
        let mut app = app();
        assert_eq!(app.apply(&Command::NextPanel), Effect::None);
        assert_eq!(app.focus, Panel::Registers);

        app.apply(&Command::PreviousPanel);
        assert_eq!(app.focus, Panel::Editor);

        app.apply(&Command::FocusPanel(Panel::Memory));
        assert_eq!(app.focus, Panel::Memory);
    }

    #[test]
    fn quitting_with_no_changes_exits_immediately() {
        let mut app = app();
        assert_eq!(app.apply(&Command::Quit), Effect::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn quitting_with_unsaved_changes_asks_first() {
        // Work must never be discarded without a question.
        let mut app = app();
        app.workspace.active_mut().insert_char('x');

        assert_eq!(app.apply(&Command::Quit), Effect::None);
        assert!(!app.should_quit, "must not quit yet");
        assert!(app.mode.is_overlay());
    }

    #[test]
    fn confirming_the_quit_prompt_exits_and_declining_does_not() {
        let mut app = app();
        app.workspace.active_mut().insert_char('x');
        app.apply(&Command::Quit);

        if let Mode::Prompt(prompt) = &mut app.mode {
            prompt.insert('n');
        }
        assert_eq!(app.accept_prompt(), Effect::None);
        assert!(!app.should_quit, "declining must not quit");

        app.apply(&Command::Quit);
        if let Mode::Prompt(prompt) = &mut app.mode {
            prompt.insert('y');
        }
        assert_eq!(app.accept_prompt(), Effect::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn stepping_without_a_session_explains_rather_than_failing_silently() {
        let mut app = app();
        assert_eq!(app.apply(&Command::StepInstruction), Effect::None);
        assert_eq!(app.status.severity, Severity::Warning);
        assert!(
            app.status.text.contains("debug session"),
            "{}",
            app.status.text
        );
    }

    #[test]
    fn stepping_while_paused_produces_the_right_effect() {
        let mut app = app();
        app.debugger.apply(Transition::BuildStarted).expect("build");
        app.debugger.apply(Transition::BuildSucceeded).expect("ok");
        app.debugger
            .apply(Transition::LaunchRequested)
            .expect("launch");
        app.debugger
            .apply(Transition::LaunchSucceeded)
            .expect("running");
        app.debugger.apply(Transition::Stopped).expect("paused");

        assert_eq!(
            app.apply(&Command::StepInstruction),
            Effect::Step(StepKind::Instruction)
        );
        assert_eq!(app.apply(&Command::StepOver), Effect::Step(StepKind::Over));
        assert_eq!(app.apply(&Command::StepLine), Effect::Step(StepKind::Line));
        assert_eq!(app.apply(&Command::StepOut), Effect::Step(StepKind::Out));
    }

    #[test]
    fn run_continues_when_the_program_is_paused() {
        // F5 is "run" from a standstill and "continue" once stopped.
        let mut app = app();
        assert_eq!(app.apply(&Command::Run), Effect::Run);

        app.debugger.apply(Transition::BuildStarted).expect("build");
        app.debugger.apply(Transition::BuildSucceeded).expect("ok");
        app.debugger
            .apply(Transition::LaunchRequested)
            .expect("launch");
        app.debugger
            .apply(Transition::LaunchSucceeded)
            .expect("running");
        app.debugger.apply(Transition::Stopped).expect("paused");

        assert_eq!(app.apply(&Command::Run), Effect::DebugContinue);
    }

    #[test]
    fn building_is_refused_while_a_session_holds_the_executable() {
        let mut app = app();
        app.debugger.apply(Transition::BuildStarted).expect("build");
        app.debugger.apply(Transition::BuildSucceeded).expect("ok");
        app.debugger
            .apply(Transition::LaunchRequested)
            .expect("launch");
        app.debugger
            .apply(Transition::LaunchSucceeded)
            .expect("running");

        assert_eq!(app.apply(&Command::Build), Effect::None);
        assert_eq!(app.status.severity, Severity::Warning);
    }

    #[test]
    fn a_build_moves_the_state_machine_and_reports() {
        let mut app = app();
        assert_eq!(app.apply(&Command::Build), Effect::Build { debug: false });
        assert_eq!(app.debugger.state(), DebuggerState::Building);

        assert_eq!(
            app.apply(&Command::BuildWithDebugInfo),
            Effect::None,
            "already building"
        );
    }

    #[test]
    fn a_breakpoint_needs_a_saved_file() {
        let mut app = app();
        assert_eq!(app.apply(&Command::ToggleBreakpoint), Effect::None);
        assert!(
            app.status.text.contains("Save the file"),
            "{}",
            app.status.text
        );
        assert!(app.breakpoints.is_empty());
    }

    #[test]
    fn toggling_a_breakpoint_adds_then_removes_it() {
        let mut app = app();
        app.workspace.active_mut().set_path("/tmp/main.asm");

        assert_eq!(
            app.apply(&Command::ToggleBreakpoint),
            Effect::SyncBreakpoints
        );
        assert_eq!(app.breakpoints.len(), 1);
        assert!(app.status.text.contains("set"));

        app.apply(&Command::ToggleBreakpoint);
        assert!(app.breakpoints.is_empty());
        assert!(app.status.text.contains("removed"));
    }

    #[test]
    fn breakpoints_cannot_be_changed_while_the_program_runs() {
        let mut app = app();
        app.workspace.active_mut().set_path("/tmp/main.asm");
        app.debugger.apply(Transition::BuildStarted).expect("build");
        app.debugger.apply(Transition::BuildSucceeded).expect("ok");
        app.debugger
            .apply(Transition::LaunchRequested)
            .expect("launch");
        app.debugger
            .apply(Transition::LaunchSucceeded)
            .expect("running");

        assert_eq!(app.apply(&Command::ToggleBreakpoint), Effect::None);
        assert!(app.breakpoints.is_empty());
    }

    #[test]
    fn the_palette_opens_filters_and_runs_a_command() {
        let mut app = app();
        app.apply(&Command::OpenPalette);
        assert!(app.mode.palette().is_some());

        if let Mode::Palette(palette) = &mut app.mode {
            for ch in "next panel".chars() {
                palette.prompt_mut().insert(ch);
            }
            palette.refresh();
        }

        let effect = app.accept_palette();
        assert_eq!(effect, Effect::None);
        assert_eq!(app.focus, Panel::Registers, "the command actually ran");
        assert_eq!(app.mode, Mode::Normal, "the palette closed");
    }

    #[test]
    fn cancelling_an_overlay_returns_to_editing() {
        let mut app = app();
        app.apply(&Command::OpenPalette);
        app.cancel_overlay();
        assert_eq!(app.mode, Mode::Normal);

        app.apply(&Command::GoToLine);
        app.cancel_overlay();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn go_to_line_moves_the_cursor() {
        let mut app = app_with_source("one\ntwo\nthree\nfour");
        app.apply(&Command::GoToLine);
        if let Mode::Prompt(prompt) = &mut app.mode {
            prompt.insert('3');
        }

        assert_eq!(app.accept_prompt(), Effect::None);
        assert_eq!(app.workspace.active().cursor().line, 2);
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn a_bad_line_number_keeps_the_prompt_open_and_says_why() {
        // Retyping the whole thing because of one typo is a bad experience.
        let mut app = app_with_source("one\ntwo");
        app.apply(&Command::GoToLine);
        if let Mode::Prompt(prompt) = &mut app.mode {
            for ch in "abc".chars() {
                prompt.insert(ch);
            }
        }

        app.accept_prompt();
        assert!(app.mode.is_overlay(), "the prompt stays open");
        assert_eq!(app.status.severity, Severity::Error);
    }

    #[test]
    fn a_bad_address_expression_reports_the_reason() {
        let mut app = app();
        app.apply(&Command::GoToAddress);
        if let Mode::Prompt(prompt) = &mut app.mode {
            for ch in "rsp".chars() {
                prompt.insert(ch);
            }
        }

        assert_eq!(app.accept_prompt(), Effect::None);
        assert_eq!(app.status.severity, Severity::Error);
        assert!(app.status.text.contains("paused"), "{}", app.status.text);
    }

    #[test]
    fn a_valid_address_asks_for_the_memory_and_focuses_the_panel() {
        let mut app = app();
        app.apply(&Command::GoToAddress);
        if let Mode::Prompt(prompt) = &mut app.mode {
            for ch in "0x4000b0".chars() {
                prompt.insert(ch);
            }
        }

        assert_eq!(app.accept_prompt(), Effect::ReadMemory(0x0040_00b0));
        assert_eq!(app.focus, Panel::Memory);
        assert_eq!(app.memory_address, Some(0x0040_00b0));
    }

    #[test]
    fn search_finds_a_match_and_selects_it() {
        let mut app = app_with_source("mov rax, 1\nadd rbx, 2\n");
        app.apply(&Command::Search);
        if let Mode::Prompt(prompt) = &mut app.mode {
            for ch in "rbx".chars() {
                prompt.insert(ch);
            }
        }
        app.accept_prompt();

        assert_eq!(app.workspace.active().selected_text(), "rbx");
        assert_eq!(app.focus, Panel::Editor);
    }

    #[test]
    fn searching_for_something_absent_says_so() {
        let mut app = app_with_source("mov rax, 1\n");
        app.search_query = "zzz".to_owned();
        app.apply(&Command::SearchNext);
        assert_eq!(app.status.severity, Severity::Warning);
    }

    #[test]
    fn replace_rewrites_every_match() {
        let mut app = app_with_source("mov rax, rax\nadd rax, 1\n");
        app.search_query = "rax".to_owned();
        app.apply(&Command::Replace);
        if let Mode::Prompt(prompt) = &mut app.mode {
            for ch in "r10".chars() {
                prompt.insert(ch);
            }
        }
        app.accept_prompt();

        assert_eq!(
            app.workspace.active().buffer().to_text(),
            "mov r10, r10\nadd r10, 1\n"
        );
        assert_eq!(app.status.severity, Severity::Success);
    }

    #[test]
    fn go_to_definition_jumps_to_the_label() {
        let source = "_start:\n    nop\n    jmp _start\n";
        let mut app = app_with_source(source);
        // Put the cursor on the reference in the jump.
        app.workspace.active_mut().move_cursor(
            Movement::To(crate::editor::Position::new(2, 9)),
            SelectionMode::Collapse,
        );

        app.apply(&Command::GoToDefinition);
        assert_eq!(app.workspace.active().cursor().line, 0);
        assert!(app.status.text.contains("_start"));
    }

    #[test]
    fn go_to_definition_on_nothing_says_so() {
        let mut app = app_with_source("    nop\n");
        app.workspace.active_mut().move_cursor(
            Movement::To(crate::editor::Position::new(0, 0)),
            SelectionMode::Collapse,
        );
        app.apply(&Command::GoToDefinition);
        assert!(matches!(
            app.status.severity,
            Severity::Info | Severity::Warning
        ));
    }

    #[test]
    fn view_settings_cycle_and_report() {
        let mut app = app();
        let before = app.register_format;
        app.apply(&Command::CycleRegisterFormat);
        assert_ne!(app.register_format, before);

        let syntax = app.disassembly_syntax;
        app.apply(&Command::ToggleDisassemblySyntax);
        assert_ne!(app.disassembly_syntax, syntax);

        let theme = app.theme.kind();
        app.apply(&Command::CycleTheme);
        assert_ne!(app.theme.kind(), theme);
        assert_eq!(app.settings.appearance.theme, app.theme.kind());

        assert!(!app.learning_mode);
        app.apply(&Command::ToggleLearningMode);
        assert!(app.learning_mode);
    }

    #[test]
    fn undo_and_redo_report_when_there_is_nothing_to_do() {
        let mut app = app();
        app.apply(&Command::Undo);
        assert!(app.status.text.contains("Nothing to undo"));
        app.apply(&Command::Redo);
        assert!(app.status.text.contains("Nothing to redo"));
    }

    #[test]
    fn the_shortcut_list_shows_every_binding() {
        let mut app = app();
        app.apply(&Command::ShowKeybindings);
        assert_eq!(app.focus, Panel::Output);
        assert!(app.output.iter().any(|line| line.contains("ctrl+s")));
        assert!(app.output.iter().any(|line| line.contains("F5")));
    }

    #[test]
    fn copying_a_selection_offers_it_to_the_terminal_too() {
        let mut app = app_with_source("mov rax, 1\nmov rdi, 0\n");
        app.workspace
            .active_mut()
            .select_range(Range::new(Position::new(0, 0), Position::new(0, 3)));

        let effect = app.apply(&Command::Copy);
        assert_eq!(app.clipboard, "mov");
        assert_eq!(effect, Effect::SetSystemClipboard("mov".to_owned()));
    }

    #[test]
    fn copying_with_no_selection_takes_the_whole_line() {
        let mut app = app_with_source("mov rax, 1\nmov rdi, 0\n");
        app.apply(&Command::Copy);
        assert_eq!(app.clipboard, "mov rax, 1\n");
    }

    #[test]
    fn cutting_removes_what_it_copied() {
        let mut app = app_with_source("mov rax, 1\nmov rdi, 0\n");
        app.apply(&Command::Cut);

        assert_eq!(app.clipboard, "mov rax, 1\n");
        assert_eq!(
            app.workspace.active().buffer().to_text(),
            "mov rdi, 0\n",
            "the line itself must be gone, not just copied"
        );
    }

    #[test]
    fn pasting_puts_back_exactly_what_was_cut() {
        let mut app = app_with_source("mov rax, 1\nmov rdi, 0\n");
        let before = app.workspace.active().buffer().to_text();

        app.apply(&Command::Cut);
        app.apply(&Command::Paste);

        assert_eq!(app.workspace.active().buffer().to_text(), before);
    }

    #[test]
    fn pasting_an_empty_clipboard_says_so_rather_than_doing_nothing() {
        let mut app = app_with_source("mov rax, 1\n");
        let effect = app.apply(&Command::Paste);

        assert_eq!(effect, Effect::None);
        assert_eq!(app.status.severity, Severity::Info);
        assert!(app.status.text.contains("copied"));
    }

    #[test]
    fn cutting_the_last_line_does_not_run_off_the_end() {
        let mut app = app_with_source("only line");
        app.apply(&Command::Cut);
        assert_eq!(app.workspace.active().buffer().to_text(), "");
    }

    #[test]
    fn an_amount_of_text_is_described_for_the_status_bar() {
        assert_eq!(describe("x"), "1 character");
        assert_eq!(describe("mov"), "3 characters");
        assert_eq!(describe("mov rax, 1\n"), "10 characters");
        assert_eq!(describe("one\ntwo\n"), "2 lines");
    }

    #[test]
    fn a_failed_build_focuses_the_output_and_jumps_to_the_error() {
        use crate::assembler::diagnostics;

        let mut app = app_with_source("one\ntwo\nthree\nfour\nfive\n");
        app.apply(&Command::Build);

        let diagnostics =
            diagnostics::parse_assembler_output("main.asm:3: error: symbol undefined\n");
        app.diagnostics = diagnostics;
        app.fail_build("Build failed: 1 error(s)");

        assert_eq!(app.focus, Panel::Output);
        assert_eq!(app.debugger.state(), DebuggerState::Failed);
        assert_eq!(app.status.severity, Severity::Error);

        app.apply(&Command::GoToFirstError);
        assert_eq!(app.workspace.active().cursor().line, 2, "line 3 is index 2");
    }

    #[test]
    fn a_finished_run_summarises_the_outcome() {
        use crate::process::Outcome;

        let mut app = app();
        app.finish_run(ProcessOutput {
            outcome: Outcome::Signalled(11),
            stdout: "partial\n".to_owned(),
            stderr: String::new(),
            duration: std::time::Duration::from_millis(7),
            command: "./main".to_owned(),
        });

        assert_eq!(app.status.severity, Severity::Error);
        assert!(app.status.text.contains("SIGSEGV"), "{}", app.status.text);
        assert!(app.output.iter().any(|line| line.contains("partial")));
        assert_eq!(app.focus, Panel::Output);
    }

    #[test]
    fn the_explanation_follows_the_cursor_when_not_debugging() {
        let mut app = app_with_source("    add rax, rbx\n    ret\n");
        app.workspace.active_mut().move_cursor(
            Movement::To(crate::editor::Position::new(0, 4)),
            SelectionMode::Collapse,
        );

        let explanation = app.current_explanation().expect("an explanation");
        assert_eq!(explanation.effect, "RAX ← RAX + RBX");
    }

    /// Puts the app in a stopped state at a given file and line.
    fn stopped_at(app: &mut App, file: &str, line: usize) {
        app.debugger.apply(Transition::BuildStarted).expect("build");
        app.debugger.apply(Transition::BuildSucceeded).expect("ok");
        app.debugger
            .apply(Transition::LaunchRequested)
            .expect("launch");
        app.debugger
            .apply(Transition::LaunchSucceeded)
            .expect("running");
        app.debugger.apply(Transition::Stopped).expect("paused");
        app.current_line = Some((PathBuf::from(file), line));
    }

    #[test]
    fn while_stopped_the_explanation_follows_the_program_counter() {
        let mut app = app_with_source("    nop\n    add rax, rbx\n");
        app.workspace.active_mut().set_path("/tmp/main.asm");
        stopped_at(&mut app, "main.asm", 2);

        // The cursor is on line 1, but execution is on line 2.
        let explanation = app.current_explanation().expect("an explanation");
        assert_eq!(explanation.mnemonic, "add");
    }

    #[test]
    fn the_explanation_comes_from_the_file_execution_stopped_in() {
        // Looking at a second file must not make the panel explain a line from
        // it as though it were the one running.
        let mut app = app_with_source("    nop\n    add rax, rbx\n");
        app.workspace.active_mut().set_path("/tmp/main.asm");

        let other = crate::editor::Document::from_file_contents(
            "/tmp/other.asm",
            "    xor rcx, rcx\n    mul rdx\n",
        );
        app.workspace.add_document(other);
        assert_eq!(app.workspace.active().display_name(), "other.asm");

        stopped_at(&mut app, "main.asm", 2);
        let explanation = app.current_explanation().expect("an explanation");
        assert_eq!(
            explanation.mnemonic, "add",
            "it must explain main.asm, not the document being viewed"
        );
    }

    #[test]
    fn without_the_source_the_disassembly_is_explained_instead() {
        use crate::disassembler::{decode, DisassemblyLine, Syntax};

        let mut app = app();
        stopped_at(&mut app, "not-open.asm", 1);
        app.current_address = Some(0x1000);
        // `xor edi, edi`
        app.disassembly = decode(&[0x31, 0xff], 0x1000, Syntax::Intel)
            .into_iter()
            .map(DisassemblyLine::bare)
            .collect();

        let explanation = app.current_explanation().expect("an explanation");
        assert_eq!(explanation.mnemonic, "xor");
    }

    #[test]
    fn with_neither_source_nor_disassembly_there_is_no_explanation() {
        let mut app = app();
        stopped_at(&mut app, "not-open.asm", 1);
        assert!(app.current_explanation().is_none());
    }

    #[test]
    fn a_line_with_no_instruction_has_no_explanation() {
        let app = app_with_source("; just a comment\n");
        assert!(app.current_explanation().is_none());
    }

    #[test]
    fn syscall_search_narrows_as_the_query_changes() {
        let mut app = app();
        assert_eq!(app.matching_syscalls().len(), app.syscalls.len());

        app.syscall_query = "write".to_owned();
        let found = app.matching_syscalls();
        assert!(found.len() < app.syscalls.len());
        assert_eq!(found.first().map(|call| call.name.as_str()), Some("write"));
    }

    #[test]
    fn clearing_breakpoints_empties_the_list_and_syncs() {
        let mut app = app();
        app.workspace.active_mut().set_path("/tmp/main.asm");
        app.apply(&Command::ToggleBreakpoint);
        assert_eq!(app.breakpoints.len(), 1);

        assert_eq!(
            app.apply(&Command::ClearBreakpoints),
            Effect::SyncBreakpoints
        );
        assert!(app.breakpoints.is_empty());
    }

    #[test]
    fn every_command_can_be_applied_without_panicking() {
        // A blunt check that no command path has an unhandled case.
        for command in Command::all() {
            let mut app = app();
            app.workspace.active_mut().set_path("/tmp/main.asm");
            let _ = app.apply(&command);
        }
    }
}

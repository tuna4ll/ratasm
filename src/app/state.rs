//! Application state, and what each command does to it.

use std::path::{Path, PathBuf};

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
use super::page::Page;
use super::panel::Panel;
use super::scroll::ScrollState;

/// Work the run loop must perform on the application's behalf.
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
    /// Write every modified buffer to disk.
    SaveAll,
    /// Write the project file back after its sources changed.
    SaveProject,
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
    /// The page on show.
    pub page: Page,
    /// Which panel has focus. Always one of [`App::page`]'s panels.
    pub focus: Panel,
    /// How far each panel is scrolled.
    pub scroll: ScrollState,
    /// The project's files, as the explorer last found them.
    pub project_files: Vec<PathBuf>,
    /// Which row of the explorer's file list is selected.
    pub explorer_selected: usize,
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
    pub fn new(project: Project, settings: Settings) -> Result<Self, serde_json::Error> {
        let (keymap, _) = settings.keymap();
        let theme = settings.theme();

        Ok(Self {
            workspace: Workspace::new(),
            project,
            keymap,
            theme,
            page: Page::default(),
            focus: Page::default().default_panel(),
            scroll: ScrollState::default(),
            project_files: Vec::new(),
            explorer_selected: 0,
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

    /// Opens a page, moving focus onto it.
    pub fn open_page(&mut self, page: Page) {
        if self.page == page {
            return;
        }
        self.page = page;
        self.focus = page.default_panel();
    }

    /// Focuses a panel, opening a page that shows it if necessary.
    pub fn focus_panel(&mut self, panel: Panel) {
        if !self.page.contains(panel) {
            self.page = Page::for_panel(panel);
        }
        self.focus = panel;
    }

    /// Copies the selection, or the whole line when there is none.
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
            Command::SaveAll => Effect::SaveAll,
            Command::AddToProject => self.add_active_to_project(),
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
                    self.mode = Mode::Prompt(Prompt::new(PromptKind::ConfirmQuit, ""));
                    self.pending_prompt = Some(PromptKind::ConfirmQuit);
                    Effect::None
                } else {
                    self.should_quit = true;
                    Effect::Quit
                }
            }

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

            Command::GoToPage(page) => {
                self.open_page(*page);
                Effect::None
            }
            Command::NextPage => {
                self.open_page(self.page.next());
                Effect::None
            }
            Command::PreviousPage => {
                self.open_page(self.page.previous());
                Effect::None
            }
            Command::NextPanel => {
                self.focus = self.page.next_panel(self.focus);
                Effect::None
            }
            Command::PreviousPanel => {
                self.focus = self.page.previous_panel(self.focus);
                Effect::None
            }
            Command::FocusPanel(panel) => {
                self.focus_panel(*panel);
                Effect::None
            }
            Command::NextDocument => {
                self.workspace.next_document();
                Effect::None
            }
            Command::ScrollUp => self.scroll_focused(-(crate::app::scroll::STEP as isize)),
            Command::ScrollDown => self.scroll_focused(crate::app::scroll::STEP as isize),
            Command::ScrollPageUp => {
                self.scroll.page(self.focus, false);
                Effect::None
            }
            Command::ScrollPageDown => {
                self.scroll.page(self.focus, true);
                Effect::None
            }
            Command::ScrollToTop => {
                self.scroll.to_start(self.focus);
                Effect::None
            }
            Command::ScrollToEnd => {
                self.scroll.to_end(self.focus);
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

            Command::Build => self.start_build(false),
            Command::BuildWithDebugInfo => self.start_build(true),
            Command::Run => {
                if self.debugger.state() == DebuggerState::Paused {
                    return self.apply(&Command::DebugContinue);
                }
                Effect::Run
            }
            Command::Stop => Effect::StopProgram,

            Command::DebugStart => {
                if self.debugger.state().can_launch() || self.debugger.state().can_build() {
                    self.open_page(Page::Debug);
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
                    self.focus_panel(Panel::Learn);
                }
                self.status = Status::info(if self.learning_mode {
                    "Learning mode on: ←→ moves through the material, ? jumps to the questions"
                } else {
                    "Learning mode off"
                });
                Effect::None
            }

            Command::OpenPalette => {
                self.mode = Mode::Palette(Palette::open());
                Effect::None
            }
            Command::OpenSyscallFinder => {
                self.focus_panel(Panel::Syscalls);
                Effect::None
            }
            Command::OpenScratchpad => {
                self.focus_panel(Panel::Scratchpad);
                Effect::None
            }
            Command::ShowKeybindings => {
                self.focus_panel(Panel::Output);
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
                    self.focus_panel(Panel::Editor);
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
                        self.focus_panel(Panel::Memory);
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
            self.focus_panel(Panel::Output);
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
        self.focus_panel(Panel::Output);
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
        self.focus_panel(Panel::Output);
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
        let local = symbols::find_definition(&all, document.buffer(), &word, cursor)
            .map(|symbol| (symbol.position, symbol.kind));

        if let Some((position, kind)) = local {
            if kind.is_definition() {
                self.workspace
                    .active_mut()
                    .move_cursor(Movement::To(position), SelectionMode::Collapse);
                self.focus_panel(Panel::Editor);
                self.status = Status::info(format!("{word} defined on line {}", position.line + 1));
                return;
            }
        }

        if self.definition_in_another_file(&word) {
            return;
        }

        if let Some((position, kind)) = local {
            self.workspace
                .active_mut()
                .move_cursor(Movement::To(position), SelectionMode::Collapse);
            self.focus_panel(Panel::Editor);
            self.status = Status::warning(format!(
                "only the {} for {word} is here, on line {}",
                kind.description(),
                position.line + 1
            ));
            return;
        }
        self.status = Status::warning(format!("'{word}' is not defined in this project"));
    }

    /// Looks for `name` in the other open documents, then in the project's
    fn definition_in_another_file(&mut self, name: &str) -> bool {
        use crate::editor::symbols;

        if name.starts_with('.') {
            return false;
        }

        let active = self.workspace.active_index();
        for index in 0..self.workspace.len() {
            if index == active {
                continue;
            }
            let buffer = self.workspace.documents()[index].buffer();
            let found = symbols::extract(buffer)
                .into_iter()
                .find(|symbol| symbol.kind.is_definition() && symbol.matches_name(name));
            if let Some(symbol) = found {
                self.workspace.set_active(index);
                self.jump_to_definition(name, symbol.position);
                return true;
            }
        }

        for path in self.project.source_paths() {
            if self.index_of_document(&path).is_some() {
                continue;
            }
            let Ok(text) = crate::editor::workspace::read_file(&path) else {
                continue;
            };
            let buffer = crate::editor::TextBuffer::from_text(&text);
            let found = symbols::extract(&buffer)
                .into_iter()
                .find(|symbol| symbol.kind.is_definition() && symbol.matches_name(name));
            let Some(symbol) = found else { continue };

            if self.show_file(&path) {
                self.jump_to_definition(name, symbol.position);
                return true;
            }
        }
        false
    }

    /// Moves the cursor to a definition found in the active document.
    fn jump_to_definition(&mut self, name: &str, position: Position) {
        self.workspace
            .active_mut()
            .move_cursor(Movement::To(position), SelectionMode::Collapse);
        self.focus_panel(Panel::Editor);
        self.status = Status::info(format!(
            "{name} defined in {} on line {}",
            self.workspace.active().display_name(),
            position.line + 1
        ));
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
        let file = diagnostic.file.clone();

        if let Some(file) = file {
            if !self.show_file(&file) {
                self.status = Status::error(format!(
                    "{}: {message}",
                    crate::editor::workspace::display_path(&file)
                ));
                return;
            }
        }

        self.workspace.active_mut().move_cursor(
            Movement::To(crate::editor::Position::new(line, column)),
            SelectionMode::Collapse,
        );
        self.focus_panel(Panel::Editor);
        self.status = Status::error(message);
    }

    /// Re-reads the project's files for the explorer.
    pub fn refresh_project_files(&mut self) {
        let mut files = crate::project::source_files_under(
            self.project.root(),
            &self.project.output_directory(),
            3,
        );

        for path in self.project.source_paths() {
            if !files.contains(&path) {
                files.push(path);
            }
        }
        for document in self.workspace.documents() {
            if let Some(path) = document.path() {
                if !files.iter().any(|known| known == path) {
                    files.push(path.to_path_buf());
                }
            }
        }

        files.sort();
        files.dedup();
        self.explorer_selected = self.explorer_selected.min(files.len().saturating_sub(1));
        self.project_files = files;
    }

    /// Moves the explorer's selection by `rows`, keeping it in the list.
    pub fn move_explorer_selection(&mut self, rows: isize) {
        if self.project_files.is_empty() {
            return;
        }
        let last = self.project_files.len() - 1;
        let target = self.explorer_selected as isize + rows;
        self.explorer_selected = target.clamp(0, last as isize) as usize;
    }

    /// Opens the file the explorer has selected.
    pub fn open_selected_file(&mut self) -> Effect {
        match self.project_files.get(self.explorer_selected) {
            Some(path) => Effect::OpenFile(path.clone()),
            None => Effect::None,
        }
    }

    /// Scrolls the focused panel, if it is one that scrolls this way.
    fn scroll_focused(&mut self, rows: isize) -> Effect {
        if ScrollState::is_scrollable(self.focus) {
            self.scroll.scroll_by(self.focus, rows);
        }
        Effect::None
    }

    /// Adds the active document to the sources the build assembles.
    fn add_active_to_project(&mut self) -> Effect {
        let Some(path) = self.workspace.active().path().map(Path::to_path_buf) else {
            self.status = Status::warning("Save the buffer first; a source needs a file name");
            return Effect::None;
        };

        match self.project.add_source(&path) {
            Ok(false) => {
                self.status = Status::info(format!(
                    "{} is already built with the project",
                    crate::editor::workspace::display_path(&path)
                ));
                Effect::None
            }
            Ok(true) => Effect::SaveProject,
            Err(error) => {
                self.status = Status::error(error.to_string());
                Effect::None
            }
        }
    }

    /// Makes `path` the active document, opening it if necessary.
    pub fn show_file(&mut self, path: &std::path::Path) -> bool {
        if let Some(index) = self.index_of_document(path) {
            self.workspace.set_active(index);
            return true;
        }
        let resolved = self.resolve_reported_path(path);
        match self.workspace.open(&resolved) {
            Ok(index) => {
                self.workspace.set_active(index);
                self.workspace
                    .active_mut()
                    .set_indent_width(self.settings.indent_width());
                true
            }
            Err(_) => false,
        }
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

        if let Some(hit) = found {
            let range = hit.range;
            self.workspace.active_mut().select_range(range);
            self.focus_panel(Panel::Editor);
            self.status = Status::info(format!(
                "'{}' on line {}",
                self.search_query,
                range.start.line + 1
            ));
            return;
        }

        if self.find_in_another_document(forward) {
            return;
        }
        self.status = Status::warning(format!("'{}' not found", self.search_query));
    }

    /// Continues the search in the next open document that has a match.
    fn find_in_another_document(&mut self, forward: bool) -> bool {
        use crate::editor::search::{self, SearchOptions};

        let count = self.workspace.len();
        if count < 2 {
            return false;
        }
        let options = SearchOptions::new();
        let active = self.workspace.active_index();

        for step in 1..count {
            let index = if forward {
                (active + step) % count
            } else {
                (active + count - step) % count
            };

            let buffer = self.workspace.documents()[index].buffer();
            let matches = search::find_all(buffer, &self.search_query, options);
            let hit = if forward {
                matches.first()
            } else {
                matches.last()
            };
            let Some(hit) = hit.map(|hit| hit.range) else {
                continue;
            };

            self.workspace.set_active(index);
            self.workspace.active_mut().select_range(hit);
            self.focus_panel(Panel::Editor);
            self.status = Status::info(format!(
                "'{}' in {} on line {}",
                self.search_query,
                self.workspace.active().display_name(),
                hit.start.line + 1
            ));
            return true;
        }
        false
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

        if self.page == Page::Reference {
            if let Some(explanation) = explain(&self.syscall_query) {
                return Some(explanation);
            }
        }

        if self.page == Page::Learn {
            if !self.scratchpad.snippet.trim().is_empty() {
                return explain(&self.scratchpad.snippet);
            }
            if let Some(question) = self.learning.current_question() {
                return explain(question.instruction);
            }
            return None;
        }

        if self.debugger.state().can_inspect() {
            if let Some((file, line)) = &self.current_line {
                if let Some(document) = self.document_for(file) {
                    let text = document.buffer().line_or_empty(line.saturating_sub(1));
                    return explain(text);
                }
            }

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
    pub fn show_execution(&mut self) {
        self.open_page(Page::Debug);
        self.follow_execution();
    }

    /// Moves the editor to the file and line execution stopped on.
    pub fn follow_execution(&mut self) -> bool {
        let Some((file, line)) = self.current_line.clone() else {
            return false;
        };

        if !self.show_file(&file) {
            return false;
        }
        self.workspace.active_mut().go_to_line(line);
        true
    }

    /// Turns a path a tool reported into one that can be opened.
    pub fn resolve_reported_path(&self, reported: &std::path::Path) -> PathBuf {
        if reported.is_absolute() || reported.is_file() {
            return reported.to_path_buf();
        }
        let rooted = self.project.root().join(reported);
        if rooted.is_file() {
            rooted
        } else {
            reported.to_path_buf()
        }
    }

    /// The index of the open document for a path a tool reported.
    pub fn index_of_document(&self, path: &std::path::Path) -> Option<usize> {
        let documents = self.workspace.documents();
        documents
            .iter()
            .position(|document| document.path() == Some(path))
            .or_else(|| {
                let rooted = self.project.root().join(path);
                documents
                    .iter()
                    .position(|document| document.path() == Some(rooted.as_path()))
            })
            .or_else(|| {
                documents.iter().position(|document| {
                    document
                        .path()
                        .is_some_and(|open| crate::editor::workspace::same_file(open, path))
                })
            })
    }

    fn document_for(&self, path: &std::path::Path) -> Option<&crate::editor::Document> {
        self.index_of_document(path)
            .and_then(|index| self.workspace.documents().get(index))
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
    fn panel_focus_cycles_within_the_page() {
        let mut app = app();
        assert_eq!(app.page, Page::Code);

        assert_eq!(app.apply(&Command::NextPanel), Effect::None);
        assert_eq!(app.focus, Panel::Explorer);
        app.apply(&Command::PreviousPanel);
        assert_eq!(app.focus, Panel::Editor);

        for _ in 0..Page::Code.panels().len() {
            app.apply(&Command::NextPanel);
            assert_eq!(app.page, Page::Code, "Tab must not change page");
        }
        assert_eq!(app.focus, Panel::Editor, "the cycle closed");
    }

    #[test]
    fn focusing_a_panel_opens_a_page_that_shows_it() {
        let mut app = app();
        app.apply(&Command::FocusPanel(Panel::Memory));

        assert_eq!(app.focus, Panel::Memory);
        assert_eq!(app.page, Page::Debug, "memory is not on the code page");
        assert!(app.page.contains(app.focus));
    }

    #[test]
    fn focusing_a_panel_the_page_already_shows_stays_put() {
        let mut app = app();
        app.apply(&Command::GoToPage(Page::Debug));
        app.apply(&Command::FocusPanel(Panel::Output));

        assert_eq!(app.page, Page::Debug);
        assert_eq!(app.focus, Panel::Output);
    }

    #[test]
    fn changing_page_moves_focus_onto_it() {
        let mut app = app();
        for page in Page::ALL {
            app.apply(&Command::GoToPage(page));
            assert_eq!(app.page, page);
            assert!(
                page.contains(app.focus),
                "{page} left focus on {}",
                app.focus
            );
        }

        app.apply(&Command::NextPage);
        assert_eq!(app.page, Page::Code, "the last page wraps to the first");
        app.apply(&Command::PreviousPage);
        assert_eq!(app.page, Page::Reference);
    }

    #[test]
    fn every_command_leaves_focus_on_the_current_page() {
        for command in Command::all() {
            let mut app = app();
            app.apply(&command);
            assert!(
                app.page.contains(app.focus),
                "{command} left focus on {} while showing {}",
                app.focus,
                app.page
            );
        }
    }

    #[test]
    fn stopping_shows_the_machine_and_the_line_it_stopped_on() {
        let mut app = app_with_source("one\ntwo\nthree\nfour\nfive\nsix\n");
        app.workspace
            .active_mut()
            .set_path("/tmp/ratasm-test/main.asm");
        app.current_line = Some((PathBuf::from("/tmp/ratasm-test/main.asm"), 5));

        app.show_execution();

        assert_eq!(app.page, Page::Debug, "the registers are on the debug page");
        assert_eq!(
            app.workspace.active().cursor().line,
            4,
            "the editor should be on the stopped line"
        );
    }

    #[test]
    fn following_execution_into_a_missing_file_does_nothing() {
        let mut app = app();
        app.current_line = Some((PathBuf::from("/nowhere/other.asm"), 3));

        assert!(!app.follow_execution(), "there is no such file to open");
    }

    #[test]
    fn following_execution_opens_a_second_source_that_is_not_yet_open() {
        let dir = tempfile::tempdir().expect("temp dir");
        let other = dir.path().join("util.asm");
        std::fs::write(&other, "one\ntwo\nthree\nfour\n").expect("write");

        let mut app = app();
        app.current_line = Some((other.clone(), 3));

        assert!(app.follow_execution(), "stepping into a file must open it");
        assert_eq!(app.workspace.active().path(), Some(other.as_path()));
        assert_eq!(app.workspace.active().cursor().line, 2);
    }

    #[test]
    fn two_open_files_sharing_a_name_are_not_confused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = dir.path().join("src");
        let second = dir.path().join("lib");
        std::fs::create_dir_all(&first).expect("mkdir");
        std::fs::create_dir_all(&second).expect("mkdir");
        std::fs::write(first.join("main.asm"), "a\n").expect("write");
        std::fs::write(second.join("main.asm"), "b\n").expect("write");

        let mut app = app();
        app.workspace.open(&first.join("main.asm")).expect("open");
        let wanted = app.workspace.open(&second.join("main.asm")).expect("open");

        assert_eq!(
            app.index_of_document(&second.join("main.asm")),
            Some(wanted)
        );
    }

    #[test]
    fn following_execution_with_no_stop_does_nothing() {
        let mut app = app();
        assert!(!app.follow_execution());
    }

    #[test]
    fn the_learn_page_explains_the_instruction_being_asked_about() {
        let mut app = app();
        app.open_page(Page::Learn);
        app.learning.jump_to_questions();

        let question = app
            .learning
            .current_question()
            .expect("jumping lands on a question");
        let explanation = app
            .current_explanation()
            .expect("the question names an instruction");

        assert!(
            question
                .instruction
                .to_lowercase()
                .contains(&explanation.mnemonic.to_lowercase()),
            "explained {} for the question {}",
            explanation.mnemonic,
            question.instruction
        );
    }

    #[test]
    fn the_reference_page_explains_an_instruction_typed_into_the_search_box() {
        let mut app = app();
        app.open_page(Page::Reference);
        app.syscall_query = "imul".to_owned();

        let explanation = app
            .current_explanation()
            .expect("imul is an instruction ratasm knows");
        assert_eq!(explanation.mnemonic.to_lowercase(), "imul");
    }

    #[test]
    fn a_search_that_is_not_an_instruction_leaves_the_panel_alone() {
        let mut app = app();
        app.open_page(Page::Reference);
        app.syscall_query = "write".to_owned();

        assert!(app.current_explanation().is_none());
    }

    #[test]
    fn a_typed_snippet_takes_precedence_over_the_question() {
        let mut app = app();
        app.open_page(Page::Learn);
        app.learning.jump_to_questions();
        app.scratchpad.snippet = "xor rcx, rcx".to_owned();

        let explanation = app.current_explanation().expect("the snippet is explained");
        assert_eq!(explanation.mnemonic.to_lowercase(), "xor");
    }

    #[test]
    fn starting_a_session_opens_the_debug_page() {
        let mut app = app();
        assert_eq!(app.page, Page::Code);
        app.apply(&Command::DebugStart);
        assert_eq!(app.page, Page::Debug);
    }

    #[test]
    fn quitting_with_no_changes_exits_immediately() {
        let mut app = app();
        assert_eq!(app.apply(&Command::Quit), Effect::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn quitting_with_unsaved_changes_asks_first() {
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
        assert_eq!(app.focus, Panel::Explorer, "the command actually ran");
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
        app.workspace.active_mut().move_cursor(
            Movement::To(crate::editor::Position::new(2, 9)),
            SelectionMode::Collapse,
        );

        app.apply(&Command::GoToDefinition);
        assert_eq!(app.workspace.active().cursor().line, 0);
        assert!(app.status.text.contains("_start"));
    }

    #[test]
    fn go_to_definition_follows_an_extern_into_another_open_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let other = dir.path().join("util.asm");
        std::fs::write(&other, "section .text\nhelper:\n    ret\n").expect("write");

        let mut app = app_with_source("extern helper\n_start:\n    call helper\n");
        app.workspace.active_mut().set_path("main.asm");
        app.workspace.open(&other).expect("open");
        app.workspace.set_active(0);
        app.workspace.active_mut().move_cursor(
            Movement::To(crate::editor::Position::new(2, 10)),
            SelectionMode::Collapse,
        );

        app.apply(&Command::GoToDefinition);
        assert_eq!(app.workspace.active().path(), Some(other.as_path()));
        assert_eq!(app.workspace.active().cursor().line, 1);
        assert!(app.status.text.contains("util.asm"), "{}", app.status.text);
    }

    #[test]
    fn the_explorer_finds_the_project_s_files_on_disk() {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = Project::create(dir.path(), "listed").expect("create");
        std::fs::write(dir.path().join("src/util.asm"), "ret\n").expect("write");
        std::fs::create_dir_all(dir.path().join("build")).expect("mkdir");
        std::fs::write(dir.path().join("build/stray.asm"), "ret\n").expect("write");

        let mut app = App::new(project, Settings::default()).expect("databases load");
        app.refresh_project_files();

        let names: Vec<String> = app
            .project_files
            .iter()
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect();

        assert!(names.contains(&"main.asm".to_owned()), "{names:?}");
        assert!(names.contains(&"util.asm".to_owned()), "not yet a source");
        assert!(
            !names.contains(&"stray.asm".to_owned()),
            "the build output is not source: {names:?}"
        );
    }

    #[test]
    fn the_explorer_selection_stays_inside_the_list() {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = Project::create(dir.path(), "select").expect("create");
        std::fs::write(dir.path().join("src/util.asm"), "ret\n").expect("write");

        let mut app = App::new(project, Settings::default()).expect("databases load");
        app.refresh_project_files();
        let count = app.project_files.len();
        assert!(count >= 2);

        app.move_explorer_selection(-5);
        assert_eq!(app.explorer_selected, 0);
        app.move_explorer_selection(100);
        assert_eq!(app.explorer_selected, count - 1);

        assert!(matches!(app.open_selected_file(), Effect::OpenFile(_)));
    }

    #[test]
    fn an_empty_file_list_has_nothing_to_open() {
        let mut app = app();
        app.project_files.clear();
        app.move_explorer_selection(1);
        assert_eq!(app.open_selected_file(), Effect::None);
    }

    #[test]
    fn go_to_definition_opens_a_project_source_that_is_not_open_yet() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut project = Project::create(dir.path(), "cross").expect("create");
        let other = dir.path().join("src/util.asm");
        std::fs::write(&other, "section .text\nhelper:\n    ret\n").expect("write");
        project.add_source(&other).expect("add");

        let mut app = App::new(project, Settings::default()).expect("databases load");
        app.workspace.active_mut().insert("    call helper\n");
        app.workspace.active_mut().set_path("main.asm");
        app.workspace.active_mut().move_cursor(
            Movement::To(crate::editor::Position::new(0, 10)),
            SelectionMode::Collapse,
        );

        app.apply(&Command::GoToDefinition);
        assert_eq!(app.workspace.active().path(), Some(other.as_path()));
    }

    #[test]
    fn a_declaration_is_not_mistaken_for_a_definition() {
        let dir = tempfile::tempdir().expect("temp dir");
        let other = dir.path().join("util.asm");
        std::fs::write(&other, "global helper\nextern helper\n").expect("write");

        let mut app = app_with_source("    call helper\n");
        app.workspace.active_mut().set_path("main.asm");
        app.workspace.open(&other).expect("open");
        app.workspace.set_active(0);
        app.workspace.active_mut().move_cursor(
            Movement::To(crate::editor::Position::new(0, 10)),
            SelectionMode::Collapse,
        );

        app.apply(&Command::GoToDefinition);
        assert_eq!(app.status.severity, Severity::Warning);
    }

    #[test]
    fn search_carries_on_into_the_next_open_document() {
        let dir = tempfile::tempdir().expect("temp dir");
        let other = dir.path().join("util.asm");
        std::fs::write(&other, "one\ntwo\nneedle here\n").expect("write");

        let mut app = app_with_source("nothing\nto see\n");
        app.workspace.active_mut().set_path("main.asm");
        app.workspace.open(&other).expect("open");
        app.workspace.set_active(0);
        app.search_query = "needle".to_owned();

        app.apply(&Command::SearchNext);
        assert_eq!(app.workspace.active().path(), Some(other.as_path()));
        assert_eq!(app.workspace.active().cursor().line, 2);
        assert_eq!(app.status.severity, Severity::Info);
    }

    #[test]
    fn a_query_in_no_open_document_is_still_reported_missing() {
        let mut app = app_with_source("nothing\n");
        app.search_query = "needle".to_owned();
        app.apply(&Command::SearchNext);
        assert_eq!(app.status.severity, Severity::Warning);
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
        app.workspace.active_mut().set_path("main.asm");
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
    fn an_error_in_another_source_jumps_to_that_source() {
        use crate::assembler::diagnostics;

        let mut app = app_with_source("one\ntwo\nthree\nfour\nfive\n");
        app.workspace.active_mut().set_path("main.asm");

        let dir = tempfile::tempdir().expect("temp dir");
        let other = dir.path().join("util.asm");
        std::fs::write(&other, "a\nb\nc\nd\ne\nf\n").expect("write");
        app.workspace.open(&other).expect("open");
        app.workspace.set_active(0);

        app.diagnostics = diagnostics::parse_assembler_output(&format!(
            "{}:4: error: symbol undefined\n",
            other.display()
        ));
        app.apply(&Command::GoToFirstError);

        assert_eq!(
            app.workspace.active().path(),
            Some(other.as_path()),
            "the jump must land in the file the diagnostic names"
        );
        assert_eq!(app.workspace.active().cursor().line, 3);
    }

    #[test]
    fn an_error_in_a_file_that_cannot_be_opened_still_names_it() {
        use crate::assembler::diagnostics;

        let mut app = app_with_source("one\ntwo\n");
        app.diagnostics = diagnostics::parse_assembler_output("/nowhere/gone.asm:2: error: bad\n");
        app.apply(&Command::GoToFirstError);

        assert_eq!(app.status.severity, Severity::Error);
        assert!(app.status.text.contains("gone.asm"), "{}", app.status.text);
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

        let explanation = app.current_explanation().expect("an explanation");
        assert_eq!(explanation.mnemonic, "add");
    }

    #[test]
    fn the_explanation_comes_from_the_file_execution_stopped_in() {
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
        for command in Command::all() {
            let mut app = app();
            app.workspace.active_mut().set_path("/tmp/main.asm");
            let _ = app.apply(&command);
        }
    }
}

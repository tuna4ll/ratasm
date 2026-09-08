//! Every action the application can perform.

pub mod palette;

use std::fmt;

use crate::app::page::Page;
use crate::app::panel::Panel;

pub use palette::{fuzzy_match, FuzzyMatch};

/// A group of related commands, used to organise the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    /// Opening, saving and closing files.
    File,
    /// Modifying text.
    Edit,
    /// Moving around.
    Navigate,
    /// Finding things.
    Search,
    /// Assembling, linking and running.
    Build,
    /// Controlling a debug session.
    Debug,
    /// Changing what is shown.
    View,
    /// Everything else.
    Application,
}

impl Category {
    /// Every category, in palette order.
    pub const ALL: [Category; 8] = [
        Category::File,
        Category::Edit,
        Category::Navigate,
        Category::Search,
        Category::Build,
        Category::Debug,
        Category::View,
        Category::Application,
    ];

    /// The name shown in the palette.
    pub const fn title(self) -> &'static str {
        match self {
            Category::File => "File",
            Category::Edit => "Edit",
            Category::Navigate => "Navigate",
            Category::Search => "Search",
            Category::Build => "Build",
            Category::Debug => "Debug",
            Category::View => "View",
            Category::Application => "Application",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.title())
    }
}

/// An action the application can perform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Command {
    /// Create a new empty buffer.
    NewFile,
    /// Prompt for a file to open.
    OpenFile,
    /// Save the active buffer.
    SaveFile,
    /// Save every modified buffer.
    SaveAll,
    /// Prompt for a name and save the active buffer under it.
    SaveFileAs,
    /// Close the active buffer.
    CloseFile,
    /// Leave the application.
    Quit,

    /// Undo the last change.
    Undo,
    /// Redo the last undone change.
    Redo,
    /// Select the whole buffer.
    SelectAll,
    /// Copy the selection.
    Copy,
    /// Cut the selection.
    Cut,
    /// Paste the clipboard.
    Paste,
    /// Add one indent level.
    Indent,
    /// Remove one indent level.
    Dedent,

    /// Open a named page.
    GoToPage(Page),
    /// Open the next page.
    NextPage,
    /// Open the previous page.
    PreviousPage,
    /// Focus the next panel.
    NextPanel,
    /// Focus the previous panel.
    PreviousPanel,
    /// Focus a named panel.
    FocusPanel(Panel),
    /// Show the next open document.
    NextDocument,
    /// Show the previous open document.
    PreviousDocument,
    /// Scroll the focused panel up a few rows.
    ScrollUp,
    /// Scroll the focused panel down a few rows.
    ScrollDown,
    /// Scroll the focused panel up one screenful.
    ScrollPageUp,
    /// Scroll the focused panel down one screenful.
    ScrollPageDown,
    /// Scroll the focused panel to its first row.
    ScrollToTop,
    /// Scroll the focused panel to its last screenful.
    ScrollToEnd,
    /// Prompt for a line number and go to it.
    GoToLine,
    /// Prompt for an address and show it in the memory panel.
    GoToAddress,
    /// Jump to the definition of the symbol under the cursor.
    GoToDefinition,
    /// Jump to the first error from the last build.
    GoToFirstError,

    /// Open the search prompt.
    Search,
    /// Go to the next match.
    SearchNext,
    /// Go to the previous match.
    SearchPrevious,
    /// Open the replace prompt.
    Replace,

    /// Assemble and link.
    Build,
    /// Assemble and link with debug information.
    BuildWithDebugInfo,
    /// Build if needed, then run.
    Run,
    /// Stop the running program.
    Stop,

    /// Start a debug session.
    DebugStart,
    /// Resume a paused program.
    DebugContinue,
    /// Interrupt a running program.
    DebugInterrupt,
    /// Execute one machine instruction.
    StepInstruction,
    /// Execute one instruction, stepping over calls.
    StepOver,
    /// Execute one source line.
    StepLine,
    /// Run until the current function returns.
    StepOut,
    /// Undo one machine instruction.
    StepBack,
    /// Undo one instruction, over calls.
    StepBackOver,
    /// Run backwards to the previous breakpoint.
    ReverseContinue,
    /// End the debug session.
    DebugStop,
    /// Add or remove a breakpoint on the current line.
    ToggleBreakpoint,
    /// Remove every breakpoint.
    ClearBreakpoints,

    /// Change how register values are displayed.
    CycleRegisterFormat,
    /// Switch between Intel and AT&T disassembly.
    ToggleDisassemblySyntax,
    /// Switch to the next theme.
    CycleTheme,
    /// Show or hide the learning-mode panel.
    ToggleLearningMode,

    /// Open the command palette.
    OpenPalette,
    /// Open the system call reference.
    OpenSyscallFinder,
    /// Open the scratchpad.
    OpenScratchpad,
    /// Show the keyboard shortcuts.
    ShowKeybindings,
}

impl Command {
    /// Every command, in a stable order.
    pub fn all() -> Vec<Command> {
        let mut commands = vec![
            Command::NewFile,
            Command::OpenFile,
            Command::SaveFile,
            Command::SaveFileAs,
            Command::SaveAll,
            Command::CloseFile,
            Command::Quit,
            Command::Undo,
            Command::Redo,
            Command::SelectAll,
            Command::Copy,
            Command::Cut,
            Command::Paste,
            Command::Indent,
            Command::Dedent,
            Command::NextPanel,
            Command::PreviousPanel,
            Command::NextDocument,
            Command::PreviousDocument,
            Command::ScrollUp,
            Command::ScrollDown,
            Command::ScrollPageUp,
            Command::ScrollPageDown,
            Command::ScrollToTop,
            Command::ScrollToEnd,
            Command::GoToLine,
            Command::GoToAddress,
            Command::GoToDefinition,
            Command::GoToFirstError,
            Command::Search,
            Command::SearchNext,
            Command::SearchPrevious,
            Command::Replace,
            Command::Build,
            Command::BuildWithDebugInfo,
            Command::Run,
            Command::Stop,
            Command::DebugStart,
            Command::DebugContinue,
            Command::DebugInterrupt,
            Command::StepInstruction,
            Command::StepOver,
            Command::StepLine,
            Command::StepOut,
            Command::StepBack,
            Command::StepBackOver,
            Command::ReverseContinue,
            Command::DebugStop,
            Command::ToggleBreakpoint,
            Command::ClearBreakpoints,
            Command::CycleRegisterFormat,
            Command::ToggleDisassemblySyntax,
            Command::CycleTheme,
            Command::ToggleLearningMode,
            Command::OpenPalette,
            Command::OpenSyscallFinder,
            Command::OpenScratchpad,
            Command::ShowKeybindings,
        ];
        commands.extend(Page::ALL.map(Command::GoToPage));
        commands.extend(Panel::ALL.map(Command::FocusPanel));
        commands
    }

    /// The stable identifier used in configuration files.
    pub fn id(&self) -> String {
        match self {
            Command::NewFile => "file.new".into(),
            Command::OpenFile => "file.open".into(),
            Command::SaveFile => "file.save".into(),
            Command::SaveAll => "file.save-all".into(),
            Command::SaveFileAs => "file.save-as".into(),
            Command::CloseFile => "file.close".into(),
            Command::Quit => "app.quit".into(),

            Command::Undo => "edit.undo".into(),
            Command::Redo => "edit.redo".into(),
            Command::SelectAll => "edit.select-all".into(),
            Command::Copy => "edit.copy".into(),
            Command::Cut => "edit.cut".into(),
            Command::Paste => "edit.paste".into(),
            Command::Indent => "edit.indent".into(),
            Command::Dedent => "edit.dedent".into(),

            Command::GoToPage(page) => format!("navigate.page.{}", page.id()),
            Command::NextPage => "navigate.next-page".into(),
            Command::PreviousPage => "navigate.previous-page".into(),
            Command::NextPanel => "navigate.next-panel".into(),
            Command::PreviousPanel => "navigate.previous-panel".into(),
            Command::FocusPanel(panel) => format!("navigate.focus.{}", panel.id()),
            Command::NextDocument => "navigate.next-document".into(),
            Command::PreviousDocument => "navigate.previous-document".into(),
            Command::ScrollUp => "navigate.scroll-up".into(),
            Command::ScrollDown => "navigate.scroll-down".into(),
            Command::ScrollPageUp => "navigate.scroll-page-up".into(),
            Command::ScrollPageDown => "navigate.scroll-page-down".into(),
            Command::ScrollToTop => "navigate.scroll-to-top".into(),
            Command::ScrollToEnd => "navigate.scroll-to-end".into(),
            Command::GoToLine => "navigate.go-to-line".into(),
            Command::GoToAddress => "navigate.go-to-address".into(),
            Command::GoToDefinition => "navigate.go-to-definition".into(),
            Command::GoToFirstError => "navigate.go-to-first-error".into(),

            Command::Search => "search.find".into(),
            Command::SearchNext => "search.next".into(),
            Command::SearchPrevious => "search.previous".into(),
            Command::Replace => "search.replace".into(),

            Command::Build => "build.build".into(),
            Command::BuildWithDebugInfo => "build.build-debug".into(),
            Command::Run => "build.run".into(),
            Command::Stop => "build.stop".into(),

            Command::DebugStart => "debug.start".into(),
            Command::DebugContinue => "debug.continue".into(),
            Command::DebugInterrupt => "debug.interrupt".into(),
            Command::StepInstruction => "debug.step-instruction".into(),
            Command::StepOver => "debug.step-over".into(),
            Command::StepLine => "debug.step-line".into(),
            Command::StepOut => "debug.step-out".into(),
            Command::StepBack => "debug.step-back".into(),
            Command::StepBackOver => "debug.step-back-over".into(),
            Command::ReverseContinue => "debug.reverse-continue".into(),
            Command::DebugStop => "debug.stop".into(),
            Command::ToggleBreakpoint => "debug.toggle-breakpoint".into(),
            Command::ClearBreakpoints => "debug.clear-breakpoints".into(),

            Command::CycleRegisterFormat => "view.cycle-register-format".into(),
            Command::ToggleDisassemblySyntax => "view.toggle-disassembly-syntax".into(),
            Command::CycleTheme => "view.cycle-theme".into(),
            Command::ToggleLearningMode => "view.toggle-learning-mode".into(),

            Command::OpenPalette => "app.palette".into(),
            Command::OpenSyscallFinder => "app.syscalls".into(),
            Command::OpenScratchpad => "app.scratchpad".into(),
            Command::ShowKeybindings => "app.keybindings".into(),
        }
    }

    /// The title shown in the palette.
    pub fn title(&self) -> String {
        match self {
            Command::NewFile => "New file".into(),
            Command::OpenFile => "Open file".into(),
            Command::SaveFile => "Save".into(),
            Command::SaveAll => "Save all".into(),
            Command::SaveFileAs => "Save as".into(),
            Command::CloseFile => "Close file".into(),
            Command::Quit => "Quit".into(),

            Command::Undo => "Undo".into(),
            Command::Redo => "Redo".into(),
            Command::SelectAll => "Select all".into(),
            Command::Copy => "Copy".into(),
            Command::Cut => "Cut".into(),
            Command::Paste => "Paste".into(),
            Command::Indent => "Indent".into(),
            Command::Dedent => "Dedent".into(),

            Command::GoToPage(page) => format!("{} page", page.title()),
            Command::NextPage => "Next page".into(),
            Command::PreviousPage => "Previous page".into(),
            Command::NextPanel => "Next panel".into(),
            Command::PreviousPanel => "Previous panel".into(),
            Command::FocusPanel(panel) => format!("Focus {}", panel.title().to_lowercase()),
            Command::ScrollUp => "Scroll up".into(),
            Command::ScrollDown => "Scroll down".into(),
            Command::ScrollPageUp => "Scroll up a page".into(),
            Command::ScrollPageDown => "Scroll down a page".into(),
            Command::ScrollToTop => "Scroll to the top".into(),
            Command::ScrollToEnd => "Scroll to the end".into(),
            Command::NextDocument => "Next document".into(),
            Command::PreviousDocument => "Previous document".into(),
            Command::GoToLine => "Go to line".into(),
            Command::GoToAddress => "Go to address".into(),
            Command::GoToDefinition => "Go to definition".into(),
            Command::GoToFirstError => "Go to first error".into(),

            Command::Search => "Find".into(),
            Command::SearchNext => "Find next".into(),
            Command::SearchPrevious => "Find previous".into(),
            Command::Replace => "Replace".into(),

            Command::Build => "Build".into(),
            Command::BuildWithDebugInfo => "Build with debug info".into(),
            Command::Run => "Run".into(),
            Command::Stop => "Stop the program".into(),

            Command::DebugStart => "Start debugging".into(),
            Command::DebugContinue => "Continue".into(),
            Command::DebugInterrupt => "Interrupt".into(),
            Command::StepInstruction => "Step instruction".into(),
            Command::StepOver => "Step over".into(),
            Command::StepLine => "Step line".into(),
            Command::StepOut => "Step out".into(),
            Command::StepBack => "Step back".into(),
            Command::StepBackOver => "Step back over".into(),
            Command::ReverseContinue => "Run backwards".into(),
            Command::DebugStop => "Stop debugging".into(),
            Command::ToggleBreakpoint => "Toggle breakpoint".into(),
            Command::ClearBreakpoints => "Clear all breakpoints".into(),

            Command::CycleRegisterFormat => "Change register format".into(),
            Command::ToggleDisassemblySyntax => "Toggle Intel/AT&T syntax".into(),
            Command::CycleTheme => "Change theme".into(),
            Command::ToggleLearningMode => "Toggle learning mode".into(),

            Command::OpenPalette => "Command palette".into(),
            Command::OpenSyscallFinder => "Find a system call".into(),
            Command::OpenScratchpad => "Open scratchpad".into(),
            Command::ShowKeybindings => "Show keyboard shortcuts".into(),
        }
    }

    /// A one-line description shown beside the title.
    pub fn description(&self) -> String {
        match self {
            Command::SaveFile => "Write the active buffer to its file".into(),
            Command::SaveAll => "Write every modified buffer to its file".into(),
            Command::Quit => "Leave ratasm, confirming any unsaved changes".into(),
            Command::Build => "Assemble and link the project".into(),
            Command::BuildWithDebugInfo => "Assemble with -g so source-level stepping works".into(),
            Command::Run => "Build if needed, then run the program".into(),
            Command::DebugStart => "Load the program under GDB and stop at the entry".into(),
            Command::DebugContinue => "Resume until the next breakpoint".into(),
            Command::DebugInterrupt => "Stop a program that is still running".into(),
            Command::StepInstruction => "Execute exactly one machine instruction".into(),
            Command::StepOver => "Execute one instruction, running calls to completion".into(),
            Command::StepLine => "Execute one line of source, which may be several \
                                  instructions"
                .into(),
            Command::StepOut => "Run until the current function returns".into(),
            Command::StepBack => "Undo the last instruction and put the registers back".into(),
            Command::StepBackOver => "Undo the last instruction, skipping calls".into(),
            Command::ReverseContinue => "Run backwards to the previous breakpoint".into(),
            Command::ToggleBreakpoint => "Add or remove a breakpoint on the current line".into(),
            Command::GoToAddress => "Show an address in the memory panel; accepts rsp-0x20".into(),
            Command::GoToDefinition => {
                "Jump to where the symbol under the cursor is defined".into()
            }
            Command::GoToFirstError => "Jump to the first error from the last build".into(),
            Command::CycleRegisterFormat => "Hexadecimal, decimal, signed, binary or ASCII".into(),
            Command::ToggleDisassemblySyntax => {
                "Intel puts the destination first; AT&T puts the source first".into()
            }
            Command::GoToPage(page) => page.description().into(),
            Command::OpenSyscallFinder => "Search Linux system calls by name or number".into(),
            Command::OpenScratchpad => "Try an instruction without making a project".into(),
            Command::ToggleLearningMode => "Show explanations and exercises alongside".into(),
            other => other.title(),
        }
    }

    /// The category this command belongs to.
    pub fn category(&self) -> Category {
        match self {
            Command::NewFile
            | Command::OpenFile
            | Command::SaveFile
            | Command::SaveFileAs
            | Command::SaveAll
            | Command::CloseFile => Category::File,

            Command::Undo
            | Command::Redo
            | Command::SelectAll
            | Command::Copy
            | Command::Cut
            | Command::Paste
            | Command::Indent
            | Command::Dedent => Category::Edit,

            Command::GoToPage(_)
            | Command::NextPage
            | Command::PreviousPage
            | Command::NextPanel
            | Command::PreviousPanel
            | Command::FocusPanel(_)
            | Command::NextDocument
            | Command::PreviousDocument
            | Command::GoToLine
            | Command::GoToAddress
            | Command::GoToDefinition
            | Command::GoToFirstError
            | Command::ScrollUp
            | Command::ScrollDown
            | Command::ScrollPageUp
            | Command::ScrollPageDown
            | Command::ScrollToTop
            | Command::ScrollToEnd => Category::Navigate,

            Command::Search | Command::SearchNext | Command::SearchPrevious | Command::Replace => {
                Category::Search
            }

            Command::Build | Command::BuildWithDebugInfo | Command::Run | Command::Stop => {
                Category::Build
            }

            Command::DebugStart
            | Command::DebugContinue
            | Command::DebugInterrupt
            | Command::StepInstruction
            | Command::StepOver
            | Command::StepLine
            | Command::StepOut
            | Command::StepBack
            | Command::StepBackOver
            | Command::ReverseContinue
            | Command::DebugStop
            | Command::ToggleBreakpoint
            | Command::ClearBreakpoints => Category::Debug,

            Command::CycleRegisterFormat
            | Command::ToggleDisassemblySyntax
            | Command::CycleTheme
            | Command::ToggleLearningMode => Category::View,

            Command::Quit
            | Command::OpenPalette
            | Command::OpenSyscallFinder
            | Command::OpenScratchpad
            | Command::ShowKeybindings => Category::Application,
        }
    }

    /// Whether the command opens a prompt for a value.
    pub fn prompts_for_input(&self) -> bool {
        matches!(
            self,
            Command::OpenFile
                | Command::SaveFileAs
                | Command::GoToLine
                | Command::GoToAddress
                | Command::Search
                | Command::Replace
        )
    }

    /// Whether the command needs a live debug session.
    pub fn needs_debug_session(&self) -> bool {
        matches!(
            self,
            Command::DebugContinue
                | Command::DebugInterrupt
                | Command::StepInstruction
                | Command::StepOver
                | Command::StepLine
                | Command::StepOut
                | Command::StepBack
                | Command::StepBackOver
                | Command::ReverseContinue
                | Command::DebugStop
        )
    }

    /// Resolves a command from its identifier.
    pub fn from_id(id: &str) -> Option<Command> {
        let id = id.trim().to_ascii_lowercase();
        Command::all()
            .into_iter()
            .find(|command| command.id() == id)
    }

    /// The text the palette searches, combining title and identifier.
    pub fn search_text(&self) -> String {
        format!("{} {}", self.title(), self.id())
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.title())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_round_trip() {
        for command in Command::all() {
            assert_eq!(
                Command::from_id(&command.id()),
                Some(command.clone()),
                "{} did not round trip",
                command.id()
            );
        }
    }

    #[test]
    fn identifiers_are_unique() {
        let mut ids: Vec<String> = Command::all().iter().map(Command::id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "two commands share an identifier");
    }

    #[test]
    fn identifiers_follow_the_group_dot_name_convention() {
        for command in Command::all() {
            let id = command.id();
            assert!(id.contains('.'), "{id} has no group prefix");
            assert_eq!(id, id.to_ascii_lowercase(), "{id} is not lowercase");
            assert!(
                !id.contains(' ') && !id.contains('_'),
                "{id} should use hyphens, not spaces or underscores"
            );
        }
    }

    #[test]
    fn every_command_has_a_title_and_description() {
        for command in Command::all() {
            assert!(!command.title().is_empty(), "{} has no title", command.id());
            assert!(
                !command.description().is_empty(),
                "{} has no description",
                command.id()
            );
        }
    }

    #[test]
    fn titles_are_unique_so_the_palette_is_unambiguous() {
        let mut titles: Vec<String> = Command::all().iter().map(Command::title).collect();
        let count = titles.len();
        titles.sort();
        titles.dedup();
        assert_eq!(titles.len(), count, "two commands share a title");
    }

    #[test]
    fn every_panel_has_a_focus_command() {
        for panel in Panel::ALL {
            let command = Command::FocusPanel(panel);
            assert!(Command::all().contains(&command), "{panel} has no command");
            assert_eq!(command.category(), Category::Navigate);
        }
    }

    #[test]
    fn every_category_has_at_least_one_command() {
        for category in Category::ALL {
            assert!(
                Command::all().iter().any(|c| c.category() == category),
                "{category} is empty"
            );
        }
    }

    #[test]
    fn stepping_commands_require_a_session() {
        for command in [
            Command::StepInstruction,
            Command::StepOver,
            Command::StepLine,
            Command::StepOut,
            Command::DebugContinue,
        ] {
            assert!(command.needs_debug_session(), "{command}");
        }
        assert!(!Command::DebugStart.needs_debug_session());
        assert!(!Command::Build.needs_debug_session());
        assert!(!Command::SaveFile.needs_debug_session());
    }

    #[test]
    fn commands_that_ask_for_a_value_are_marked() {
        assert!(Command::GoToLine.prompts_for_input());
        assert!(Command::GoToAddress.prompts_for_input());
        assert!(Command::OpenFile.prompts_for_input());
        assert!(!Command::SaveFile.prompts_for_input());
        assert!(!Command::Build.prompts_for_input());
    }

    #[test]
    fn an_unknown_identifier_resolves_to_nothing() {
        assert_eq!(Command::from_id("nonsense.command"), None);
        assert_eq!(Command::from_id(""), None);
    }

    #[test]
    fn identifier_lookup_is_forgiving_about_case_and_space() {
        assert_eq!(Command::from_id("  FILE.SAVE "), Some(Command::SaveFile));
    }

    #[test]
    fn search_text_covers_both_the_title_and_the_identifier() {
        let text = Command::StepOver.search_text();
        assert!(text.contains("Step over"));
        assert!(text.contains("debug.step-over"));
    }

    #[test]
    fn the_documented_shortcuts_all_name_a_real_command() {
        for id in [
            "build.run",
            "build.build",
            "debug.step-instruction",
            "debug.step-over",
            "debug.toggle-breakpoint",
            "debug.step-line",
            "file.save",
            "file.open",
            "app.palette",
            "search.find",
            "navigate.go-to-line",
            "app.syscalls",
            "app.quit",
            "navigate.next-panel",
            "navigate.previous-panel",
        ] {
            assert!(Command::from_id(id).is_some(), "{id} does not exist");
        }
    }

    #[test]
    fn every_command_identifier_is_documented() {
        let docs = include_str!("../../docs/keybindings.md");
        for command in Command::all() {
            let row = format!("| `{}` | {} |", command.id(), command.title());
            assert!(
                docs.contains(&row),
                "docs/keybindings.md is missing the row: {row}"
            );
        }
    }
}

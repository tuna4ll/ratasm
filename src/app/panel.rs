//! The panels the interface is made of, and which one has focus.
//!
//! Panels are named by an enum rather than by index or string so that focus
//! handling, layout and key routing all refer to the same closed set. Adding a
//! panel then means adding a variant, and the compiler points at every place
//! that has to account for it.

use std::fmt;

/// One panel of the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Panel {
    /// The assembly source editor.
    #[default]
    Editor,
    /// General-purpose register values.
    Registers,
    /// The processor flags.
    Flags,
    /// Memory around the stack pointer.
    Stack,
    /// The chain of calls that reached the current instruction.
    CallStack,
    /// A hex dump of an arbitrary address.
    Memory,
    /// Disassembled machine code.
    Disassembly,
    /// The list of breakpoints.
    Breakpoints,
    /// Build and program output.
    Output,
    /// An explanation of the current instruction.
    Explain,
    /// The system call reference.
    Syscalls,
    /// Files and symbols in the project.
    Explorer,
    /// Trying one instruction without making a project.
    Scratchpad,
    /// Lessons and questions.
    Learn,
}

impl Panel {
    /// Every panel, in a fixed order used for cycling with Tab.
    ///
    /// The order follows how the work actually flows: write code, look at the
    /// registers and flags it changed, look at memory, then at the machine
    /// code, then at the supporting references.
    pub const ALL: [Panel; 14] = [
        Panel::Editor,
        Panel::Registers,
        Panel::Flags,
        Panel::Stack,
        Panel::CallStack,
        Panel::Memory,
        Panel::Disassembly,
        Panel::Breakpoints,
        Panel::Output,
        Panel::Explain,
        Panel::Syscalls,
        Panel::Explorer,
        Panel::Scratchpad,
        Panel::Learn,
    ];

    /// The stable identifier used in configuration and command names.
    pub const fn id(self) -> &'static str {
        match self {
            Panel::Editor => "editor",
            Panel::Registers => "registers",
            Panel::Flags => "flags",
            Panel::Stack => "stack",
            Panel::CallStack => "call-stack",
            Panel::Memory => "memory",
            Panel::Disassembly => "disassembly",
            Panel::Breakpoints => "breakpoints",
            Panel::Output => "output",
            Panel::Explain => "explain",
            Panel::Syscalls => "syscalls",
            Panel::Explorer => "explorer",
            Panel::Scratchpad => "scratchpad",
            Panel::Learn => "learn",
        }
    }

    /// The title drawn in the panel's border.
    pub const fn title(self) -> &'static str {
        match self {
            Panel::Editor => "Editor",
            Panel::Registers => "Registers",
            Panel::Flags => "Flags",
            Panel::Stack => "Stack",
            Panel::CallStack => "Call stack",
            Panel::Memory => "Memory",
            Panel::Disassembly => "Disassembly",
            Panel::Breakpoints => "Breakpoints",
            Panel::Output => "Output",
            Panel::Explain => "Explain",
            Panel::Syscalls => "Syscalls",
            Panel::Explorer => "Explorer",
            Panel::Scratchpad => "Scratchpad",
            Panel::Learn => "Learn",
        }
    }

    /// Whether the panel needs a paused debug session to show anything.
    ///
    /// Used to draw a helpful "start a debug session" message instead of an
    /// empty box, which would look like a bug.
    pub const fn needs_debug_session(self) -> bool {
        matches!(
            self,
            Panel::Registers | Panel::Flags | Panel::Stack | Panel::CallStack | Panel::Memory
        )
    }

    /// Whether typing into the panel inserts text.
    ///
    /// The editor consumes ordinary characters; every other panel is free to
    /// use them as single-key shortcuts.
    pub const fn is_text_input(self) -> bool {
        matches!(self, Panel::Editor | Panel::Scratchpad)
    }

    /// Resolves a panel from its identifier.
    pub fn from_id(id: &str) -> Option<Self> {
        let id = id.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|panel| panel.id() == id)
    }

    /// The next panel in cycle order.
    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    /// The previous panel in cycle order.
    pub fn previous(self) -> Self {
        let index = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

impl fmt::Display for Panel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.title())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_round_trip() {
        for panel in Panel::ALL {
            assert_eq!(Panel::from_id(panel.id()), Some(panel));
        }
        assert_eq!(Panel::from_id("REGISTERS"), Some(Panel::Registers));
        assert_eq!(Panel::from_id("  editor "), Some(Panel::Editor));
        assert_eq!(Panel::from_id("nonsense"), None);
    }

    #[test]
    fn identifiers_and_titles_are_unique() {
        let mut ids: Vec<&str> = Panel::ALL.iter().map(|p| p.id()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate panel identifier");

        let mut titles: Vec<&str> = Panel::ALL.iter().map(|p| p.title()).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), count, "duplicate panel title");
    }

    #[test]
    fn cycling_visits_every_panel_and_returns() {
        let mut panel = Panel::Editor;
        let mut seen = Vec::new();
        for _ in 0..Panel::ALL.len() {
            seen.push(panel);
            panel = panel.next();
        }
        assert_eq!(panel, Panel::Editor, "the cycle must wrap");
        for expected in Panel::ALL {
            assert!(seen.contains(&expected), "{expected} was skipped");
        }
    }

    #[test]
    fn previous_undoes_next() {
        for panel in Panel::ALL {
            assert_eq!(panel.next().previous(), panel);
            assert_eq!(panel.previous().next(), panel);
        }
    }

    #[test]
    fn only_the_text_panels_consume_typed_characters() {
        // Every other panel is free to use letters as shortcuts.
        for panel in Panel::ALL {
            let expected = matches!(panel, Panel::Editor | Panel::Scratchpad);
            assert_eq!(panel.is_text_input(), expected, "{panel}");
        }
    }

    #[test]
    fn panels_needing_a_session_are_the_cpu_state_panels() {
        assert!(Panel::Registers.needs_debug_session());
        assert!(Panel::Flags.needs_debug_session());
        assert!(Panel::Stack.needs_debug_session());
        assert!(Panel::Memory.needs_debug_session());

        // These have something to show without a running program.
        assert!(!Panel::Editor.needs_debug_session());
        assert!(!Panel::Disassembly.needs_debug_session());
        assert!(!Panel::Syscalls.needs_debug_session());
        assert!(!Panel::Output.needs_debug_session());
    }

    #[test]
    fn the_editor_is_the_default_focus() {
        assert_eq!(Panel::default(), Panel::Editor);
    }
}

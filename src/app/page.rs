//! The pages the interface is divided into.

use std::fmt;

use super::panel::Panel;

/// One page of the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Page {
    /// Writing and building: the source, the project, the build output.
    #[default]
    Code,
    /// Watching the machine: registers, flags, memory and machine code.
    Debug,
    /// Reading about the machine, and trying single instructions.
    Learn,
    /// Looking things up: system calls and instruction semantics.
    Reference,
}

impl Page {
    /// Every page, in the order they appear in the page bar.
    pub const ALL: [Page; 4] = [Page::Code, Page::Debug, Page::Learn, Page::Reference];

    /// The stable identifier used in configuration and command names.
    pub const fn id(self) -> &'static str {
        match self {
            Page::Code => "code",
            Page::Debug => "debug",
            Page::Learn => "learn",
            Page::Reference => "reference",
        }
    }

    /// The name shown in the page bar.
    pub const fn title(self) -> &'static str {
        match self {
            Page::Code => "Code",
            Page::Debug => "Debug",
            Page::Learn => "Learn",
            Page::Reference => "Reference",
        }
    }

    /// A one-line description, used by the command palette.
    pub const fn description(self) -> &'static str {
        match self {
            Page::Code => "Write and build: source, project files, build output",
            Page::Debug => "Watch the machine: registers, flags, stack, disassembly",
            Page::Learn => "Lessons and questions, with a scratchpad to try them in",
            Page::Reference => "Look up a system call or what an instruction does",
        }
    }

    /// The panels on this page, in Tab order.
    pub const fn panels(self) -> &'static [Panel] {
        match self {
            Page::Code => &[Panel::Editor, Panel::Explorer, Panel::Output],
            Page::Debug => &[
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
            ],
            Page::Learn => &[Panel::Learn, Panel::Scratchpad, Panel::Explain],
            Page::Reference => &[Panel::Syscalls, Panel::Explain],
        }
    }

    /// The panel focused when the page is opened.
    pub const fn default_panel(self) -> Panel {
        match self {
            Page::Code | Page::Debug => Panel::Editor,
            Page::Learn => Panel::Learn,
            Page::Reference => Panel::Syscalls,
        }
    }

    /// Whether `panel` is on this page.
    pub fn contains(self, panel: Panel) -> bool {
        self.panels().contains(&panel)
    }

    /// The page a panel is reached from.
    pub fn for_panel(panel: Panel) -> Page {
        Self::ALL
            .into_iter()
            .find(|page| page.contains(panel))
            .unwrap_or(Page::Code)
    }

    /// The page's position in the bar, counting from one.
    pub fn number(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0) + 1
    }

    /// Resolves a page from its identifier.
    pub fn from_id(id: &str) -> Option<Self> {
        let id = id.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|page| page.id() == id)
    }

    /// The next page, wrapping around.
    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    /// The previous page, wrapping around.
    pub fn previous(self) -> Self {
        let index = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// The panel after `panel` on this page, wrapping around.
    pub fn next_panel(self, panel: Panel) -> Panel {
        let panels = self.panels();
        match panels.iter().position(|p| *p == panel) {
            Some(index) => panels[(index + 1) % panels.len()],
            None => panels[0],
        }
    }

    /// The panel before `panel` on this page, wrapping around.
    pub fn previous_panel(self, panel: Panel) -> Panel {
        let panels = self.panels();
        match panels.iter().position(|p| *p == panel) {
            Some(index) => panels[(index + panels.len() - 1) % panels.len()],
            None => panels[0],
        }
    }
}

impl fmt::Display for Page {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.title())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_panel_is_on_a_page() {
        for panel in Panel::ALL {
            assert!(
                Page::ALL.iter().any(|page| page.contains(panel)),
                "{panel} is on no page"
            );
        }
    }

    #[test]
    fn identifiers_and_titles_round_trip_and_are_unique() {
        for page in Page::ALL {
            assert_eq!(Page::from_id(page.id()), Some(page));
        }
        assert_eq!(Page::from_id("  DEBUG "), Some(Page::Debug));
        assert_eq!(Page::from_id("nonsense"), None);

        let mut ids: Vec<&str> = Page::ALL.iter().map(|p| p.id()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate page identifier");
    }

    #[test]
    fn a_page_opens_on_one_of_its_own_panels() {
        for page in Page::ALL {
            assert!(
                page.contains(page.default_panel()),
                "{page} opens on a panel it does not show"
            );
        }
    }

    #[test]
    fn no_page_is_empty_and_none_repeats_a_panel() {
        for page in Page::ALL {
            let panels = page.panels();
            assert!(!panels.is_empty(), "{page} has no panels");

            let mut seen = panels.to_vec();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), panels.len(), "{page} lists a panel twice");
        }
    }

    #[test]
    fn tab_cycles_within_the_page_and_comes_back() {
        let page = Page::Debug;
        let mut panel = page.default_panel();
        for _ in 0..page.panels().len() {
            assert!(page.contains(panel), "{panel} is not on {page}");
            panel = page.next_panel(panel);
        }
        assert_eq!(panel, page.default_panel(), "the cycle must close");
    }

    #[test]
    fn moving_backwards_undoes_moving_forwards() {
        for page in Page::ALL {
            for panel in page.panels() {
                assert_eq!(page.previous_panel(page.next_panel(*panel)), *panel);
            }
        }
    }

    #[test]
    fn a_panel_from_another_page_lands_on_the_first_one() {
        assert_eq!(
            Page::Reference.next_panel(Panel::Registers),
            Panel::Syscalls
        );
        assert_eq!(
            Page::Reference.previous_panel(Panel::Registers),
            Panel::Syscalls
        );
    }

    #[test]
    fn pages_cycle_in_both_directions() {
        let mut page = Page::Code;
        for _ in 0..Page::ALL.len() {
            page = page.next();
        }
        assert_eq!(page, Page::Code);
        assert_eq!(Page::Code.previous(), Page::Reference);
    }

    #[test]
    fn a_panel_resolves_to_a_page_that_shows_it() {
        for panel in Panel::ALL {
            assert!(Page::for_panel(panel).contains(panel), "{panel}");
        }
    }

    #[test]
    fn the_numbers_match_the_bar_order() {
        assert_eq!(Page::Code.number(), 1);
        assert_eq!(Page::Reference.number(), Page::ALL.len());
    }
}

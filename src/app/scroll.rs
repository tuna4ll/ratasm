//! How far each panel is scrolled.

use std::collections::BTreeMap;

use super::panel::Panel;

/// Rows a wheel notch or an arrow key moves.
pub const STEP: usize = 3;

/// One panel's scroll position and the extent it last drew at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Extent {
    /// The first content row drawn.
    pub offset: usize,
    /// Rows the panel had room for.
    pub viewport: usize,
    /// Rows the content needs.
    pub content: usize,
    /// Whether the panel sticks to the end as content arrives.
    pub following: bool,
}

impl Extent {
    /// The largest offset that still fills the panel.
    pub fn max_offset(self) -> usize {
        self.content.saturating_sub(self.viewport)
    }

    /// Whether the content is taller than the room it has.
    pub fn overflows(self) -> bool {
        self.content > self.viewport
    }

    /// The rows drawn, as a fraction for a scrollbar.
    pub fn thumb(self, track: u16) -> (u16, u16) {
        if !self.overflows() || track == 0 {
            return (0, track);
        }
        let track = usize::from(track);
        let size = (track * self.viewport / self.content).clamp(1, track);
        let travel = track - size;
        let position = if self.max_offset() == 0 {
            0
        } else {
            travel * self.offset / self.max_offset()
        };
        (position as u16, size as u16)
    }
}

/// Scroll offsets for every panel that has one.
#[derive(Debug, Clone, Default)]
pub struct ScrollState {
    panels: BTreeMap<Panel, Extent>,
}

impl ScrollState {
    /// Panels that keep showing their end as new rows arrive.
    pub const fn follows_the_end(panel: Panel) -> bool {
        matches!(panel, Panel::Output)
    }

    /// Whether a panel scrolls through this state at all.
    pub const fn is_scrollable(panel: Panel) -> bool {
        !matches!(panel, Panel::Editor | Panel::Scratchpad)
    }

    /// The extent a panel last drew at.
    pub fn extent(&self, panel: Panel) -> Extent {
        self.panels.get(&panel).copied().unwrap_or_else(|| Extent {
            following: Self::follows_the_end(panel),
            ..Extent::default()
        })
    }

    /// The first content row a panel should draw.
    pub fn offset(&self, panel: Panel) -> usize {
        self.extent(panel).offset
    }

    fn entry(&mut self, panel: Panel) -> &mut Extent {
        self.panels.entry(panel).or_insert(Extent {
            following: Self::follows_the_end(panel),
            ..Extent::default()
        })
    }

    /// Records the room a panel has and the content it holds, clamping it.
    pub fn fit(&mut self, panel: Panel, content: usize, viewport: usize) {
        let entry = self.entry(panel);
        entry.content = content;
        entry.viewport = viewport;
        entry.offset = if entry.following {
            entry.max_offset()
        } else {
            entry.offset.min(entry.max_offset())
        };
    }

    /// Scrolls by `rows`, downwards when positive.
    pub fn scroll_by(&mut self, panel: Panel, rows: isize) {
        let entry = self.entry(panel);
        let offset = entry.offset as isize + rows;
        entry.offset = offset.max(0) as usize;
        entry.offset = entry.offset.min(entry.max_offset());
        entry.following = Self::follows_the_end(panel) && entry.offset == entry.max_offset();
    }

    /// Scrolls by one screenful, downwards when `forward`.
    pub fn page(&mut self, panel: Panel, forward: bool) {
        let rows = self.extent(panel).viewport.max(1) as isize;
        self.scroll_by(panel, if forward { rows } else { -rows });
    }

    /// Jumps to the first row.
    pub fn to_start(&mut self, panel: Panel) {
        let entry = self.entry(panel);
        entry.offset = 0;
        entry.following = false;
    }

    /// Jumps to the last screenful.
    pub fn to_end(&mut self, panel: Panel) {
        let entry = self.entry(panel);
        entry.offset = entry.max_offset();
        entry.following = Self::follows_the_end(panel);
    }

    /// Puts a panel back to the top, for when its content is replaced.
    pub fn reset(&mut self, panel: Panel) {
        let following = Self::follows_the_end(panel);
        self.panels.insert(
            panel,
            Extent {
                following,
                ..Extent::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fitted(panel: Panel, content: usize, viewport: usize) -> ScrollState {
        let mut scroll = ScrollState::default();
        scroll.fit(panel, content, viewport);
        scroll
    }

    #[test]
    fn a_panel_that_fits_never_scrolls() {
        let mut scroll = fitted(Panel::Registers, 6, 10);
        scroll.scroll_by(Panel::Registers, 5);
        assert_eq!(scroll.offset(Panel::Registers), 0);
        assert!(!scroll.extent(Panel::Registers).overflows());
    }

    #[test]
    fn scrolling_stops_at_the_last_screenful() {
        let mut scroll = fitted(Panel::Registers, 18, 8);
        scroll.scroll_by(Panel::Registers, 100);
        assert_eq!(scroll.offset(Panel::Registers), 10);
        scroll.scroll_by(Panel::Registers, -100);
        assert_eq!(scroll.offset(Panel::Registers), 0);
    }

    #[test]
    fn a_shrinking_panel_pulls_its_offset_back() {
        let mut scroll = fitted(Panel::Disassembly, 100, 10);
        scroll.scroll_by(Panel::Disassembly, 80);
        assert_eq!(scroll.offset(Panel::Disassembly), 80);

        scroll.fit(Panel::Disassembly, 100, 40);
        assert_eq!(scroll.offset(Panel::Disassembly), 60);
        scroll.fit(Panel::Disassembly, 12, 40);
        assert_eq!(scroll.offset(Panel::Disassembly), 0);
    }

    #[test]
    fn paging_moves_one_screenful() {
        let mut scroll = fitted(Panel::Memory, 100, 20);
        scroll.page(Panel::Memory, true);
        assert_eq!(scroll.offset(Panel::Memory), 20);
        scroll.page(Panel::Memory, false);
        assert_eq!(scroll.offset(Panel::Memory), 0);
    }

    #[test]
    fn the_output_keeps_showing_its_end_until_the_reader_leaves_it() {
        let mut scroll = fitted(Panel::Output, 50, 10);
        assert_eq!(
            scroll.offset(Panel::Output),
            40,
            "a log is read from the end"
        );

        scroll.fit(Panel::Output, 60, 10);
        assert_eq!(scroll.offset(Panel::Output), 50, "and follows new lines");

        scroll.scroll_by(Panel::Output, -20);
        scroll.fit(Panel::Output, 70, 10);
        assert_eq!(
            scroll.offset(Panel::Output),
            30,
            "but holds where it was put"
        );

        scroll.to_end(Panel::Output);
        scroll.fit(Panel::Output, 80, 10);
        assert_eq!(
            scroll.offset(Panel::Output),
            70,
            "until told to follow again"
        );
    }

    #[test]
    fn other_panels_do_not_chase_their_content() {
        let scroll = fitted(Panel::Disassembly, 50, 10);
        assert_eq!(scroll.offset(Panel::Disassembly), 0);
    }

    #[test]
    fn the_scrollbar_thumb_stays_inside_its_track() {
        let scroll = fitted(Panel::Output, 200, 10);
        for offset in [0, 5, 95, 190] {
            let mut extent = scroll.extent(Panel::Output);
            extent.offset = offset.min(extent.max_offset());
            let (position, size) = extent.thumb(10);
            assert!(size >= 1, "the thumb must be visible");
            assert!(position + size <= 10, "at offset {offset}");
        }
    }

    #[test]
    fn a_panel_that_fits_has_no_thumb() {
        let scroll = fitted(Panel::Stack, 4, 10);
        assert_eq!(scroll.extent(Panel::Stack).thumb(10), (0, 10));
    }

    #[test]
    fn the_text_panels_scroll_with_their_cursor_instead() {
        assert!(!ScrollState::is_scrollable(Panel::Editor));
        assert!(!ScrollState::is_scrollable(Panel::Scratchpad));
        for panel in Panel::ALL {
            if !matches!(panel, Panel::Editor | Panel::Scratchpad) {
                assert!(ScrollState::is_scrollable(panel), "{panel}");
            }
        }
    }
}

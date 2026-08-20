//! Working out where each panel goes.
//!
//! # Three shapes, not a squeeze
//!
//! Below a certain width a panel stops being useful rather than merely small:
//! a register view narrower than `RAX  0x0000000000000000` shows nothing at
//! all. So instead of scaling every panel down, the layout picks one of three
//! arrangements and puts whatever does not fit behind tabs.
//!
//! - **Wide** — the editor with the CPU state beside it and the output below.
//! - **Medium** — the editor with one side column; the rest share a tab strip.
//! - **Narrow** — one panel at a time, all of them in tabs.
//!
//! The focused panel is always visible. That is the invariant the tests pin
//! down, because a focus that lands on a hidden panel makes the keyboard
//! appear to stop working.

use ratatui::layout::{Constraint, Direction, Layout as RatatuiLayout, Rect};

use crate::app::panel::Panel;

/// The narrowest terminal that gets the full three-column arrangement.
pub const WIDE_THRESHOLD: u16 = 120;
/// The narrowest terminal that gets a side column at all.
pub const MEDIUM_THRESHOLD: u16 = 80;
/// The shortest terminal that gets a bottom row.
pub const TALL_THRESHOLD: u16 = 28;
/// Below this, ratasm shows a message instead of an unusable layout.
pub const MINIMUM_WIDTH: u16 = 40;
/// Below this, ratasm shows a message instead of an unusable layout.
pub const MINIMUM_HEIGHT: u16 = 10;

/// Which arrangement was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Everything on screen at once.
    Wide,
    /// The editor plus one side column; the rest in tabs.
    Medium,
    /// One panel at a time.
    Narrow,
    /// Too small to draw anything useful.
    TooSmall,
}

/// Where everything goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The arrangement in use.
    pub mode: LayoutMode,
    /// The panels that have their own area, with it.
    pub panels: Vec<(Panel, Rect)>,
    /// Panels sharing the tab strip, in tab order.
    pub tabbed: Vec<Panel>,
    /// The tab strip's area, when there is one.
    pub tab_bar: Option<Rect>,
    /// The document tab bar at the top.
    pub document_bar: Rect,
    /// The status bar at the bottom.
    pub status_bar: Rect,
}

impl Layout {
    /// The area assigned to a panel, if it has one.
    pub fn area_of(&self, panel: Panel) -> Option<Rect> {
        self.panels
            .iter()
            .find(|(candidate, _)| *candidate == panel)
            .map(|(_, area)| *area)
    }

    /// Whether a panel is currently drawn.
    pub fn is_visible(&self, panel: Panel) -> bool {
        self.area_of(panel).is_some()
    }

    /// The panels that are drawn, in layout order.
    pub fn visible_panels(&self) -> Vec<Panel> {
        self.panels.iter().map(|(panel, _)| *panel).collect()
    }
}

/// Computes the layout for a terminal of `area`, with `focus` focused.
///
/// The focused panel is guaranteed an area in every mode except
/// [`LayoutMode::TooSmall`].
pub fn compute(area: Rect, focus: Panel) -> Layout {
    if area.width < MINIMUM_WIDTH || area.height < MINIMUM_HEIGHT {
        return Layout {
            mode: LayoutMode::TooSmall,
            panels: Vec::new(),
            tabbed: Vec::new(),
            tab_bar: None,
            document_bar: Rect::new(area.x, area.y, area.width, 0),
            status_bar: Rect::new(area.x, area.y, area.width, 0),
        };
    }

    // One row for open documents at the top, one for the status bar at the
    // bottom. Both are always present: losing the status bar would hide the
    // only place errors are reported.
    let rows = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(MINIMUM_HEIGHT - 2),
            Constraint::Length(1),
        ])
        .split(area);
    let (document_bar, body, status_bar) = (rows[0], rows[1], rows[2]);

    if area.width >= WIDE_THRESHOLD && area.height >= TALL_THRESHOLD {
        wide(body, document_bar, status_bar, focus)
    } else if area.width >= MEDIUM_THRESHOLD {
        medium(body, document_bar, status_bar, focus)
    } else {
        narrow(body, document_bar, status_bar, focus)
    }
}

/// Everything visible at once.
fn wide(body: Rect, document_bar: Rect, status_bar: Rect, focus: Panel) -> Layout {
    // Editor and disassembly on the left, CPU state on the right, output
    // across the bottom.
    let columns = RatatuiLayout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(body);

    let left = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(58),
            Constraint::Percentage(25),
            Constraint::Percentage(17),
        ])
        .split(columns[0]);

    let right = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(22),
            Constraint::Percentage(38),
        ])
        .split(columns[1]);

    let side_bottom = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(right[2]);

    // The lower-right slot is shared: whichever of these the user is looking
    // at takes it, so all four remain reachable without a tab strip.
    let shared = [
        Panel::Stack,
        Panel::CallStack,
        Panel::Memory,
        Panel::Syscalls,
        Panel::Explorer,
    ];
    let shown = if shared.contains(&focus) {
        focus
    } else {
        Panel::Stack
    };

    let mut panels = vec![
        (Panel::Editor, left[0]),
        (Panel::Disassembly, left[1]),
        (Panel::Explain, left[2]),
        (Panel::Registers, right[0]),
        (Panel::Flags, right[1]),
        (shown, side_bottom[0]),
        (Panel::Output, side_bottom[1]),
    ];

    // Breakpoints share the output slot unless the user is looking at them.
    if focus == Panel::Breakpoints {
        panels.retain(|(panel, _)| *panel != Panel::Output);
        panels.push((Panel::Breakpoints, side_bottom[1]));
    }

    ensure_focus_visible(&mut panels, focus, side_bottom[0]);

    Layout {
        mode: LayoutMode::Wide,
        panels,
        tabbed: shared.to_vec(),
        tab_bar: None,
        document_bar,
        status_bar,
    }
}

/// The editor plus a side column, with the rest behind tabs.
fn medium(body: Rect, document_bar: Rect, status_bar: Rect, focus: Panel) -> Layout {
    let columns = RatatuiLayout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(body);

    let left = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(4)])
        .split(columns[0]);

    let right = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(4)])
        .split(columns[1]);

    // Everything except the editor and the explanation shares one slot.
    let tabbed: Vec<Panel> = Panel::ALL
        .into_iter()
        .filter(|panel| !matches!(panel, Panel::Editor | Panel::Explain))
        .collect();
    let shown = if tabbed.contains(&focus) {
        focus
    } else {
        Panel::Registers
    };

    let panels = vec![
        (Panel::Editor, left[0]),
        (Panel::Explain, left[1]),
        (shown, right[1]),
    ];

    Layout {
        mode: LayoutMode::Medium,
        panels,
        tabbed,
        tab_bar: Some(right[0]),
        document_bar,
        status_bar,
    }
}

/// One panel at a time.
fn narrow(body: Rect, document_bar: Rect, status_bar: Rect, focus: Panel) -> Layout {
    let rows = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(4)])
        .split(body);

    Layout {
        mode: LayoutMode::Narrow,
        panels: vec![(focus, rows[1])],
        tabbed: Panel::ALL.to_vec(),
        tab_bar: Some(rows[0]),
        document_bar,
        status_bar,
    }
}

/// Guarantees the focused panel has somewhere to draw.
///
/// Without this, focusing a panel the current arrangement does not show would
/// make the keyboard appear to stop responding.
fn ensure_focus_visible(panels: &mut Vec<(Panel, Rect)>, focus: Panel, fallback: Rect) {
    if panels.iter().any(|(panel, _)| *panel == focus) {
        return;
    }
    panels.retain(|(_, area)| *area != fallback);
    panels.push((focus, fallback));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide_area() -> Rect {
        Rect::new(0, 0, 160, 48)
    }

    fn medium_area() -> Rect {
        Rect::new(0, 0, 100, 30)
    }

    fn narrow_area() -> Rect {
        Rect::new(0, 0, 60, 20)
    }

    /// Whether two rectangles share any cell.
    fn overlaps(a: Rect, b: Rect) -> bool {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    }

    #[test]
    fn each_width_selects_its_arrangement() {
        assert_eq!(compute(wide_area(), Panel::Editor).mode, LayoutMode::Wide);
        assert_eq!(
            compute(medium_area(), Panel::Editor).mode,
            LayoutMode::Medium
        );
        assert_eq!(
            compute(narrow_area(), Panel::Editor).mode,
            LayoutMode::Narrow
        );
    }

    #[test]
    fn a_short_but_wide_terminal_does_not_get_the_full_layout() {
        // Height matters as much as width: the wide arrangement needs three
        // stacked rows to be worth anything.
        let layout = compute(Rect::new(0, 0, 160, 20), Panel::Editor);
        assert_ne!(layout.mode, LayoutMode::Wide);
    }

    #[test]
    fn a_tiny_terminal_is_reported_rather_than_drawn_badly() {
        for area in [
            Rect::new(0, 0, 20, 20),
            Rect::new(0, 0, 100, 5),
            Rect::new(0, 0, 1, 1),
            Rect::new(0, 0, 0, 0),
        ] {
            let layout = compute(area, Panel::Editor);
            assert_eq!(layout.mode, LayoutMode::TooSmall, "for {area:?}");
            assert!(layout.panels.is_empty());
        }
    }

    #[test]
    fn the_focused_panel_is_always_visible() {
        // The invariant that keeps the keyboard working: focus can never land
        // somewhere the user cannot see.
        for area in [wide_area(), medium_area(), narrow_area()] {
            for focus in Panel::ALL {
                let layout = compute(area, focus);
                assert!(
                    layout.is_visible(focus),
                    "{focus} is hidden in {:?} at {}x{}",
                    layout.mode,
                    area.width,
                    area.height
                );
            }
        }
    }

    #[test]
    fn panels_never_overlap() {
        for area in [wide_area(), medium_area(), narrow_area()] {
            for focus in Panel::ALL {
                let layout = compute(area, focus);
                let areas: Vec<Rect> = layout.panels.iter().map(|(_, area)| *area).collect();

                for (index, first) in areas.iter().enumerate() {
                    for second in &areas[index + 1..] {
                        assert!(
                            !overlaps(*first, *second),
                            "{first:?} overlaps {second:?} at {}x{} focused on {focus}",
                            area.width,
                            area.height
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_panel_stays_inside_the_terminal() {
        for area in [wide_area(), medium_area(), narrow_area()] {
            for focus in Panel::ALL {
                for (panel, rect) in compute(area, focus).panels {
                    assert!(
                        rect.x + rect.width <= area.width && rect.y + rect.height <= area.height,
                        "{panel} at {rect:?} escapes {}x{}",
                        area.width,
                        area.height
                    );
                }
            }
        }
    }

    #[test]
    fn no_panel_is_given_a_zero_sized_area() {
        // A zero-sized panel draws nothing and looks like a bug.
        for area in [wide_area(), medium_area(), narrow_area()] {
            for focus in Panel::ALL {
                for (panel, rect) in compute(area, focus).panels {
                    assert!(
                        rect.width > 0 && rect.height > 0,
                        "{panel} has no area at {}x{}",
                        area.width,
                        area.height
                    );
                }
            }
        }
    }

    #[test]
    fn the_status_bar_is_always_present_and_one_row_tall() {
        // It is the only place errors are reported, so it never disappears.
        for area in [wide_area(), medium_area(), narrow_area()] {
            let layout = compute(area, Panel::Editor);
            assert_eq!(layout.status_bar.height, 1);
            assert_eq!(layout.status_bar.width, area.width);
            assert_eq!(
                layout.status_bar.y,
                area.height - 1,
                "the status bar sits at the bottom"
            );
        }
    }

    #[test]
    fn the_document_bar_sits_at_the_top() {
        for area in [wide_area(), medium_area(), narrow_area()] {
            let layout = compute(area, Panel::Editor);
            assert_eq!(layout.document_bar.y, 0);
            assert_eq!(layout.document_bar.height, 1);
        }
    }

    #[test]
    fn the_wide_layout_shows_the_editor_and_cpu_state_together() {
        let layout = compute(wide_area(), Panel::Editor);
        for panel in [
            Panel::Editor,
            Panel::Registers,
            Panel::Flags,
            Panel::Disassembly,
            Panel::Explain,
        ] {
            assert!(layout.is_visible(panel), "{panel} should be visible");
        }
        assert!(layout.tab_bar.is_none(), "no tabs are needed when wide");
    }

    #[test]
    fn narrower_layouts_offer_a_tab_strip() {
        for area in [medium_area(), narrow_area()] {
            let layout = compute(area, Panel::Editor);
            assert!(layout.tab_bar.is_some(), "tabs are needed at {area:?}");
            assert!(!layout.tabbed.is_empty());
        }
    }

    #[test]
    fn the_narrow_layout_shows_exactly_one_panel() {
        let layout = compute(narrow_area(), Panel::Registers);
        assert_eq!(layout.panels.len(), 1);
        assert_eq!(layout.visible_panels(), vec![Panel::Registers]);
    }

    #[test]
    fn focusing_a_shared_slot_swaps_which_panel_it_shows() {
        // In the wide layout the lower-right slot is shared; focusing one of
        // its members must bring that member forward.
        let stack = compute(wide_area(), Panel::Stack);
        let memory = compute(wide_area(), Panel::Memory);

        assert!(stack.is_visible(Panel::Stack));
        assert!(memory.is_visible(Panel::Memory));
        assert_eq!(
            stack.area_of(Panel::Stack),
            memory.area_of(Panel::Memory),
            "they share the same slot"
        );
    }

    #[test]
    fn asking_for_a_missing_panel_returns_nothing() {
        let layout = compute(narrow_area(), Panel::Editor);
        assert!(layout.area_of(Panel::Registers).is_none());
        assert!(!layout.is_visible(Panel::Registers));
    }

    #[test]
    fn resizing_across_a_threshold_keeps_the_focus_visible() {
        // The moment a user drags a terminal narrower is exactly when a
        // layout bug would bite.
        for width in (MINIMUM_WIDTH..=WIDE_THRESHOLD + 10).step_by(3) {
            for height in [MINIMUM_HEIGHT, 20, 30, 50] {
                let area = Rect::new(0, 0, width, height);
                for focus in Panel::ALL {
                    let layout = compute(area, focus);
                    if layout.mode == LayoutMode::TooSmall {
                        continue;
                    }
                    assert!(
                        layout.is_visible(focus),
                        "{focus} hidden at {width}x{height}"
                    );
                }
            }
        }
    }
}

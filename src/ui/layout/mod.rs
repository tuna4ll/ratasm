//! Working out where each panel goes.

use ratatui::layout::{Constraint, Direction, Layout as RatatuiLayout, Rect};

use crate::app::page::Page;
use crate::app::panel::Panel;

/// The narrowest terminal that gets a page's full arrangement.
pub const WIDE_THRESHOLD: u16 = 120;
/// The narrowest terminal that gets a main panel and two companions.
pub const COMPACT_THRESHOLD: u16 = 100;
/// The narrowest terminal that gets a side column at all.
pub const MEDIUM_THRESHOLD: u16 = 80;
/// The shortest terminal that gets a page's full arrangement.
pub const TALL_THRESHOLD: u16 = 28;
/// Below this, ratasm shows a message instead of an unusable layout.
pub const MINIMUM_WIDTH: u16 = 40;
/// Below this, ratasm shows a message instead of an unusable layout.
pub const MINIMUM_HEIGHT: u16 = 10;

/// Which arrangement was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// The page's full arrangement.
    Wide,
    /// The main panel, one companion and a shared slot; the rest in tabs.
    Compact,
    /// The page's main panel plus one side slot; the rest in tabs.
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
    /// The page bar at the top.
    pub page_bar: Rect,
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
    /// The panel drawn at a cell, for routing a mouse event.
    pub fn panel_at(&self, x: u16, y: u16) -> Option<Panel> {
        self.panels
            .iter()
            .find(|(_, area)| {
                x >= area.x && x < area.x + area.width && y >= area.y && y < area.y + area.height
            })
            .map(|(panel, _)| *panel)
    }

    /// The tab in the strip at a cell, given the panels sharing it.
    pub fn tab_at(&self, x: u16, y: u16) -> Option<Panel> {
        let bar = self.tab_bar?;
        if y != bar.y || x < bar.x {
            return None;
        }
        let mut offset = bar.x;
        for panel in &self.tabbed {
            let width = panel.title().chars().count() as u16 + 2;
            if x < offset + width {
                return Some(*panel);
            }
            offset += width;
        }
        None
    }
}

/// Computes the layout for a terminal of `area` showing `page`, focused on
pub fn compute(area: Rect, page: Page, focus: Panel) -> Layout {
    if area.width < MINIMUM_WIDTH || area.height < MINIMUM_HEIGHT {
        return Layout {
            mode: LayoutMode::TooSmall,
            panels: Vec::new(),
            tabbed: Vec::new(),
            tab_bar: None,
            page_bar: Rect::new(area.x, area.y, area.width, 0),
            status_bar: Rect::new(area.x, area.y, area.width, 0),
        };
    }

    let bars = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(MINIMUM_HEIGHT - 2),
            Constraint::Length(1),
        ])
        .split(area);
    let (page_bar, body, status_bar) = (bars[0], bars[1], bars[2]);

    let mut layout = if area.width >= WIDE_THRESHOLD && area.height >= TALL_THRESHOLD {
        wide(body, page, focus)
    } else if area.width >= COMPACT_THRESHOLD {
        compact(body, page, focus)
    } else if area.width >= MEDIUM_THRESHOLD {
        medium(body, page, focus)
    } else {
        narrow(body, page, focus)
    };

    layout.page_bar = page_bar;
    layout.status_bar = status_bar;
    layout
}

/// A layout with the bars left for [`compute`] to fill in.
fn assembled(
    mode: LayoutMode,
    panels: Vec<(Panel, Rect)>,
    tabbed: Vec<Panel>,
    tab_bar: Option<Rect>,
) -> Layout {
    Layout {
        mode,
        panels,
        tabbed,
        tab_bar,
        page_bar: Rect::default(),
        status_bar: Rect::default(),
    }
}

/// Stacks `panels` in `area`, giving each at least its minimum height.
fn stack(area: Rect, panels: &[(Panel, u16)], keep: Panel) -> (Vec<(Panel, Rect)>, Vec<Panel>) {
    let mut shown: Vec<(Panel, u16)> = panels.to_vec();
    let mut dropped: Vec<Panel> = Vec::new();

    let needed = |shown: &[(Panel, u16)]| -> u16 {
        shown.iter().map(|(panel, _)| panel.minimum_height()).sum()
    };

    while needed(&shown) > area.height && shown.len() > 1 {
        let Some(index) = shown.iter().rposition(|(panel, _)| *panel != keep) else {
            break;
        };
        dropped.push(shown.remove(index).0);
    }
    dropped.reverse();

    let minimums: u16 = needed(&shown);
    let surplus = area.height.saturating_sub(minimums);
    let total: u16 = shown.iter().map(|(_, weight)| *weight).sum::<u16>().max(1);

    let mut heights: Vec<u16> = shown
        .iter()
        .map(|(panel, weight)| panel.minimum_height() + surplus * weight / total)
        .collect();
    let assigned: u16 = heights.iter().sum();
    if let Some(first) = heights.first_mut() {
        *first += area.height.saturating_sub(assigned);
    }

    let mut placed = Vec::new();
    let mut y = area.y;
    for ((panel, _), height) in shown.iter().zip(heights) {
        let height = height.min(area.y + area.height - y);
        placed.push((*panel, Rect::new(area.x, y, area.width, height)));
        y += height;
    }

    (placed, dropped)
}

/// Splits an area into stacked rows by percentage.
fn rows(area: Rect, parts: &[u16]) -> std::rc::Rc<[Rect]> {
    RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints(
            parts
                .iter()
                .map(|part| Constraint::Percentage(*part))
                .collect::<Vec<_>>(),
        )
        .split(area)
}

/// Splits an area into side-by-side columns by percentage.
fn columns(area: Rect, parts: &[u16]) -> std::rc::Rc<[Rect]> {
    RatatuiLayout::default()
        .direction(Direction::Horizontal)
        .constraints(
            parts
                .iter()
                .map(|part| Constraint::Percentage(*part))
                .collect::<Vec<_>>(),
        )
        .split(area)
}

/// The page's full arrangement.
fn wide(body: Rect, page: Page, focus: Panel) -> Layout {
    match page {
        Page::Code => wide_code(body),
        Page::Debug => wide_debug(body, focus),
        Page::Learn => wide_learn(body),
        Page::Reference => wide_reference(body),
    }
}

/// Source and project files on top, build output across the bottom.
fn wide_code(body: Rect) -> Layout {
    let stacked = rows(body, &[72, 28]);
    let top = columns(stacked[0], &[76, 24]);

    assembled(
        LayoutMode::Wide,
        vec![
            (Panel::Editor, top[0]),
            (Panel::Explorer, top[1]),
            (Panel::Output, stacked[1]),
        ],
        Vec::new(),
        None,
    )
}

/// The source and its machine code on the left, the CPU state on the right.
fn wide_debug(body: Rect, focus: Panel) -> Layout {
    let shared = [
        Panel::Stack,
        Panel::CallStack,
        Panel::Memory,
        Panel::Breakpoints,
    ];
    let shown = if shared.contains(&focus) {
        focus
    } else {
        Panel::Stack
    };

    let split = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(18),
            Constraint::Length((body.height / 5).max(Panel::Output.minimum_height())),
        ])
        .split(body);
    let halves = columns(split[0], &[58, 42]);

    let (left, mut left_out) = stack(
        halves[0],
        &[
            (Panel::Editor, 3),
            (Panel::Disassembly, 2),
            (Panel::Explain, 1),
        ],
        focus,
    );
    let (right, mut right_out) = stack(
        halves[1],
        &[(Panel::Registers, 3), (Panel::Flags, 1), (shown, 2)],
        focus,
    );

    let mut panels = left;
    let mut tab_bar = None;

    for (panel, area) in right {
        if panel != shown {
            panels.push((panel, area));
            continue;
        }
        let carved = RatatuiLayout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(3)])
            .split(area);
        tab_bar = Some(carved[0]);
        panels.push((panel, carved[1]));
    }

    panels.push((Panel::Output, split[1]));

    let mut tabbed = shared.to_vec();
    tabbed.append(&mut left_out);
    tabbed.append(&mut right_out);
    if tab_bar.is_none() {
        tab_bar = Some(Rect::new(split[1].x, split[1].y, split[1].width, 0));
    }

    assembled(LayoutMode::Wide, panels, tabbed, tab_bar)
}

/// Something to read beside somewhere to try it.
fn wide_learn(body: Rect) -> Layout {
    let stacked = rows(body, &[74, 26]);
    let top = columns(stacked[0], &[56, 44]);

    assembled(
        LayoutMode::Wide,
        vec![
            (Panel::Learn, top[0]),
            (Panel::Scratchpad, top[1]),
            (Panel::Explain, stacked[1]),
        ],
        Vec::new(),
        None,
    )
}

/// The syscall table beside the instruction explanation.
fn wide_reference(body: Rect) -> Layout {
    let halves = columns(body, &[58, 42]);

    assembled(
        LayoutMode::Wide,
        vec![(Panel::Syscalls, halves[0]), (Panel::Explain, halves[1])],
        Vec::new(),
        None,
    )
}

/// The main panel, its closest companion, and a shared slot for the rest.
fn compact(body: Rect, page: Page, focus: Panel) -> Layout {
    let main = page.default_panel();
    let others: Vec<Panel> = page
        .panels()
        .iter()
        .copied()
        .filter(|panel| *panel != main)
        .collect();

    if others.is_empty() {
        return assembled(LayoutMode::Compact, vec![(main, body)], Vec::new(), None);
    }

    let halves = columns(body, &[56, 44]);
    let companion = others[0];
    let tabbed: Vec<Panel> = others.iter().copied().skip(1).collect();
    if tabbed.is_empty() {
        return assembled(
            LayoutMode::Compact,
            vec![(main, halves[0]), (companion, halves[1])],
            Vec::new(),
            None,
        );
    }

    let right = rows(halves[1], &[46, 54]);
    let shared = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(right[1]);

    let shown = if tabbed.contains(&focus) {
        focus
    } else {
        tabbed[0]
    };

    assembled(
        LayoutMode::Compact,
        vec![(main, halves[0]), (companion, right[0]), (shown, shared[1])],
        tabbed,
        Some(shared[0]),
    )
}

/// The page's main panel with one side slot, the rest behind tabs.
fn medium(body: Rect, page: Page, focus: Panel) -> Layout {
    let main = page.default_panel();
    let tabbed: Vec<Panel> = page
        .panels()
        .iter()
        .copied()
        .filter(|panel| *panel != main)
        .collect();

    if tabbed.is_empty() {
        return assembled(LayoutMode::Medium, vec![(main, body)], Vec::new(), None);
    }

    let halves = columns(body, &[58, 42]);
    let side = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(4)])
        .split(halves[1]);

    let shown = if tabbed.contains(&focus) {
        focus
    } else {
        tabbed[0]
    };

    assembled(
        LayoutMode::Medium,
        vec![(main, halves[0]), (shown, side[1])],
        tabbed,
        Some(side[0]),
    )
}

/// One panel at a time, with every panel on the page in the tab strip.
fn narrow(body: Rect, page: Page, focus: Panel) -> Layout {
    let stacked = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(4)])
        .split(body);

    let shown = if page.contains(focus) {
        focus
    } else {
        page.default_panel()
    };

    assembled(
        LayoutMode::Narrow,
        vec![(shown, stacked[1])],
        page.panels().to_vec(),
        Some(stacked[0]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide_area() -> Rect {
        Rect::new(0, 0, 160, 48)
    }

    fn compact_area() -> Rect {
        Rect::new(0, 0, 108, 26)
    }

    fn medium_area() -> Rect {
        Rect::new(0, 0, 90, 26)
    }

    fn narrow_area() -> Rect {
        Rect::new(0, 0, 60, 20)
    }

    /// Every (page, focus) pair, which is the space these invariants cover.
    fn every_focus() -> Vec<(Page, Panel)> {
        Page::ALL
            .into_iter()
            .flat_map(|page| page.panels().iter().map(move |panel| (page, *panel)))
            .collect()
    }

    /// Whether two rectangles share any cell.
    fn overlaps(a: Rect, b: Rect) -> bool {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    }

    #[test]
    fn each_width_selects_its_arrangement() {
        assert_eq!(
            compute(wide_area(), Page::Debug, Panel::Editor).mode,
            LayoutMode::Wide
        );
        assert_eq!(
            compute(compact_area(), Page::Debug, Panel::Editor).mode,
            LayoutMode::Compact
        );
        assert_eq!(
            compute(medium_area(), Page::Debug, Panel::Editor).mode,
            LayoutMode::Medium
        );
        assert_eq!(
            compute(narrow_area(), Page::Debug, Panel::Editor).mode,
            LayoutMode::Narrow
        );
    }

    #[test]
    fn a_short_but_wide_terminal_does_not_get_the_full_layout() {
        let layout = compute(Rect::new(0, 0, 160, 20), Page::Debug, Panel::Editor);
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
            let layout = compute(area, Page::Code, Panel::Editor);
            assert_eq!(layout.mode, LayoutMode::TooSmall, "for {area:?}");
            assert!(layout.panels.is_empty());
        }
    }

    #[test]
    fn the_focused_panel_is_always_visible() {
        for area in [wide_area(), compact_area(), medium_area(), narrow_area()] {
            for (page, focus) in every_focus() {
                let layout = compute(area, page, focus);
                assert!(
                    layout.is_visible(focus),
                    "{focus} is hidden on {page} in {:?} at {}x{}",
                    layout.mode,
                    area.width,
                    area.height
                );
            }
        }
    }

    #[test]
    fn only_the_pages_own_panels_are_drawn() {
        for area in [wide_area(), compact_area(), medium_area(), narrow_area()] {
            for (page, focus) in every_focus() {
                for panel in compute(area, page, focus).visible_panels() {
                    assert!(page.contains(panel), "{panel} is not on {page}");
                }
            }
        }
    }

    #[test]
    fn every_panel_on_a_page_is_reachable_when_wide() {
        for page in Page::ALL {
            let layout = compute(wide_area(), page, page.default_panel());
            for panel in page.panels() {
                assert!(
                    layout.is_visible(*panel) || layout.tabbed.contains(panel),
                    "{panel} is neither shown nor tabbed on {page}"
                );
            }
        }
    }

    #[test]
    fn no_drawn_panel_is_squeezed_below_what_it_can_use() {
        for area in [
            Rect::new(0, 0, 120, 30),
            Rect::new(0, 0, 140, 36),
            wide_area(),
            compact_area(),
        ] {
            for (page, focus) in every_focus() {
                for (panel, rect) in compute(area, page, focus).panels {
                    assert!(
                        rect.height + 1 >= panel.minimum_height(),
                        "{panel} got {} rows of the {} it needs on {page} at {}x{}",
                        rect.height,
                        panel.minimum_height(),
                        area.width,
                        area.height
                    );
                }
            }
        }
    }

    #[test]
    fn one_row_no_longer_costs_five_panels() {
        let tall = compute(Rect::new(0, 0, 120, 28), Page::Debug, Panel::Editor);
        let short = compute(Rect::new(0, 0, 120, 27), Page::Debug, Panel::Editor);

        assert_eq!(tall.mode, LayoutMode::Wide);
        assert_eq!(short.mode, LayoutMode::Compact);
        assert!(
            short.panels.len() >= 3,
            "one row short should not leave {} panels",
            short.panels.len()
        );
        assert!(short.is_visible(Panel::Registers));
    }

    #[test]
    fn a_dropped_panel_is_reachable_through_the_tab_strip() {
        for area in [Rect::new(0, 0, 120, 28), wide_area(), compact_area()] {
            for (page, focus) in every_focus() {
                let layout = compute(area, page, focus);
                for panel in page.panels() {
                    assert!(
                        layout.is_visible(*panel) || layout.tabbed.contains(panel),
                        "{panel} is neither drawn nor tabbed on {page} at {}x{}",
                        area.width,
                        area.height
                    );
                }
            }
        }
    }

    #[test]
    fn panels_never_overlap() {
        for area in [wide_area(), compact_area(), medium_area(), narrow_area()] {
            for (page, focus) in every_focus() {
                let layout = compute(area, page, focus);
                let areas: Vec<Rect> = layout.panels.iter().map(|(_, area)| *area).collect();

                for (index, first) in areas.iter().enumerate() {
                    for second in &areas[index + 1..] {
                        assert!(
                            !overlaps(*first, *second),
                            "{first:?} overlaps {second:?} on {page} at {}x{} focused on {focus}",
                            area.width,
                            area.height
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn nothing_overlaps_the_bars() {
        for area in [wide_area(), compact_area(), medium_area(), narrow_area()] {
            for (page, focus) in every_focus() {
                let layout = compute(area, page, focus);
                for (panel, rect) in &layout.panels {
                    assert!(
                        !overlaps(*rect, layout.page_bar),
                        "{panel} covers the page bar"
                    );
                    assert!(
                        !overlaps(*rect, layout.status_bar),
                        "{panel} covers the status bar"
                    );
                }
            }
        }
    }

    #[test]
    fn every_panel_stays_inside_the_terminal() {
        for area in [wide_area(), compact_area(), medium_area(), narrow_area()] {
            for (page, focus) in every_focus() {
                for (panel, rect) in compute(area, page, focus).panels {
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
        for area in [wide_area(), compact_area(), medium_area(), narrow_area()] {
            for (page, focus) in every_focus() {
                for (panel, rect) in compute(area, page, focus).panels {
                    assert!(
                        rect.width > 0 && rect.height > 0,
                        "{panel} has no area on {page} at {}x{}",
                        area.width,
                        area.height
                    );
                }
            }
        }
    }

    #[test]
    fn the_status_bar_is_always_present_and_one_row_tall() {
        for area in [wide_area(), compact_area(), medium_area(), narrow_area()] {
            let layout = compute(area, Page::Code, Panel::Editor);
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
    fn the_page_bar_sits_at_the_top() {
        for area in [wide_area(), compact_area(), medium_area(), narrow_area()] {
            let layout = compute(area, Page::Code, Panel::Editor);
            assert_eq!(layout.page_bar.y, 0);
            assert_eq!(layout.page_bar.height, 1);
            assert_eq!(layout.page_bar.width, area.width);
        }
    }

    #[test]
    fn the_debug_page_shows_the_source_and_the_cpu_together() {
        let layout = compute(wide_area(), Page::Debug, Panel::Editor);
        for panel in [
            Panel::Editor,
            Panel::Registers,
            Panel::Flags,
            Panel::Disassembly,
            Panel::Explain,
            Panel::Output,
        ] {
            assert!(layout.is_visible(panel), "{panel} should be visible");
        }
    }

    #[test]
    fn the_code_page_gives_the_editor_most_of_the_screen() {
        let area = wide_area();
        let editor = compute(area, Page::Code, Panel::Editor)
            .area_of(Panel::Editor)
            .expect("the editor is on the code page");
        let debugging = compute(area, Page::Debug, Panel::Editor)
            .area_of(Panel::Editor)
            .expect("the editor is on the debug page");

        assert!(editor.width > debugging.width);
        assert!(
            u32::from(editor.width) * u32::from(editor.height) > u32::from(area.width) / 2 * 20,
            "the editor got {}x{}",
            editor.width,
            editor.height
        );
    }

    #[test]
    fn the_learn_page_puts_the_scratchpad_beside_the_material() {
        let layout = compute(wide_area(), Page::Learn, Panel::Learn);
        let learn = layout.area_of(Panel::Learn).expect("the material");
        let scratchpad = layout.area_of(Panel::Scratchpad).expect("the scratchpad");

        assert_eq!(learn.y, scratchpad.y, "side by side, not stacked");
        assert!(scratchpad.x >= learn.x + learn.width);
        assert!(layout.is_visible(Panel::Explain));
    }

    #[test]
    fn narrower_layouts_offer_a_tab_strip() {
        for area in [medium_area(), narrow_area()] {
            let layout = compute(area, Page::Debug, Panel::Editor);
            assert!(layout.tab_bar.is_some(), "tabs are needed at {area:?}");
            assert!(!layout.tabbed.is_empty());
        }
    }

    #[test]
    fn the_narrow_layout_shows_exactly_one_panel() {
        let layout = compute(narrow_area(), Page::Debug, Panel::Registers);
        assert_eq!(layout.panels.len(), 1);
        assert_eq!(layout.visible_panels(), vec![Panel::Registers]);
    }

    #[test]
    fn focusing_a_shared_slot_swaps_which_panel_it_shows() {
        let stack = compute(wide_area(), Page::Debug, Panel::Stack);
        let memory = compute(wide_area(), Page::Debug, Panel::Memory);

        assert!(stack.is_visible(Panel::Stack));
        assert!(memory.is_visible(Panel::Memory));
        assert_eq!(
            stack.area_of(Panel::Stack),
            memory.area_of(Panel::Memory),
            "they share the same slot"
        );
        assert!(
            stack.tab_bar.is_some(),
            "a shared slot needs a strip saying what else is in it"
        );
    }

    #[test]
    fn asking_for_a_missing_panel_returns_nothing() {
        let layout = compute(narrow_area(), Page::Code, Panel::Editor);
        assert!(layout.area_of(Panel::Registers).is_none());
        assert!(!layout.is_visible(Panel::Registers));
    }

    #[test]
    fn resizing_across_a_threshold_keeps_the_focus_visible() {
        for width in (MINIMUM_WIDTH..=WIDE_THRESHOLD + 10).step_by(3) {
            for height in [MINIMUM_HEIGHT, 20, 30, 50] {
                let area = Rect::new(0, 0, width, height);
                for (page, focus) in every_focus() {
                    let layout = compute(area, page, focus);
                    if layout.mode == LayoutMode::TooSmall {
                        continue;
                    }
                    assert!(
                        layout.is_visible(focus),
                        "{focus} hidden on {page} at {width}x{height}"
                    );
                }
            }
        }
    }
}

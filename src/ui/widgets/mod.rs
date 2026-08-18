//! The panels, drawn.
//!
//! Every widget here is a function taking `&App` and a [`Rect`] and drawing
//! into a [`Frame`]. None of them mutates application state — rendering reads,
//! and commands write. That is what makes it safe to draw at any moment
//! without wondering whether the act of drawing changed anything.
//!
//! Panels that have nothing to show because no program is running say so, in
//! words, rather than presenting an empty box that reads as a bug.

pub mod chrome;
pub mod code;
pub mod cpu;
pub mod editor;

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::panel::Panel;
use crate::app::App;
use crate::ui::Theme;

/// Builds the bordered block a panel draws inside.
///
/// The focused panel differs in border colour, border weight *and* border
/// style, so focus is visible without relying on colour alone.
pub fn panel_block(theme: &Theme, panel: Panel, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(if focused {
            BorderType::Thick
        } else {
            BorderType::Plain
        })
        .border_style(theme.border(focused))
        .title(Span::styled(
            format!(" {} ", panel.title()),
            theme.title(focused),
        ))
}

/// Draws a centred message inside a panel.
///
/// Used when a panel has nothing to show: an explanation is far better than an
/// empty rectangle, which a user reasonably reads as broken.
pub fn draw_placeholder(frame: &mut Frame, area: Rect, theme: &Theme, block: Block, message: &str) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let paragraph = Paragraph::new(message)
        .style(theme.dim())
        .wrap(Wrap { trim: true });

    // Roughly vertically centred; exact centring is not worth the arithmetic
    // for a hint.
    let y = inner.y + inner.height / 3;
    let target = Rect::new(
        inner.x,
        y,
        inner.width,
        inner.height.saturating_sub(y - inner.y),
    );
    frame.render_widget(paragraph, target);
}

/// Draws a panel's frame and returns the area inside it.
///
/// Returns `None` when the area is too small to hold anything, so callers can
/// skip their work rather than computing a layout for zero cells.
pub fn frame_panel(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    panel: Panel,
    focused: bool,
) -> Option<Rect> {
    let block = panel_block(theme, panel, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    (inner.width > 0 && inner.height > 0).then_some(inner)
}

/// Renders one panel into its area.
pub fn draw_panel(frame: &mut Frame, app: &App, panel: Panel, area: Rect) {
    let focused = app.focus == panel;
    match panel {
        Panel::Editor => editor::draw(frame, app, area, focused),
        Panel::Registers => cpu::draw_registers(frame, app, area, focused),
        Panel::Flags => cpu::draw_flags(frame, app, area, focused),
        Panel::Stack => cpu::draw_stack(frame, app, area, focused),
        Panel::Memory => cpu::draw_memory(frame, app, area, focused),
        Panel::Disassembly => code::draw_disassembly(frame, app, area, focused),
        Panel::Explain => code::draw_explanation(frame, app, area, focused),
        Panel::Breakpoints => code::draw_breakpoints(frame, app, area, focused),
        Panel::Output => chrome::draw_output(frame, app, area, focused),
        Panel::Syscalls => chrome::draw_syscalls(frame, app, area, focused),
        Panel::Explorer => chrome::draw_explorer(frame, app, area, focused),
    }
}

/// Truncates `text` to `width` cells, marking that it was cut.
///
/// Truncating by characters rather than bytes keeps multi-byte text intact,
/// and the ellipsis tells the user something is missing rather than letting
/// them read a half-line as complete.
pub fn truncate(text: &str, width: usize, ellipsis: &str) -> String {
    if width == 0 {
        return String::new();
    }
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let keep = width.saturating_sub(ellipsis.chars().count());
    let mut out: String = text.chars().take(keep).collect();
    out.push_str(ellipsis);
    out
}

/// Builds a label and value line, as most of the data panels use.
pub fn field_line<'a>(
    label: impl Into<String>,
    value: impl Into<String>,
    label_style: Style,
    value_style: Style,
) -> Line<'a> {
    Line::from(vec![
        Span::styled(label.into(), label_style),
        Span::styled(value.into(), value_style),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::ThemeKind;

    #[test]
    fn truncation_marks_what_it_cut() {
        assert_eq!(truncate("short", 10, "…"), "short");
        assert_eq!(truncate("a longer line", 6, "…"), "a lon…");
        assert_eq!(truncate("exact", 5, "…"), "exact");
        assert_eq!(truncate("anything", 0, "…"), "");
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        // A byte-based implementation would split a character and panic.
        let text = "ölçüm değeri";
        let cut = truncate(text, 6, "…");
        assert_eq!(cut.chars().count(), 6);
        assert!(text.starts_with(&cut[..cut.len() - "…".len()]));
    }

    #[test]
    fn truncation_works_with_an_ascii_ellipsis() {
        assert_eq!(truncate("a longer line", 8, "..."), "a lon...");
    }

    #[test]
    fn a_focused_panel_differs_by_more_than_colour() {
        // Focus must be visible on a monochrome terminal too.
        let theme = Theme::new(ThemeKind::Dark, true);
        let focused = panel_block(&theme, Panel::Editor, true);
        let idle = panel_block(&theme, Panel::Editor, false);
        assert_ne!(focused, idle);
    }
}

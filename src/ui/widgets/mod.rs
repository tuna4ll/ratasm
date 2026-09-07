//! The panels, drawn.

pub mod chrome;
pub mod code;
pub mod cpu;
pub mod editor;
pub mod learn;

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::panel::Panel;
use crate::app::App;
use crate::ui::Theme;

/// Builds the bordered block a panel draws inside.
pub fn panel_block(theme: &Theme, panel: Panel, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(if focused {
            BorderType::Thick
        } else {
            BorderType::Plain
        })
        .border_style(theme.border(focused))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", panel.title()),
            theme.title(focused),
        ))
}

/// Draws a message inside a panel that has nothing else to show.
pub fn draw_placeholder(frame: &mut Frame, area: Rect, theme: &Theme, block: Block, message: &str) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let paragraph = Paragraph::new(message)
        .style(theme.dim())
        .wrap(Wrap { trim: true });

    let needed = message.lines().count() as u16;
    let slack = inner.height.saturating_sub(needed);
    let y = inner.y + (slack / 2).min(inner.height / 3);
    let target = Rect::new(
        inner.x,
        y,
        inner.width,
        inner.height.saturating_sub(y - inner.y),
    );
    frame.render_widget(paragraph, target);
}

/// Draws a panel's frame and returns the area inside it, or `None` if it is
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
        Panel::CallStack => code::draw_call_stack(frame, app, area, focused),
        Panel::Memory => cpu::draw_memory(frame, app, area, focused),
        Panel::Disassembly => code::draw_disassembly(frame, app, area, focused),
        Panel::Explain => code::draw_explanation(frame, app, area, focused),
        Panel::Breakpoints => code::draw_breakpoints(frame, app, area, focused),
        Panel::Output => chrome::draw_output(frame, app, area, focused),
        Panel::Syscalls => chrome::draw_syscalls(frame, app, area, focused),
        Panel::Explorer => chrome::draw_explorer(frame, app, area, focused),
        Panel::Scratchpad => learn::draw_scratchpad(frame, app, area, focused),
        Panel::Learn => learn::draw_learn(frame, app, area, focused),
    }
}

/// Truncates `text` to `width` characters, marking with `ellipsis` that it
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

/// How many rows `lines` occupies once wrapped to `width`.
pub fn wrapped_rows(lines: &[Line<'_>], width: usize) -> usize {
    if width == 0 {
        return lines.len();
    }
    lines
        .iter()
        .map(|line| {
            let text: String = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect();
            let text = text.trim();
            if text.is_empty() {
                return 1;
            }

            let mut rows = 1;
            let mut used = 0;
            for word in text.split_whitespace() {
                let length = word.chars().count();
                let needed = if used == 0 { length } else { used + 1 + length };
                if needed <= width {
                    used = needed;
                } else {
                    rows += 1;
                    used = length.min(width);
                }
            }
            rows
        })
        .sum()
}

/// The rows a scrollable panel's content needs in an area `width` wide.
pub fn content_rows(app: &App, panel: Panel, width: u16) -> usize {
    let width = usize::from(width);
    match panel {
        Panel::Registers => cpu::register_lines(app, width).len(),
        Panel::Flags => cpu::flag_lines(app, width).len(),
        Panel::Stack => cpu::stack_lines(app).len(),
        Panel::Memory => cpu::memory_lines(app).len(),
        Panel::Disassembly => app.disassembly.len(),
        Panel::CallStack => app.frames.len(),
        Panel::Breakpoints => app.breakpoints.all().len(),
        Panel::Output => app.output.len(),
        Panel::Explorer => chrome::explorer_lines(app).len(),
        Panel::Explain => wrapped_rows(&code::explanation_lines(app), width),
        Panel::Learn => wrapped_rows(&learn::learn_lines(app), width.saturating_sub(0)),
        _ => 0,
    }
}

/// Draws `lines` inside `inner` at the panel's scroll offset.
pub fn draw_scrolled(
    frame: &mut Frame,
    app: &App,
    panel: Panel,
    inner: Rect,
    lines: Vec<Line<'_>>,
    wrap: bool,
) {
    let extent = app.scroll.extent(panel);
    let text_area = if extent.overflows() && inner.width > 1 {
        draw_scrollbar(frame, app, extent, inner);
        Rect::new(inner.x, inner.y, inner.width - 1, inner.height)
    } else {
        inner
    };

    let offset = u16::try_from(extent.offset).unwrap_or(u16::MAX);
    let paragraph = Paragraph::new(lines).scroll((offset, 0));
    let paragraph = if wrap {
        paragraph.wrap(Wrap { trim: true })
    } else {
        paragraph
    };
    frame.render_widget(paragraph, text_area);
}

/// Draws the scrollbar for `extent` down the right edge of `inner`.
fn draw_scrollbar(frame: &mut Frame, app: &App, extent: crate::app::scroll::Extent, inner: Rect) {
    let symbols = app.theme.symbols();
    let (start, size) = extent.thumb(inner.height);
    let x = inner.x + inner.width - 1;

    for row in 0..inner.height {
        let on_thumb = row >= start && row < start + size;
        let (glyph, style) = if on_thumb {
            (symbols.scroll_thumb, app.theme.accent())
        } else {
            (symbols.scroll_track, app.theme.dim())
        };
        frame.render_widget(
            Paragraph::new(Span::styled(glyph, style)),
            Rect::new(x, inner.y + row, 1, 1),
        );
    }
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
        let theme = Theme::new(ThemeKind::Dark, true);
        let focused = panel_block(&theme, Panel::Editor, true);
        let idle = panel_block(&theme, Panel::Editor, false);
        assert_ne!(focused, idle);
    }
}

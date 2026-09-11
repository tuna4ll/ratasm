//! The source editor panel: highlighted text, gutter, breakpoints and the

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::panel::Panel;
use crate::app::App;
use crate::editor::syntax::{self, TokenKind};
use crate::ui::theme::Palette;

/// Draws the editor.
pub fn draw(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let Some(inner) = super::frame_panel(frame, area, &app.theme, Panel::Editor, focused) else {
        return;
    };

    let document = app.workspace.active();
    let buffer = document.buffer();
    let theme = &app.theme;
    let symbols = theme.symbols();

    let gutter = if app.settings.editor.line_numbers {
        buffer.line_count().to_string().len().max(2) + 1
    } else {
        0
    };
    let marker_width = 2;
    let text_width = inner.width as usize;

    let first = document.scroll_line();
    let height = inner.height as usize;
    let cursor = document.cursor();
    let path = document.path().map(std::path::Path::to_path_buf);

    let stopped_here = match (&app.current_line, path.as_deref()) {
        (Some((file, _)), Some(open)) => crate::editor::workspace::same_file(open, file),
        _ => false,
    };

    let selection = document.selection();

    let mut lines: Vec<Line> = Vec::with_capacity(height);
    for offset in 0..height {
        let index = first + offset;
        if index >= buffer.line_count() {
            break;
        }

        let mut spans: Vec<Span> = Vec::new();
        let number = index + 1;

        let has_breakpoint = path
            .as_deref()
            .is_some_and(|path| app.breakpoints.is_set_at(path, number));
        let is_current = stopped_here
            && app
                .current_line
                .as_ref()
                .is_some_and(|(_, line)| *line == number);

        let marker = if is_current {
            Span::styled(
                format!(
                    "{:<width$}",
                    symbols.current_instruction,
                    width = marker_width
                ),
                theme.current_line(),
            )
        } else if has_breakpoint {
            Span::styled(
                format!("{:<width$}", symbols.breakpoint, width = marker_width),
                theme.breakpoint(),
            )
        } else {
            Span::raw(" ".repeat(marker_width))
        };
        spans.push(marker);

        if gutter > 0 {
            let style = if index == cursor.line {
                theme.bright()
            } else {
                theme.dim()
            };
            spans.push(Span::styled(
                format!("{number:>width$} ", width = gutter - 1),
                style,
            ));
        }

        let text = buffer.line_or_empty(index);
        let mut text_spans = highlight(text, theme.palette());
        if let Some(range) = selected_columns(selection, index, text.chars().count()) {
            text_spans = mark_selection(text_spans, range, theme.text_selection());
        }
        spans.extend(text_spans);

        let mut line = Line::from(spans);
        if index == cursor.line && app.settings.editor.highlight_current_line {
            line = line.style(theme.cursor_line());
        }
        lines.push(line);
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled("  (empty file)", theme.dim())));
    }

    let paragraph = Paragraph::new(lines).scroll((0, document.scroll_column() as u16));
    frame.render_widget(paragraph, inner);

    if focused && cursor.line >= first && cursor.line < first + height {
        let column = buffer.display_column(cursor.line, cursor.column);
        let x = inner.x
            + (marker_width + gutter + column).saturating_sub(document.scroll_column()) as u16;
        let y = inner.y + (cursor.line - first) as u16;
        if x < inner.x + inner.width && y < inner.y + inner.height && text_width > 0 {
            frame.set_cursor_position((x, y));
        }
    }
}

/// The columns of `line` covered by `selection`, if any.
fn selected_columns(
    selection: Option<crate::editor::Range>,
    line: usize,
    length: usize,
) -> Option<(usize, usize)> {
    let selection = selection?;
    if line < selection.start.line || line > selection.end.line {
        return None;
    }

    let start = if line == selection.start.line {
        selection.start.column
    } else {
        0
    };
    let end = if line == selection.end.line {
        selection.end.column
    } else {
        length + 1
    };

    (start < end).then_some((start.min(length), end.min(length + 1)))
}

/// Repaints the spans covering `range` with the selection style.
fn mark_selection<'a>(spans: Vec<Span<'a>>, range: (usize, usize), style: Style) -> Vec<Span<'a>> {
    let (from, to) = range;
    let mut out: Vec<Span<'a>> = Vec::with_capacity(spans.len() + 2);
    let mut column = 0;

    for span in spans {
        let length = span.content.chars().count();
        let (start, end) = (column, column + length);
        column = end;

        if end <= from || start >= to {
            out.push(span);
            continue;
        }

        let take = |span: &Span<'a>, first: usize, last: usize| -> Span<'a> {
            let text: String = span
                .content
                .chars()
                .skip(first)
                .take(last.saturating_sub(first))
                .collect();
            Span::styled(text, span.style)
        };

        if start < from {
            out.push(take(&span, 0, from - start));
        }
        let inner_start = from.saturating_sub(start);
        let inner_end = (to - start).min(length);
        let mut selected = take(&span, inner_start, inner_end);
        selected.style = selected.style.patch(style);
        out.push(selected);
        if end > to {
            out.push(take(&span, to - start, length));
        }
    }

    if to > column {
        out.push(Span::styled(" ".repeat(to - column.max(from)), style));
    }

    out
}

/// Converts one line of NASM into styled spans.
pub fn highlight<'a>(line: &'a str, palette: &Palette) -> Vec<Span<'a>> {
    syntax::tokenize(line)
        .into_iter()
        .map(|token| {
            let text = token.text(line);
            Span::styled(text, style_for(token.kind, palette))
        })
        .collect()
}

/// The style a token kind is drawn in.
fn style_for(kind: TokenKind, palette: &Palette) -> Style {
    match kind {
        TokenKind::Whitespace => Style::default(),
        TokenKind::Comment => Style::default()
            .fg(palette.syntax_comment)
            .add_modifier(Modifier::ITALIC),
        TokenKind::String => Style::default().fg(palette.syntax_string),
        TokenKind::Number => Style::default().fg(palette.syntax_number),
        TokenKind::LabelDefinition => Style::default()
            .fg(palette.syntax_label)
            .add_modifier(Modifier::BOLD),
        TokenKind::Instruction => Style::default().fg(palette.syntax_instruction),
        TokenKind::Directive => Style::default().fg(palette.syntax_directive),
        TokenKind::Preprocessor => Style::default().fg(palette.syntax_macro),
        TokenKind::Register => Style::default().fg(palette.syntax_register),
        TokenKind::SizeKeyword => Style::default().fg(palette.syntax_keyword),
        TokenKind::Identifier => Style::default().fg(palette.foreground),
        TokenKind::Punctuation => Style::default().fg(palette.syntax_punctuation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::ThemeKind;

    fn palette() -> Palette {
        ThemeKind::Dark.palette()
    }

    #[test]
    fn highlighting_reproduces_the_line_exactly() {
        for line in [
            "",
            "    mov rax, 60          ; exit",
            "_start:",
            "%define SIZE 64",
            "msg: db `hi\\n`, 0",
            "    lea rdi, [rel msg]",
            "; ölçüm değeri",
        ] {
            let rebuilt: String = highlight(line, &palette())
                .iter()
                .map(|span| span.content.as_ref())
                .collect();
            assert_eq!(rebuilt, line, "highlighting changed {line:?}");
        }
    }

    #[test]
    fn different_token_kinds_get_different_colours() {
        let palette = palette();
        let spans = highlight("    mov rax, 60 ; note", &palette);

        let styles: Vec<Style> = spans.iter().map(|span| span.style).collect();
        let distinct: std::collections::HashSet<_> =
            styles.iter().map(|style| format!("{style:?}")).collect();
        assert!(
            distinct.len() >= 4,
            "expected several distinct styles, got {distinct:?}"
        );
    }

    #[test]
    fn comments_are_italic_as_well_as_coloured() {
        let spans = highlight("; a comment", &palette());
        let comment = spans.last().expect("a span");
        assert!(comment.style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn label_definitions_are_bold() {
        let spans = highlight("_start:", &palette());
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn an_empty_line_produces_no_spans() {
        assert!(highlight("", &palette()).is_empty());
    }
}

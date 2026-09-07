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
        spans.extend(highlight(text, theme.palette()));

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

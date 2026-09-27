//! The Editor panel: `$EDITOR` running inside it, or, when the editor is
//! closed or a debug session is on, a read-only listing of the source with
//! its breakpoints and the line the program stopped on. The listing has no
//! cursor; ratasm does not edit text.

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
    if let Some(screen) = app.editor_screen.as_ref().filter(|_| app.shows_editor()) {
        frame.render_widget(
            Paragraph::new(super::chrome::terminal_lines(screen, &app.theme)),
            inner,
        );
        if focused && !screen.screen().hide_cursor() {
            let (row, column) = screen.screen().cursor_position();
            if row < inner.height && column < inner.width {
                frame.set_cursor_position((inner.x + column, inner.y + row));
            }
        }
        return;
    }

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

    let first = document.scroll_line();
    let height = inner.height as usize;
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
            spans.push(Span::styled(
                format!("{number:>width$} ", width = gutter - 1),
                theme.dim(),
            ));
        }
        spans.extend(highlight(buffer.line_or_empty(index), theme.palette()));
        lines.push(Line::from(spans));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled("  (empty file)", theme.dim())));
    }

    frame.render_widget(Paragraph::new(lines), inner);
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

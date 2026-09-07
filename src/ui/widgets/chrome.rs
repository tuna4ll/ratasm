//! The surrounding furniture: output, references, tabs, status bar, overlays.

use ratatui::layout::{Constraint, Direction, Layout as RatatuiLayout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::mode::Mode;
use crate::app::page::Page;
use crate::app::panel::Panel;
use crate::app::state::Severity;
use crate::app::App;
use crate::debugger::state::DebuggerState;

/// Draws the build and program output.
pub fn draw_output(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let block = super::panel_block(&app.theme, Panel::Output, focused);

    if app.output.is_empty() {
        super::draw_placeholder(
            frame,
            area,
            &app.theme,
            block,
            "Nothing has run yet.\n\nF6 builds, F5 runs. Output and errors appear here.",
        );
        return;
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = output_lines(app);
    super::draw_scrolled(frame, app, Panel::Output, inner, lines, false);
}

/// One line per line of tool output, coloured by what it says.
pub fn output_lines<'a>(app: &App) -> Vec<Line<'a>> {
    let theme = &app.theme;
    app.output
        .iter()
        .map(|text| {
            let lowered = text.to_ascii_lowercase();
            let style = if lowered.contains("error") {
                theme.error()
            } else if lowered.contains("warning") {
                theme.warning()
            } else if text.starts_with('$') {
                theme.accent()
            } else {
                theme.base()
            };
            Line::from(Span::styled(text.clone(), style))
        })
        .collect()
}

/// Draws the system call reference.
const MOST_LISTED_SYSCALLS: u16 = 12;

/// Draws the system call finder.
pub fn draw_syscalls(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let Some(inner) = super::frame_panel(frame, area, &app.theme, Panel::Syscalls, focused) else {
        return;
    };

    let theme = &app.theme;
    let symbols = theme.symbols();
    let matches = app.matching_syscalls();

    let rows = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    let query = if app.syscall_query.is_empty() {
        Span::styled("type to search by name or number", theme.dim())
    } else {
        Span::styled(app.syscall_query.clone(), theme.base())
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("find ", theme.accent()),
            query,
        ])),
        rows[0],
    );

    if matches.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("No system call matches.", theme.dim())),
            rows[1],
        );
        return;
    }

    let selected = app.syscall_selected.min(matches.len() - 1);
    let show_detail = rows[1].height >= 10 && rows[1].width >= 46;

    let (list_area, detail_area) = if show_detail {
        let listed = u16::try_from(matches.len())
            .unwrap_or(u16::MAX)
            .clamp(1, MOST_LISTED_SYSCALLS)
            .min(rows[1].height.saturating_sub(6));
        let split = RatatuiLayout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(listed + 1), Constraint::Min(5)])
            .split(rows[1]);
        (split[0], Some(split[1]))
    } else {
        (rows[1], None)
    };

    let height = usize::from(list_area.height);
    let first = selected
        .saturating_sub(height / 2)
        .min(matches.len().saturating_sub(height));

    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(first)
        .take(height)
        .map(|(index, call)| {
            let marker = if index == selected {
                symbols.selection
            } else {
                " "
            };
            let style = if index == selected {
                theme.selection()
            } else {
                theme.base()
            };
            Line::from(Span::styled(
                format!("{marker} {:>3}  {}", call.number, call.name),
                style,
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), list_area);

    let Some(detail_area) = detail_area else {
        return;
    };
    let Some(call) = matches.get(selected) else {
        return;
    };

    let mut lines = vec![
        Line::from(Span::styled(
            format!("{} ({})", call.name, call.number),
            theme.bright(),
        )),
        Line::from(Span::styled(call.summary.clone(), theme.dim())),
    ];

    if call.detailed {
        lines.push(Line::from(""));
        lines.push(super::field_line(
            "RAX   ",
            format!("{}", call.number),
            theme.accent(),
            theme.base(),
        ));
        for argument in &call.args {
            lines.push(super::field_line(
                format!("{:<6}", argument.register_display()),
                format!("{} — {}", argument.name, argument.description),
                theme.accent(),
                theme.base(),
            ));
        }
        if let Some(returns) = &call.returns {
            lines.push(Line::from(""));
            lines.push(super::field_line(
                "returns ",
                returns.clone(),
                theme.dim(),
                theme.base(),
            ));
        }
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "Arguments are not modelled for this call. See: man 2 {}",
                call.name
            ),
            theme.warning(),
        )));
    }

    if let Some(example) = &call.example {
        let room = usize::from(detail_area.height).saturating_sub(lines.len() + 2);
        if room >= example.lines().count() {
            lines.push(Line::from(""));
            for source in example.lines() {
                lines.push(Line::from(super::editor::highlight(
                    source,
                    theme.palette(),
                )));
            }
        }
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), detail_area);
}

/// Draws the file and symbol explorer.
pub fn draw_explorer(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let Some(inner) = super::frame_panel(frame, area, &app.theme, Panel::Explorer, focused) else {
        return;
    };

    let lines = explorer_lines(app);
    super::draw_scrolled(frame, app, Panel::Explorer, inner, lines, false);
}

/// The open documents and the active one's symbols.
pub fn explorer_lines<'a>(app: &App) -> Vec<Line<'a>> {
    let theme = &app.theme;
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        format!("Project: {}", app.project.name()),
        theme.bright(),
    )));
    lines.push(Line::from(""));

    for (index, document) in app.workspace.documents().iter().enumerate() {
        let active = index == app.workspace.active_index();
        let marker = if active {
            theme.symbols().selection
        } else {
            " "
        };
        let modified = if document.is_modified() {
            theme.symbols().modified
        } else {
            " "
        };
        lines.push(Line::from(Span::styled(
            format!("{marker} {modified} {}", document.display_name()),
            if active { theme.base() } else { theme.dim() },
        )));
    }

    let symbols = crate::editor::symbols::extract(app.workspace.active().buffer());
    if !symbols.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Symbols", theme.dim())));
        for symbol in &symbols {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<14}", symbol.name), theme.base()),
                Span::styled(format!("{:>4}  ", symbol.position.line + 1), theme.dim()),
                Span::styled(symbol.kind.description(), theme.dim()),
            ]));
        }
    }

    lines
}

/// Draws the bar listing open documents.
pub fn draw_page_bar(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let theme = &app.theme;
    let symbols = theme.symbols();

    let mut spans: Vec<Span> = Vec::new();
    for page in Page::ALL {
        let active = page == app.page;
        spans.push(Span::styled(
            format!(" {} {} ", page.number(), page.title()),
            if active {
                theme.selection()
            } else {
                theme.dim()
            },
        ));
    }

    let mut documents: Vec<Span> = Vec::new();
    for (index, document) in app.workspace.documents().iter().enumerate() {
        let active = index == app.workspace.active_index();
        let modified = if document.is_modified() {
            symbols.modified
        } else {
            ""
        };
        documents.push(Span::styled(
            format!(" {}{} ", document.display_name(), modified),
            if active { theme.bright() } else { theme.dim() },
        ));
    }

    let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
    let room = usize::from(area.width).saturating_sub(used);
    let listed: usize = documents
        .iter()
        .map(|span| span.content.chars().count())
        .sum();
    if listed + 3 <= room {
        spans.push(Span::styled(
            format!("  {}  ", symbols.separator),
            theme.dim(),
        ));
        spans.append(&mut documents);
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Draws the tab strip for panels sharing a slot.
pub fn draw_tab_bar(frame: &mut Frame, app: &App, area: Rect, tabs: &[Panel]) {
    if area.height == 0 || tabs.is_empty() {
        return;
    }
    let theme = &app.theme;

    let spans: Vec<Span> = tabs
        .iter()
        .map(|panel| {
            let active = *panel == app.focus;
            Span::styled(
                format!(" {} ", panel.title()),
                if active {
                    theme.selection()
                } else {
                    theme.dim()
                },
            )
        })
        .collect();

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Draws the status bar.
pub fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let theme = &app.theme;
    let symbols = theme.symbols();

    let (glyph, style) = match app.status.severity {
        Severity::Info => (symbols.info, theme.info()),
        Severity::Warning => (symbols.warning, theme.warning()),
        Severity::Error => (symbols.error, theme.error()),
        Severity::Success => (symbols.success, theme.success()),
    };

    let document = app.workspace.active();
    let cursor = document.cursor();
    let position = format!("{}:{}", cursor.line + 1, cursor.column + 1);

    let state = app.debugger.state();
    let state_style = match state {
        DebuggerState::Failed => theme.error(),
        DebuggerState::Paused => theme.success(),
        DebuggerState::Running => theme.warning(),
        _ => theme.dim(),
    };

    let right = format!(
        " {} {} {} {} {} {} ",
        position,
        symbols.separator,
        app.focus.title(),
        symbols.separator,
        app.register_format.label(),
        symbols.separator,
    );

    let used = right.chars().count() + state.label().chars().count() + 4;
    let message_width = usize::from(area.width).saturating_sub(used);
    let message = super::truncate(&app.status.text, message_width, symbols.ellipsis);

    let spans = vec![
        Span::styled(format!(" {glyph} "), style),
        Span::styled(message, style),
        Span::styled(
            " ".repeat(
                usize::from(area.width)
                    .saturating_sub(used)
                    .saturating_sub(app.status.text.chars().count().min(message_width)),
            ),
            theme.base(),
        ),
        Span::styled(right, theme.dim()),
        Span::styled(state.label().to_owned(), state_style),
    ];

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Draws whichever overlay is open, if any.
pub fn draw_overlay(frame: &mut Frame, app: &App, area: Rect) {
    match &app.mode {
        Mode::Normal => {}
        Mode::Prompt(prompt) => draw_prompt(frame, app, area, prompt),
        Mode::Palette(palette) => draw_palette(frame, app, area, palette),
    }
}

/// Draws a single-line prompt near the bottom of the screen.
fn draw_prompt(frame: &mut Frame, app: &App, area: Rect, prompt: &crate::app::mode::Prompt) {
    let theme = &app.theme;
    let Some(kind) = prompt.kind() else {
        return;
    };

    let height = 3u16;
    if area.height < height + 2 || area.width < 20 {
        return;
    }
    let width = area.width.saturating_sub(4).min(80);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + area.height - height - 2,
        width,
        height,
    );

    frame.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border(true))
        .title(Span::styled(
            format!(" {} ", kind.label()),
            theme.title(true),
        ));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let content = if prompt.is_empty() {
        Span::styled(kind.hint().to_owned(), theme.dim())
    } else {
        Span::styled(prompt.text().to_owned(), theme.base())
    };
    frame.render_widget(Paragraph::new(Line::from(content)), inner);

    if !kind.is_confirmation() {
        let x = inner.x + prompt.cursor() as u16;
        if x < inner.x + inner.width {
            frame.set_cursor_position((x, inner.y));
        }
    }
}

/// Draws the command palette over the middle of the screen.
fn draw_palette(frame: &mut Frame, app: &App, area: Rect, palette: &crate::app::mode::Palette) {
    let theme = &app.theme;
    let symbols = theme.symbols();

    if area.width < 30 || area.height < 8 {
        return;
    }
    let width = area.width.saturating_sub(8).min(90);
    let height = area.height.saturating_sub(6).min(20);
    let rect = Rect::new(area.x + (area.width - width) / 2, area.y + 2, width, height);

    frame.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border(true))
        .title(Span::styled(" Command palette ", theme.title(true)));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    if inner.height < 2 {
        return;
    }

    let rows = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    let query = if palette.prompt().is_empty() {
        Span::styled("type to search commands", theme.dim())
    } else {
        Span::styled(palette.prompt().text().to_owned(), theme.base())
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled("> ", theme.accent()), query])),
        rows[0],
    );

    if palette.matches().is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("No matching command.", theme.dim())),
            rows[1],
        );
        return;
    }

    let height = usize::from(rows[1].height);
    let selected = palette.selected();
    let first = selected
        .saturating_sub(height / 2)
        .min(palette.matches().len().saturating_sub(height));

    let lines: Vec<Line> = palette
        .matches()
        .iter()
        .enumerate()
        .skip(first)
        .take(height)
        .map(|(index, command)| {
            let active = index == selected;
            let marker = if active { symbols.selection } else { " " };
            let binding = app
                .keymap
                .binding_for(command)
                .map(|binding| binding.to_string())
                .unwrap_or_default();

            let suffix = if command.prompts_for_input() {
                symbols.ellipsis
            } else {
                ""
            };

            Line::from(vec![
                Span::styled(
                    format!("{marker} {}{suffix}", command.title()),
                    if active {
                        theme.selection()
                    } else {
                        theme.base()
                    },
                ),
                Span::styled(format!("  {binding}"), theme.dim()),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), rows[1]);

    let x = rows[0].x + 2 + palette.prompt().cursor() as u16;
    if x < rows[0].x + rows[0].width {
        frame.set_cursor_position((x, rows[0].y));
    }
}

/// Draws the message shown when the terminal is too small to work in.
pub fn draw_too_small(frame: &mut Frame, area: Rect) {
    let message = format!(
        "Terminal too small.\n\n{}x{} available; ratasm needs at least {}x{}.",
        area.width,
        area.height,
        crate::ui::layout::MINIMUM_WIDTH,
        crate::ui::layout::MINIMUM_HEIGHT
    );
    frame.render_widget(Paragraph::new(message).wrap(Wrap { trim: true }), area);
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::config::Settings;
    use crate::project::Project;

    fn app() -> App {
        let project = Project::for_file(std::path::Path::new("/tmp/ratasm-test/main.asm"));
        App::new(project, Settings::default()).expect("databases load")
    }

    #[test]
    fn a_syscall_without_modelled_arguments_is_marked_as_such() {
        let app = app();
        let call = app
            .syscalls
            .syscalls
            .iter()
            .find(|call| !call.detailed)
            .expect("an undetailed call");
        assert!(call.args.is_empty());
        assert!(call.summary.contains("man 2"));
    }

    #[test]
    fn a_detailed_syscall_lists_its_argument_registers() {
        let app = app();
        let write = app.syscalls.by_name("write").expect("write");
        assert!(write.detailed);
        assert_eq!(write.args[0].register_display(), "RDI");
    }
}

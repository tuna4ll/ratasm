//! The scratchpad and the learning panels.

use ratatui::layout::{Constraint, Direction, Layout as RatatuiLayout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use crate::app::panel::Panel;
use crate::app::App;
use crate::learning::Verdict;

/// Draws the scratchpad.
pub fn draw_scratchpad(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let Some(inner) = super::frame_panel(frame, area, &app.theme, Panel::Scratchpad, focused)
    else {
        return;
    };
    let theme = &app.theme;

    let rows = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(3),
        ])
        .split(inner);

    let setup: Vec<Span> = if app.scratchpad.initial.is_empty() {
        vec![Span::styled(
            "no starting values — type  rax=1  to set one",
            theme.dim(),
        )]
    } else {
        app.scratchpad
            .initial
            .iter()
            .map(|(name, value)| {
                Span::styled(
                    format!("{}={value:#x}  ", name.to_uppercase()),
                    theme.accent(),
                )
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("Starting values", theme.dim())),
            Line::from(setup),
        ]),
        rows[0],
    );

    let snippet = if app.scratchpad.snippet.is_empty() {
        Span::styled("type an instruction, then Enter to run it", theme.dim())
    } else {
        Span::styled(app.scratchpad.snippet.clone(), theme.base())
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("Instruction", theme.dim())),
            Line::from(vec![Span::styled("> ", theme.accent()), snippet]),
        ]),
        rows[1],
    );

    let mut lines: Vec<Line> = Vec::new();
    match &app.scratchpad_result {
        None => {
            lines.push(Line::from(Span::styled("Nothing run yet.", theme.dim())));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "The snippet is assembled and executed natively on this machine. \
                 It is not sandboxed.",
                theme.warning(),
            )));
        }
        Some(Err(message)) => {
            lines.push(Line::from(Span::styled(message.clone(), theme.error())));
        }
        Some(Ok(outcome)) => {
            if outcome.is_empty() {
                lines.push(Line::from(Span::styled("Nothing changed.", theme.dim())));
            } else {
                lines.push(Line::from(Span::styled("Changed", theme.dim())));
                for change in &outcome.changes {
                    lines.push(Line::from(Span::styled(change.describe(), theme.changed())));
                }

                let flags = outcome.flag_changes();
                if !flags.is_empty() {
                    let text: Vec<String> = flags
                        .iter()
                        .map(|flag| {
                            format!(
                                "{}={}",
                                flag.abbreviation(),
                                u8::from(outcome.flags_after.has(*flag))
                            )
                        })
                        .collect();
                    lines.push(Line::from(vec![
                        Span::styled("Flags  ", theme.dim()),
                        Span::styled(text.join(" "), theme.changed()),
                    ]));
                }
            }
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), rows[2]);
}

/// Draws the learning panel.
pub fn draw_learn(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let Some(inner) = super::frame_panel(frame, area, &app.theme, Panel::Learn, focused) else {
        return;
    };
    let theme = &app.theme;
    let progress = &app.learning;

    let rows = RatatuiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(2)])
        .split(inner);

    let header = format!(
        "{}/{}   solved {}/{}",
        progress.position(),
        crate::learning::item_count(),
        progress.solved_count(),
        crate::learning::QUESTIONS.len()
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(header, theme.accent()),
            Span::styled("   ←→ move   ? questions", theme.dim()),
        ])),
        rows[0],
    );

    let lines = learn_lines(app);
    super::draw_scrolled(frame, app, Panel::Learn, rows[1], lines, true);
}

/// The lesson or question the reader is on.
pub fn learn_lines<'a>(app: &App) -> Vec<Line<'a>> {
    let theme = &app.theme;
    let progress = &app.learning;
    let mut lines: Vec<Line> = Vec::new();

    if let Some(lesson) = progress.current_lesson() {
        lines.push(Line::from(Span::styled(lesson.title, theme.bright())));
        lines.push(Line::from(""));
        for paragraph in lesson.body {
            lines.push(Line::from(Span::styled(*paragraph, theme.base())));
            lines.push(Line::from(""));
        }
    } else if let Some(question) = progress.current_question() {
        lines.push(Line::from(Span::styled(question.prompt, theme.bright())));
        lines.push(Line::from(""));

        let typed = if progress.typed.is_empty() {
            Span::styled("type a value, then Enter", theme.dim())
        } else {
            Span::styled(progress.typed.clone(), theme.base())
        };
        lines.push(Line::from(vec![
            Span::styled("Answer: ", theme.dim()),
            typed,
        ]));
        lines.push(Line::from(""));

        match &progress.verdict {
            Verdict::Unanswered => {}
            Verdict::Correct => {
                lines.push(Line::from(Span::styled(
                    format!("{} Correct.", theme.symbols().success),
                    theme.success(),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(question.explanation, theme.dim())));
            }
            Verdict::Wrong { given } => {
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} Not {given}. The answer is {}.",
                        theme.symbols().error,
                        question.answer_text()
                    ),
                    theme.error(),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(question.explanation, theme.base())));
            }
        }
    }

    lines
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
    fn the_scratchpad_starts_empty_and_says_so() {
        let app = app();
        assert!(app.scratchpad.snippet.is_empty());
        assert!(app.scratchpad_result.is_none());
    }

    #[test]
    fn the_learning_panel_starts_on_the_first_lesson() {
        let app = app();
        assert_eq!(app.learning.position(), 1);
        assert_eq!(
            app.learning.current_lesson().map(|lesson| lesson.id),
            Some(crate::learning::LESSONS[0].id)
        );
    }
}

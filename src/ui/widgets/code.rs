//! Panels about the code itself: disassembly, the explanation, breakpoints.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::page::Page;
use crate::app::panel::Panel;
use crate::app::App;

/// Draws the disassembly panel.
pub fn draw_disassembly(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let block = super::panel_block(&app.theme, Panel::Disassembly, focused);

    if app.disassembly.is_empty() {
        super::draw_placeholder(
            frame,
            area,
            &app.theme,
            block,
            "No disassembly yet.\n\nBuild the project (F6) to disassemble the executable, or \
             start a debug session (F12) to follow the program counter.",
        );
        return;
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let theme = &app.theme;
    let symbols = theme.symbols();
    let height = usize::from(inner.height);

    // Keep the program counter in view: scroll so it sits a third of the way
    // down, which shows both where execution came from and where it is going.
    let current = app
        .current_address
        .and_then(|address| {
            app.disassembly
                .iter()
                .position(|line| line.instruction.address == address)
        })
        .unwrap_or(0);
    let first = current.saturating_sub(height / 3);

    let lines: Vec<Line> = app
        .disassembly
        .iter()
        .skip(first)
        .take(height)
        .map(|line| {
            let instruction = &line.instruction;
            let is_current = app.current_address == Some(instruction.address);

            let marker = if is_current {
                Span::styled(
                    format!("{} ", symbols.current_instruction),
                    theme.current_line(),
                )
            } else {
                Span::raw("  ".to_owned())
            };

            let text_style = if instruction.invalid {
                theme.error()
            } else if is_current {
                theme.current_line()
            } else {
                theme.base()
            };

            let mut spans = vec![
                marker,
                Span::styled(format!("{:016x}  ", instruction.address), theme.address()),
                Span::styled(format!("{:<24}", instruction.bytes_text()), theme.bytes()),
                Span::styled(instruction.text(), text_style),
            ];

            // A branch with a known target is worth annotating; an indirect
            // one has no target to show, and inventing one would be a lie.
            if let Some(target) = instruction.branch_target {
                spans.push(Span::styled(
                    format!("  {} 0x{target:x}", symbols.stack_pointer),
                    theme.dim(),
                ));
            }
            Line::from(spans)
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

/// Draws the instruction explanation panel.
pub fn draw_explanation(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let block = super::panel_block(&app.theme, Panel::Explain, focused);

    let Some(explanation) = app.current_explanation() else {
        // Where the instruction comes from depends on the page, so telling
        // the reader to move a cursor on a page with no editor would be
        // advice they cannot follow.
        let hint = match app.page {
            Page::Reference => "Type an instruction name in the search box to see what it does.",
            Page::Learn => "Type an instruction in the scratchpad to see what it does.",
            Page::Code | Page::Debug => "Put the cursor on an instruction to see what it does.",
        };
        super::draw_placeholder(frame, area, &app.theme, block, hint);
        return;
    };
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let theme = &app.theme;
    let mut lines: Vec<Line> = Vec::new();

    let mut heading = vec![Span::styled(
        explanation.mnemonic.to_uppercase(),
        theme.bright(),
    )];
    if let Some(prefix) = &explanation.prefix {
        heading.insert(
            0,
            Span::styled(format!("{} ", prefix.to_uppercase()), theme.accent()),
        );
    }
    heading.push(Span::styled(
        format!("  {}", explanation.summary),
        theme.dim(),
    ));
    lines.push(Line::from(heading));

    for (label, value) in explanation.lines() {
        lines.push(super::field_line(
            format!("{label:<16}"),
            value,
            theme.dim(),
            theme.base(),
        ));

        // Show the same effect with real values right under the symbolic one,
        // so the relationship between them is obvious.
        if label == "Effect" {
            if let Some(concrete) = &explanation.concrete {
                lines.push(super::field_line(
                    format!("{:<16}", "  with values"),
                    concrete.clone(),
                    theme.dim(),
                    theme.current_line(),
                ));
            }
        }
    }

    // The limits of what is known are stated, not hidden.
    if explanation.approximate {
        lines.push(Line::from(Span::styled(
            "Vector instruction: named, but its per-lane effect is not modelled.",
            theme.warning(),
        )));
    }
    if let Some(notes) = &explanation.notes {
        if inner.height as usize > lines.len() + 1 {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(notes.clone(), theme.dim())));
        }
    }

    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: true }),
        inner,
    );
}

/// Draws the call stack.
///
/// This is the chain of calls, not the stack memory: the panel next door shows
/// the bytes around `RSP`, and confusing the two is exactly what this panel
/// exists to prevent.
pub fn draw_call_stack(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let block = super::panel_block(&app.theme, Panel::CallStack, focused);

    if app.frames.is_empty() {
        let message = if app.debugger.state().can_inspect() {
            "No frames reported.\n\nGDB could not walk the stack from here."
        } else {
            "No debug session.\n\nPress F12 to start one; the call stack shows how execution \
             reached the current instruction."
        };
        super::draw_placeholder(frame, area, &app.theme, block, message);
        return;
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let theme = &app.theme;
    let symbols = theme.symbols();

    let lines: Vec<Line> = app
        .frames
        .iter()
        .enumerate()
        .take(usize::from(inner.height))
        .map(|(index, stack_frame)| {
            let selected = index == app.frame_selected && focused;
            let marker = if stack_frame.is_innermost() {
                symbols.current_instruction
            } else if selected {
                symbols.selection
            } else {
                " "
            };

            let style = if selected {
                theme.selection()
            } else if stack_frame.is_innermost() {
                theme.current_line()
            } else {
                theme.dim()
            };

            Line::from(vec![
                Span::styled(format!("{marker} "), theme.accent()),
                Span::styled(
                    super::truncate(
                        &stack_frame.describe(),
                        usize::from(inner.width).saturating_sub(2),
                        symbols.ellipsis,
                    ),
                    style,
                ),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

/// Draws the breakpoint list.
pub fn draw_breakpoints(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let block = super::panel_block(&app.theme, Panel::Breakpoints, focused);

    if app.breakpoints.is_empty() {
        super::draw_placeholder(
            frame,
            area,
            &app.theme,
            block,
            "No breakpoints.\n\nPut the cursor on a line and press F9 to set one.",
        );
        return;
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let theme = &app.theme;
    let symbols = theme.symbols();

    let lines: Vec<Line> = app
        .breakpoints
        .all()
        .iter()
        .enumerate()
        .take(usize::from(inner.height))
        .map(|(index, breakpoint)| {
            let glyph = if breakpoint.enabled {
                symbols.breakpoint
            } else {
                symbols.breakpoint_disabled
            };
            let style = if index == app.breakpoint_selected && focused {
                theme.selection()
            } else if breakpoint.enabled {
                theme.base()
            } else {
                theme.dim()
            };

            Line::from(vec![
                Span::styled(format!("{glyph} "), theme.breakpoint()),
                Span::styled(breakpoint.describe(), style),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
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
    fn the_explanation_panel_has_content_for_a_real_instruction() {
        let mut app = app();
        app.workspace.active_mut().insert("    add rax, rbx\n");
        app.workspace.active_mut().move_cursor(
            crate::editor::Movement::To(crate::editor::Position::new(0, 6)),
            crate::editor::SelectionMode::Collapse,
        );

        let explanation = app.current_explanation().expect("an explanation");
        assert_eq!(explanation.effect, "RAX ← RAX + RBX");
        assert!(!explanation.lines().is_empty());
    }

    #[test]
    fn a_comment_line_yields_no_explanation_to_draw() {
        let mut app = app();
        app.workspace.active_mut().insert("; nothing here\n");
        assert!(app.current_explanation().is_none());
    }
}

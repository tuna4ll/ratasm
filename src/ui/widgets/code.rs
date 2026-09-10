//! Panels about the code itself: disassembly, the explanation, breakpoints.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Wrap;
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

    let lines = disassembly_lines(app, usize::from(inner.width));
    super::draw_scrolled(frame, app, Panel::Disassembly, inner, lines, None);
}

/// Columns kept for the mnemonic and its operands, whatever else is dropped.
const INSTRUCTION_WIDTH: usize = 20;

/// How wide the address and byte columns should be for this listing.
fn column_widths(app: &App, width: usize) -> (usize, usize) {
    let widest_address = app
        .disassembly
        .iter()
        .map(|line| line.instruction.address)
        .max()
        .unwrap_or(0);
    let digits = (16 - widest_address.leading_zeros() as usize / 4).max(8);
    let address = digits.min(16);

    let bytes = app
        .disassembly
        .iter()
        .map(|line| line.instruction.bytes_text().chars().count())
        .max()
        .unwrap_or(0)
        .min(24);

    let fixed = 2 + address + 2;
    if fixed + bytes + 2 + INSTRUCTION_WIDTH > width {
        (address, 0)
    } else {
        (address, bytes)
    }
}

/// One line per decoded instruction, for a panel `width` columns wide.
pub fn disassembly_lines<'a>(app: &App, width: usize) -> Vec<Line<'a>> {
    let theme = &app.theme;
    let symbols = theme.symbols();
    let (address_width, byte_width) = column_widths(app, width);

    app.disassembly
        .iter()
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
                Span::styled(
                    format!("{:0address_width$x}  ", instruction.address),
                    theme.address(),
                ),
            ];
            if byte_width > 0 {
                spans.push(Span::styled(
                    format!("{:<byte_width$}  ", instruction.bytes_text()),
                    theme.bytes(),
                ));
            }
            spans.push(Span::styled(instruction.text(), text_style));

            if let Some(target) = instruction.branch_target {
                spans.push(Span::styled(
                    format!("  {} 0x{target:x}", symbols.stack_pointer),
                    theme.dim(),
                ));
            }
            Line::from(spans)
        })
        .collect()
}

/// Draws the instruction explanation panel.
pub fn draw_explanation(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let block = super::panel_block(&app.theme, Panel::Explain, focused);

    if app.current_explanation().is_none() {
        let hint = match app.page {
            Page::Reference => "Type an instruction name in the search box to see what it does.",
            Page::Learn => "Type an instruction in the scratchpad to see what it does.",
            Page::Code | Page::Debug => "Put the cursor on an instruction to see what it does.",
        };
        super::draw_placeholder(frame, area, &app.theme, block, hint);
        return;
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = explanation_lines(app);
    super::draw_scrolled(
        frame,
        app,
        Panel::Explain,
        inner,
        lines,
        Some(Wrap { trim: true }),
    );
}

/// The explanation of whichever instruction is under discussion.
pub fn explanation_lines<'a>(app: &App) -> Vec<Line<'a>> {
    let Some(explanation) = app.current_explanation() else {
        return Vec::new();
    };
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

    if explanation.approximate {
        lines.push(Line::from(Span::styled(
            "Vector instruction: named, but its per-lane effect is not modelled.",
            theme.warning(),
        )));
    }
    if let Some(notes) = &explanation.notes {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(notes.clone(), theme.dim())));
    }

    lines
}

/// Draws the call stack: the chain of calls, not the stack memory.
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

    super::draw_scrolled(frame, app, Panel::CallStack, inner, lines, None);
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

    super::draw_scrolled(frame, app, Panel::Breakpoints, inner, lines, None);
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

    fn decoded(address: u64, bytes: Vec<u8>) -> crate::disassembler::DisassemblyLine {
        let mut decoded = crate::disassembler::decode(&bytes, address, Default::default());
        crate::disassembler::DisassemblyLine::bare(decoded.pop().expect("one instruction"))
    }

    #[test]
    fn a_low_address_does_not_pay_for_sixteen_hex_digits() {
        let mut app = app();
        app.disassembly = vec![decoded(0x40_00b0, vec![0x90])];

        let (address, _) = super::column_widths(&app, 120);
        assert_eq!(address, 8, "a user program sits well below 2^32");

        app.disassembly = vec![decoded(0x7fff_0000_1000, vec![0x90])];
        let (address, _) = super::column_widths(&app, 120);
        assert!(address > 8, "a high address still gets the digits it needs");
    }

    #[test]
    fn the_byte_column_fits_the_listing_and_goes_when_there_is_no_room() {
        let mut app = app();
        app.disassembly = vec![decoded(0x40_00b0, vec![0x90])];

        let (_, bytes) = super::column_widths(&app, 120);
        assert_eq!(bytes, 2, "one byte needs two columns, not twenty-four");

        let (_, bytes) = super::column_widths(&app, 24);
        assert_eq!(bytes, 0, "the mnemonic outranks the machine code");
    }

    #[test]
    fn an_empty_listing_still_produces_usable_widths() {
        let app = app();
        let (address, bytes) = super::column_widths(&app, 120);
        assert_eq!((address, bytes), (8, 0));
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

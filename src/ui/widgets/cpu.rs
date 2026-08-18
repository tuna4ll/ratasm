//! The panels showing processor state: registers, flags, the stack and memory.
//!
//! All four depend on a paused debug session. When there is none they say so
//! rather than drawing an empty box, because an empty box reads as a bug and
//! sends the user looking for one.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::panel::Panel;
use crate::app::App;
use crate::debugger::memory::MemoryBlock;
use crate::debugger::registers::{format_value, RegisterEntry};
use crate::instruction::conditions::ConditionCode;
use crate::instruction::registers::RegisterWidth;
use crate::instruction::Flag;
use crate::ui::Theme;

/// The message shown when a panel needs a session that is not there.
const NO_SESSION: &str =
    "No debug session.\n\nPress F12 to start one, or F6 to build first.\nRegisters, flags, the \
     stack and memory are read from the running program.";

/// Draws the register panel.
pub fn draw_registers(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let block = super::panel_block(&app.theme, Panel::Registers, focused);

    if app.registers.is_empty() {
        super::draw_placeholder(frame, area, &app.theme, block, NO_SESSION);
        return;
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let theme = &app.theme;
    let symbols = theme.symbols();
    let entries = app.registers.entries();

    // Two columns when there is room; the general-purpose registers are the
    // point of the panel and sixteen of them do not fit in one column.
    let value_width = match app.register_format {
        crate::debugger::registers::Format::Hex => 18,
        _ => 20,
    };
    let column_width = 7 + value_width + 2;
    let columns = usize::from(inner.width).max(1) / column_width.max(1);
    let columns = columns.clamp(1, 2);

    let mut lines: Vec<Line> = Vec::new();
    let mut row: Vec<Span> = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        row.extend(register_spans(entry, app, theme));
        if (index + 1) % columns == 0 {
            lines.push(Line::from(std::mem::take(&mut row)));
        } else {
            row.push(Span::raw("  "));
        }
    }
    if !row.is_empty() {
        lines.push(Line::from(row));
    }

    // A footer explaining the highlighted register turns the panel from a
    // dump into something that teaches.
    if let Some(entry) = entries.iter().find(|entry| entry.has_changed()) {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", symbols.changed), theme.changed()),
            Span::styled(
                format!(
                    "{} changed: {}",
                    entry.register.display_name(),
                    entry.register.summary
                ),
                theme.dim(),
            ),
        ]));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

/// The spans for one register row.
fn register_spans<'a>(entry: &RegisterEntry, app: &App, theme: &Theme) -> Vec<Span<'a>> {
    let symbols = theme.symbols();
    let changed = entry.has_changed();

    let marker = if changed {
        Span::styled(symbols.changed.to_owned(), theme.changed())
    } else {
        Span::raw(symbols.unchanged.to_owned())
    };

    let name = Span::styled(
        format!("{:<6}", entry.register.display_name()),
        if changed {
            theme.changed()
        } else {
            theme.dim()
        },
    );

    let value = format_value(entry.value, RegisterWidth::Qword, app.register_format);
    let value = Span::styled(
        value,
        if changed {
            theme.changed()
        } else {
            theme.base()
        },
    );

    vec![marker, name, value]
}

/// Draws the flags panel.
pub fn draw_flags(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let block = super::panel_block(&app.theme, Panel::Flags, focused);

    let Some(flags) = app.registers.flags() else {
        super::draw_placeholder(frame, area, &app.theme, block, NO_SESSION);
        return;
    };
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let theme = &app.theme;
    let symbols = theme.symbols();
    let previous = app.registers.previous_flags();
    let changed = previous
        .map(|previous| flags.changed_from(previous))
        .unwrap_or_default();

    let mut lines: Vec<Line> = Vec::new();
    let mut row: Vec<Span> = Vec::new();

    for (index, flag) in Flag::ALL.iter().enumerate() {
        let set = flags.has(*flag);
        let just_changed = changed.contains(flag);

        let style = if just_changed {
            theme.changed()
        } else if set {
            theme.success()
        } else {
            theme.dim()
        };

        row.push(Span::styled(
            format!("{} {} ", flag.abbreviation(), symbols.flag(set)),
            style,
        ));

        // Three per row keeps the nine flags readable in a narrow panel.
        if (index + 1) % 3 == 0 {
            lines.push(Line::from(std::mem::take(&mut row)));
        }
    }
    if !row.is_empty() {
        lines.push(Line::from(row));
    }

    // The panel's real value: which branches would be taken right now.
    let taken: Vec<String> = ConditionCode::ALL
        .into_iter()
        .filter(|code| code.evaluate(flags))
        .flat_map(ConditionCode::jump_mnemonics)
        .collect();

    if !taken.is_empty() && inner.height > lines.len() as u16 + 1 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Would be taken:", theme.dim())));
        let text = super::truncate(&taken.join(" "), usize::from(inner.width), symbols.ellipsis);
        lines.push(Line::from(Span::styled(text, theme.success())));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

/// Draws the stack panel.
pub fn draw_stack(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let block = super::panel_block(&app.theme, Panel::Stack, focused);

    if app.stack.is_empty() {
        super::draw_placeholder(frame, area, &app.theme, block, NO_SESSION);
        return;
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let theme = &app.theme;
    let symbols = theme.symbols();
    let rsp = app.registers.rsp();
    let rbp = app.registers.rbp();

    let mut lines: Vec<Line> = Vec::new();
    let mut address = app.stack.address;

    // The stack is read eight bytes at a time because that is the width the
    // architecture pushes and pops.
    while address + 8 <= app.stack.end_address() && lines.len() < usize::from(inner.height) {
        let Some(value) = app.stack.read_integer(address, 8) else {
            break;
        };

        let marker = if Some(address) == rsp {
            Span::styled(format!("{} ", symbols.stack_pointer), theme.current_line())
        } else if Some(address) == rbp {
            Span::styled(format!("{} ", symbols.base_pointer), theme.accent())
        } else {
            Span::raw("  ".to_owned())
        };

        let label = if Some(address) == rsp {
            Span::styled("RSP ", theme.current_line())
        } else if Some(address) == rbp {
            Span::styled("RBP ", theme.accent())
        } else {
            Span::raw("    ".to_owned())
        };

        lines.push(Line::from(vec![
            marker,
            label,
            Span::styled(format!("{address:016x}  "), theme.address()),
            Span::styled(format!("{value:016x}"), theme.base()),
        ]));

        address = address.wrapping_add(8);
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

/// Draws the memory panel.
pub fn draw_memory(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let block = super::panel_block(&app.theme, Panel::Memory, focused);

    if app.memory.is_empty() {
        let message = if app.registers.is_empty() {
            NO_SESSION
        } else {
            "No address selected.\n\nPress Ctrl+G and type an address: 0x4000b0, rsp-0x20, rbp+8."
        };
        super::draw_placeholder(frame, area, &app.theme, block, message);
        return;
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(hex_dump(&app.memory, &app.theme, usize::from(inner.height))),
        inner,
    );
}

/// Renders a memory block as hex-dump lines.
fn hex_dump<'a>(block: &MemoryBlock, theme: &Theme, rows: usize) -> Vec<Line<'a>> {
    block
        .rows()
        .into_iter()
        .take(rows)
        .map(|row| {
            Line::from(vec![
                Span::styled(format!("{}  ", row.address_text()), theme.address()),
                Span::styled(format!("{}  ", row.hex()), theme.base()),
                Span::styled(format!("|{}|", row.ascii()), theme.dim()),
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::memory::MemoryBlock;
    use crate::ui::theme::ThemeKind;

    fn theme() -> Theme {
        Theme::new(ThemeKind::Dark, true)
    }

    #[test]
    fn a_hex_dump_line_has_all_three_columns() {
        let block = MemoryBlock::new(0x4000, b"Hello, world!\n\0\0".to_vec());
        let lines = hex_dump(&block, &theme(), 10);
        assert_eq!(lines.len(), 1);

        let text: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.starts_with("0000000000004000"));
        assert!(text.contains("48 65 6c 6c 6f"));
        assert!(text.contains("|Hello, world!...|"));
    }

    #[test]
    fn the_hex_dump_is_limited_to_the_rows_that_fit() {
        let block = MemoryBlock::new(0x4000, vec![0; 16 * 20]);
        assert_eq!(hex_dump(&block, &theme(), 5).len(), 5);
        assert_eq!(hex_dump(&block, &theme(), 100).len(), 20);
    }

    #[test]
    fn an_empty_block_produces_no_lines() {
        assert!(hex_dump(&MemoryBlock::default(), &theme(), 10).is_empty());
    }

    #[test]
    fn the_no_session_message_says_what_to_press() {
        // A dead end with no next step is the worst kind of empty state.
        assert!(NO_SESSION.contains("F12"));
        assert!(NO_SESSION.contains("F6"));
    }
}

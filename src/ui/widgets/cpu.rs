//! The panels showing processor state: registers, flags, the stack and memory.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
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

/// Columns reserved for a register's name; `RFLAGS` plus a separating space.
const NAME_WIDTH: usize = 7;

/// Columns one flag takes: two for the name, one space, the glyph, a gap.
const FLAG_WIDTH: usize = 5;

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

    let lines = register_lines(app, usize::from(inner.width));
    super::draw_scrolled(frame, app, Panel::Registers, inner, lines, None);
}

/// The register rows for a panel `width` columns wide.
pub fn register_lines<'a>(app: &App, width: usize) -> Vec<Line<'a>> {
    let theme = &app.theme;
    let symbols = theme.symbols();
    let entries = app.registers.entries();
    if entries.is_empty() {
        return Vec::new();
    }

    let value_width = match app.register_format {
        crate::debugger::registers::Format::Hex => 18,
        _ => 20,
    };
    let column_width = 1 + NAME_WIDTH + value_width + 2;
    let columns = (width.max(1) / column_width.max(1)).clamp(1, 2);

    let mut lines: Vec<Line> = Vec::new();
    let mut row: Vec<Span> = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        if columns == 1 && entry.register.name == "rip" && !lines.is_empty() {
            lines.push(Line::from(""));
        }
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

    if let Some(entry) = entries.iter().find(|entry| entry.has_changed()) {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", symbols.changed), theme.changed()),
            Span::styled(
                super::truncate(
                    &format!(
                        "{} changed: {}",
                        entry.register.display_name(),
                        entry.register.summary
                    ),
                    width.saturating_sub(3),
                    symbols.ellipsis,
                ),
                theme.dim(),
            ),
        ]));
    }

    lines
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
        format!("{:<NAME_WIDTH$}", entry.register.display_name()),
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

    if app.registers.flags().is_none() {
        super::draw_placeholder(frame, area, &app.theme, block, NO_SESSION);
        return;
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = flag_lines(app, usize::from(inner.width));
    super::draw_scrolled(frame, app, Panel::Flags, inner, lines, None);
}

/// The flag rows and the jumps they would take, for a panel `width` wide.
pub fn flag_lines<'a>(app: &App, width: usize) -> Vec<Line<'a>> {
    let Some(flags) = app.registers.flags() else {
        return Vec::new();
    };

    let theme = &app.theme;
    let symbols = theme.symbols();
    let changed = app
        .registers
        .previous_flags()
        .map(|previous| flags.changed_from(previous))
        .unwrap_or_default();

    let per_row = match width / FLAG_WIDTH {
        fits if fits >= Flag::ALL.len() => Flag::ALL.len(),
        fits if fits >= 6 => 6,
        _ => 3,
    };

    let mut lines: Vec<Line> = Vec::new();
    let mut row: Vec<Span> = Vec::new();

    for (index, flag) in Flag::ALL.iter().enumerate() {
        let set = flags.has(*flag);
        let style = if changed.contains(flag) {
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

        if (index + 1) % per_row == 0 {
            lines.push(Line::from(std::mem::take(&mut row)));
        }
    }
    if !row.is_empty() {
        lines.push(Line::from(row));
    }

    let taken: Vec<String> = ConditionCode::ALL
        .into_iter()
        .filter(|code| code.evaluate(flags))
        .flat_map(ConditionCode::jump_mnemonics)
        .collect();

    if !taken.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Would be taken:", theme.dim())));
        for chunk in taken.chunks(width.max(8) / 5) {
            lines.push(Line::from(Span::styled(chunk.join(" "), theme.success())));
        }
    }

    lines
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

    let lines = stack_lines(app);
    super::draw_scrolled(frame, app, Panel::Stack, inner, lines, None);
}

/// One row per eight-byte stack slot, the width the architecture pushes.
pub fn stack_lines<'a>(app: &App) -> Vec<Line<'a>> {
    let theme = &app.theme;
    let symbols = theme.symbols();
    let rsp = app.registers.rsp();
    let rbp = app.registers.rbp();

    let mut lines: Vec<Line> = Vec::new();
    let mut address = app.stack.address;

    while address + 8 <= app.stack.end_address() {
        let Some(value) = app.stack.read_integer(address, 8) else {
            break;
        };

        let (marker, label) = if Some(address) == rsp {
            (
                Span::styled(format!("{} ", symbols.stack_pointer), theme.current_line()),
                Span::styled("RSP ", theme.current_line()),
            )
        } else if Some(address) == rbp {
            (
                Span::styled(format!("{} ", symbols.base_pointer), theme.accent()),
                Span::styled("RBP ", theme.accent()),
            )
        } else {
            (Span::raw("  ".to_owned()), Span::raw("    ".to_owned()))
        };

        lines.push(Line::from(vec![
            marker,
            label,
            Span::styled(format!("{address:016x}  "), theme.address()),
            Span::styled(format!("{value:016x}"), theme.base()),
        ]));

        address = address.wrapping_add(8);
    }

    lines
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

    let lines = memory_lines(app);
    super::draw_scrolled(frame, app, Panel::Memory, inner, lines, None);
}

/// The hex dump of the memory panel's block.
pub fn memory_lines<'a>(app: &App) -> Vec<Line<'a>> {
    hex_dump(&app.memory, &app.theme, usize::MAX)
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
        assert!(NO_SESSION.contains("F12"));
        assert!(NO_SESSION.contains("F6"));
    }

    #[test]
    fn the_name_column_leaves_a_space_after_the_longest_name() {
        let longest = crate::instruction::registers::all()
            .iter()
            .map(|register| register.display_name().chars().count())
            .max()
            .expect("there are registers");
        assert!(
            longest < NAME_WIDTH,
            "{longest} characters do not fit a {NAME_WIDTH}-wide column with a gap"
        );
    }
}

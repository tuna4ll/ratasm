//! Drawing one frame.
//!
//! This is the only entry point into rendering. It takes an immutable `&App`,
//! so a frame can be drawn at any moment without the act of drawing changing
//! anything — a guarantee worth having when the alternative is chasing a
//! redraw that quietly moved the cursor.

use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::app::App;
use crate::ui::layout::{self, LayoutMode};
use crate::ui::widgets::{self, chrome};

/// Draws the whole interface.
pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Paint the background explicitly: a transparent body would inherit the
    // terminal's colours and fight the theme.
    frame.render_widget(
        Block::default()
            .borders(Borders::NONE)
            .style(app.theme.base()),
        area,
    );

    let layout = layout::compute(area, app.focus);

    if layout.mode == LayoutMode::TooSmall {
        chrome::draw_too_small(frame, area);
        return;
    }

    chrome::draw_document_bar(frame, app, layout.document_bar);

    if let Some(tab_bar) = layout.tab_bar {
        chrome::draw_tab_bar(frame, app, tab_bar, &layout.tabbed);
    }

    for (panel, rect) in &layout.panels {
        widgets::draw_panel(frame, app, *panel, *rect);
    }

    chrome::draw_status_bar(frame, app, layout.status_bar);

    // Last, so it sits above everything and owns the cursor.
    chrome::draw_overlay(frame, app, area);
}

/// Scrolls the active document so the cursor is visible in `area`.
///
/// Called before drawing, because the viewport depends on the height the
/// terminal happens to have and the document deliberately does not know it.
pub fn sync_scroll(app: &mut App, width: u16, height: u16) {
    let layout = layout::compute(ratatui::layout::Rect::new(0, 0, width, height), app.focus);
    let Some(area) = layout.area_of(crate::app::panel::Panel::Editor) else {
        return;
    };

    // Subtract the borders and the gutter the editor draws inside its area.
    let buffer_lines = app.workspace.active().buffer().line_count();
    let gutter = if app.settings.editor.line_numbers {
        buffer_lines.to_string().len().max(2) + 1
    } else {
        0
    };
    let text_width = usize::from(area.width)
        .saturating_sub(2)
        .saturating_sub(gutter + 2);
    let text_height = usize::from(area.height).saturating_sub(2);

    app.workspace
        .active_mut()
        .scroll_into_view(text_height, text_width);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::project::Project;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn app() -> App {
        let project = Project::for_file(std::path::Path::new("/tmp/ratasm-test/main.asm"));
        App::new(project, Settings::default()).expect("databases load")
    }

    /// Renders one frame and returns the screen as text, one string per row.
    fn render(app: &App, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| draw(frame, app)).expect("draw");

        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| {
                        buffer
                            .cell((x, y))
                            .map_or(" ", ratatui::buffer::Cell::symbol)
                    })
                    .collect::<String>()
            })
            .collect()
    }

    fn screen(app: &App, width: u16, height: u16) -> String {
        render(app, width, height).join("\n")
    }

    #[test]
    fn a_wide_terminal_shows_the_main_panels() {
        let app = app();
        let text = screen(&app, 160, 48);

        for title in ["Editor", "Registers", "Flags", "Disassembly", "Explain"] {
            assert!(text.contains(title), "{title} is missing from the screen");
        }
    }

    #[test]
    fn the_status_bar_shows_the_state_and_position() {
        let app = app();
        let rows = render(&app, 160, 48);
        let status = rows.last().expect("a status bar");

        assert!(status.contains("1:1"), "cursor position: {status}");
        assert!(status.contains("idle"), "debugger state: {status}");
    }

    #[test]
    fn source_text_reaches_the_screen() {
        let mut app = app();
        app.workspace
            .active_mut()
            .insert("section .text\n_start:\n    mov rax, 60\n");

        let text = screen(&app, 160, 48);
        assert!(text.contains("mov rax, 60"), "the source is not drawn");
        assert!(text.contains("_start:"));
    }

    #[test]
    fn panels_with_no_data_explain_themselves() {
        // An empty box reads as a bug; a sentence does not.
        let app = app();
        let text = screen(&app, 160, 48);
        assert!(
            text.contains("No debug session"),
            "the register panel should explain why it is empty"
        );
    }

    #[test]
    fn a_narrow_terminal_still_renders_the_focused_panel() {
        let app = app();
        let text = screen(&app, 60, 20);
        assert!(text.contains("Editor"), "the focused panel is missing");
    }

    #[test]
    fn a_tiny_terminal_says_so_instead_of_drawing_rubbish() {
        let app = app();
        let text = screen(&app, 20, 8);
        assert!(text.contains("too small"), "expected a message: {text}");
    }

    #[test]
    fn every_size_renders_without_panicking() {
        // Terminals get resized to strange shapes; none of them may crash.
        let app = app();
        for width in [1u16, 5, 20, 40, 79, 80, 119, 120, 200] {
            for height in [1u16, 3, 9, 10, 27, 28, 50] {
                let _ = render(&app, width, height);
            }
        }
    }

    #[test]
    fn every_panel_can_be_focused_and_drawn_at_every_size() {
        let mut app = app();
        for panel in crate::app::panel::Panel::ALL {
            app.focus = panel;
            for (width, height) in [(160u16, 48u16), (100, 30), (60, 20), (45, 12)] {
                let text = render(&app, width, height).join("\n");
                assert!(!text.is_empty(), "{panel} drew nothing at {width}x{height}");
            }
        }
    }

    #[test]
    fn the_palette_appears_over_the_interface() {
        let mut app = app();
        app.apply(&crate::command::Command::OpenPalette);

        let text = screen(&app, 160, 48);
        assert!(text.contains("Command palette"));
        assert!(text.contains("Build"), "commands should be listed");
    }

    #[test]
    fn the_palette_shows_the_shortcut_for_a_command() {
        // So the palette teaches the bindings rather than replacing them.
        let mut app = app();
        app.apply(&crate::command::Command::OpenPalette);
        let text = screen(&app, 160, 48);
        assert!(text.contains("F6") || text.contains("ctrl+"), "{text}");
    }

    #[test]
    fn a_prompt_appears_with_its_hint() {
        let mut app = app();
        app.apply(&crate::command::Command::GoToAddress);

        let text = screen(&app, 160, 48);
        assert!(text.contains("Go to address"));
        assert!(text.contains("rsp-0x20"), "the hint should be shown");
    }

    #[test]
    fn an_error_status_is_drawn_with_its_message() {
        let mut app = app();
        app.status = crate::app::Status::error("something went wrong");

        let rows = render(&app, 160, 48);
        let status = rows.last().expect("status bar");
        assert!(status.contains("something went wrong"), "{status}");
    }

    #[test]
    fn a_long_status_message_is_truncated_rather_than_wrapping() {
        let mut app = app();
        app.status = crate::app::Status::info("x".repeat(500));

        let rows = render(&app, 100, 30);
        assert_eq!(rows.len(), 30, "the layout must not grow");
        // Counted in characters, not bytes: the ellipsis is three bytes.
        assert_eq!(
            rows.last().map(|row| row.chars().count()),
            Some(100),
            "the status bar stays exactly one row wide"
        );
    }

    #[test]
    fn breakpoints_are_marked_in_the_gutter() {
        let mut app = app();
        app.workspace
            .active_mut()
            .set_path("/tmp/ratasm-test/main.asm");
        app.workspace.active_mut().insert("nop\nnop\nnop\n");
        app.workspace.active_mut().go_to_line(2);
        app.apply(&crate::command::Command::ToggleBreakpoint);

        let text = screen(&app, 160, 48);
        let glyph = app.theme.symbols().breakpoint;
        assert!(text.contains(glyph), "the breakpoint marker is missing");
    }

    #[test]
    fn the_explanation_panel_shows_the_instruction_under_the_cursor() {
        let mut app = app();
        app.workspace.active_mut().insert("    add rax, rbx\n");
        app.workspace.active_mut().move_cursor(
            crate::editor::Movement::To(crate::editor::Position::new(0, 6)),
            crate::editor::SelectionMode::Collapse,
        );

        let text = screen(&app, 160, 48);
        assert!(text.contains("ADD"), "the mnemonic should be shown");
    }

    #[test]
    fn scrolling_follows_the_cursor_into_a_long_file() {
        let mut app = app();
        let source: String = (1..=200).map(|n| format!("    nop  ; {n}\n")).collect();
        app.workspace.active_mut().insert(&source);
        app.workspace.active_mut().go_to_line(150);

        sync_scroll(&mut app, 160, 48);
        let text = screen(&app, 160, 48);
        assert!(text.contains("; 150"), "line 150 should be on screen");
    }

    #[test]
    fn rendering_does_not_change_the_application() {
        // Drawing reads; commands write. A renderer with side effects would
        // make every redraw a potential state change.
        let mut app = app();
        app.workspace.active_mut().insert("    mov rax, 1\n");

        let before = (
            app.focus,
            app.workspace.active().cursor(),
            app.status.text.clone(),
            app.workspace.active().buffer().to_text(),
        );
        let _ = render(&app, 160, 48);
        let after = (
            app.focus,
            app.workspace.active().cursor(),
            app.status.text.clone(),
            app.workspace.active().buffer().to_text(),
        );

        assert_eq!(before, after);
    }

    #[test]
    fn every_theme_renders() {
        let mut app = app();
        for kind in crate::ui::theme::ThemeKind::ALL {
            app.theme.set_kind(kind);
            let text = screen(&app, 160, 48);
            assert!(text.contains("Editor"), "{kind} failed to render");
        }
    }

    #[test]
    fn an_ascii_only_terminal_renders_without_unicode_glyphs() {
        let mut app = app();
        app.theme = crate::ui::Theme::new(crate::ui::theme::ThemeKind::Ansi16, false);
        app.workspace
            .active_mut()
            .set_path("/tmp/ratasm-test/main.asm");
        app.workspace.active_mut().insert("nop\n");
        app.apply(&crate::command::Command::ToggleBreakpoint);

        let text = screen(&app, 160, 48);
        // Box-drawing characters come from ratatui's borders, so only the
        // content glyphs are checked here.
        assert!(text.contains('*'), "the ASCII breakpoint marker is missing");
    }
}

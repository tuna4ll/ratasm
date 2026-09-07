//! Drawing one frame. [`draw`] takes an immutable `&App`, so drawing can never

use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::app::App;
use crate::ui::layout::{self, LayoutMode};
use crate::ui::widgets::{self, chrome};

/// Draws the whole interface.
pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    frame.render_widget(
        Block::default()
            .borders(Borders::NONE)
            .style(app.theme.base()),
        area,
    );

    let layout = layout::compute(area, app.page, app.focus);

    if layout.mode == LayoutMode::TooSmall {
        chrome::draw_too_small(frame, area);
        return;
    }

    chrome::draw_page_bar(frame, app, layout.page_bar);

    if let Some(tab_bar) = layout.tab_bar {
        chrome::draw_tab_bar(frame, app, tab_bar, &layout.tabbed);
    }

    for (panel, rect) in &layout.panels {
        widgets::draw_panel(frame, app, *panel, *rect);
    }

    chrome::draw_status_bar(frame, app, layout.status_bar);

    chrome::draw_overlay(frame, app, area);
}

/// Brings every viewport in line with the room the terminal actually has.
pub fn sync_scroll(app: &mut App, width: u16, height: u16) {
    use crate::app::panel::Panel;
    use crate::app::scroll::ScrollState;

    let layout = layout::compute(
        ratatui::layout::Rect::new(0, 0, width, height),
        app.page,
        app.focus,
    );

    for (panel, area) in layout.panels.clone() {
        if !ScrollState::is_scrollable(panel) {
            continue;
        }
        let inner = inner_area(area);
        let rows = widgets::content_rows(app, panel, inner.0);
        app.scroll.fit(panel, rows, usize::from(inner.1));
    }

    let Some(area) = layout.area_of(Panel::Editor) else {
        return;
    };

    let buffer_lines = app.workspace.active().buffer().line_count();
    let gutter = if app.settings.editor.line_numbers {
        buffer_lines.to_string().len().max(2) + 1
    } else {
        0
    };
    let (inner_width, inner_height) = inner_area(area);
    let text_width = usize::from(inner_width).saturating_sub(gutter + 2);

    app.workspace
        .active_mut()
        .scroll_into_view(usize::from(inner_height), text_width);
}

/// The width and height inside a panel's border and horizontal padding.
fn inner_area(area: ratatui::layout::Rect) -> (u16, u16) {
    (area.width.saturating_sub(4), area.height.saturating_sub(2))
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

    /// Renders after a scroll sync, the way the run loop does.
    fn settled(app: &mut App, width: u16, height: u16) -> String {
        sync_scroll(app, width, height);
        screen(app, width, height)
    }

    #[test]
    fn a_long_build_log_can_be_read_from_either_end() {
        let mut app = app();
        app.output = (1..=60).map(|n| format!("line-{n:02}")).collect();
        app.focus_panel(crate::app::Panel::Output);

        let text = settled(&mut app, 120, 30);
        assert!(text.contains("line-60"), "a log opens at its newest line");
        assert!(!text.contains("line-01"), "which is not its oldest");

        app.apply(&crate::command::Command::ScrollToTop);
        let text = settled(&mut app, 120, 30);
        assert!(text.contains("line-01"), "and can be wound back:\n{text}");

        app.apply(&crate::command::Command::ScrollToEnd);
        let text = settled(&mut app, 120, 30);
        assert!(text.contains("line-60"));
    }

    #[test]
    fn a_panel_with_more_content_than_room_says_so() {
        let mut app = app();
        app.output = (1..=60).map(|n| format!("line-{n:02}")).collect();
        let crowded = settled(&mut app, 120, 30);

        app.output = vec!["only one line".to_owned()];
        app.scroll.reset(crate::app::Panel::Output);
        let roomy = settled(&mut app, 120, 30);

        let thumb = app.theme.symbols().scroll_thumb;
        assert!(
            crowded.contains(thumb),
            "a scrollbar marks what is off-screen"
        );
        assert!(
            !roomy.contains(thumb),
            "and stays away when everything fits"
        );
    }

    #[test]
    fn scrolling_stops_where_the_content_does() {
        let mut app = app();
        app.output = (1..=60).map(|n| format!("line-{n:02}")).collect();
        app.focus_panel(crate::app::Panel::Output);
        sync_scroll(&mut app, 120, 30);

        for _ in 0..100 {
            app.apply(&crate::command::Command::ScrollDown);
            sync_scroll(&mut app, 120, 30);
        }
        let extent = app.scroll.extent(crate::app::Panel::Output);
        assert_eq!(extent.offset, extent.max_offset());
        assert!(settled(&mut app, 120, 30).contains("line-60"));
    }

    #[test]
    fn a_wide_terminal_shows_the_main_panels() {
        let mut app = app();
        app.open_page(crate::app::Page::Debug);
        let text = screen(&app, 160, 48);

        for title in ["Editor", "Registers", "Flags", "Disassembly", "Explain"] {
            assert!(text.contains(title), "{title} is missing from the screen");
        }
    }

    #[test]
    fn the_page_bar_names_every_page_and_marks_the_open_one() {
        let app = app();
        let bar = render(&app, 160, 48).remove(0);

        for page in crate::app::Page::ALL {
            assert!(bar.contains(page.title()), "{page} is missing: {bar}");
            assert!(
                bar.contains(&page.number().to_string()),
                "the shortcut number for {page} is missing: {bar}"
            );
        }
        assert!(
            bar.contains(&app.workspace.active().display_name()),
            "the open file is missing: {bar}"
        );
    }

    #[test]
    fn each_page_draws_its_own_panels_and_no_others() {
        let mut app = app();
        for page in crate::app::Page::ALL {
            app.open_page(page);
            let text = screen(&app, 160, 48);
            for panel in page.panels() {
                let shown = text.contains(panel.title());
                assert!(shown, "{panel} is nowhere on the {page} page:\n{text}");
            }
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
        let mut app = app();
        app.open_page(crate::app::Page::Debug);
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
            app.focus_panel(panel);
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
        assert!(
            text.contains("New file"),
            "commands should be listed:\n{text}"
        );
        assert!(text.contains("ctrl+n"), "bindings should be listed");
    }

    #[test]
    fn the_palette_shows_the_shortcut_for_a_command() {
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
        app.open_page(crate::app::Page::Debug);
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
        assert!(text.contains('*'), "the ASCII breakpoint marker is missing");
    }
}

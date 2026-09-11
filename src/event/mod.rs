//! Turning terminal input into state changes.

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::app::mode::{Mode, PromptKind};
use crate::app::panel::Panel;
#[cfg(test)]
use crate::app::state::Severity;
use crate::app::state::{Effect, Status};
use crate::app::App;
use crate::command::Command;
use crate::editor::{Movement, SelectionMode};

/// How many lines a page key moves.
const PAGE_LINES: usize = 20;

/// Handles one key event, returning any I/O it implies.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Effect {
    if key.kind == KeyEventKind::Release {
        return Effect::None;
    }

    if app.mode.is_overlay() {
        return handle_overlay(app, key);
    }

    let editor_claims_key = app.focus.is_text_input()
        && matches!(key.code, KeyCode::Char(_) | KeyCode::Tab | KeyCode::BackTab)
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);

    if !editor_claims_key {
        if let Some(command) = app.keymap.command_for_event(key).cloned() {
            return app.apply(&command);
        }
    }

    match app.focus {
        Panel::Editor => handle_editor(app, key),
        Panel::Syscalls => handle_syscalls(app, key),
        Panel::Breakpoints => handle_breakpoints(app, key),
        Panel::Scratchpad => handle_scratchpad(app, key),
        Panel::Learn => handle_learn(app, key),
        Panel::Explorer => handle_explorer(app, key),
        panel => handle_scrollable(app, panel, key),
    }
}

/// Whether the open prompt is one Tab should complete.
fn prompt_completes_paths(app: &App) -> bool {
    match &app.mode {
        Mode::Prompt(prompt) => prompt.kind().is_some_and(PromptKind::completes_paths),
        _ => false,
    }
}

/// Extends the prompt's text as far as the filesystem agrees.
fn complete_prompt_path(app: &mut App) {
    let Mode::Prompt(prompt) = &app.mode else {
        return;
    };
    let (completed, candidates) = crate::editor::workspace::complete_path(prompt.text());

    let unchanged = completed == prompt.text();
    if let Mode::Prompt(prompt) = &mut app.mode {
        prompt.set_text(completed);
    }

    app.status = match candidates.len() {
        0 => Status::warning("No file matches"),
        1 => Status::info(candidates[0].clone()),
        _ if unchanged => Status::info(candidates.join("  ")),
        count => Status::info(format!("{count} matches")),
    };
}

/// Moves the explorer's selection, or opens the file it is on.
fn handle_explorer(app: &mut App, key: KeyEvent) -> Effect {
    match key.code {
        KeyCode::Up => {
            app.move_explorer_selection(-1);
            Effect::None
        }
        KeyCode::Down => {
            app.move_explorer_selection(1);
            Effect::None
        }
        KeyCode::Enter => app.open_selected_file(),
        KeyCode::Char('r') => {
            app.refresh_project_files();
            app.status = Status::info("File list refreshed");
            Effect::None
        }
        _ => handle_scrollable(app, Panel::Explorer, key),
    }
}

/// Moves a panel that has no other use for the navigation keys.
fn handle_scrollable(app: &mut App, panel: Panel, key: KeyEvent) -> Effect {
    if !crate::app::ScrollState::is_scrollable(panel) {
        return Effect::None;
    }

    let command = match key.code {
        KeyCode::Up => Command::ScrollUp,
        KeyCode::Down => Command::ScrollDown,
        KeyCode::PageUp => Command::ScrollPageUp,
        KeyCode::PageDown => Command::ScrollPageDown,
        KeyCode::Home => Command::ScrollToTop,
        KeyCode::End => Command::ScrollToEnd,
        _ => return Effect::None,
    };
    app.apply(&command)
}

/// Handles one mouse event against the layout a terminal of `width` by
pub fn handle_mouse(app: &mut App, mouse: MouseEvent, width: u16, height: u16) -> Effect {
    use crate::ui::layout;

    if app.mode.is_overlay() {
        return Effect::None;
    }

    let layout = layout::compute(
        ratatui::layout::Rect::new(0, 0, width, height),
        app.page,
        app.focus,
    );
    let (column, row) = (mouse.column, mouse.row);

    match mouse.kind {
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let down = mouse.kind == MouseEventKind::ScrollDown;
            let Some(panel) = layout.panel_at(column, row) else {
                return Effect::None;
            };
            if !crate::app::ScrollState::is_scrollable(panel) {
                return scroll_text_panel(app, panel, down);
            }
            let step = crate::app::scroll::STEP as isize;
            app.scroll.scroll_by(panel, if down { step } else { -step });
            Effect::None
        }

        MouseEventKind::Down(MouseButton::Left) => {
            if row == layout.page_bar.y {
                return click_page_bar(app, column);
            }
            if let Some(panel) = layout.tab_at(column, row) {
                app.focus_panel(panel);
                return Effect::None;
            }
            let Some(panel) = layout.panel_at(column, row) else {
                return Effect::None;
            };
            app.focus_panel(panel);
            match panel {
                Panel::Editor => {
                    if let Some(area) = layout.area_of(Panel::Editor) {
                        place_cursor(app, area, column, row);
                    }
                    Effect::None
                }
                Panel::Explorer => layout
                    .area_of(Panel::Explorer)
                    .map_or(Effect::None, |area| select_file(app, area, row)),
                _ => Effect::None,
            }
        }

        _ => Effect::None,
    }
}

/// Selects and opens the explorer file the pointer landed on.
fn select_file(app: &mut App, area: ratatui::layout::Rect, row: u16) -> Effect {
    let first = area.y + 1;
    if row < first {
        return Effect::None;
    }
    let line = app.scroll.offset(Panel::Explorer) + usize::from(row - first);
    let Some(index) = line.checked_sub(2) else {
        return Effect::None;
    };
    if index >= app.project_files.len() {
        return Effect::None;
    }
    app.explorer_selected = index;
    app.open_selected_file()
}

/// Scrolls the editor or scratchpad, which move a cursor rather than a view.
fn scroll_text_panel(app: &mut App, panel: Panel, down: bool) -> Effect {
    if panel != Panel::Editor {
        return Effect::None;
    }
    let movement = if down { Movement::Down } else { Movement::Up };
    for _ in 0..crate::app::scroll::STEP {
        app.workspace
            .active_mut()
            .move_cursor(movement, SelectionMode::Collapse);
    }
    Effect::None
}

/// Opens the page or document whose label was clicked in the page bar.
fn click_page_bar(app: &mut App, column: u16) -> Effect {
    let mut offset = 0u16;
    for page in crate::app::Page::ALL {
        let width = format!(" {} {} ", page.number(), page.title())
            .chars()
            .count() as u16;
        if column < offset + width {
            app.open_page(page);
            return Effect::None;
        }
        offset += width;
    }

    offset += 5;
    for (index, document) in app.workspace.documents().iter().enumerate() {
        let modified = usize::from(document.is_modified());
        let width = document.display_name().chars().count() as u16 + 2 + modified as u16;
        if column >= offset && column < offset + width {
            app.workspace.set_active(index);
            app.focus_panel(Panel::Editor);
            return Effect::None;
        }
        offset += width;
    }
    Effect::None
}

/// Puts the editor cursor at the cell that was clicked.
fn place_cursor(app: &mut App, area: ratatui::layout::Rect, column: u16, row: u16) {
    let document = app.workspace.active();
    let gutter = if app.settings.editor.line_numbers {
        document.buffer().line_count().to_string().len().max(2) + 1
    } else {
        0
    };
    let text_x = area.x + 2 + 2 + gutter as u16;
    let text_y = area.y + 1;
    if row < text_y || column < text_x {
        return;
    }

    let line = document.scroll_line() + usize::from(row - text_y);
    let offset = document.scroll_column() + usize::from(column - text_x);
    let line = line.min(document.buffer().line_count().saturating_sub(1));
    let position = crate::editor::Position::new(line, offset);

    app.workspace
        .active_mut()
        .move_cursor(Movement::To(position), SelectionMode::Collapse);
}

/// Handles a key while the palette or a prompt is open.
fn handle_overlay(app: &mut App, key: KeyEvent) -> Effect {
    match key.code {
        KeyCode::Esc => {
            app.cancel_overlay();
            Effect::None
        }
        KeyCode::Enter => match &app.mode {
            Mode::Palette(_) => app.accept_palette(),
            _ => app.accept_prompt(),
        },
        KeyCode::Up | KeyCode::BackTab => {
            if let Mode::Palette(palette) = &mut app.mode {
                palette.select_previous();
            }
            Effect::None
        }
        KeyCode::Tab if prompt_completes_paths(app) => {
            complete_prompt_path(app);
            Effect::None
        }
        KeyCode::Down | KeyCode::Tab => {
            if let Mode::Palette(palette) = &mut app.mode {
                palette.select_next();
            }
            Effect::None
        }
        _ => {
            let refresh = matches!(app.mode, Mode::Palette(_));
            let Some(prompt) = prompt_mut(app) else {
                return Effect::None;
            };

            let mut confirmed = None;
            match key.code {
                KeyCode::Char(ch) => {
                    prompt.insert(ch);
                    if let Mode::Prompt(prompt) = &app.mode {
                        if prompt.kind().is_some_and(|kind| kind.is_confirmation()) {
                            confirmed = Some(());
                        }
                    }
                }
                KeyCode::Backspace => prompt.backspace(),
                KeyCode::Delete => prompt.delete(),
                KeyCode::Left => prompt.move_left(),
                KeyCode::Right => prompt.move_right(),
                KeyCode::Home => prompt.move_home(),
                KeyCode::End => prompt.move_end(),
                _ => return Effect::None,
            }

            if confirmed.is_some() {
                return app.accept_prompt();
            }
            if refresh {
                if let Mode::Palette(palette) = &mut app.mode {
                    palette.refresh();
                }
            }
            Effect::None
        }
    }
}

/// The prompt behind whichever overlay is open.
fn prompt_mut(app: &mut App) -> Option<&mut crate::app::mode::Prompt> {
    match &mut app.mode {
        Mode::Prompt(prompt) => Some(prompt),
        Mode::Palette(palette) => Some(palette.prompt_mut()),
        Mode::Normal => None,
    }
}

/// Handles a key with the editor focused.
fn handle_editor(app: &mut App, key: KeyEvent) -> Effect {
    let extend = if key.modifiers.contains(KeyModifiers::SHIFT) {
        SelectionMode::Extend
    } else {
        SelectionMode::Collapse
    };
    let word_wise = key.modifiers.contains(KeyModifiers::CONTROL);
    let document = app.workspace.active_mut();

    match key.code {
        KeyCode::Char(ch) => document.insert_char(ch),
        KeyCode::Enter => document.insert_newline(),
        KeyCode::Backspace => document.backspace(),
        KeyCode::Delete => document.delete_forward(),
        KeyCode::Tab => document.indent(),
        KeyCode::BackTab => document.dedent(),

        KeyCode::Left => document.move_cursor(
            if word_wise {
                Movement::WordLeft
            } else {
                Movement::Left
            },
            extend,
        ),
        KeyCode::Right => document.move_cursor(
            if word_wise {
                Movement::WordRight
            } else {
                Movement::Right
            },
            extend,
        ),
        KeyCode::Up => document.move_cursor(Movement::Up, extend),
        KeyCode::Down => document.move_cursor(Movement::Down, extend),
        KeyCode::Home => document.move_cursor(
            if word_wise {
                Movement::DocumentStart
            } else {
                Movement::LineStart
            },
            extend,
        ),
        KeyCode::End => document.move_cursor(
            if word_wise {
                Movement::DocumentEnd
            } else {
                Movement::LineEnd
            },
            extend,
        ),
        KeyCode::PageUp => document.move_cursor(Movement::PageUp(PAGE_LINES), extend),
        KeyCode::PageDown => document.move_cursor(Movement::PageDown(PAGE_LINES), extend),
        KeyCode::Esc => document.clear_selection(),
        _ => return Effect::None,
    }

    Effect::None
}

/// Handles a key with the syscall panel focused.
fn handle_syscalls(app: &mut App, key: KeyEvent) -> Effect {
    let count = app.matching_syscalls().len();

    match key.code {
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.syscall_query.push(ch);
            app.syscall_selected = 0;
        }
        KeyCode::Backspace => {
            app.syscall_query.pop();
            app.syscall_selected = 0;
        }
        KeyCode::Down => {
            if count > 0 {
                app.syscall_selected = (app.syscall_selected + 1).min(count - 1);
            }
        }
        KeyCode::Up => app.syscall_selected = app.syscall_selected.saturating_sub(1),
        KeyCode::Home => app.syscall_selected = 0,
        KeyCode::End => app.syscall_selected = count.saturating_sub(1),
        KeyCode::Esc => {
            app.syscall_query.clear();
            app.syscall_selected = 0;
        }
        _ => {}
    }

    Effect::None
}

/// Handles a key with the breakpoint list focused.
fn handle_breakpoints(app: &mut App, key: KeyEvent) -> Effect {
    let count = app.breakpoints.len();
    if count == 0 {
        return Effect::None;
    }

    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            app.breakpoint_selected = (app.breakpoint_selected + 1).min(count - 1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.breakpoint_selected = app.breakpoint_selected.saturating_sub(1);
        }
        KeyCode::Delete | KeyCode::Char('d') => {
            app.breakpoints.remove_index(app.breakpoint_selected);
            app.breakpoint_selected = app
                .breakpoint_selected
                .min(app.breakpoints.len().saturating_sub(1));
            return Effect::SyncBreakpoints;
        }
        KeyCode::Char(' ') => {
            let index = app.breakpoint_selected;
            let enabled = app
                .breakpoints
                .all()
                .get(index)
                .is_some_and(|breakpoint| breakpoint.enabled);
            app.breakpoints.set_enabled(index, !enabled);
            return Effect::SyncBreakpoints;
        }
        _ => {}
    }

    Effect::None
}

/// Handles a key with the scratchpad focused.
fn handle_scratchpad(app: &mut App, key: KeyEvent) -> Effect {
    match key.code {
        KeyCode::Tab => return app.apply(&Command::NextPanel),
        KeyCode::BackTab => return app.apply(&Command::PreviousPanel),
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.scratchpad.snippet.push(ch);
        }
        KeyCode::Backspace => {
            app.scratchpad.snippet.pop();
        }
        KeyCode::Esc => {
            app.scratchpad.snippet.clear();
            app.scratchpad_result = None;
        }
        KeyCode::Enter => return submit_scratchpad(app),
        _ => {}
    }

    Effect::None
}

/// Acts on the typed scratchpad line: either a starting value or a snippet.
fn submit_scratchpad(app: &mut App) -> Effect {
    let line = app.scratchpad.snippet.trim().to_owned();

    let Some((name, value)) = parse_assignment(&line) else {
        return Effect::RunScratchpad;
    };

    if value.is_empty() {
        let removed = app.scratchpad.initial.remove(&name.to_ascii_lowercase());
        app.scratchpad.snippet.clear();
        app.status = if removed.is_some() {
            Status::info(format!("{} is no longer set", name.to_uppercase()))
        } else {
            Status::warning(format!("{} was not set", name.to_uppercase()))
        };
        return Effect::None;
    }

    let Some(parsed) = crate::learning::parse_answer(value) else {
        app.status = Status::error(format!("'{value}' is not a number"));
        return Effect::None;
    };

    match app.scratchpad.set(name, parsed) {
        Ok(()) => {
            app.scratchpad.snippet.clear();
            app.status = Status::info(format!("{} starts at {parsed:#x}", name.to_uppercase()));
        }
        Err(error) => app.status = Status::error(error.to_string()),
    }

    Effect::None
}

/// Splits `name=value` into its halves, or returns `None` when there is no `=`.
fn parse_assignment(line: &str) -> Option<(&str, &str)> {
    let (name, value) = line.split_once('=')?;
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return None;
    }
    Some((name, value.trim()))
}

/// Handles a key with the learning panel focused.
fn handle_learn(app: &mut App, key: KeyEvent) -> Effect {
    match key.code {
        KeyCode::Right | KeyCode::Down | KeyCode::PageDown => app.learning.next(),
        KeyCode::Left | KeyCode::Up | KeyCode::PageUp => app.learning.previous(),
        KeyCode::Char('?') => app.learning.jump_to_questions(),
        KeyCode::Enter if app.learning.is_question() => app.learning.submit(),
        KeyCode::Enter => app.learning.next(),
        KeyCode::Esc => app.learning.reset_answer(),
        KeyCode::Backspace if app.learning.is_question() => {
            app.learning.typed.pop();
        }
        KeyCode::Char(ch)
            if app.learning.is_question() && !key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.learning.typed.push(ch);
        }
        _ => {}
    }

    Effect::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::project::Project;

    fn app() -> App {
        let project = Project::for_file(std::path::Path::new("/tmp/ratasm-test/main.asm"));
        App::new(project, Settings::default()).expect("databases load")
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    fn shift(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    /// A mouse event of `kind` at a cell, with no modifiers held.
    fn at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    /// The layout the mouse tests aim at, so a cell can be named by panel.
    fn cell_in(app: &App, panel: Panel, width: u16, height: u16) -> (u16, u16) {
        let layout = crate::ui::layout::compute(
            ratatui::layout::Rect::new(0, 0, width, height),
            app.page,
            app.focus,
        );
        let area = layout.area_of(panel).expect("the panel is drawn");
        (area.x + 2, area.y + 2)
    }

    #[test]
    fn tab_completes_a_path_in_the_open_prompt() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("horizon.asm"), "").expect("write");

        let mut app = app();
        app.apply(&Command::OpenFile);
        type_text(&mut app, &format!("{}/hor", dir.path().display()));
        handle_key(&mut app, press(KeyCode::Tab));

        let Mode::Prompt(prompt) = &app.mode else {
            panic!("the prompt should still be open");
        };
        assert!(prompt.text().ends_with("horizon.asm"), "{}", prompt.text());
    }

    #[test]
    fn tab_still_moves_the_palette_selection() {
        let mut app = app();
        app.apply(&Command::OpenPalette);
        handle_key(&mut app, press(KeyCode::Tab));

        let Mode::Palette(palette) = &app.mode else {
            panic!("the palette should be open");
        };
        assert_eq!(palette.selected(), 1);
    }

    #[test]
    fn the_wheel_scrolls_whatever_it_is_pointing_at() {
        let mut app = app();
        app.open_page(crate::app::Page::Code);
        app.output = (1..=80).map(|n| format!("line-{n}")).collect();
        crate::ui::render::sync_scroll(&mut app, 160, 48);

        let (column, row) = cell_in(&app, Panel::Output, 160, 48);
        let before = app.scroll.offset(Panel::Output);

        handle_mouse(&mut app, at(MouseEventKind::ScrollUp, column, row), 160, 48);
        assert!(
            app.scroll.offset(Panel::Output) < before,
            "the wheel must move the panel under the pointer"
        );
        assert_eq!(app.focus, Panel::Editor, "without stealing focus");
    }

    #[test]
    fn clicking_a_panel_focuses_it() {
        let mut app = app();
        app.open_page(crate::app::Page::Code);
        let (column, row) = cell_in(&app, Panel::Output, 160, 48);

        handle_mouse(
            &mut app,
            at(MouseEventKind::Down(MouseButton::Left), column, row),
            160,
            48,
        );
        assert_eq!(app.focus, Panel::Output);
    }

    #[test]
    fn clicking_the_page_bar_opens_that_page() {
        let mut app = app();
        handle_mouse(
            &mut app,
            at(MouseEventKind::Down(MouseButton::Left), 10, 0),
            160,
            48,
        );
        assert_eq!(app.page, crate::app::Page::Debug);
    }

    #[test]
    fn the_mouse_is_ignored_while_an_overlay_is_open() {
        let mut app = app();
        app.apply(&Command::OpenPalette);
        let before = app.focus;

        handle_mouse(
            &mut app,
            at(MouseEventKind::Down(MouseButton::Left), 40, 20),
            160,
            48,
        );
        assert_eq!(app.focus, before, "the palette owns the screen");
    }

    #[test]
    fn arrow_keys_scroll_a_panel_that_has_no_other_use_for_them() {
        let mut app = app();
        app.open_page(crate::app::Page::Code);
        app.output = (1..=80).map(|n| format!("line-{n}")).collect();
        app.focus_panel(Panel::Output);
        crate::ui::render::sync_scroll(&mut app, 160, 48);

        let before = app.scroll.offset(Panel::Output);
        handle_key(&mut app, press(KeyCode::Up));
        assert!(app.scroll.offset(Panel::Output) < before);

        handle_key(&mut app, press(KeyCode::Home));
        assert_eq!(app.scroll.offset(Panel::Output), 0);
    }

    fn type_text(app: &mut App, text: &str) {
        for ch in text.chars() {
            handle_key(app, press(KeyCode::Char(ch)));
        }
    }

    #[test]
    fn typing_inserts_text_into_the_editor() {
        let mut app = app();
        type_text(&mut app, "mov rax, 1");
        assert_eq!(app.workspace.active().buffer().to_text(), "mov rax, 1");
    }

    #[test]
    fn a_printable_character_does_not_run_a_bound_command() {
        let mut app = app();
        type_text(&mut app, "s");
        assert_eq!(app.workspace.active().buffer().to_text(), "s");
    }

    #[test]
    fn a_control_chord_runs_its_command_even_in_the_editor() {
        let mut app = app();
        let effect = handle_key(&mut app, ctrl('p'));
        assert_eq!(effect, Effect::None);
        assert!(app.mode.palette().is_some(), "ctrl+p opens the palette");
    }

    #[test]
    fn a_function_key_runs_its_command() {
        let mut app = app();
        assert_eq!(
            handle_key(&mut app, press(KeyCode::F(6))),
            Effect::Build { debug: false }
        );
    }

    #[test]
    fn key_release_events_are_ignored() {
        let mut app = app();
        let mut event = press(KeyCode::Char('x'));
        event.kind = KeyEventKind::Release;

        handle_key(&mut app, event);
        assert_eq!(app.workspace.active().buffer().to_text(), "");
    }

    #[test]
    fn enter_and_backspace_edit_the_document() {
        let mut app = app();
        type_text(&mut app, "ab");
        handle_key(&mut app, press(KeyCode::Enter));
        type_text(&mut app, "cd");
        assert_eq!(app.workspace.active().buffer().to_text(), "ab\ncd");

        handle_key(&mut app, press(KeyCode::Backspace));
        assert_eq!(app.workspace.active().buffer().to_text(), "ab\nc");
    }

    #[test]
    fn shift_with_an_arrow_extends_the_selection() {
        let mut app = app();
        type_text(&mut app, "mov rax");
        app.workspace
            .active_mut()
            .move_cursor(Movement::LineStart, SelectionMode::Collapse);

        for _ in 0..3 {
            handle_key(&mut app, shift(KeyCode::Right));
        }
        assert_eq!(app.workspace.active().selected_text(), "mov");
    }

    #[test]
    fn control_with_an_arrow_moves_by_word() {
        let mut app = app();
        type_text(&mut app, "mov rax, rbx");
        app.workspace
            .active_mut()
            .move_cursor(Movement::LineStart, SelectionMode::Collapse);

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL),
        );
        assert_eq!(app.workspace.active().cursor().column, 3, "end of 'mov'");

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL),
        );
        assert_eq!(app.workspace.active().cursor().column, 7, "end of 'rax'");
    }

    #[test]
    fn escape_clears_a_selection_without_leaving_the_editor() {
        let mut app = app();
        type_text(&mut app, "mov");
        app.workspace.active_mut().select_all();
        assert!(app.workspace.active().has_selection());

        handle_key(&mut app, press(KeyCode::Esc));
        assert!(!app.workspace.active().has_selection());
    }

    #[test]
    fn an_open_overlay_swallows_every_key() {
        let mut app = app();
        handle_key(&mut app, ctrl('p'));
        type_text(&mut app, "build");

        assert_eq!(
            app.workspace.active().buffer().to_text(),
            "",
            "the editor must not have received the text"
        );
        assert_eq!(
            app.mode.prompt().map(crate::app::mode::Prompt::text),
            Some("build")
        );
    }

    #[test]
    fn escape_closes_an_overlay() {
        let mut app = app();
        handle_key(&mut app, ctrl('p'));
        handle_key(&mut app, press(KeyCode::Esc));
        assert!(!app.mode.is_overlay());
    }

    #[test]
    fn arrows_move_the_palette_selection() {
        let mut app = app();
        handle_key(&mut app, ctrl('p'));
        let first = app
            .mode
            .palette()
            .and_then(|palette| palette.selected_command().cloned());

        handle_key(&mut app, press(KeyCode::Down));
        let second = app
            .mode
            .palette()
            .and_then(|palette| palette.selected_command().cloned());

        assert_ne!(first, second, "the selection should have moved");
    }

    #[test]
    fn enter_runs_the_highlighted_palette_command() {
        let mut app = app();
        handle_key(&mut app, ctrl('p'));
        type_text(&mut app, "next panel");
        handle_key(&mut app, press(KeyCode::Enter));

        assert_eq!(app.focus, Panel::Explorer);
        assert!(!app.mode.is_overlay());
    }

    #[test]
    fn typing_narrows_the_palette_as_it_goes() {
        let mut app = app();
        handle_key(&mut app, ctrl('p'));
        let before = app.mode.palette().map(|p| p.matches().len()).unwrap_or(0);

        type_text(&mut app, "step");
        let after = app.mode.palette().map(|p| p.matches().len()).unwrap_or(0);
        assert!(after < before, "{after} should be fewer than {before}");
    }

    #[test]
    fn a_prompt_accepts_text_and_acts_on_enter() {
        let mut app = app();
        app.workspace.active_mut().insert("a\nb\nc\nd\n");

        handle_key(&mut app, ctrl('g'));
        type_text(&mut app, "3");
        handle_key(&mut app, press(KeyCode::Enter));

        assert_eq!(app.workspace.active().cursor().line, 2);
        assert!(!app.mode.is_overlay());
    }

    #[test]
    fn a_confirmation_prompt_acts_on_a_single_keypress() {
        let mut app = app();
        app.workspace.active_mut().insert_char('x');
        app.apply(&Command::Quit);
        assert!(app.mode.is_overlay());

        let effect = handle_key(&mut app, press(KeyCode::Char('y')));
        assert_eq!(effect, Effect::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn declining_a_confirmation_does_not_quit() {
        let mut app = app();
        app.workspace.active_mut().insert_char('x');
        app.apply(&Command::Quit);

        handle_key(&mut app, press(KeyCode::Char('n')));
        assert!(!app.should_quit);
        assert!(!app.mode.is_overlay());
    }

    #[test]
    fn the_syscall_panel_filters_as_you_type() {
        let mut app = app();
        app.focus_panel(Panel::Syscalls);

        type_text(&mut app, "write");
        assert_eq!(app.syscall_query, "write");
        assert_eq!(
            app.matching_syscalls()
                .first()
                .map(|call| call.name.as_str()),
            Some("write")
        );

        handle_key(&mut app, press(KeyCode::Backspace));
        assert_eq!(app.syscall_query, "writ");
    }

    #[test]
    fn the_syscall_selection_is_clamped_to_the_matches() {
        let mut app = app();
        app.focus_panel(Panel::Syscalls);
        type_text(&mut app, "write");

        let count = app.matching_syscalls().len();
        for _ in 0..count + 20 {
            handle_key(&mut app, press(KeyCode::Down));
        }
        assert!(app.syscall_selected < count, "selection escaped the list");

        for _ in 0..count + 20 {
            handle_key(&mut app, press(KeyCode::Up));
        }
        assert_eq!(app.syscall_selected, 0);
    }

    #[test]
    fn escape_clears_the_syscall_query() {
        let mut app = app();
        app.focus_panel(Panel::Syscalls);
        type_text(&mut app, "write");

        handle_key(&mut app, press(KeyCode::Esc));
        assert!(app.syscall_query.is_empty());
    }

    #[test]
    fn the_breakpoint_list_can_be_navigated_and_edited() {
        let mut app = app();
        app.workspace.active_mut().set_path("/tmp/main.asm");
        app.workspace.active_mut().insert("nop\nnop\nnop\n");

        for line in [1, 2, 3] {
            app.workspace.active_mut().go_to_line(line);
            app.apply(&Command::ToggleBreakpoint);
        }
        assert_eq!(app.breakpoints.len(), 3);

        app.focus_panel(Panel::Breakpoints);
        handle_key(&mut app, press(KeyCode::Down));
        assert_eq!(app.breakpoint_selected, 1);

        assert_eq!(
            handle_key(&mut app, press(KeyCode::Char(' '))),
            Effect::SyncBreakpoints
        );
        assert!(!app.breakpoints.all()[1].enabled);

        assert_eq!(
            handle_key(&mut app, press(KeyCode::Delete)),
            Effect::SyncBreakpoints
        );
        assert_eq!(app.breakpoints.len(), 2);
    }

    #[test]
    fn keys_in_a_panel_with_no_handler_do_nothing_rather_than_crashing() {
        let mut app = app();
        for panel in Panel::ALL {
            app.focus = panel;
            for code in [
                KeyCode::Char('x'),
                KeyCode::Up,
                KeyCode::Down,
                KeyCode::Enter,
                KeyCode::Delete,
                KeyCode::Esc,
                KeyCode::PageUp,
            ] {
                let _ = handle_key(&mut app, press(code));
            }
        }
    }

    #[test]
    fn tab_moves_between_panels_outside_the_editor() {
        let mut app = app();
        app.focus_panel(Panel::Registers);
        handle_key(&mut app, press(KeyCode::Tab));
        assert_eq!(app.focus, Panel::Flags);

        handle_key(&mut app, press(KeyCode::BackTab));
        assert_eq!(app.focus, Panel::Registers);
    }

    #[test]
    fn alt_and_a_digit_leaves_the_editor_without_using_tab() {
        let mut app = app();
        app.focus_panel(Panel::Editor);
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('3'), KeyModifiers::ALT),
        );
        assert_eq!(app.page, crate::app::Page::Learn);
        assert_eq!(app.focus, Panel::Learn);
        assert_eq!(
            app.workspace.active().buffer().to_text(),
            "",
            "the digit must not have been typed"
        );
    }

    #[test]
    fn tab_indents_in_the_editor_rather_than_changing_panel() {
        let mut app = app();
        app.focus_panel(Panel::Editor);
        type_text(&mut app, "ret");
        app.workspace
            .active_mut()
            .move_cursor(Movement::LineStart, SelectionMode::Collapse);

        handle_key(&mut app, press(KeyCode::Tab));
        assert_eq!(app.focus, Panel::Editor, "focus must not have moved");
        assert!(app
            .workspace
            .active()
            .buffer()
            .to_text()
            .starts_with("    "));
    }

    #[test]
    fn typing_in_the_scratchpad_builds_a_snippet_and_enter_runs_it() {
        let mut app = app();
        app.focus_panel(Panel::Scratchpad);
        type_text(&mut app, "add rax, rbx");
        assert_eq!(app.scratchpad.snippet, "add rax, rbx");

        handle_key(&mut app, press(KeyCode::Backspace));
        assert_eq!(app.scratchpad.snippet, "add rax, rb");

        type_text(&mut app, "x");
        assert_eq!(
            handle_key(&mut app, press(KeyCode::Enter)),
            Effect::RunScratchpad
        );
    }

    #[test]
    fn an_assignment_sets_a_starting_value_instead_of_running() {
        let mut app = app();
        app.focus_panel(Panel::Scratchpad);
        type_text(&mut app, "rax=0x10");

        assert_eq!(handle_key(&mut app, press(KeyCode::Enter)), Effect::None);
        assert_eq!(app.scratchpad.initial.get("rax"), Some(&0x10));
        assert!(
            app.scratchpad.snippet.is_empty(),
            "the line is consumed so the next one can be typed"
        );
    }

    #[test]
    fn an_assignment_with_no_value_removes_the_starting_value() {
        let mut app = app();
        app.focus_panel(Panel::Scratchpad);
        app.scratchpad.set("rbx", 7).expect("rbx is a register");

        type_text(&mut app, "rbx=");
        handle_key(&mut app, press(KeyCode::Enter));
        assert!(app.scratchpad.initial.is_empty());
    }

    #[test]
    fn an_unknown_register_is_reported_rather_than_stored() {
        let mut app = app();
        app.focus_panel(Panel::Scratchpad);
        type_text(&mut app, "rzz=1");
        handle_key(&mut app, press(KeyCode::Enter));

        assert!(app.scratchpad.initial.is_empty());
        assert_eq!(app.status.severity, Severity::Error);
    }

    #[test]
    fn an_unparsable_value_is_reported_rather_than_stored() {
        let mut app = app();
        app.focus_panel(Panel::Scratchpad);
        type_text(&mut app, "rax=nonsense");
        handle_key(&mut app, press(KeyCode::Enter));

        assert!(app.scratchpad.initial.is_empty());
        assert_eq!(app.status.severity, Severity::Error);
    }

    #[test]
    fn escape_clears_the_scratchpad_line_and_its_result() {
        let mut app = app();
        app.focus_panel(Panel::Scratchpad);
        type_text(&mut app, "mov rax, 1");
        app.scratchpad_result = Some(Err("boom".to_owned()));

        handle_key(&mut app, press(KeyCode::Esc));
        assert!(app.scratchpad.snippet.is_empty());
        assert!(app.scratchpad_result.is_none());
    }

    #[test]
    fn tab_still_leaves_the_scratchpad() {
        let mut app = app();
        app.focus_panel(Panel::Scratchpad);
        handle_key(&mut app, press(KeyCode::Tab));
        assert_ne!(app.focus, Panel::Scratchpad);
        assert!(
            app.scratchpad.snippet.is_empty(),
            "tab must not be typed into the snippet"
        );
    }

    #[test]
    fn arrows_move_through_the_learning_material() {
        let mut app = app();
        app.focus_panel(Panel::Learn);
        assert_eq!(app.learning.position(), 1);

        handle_key(&mut app, press(KeyCode::Right));
        assert_eq!(app.learning.position(), 2);

        handle_key(&mut app, press(KeyCode::Left));
        assert_eq!(app.learning.position(), 1);

        handle_key(&mut app, press(KeyCode::Left));
        assert_eq!(app.learning.position(), crate::learning::item_count());
    }

    #[test]
    fn a_question_mark_jumps_to_the_questions() {
        let mut app = app();
        app.focus_panel(Panel::Learn);
        handle_key(&mut app, press(KeyCode::Char('?')));

        assert!(app.learning.is_question());
        assert!(app.learning.current_question().is_some());
    }

    #[test]
    fn a_correct_answer_is_typed_and_accepted() {
        let mut app = app();
        app.focus_panel(Panel::Learn);
        handle_key(&mut app, press(KeyCode::Char('?')));

        let question = app.learning.current_question().expect("on a question");
        let answer = format!("{:#x}", question.answer);
        type_text(&mut app, &answer);
        assert_eq!(app.learning.typed, answer);

        handle_key(&mut app, press(KeyCode::Enter));
        assert_eq!(app.learning.verdict, crate::learning::Verdict::Correct);
        assert_eq!(app.learning.solved_count(), 1);
    }

    #[test]
    fn a_wrong_answer_keeps_the_question_open() {
        let mut app = app();
        app.focus_panel(Panel::Learn);
        handle_key(&mut app, press(KeyCode::Char('?')));
        let position = app.learning.position();

        type_text(&mut app, "123456");
        handle_key(&mut app, press(KeyCode::Backspace));
        handle_key(&mut app, press(KeyCode::Enter));

        assert_eq!(
            app.learning.verdict,
            crate::learning::Verdict::Wrong {
                given: "12345".to_owned()
            }
        );
        assert_eq!(
            app.learning.position(),
            position,
            "a wrong answer must not skip past the explanation"
        );
        assert_eq!(app.learning.solved_count(), 0);
    }

    #[test]
    fn escape_clears_a_typed_answer() {
        let mut app = app();
        app.focus_panel(Panel::Learn);
        handle_key(&mut app, press(KeyCode::Char('?')));
        type_text(&mut app, "42");

        handle_key(&mut app, press(KeyCode::Esc));
        assert!(app.learning.typed.is_empty());
        assert_eq!(app.learning.verdict, crate::learning::Verdict::Unanswered);
    }

    #[test]
    fn tab_still_leaves_the_learning_panel() {
        let mut app = app();
        app.focus_panel(Panel::Learn);
        handle_key(&mut app, press(KeyCode::Tab));
        assert_ne!(app.focus, Panel::Learn);
    }
}

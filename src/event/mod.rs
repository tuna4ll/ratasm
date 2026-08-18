//! Turning terminal input into state changes.
//!
//! Key handling is a set of plain functions over `&mut App`. Nothing here
//! touches a terminal, so every routing decision — does this key insert text,
//! run a command, or move a selection? — is testable by constructing a
//! [`KeyEvent`] and calling a function.
//!
//! # The order keys are considered
//!
//! 1. **An open overlay takes everything.** While the palette or a prompt is
//!    up, keys go there; otherwise `p` in the palette would both type a letter
//!    and run whatever `p` is bound to.
//! 2. **Bound chords run their command**, unless the editor has focus and the
//!    key is one the editor must own: a bare printable character, or Tab.
//!    Typing `s` into source must insert an `s`, and Tab must indent — so in
//!    the editor those beat the binding. Alt plus a digit focuses a panel
//!    directly, which is how you leave the editor without reaching for Tab.
//! 3. **The focused panel handles the rest**: arrows, page keys, and in the
//!    editor, text.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::mode::Mode;
use crate::app::panel::Panel;
use crate::app::state::Effect;
use crate::app::App;
#[cfg(test)]
use crate::command::Command;
use crate::editor::{Movement, SelectionMode};

/// How many lines a page key moves.
const PAGE_LINES: usize = 20;

/// Handles one key event, returning any I/O it implies.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Effect {
    // Windows terminals report both press and release; acting on both would
    // run every command twice.
    if key.kind == KeyEventKind::Release {
        return Effect::None;
    }

    if app.mode.is_overlay() {
        return handle_overlay(app, key);
    }

    // Keys the editor must own, even though they are bound to commands:
    // printable characters insert text, and Tab indents. Without this, typing
    // `s` would save the file and Tab would jump to another panel.
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
        _ => Effect::None,
    }
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
                    // A confirmation takes a single keypress rather than
                    // needing Enter as well.
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
        // 's' must insert an 's', not save the file, even though ctrl+s saves.
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
        // Some terminals report press and release; acting on both would run
        // every command twice.
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
        // Word-right lands at the end of the word under the cursor, the
        // convention readline uses; from there it steps word by word.
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
        // Otherwise typing into the palette would also act on the editor.
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

        assert_eq!(app.focus, Panel::Registers);
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
        // Requiring Enter after "y" for a yes/no question is needless.
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
        app.focus = Panel::Syscalls;

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
        app.focus = Panel::Syscalls;
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
        app.focus = Panel::Syscalls;
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

        app.focus = Panel::Breakpoints;
        handle_key(&mut app, press(KeyCode::Down));
        assert_eq!(app.breakpoint_selected, 1);

        // Space toggles the highlighted breakpoint.
        assert_eq!(
            handle_key(&mut app, press(KeyCode::Char(' '))),
            Effect::SyncBreakpoints
        );
        assert!(!app.breakpoints.all()[1].enabled);

        // Delete removes it.
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
        app.focus = Panel::Registers;
        handle_key(&mut app, press(KeyCode::Tab));
        assert_eq!(app.focus, Panel::Flags);

        handle_key(&mut app, press(KeyCode::BackTab));
        assert_eq!(app.focus, Panel::Registers);
    }

    #[test]
    fn alt_and_a_digit_leaves_the_editor_without_using_tab() {
        // Since the editor keeps Tab for indentation, there has to be another
        // way out that works from inside it.
        let mut app = app();
        app.focus = Panel::Editor;
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::ALT),
        );
        assert_eq!(app.focus, Panel::Registers);
        assert_eq!(
            app.workspace.active().buffer().to_text(),
            "",
            "the digit must not have been typed"
        );
    }

    #[test]
    fn tab_indents_in_the_editor_rather_than_changing_panel() {
        // The editor needs Tab for indentation, so it wins there.
        let mut app = app();
        app.focus = Panel::Editor;
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
}

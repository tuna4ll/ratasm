//! Terminal lifecycle management with guaranteed restoration.

use std::io::{self, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

/// Tracks whether the terminal is currently in raw mode.
static RAW_MODE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Convenience alias for the concrete terminal type used throughout the UI.
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// An RAII guard owning the terminal's raw/alternate-screen state.
pub struct TerminalGuard {
    terminal: Tui,
}

impl TerminalGuard {
    /// Enters raw mode and the alternate screen, returning a ready terminal.
    pub fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        RAW_MODE_ACTIVE.store(true, Ordering::SeqCst);

        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            cursor::Hide
        )?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        Ok(Self { terminal })
    }

    /// Returns a mutable reference to the underlying ratatui terminal.
    pub fn terminal_mut(&mut self) -> &mut Tui {
        &mut self.terminal
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = restore();
    }
}

/// Returns the terminal to cooked mode and the main screen.
pub fn restore() -> io::Result<()> {
    if !RAW_MODE_ACTIVE.swap(false, Ordering::SeqCst) {
        return Ok(());
    }
    let mut stdout = io::stdout();
    execute!(
        stdout,
        LeaveAlternateScreen,
        DisableMouseCapture,
        cursor::Show
    )?;
    disable_raw_mode()?;
    stdout.flush()?;
    Ok(())
}

/// Reports whether the terminal is currently held in raw mode.
pub fn is_raw_mode_active() -> bool {
    RAW_MODE_ACTIVE.load(Ordering::SeqCst)
}

/// Installs a panic hook that restores the terminal before reporting a panic.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore();
        previous(info);
    }));
}

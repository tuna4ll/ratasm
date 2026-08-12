//! Terminal lifecycle management with guaranteed restoration.
//!
//! The terminal is put into raw mode and switched to the alternate screen for
//! the duration of the application. Restoring it is safety critical: if the
//! process exits while the terminal is still in raw mode the user is left with
//! an unusable shell. Three mechanisms cooperate to make that impossible:
//!
//! 1. [`TerminalGuard`] restores on [`Drop`], covering normal exit and unwind.
//! 2. [`install_panic_hook`] restores before the default panic message is
//!    printed, so the backtrace is readable.
//! 3. [`restore`] is idempotent, so running it more than once is harmless.

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
///
/// Guards against double restoration, which would otherwise emit stray escape
/// sequences when both [`Drop`] and the panic hook run.
static RAW_MODE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Convenience alias for the concrete terminal type used throughout the UI.
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// An RAII guard owning the terminal's raw/alternate-screen state.
///
/// Construct exactly one of these for the lifetime of the application. Dropping
/// it returns the terminal to its original state.
pub struct TerminalGuard {
    terminal: Tui,
}

impl TerminalGuard {
    /// Enters raw mode and the alternate screen, returning a ready terminal.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal cannot be reconfigured, for example
    /// when stdout is not a TTY.
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
        // Errors during teardown are deliberately ignored: there is nothing
        // useful to do with them and propagating from `drop` is not possible.
        let _ = restore();
    }
}

/// Returns the terminal to cooked mode and the main screen.
///
/// Idempotent: calling it when the terminal was never configured, or calling it
/// repeatedly, is a no-op that returns `Ok(())`.
///
/// # Errors
///
/// Returns an error if the escape sequences cannot be written to stdout.
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
///
/// Without this the panic message would be printed into the alternate screen
/// with raw mode still active, making it unreadable and leaving the shell
/// broken. The previously installed hook is preserved and invoked afterwards.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore();
        previous(info);
    }));
}

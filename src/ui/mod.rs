//! Terminal user interface: lifecycle, layout, widgets and theming.

pub mod terminal;

pub use terminal::{install_panic_hook, restore, TerminalGuard, Tui};

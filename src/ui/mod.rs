//! Terminal user interface: lifecycle, layout, widgets and theming.

pub mod terminal;
pub mod theme;

pub use terminal::{install_panic_hook, restore, TerminalGuard, Tui};
pub use theme::{Theme, ThemeKind};

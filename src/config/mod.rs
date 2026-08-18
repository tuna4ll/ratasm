//! User configuration: settings and key bindings.

pub mod keymap;

pub use keymap::{parse_binding, KeyBinding, Keymap, KeymapError};

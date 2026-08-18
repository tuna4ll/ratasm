//! User configuration: settings and key bindings.
//!
//! These are the user's preferences, distinct from the project settings in
//! `.ratasm.toml`. A repository can change how a project is built; it cannot
//! change your theme or your key bindings.

pub mod keymap;
pub mod settings;

pub use keymap::{parse_binding, KeyBinding, Keymap, KeymapError};
pub use settings::{Settings, SettingsError};

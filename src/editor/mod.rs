//! The text editor: buffers, cursor movement, history and language support.
//!
//! The editor is deliberately independent of ratatui. Nothing in this module
//! draws, and nothing here knows the size of the terminal. Rendering code
//! reads editor state and produces widgets; it never mutates state. That split
//! is what makes the editor testable without a terminal attached.

pub mod buffer;
pub mod history;
pub mod position;

pub use buffer::{TextBuffer, TAB_WIDTH};
pub use history::{Edit, History};
pub use position::{Position, Range};

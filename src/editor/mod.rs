//! The text editor: buffers, cursor movement, history and language support.
//!
//! The editor is deliberately independent of ratatui. Nothing in this module
//! draws, and nothing here knows the size of the terminal. Rendering code
//! reads editor state and produces widgets; it never mutates state. That split
//! is what makes the editor testable without a terminal attached.

pub mod brackets;
pub mod buffer;
pub mod completion;
pub mod document;
pub mod history;
pub mod position;
pub mod search;
pub mod symbols;
pub mod syntax;

pub use buffer::{TextBuffer, TAB_WIDTH};
pub use completion::{candidates, Candidate};
pub use document::{Document, Movement, SelectionMode};
pub use history::{Edit, History};
pub use position::{Position, Range};
pub use search::{find_all, SearchOptions};
pub use symbols::{Symbol, SymbolKind};
pub use syntax::{tokenize, Token, TokenKind};

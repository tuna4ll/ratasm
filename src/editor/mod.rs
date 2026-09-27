//! The source view: buffers, cursor movement and language support.
//!
//! Editing happens in the user's own `$EDITOR`; ratasm reads the files back
//! when it returns.
//!
//! The module is deliberately independent of ratatui. Nothing in this module
//! draws, and nothing here knows the size of the terminal. Rendering code
//! reads editor state and produces widgets; it never mutates state. That split
//! is what makes the editor testable without a terminal attached.

pub mod buffer;
pub mod document;
pub mod position;
pub mod symbols;
pub mod syntax;
pub mod workspace;

pub use buffer::{TextBuffer, TAB_WIDTH};
pub use document::Document;
pub use position::{Position, Range};
pub use symbols::{Symbol, SymbolKind};
pub use syntax::{tokenize, Token, TokenKind};
pub use workspace::{FileError, Workspace};

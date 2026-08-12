//! `ratasm` is a terminal IDE and debugger for x86_64 assembly.
//!
//! The crate is organised so that state, rendering, process management and the
//! debugger protocol stay independent of one another:
//!
//! - [`ui`] owns terminal lifecycle, layout and widgets and never mutates
//!   application state.
//! - [`logging`] redirects diagnostics to a file so the TUI keeps stdout.
//!
//! Every subsystem is usable without a terminal attached, which is what makes
//! the parsers, the build pipeline and the debugger testable in isolation.

#![warn(missing_docs)]

pub mod logging;
pub mod ui;

//! `ratasm` is a terminal IDE and debugger for x86_64 assembly.
//!
//! The crate is organised so that state, rendering, process management and the
//! debugger protocol stay independent of one another:
//!
//! - [`ui`] owns terminal lifecycle, layout and widgets and never mutates
//!   application state.
//! - [`assembler`] drives the toolchain and parses what it says.
//! - [`debugger`] speaks GDB/MI and never touches the terminal.
//! - [`editor`] is pure text state with no rendering dependency.
//! - [`instruction`] holds static ISA knowledge shared by every panel.
//! - [`syscall`] embeds the Linux system call table taken from the kernel headers.
//! - [`process`] runs external tools without a shell and without zombies.
//! - [`logging`] redirects diagnostics to a file so the TUI keeps stdout.
//!
//! Every subsystem is usable without a terminal attached, which is what makes
//! the parsers, the build pipeline and the debugger testable in isolation.

#![warn(missing_docs)]

pub mod assembler;
pub mod debugger;
pub mod editor;
pub mod instruction;
pub mod logging;
pub mod process;
pub mod project;
pub mod syscall;
pub mod ui;

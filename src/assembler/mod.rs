//! Assembling and linking: running the toolchain and interpreting its output.
//!
//! [`build`] drives the pipeline; [`diagnostics`] turns what the tools print
//! into structured messages the editor can navigate to.

pub mod build;
pub mod diagnostics;

pub use build::{build, run_executable, BuildError, BuildOptions, BuildOutcome, BuildStep};
pub use diagnostics::{Diagnostic, Producer, Severity};

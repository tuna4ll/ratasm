//! Application state and the loop that drives it.
//!
//! [`state::App`] holds everything the interface shows and applies commands to
//! it synchronously, returning an [`state::Effect`] for any I/O. Keeping the
//! two apart is what lets every command be tested without a terminal, a
//! toolchain or an async runtime.

pub mod mode;
pub mod panel;
pub mod run;
pub mod state;

pub use mode::{Mode, Prompt, PromptKind};
pub use panel::Panel;
pub use run::run;
pub use state::{App, Effect, Severity, Status, StepKind};

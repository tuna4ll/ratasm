//! Application state and the loop that drives it.

pub mod mode;
pub mod page;
pub mod panel;
pub mod run;
pub mod scroll;
pub mod state;

pub use mode::{Mode, Prompt, PromptKind};
pub use page::Page;
pub use panel::Panel;
pub use run::run;
pub use scroll::ScrollState;
pub use state::{App, Effect, Severity, Status, StepKind};

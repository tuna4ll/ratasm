//! Debugging: the GDB/MI transport, the session state machine and the models
//! behind the register, memory and breakpoint panels.

pub mod mi;
pub mod state;

pub use state::{DebuggerState, InvalidTransition, StateMachine, Transition};

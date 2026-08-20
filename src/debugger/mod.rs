//! Debugging: the GDB/MI transport, the session state machine and the models
//! behind the register, memory and breakpoint panels.

pub mod breakpoints;
pub mod frames;
pub mod memory;
pub mod mi;
pub mod registers;
pub mod session;
pub mod state;

pub use breakpoints::{Breakpoint, BreakpointSet, Location};
pub use frames::{parse_frames, Frame};
pub use memory::{evaluate_address, AddressError, MemoryBlock};
pub use registers::{Format, RegisterEntry, RegisterFile};
pub use session::{GdbSession, SessionError};
pub use state::{DebuggerState, InvalidTransition, StateMachine, Transition};

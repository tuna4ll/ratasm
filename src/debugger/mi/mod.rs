//! The GDB machine interface protocol.
//!
//! [`value`] parses the MI value grammar; [`record`] turns a line of GDB
//! output into a typed [`Record`]. Neither knows anything about processes, so
//! both are exercised entirely from fixtures.

pub mod record;
pub mod value;

pub use record::{parse_line, AsyncKind, Record, ResultClass, StreamKind};
pub use value::{parse_address, ParseError, Parser, Value};

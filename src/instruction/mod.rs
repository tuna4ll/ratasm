//! Architectural knowledge: registers, flags, conditions and instruction
//! semantics.
//!
//! Everything here is static data about the instruction set rather than state
//! about a running program. That separation is deliberate: the register panel,
//! the flag panel, the syntax highlighter, the completion engine and the
//! instruction explainer all need the same tables, and none of them should
//! have to reach into the debugger to get them.

pub mod conditions;
pub mod flags;
pub mod mnemonics;
pub mod registers;

pub use conditions::{satisfied_conditions, ConditionCode};
pub use flags::{Flag, Flags};
pub use mnemonics::is_mnemonic;
pub use registers::{AbiRole, Register, RegisterWidth, SyscallRole};

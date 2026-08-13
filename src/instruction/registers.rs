//! The x86-64 register model.
//!
//! This module is pure metadata: it knows the *names*, widths and roles of
//! registers, but never their values. Register values come from the debugger
//! and are formatted in [`crate::debugger::registers`]. Keeping the two apart
//! means the ABI knowledge here is testable without a running program, and it
//! is what lets the editor's syntax highlighter reuse the same tables the
//! register panel uses.
//!
//! # What is modelled
//!
//! - the sixteen general-purpose registers plus `RIP` and `RFLAGS`;
//! - the sub-register aliases (`rax` / `eax` / `ax` / `al` / `ah`) and how they
//!   overlay the parent, including the high-byte registers that read bits 8-15
//!   rather than the low byte;
//! - each register's role in the System V AMD64 calling convention;
//! - each register's role in the Linux `syscall` convention, which differs
//!   from the function-call convention in one place that trips up nearly every
//!   beginner: the fourth argument travels in `R10`, not `RCX`.

use std::fmt;

/// The width of a register or register alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RegisterWidth {
    /// 8 bits.
    Byte,
    /// 16 bits.
    Word,
    /// 32 bits.
    Dword,
    /// 64 bits.
    Qword,
}

impl RegisterWidth {
    /// All widths, narrowest first.
    pub const ALL: [RegisterWidth; 4] = [
        RegisterWidth::Byte,
        RegisterWidth::Word,
        RegisterWidth::Dword,
        RegisterWidth::Qword,
    ];

    /// The width in bits.
    pub const fn bits(self) -> u32 {
        match self {
            RegisterWidth::Byte => 8,
            RegisterWidth::Word => 16,
            RegisterWidth::Dword => 32,
            RegisterWidth::Qword => 64,
        }
    }

    /// The width in bytes.
    pub const fn bytes(self) -> u32 {
        self.bits() / 8
    }

    /// A mask selecting the bits this width covers.
    pub const fn mask(self) -> u64 {
        match self {
            RegisterWidth::Byte => 0xff,
            RegisterWidth::Word => 0xffff,
            RegisterWidth::Dword => 0xffff_ffff,
            RegisterWidth::Qword => u64::MAX,
        }
    }

    /// The NASM size specifier keyword for this width.
    pub const fn nasm_keyword(self) -> &'static str {
        match self {
            RegisterWidth::Byte => "byte",
            RegisterWidth::Word => "word",
            RegisterWidth::Dword => "dword",
            RegisterWidth::Qword => "qword",
        }
    }
}

impl fmt::Display for RegisterWidth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-bit", self.bits())
    }
}

/// A register's role in the System V AMD64 calling convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiRole {
    /// Passes integer argument `n` (one-based).
    Argument(u8),
    /// Holds the integer return value.
    ReturnValue,
    /// Preserved across calls; a callee must restore it.
    CalleeSaved,
    /// Free for the callee to destroy.
    CallerSaved,
    /// Points at the top of the stack.
    StackPointer,
    /// Conventional frame pointer.
    FramePointer,
    /// Points at the next instruction.
    InstructionPointer,
    /// Holds the processor status flags.
    Flags,
}

impl AbiRole {
    /// A short human-readable description of the role.
    pub fn description(self) -> String {
        match self {
            AbiRole::Argument(n) => format!("integer argument {n}"),
            AbiRole::ReturnValue => "integer return value".to_owned(),
            AbiRole::CalleeSaved => "callee-saved (must be preserved)".to_owned(),
            AbiRole::CallerSaved => "caller-saved (freely clobbered)".to_owned(),
            AbiRole::StackPointer => "stack pointer".to_owned(),
            AbiRole::FramePointer => "frame pointer (callee-saved)".to_owned(),
            AbiRole::InstructionPointer => "instruction pointer".to_owned(),
            AbiRole::Flags => "status flags".to_owned(),
        }
    }

    /// Whether a callee is required to preserve this register.
    pub fn is_preserved(self) -> bool {
        matches!(
            self,
            AbiRole::CalleeSaved | AbiRole::FramePointer | AbiRole::StackPointer
        )
    }
}

/// A register's role in the Linux x86-64 `syscall` convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallRole {
    /// Carries the syscall number on entry and the result on return.
    Number,
    /// Carries syscall argument `n` (one-based, 1..=6).
    Argument(u8),
    /// Destroyed by the `syscall` instruction itself.
    Clobbered,
    /// Not used by the syscall convention.
    Unused,
}

impl SyscallRole {
    /// A short human-readable description of the role.
    pub fn description(self) -> String {
        match self {
            SyscallRole::Number => "syscall number in, return value out".to_owned(),
            SyscallRole::Argument(n) => format!("syscall argument {n}"),
            SyscallRole::Clobbered => "destroyed by `syscall`".to_owned(),
            SyscallRole::Unused => "not used by the syscall ABI".to_owned(),
        }
    }
}

/// A 64-bit architectural register and everything known about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Register {
    /// Canonical 64-bit name, lowercase, for example `rax`.
    pub name: &'static str,
    /// 32-bit alias, for example `eax`.
    pub dword: Option<&'static str>,
    /// 16-bit alias, for example `ax`.
    pub word: Option<&'static str>,
    /// Low 8-bit alias, for example `al`.
    pub low_byte: Option<&'static str>,
    /// High 8-bit alias covering bits 8-15, for example `ah`.
    ///
    /// Only the four legacy registers `rax`, `rbx`, `rcx` and `rdx` have one.
    pub high_byte: Option<&'static str>,
    /// Role in the System V AMD64 calling convention.
    pub abi_role: AbiRole,
    /// Role in the Linux syscall convention.
    pub syscall_role: SyscallRole,
    /// Conventional use, phrased for someone learning the architecture.
    pub summary: &'static str,
}

impl Register {
    /// The alias of `width`, if the register has one.
    ///
    /// The high-byte alias is deliberately not returned here: `ah` is not "the
    /// 8-bit view of rax", it is a different 8-bit window into it. Use
    /// [`Register::high_byte`] explicitly.
    pub fn alias(&self, width: RegisterWidth) -> Option<&'static str> {
        match width {
            RegisterWidth::Qword => Some(self.name),
            RegisterWidth::Dword => self.dword,
            RegisterWidth::Word => self.word,
            RegisterWidth::Byte => self.low_byte,
        }
    }

    /// Every name this register answers to, widest first.
    pub fn aliases(&self) -> Vec<&'static str> {
        let mut names = vec![self.name];
        names.extend(self.dword);
        names.extend(self.word);
        names.extend(self.high_byte);
        names.extend(self.low_byte);
        names
    }

    /// Extracts the value of one alias out of the full 64-bit value.
    ///
    /// `alias` is matched case-insensitively. High-byte aliases read bits 8-15,
    /// which is the detail that makes `ah` differ from `al`.
    pub fn extract(&self, value: u64, alias: &str) -> Option<u64> {
        let alias = alias.trim().to_ascii_lowercase();
        if self.high_byte == Some(alias.as_str()) {
            return Some((value >> 8) & 0xff);
        }
        let width = self.width_of(&alias)?;
        Some(value & width.mask())
    }

    /// The width of one of this register's aliases.
    pub fn width_of(&self, alias: &str) -> Option<RegisterWidth> {
        let alias = alias.trim().to_ascii_lowercase();
        let alias = alias.as_str();
        if alias == self.name {
            Some(RegisterWidth::Qword)
        } else if self.dword == Some(alias) {
            Some(RegisterWidth::Dword)
        } else if self.word == Some(alias) {
            Some(RegisterWidth::Word)
        } else if self.low_byte == Some(alias) || self.high_byte == Some(alias) {
            Some(RegisterWidth::Byte)
        } else {
            None
        }
    }

    /// The uppercase display name used in the register panel.
    pub fn display_name(&self) -> String {
        self.name.to_ascii_uppercase()
    }
}

/// Builds a general-purpose register entry.
///
/// `aliases` lists the 32-, 16- and low-8-bit names in that order, which keeps
/// the table below readable as a grid.
const fn gpr(
    name: &'static str,
    aliases: [&'static str; 3],
    high_byte: Option<&'static str>,
    abi_role: AbiRole,
    syscall_role: SyscallRole,
    summary: &'static str,
) -> Register {
    Register {
        name,
        dword: Some(aliases[0]),
        word: Some(aliases[1]),
        low_byte: Some(aliases[2]),
        high_byte,
        abi_role,
        syscall_role,
        summary,
    }
}

/// The sixteen general-purpose registers, in architectural encoding order.
pub const GENERAL_PURPOSE: [Register; 16] = [
    gpr(
        "rax",
        ["eax", "ax", "al"],
        Some("ah"),
        AbiRole::ReturnValue,
        SyscallRole::Number,
        "Accumulator. Holds a function's return value and the syscall number.",
    ),
    gpr(
        "rbx",
        ["ebx", "bx", "bl"],
        Some("bh"),
        AbiRole::CalleeSaved,
        SyscallRole::Unused,
        "Base register. Callee-saved, so save it before use and restore it after.",
    ),
    gpr(
        "rcx",
        ["ecx", "cx", "cl"],
        Some("ch"),
        AbiRole::Argument(4),
        SyscallRole::Clobbered,
        "Counter for string and loop instructions. Fourth function argument, but \
         `syscall` destroys it, which is why syscalls use R10 instead.",
    ),
    gpr(
        "rdx",
        ["edx", "dx", "dl"],
        Some("dh"),
        AbiRole::Argument(3),
        SyscallRole::Argument(3),
        "Data register. Third argument, and the high half of a 128-bit product.",
    ),
    gpr(
        "rsi",
        ["esi", "si", "sil"],
        None,
        AbiRole::Argument(2),
        SyscallRole::Argument(2),
        "Source index for string instructions. Second argument.",
    ),
    gpr(
        "rdi",
        ["edi", "di", "dil"],
        None,
        AbiRole::Argument(1),
        SyscallRole::Argument(1),
        "Destination index for string instructions. First argument.",
    ),
    Register {
        name: "rbp",
        dword: Some("ebp"),
        word: Some("bp"),
        low_byte: Some("bpl"),
        high_byte: None,
        abi_role: AbiRole::FramePointer,
        syscall_role: SyscallRole::Unused,
        summary: "Frame pointer. Anchors the current stack frame; callee-saved.",
    },
    Register {
        name: "rsp",
        dword: Some("esp"),
        word: Some("sp"),
        low_byte: Some("spl"),
        high_byte: None,
        abi_role: AbiRole::StackPointer,
        syscall_role: SyscallRole::Unused,
        summary: "Stack pointer. Points at the most recently pushed value.",
    },
    gpr(
        "r8",
        ["r8d", "r8w", "r8b"],
        None,
        AbiRole::Argument(5),
        SyscallRole::Argument(5),
        "Fifth argument in both the function and syscall conventions.",
    ),
    gpr(
        "r9",
        ["r9d", "r9w", "r9b"],
        None,
        AbiRole::Argument(6),
        SyscallRole::Argument(6),
        "Sixth argument in both the function and syscall conventions.",
    ),
    gpr(
        "r10",
        ["r10d", "r10w", "r10b"],
        None,
        AbiRole::CallerSaved,
        SyscallRole::Argument(4),
        "Fourth syscall argument. In function calls it is a scratch register.",
    ),
    gpr(
        "r11",
        ["r11d", "r11w", "r11b"],
        None,
        AbiRole::CallerSaved,
        SyscallRole::Clobbered,
        "Scratch register. `syscall` stores RFLAGS here, destroying it.",
    ),
    gpr(
        "r12",
        ["r12d", "r12w", "r12b"],
        None,
        AbiRole::CalleeSaved,
        SyscallRole::Unused,
        "Callee-saved scratch register.",
    ),
    gpr(
        "r13",
        ["r13d", "r13w", "r13b"],
        None,
        AbiRole::CalleeSaved,
        SyscallRole::Unused,
        "Callee-saved scratch register.",
    ),
    gpr(
        "r14",
        ["r14d", "r14w", "r14b"],
        None,
        AbiRole::CalleeSaved,
        SyscallRole::Unused,
        "Callee-saved scratch register.",
    ),
    gpr(
        "r15",
        ["r15d", "r15w", "r15b"],
        None,
        AbiRole::CalleeSaved,
        SyscallRole::Unused,
        "Callee-saved scratch register.",
    ),
];

/// The instruction pointer.
pub const RIP: Register = Register {
    name: "rip",
    dword: Some("eip"),
    word: Some("ip"),
    low_byte: None,
    high_byte: None,
    abi_role: AbiRole::InstructionPointer,
    syscall_role: SyscallRole::Unused,
    summary: "Instruction pointer. Holds the address of the next instruction.",
};

/// The flags register.
pub const RFLAGS: Register = Register {
    name: "rflags",
    dword: Some("eflags"),
    word: Some("flags"),
    low_byte: None,
    high_byte: None,
    abi_role: AbiRole::Flags,
    syscall_role: SyscallRole::Unused,
    summary: "Status flags set by arithmetic and logic instructions.",
};

/// Every register the debugger displays, in panel order.
pub fn all() -> Vec<Register> {
    let mut registers = GENERAL_PURPOSE.to_vec();
    registers.push(RIP);
    registers.push(RFLAGS);
    registers
}

/// Looks up a register by any of its aliases, case-insensitively.
///
/// Accepts a leading `%` so AT&T-style names resolve too.
pub fn lookup(name: &str) -> Option<Register> {
    let name = name.trim().trim_start_matches('%').to_ascii_lowercase();
    all()
        .into_iter()
        .find(|register| register.aliases().iter().any(|alias| *alias == name))
}

/// Returns `true` when `name` is any register alias.
pub fn is_register(name: &str) -> bool {
    lookup(name).is_some()
}

/// The registers carrying syscall arguments, in argument order.
///
/// The order — `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9` — is the one detail of
/// the Linux syscall ABI most worth memorising, and the one place it diverges
/// from the function-call ABI.
pub fn syscall_argument_registers() -> Vec<Register> {
    let mut found: Vec<Register> = GENERAL_PURPOSE
        .iter()
        .filter(|register| matches!(register.syscall_role, SyscallRole::Argument(_)))
        .copied()
        .collect();
    found.sort_by_key(|register| match register.syscall_role {
        SyscallRole::Argument(n) => n,
        _ => u8::MAX,
    });
    found
}

/// The registers carrying integer function arguments, in argument order.
pub fn abi_argument_registers() -> Vec<Register> {
    let mut found: Vec<Register> = GENERAL_PURPOSE
        .iter()
        .filter(|register| matches!(register.abi_role, AbiRole::Argument(_)))
        .copied()
        .collect();
    found.sort_by_key(|register| match register.abi_role {
        AbiRole::Argument(n) => n,
        _ => u8::MAX,
    });
    found
}

/// Every register alias, for completion and syntax highlighting.
pub fn all_alias_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = all()
        .iter()
        .flat_map(|register| register.aliases())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_general_purpose_register_is_present() {
        assert_eq!(GENERAL_PURPOSE.len(), 16);
        for name in [
            "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp", "r8", "r9", "r10", "r11",
            "r12", "r13", "r14", "r15",
        ] {
            assert!(lookup(name).is_some(), "missing register {name}");
        }
        assert!(lookup("rip").is_some());
        assert!(lookup("rflags").is_some());
    }

    #[test]
    fn aliases_resolve_to_their_parent() {
        for alias in ["eax", "ax", "al", "ah", "RAX", " rax "] {
            let register = lookup(alias).unwrap_or_else(|| panic!("{alias} must resolve"));
            assert_eq!(register.name, "rax");
        }
    }

    #[test]
    fn at_and_t_style_names_resolve() {
        assert_eq!(lookup("%rsp").map(|r| r.name), Some("rsp"));
    }

    #[test]
    fn unknown_names_are_rejected() {
        for name in ["", "rax2", "zmm0", "nop", "r16"] {
            assert!(!is_register(name), "{name} must not resolve");
        }
    }

    #[test]
    fn alias_widths_are_correct() {
        let rax = lookup("rax").expect("rax");
        assert_eq!(rax.width_of("rax"), Some(RegisterWidth::Qword));
        assert_eq!(rax.width_of("eax"), Some(RegisterWidth::Dword));
        assert_eq!(rax.width_of("ax"), Some(RegisterWidth::Word));
        assert_eq!(rax.width_of("al"), Some(RegisterWidth::Byte));
        assert_eq!(rax.width_of("ah"), Some(RegisterWidth::Byte));
        assert_eq!(rax.width_of("bl"), None);
    }

    #[test]
    fn extract_narrows_a_value_to_the_alias_width() {
        let rax = lookup("rax").expect("rax");
        let value = 0x1122_3344_5566_7788u64;
        assert_eq!(rax.extract(value, "rax"), Some(0x1122_3344_5566_7788));
        assert_eq!(rax.extract(value, "eax"), Some(0x5566_7788));
        assert_eq!(rax.extract(value, "ax"), Some(0x7788));
        assert_eq!(rax.extract(value, "al"), Some(0x88));
    }

    #[test]
    fn high_byte_alias_reads_bits_eight_to_fifteen() {
        // The distinction that makes AH more than a second name for AL.
        let rax = lookup("rax").expect("rax");
        let value = 0x1122_3344_5566_7788u64;
        assert_eq!(rax.extract(value, "ah"), Some(0x77));
        assert_eq!(rax.extract(value, "al"), Some(0x88));
        assert_ne!(rax.extract(value, "ah"), rax.extract(value, "al"));
    }

    #[test]
    fn only_the_legacy_four_have_high_byte_aliases() {
        for name in ["rax", "rbx", "rcx", "rdx"] {
            assert!(
                lookup(name).and_then(|r| r.high_byte).is_some(),
                "{name} should have a high-byte alias"
            );
        }
        for name in ["rsi", "rdi", "rbp", "rsp", "r8", "r15"] {
            assert!(
                lookup(name).and_then(|r| r.high_byte).is_none(),
                "{name} must not have a high-byte alias"
            );
        }
    }

    #[test]
    fn extract_rejects_an_alias_from_another_register() {
        let rax = lookup("rax").expect("rax");
        assert_eq!(rax.extract(0xff, "bl"), None);
    }

    #[test]
    fn syscall_arguments_follow_the_linux_order() {
        let names: Vec<&str> = syscall_argument_registers()
            .iter()
            .map(|register| register.name)
            .collect();
        assert_eq!(names, ["rdi", "rsi", "rdx", "r10", "r8", "r9"]);
    }

    #[test]
    fn function_arguments_follow_the_system_v_order() {
        let names: Vec<&str> = abi_argument_registers()
            .iter()
            .map(|register| register.name)
            .collect();
        assert_eq!(names, ["rdi", "rsi", "rdx", "rcx", "r8", "r9"]);
    }

    #[test]
    fn the_syscall_and_function_conventions_differ_only_at_argument_four() {
        // The classic beginner trap, pinned down by a test.
        let syscall: Vec<&str> = syscall_argument_registers()
            .iter()
            .map(|r| r.name)
            .collect();
        let function: Vec<&str> = abi_argument_registers().iter().map(|r| r.name).collect();
        let differing: Vec<usize> = (0..6).filter(|i| syscall[*i] != function[*i]).collect();
        assert_eq!(differing, vec![3]);
        assert_eq!(syscall[3], "r10");
        assert_eq!(function[3], "rcx");
    }

    #[test]
    fn syscall_clobbers_rcx_and_r11() {
        for name in ["rcx", "r11"] {
            let register = lookup(name).expect(name);
            assert_eq!(register.syscall_role, SyscallRole::Clobbered);
        }
    }

    #[test]
    fn callee_saved_registers_are_marked_preserved() {
        for name in ["rbx", "rbp", "rsp", "r12", "r13", "r14", "r15"] {
            let register = lookup(name).expect(name);
            assert!(
                register.abi_role.is_preserved(),
                "{name} must be reported as preserved"
            );
        }
        for name in ["rax", "rcx", "rdi", "r11"] {
            let register = lookup(name).expect(name);
            assert!(!register.abi_role.is_preserved(), "{name} is not preserved");
        }
    }

    #[test]
    fn width_masks_cover_the_right_number_of_bits() {
        assert_eq!(RegisterWidth::Byte.mask(), 0xff);
        assert_eq!(RegisterWidth::Word.mask(), 0xffff);
        assert_eq!(RegisterWidth::Dword.mask(), 0xffff_ffff);
        assert_eq!(RegisterWidth::Qword.mask(), u64::MAX);
        for width in RegisterWidth::ALL {
            assert_eq!(width.bytes() * 8, width.bits());
        }
    }

    #[test]
    fn alias_names_are_unique() {
        let names = all_alias_names();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names.len(), sorted.len(), "alias names must not repeat");
        assert!(names.contains(&"r15b"));
        assert!(names.contains(&"eflags"));
    }

    #[test]
    fn alias_lookup_by_width_skips_the_high_byte() {
        let rax = lookup("rax").expect("rax");
        assert_eq!(rax.alias(RegisterWidth::Byte), Some("al"));
        assert_eq!(rax.alias(RegisterWidth::Qword), Some("rax"));
    }
}

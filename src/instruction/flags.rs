//! The `RFLAGS` register: individual flags, their meanings and a bit set.
//!
//! Two types live here. [`Flag`] is one named flag with the metadata the flag
//! panel displays — its bit position, what sets it and what it means. [`Flags`]
//! is a bit set over the whole register, used to evaluate condition codes.
//!
//! Only the nine flags a user-mode assembly programmer reasons about are
//! modelled. `IOPL`, `NT`, `RF`, `VM` and friends are deliberately absent:
//! showing bits that cannot be acted on from user code would be noise, and
//! inventing explanations for them would be worse.

use std::fmt;
use std::ops::{BitAnd, BitOr, BitXor, Not, Sub};

/// A single named bit in the flags register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flag {
    /// Carry: unsigned overflow out of the most significant bit.
    Carry,
    /// Parity: the low byte of the result has an even number of set bits.
    Parity,
    /// Auxiliary carry: carry out of bit 3, used by BCD arithmetic.
    Auxiliary,
    /// Zero: the result was zero.
    Zero,
    /// Sign: the result's most significant bit was set.
    Sign,
    /// Trap: the CPU single-steps while this is set.
    Trap,
    /// Interrupt enable: maskable interrupts are accepted.
    Interrupt,
    /// Direction: string instructions step backwards while set.
    Direction,
    /// Overflow: signed overflow occurred.
    Overflow,
}

impl Flag {
    /// Every modelled flag, in bit order.
    pub const ALL: [Flag; 9] = [
        Flag::Carry,
        Flag::Parity,
        Flag::Auxiliary,
        Flag::Zero,
        Flag::Sign,
        Flag::Trap,
        Flag::Interrupt,
        Flag::Direction,
        Flag::Overflow,
    ];

    /// The flags that arithmetic and logic instructions typically set.
    ///
    /// These are the ones worth highlighting after a step; the system flags
    /// change rarely and for different reasons.
    pub const ARITHMETIC: [Flag; 6] = [
        Flag::Carry,
        Flag::Parity,
        Flag::Auxiliary,
        Flag::Zero,
        Flag::Sign,
        Flag::Overflow,
    ];

    /// The two-letter abbreviation, for example `"ZF"`.
    pub const fn abbreviation(self) -> &'static str {
        match self {
            Flag::Carry => "CF",
            Flag::Parity => "PF",
            Flag::Auxiliary => "AF",
            Flag::Zero => "ZF",
            Flag::Sign => "SF",
            Flag::Trap => "TF",
            Flag::Interrupt => "IF",
            Flag::Direction => "DF",
            Flag::Overflow => "OF",
        }
    }

    /// The full name of the flag.
    pub const fn name(self) -> &'static str {
        match self {
            Flag::Carry => "Carry",
            Flag::Parity => "Parity",
            Flag::Auxiliary => "Auxiliary carry",
            Flag::Zero => "Zero",
            Flag::Sign => "Sign",
            Flag::Trap => "Trap",
            Flag::Interrupt => "Interrupt enable",
            Flag::Direction => "Direction",
            Flag::Overflow => "Overflow",
        }
    }

    /// The bit position of the flag within `RFLAGS`.
    pub const fn bit(self) -> u32 {
        match self {
            Flag::Carry => 0,
            Flag::Parity => 2,
            Flag::Auxiliary => 4,
            Flag::Zero => 6,
            Flag::Sign => 7,
            Flag::Trap => 8,
            Flag::Interrupt => 9,
            Flag::Direction => 10,
            Flag::Overflow => 11,
        }
    }

    /// The flag as a single-bit mask.
    pub const fn mask(self) -> Flags {
        Flags(1u64 << self.bit())
    }

    /// What the flag means, phrased for someone learning the architecture.
    pub const fn description(self) -> &'static str {
        match self {
            Flag::Carry => {
                "Set when an unsigned operation carried out of, or borrowed \
                 into, the most significant bit. This is the unsigned overflow \
                 indicator."
            }
            Flag::Parity => {
                "Set when the low byte of the result contains an even number of \
                 set bits. A legacy of serial communication; rarely used today."
            }
            Flag::Auxiliary => {
                "Set on a carry out of bit 3. Only binary-coded decimal \
                 instructions consult it."
            }
            Flag::Zero => "Set when the result was exactly zero. Equality tests read this flag.",
            Flag::Sign => {
                "Set when the result's most significant bit is 1, meaning the \
                 result is negative when read as a signed value."
            }
            Flag::Trap => {
                "When set the CPU raises a debug exception after every \
                 instruction. Debuggers use it to single-step."
            }
            Flag::Interrupt => {
                "When set the CPU accepts maskable hardware interrupts. User \
                 code cannot clear it."
            }
            Flag::Direction => {
                "Controls the direction of string instructions: clear counts \
                 addresses up, set counts them down. The ABI requires it to be \
                 clear on function entry."
            }
            Flag::Overflow => {
                "Set when a signed operation produced a result too large for \
                 the destination. This is the signed overflow indicator."
            }
        }
    }

    /// A short phrase describing what the flag means when it is set.
    pub const fn when_set(self) -> &'static str {
        match self {
            Flag::Carry => "unsigned overflow or borrow occurred",
            Flag::Parity => "low byte has an even number of set bits",
            Flag::Auxiliary => "carry out of bit 3",
            Flag::Zero => "result was zero",
            Flag::Sign => "result was negative",
            Flag::Trap => "single-step mode is active",
            Flag::Interrupt => "interrupts are enabled",
            Flag::Direction => "string operations count downwards",
            Flag::Overflow => "signed overflow occurred",
        }
    }

    /// Whether the flag is a status flag set by arithmetic, rather than a
    /// system flag controlling processor behaviour.
    pub const fn is_status_flag(self) -> bool {
        matches!(
            self,
            Flag::Carry | Flag::Parity | Flag::Auxiliary | Flag::Zero | Flag::Sign | Flag::Overflow
        )
    }

    /// Resolves an abbreviation such as `"zf"` to a flag.
    pub fn from_abbreviation(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_uppercase();
        Self::ALL
            .into_iter()
            .find(|flag| flag.abbreviation() == text)
    }
}

impl fmt::Display for Flag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.abbreviation())
    }
}

/// A set of flags, backed by the raw `RFLAGS` value.
///
/// Bits outside the modelled flags are preserved by the arithmetic operators
/// but ignored by [`Flags::iter`], so a real `RFLAGS` read from the debugger
/// can be stored here without losing information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Flags(u64);

impl Flags {
    /// The carry flag.
    pub const CARRY: Flags = Flag::Carry.mask();
    /// The parity flag.
    pub const PARITY: Flags = Flag::Parity.mask();
    /// The auxiliary carry flag.
    pub const AUXILIARY: Flags = Flag::Auxiliary.mask();
    /// The zero flag.
    pub const ZERO: Flags = Flag::Zero.mask();
    /// The sign flag.
    pub const SIGN: Flags = Flag::Sign.mask();
    /// The trap flag.
    pub const TRAP: Flags = Flag::Trap.mask();
    /// The interrupt enable flag.
    pub const INTERRUPT: Flags = Flag::Interrupt.mask();
    /// The direction flag.
    pub const DIRECTION: Flags = Flag::Direction.mask();
    /// The overflow flag.
    pub const OVERFLOW: Flags = Flag::Overflow.mask();

    /// An empty set.
    pub const fn empty() -> Self {
        Flags(0)
    }

    /// Builds a set from a raw `RFLAGS` value, keeping every bit.
    pub const fn from_bits(bits: u64) -> Self {
        Flags(bits)
    }

    /// Builds a set from a raw value, discarding unmodelled bits.
    pub const fn from_bits_truncate(bits: u64) -> Self {
        Flags(bits & Self::known_mask().0)
    }

    /// The mask covering every modelled flag.
    pub const fn known_mask() -> Flags {
        let mut mask = 0u64;
        let mut index = 0;
        // A `const fn` cannot use iterators, so the loop is written out.
        while index < Flag::ALL.len() {
            mask |= 1u64 << Flag::ALL[index].bit();
            index += 1;
        }
        Flags(mask)
    }

    /// The raw value.
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Whether every flag in `other` is present.
    pub const fn contains(self, other: Flags) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Whether any flag in `other` is present.
    pub const fn intersects(self, other: Flags) -> bool {
        (self.0 & other.0) != 0
    }

    /// Whether no modelled flag is set.
    pub const fn is_empty(self) -> bool {
        (self.0 & Self::known_mask().0) == 0
    }

    /// Whether a single flag is set.
    pub const fn has(self, flag: Flag) -> bool {
        self.contains(flag.mask())
    }

    /// Returns the set with `flag` set or cleared.
    pub fn with(self, flag: Flag, value: bool) -> Self {
        if value {
            self | flag.mask()
        } else {
            self - flag.mask()
        }
    }

    /// Every modelled flag that is currently set.
    pub fn iter(self) -> impl Iterator<Item = Flag> {
        Flag::ALL.into_iter().filter(move |flag| self.has(*flag))
    }

    /// The flags that differ between `self` and `other`.
    ///
    /// Used to highlight what the last instruction changed.
    pub fn changed_from(self, other: Flags) -> Vec<Flag> {
        Flag::ALL
            .into_iter()
            .filter(|flag| self.has(*flag) != other.has(*flag))
            .collect()
    }

    /// A compact rendering such as `"ZF PF"`, or `"-"` when nothing is set.
    pub fn summary(self) -> String {
        let names: Vec<&str> = self.iter().map(Flag::abbreviation).collect();
        if names.is_empty() {
            "-".to_owned()
        } else {
            names.join(" ")
        }
    }
}

impl BitOr for Flags {
    type Output = Flags;
    fn bitor(self, rhs: Flags) -> Flags {
        Flags(self.0 | rhs.0)
    }
}

impl BitAnd for Flags {
    type Output = Flags;
    fn bitand(self, rhs: Flags) -> Flags {
        Flags(self.0 & rhs.0)
    }
}

impl BitXor for Flags {
    type Output = Flags;
    fn bitxor(self, rhs: Flags) -> Flags {
        Flags(self.0 ^ rhs.0)
    }
}

impl Sub for Flags {
    type Output = Flags;
    fn sub(self, rhs: Flags) -> Flags {
        Flags(self.0 & !rhs.0)
    }
}

impl Not for Flags {
    type Output = Flags;
    fn not(self) -> Flags {
        Flags(!self.0)
    }
}

impl fmt::Display for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.summary())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_bit_positions_match_the_architecture() {
        // Values from the Intel SDM, volume 1, section 3.4.3.
        assert_eq!(Flag::Carry.bit(), 0);
        assert_eq!(Flag::Parity.bit(), 2);
        assert_eq!(Flag::Auxiliary.bit(), 4);
        assert_eq!(Flag::Zero.bit(), 6);
        assert_eq!(Flag::Sign.bit(), 7);
        assert_eq!(Flag::Trap.bit(), 8);
        assert_eq!(Flag::Interrupt.bit(), 9);
        assert_eq!(Flag::Direction.bit(), 10);
        assert_eq!(Flag::Overflow.bit(), 11);
    }

    #[test]
    fn all_nine_required_flags_are_modelled() {
        let abbreviations: Vec<&str> = Flag::ALL.iter().map(|f| f.abbreviation()).collect();
        for expected in ["CF", "PF", "AF", "ZF", "SF", "TF", "IF", "DF", "OF"] {
            assert!(abbreviations.contains(&expected), "missing {expected}");
        }
        assert_eq!(Flag::ALL.len(), 9);
    }

    #[test]
    fn a_real_rflags_value_decodes_correctly() {
        // 0x246 is the value Linux hands a fresh process: ZF, PF and IF set.
        let flags = Flags::from_bits(0x246);
        assert!(flags.has(Flag::Zero));
        assert!(flags.has(Flag::Parity));
        assert!(flags.has(Flag::Interrupt));
        assert!(!flags.has(Flag::Carry));
        assert!(!flags.has(Flag::Sign));
        assert!(!flags.has(Flag::Overflow));
    }

    #[test]
    fn summary_lists_set_flags_in_bit_order() {
        let flags = Flags::from_bits(0x246);
        assert_eq!(flags.summary(), "PF ZF IF");
        assert_eq!(Flags::empty().summary(), "-");
    }

    #[test]
    fn raw_bits_are_preserved_but_unmodelled_bits_are_not_reported() {
        // Bit 1 always reads as 1 on real hardware and is not a modelled flag.
        let flags = Flags::from_bits(0b10);
        assert_eq!(flags.bits(), 0b10);
        assert!(flags.is_empty(), "reserved bits must not appear as flags");
        assert_eq!(flags.iter().count(), 0);
    }

    #[test]
    fn truncation_drops_unmodelled_bits() {
        let flags = Flags::from_bits_truncate(u64::MAX);
        assert_eq!(flags, Flags::known_mask());
        assert_eq!(flags.iter().count(), Flag::ALL.len());
    }

    #[test]
    fn set_operations_behave_like_a_bit_set() {
        let a = Flags::CARRY | Flags::ZERO;
        assert!(a.contains(Flags::CARRY));
        assert!(a.contains(Flags::CARRY | Flags::ZERO));
        assert!(!a.contains(Flags::CARRY | Flags::SIGN));
        assert!(a.intersects(Flags::CARRY | Flags::SIGN));
        assert_eq!(a - Flags::CARRY, Flags::ZERO);
        assert_eq!((a & Flags::CARRY), Flags::CARRY);
        assert_eq!(a ^ a, Flags::empty());
    }

    #[test]
    fn with_sets_and_clears_a_single_flag() {
        let flags = Flags::empty().with(Flag::Sign, true);
        assert!(flags.has(Flag::Sign));
        let flags = flags.with(Flag::Sign, false);
        assert!(!flags.has(Flag::Sign));
    }

    #[test]
    fn changed_from_reports_only_the_differing_flags() {
        let before = Flags::CARRY | Flags::ZERO;
        let after = Flags::ZERO | Flags::SIGN;
        let changed = after.changed_from(before);
        assert_eq!(changed, vec![Flag::Carry, Flag::Sign]);
        assert!(after.changed_from(after).is_empty());
    }

    #[test]
    fn status_and_system_flags_are_distinguished() {
        for flag in Flag::ARITHMETIC {
            assert!(flag.is_status_flag(), "{flag} should be a status flag");
        }
        for flag in [Flag::Trap, Flag::Interrupt, Flag::Direction] {
            assert!(!flag.is_status_flag(), "{flag} is a system flag");
        }
    }

    #[test]
    fn abbreviations_resolve_case_insensitively() {
        assert_eq!(Flag::from_abbreviation("zf"), Some(Flag::Zero));
        assert_eq!(Flag::from_abbreviation(" OF "), Some(Flag::Overflow));
        assert_eq!(Flag::from_abbreviation("xx"), None);
    }

    #[test]
    fn every_flag_has_non_empty_documentation() {
        for flag in Flag::ALL {
            assert!(!flag.name().is_empty());
            assert!(!flag.description().is_empty());
            assert!(!flag.when_set().is_empty());
            assert_eq!(flag.abbreviation().len(), 2);
        }
    }

    #[test]
    fn flag_masks_are_distinct() {
        let mut seen: Vec<u64> = Vec::new();
        for flag in Flag::ALL {
            let bits = flag.mask().bits();
            assert!(
                !seen.contains(&bits),
                "{flag} shares a bit with another flag"
            );
            seen.push(bits);
        }
    }
}

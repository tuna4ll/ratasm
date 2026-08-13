//! x86 condition codes and the flag tests behind them.
//!
//! Conditional jumps, `setcc` and `cmovcc` all share one set of sixteen
//! conditions. Modelling them once, as code that can be *evaluated* against a
//! real `RFLAGS` value, is what lets the flag panel answer the question a
//! learner actually has: given the flags right now, which branches would be
//! taken?
//!
//! Signed and unsigned comparisons are the classic confusion here, and the
//! table makes the difference explicit: unsigned ordering tests `CF` and `ZF`,
//! signed ordering tests `SF`, `OF` and `ZF`. `ja` and `jg` are not synonyms,
//! and [`ConditionCode::is_signed`] says which is which.

use super::flags::Flags;

/// One of the sixteen x86 condition codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConditionCode {
    /// Overflow: `OF = 1`.
    Overflow,
    /// Not overflow: `OF = 0`.
    NotOverflow,
    /// Below / carry, unsigned `<`: `CF = 1`.
    Below,
    /// Above or equal / no carry, unsigned `>=`: `CF = 0`.
    AboveOrEqual,
    /// Equal / zero: `ZF = 1`.
    Equal,
    /// Not equal / not zero: `ZF = 0`.
    NotEqual,
    /// Below or equal, unsigned `<=`: `CF = 1 or ZF = 1`.
    BelowOrEqual,
    /// Above, unsigned `>`: `CF = 0 and ZF = 0`.
    Above,
    /// Sign: `SF = 1`.
    Sign,
    /// Not sign: `SF = 0`.
    NotSign,
    /// Parity even: `PF = 1`.
    Parity,
    /// Parity odd: `PF = 0`.
    NotParity,
    /// Less, signed `<`: `SF <> OF`.
    Less,
    /// Greater or equal, signed `>=`: `SF = OF`.
    GreaterOrEqual,
    /// Less or equal, signed `<=`: `ZF = 1 or SF <> OF`.
    LessOrEqual,
    /// Greater, signed `>`: `ZF = 0 and SF = OF`.
    Greater,
}

impl ConditionCode {
    /// Every condition code.
    pub const ALL: [ConditionCode; 16] = [
        ConditionCode::Overflow,
        ConditionCode::NotOverflow,
        ConditionCode::Below,
        ConditionCode::AboveOrEqual,
        ConditionCode::Equal,
        ConditionCode::NotEqual,
        ConditionCode::BelowOrEqual,
        ConditionCode::Above,
        ConditionCode::Sign,
        ConditionCode::NotSign,
        ConditionCode::Parity,
        ConditionCode::NotParity,
        ConditionCode::Less,
        ConditionCode::GreaterOrEqual,
        ConditionCode::LessOrEqual,
        ConditionCode::Greater,
    ];

    /// The mnemonic suffixes that select this condition.
    ///
    /// The first entry is the canonical spelling; the rest are the documented
    /// synonyms, so `jz` and `je` both resolve to [`ConditionCode::Equal`].
    pub const fn suffixes(self) -> &'static [&'static str] {
        match self {
            ConditionCode::Overflow => &["o"],
            ConditionCode::NotOverflow => &["no"],
            ConditionCode::Below => &["b", "c", "nae"],
            ConditionCode::AboveOrEqual => &["ae", "nb", "nc"],
            ConditionCode::Equal => &["e", "z"],
            ConditionCode::NotEqual => &["ne", "nz"],
            ConditionCode::BelowOrEqual => &["be", "na"],
            ConditionCode::Above => &["a", "nbe"],
            ConditionCode::Sign => &["s"],
            ConditionCode::NotSign => &["ns"],
            ConditionCode::Parity => &["p", "pe"],
            ConditionCode::NotParity => &["np", "po"],
            ConditionCode::Less => &["l", "nge"],
            ConditionCode::GreaterOrEqual => &["ge", "nl"],
            ConditionCode::LessOrEqual => &["le", "ng"],
            ConditionCode::Greater => &["g", "nle"],
        }
    }

    /// The canonical mnemonic suffix, for example `"e"` for `je`.
    pub fn canonical_suffix(self) -> &'static str {
        self.suffixes()[0]
    }

    /// The flag test in symbolic form, for example `"ZF = 1"`.
    pub const fn expression(self) -> &'static str {
        match self {
            ConditionCode::Overflow => "OF = 1",
            ConditionCode::NotOverflow => "OF = 0",
            ConditionCode::Below => "CF = 1",
            ConditionCode::AboveOrEqual => "CF = 0",
            ConditionCode::Equal => "ZF = 1",
            ConditionCode::NotEqual => "ZF = 0",
            ConditionCode::BelowOrEqual => "CF = 1 or ZF = 1",
            ConditionCode::Above => "CF = 0 and ZF = 0",
            ConditionCode::Sign => "SF = 1",
            ConditionCode::NotSign => "SF = 0",
            ConditionCode::Parity => "PF = 1",
            ConditionCode::NotParity => "PF = 0",
            ConditionCode::Less => "SF <> OF",
            ConditionCode::GreaterOrEqual => "SF = OF",
            ConditionCode::LessOrEqual => "ZF = 1 or SF <> OF",
            ConditionCode::Greater => "ZF = 0 and SF = OF",
        }
    }

    /// A plain-language description of when the condition holds.
    pub const fn description(self) -> &'static str {
        match self {
            ConditionCode::Overflow => "signed overflow occurred",
            ConditionCode::NotOverflow => "no signed overflow occurred",
            ConditionCode::Below => "unsigned less than",
            ConditionCode::AboveOrEqual => "unsigned greater than or equal",
            ConditionCode::Equal => "equal",
            ConditionCode::NotEqual => "not equal",
            ConditionCode::BelowOrEqual => "unsigned less than or equal",
            ConditionCode::Above => "unsigned greater than",
            ConditionCode::Sign => "result is negative",
            ConditionCode::NotSign => "result is not negative",
            ConditionCode::Parity => "low byte has an even number of set bits",
            ConditionCode::NotParity => "low byte has an odd number of set bits",
            ConditionCode::Less => "signed less than",
            ConditionCode::GreaterOrEqual => "signed greater than or equal",
            ConditionCode::LessOrEqual => "signed less than or equal",
            ConditionCode::Greater => "signed greater than",
        }
    }

    /// Whether the condition interprets its operands as signed.
    ///
    /// Distinguishes `jg` (signed) from `ja` (unsigned); conditions that test a
    /// single flag directly are neither and report `false`.
    pub const fn is_signed(self) -> bool {
        matches!(
            self,
            ConditionCode::Less
                | ConditionCode::LessOrEqual
                | ConditionCode::Greater
                | ConditionCode::GreaterOrEqual
        )
    }

    /// Whether the condition interprets its operands as unsigned.
    pub const fn is_unsigned(self) -> bool {
        matches!(
            self,
            ConditionCode::Below
                | ConditionCode::BelowOrEqual
                | ConditionCode::Above
                | ConditionCode::AboveOrEqual
        )
    }

    /// The flags this condition inspects.
    pub fn flags_used(self) -> Vec<Flags> {
        match self {
            ConditionCode::Overflow | ConditionCode::NotOverflow => vec![Flags::OVERFLOW],
            ConditionCode::Below | ConditionCode::AboveOrEqual => vec![Flags::CARRY],
            ConditionCode::Equal | ConditionCode::NotEqual => vec![Flags::ZERO],
            ConditionCode::BelowOrEqual | ConditionCode::Above => {
                vec![Flags::CARRY, Flags::ZERO]
            }
            ConditionCode::Sign | ConditionCode::NotSign => vec![Flags::SIGN],
            ConditionCode::Parity | ConditionCode::NotParity => vec![Flags::PARITY],
            ConditionCode::Less | ConditionCode::GreaterOrEqual => {
                vec![Flags::SIGN, Flags::OVERFLOW]
            }
            ConditionCode::LessOrEqual | ConditionCode::Greater => {
                vec![Flags::ZERO, Flags::SIGN, Flags::OVERFLOW]
            }
        }
    }

    /// Evaluates the condition against a concrete flags value.
    ///
    /// This is what turns the flag panel from a bit display into an answer:
    /// with real flags in hand it reports exactly which branches would be
    /// taken.
    pub fn evaluate(self, flags: Flags) -> bool {
        let cf = flags.contains(Flags::CARRY);
        let zf = flags.contains(Flags::ZERO);
        let sf = flags.contains(Flags::SIGN);
        let of = flags.contains(Flags::OVERFLOW);
        let pf = flags.contains(Flags::PARITY);

        match self {
            ConditionCode::Overflow => of,
            ConditionCode::NotOverflow => !of,
            ConditionCode::Below => cf,
            ConditionCode::AboveOrEqual => !cf,
            ConditionCode::Equal => zf,
            ConditionCode::NotEqual => !zf,
            ConditionCode::BelowOrEqual => cf || zf,
            ConditionCode::Above => !cf && !zf,
            ConditionCode::Sign => sf,
            ConditionCode::NotSign => !sf,
            ConditionCode::Parity => pf,
            ConditionCode::NotParity => !pf,
            ConditionCode::Less => sf != of,
            ConditionCode::GreaterOrEqual => sf == of,
            ConditionCode::LessOrEqual => zf || (sf != of),
            ConditionCode::Greater => !zf && (sf == of),
        }
    }

    /// Resolves a bare suffix such as `"nz"` to a condition code.
    pub fn from_suffix(suffix: &str) -> Option<Self> {
        let suffix = suffix.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|code| code.suffixes().contains(&suffix.as_str()))
    }

    /// Splits a conditional mnemonic into its prefix and condition.
    ///
    /// Returns `None` when the mnemonic does not start with `prefix` or the
    /// remainder is not a condition suffix, so `jmp` is correctly *not* read as
    /// a conditional jump with suffix `mp`.
    pub fn from_mnemonic(mnemonic: &str, prefix: &str) -> Option<Self> {
        let mnemonic = mnemonic.trim().to_ascii_lowercase();
        let suffix = mnemonic.strip_prefix(prefix)?;
        Self::from_suffix(suffix)
    }

    /// All conditional-jump mnemonics for this condition, such as `je`, `jz`.
    pub fn jump_mnemonics(self) -> Vec<String> {
        self.suffixes()
            .iter()
            .map(|suffix| format!("j{suffix}"))
            .collect()
    }
}

/// Reports every condition that currently holds, with its jump mnemonics.
///
/// Used by the flag panel to show which branches would be taken right now.
pub fn satisfied_conditions(flags: Flags) -> Vec<ConditionCode> {
    ConditionCode::ALL
        .into_iter()
        .filter(|code| code.evaluate(flags))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_flag_selects_the_equal_conditions() {
        let flags = Flags::ZERO;
        assert!(ConditionCode::Equal.evaluate(flags));
        assert!(!ConditionCode::NotEqual.evaluate(flags));
    }

    #[test]
    fn je_and_jz_are_the_same_condition() {
        assert_eq!(ConditionCode::from_suffix("e"), Some(ConditionCode::Equal));
        assert_eq!(ConditionCode::from_suffix("z"), Some(ConditionCode::Equal));
        assert!(ConditionCode::Equal
            .jump_mnemonics()
            .contains(&"jz".to_owned()));
        assert!(ConditionCode::Equal
            .jump_mnemonics()
            .contains(&"je".to_owned()));
    }

    #[test]
    fn signed_and_unsigned_comparisons_are_not_synonyms() {
        // CF=1 (unsigned below) with SF=OF (signed greater or equal).
        let flags = Flags::CARRY;
        assert!(ConditionCode::Below.evaluate(flags), "unsigned: below");
        assert!(
            ConditionCode::GreaterOrEqual.evaluate(flags),
            "signed: greater or equal"
        );
        assert!(ConditionCode::Above.is_unsigned());
        assert!(ConditionCode::Greater.is_signed());
        assert!(!ConditionCode::Above.is_signed());
    }

    #[test]
    fn signed_less_than_is_sign_not_equal_to_overflow() {
        assert!(ConditionCode::Less.evaluate(Flags::SIGN));
        assert!(ConditionCode::Less.evaluate(Flags::OVERFLOW));
        assert!(!ConditionCode::Less.evaluate(Flags::SIGN | Flags::OVERFLOW));
        assert!(!ConditionCode::Less.evaluate(Flags::empty()));
    }

    #[test]
    fn greater_requires_zero_clear_and_sign_matching_overflow() {
        assert!(ConditionCode::Greater.evaluate(Flags::empty()));
        assert!(!ConditionCode::Greater.evaluate(Flags::ZERO));
        assert!(!ConditionCode::Greater.evaluate(Flags::SIGN));
        assert!(ConditionCode::Greater.evaluate(Flags::SIGN | Flags::OVERFLOW));
    }

    #[test]
    fn every_condition_has_an_exact_complement() {
        // Each pair in ALL is a condition and its negation.
        for pair in ConditionCode::ALL.chunks(2) {
            let (a, b) = (pair[0], pair[1]);
            for bits in 0u64..64 {
                let flags = Flags::from_bits_truncate(bits);
                assert_ne!(
                    a.evaluate(flags),
                    b.evaluate(flags),
                    "{a:?} and {b:?} must be complementary for flags {bits:#x}"
                );
            }
        }
    }

    #[test]
    fn mnemonic_splitting_rejects_non_conditional_jumps() {
        assert_eq!(ConditionCode::from_mnemonic("jmp", "j"), None);
        assert_eq!(
            ConditionCode::from_mnemonic("jne", "j"),
            Some(ConditionCode::NotEqual)
        );
        assert_eq!(
            ConditionCode::from_mnemonic("setle", "set"),
            Some(ConditionCode::LessOrEqual)
        );
        assert_eq!(
            ConditionCode::from_mnemonic("cmovg", "cmov"),
            Some(ConditionCode::Greater)
        );
    }

    #[test]
    fn suffix_lookup_is_case_insensitive() {
        // NASM accepts mnemonics in any case, so lookups must too.
        assert_eq!(
            ConditionCode::from_suffix("NZ"),
            Some(ConditionCode::NotEqual)
        );
        for spelling in ["jge", "JGE", "Jge"] {
            assert_eq!(
                ConditionCode::from_mnemonic(spelling, "j"),
                Some(ConditionCode::GreaterOrEqual),
                "{spelling} must resolve"
            );
        }
    }

    #[test]
    fn satisfied_conditions_reports_a_consistent_set() {
        let flags = Flags::ZERO | Flags::PARITY;
        let satisfied = satisfied_conditions(flags);
        assert!(satisfied.contains(&ConditionCode::Equal));
        assert!(satisfied.contains(&ConditionCode::BelowOrEqual));
        assert!(satisfied.contains(&ConditionCode::LessOrEqual));
        assert!(!satisfied.contains(&ConditionCode::NotEqual));
        assert!(!satisfied.contains(&ConditionCode::Greater));
        // Exactly half of the sixteen conditions hold for any flags value.
        assert_eq!(satisfied.len(), 8);
    }

    #[test]
    fn every_condition_ignores_flags_it_does_not_read() {
        for code in ConditionCode::ALL {
            let used = code.flags_used();
            assert!(!used.is_empty(), "{code:?} must read at least one flag");
            let relevant = used.iter().fold(Flags::empty(), |acc, f| acc | *f);
            // Setting every flag the condition does not consult must not
            // change its verdict.
            let noise = Flags::known_mask() - relevant;
            assert_eq!(
                code.evaluate(Flags::empty()),
                code.evaluate(noise),
                "{code:?} must ignore flags outside {relevant}"
            );
        }
    }

    #[test]
    fn suffixes_are_unique_across_conditions() {
        let mut seen = Vec::new();
        for code in ConditionCode::ALL {
            for suffix in code.suffixes() {
                assert!(!seen.contains(suffix), "suffix {suffix} appears twice");
                seen.push(suffix);
            }
        }
    }
}

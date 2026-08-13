//! Recognised x86-64 instruction mnemonics.
//!
//! The syntax highlighter and the completion engine both need to answer "is
//! this word an instruction?" for every identifier they see, so the answer has
//! to be cheap and it has to be one shared source of truth.
//!
//! Condition-code variants are not listed individually. `j`, `set`, `cmov` and
//! `fcmov` each combine with all sixteen conditions and their synonyms, which
//! would be over a hundred near-duplicate entries to keep in step by hand.
//! They are recognised structurally instead, by splitting the prefix and
//! resolving the remainder through [`ConditionCode`].

use super::conditions::ConditionCode;

/// Mnemonics that take a condition-code suffix.
const CONDITIONAL_PREFIXES: [&str; 4] = ["j", "set", "cmov", "fcmov"];

/// Mnemonics recognised verbatim, sorted so lookup can binary search.
///
/// Kept in one sorted list rather than grouped by category because the only
/// question asked of it is membership; semantic grouping lives in the
/// instruction database, where it can carry explanations with it.
// rustfmt would print this table one entry per line, turning a scannable grid
// into four hundred lines of noise.
#[rustfmt::skip]
const BASE_MNEMONICS: &[&str] = &[
    "aaa", "aad", "aam", "aas", "adc", "add", "addpd", "addps", "addsd", "addss", "and", "andn",
    "andnpd", "andnps", "andpd", "andps", "bextr", "blsi", "blsmsk", "blsr", "bsf", "bsr",
    "bswap", "bt", "btc", "btr", "bts", "call", "cbw", "cdq", "cdqe", "clc", "cld", "clflush",
    "cli", "cmc", "cmp", "cmppd", "cmpps", "cmps", "cmpsb", "cmpsd", "cmpsq", "cmpsw",
    "cmpxchg", "cmpxchg16b", "cmpxchg8b", "comisd", "comiss", "cpuid", "cqo", "cvtsd2si",
    "cvtsd2ss", "cvtsi2sd", "cvtsi2ss", "cvtss2sd", "cvtss2si", "cvttsd2si", "cvttss2si", "cwd",
    "cwde", "daa", "das", "dec", "div", "divpd", "divps", "divsd", "divss", "emms", "endbr32",
    "endbr64", "enter", "f2xm1", "fabs", "fadd", "faddp", "fchs", "fcom", "fcomp", "fcos",
    "fdiv", "fdivp", "fdivr", "fild", "fist", "fistp", "fld", "fld1", "fldz", "fmul", "fmulp",
    "fnstsw", "fprem", "fptan", "frndint", "fsin", "fsqrt", "fst", "fstp", "fsub", "fsubp",
    "fsubr", "ftst", "fucom", "fucomp", "fwait", "fxch", "hlt", "idiv", "imul", "in", "inc",
    "insb", "insd", "insw", "int", "int1", "int3", "into", "invd", "iret", "iretd", "iretq",
    "jecxz", "jmp", "jrcxz", "lahf", "lddqu", "ldmxcsr", "lea", "leave", "lfence", "lock",
    "lods", "lodsb", "lodsd", "lodsq", "lodsw", "loop", "loope", "loopne", "loopnz", "loopz",
    "lzcnt", "maxpd", "maxps", "maxsd", "maxss", "mfence", "minpd", "minps", "minsd", "minss",
    "monitor", "mov", "movabs", "movapd", "movaps", "movbe", "movd", "movdqa", "movdqu",
    "movhpd", "movhps", "movlpd", "movlps", "movmskpd", "movmskps", "movntdq", "movnti",
    "movntpd", "movntps", "movq", "movs", "movsb", "movsd", "movsq", "movss", "movsw", "movsx",
    "movsxd", "movupd", "movups", "movzx", "mul", "mulpd", "mulps", "mulsd", "mulss", "mwait",
    "neg", "nop", "not", "or", "orpd", "orps", "out", "outsb", "outsd", "outsw", "packssdw",
    "packsswb", "packuswb", "paddb", "paddd", "paddq", "paddsb", "paddsw", "paddusb", "paddusw",
    "paddw", "pand", "pandn", "pause", "pavgb", "pavgw", "pcmpeqb", "pcmpeqd", "pcmpeqw",
    "pcmpgtb", "pcmpgtd", "pcmpgtw", "pextrw", "pinsrw", "pmaddwd", "pmaxsw", "pmaxub",
    "pminsw", "pminub", "pmovmskb", "pmulhuw", "pmulhw", "pmullw", "pmuludq", "pop", "popa",
    "popad", "popcnt", "popf", "popfd", "popfq", "por", "prefetchnta", "prefetcht0",
    "prefetcht1", "prefetcht2", "psadbw", "pshufd", "pshufhw", "pshuflw", "pshufw", "pslld",
    "pslldq", "psllq", "psllw", "psrad", "psraw", "psrld", "psrldq", "psrlq", "psrlw", "psubb",
    "psubd", "psubq", "psubsb", "psubsw", "psubusb", "psubusw", "psubw", "punpckhbw",
    "punpckhdq", "punpckhqdq", "punpckhwd", "punpcklbw", "punpckldq", "punpcklqdq", "punpcklwd",
    "push", "pusha", "pushad", "pushf", "pushfd", "pushfq", "pxor", "rcl", "rcpps", "rcpss",
    "rcr", "rdmsr", "rdpmc", "rdrand", "rdseed", "rdtsc", "rdtscp", "rep", "repe", "repne",
    "repnz", "repz", "ret", "retf", "retn", "rol", "ror", "rsqrtps", "rsqrtss", "sahf", "sal",
    "sar", "sbb", "scas", "scasb", "scasd", "scasq", "scasw", "sfence", "shl", "shld", "shr",
    "shrd", "shufpd", "shufps", "sqrtpd", "sqrtps", "sqrtsd", "sqrtss", "stc", "std", "sti",
    "stmxcsr", "stos", "stosb", "stosd", "stosq", "stosw", "sub", "subpd", "subps", "subsd",
    "subss", "syscall", "sysenter", "sysexit", "sysret", "test", "tzcnt", "ucomisd", "ucomiss",
    "ud2", "unpckhpd", "unpckhps", "unpcklpd", "unpcklps", "vzeroall", "vzeroupper", "wait",
    "wbinvd", "wrmsr", "xadd", "xchg", "xgetbv", "xlat", "xlatb", "xor", "xorpd", "xorps"
];

/// Returns `true` when `word` names an instruction.
///
/// Matching is case-insensitive because NASM accepts any case, and it accounts
/// for condition-code variants such as `jne`, `setle` and `cmovg`.
pub fn is_mnemonic(word: &str) -> bool {
    let word = word.trim().to_ascii_lowercase();
    if word.is_empty() {
        return false;
    }
    if BASE_MNEMONICS.binary_search(&word.as_str()).is_ok() {
        return true;
    }
    conditional_parts(&word).is_some()
}

/// Splits a conditional mnemonic into its prefix and condition code.
///
/// Returns `None` for mnemonics that are not condition-code variants, so
/// `jmp` — which starts with `j` but whose remainder is not a condition — is
/// correctly excluded.
pub fn conditional_parts(word: &str) -> Option<(&'static str, ConditionCode)> {
    let word = word.trim().to_ascii_lowercase();
    // Longest prefix first, so `cmov` wins over nothing and `set` is not
    // mistaken for part of a longer name.
    let mut prefixes = CONDITIONAL_PREFIXES;
    prefixes.sort_unstable_by_key(|prefix| std::cmp::Reverse(prefix.len()));
    for prefix in prefixes {
        if let Some(code) = ConditionCode::from_mnemonic(&word, prefix) {
            return Some((prefix, code));
        }
    }
    None
}

/// The condition code a mnemonic tests, if it is a conditional instruction.
pub fn condition_of(word: &str) -> Option<ConditionCode> {
    conditional_parts(word).map(|(_, code)| code)
}

/// Whether the mnemonic is a conditional jump.
pub fn is_conditional_jump(word: &str) -> bool {
    matches!(conditional_parts(word), Some(("j", _)))
}

/// Every recognised mnemonic, including generated condition-code variants.
///
/// Allocates, so this is for completion lists and tests rather than for the
/// per-token hot path; use [`is_mnemonic`] there.
pub fn all() -> Vec<String> {
    let mut names: Vec<String> = BASE_MNEMONICS.iter().map(|m| (*m).to_owned()).collect();
    for prefix in CONDITIONAL_PREFIXES {
        for code in ConditionCode::ALL {
            for suffix in code.suffixes() {
                names.push(format!("{prefix}{suffix}"));
            }
        }
    }
    names.sort_unstable();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_table_is_sorted_and_unique() {
        // Membership uses a binary search, so ordering is load-bearing.
        let mut sorted = BASE_MNEMONICS.to_vec();
        sorted.sort_unstable();
        assert_eq!(
            BASE_MNEMONICS,
            &sorted[..],
            "BASE_MNEMONICS must stay sorted"
        );
        sorted.dedup();
        assert_eq!(BASE_MNEMONICS.len(), sorted.len(), "entries must be unique");
    }

    #[test]
    fn base_table_is_lowercase() {
        for mnemonic in BASE_MNEMONICS {
            assert_eq!(
                *mnemonic,
                mnemonic.to_ascii_lowercase(),
                "{mnemonic} must be lowercase"
            );
        }
    }

    #[test]
    fn common_instructions_are_recognised() {
        for word in [
            "mov", "add", "sub", "push", "pop", "call", "ret", "syscall", "lea", "cmp", "xor",
            "int", "nop", "imul", "shl", "movzx", "leave",
        ] {
            assert!(is_mnemonic(word), "{word} should be a mnemonic");
        }
    }

    #[test]
    fn recognition_is_case_insensitive() {
        for word in ["MOV", "Mov", "SYSCALL", "JNE"] {
            assert!(is_mnemonic(word), "{word} should be a mnemonic");
        }
    }

    #[test]
    fn conditional_variants_are_recognised_without_being_listed() {
        for word in [
            "je", "jz", "jne", "jnz", "jg", "jle", "jae", "setne", "setl", "cmovg",
        ] {
            assert!(is_mnemonic(word), "{word} should be a mnemonic");
        }
        assert!(
            !BASE_MNEMONICS.contains(&"je"),
            "je is generated, not listed"
        );
    }

    #[test]
    fn non_conditional_lookalikes_are_not_mistaken_for_conditionals() {
        // `jmp` starts with `j` but `mp` is not a condition suffix.
        assert_eq!(conditional_parts("jmp"), None);
        assert!(!is_conditional_jump("jmp"));
        assert!(is_mnemonic("jmp"), "jmp is still an instruction");
    }

    #[test]
    fn conditional_parts_reports_prefix_and_condition() {
        assert_eq!(
            conditional_parts("jle"),
            Some(("j", ConditionCode::LessOrEqual))
        );
        assert_eq!(
            conditional_parts("setae"),
            Some(("set", ConditionCode::AboveOrEqual))
        );
        assert_eq!(
            conditional_parts("cmovnz"),
            Some(("cmov", ConditionCode::NotEqual))
        );
    }

    #[test]
    fn conditional_jumps_are_distinguished_from_setcc_and_cmovcc() {
        assert!(is_conditional_jump("jne"));
        assert!(!is_conditional_jump("setne"));
        assert!(!is_conditional_jump("cmovne"));
    }

    #[test]
    fn condition_of_resolves_the_tested_condition() {
        assert_eq!(condition_of("jz"), Some(ConditionCode::Equal));
        assert_eq!(condition_of("mov"), None);
    }

    #[test]
    fn non_instructions_are_rejected() {
        for word in [
            "", "   ", "rax", "section", "_start", "byte", "db", "hello", "movq2", "j",
        ] {
            assert!(!is_mnemonic(word), "{word} must not be a mnemonic");
        }
    }

    #[test]
    fn registers_are_not_mistaken_for_instructions() {
        // A real collision risk: both tables are consulted for every word.
        for register in crate::instruction::registers::all_alias_names() {
            assert!(
                !is_mnemonic(register),
                "register {register} must not be recognised as an instruction"
            );
        }
    }

    #[test]
    fn generated_list_contains_both_base_and_conditional_entries() {
        let all = all();
        assert!(all.contains(&"mov".to_owned()));
        assert!(all.contains(&"jne".to_owned()));
        assert!(all.contains(&"cmovle".to_owned()));
        assert!(all.len() > BASE_MNEMONICS.len());
        // Every generated entry must also pass the membership test.
        for mnemonic in &all {
            assert!(is_mnemonic(mnemonic), "{mnemonic} failed round trip");
        }
    }

    #[test]
    fn required_instruction_groups_are_represented() {
        // The groups the tool promises to explain must all have members.
        let groups: [(&str, &[&str]); 11] = [
            ("data movement", &["mov", "movzx", "lea", "xchg"]),
            ("arithmetic", &["add", "sub", "imul", "idiv", "neg"]),
            ("bitwise", &["and", "or", "xor", "not", "test"]),
            ("shift and rotate", &["shl", "shr", "sar", "rol", "rcr"]),
            ("comparison", &["cmp", "test", "ucomisd"]),
            ("conditional jump", &["je", "jne", "jg", "jbe"]),
            ("unconditional jump", &["jmp"]),
            ("stack", &["push", "pop", "pushfq", "leave"]),
            ("call and return", &["call", "ret", "enter"]),
            (
                "string",
                &["movsb", "stosq", "lodsb", "scasb", "cmpsb", "rep"],
            ),
            ("syscall", &["syscall", "int", "sysenter"]),
        ];
        for (group, members) in groups {
            for member in members {
                assert!(is_mnemonic(member), "{group}: {member} is missing");
            }
        }
    }

    #[test]
    fn basic_simd_instructions_are_recognised() {
        for word in ["movaps", "movdqu", "addps", "pxor", "punpcklbw", "cvtsi2sd"] {
            assert!(is_mnemonic(word), "{word} should be recognised");
        }
    }
}

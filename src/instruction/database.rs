//! The instruction semantics database.
//!
//! `assets/instructions.json` describes what each instruction does, which
//! operands it reads and writes, and which flags it touches. Keeping this as
//! data rather than as code in the UI is what lets the explanation panel, the
//! learning mode and the flag panel all agree, and it means adding an
//! instruction is a data change rather than a code change.
//!
//! # What "affected flags" means
//!
//! The database distinguishes four relationships, because collapsing them
//! loses the information a learner needs:
//!
//! - `flags_set` — the instruction computes this flag from its result.
//! - `flags_cleared` — the instruction forces it to zero regardless of the
//!   result, as `and` does to `CF` and `OF`.
//! - `flags_undefined` — the architecture leaves it in an unspecified state.
//!   These are reported as undefined rather than guessed, because code that
//!   relies on them is broken even when it happens to work.
//! - `flags_read` — the instruction consults the flag, as a conditional jump
//!   does.

use serde::{Deserialize, Serialize};

use super::conditions::ConditionCode;
use super::flags::Flag;

/// The embedded database.
const DATABASE_JSON: &str = include_str!("../../assets/instructions.json");

/// The semantics of one instruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionInfo {
    /// The mnemonic, lowercase.
    pub mnemonic: String,
    /// The category it belongs to, such as `arithmetic`.
    pub group: String,
    /// A one-line description.
    pub summary: String,
    /// The names of the operand slots, in written order.
    pub operands: Vec<String>,
    /// What the instruction does, with `{slot}` placeholders.
    pub effect: String,
    /// Slots or fixed registers the instruction reads.
    pub reads: Vec<String>,
    /// Slots or fixed registers the instruction writes.
    pub writes: Vec<String>,
    /// Flags computed from the result.
    pub flags_set: Vec<String>,
    /// Flags the instruction consults.
    pub flags_read: Vec<String>,
    /// Flags forced to zero.
    pub flags_cleared: Vec<String>,
    /// Flags left in an architecturally unspecified state.
    pub flags_undefined: Vec<String>,
    /// Extra guidance worth showing alongside the mechanics.
    pub notes: Option<String>,
}

impl InstructionInfo {
    /// Every flag the instruction modifies, in bit order.
    ///
    /// Combines the computed and forced-to-zero sets, which is what a user
    /// means by "which flags does this change".
    pub fn flags_modified(&self) -> Vec<Flag> {
        let mut flags: Vec<Flag> = self
            .flags_set
            .iter()
            .chain(self.flags_cleared.iter())
            .filter_map(|name| Flag::from_abbreviation(name))
            .collect();
        flags.sort_by_key(|flag| flag.bit());
        flags.dedup();
        flags
    }

    /// The flags the instruction consults, in bit order.
    pub fn flags_consulted(&self) -> Vec<Flag> {
        let mut flags: Vec<Flag> = self
            .flags_read
            .iter()
            .filter_map(|name| Flag::from_abbreviation(name))
            .collect();
        flags.sort_by_key(|flag| flag.bit());
        flags.dedup();
        flags
    }

    /// The flags left architecturally undefined, in bit order.
    pub fn flags_left_undefined(&self) -> Vec<Flag> {
        let mut flags: Vec<Flag> = self
            .flags_undefined
            .iter()
            .filter_map(|name| Flag::from_abbreviation(name))
            .collect();
        flags.sort_by_key(|flag| flag.bit());
        flags.dedup();
        flags
    }

    /// Whether the instruction changes control flow.
    pub fn is_branch(&self) -> bool {
        matches!(
            self.group.as_str(),
            "conditional-jump" | "unconditional-jump" | "call-return"
        )
    }

    /// The condition this instruction tests, when it is a conditional form.
    pub fn condition(&self) -> Option<ConditionCode> {
        super::mnemonics::condition_of(&self.mnemonic)
    }

    /// Whether the instruction's effect is modelled only by name.
    ///
    /// Vector instructions are recognised and described, but their per-lane
    /// behaviour is not simulated; the explanation says so rather than
    /// implying more precision than exists.
    pub fn is_approximate(&self) -> bool {
        self.group == "simd"
    }
}

/// The whole instruction database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Database {
    /// Schema version.
    pub version: u32,
    /// The architecture described.
    pub architecture: String,
    /// Every modelled instruction.
    pub instructions: Vec<InstructionInfo>,
}

impl Database {
    /// Parses the embedded database.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded asset is not valid JSON.
    pub fn load() -> Result<Self, serde_json::Error> {
        serde_json::from_str(DATABASE_JSON)
    }

    /// Looks up an instruction by mnemonic, case-insensitively.
    pub fn get(&self, mnemonic: &str) -> Option<&InstructionInfo> {
        let mnemonic = mnemonic.trim().to_ascii_lowercase();
        self.instructions
            .iter()
            .find(|info| info.mnemonic == mnemonic)
            .or_else(|| self.get_by_synonym(&mnemonic))
    }

    /// Resolves a documented synonym to its canonical entry.
    ///
    /// `jz` and `je` are the same instruction with two spellings, and the
    /// database stores one of them. Rather than duplicating sixteen families
    /// of entries, the condition is resolved and the canonical spelling looked
    /// up.
    fn get_by_synonym(&self, mnemonic: &str) -> Option<&InstructionInfo> {
        let (prefix, condition) = super::mnemonics::conditional_parts(mnemonic)?;
        let canonical = format!("{prefix}{}", condition.canonical_suffix());
        self.instructions
            .iter()
            .find(|info| info.mnemonic == canonical)
    }

    /// Every instruction in a group.
    pub fn in_group(&self, group: &str) -> Vec<&InstructionInfo> {
        self.instructions
            .iter()
            .filter(|info| info.group == group)
            .collect()
    }

    /// Every group name, sorted.
    pub fn groups(&self) -> Vec<&str> {
        let mut groups: Vec<&str> = self
            .instructions
            .iter()
            .map(|info| info.group.as_str())
            .collect();
        groups.sort_unstable();
        groups.dedup();
        groups
    }

    /// Searches mnemonics and summaries.
    pub fn search(&self, query: &str) -> Vec<&InstructionInfo> {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return self.instructions.iter().collect();
        }
        let mut found: Vec<&InstructionInfo> = self
            .instructions
            .iter()
            .filter(|info| {
                info.mnemonic.contains(&query) || info.summary.to_ascii_lowercase().contains(&query)
            })
            .collect();
        found.sort_by_key(|info| {
            if info.mnemonic == query {
                0
            } else if info.mnemonic.starts_with(&query) {
                1
            } else if info.mnemonic.contains(&query) {
                2
            } else {
                3
            }
        });
        found
    }

    /// The number of modelled instructions.
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    /// Whether the database is empty, which would mean a broken asset.
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Database {
        Database::load().expect("the embedded database must parse")
    }

    #[test]
    fn the_embedded_database_parses() {
        let db = database();
        assert!(!db.is_empty());
        assert_eq!(db.architecture, "x86_64");
        assert!(
            db.len() > 100,
            "expected a substantial table, got {}",
            db.len()
        );
    }

    #[test]
    fn mnemonics_are_unique_and_lowercase() {
        let db = database();
        let mut seen: Vec<&str> = Vec::new();
        for info in &db.instructions {
            assert_eq!(
                info.mnemonic,
                info.mnemonic.to_ascii_lowercase(),
                "{} is not lowercase",
                info.mnemonic
            );
            assert!(
                !seen.contains(&info.mnemonic.as_str()),
                "{} is duplicated",
                info.mnemonic
            );
            seen.push(&info.mnemonic);
        }
    }

    #[test]
    fn every_required_group_is_populated() {
        // The groups the tool promises to explain.
        let db = database();
        for group in [
            "data-movement",
            "arithmetic",
            "bitwise",
            "shift-rotate",
            "comparison",
            "conditional-jump",
            "unconditional-jump",
            "stack",
            "call-return",
            "string",
            "syscall",
            "simd",
        ] {
            assert!(
                !db.in_group(group).is_empty(),
                "group {group} has no instructions"
            );
        }
    }

    #[test]
    fn add_is_described_accurately() {
        let db = database();
        let add = db.get("add").expect("add");
        assert_eq!(add.group, "arithmetic");
        assert_eq!(add.operands, ["dst", "src"]);
        assert_eq!(add.effect, "{dst} ← {dst} + {src}");
        assert_eq!(add.reads, ["dst", "src"]);
        assert_eq!(add.writes, ["dst"]);

        let modified: Vec<&str> = add
            .flags_modified()
            .iter()
            .map(|f| f.abbreviation())
            .collect();
        assert_eq!(modified, ["CF", "PF", "AF", "ZF", "SF", "OF"]);
    }

    #[test]
    fn inc_does_not_touch_the_carry_flag() {
        // The detail that distinguishes inc from `add dst, 1`.
        let db = database();
        let inc = db.get("inc").expect("inc");
        let modified: Vec<&str> = inc
            .flags_modified()
            .iter()
            .map(|f| f.abbreviation())
            .collect();
        assert!(!modified.contains(&"CF"), "inc must leave CF alone");
        assert!(modified.contains(&"ZF"));
        assert!(modified.contains(&"OF"));
    }

    #[test]
    fn logical_operations_clear_carry_and_overflow() {
        let db = database();
        for mnemonic in ["and", "or", "xor", "test"] {
            let info = db.get(mnemonic).unwrap_or_else(|| panic!("{mnemonic}"));
            assert!(
                info.flags_cleared.contains(&"CF".to_owned()),
                "{mnemonic} should clear CF"
            );
            assert!(
                info.flags_cleared.contains(&"OF".to_owned()),
                "{mnemonic} should clear OF"
            );
        }
    }

    #[test]
    fn division_leaves_every_flag_undefined() {
        // Reporting a guess here would teach a genuinely wrong lesson.
        let db = database();
        for mnemonic in ["div", "idiv"] {
            let info = db.get(mnemonic).unwrap_or_else(|| panic!("{mnemonic}"));
            assert!(info.flags_set.is_empty(), "{mnemonic} defines no flags");
            assert_eq!(
                info.flags_left_undefined().len(),
                6,
                "{mnemonic} leaves all six arithmetic flags undefined"
            );
        }
    }

    #[test]
    fn not_affects_no_flags() {
        let db = database();
        let not = db.get("not").expect("not");
        assert!(not.flags_modified().is_empty());
        assert!(not.flags_consulted().is_empty());
    }

    #[test]
    fn conditional_jumps_read_the_flags_they_test() {
        let db = database();
        let je = db.get("je").expect("je");
        assert_eq!(je.group, "conditional-jump");
        let read: Vec<&str> = je
            .flags_consulted()
            .iter()
            .map(|f| f.abbreviation())
            .collect();
        assert_eq!(read, ["ZF"]);
        assert!(je.flags_modified().is_empty(), "a jump changes no flags");
        assert!(je.is_branch());
    }

    #[test]
    fn a_synonym_resolves_to_the_canonical_entry() {
        // `jz` is not stored separately; it must still resolve.
        let db = database();
        let jz = db.get("jz").expect("jz must resolve");
        assert_eq!(jz.mnemonic, "je");
        assert_eq!(db.get("jnz").map(|i| i.mnemonic.as_str()), Some("jne"));
        assert_eq!(db.get("setnge").map(|i| i.mnemonic.as_str()), Some("setl"));
    }

    #[test]
    fn signed_and_unsigned_jumps_read_different_flags() {
        let db = database();
        let signed: Vec<&str> = db
            .get("jg")
            .expect("jg")
            .flags_consulted()
            .iter()
            .map(|f| f.abbreviation())
            .collect();
        let unsigned: Vec<&str> = db
            .get("ja")
            .expect("ja")
            .flags_consulted()
            .iter()
            .map(|f| f.abbreviation())
            .collect();
        assert_eq!(signed, ["ZF", "SF", "OF"]);
        assert_eq!(unsigned, ["CF", "ZF"]);
        assert_ne!(signed, unsigned);
    }

    #[test]
    fn the_condition_of_a_conditional_instruction_is_recoverable() {
        let db = database();
        assert_eq!(
            db.get("jle").expect("jle").condition(),
            Some(ConditionCode::LessOrEqual)
        );
        assert_eq!(db.get("mov").expect("mov").condition(), None);
    }

    #[test]
    fn stack_instructions_describe_their_effect_on_rsp() {
        let db = database();
        let push = db.get("push").expect("push");
        assert!(push.writes.contains(&"rsp".to_owned()));
        assert!(push.effect.contains("RSP"));
        let pop = db.get("pop").expect("pop");
        assert!(pop.reads.contains(&"rsp".to_owned()));
    }

    #[test]
    fn syscall_documents_the_registers_it_destroys() {
        let db = database();
        let syscall = db.get("syscall").expect("syscall");
        assert!(syscall.writes.contains(&"rcx".to_owned()));
        assert!(syscall.writes.contains(&"r11".to_owned()));
        assert!(syscall.reads.contains(&"r10".to_owned()));
        let notes = syscall.notes.clone().unwrap_or_default();
        assert!(notes.contains("R10"), "the R10 rule must be explained");
    }

    #[test]
    fn string_instructions_read_the_direction_flag() {
        let db = database();
        for mnemonic in ["movsb", "stosq", "lodsb", "scasb", "cmpsb"] {
            let info = db.get(mnemonic).unwrap_or_else(|| panic!("{mnemonic}"));
            let read: Vec<&str> = info
                .flags_consulted()
                .iter()
                .map(|f| f.abbreviation())
                .collect();
            assert!(read.contains(&"DF"), "{mnemonic} must read DF");
        }
    }

    #[test]
    fn simd_entries_admit_they_are_approximate() {
        // The tool must not imply it simulates vector lanes.
        let db = database();
        for info in db.in_group("simd") {
            assert!(info.is_approximate());
            let notes = info.notes.clone().unwrap_or_default();
            assert!(
                notes.contains("does not model"),
                "{} should state its limits",
                info.mnemonic
            );
        }
    }

    #[test]
    fn every_entry_has_a_summary_and_an_effect() {
        let db = database();
        for info in &db.instructions {
            assert!(!info.summary.is_empty(), "{} has no summary", info.mnemonic);
            assert!(!info.effect.is_empty(), "{} has no effect", info.mnemonic);
            assert!(!info.group.is_empty(), "{} has no group", info.mnemonic);
        }
    }

    #[test]
    fn every_flag_name_in_the_database_is_real() {
        // A typo such as "FZ" would silently drop the flag from the display.
        let db = database();
        for info in &db.instructions {
            for name in info
                .flags_set
                .iter()
                .chain(&info.flags_read)
                .chain(&info.flags_cleared)
                .chain(&info.flags_undefined)
            {
                assert!(
                    Flag::from_abbreviation(name).is_some(),
                    "{}: {name:?} is not a flag",
                    info.mnemonic
                );
            }
        }
    }

    #[test]
    fn every_effect_placeholder_names_a_declared_operand() {
        // A placeholder with no matching operand would render as literal text.
        let db = database();
        for info in &db.instructions {
            let mut rest = info.effect.as_str();
            while let Some(start) = rest.find('{') {
                let Some(end) = rest[start..].find('}') else {
                    panic!(
                        "{}: unclosed placeholder in {:?}",
                        info.mnemonic, info.effect
                    )
                };
                let slot = &rest[start + 1..start + end];
                assert!(
                    info.operands.iter().any(|operand| operand == slot),
                    "{}: placeholder {{{slot}}} is not a declared operand",
                    info.mnemonic
                );
                rest = &rest[start + end + 1..];
            }
        }
    }

    #[test]
    fn every_read_and_write_slot_is_an_operand_or_a_register() {
        let db = database();
        for info in &db.instructions {
            for slot in info.reads.iter().chain(&info.writes) {
                let is_operand = info.operands.iter().any(|operand| operand == slot);
                let is_register = super::super::registers::is_register(slot);
                let is_pseudo = matches!(slot.as_str(), "memory" | "rip" | "rflags")
                    || Flag::from_abbreviation(slot).is_some();
                assert!(
                    is_operand || is_register || is_pseudo,
                    "{}: {slot:?} is neither an operand nor a register",
                    info.mnemonic
                );
            }
        }
    }

    #[test]
    fn every_database_mnemonic_is_recognised_by_the_lexer() {
        // The two tables must agree, or an instruction would highlight as an
        // unknown identifier while still having an explanation.
        let db = database();
        for info in &db.instructions {
            assert!(
                super::super::mnemonics::is_mnemonic(&info.mnemonic),
                "{} is in the database but not the mnemonic table",
                info.mnemonic
            );
        }
    }

    #[test]
    fn search_finds_by_mnemonic_and_description() {
        let db = database();
        assert_eq!(
            db.search("xor").first().map(|i| i.mnemonic.as_str()),
            Some("xor")
        );
        assert!(db.search("stack").iter().any(|i| i.mnemonic == "push"));
        assert!(db.search("zzzznothing").is_empty());
        assert_eq!(db.search("").len(), db.len());
    }

    #[test]
    fn an_unknown_mnemonic_yields_nothing_rather_than_a_guess() {
        let db = database();
        assert!(db.get("frobnicate").is_none());
        assert!(db.get("").is_none());
    }
}

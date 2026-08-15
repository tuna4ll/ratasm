//! The Linux x86-64 system call database.
//!
//! # Where the data comes from
//!
//! `assets/syscalls-x86_64.json` is embedded at compile time. The numbers were
//! taken from the kernel's own `asm/unistd_64.h`, not typed from memory, so
//! they are exactly what the running kernel expects.
//!
//! Roughly sixty calls — the ones an assembly programmer actually writes by
//! hand — carry a full description: argument names, types, which register each
//! travels in, the return convention, the errors worth knowing about, and a
//! NASM example. The remaining entries carry their name and number only and
//! are marked [`Syscall::detailed`] `== false`. That distinction is deliberate:
//! a plausible-looking but invented argument list would be worse than an
//! honest "look at the man page", because the user would act on it.
//!
//! # The calling convention
//!
//! The number goes in `RAX` and the arguments in `RDI`, `RSI`, `RDX`, `R10`,
//! `R8`, `R9`. The result comes back in `RAX`, with errors as small negative
//! values. The fourth argument is in `R10` rather than `RCX` because the
//! `syscall` instruction itself destroys `RCX` and `R11`.

use serde::{Deserialize, Serialize};

/// The embedded database, parsed on first use.
const DATABASE_JSON: &str = include_str!("../../assets/syscalls-x86_64.json");

/// One argument of a system call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argument {
    /// The register carrying this argument, such as `rdi`.
    pub register: String,
    /// The parameter name from the manual page.
    pub name: String,
    /// The C type of the parameter.
    #[serde(rename = "type")]
    pub c_type: String,
    /// What the parameter means, including useful constant values.
    pub description: String,
}

impl Argument {
    /// The register name in upper case, as the register panel shows it.
    pub fn register_display(&self) -> String {
        self.register.to_ascii_uppercase()
    }
}

/// One system call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Syscall {
    /// The number placed in `RAX`.
    pub number: u32,
    /// The call's name.
    pub name: String,
    /// A one-line summary.
    pub summary: String,
    /// The C prototype, when it is modelled.
    pub signature: Option<String>,
    /// A coarse category such as `file` or `process`.
    pub group: String,
    /// The arguments, in register order.
    pub args: Vec<Argument>,
    /// What the call returns in `RAX`.
    pub returns: Option<String>,
    /// Errors worth knowing about, as `-ERRNO: explanation`.
    pub errors: Vec<String>,
    /// A short NASM example, when one is useful.
    pub example: Option<String>,
    /// Whether the arguments and return value are modelled.
    ///
    /// `false` means only the name and number are known; nothing else about
    /// this call has been filled in, and nothing should be inferred.
    pub detailed: bool,
}

impl Syscall {
    /// The number of modelled arguments.
    pub fn arity(&self) -> usize {
        self.args.len()
    }

    /// The argument carried in `register`, if any.
    pub fn argument_in(&self, register: &str) -> Option<&Argument> {
        let register = register.trim().to_ascii_lowercase();
        self.args
            .iter()
            .find(|argument| argument.register == register)
    }

    /// A ready-to-paste NASM snippet setting up the call.
    ///
    /// Generated from the argument list when the call has no hand-written
    /// example, so every detailed call has something to copy.
    pub fn nasm_template(&self) -> String {
        if let Some(example) = &self.example {
            return example.clone();
        }
        let mut out = format!(
            "    mov     rax, {}{:width$}; {}\n",
            self.number,
            "",
            self.name,
            width = 14usize.saturating_sub(self.number.to_string().len())
        );
        for argument in &self.args {
            out.push_str(&format!(
                "    mov     {}, {:width$}; {}\n",
                argument.register,
                "?",
                argument.name,
                width = 18usize.saturating_sub(argument.register.len())
            ));
        }
        out.push_str("    syscall\n");
        out
    }

    /// Whether `query` matches this call's name, number or summary.
    ///
    /// Matching the number as text as well as numerically means typing `60`
    /// finds `exit` and typing `exit` finds it too, without a mode switch.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return true;
        }
        if self.name.to_ascii_lowercase().contains(&query) {
            return true;
        }
        if self.number.to_string() == query {
            return true;
        }
        if let Some(number) = parse_number(&query) {
            if number == self.number {
                return true;
            }
        }
        self.summary.to_ascii_lowercase().contains(&query)
    }

    /// A relevance score for ranking search results; lower sorts first.
    fn score(&self, query: &str) -> u8 {
        let query = query.trim().to_ascii_lowercase();
        let name = self.name.to_ascii_lowercase();
        if query.is_empty() {
            return 4;
        }
        if name == query || self.number.to_string() == query {
            0
        } else if name.starts_with(&query) {
            1
        } else if name.contains(&query) {
            2
        } else {
            3
        }
    }
}

/// The syscall calling convention, as described in the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Abi {
    /// The register holding the call number.
    pub number: String,
    /// The registers holding arguments, in order.
    pub arguments: Vec<String>,
    /// The register holding the return value.
    #[serde(rename = "return")]
    pub return_register: String,
    /// Registers destroyed by the `syscall` instruction.
    pub clobbered: Vec<String>,
    /// The instruction used to enter the kernel.
    pub instruction: String,
    /// A note explaining the convention's one surprise.
    pub note: String,
}

/// The whole database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Database {
    /// The architecture the table describes.
    pub architecture: String,
    /// The calling convention.
    pub abi: Abi,
    /// Where the data came from.
    pub source: String,
    /// Every system call, ordered by number.
    pub syscalls: Vec<Syscall>,
}

impl Database {
    /// Parses the embedded database.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded asset is not valid JSON, which would
    /// mean the build is broken; a test in this module checks it at build
    /// time so it cannot reach a user.
    pub fn load() -> Result<Self, serde_json::Error> {
        serde_json::from_str(DATABASE_JSON)
    }

    /// Looks up a call by number.
    pub fn by_number(&self, number: u32) -> Option<&Syscall> {
        self.syscalls.iter().find(|call| call.number == number)
    }

    /// Looks up a call by exact name, case-insensitively.
    pub fn by_name(&self, name: &str) -> Option<&Syscall> {
        let name = name.trim().to_ascii_lowercase();
        self.syscalls
            .iter()
            .find(|call| call.name.to_ascii_lowercase() == name)
    }

    /// Searches names, numbers and summaries, best match first.
    ///
    /// An empty query returns every call in number order, which is what the
    /// search panel shows before the user types anything.
    pub fn search(&self, query: &str) -> Vec<&Syscall> {
        let mut found: Vec<&Syscall> = self
            .syscalls
            .iter()
            .filter(|call| call.matches(query))
            .collect();
        found.sort_by_key(|call| (call.score(query), call.number));
        found
    }

    /// Every call in a group, such as `file` or `process`.
    pub fn in_group(&self, group: &str) -> Vec<&Syscall> {
        self.syscalls
            .iter()
            .filter(|call| call.group == group)
            .collect()
    }

    /// The calls whose arguments are fully modelled.
    pub fn detailed(&self) -> Vec<&Syscall> {
        self.syscalls.iter().filter(|call| call.detailed).collect()
    }

    /// The number of calls in the table.
    pub fn len(&self) -> usize {
        self.syscalls.len()
    }

    /// Whether the table is empty, which would indicate a broken asset.
    pub fn is_empty(&self) -> bool {
        self.syscalls.is_empty()
    }
}

/// Parses a syscall number written in decimal or hexadecimal.
fn parse_number(text: &str) -> Option<u32> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        text.parse().ok()
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
        assert!(db.len() > 300, "expected the full table, got {}", db.len());
    }

    #[test]
    fn well_known_numbers_are_correct() {
        // These are the numbers a Linux assembly programmer memorises; if any
        // of them is wrong the whole table is suspect.
        let db = database();
        for (name, number) in [
            ("read", 0),
            ("write", 1),
            ("open", 2),
            ("close", 3),
            ("mmap", 9),
            ("exit", 60),
            ("exit_group", 231),
            ("openat", 257),
        ] {
            let call = db.by_name(name).unwrap_or_else(|| panic!("{name} missing"));
            assert_eq!(call.number, number, "{name} has the wrong number");
        }
    }

    #[test]
    fn numbers_are_unique_and_ordered() {
        let db = database();
        let mut previous = None;
        for call in &db.syscalls {
            if let Some(previous) = previous {
                assert!(call.number > previous, "numbers must increase");
            }
            previous = Some(call.number);
        }
    }

    #[test]
    fn names_are_unique() {
        let db = database();
        let mut names: Vec<&str> = db.syscalls.iter().map(|call| call.name.as_str()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate syscall names");
    }

    #[test]
    fn the_abi_matches_the_linux_convention() {
        let db = database();
        assert_eq!(db.abi.number, "rax");
        assert_eq!(db.abi.arguments, ["rdi", "rsi", "rdx", "r10", "r8", "r9"]);
        assert_eq!(db.abi.return_register, "rax");
        assert_eq!(db.abi.clobbered, ["rcx", "r11"]);
        assert_eq!(db.abi.instruction, "syscall");
        assert!(
            db.abi.note.contains("R10"),
            "the R10 trap must be explained"
        );
    }

    #[test]
    fn the_abi_agrees_with_the_register_model() {
        // Two independent sources of the same fact must not drift apart.
        use crate::instruction::registers;
        let db = database();
        let from_registers: Vec<String> = registers::syscall_argument_registers()
            .iter()
            .map(|register| register.name.to_owned())
            .collect();
        assert_eq!(db.abi.arguments, from_registers);
    }

    #[test]
    fn arguments_are_assigned_to_the_right_registers() {
        let db = database();
        let write = db.by_name("write").expect("write");
        assert_eq!(write.arity(), 3);
        assert_eq!(write.args[0].register, "rdi");
        assert_eq!(write.args[1].register, "rsi");
        assert_eq!(write.args[2].register, "rdx");
        assert_eq!(write.args[0].name, "fd");
        assert_eq!(
            write.argument_in("RSI").map(|a| a.name.as_str()),
            Some("buf")
        );
    }

    #[test]
    fn a_six_argument_call_uses_r10_for_the_fourth() {
        // The detail the whole ABI section exists to make obvious.
        let db = database();
        let mmap = db.by_name("mmap").expect("mmap");
        assert_eq!(mmap.arity(), 6);
        let registers: Vec<&str> = mmap.args.iter().map(|a| a.register.as_str()).collect();
        assert_eq!(registers, ["rdi", "rsi", "rdx", "r10", "r8", "r9"]);
    }

    #[test]
    fn detailed_calls_carry_a_return_description_and_errors() {
        let db = database();
        for call in db.detailed() {
            assert!(
                call.returns.is_some(),
                "{} is marked detailed but has no return description",
                call.name
            );
            assert!(
                call.signature.is_some(),
                "{} is marked detailed but has no signature",
                call.name
            );
            assert!(!call.summary.is_empty(), "{} has no summary", call.name);
        }
        assert!(db.detailed().len() >= 50, "too few detailed entries");
    }

    #[test]
    fn undetailed_calls_claim_nothing_they_do_not_know() {
        // The honesty property: an entry without data must not invent any.
        let db = database();
        for call in db.syscalls.iter().filter(|call| !call.detailed) {
            assert!(call.args.is_empty(), "{} invented arguments", call.name);
            assert!(call.returns.is_none(), "{} invented a return", call.name);
            assert!(call.errors.is_empty(), "{} invented errors", call.name);
            assert!(call.example.is_none(), "{} invented an example", call.name);
            assert!(
                call.summary.contains("man 2"),
                "{} should point at the manual page",
                call.name
            );
        }
    }

    #[test]
    fn search_by_name_ranks_the_exact_match_first() {
        let db = database();
        let found = db.search("write");
        assert_eq!(found.first().map(|call| call.name.as_str()), Some("write"));
        assert!(found.len() > 1, "related calls should also be offered");
    }

    #[test]
    fn search_by_number_finds_the_call() {
        let db = database();
        let found = db.search("60");
        assert_eq!(found.first().map(|call| call.name.as_str()), Some("exit"));
    }

    #[test]
    fn search_by_hexadecimal_number_works() {
        let db = database();
        let found = db.search("0x3c");
        assert!(found.iter().any(|call| call.name == "exit"));
    }

    #[test]
    fn search_matches_the_description() {
        let db = database();
        let found = db.search("random");
        assert!(found.iter().any(|call| call.name == "getrandom"));
    }

    #[test]
    fn search_is_case_insensitive() {
        let db = database();
        assert!(db.search("WRITE").iter().any(|call| call.name == "write"));
    }

    #[test]
    fn an_empty_query_lists_everything_in_number_order() {
        let db = database();
        let found = db.search("");
        assert_eq!(found.len(), db.len());
        assert_eq!(found[0].number, 0);
    }

    #[test]
    fn a_query_matching_nothing_returns_nothing() {
        let db = database();
        assert!(db.search("zzzzzznotasyscall").is_empty());
    }

    #[test]
    fn lookup_by_name_is_case_insensitive_and_trims() {
        let db = database();
        assert!(db.by_name("  WRITE ").is_some());
        assert!(db.by_name("no_such_call").is_none());
    }

    #[test]
    fn lookup_by_number_finds_and_misses_correctly() {
        let db = database();
        assert_eq!(db.by_number(1).map(|c| c.name.as_str()), Some("write"));
        assert!(db.by_number(99_999).is_none());
    }

    #[test]
    fn a_hand_written_example_is_used_verbatim() {
        let db = database();
        let write = db.by_name("write").expect("write");
        let template = write.nasm_template();
        assert!(template.contains("mov     rax, 1"));
        assert!(template.contains("syscall"));
    }

    #[test]
    fn a_template_is_generated_for_calls_without_an_example() {
        let db = database();
        let call = db
            .detailed()
            .into_iter()
            .find(|call| call.example.is_none() && call.arity() > 0)
            .expect("a detailed call without an example");
        let template = call.nasm_template();
        assert!(template.contains(&format!("mov     rax, {}", call.number)));
        assert!(template.contains("syscall"));
        for argument in &call.args {
            assert!(
                template.contains(&argument.register),
                "{} missing from the template",
                argument.register
            );
        }
    }

    #[test]
    fn generated_templates_lex_as_valid_assembly_lines() {
        // A template that does not survive the lexer would render wrongly.
        use crate::editor::syntax;
        let db = database();
        for call in db.detailed() {
            for line in call.nasm_template().lines() {
                let covered: usize = syntax::tokenize(line)
                    .iter()
                    .map(crate::editor::Token::len)
                    .sum();
                assert_eq!(covered, line.len(), "{}: {line:?}", call.name);
            }
        }
    }

    #[test]
    fn groups_partition_the_detailed_calls() {
        let db = database();
        assert!(!db.in_group("file").is_empty());
        assert!(!db.in_group("process").is_empty());
        assert!(!db.in_group("memory").is_empty());
        assert!(db.in_group("no-such-group").is_empty());
    }

    #[test]
    fn errors_are_written_in_the_negative_errno_form() {
        // Syscalls return errors as small negative values in RAX, and the
        // text must teach that rather than the C wrapper's -1/errno split.
        let db = database();
        for call in db.detailed() {
            for error in &call.errors {
                assert!(
                    error.starts_with("-E"),
                    "{}: {error:?} should be a negative errno",
                    call.name
                );
            }
        }
    }
}

//! Reading and displaying target memory.
//!
//! Two things live here: a hex-dump model, and a parser for the address
//! expressions a user types into the "go to address" box.
//!
//! # Address expressions must never panic
//!
//! The address box accepts arbitrary text. `rsp-0x20`, `0x4000b0`, `rbp+8`,
//! and also `))))`, `0xzzzz` and a hundred megabytes of pasted nonsense. Every
//! one of those must produce either an address or a clear message — never a
//! panic, and never a silently wrong address. The parser therefore evaluates a
//! deliberately small grammar with checked arithmetic throughout:
//!
//! ```text
//! expression → term (("+" | "-") term)*
//! term       → hex | decimal | register | "(" expression ")"
//! ```
//!
//! Wrapping arithmetic is used for the additions because address arithmetic
//! genuinely wraps on the target, but every parse failure is reported rather
//! than guessed at.

use std::fmt;

use super::registers::RegisterFile;
use crate::instruction::registers;

/// How many bytes a hex-dump row shows.
pub const BYTES_PER_ROW: usize = 16;

/// Errors from evaluating an address expression.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AddressError {
    /// The expression was blank.
    #[error("enter an address, for example 0x4000b0 or rsp-0x20")]
    Empty,
    /// A token was not a number, a register or an operator.
    #[error("'{token}' is not a number or a register name")]
    UnknownToken {
        /// The offending text.
        token: String,
    },
    /// A number was written in a form that could not be read.
    #[error("'{token}' is not a valid number")]
    BadNumber {
        /// The offending text.
        token: String,
    },
    /// A register was named but its value is not known.
    #[error("the value of {register} is not known; the program must be paused")]
    RegisterUnavailable {
        /// The register that was named.
        register: String,
    },
    /// The parentheses did not balance.
    #[error("unbalanced parentheses")]
    UnbalancedParentheses,
    /// The expression ended where a value was expected.
    #[error("the expression is incomplete")]
    Incomplete,
    /// Text remained after a complete expression.
    #[error("unexpected '{token}' after the expression")]
    TrailingInput {
        /// The unconsumed text.
        token: String,
    },
}

/// Evaluates an address expression against the current register values.
///
/// # Errors
///
/// Returns an [`AddressError`] describing what was wrong with the input. This
/// function never panics, whatever it is given.
pub fn evaluate_address(text: &str, registers: &RegisterFile) -> Result<u64, AddressError> {
    let tokens = tokenize(text)?;
    if tokens.is_empty() {
        return Err(AddressError::Empty);
    }
    let mut parser = ExpressionParser {
        tokens: &tokens,
        position: 0,
        registers,
    };
    let value = parser.expression()?;
    if parser.position < tokens.len() {
        return Err(AddressError::TrailingInput {
            token: parser.tokens[parser.position].text().to_owned(),
        });
    }
    Ok(value)
}

/// A token in an address expression.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Number(u64),
    Register(String),
    Plus,
    Minus,
    Open,
    Close,
}

impl Token {
    fn text(&self) -> &str {
        match self {
            Token::Number(_) => "number",
            Token::Register(name) => name,
            Token::Plus => "+",
            Token::Minus => "-",
            Token::Open => "(",
            Token::Close => ")",
        }
    }
}

/// Splits an address expression into tokens.
fn tokenize(text: &str) -> Result<Vec<Token>, AddressError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = text.trim().chars().collect();
    let mut index = 0usize;

    while index < chars.len() {
        let ch = chars[index];
        match ch {
            ' ' | '\t' => index += 1,
            '+' => {
                tokens.push(Token::Plus);
                index += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                index += 1;
            }
            '(' => {
                tokens.push(Token::Open);
                index += 1;
            }
            ')' => {
                tokens.push(Token::Close);
                index += 1;
            }
            '$' | '%' => index += 1, // sigils some users type before registers
            _ if ch.is_ascii_alphanumeric() || ch == '_' => {
                let start = index;
                while index < chars.len()
                    && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
                {
                    index += 1;
                }
                let word: String = chars[start..index].iter().collect();
                tokens.push(classify(&word)?);
            }
            other => {
                return Err(AddressError::UnknownToken {
                    token: other.to_string(),
                })
            }
        }
    }

    Ok(tokens)
}

/// Decides whether a word is a number or a register name.
fn classify(word: &str) -> Result<Token, AddressError> {
    let lowered = word.to_ascii_lowercase();

    if let Some(hex) = lowered.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16)
            .map(Token::Number)
            .map_err(|_| AddressError::BadNumber {
                token: word.to_owned(),
            });
    }
    if let Some(binary) = lowered.strip_prefix("0b") {
        return u64::from_str_radix(binary, 2)
            .map(Token::Number)
            .map_err(|_| AddressError::BadNumber {
                token: word.to_owned(),
            });
    }
    if registers::is_register(&lowered) {
        return Ok(Token::Register(lowered));
    }
    if lowered.chars().all(|ch| ch.is_ascii_digit()) {
        return lowered
            .parse::<u64>()
            .map(Token::Number)
            .map_err(|_| AddressError::BadNumber {
                token: word.to_owned(),
            });
    }
    // A bare hexadecimal value without the 0x prefix, as GDB accepts.
    if lowered.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return u64::from_str_radix(&lowered, 16)
            .map(Token::Number)
            .map_err(|_| AddressError::BadNumber {
                token: word.to_owned(),
            });
    }

    Err(AddressError::UnknownToken {
        token: word.to_owned(),
    })
}

/// A recursive-descent parser over the token list.
struct ExpressionParser<'a> {
    tokens: &'a [Token],
    position: usize,
    registers: &'a RegisterFile,
}

impl ExpressionParser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn expression(&mut self) -> Result<u64, AddressError> {
        let mut value = self.term()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.position += 1;
                    // Address arithmetic wraps on the target, so wrapping here
                    // is correct rather than an overflow to report.
                    value = value.wrapping_add(self.term()?);
                }
                Some(Token::Minus) => {
                    self.position += 1;
                    value = value.wrapping_sub(self.term()?);
                }
                _ => break,
            }
        }
        Ok(value)
    }

    fn term(&mut self) -> Result<u64, AddressError> {
        match self.peek().cloned() {
            Some(Token::Number(value)) => {
                self.position += 1;
                Ok(value)
            }
            Some(Token::Register(name)) => {
                self.position += 1;
                self.registers
                    .value_of(&name)
                    .ok_or(AddressError::RegisterUnavailable {
                        register: name.to_ascii_uppercase(),
                    })
            }
            Some(Token::Minus) => {
                // Unary minus, so `-8` and `rsp + -8` both work.
                self.position += 1;
                Ok(0u64.wrapping_sub(self.term()?))
            }
            Some(Token::Open) => {
                self.position += 1;
                let value = self.expression()?;
                match self.peek() {
                    Some(Token::Close) => {
                        self.position += 1;
                        Ok(value)
                    }
                    _ => Err(AddressError::UnbalancedParentheses),
                }
            }
            Some(Token::Close) => Err(AddressError::UnbalancedParentheses),
            Some(Token::Plus) => {
                self.position += 1;
                self.term()
            }
            None => Err(AddressError::Incomplete),
        }
    }
}

/// A block of memory read from the target.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryBlock {
    /// The address the block starts at.
    pub address: u64,
    /// The bytes, in target order.
    pub bytes: Vec<u8>,
}

impl MemoryBlock {
    /// Creates a block.
    pub fn new(address: u64, bytes: Vec<u8>) -> Self {
        Self { address, bytes }
    }

    /// Whether the block holds no bytes.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The number of bytes held.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// The address just past the last byte.
    pub fn end_address(&self) -> u64 {
        self.address.wrapping_add(self.bytes.len() as u64)
    }

    /// Whether `address` falls inside this block.
    pub fn contains(&self, address: u64) -> bool {
        address >= self.address && address < self.end_address()
    }

    /// The byte at `address`, if the block covers it.
    pub fn byte_at(&self, address: u64) -> Option<u8> {
        if !self.contains(address) {
            return None;
        }
        let offset = usize::try_from(address - self.address).ok()?;
        self.bytes.get(offset).copied()
    }

    /// Reads `size` bytes at `address` as a little-endian integer.
    ///
    /// Returns `None` when the block does not hold every byte, rather than
    /// padding with zeros, which would silently show a wrong value.
    pub fn read_integer(&self, address: u64, size: usize) -> Option<u64> {
        if size == 0 || size > 8 {
            return None;
        }
        let mut value = 0u64;
        for index in 0..size {
            let byte = self.byte_at(address.wrapping_add(index as u64))?;
            value |= u64::from(byte) << (index * 8);
        }
        Some(value)
    }

    /// The rows of a hex dump, each covering [`BYTES_PER_ROW`] bytes.
    pub fn rows(&self) -> Vec<MemoryRow> {
        self.bytes
            .chunks(BYTES_PER_ROW)
            .enumerate()
            .map(|(index, chunk)| MemoryRow {
                address: self.address.wrapping_add((index * BYTES_PER_ROW) as u64),
                bytes: chunk.to_vec(),
            })
            .collect()
    }
}

/// One row of a hex dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRow {
    /// The address of the first byte in the row.
    pub address: u64,
    /// The bytes in the row, at most [`BYTES_PER_ROW`] of them.
    pub bytes: Vec<u8>,
}

impl MemoryRow {
    /// The hexadecimal column, padded so short rows still line up.
    pub fn hex(&self) -> String {
        let mut parts: Vec<String> = self
            .bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        while parts.len() < BYTES_PER_ROW {
            parts.push("  ".to_owned());
        }
        parts.join(" ")
    }

    /// The ASCII column, with non-printable bytes shown as dots.
    pub fn ascii(&self) -> String {
        self.bytes
            .iter()
            .map(|byte| {
                if byte.is_ascii_graphic() || *byte == b' ' {
                    *byte as char
                } else {
                    '.'
                }
            })
            .collect()
    }

    /// The address column.
    pub fn address_text(&self) -> String {
        format!("{:016x}", self.address)
    }
}

impl fmt::Display for MemoryRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}  {}  |{}|",
            self.address_text(),
            self.hex(),
            self.ascii()
        )
    }
}

/// Decodes the `memory` payload of a `-data-read-memory-bytes` reply.
///
/// The reply carries the contents as a hexadecimal string; an odd-length or
/// non-hexadecimal string yields `None` rather than a partially decoded block.
pub fn parse_memory_reply(value: &crate::debugger::mi::Value) -> Option<MemoryBlock> {
    let entries = value.as_list()?;
    let first = entries.first()?;
    let address = first.get_address("begin")?;
    let contents = first.get_str("contents")?;

    if contents.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(contents.len() / 2);
    for pair in contents.as_bytes().chunks(2) {
        let text = std::str::from_utf8(pair).ok()?;
        bytes.push(u8::from_str_radix(text, 16).ok()?);
    }
    Some(MemoryBlock::new(address, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn registers_with(pairs: &[(&str, u64)]) -> RegisterFile {
        let mut file = RegisterFile::new();
        let map: BTreeMap<String, u64> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), *value))
            .collect();
        file.update(map);
        file
    }

    fn empty() -> RegisterFile {
        RegisterFile::new()
    }

    #[test]
    fn a_hexadecimal_address_is_accepted() {
        assert_eq!(evaluate_address("0x4000b0", &empty()), Ok(0x0040_00b0));
        assert_eq!(evaluate_address("  0X4000B0  ", &empty()), Ok(0x0040_00b0));
    }

    #[test]
    fn a_bare_hexadecimal_address_is_accepted() {
        // GDB accepts this form, so users expect it to work.
        assert_eq!(evaluate_address("4000b0", &empty()), Ok(0x0040_00b0));
    }

    #[test]
    fn a_decimal_address_is_accepted() {
        assert_eq!(evaluate_address("4194480", &empty()), Ok(4_194_480));
    }

    #[test]
    fn a_binary_literal_is_accepted() {
        assert_eq!(evaluate_address("0b1010", &empty()), Ok(10));
    }

    #[test]
    fn a_register_name_resolves_to_its_value() {
        let registers = registers_with(&[("rsp", 0x7fff_ffff_e000)]);
        assert_eq!(evaluate_address("rsp", &registers), Ok(0x7fff_ffff_e000));
        assert_eq!(evaluate_address("RSP", &registers), Ok(0x7fff_ffff_e000));
        assert_eq!(evaluate_address("$rsp", &registers), Ok(0x7fff_ffff_e000));
        assert_eq!(evaluate_address("%rsp", &registers), Ok(0x7fff_ffff_e000));
    }

    #[test]
    fn arithmetic_on_a_register_works() {
        let registers = registers_with(&[("rsp", 0x1000), ("rbp", 0x2000)]);
        assert_eq!(evaluate_address("rsp+8", &registers), Ok(0x1008));
        assert_eq!(evaluate_address("rsp - 0x20", &registers), Ok(0x0fe0));
        assert_eq!(evaluate_address("rbp - rsp", &registers), Ok(0x1000));
        assert_eq!(evaluate_address("rsp + 8 - 4", &registers), Ok(0x1004));
    }

    #[test]
    fn parentheses_group_correctly() {
        let registers = registers_with(&[("rsp", 0x1000)]);
        assert_eq!(evaluate_address("(rsp + 8) - 4", &registers), Ok(0x1004));
        assert_eq!(evaluate_address("rsp - (8 - 4)", &registers), Ok(0x0ffc));
    }

    #[test]
    fn a_narrow_register_alias_resolves() {
        let registers = registers_with(&[("rax", 0x1122_3344)]);
        assert_eq!(evaluate_address("eax", &registers), Ok(0x1122_3344));
        assert_eq!(evaluate_address("al", &registers), Ok(0x44));
    }

    #[test]
    fn address_arithmetic_wraps_rather_than_overflowing() {
        // Subtracting past zero is meaningful on a wrapping address space and
        // must not panic in a debug build.
        assert_eq!(evaluate_address("0 - 1", &empty()), Ok(u64::MAX));
        assert_eq!(evaluate_address("-8", &empty()), Ok(u64::MAX - 7));
    }

    #[test]
    fn an_empty_expression_is_reported_not_guessed() {
        assert_eq!(evaluate_address("", &empty()), Err(AddressError::Empty));
        assert_eq!(evaluate_address("   ", &empty()), Err(AddressError::Empty));
    }

    #[test]
    fn an_unknown_name_is_reported() {
        let error = evaluate_address("nonsense", &empty()).expect_err("must fail");
        assert!(matches!(error, AddressError::UnknownToken { .. }));
        assert!(error.to_string().contains("nonsense"));
    }

    #[test]
    fn a_register_with_no_value_is_reported_clearly() {
        let error = evaluate_address("rsp", &empty()).expect_err("must fail");
        assert!(matches!(error, AddressError::RegisterUnavailable { .. }));
        assert!(error.to_string().contains("paused"));
    }

    #[test]
    fn malformed_input_never_panics() {
        // The property that matters most for a free-text box.
        let registers = registers_with(&[("rsp", 0x1000)]);
        let inputs = [
            "))))",
            "((((",
            "(rsp",
            "rsp)",
            "0xzzzz",
            "0x",
            "+",
            "-",
            "rsp +",
            "rsp + + +",
            "* & ^",
            "0x1 0x2",
            "rsp rsp",
            "!!!",
            "\u{1F600}",
            &"9".repeat(500),
            &"(".repeat(200),
        ];
        for input in inputs {
            // Any outcome is acceptable except a panic.
            let _ = evaluate_address(input, &registers);
        }
    }

    #[test]
    fn specific_malformed_inputs_give_specific_errors() {
        let registers = registers_with(&[("rsp", 0x1000)]);
        assert_eq!(
            evaluate_address("(rsp", &registers),
            Err(AddressError::UnbalancedParentheses)
        );
        assert_eq!(
            evaluate_address("rsp +", &registers),
            Err(AddressError::Incomplete)
        );
        assert!(matches!(
            evaluate_address("0x1 0x2", &registers),
            Err(AddressError::TrailingInput { .. })
        ));
        // "0xzzzz" looks like a number and fails as one, which is a more
        // useful message than "unknown token".
        assert!(matches!(
            evaluate_address("0xzzzz", &registers),
            Err(AddressError::BadNumber { .. })
        ));
        assert!(matches!(
            evaluate_address("nonsense", &registers),
            Err(AddressError::UnknownToken { .. })
        ));
    }

    #[test]
    fn an_enormous_number_is_reported_rather_than_wrapping_silently() {
        let error =
            evaluate_address(&format!("0x{}", "f".repeat(40)), &empty()).expect_err("must fail");
        assert!(matches!(error, AddressError::BadNumber { .. }));
    }

    #[test]
    fn a_hex_dump_row_renders_all_three_columns() {
        let block = MemoryBlock::new(0x4000, b"Hello, world!\n\0\0".to_vec());
        let rows = block.rows();
        assert_eq!(rows.len(), 1);

        let row = &rows[0];
        assert_eq!(row.address_text(), "0000000000004000");
        assert!(row.hex().starts_with("48 65 6c 6c 6f"));
        assert_eq!(row.ascii(), "Hello, world!...");
    }

    #[test]
    fn a_short_final_row_stays_aligned() {
        // Without padding the ASCII column of the last row would shift left.
        let block = MemoryBlock::new(0x4000, vec![0xaa, 0xbb, 0xcc]);
        let row = &block.rows()[0];
        let full = MemoryRow {
            address: 0,
            bytes: vec![0; BYTES_PER_ROW],
        };
        assert_eq!(row.hex().len(), full.hex().len());
        assert_eq!(row.ascii(), "...");
    }

    #[test]
    fn rows_are_split_at_the_row_width() {
        let block = MemoryBlock::new(0x4000, vec![0; 40]);
        let rows = block.rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].address, 0x4000);
        assert_eq!(rows[1].address, 0x4010);
        assert_eq!(rows[2].bytes.len(), 8);
    }

    #[test]
    fn integers_are_read_little_endian() {
        // x86 is little-endian, so the first byte is the least significant.
        let block = MemoryBlock::new(0x4000, vec![0x78, 0x56, 0x34, 0x12, 0, 0, 0, 0]);
        assert_eq!(block.read_integer(0x4000, 1), Some(0x78));
        assert_eq!(block.read_integer(0x4000, 2), Some(0x5678));
        assert_eq!(block.read_integer(0x4000, 4), Some(0x1234_5678));
        assert_eq!(block.read_integer(0x4000, 8), Some(0x0000_0000_1234_5678));
    }

    #[test]
    fn reading_past_the_block_returns_nothing_rather_than_padding() {
        let block = MemoryBlock::new(0x4000, vec![1, 2, 3]);
        assert_eq!(block.read_integer(0x4000, 4), None);
        assert_eq!(block.read_integer(0x5000, 1), None);
        assert_eq!(block.byte_at(0x4003), None);
        assert_eq!(block.read_integer(0x4000, 0), None);
        assert_eq!(block.read_integer(0x4000, 9), None);
    }

    #[test]
    fn an_empty_block_reports_itself_as_empty() {
        let block = MemoryBlock::default();
        assert!(block.is_empty());
        assert_eq!(block.len(), 0);
        assert!(block.rows().is_empty());
        assert!(!block.contains(0));
    }

    #[test]
    fn a_gdb_memory_reply_is_decoded() {
        use crate::debugger::mi::Value;
        let value = Value::List(vec![Value::Tuple(vec![
            ("begin".to_owned(), Value::String("0x4000b0".to_owned())),
            ("offset".to_owned(), Value::String("0x0".to_owned())),
            ("end".to_owned(), Value::String("0x4000b4".to_owned())),
            ("contents".to_owned(), Value::String("48c7c001".to_owned())),
        ])]);

        let block = parse_memory_reply(&value).expect("decode");
        assert_eq!(block.address, 0x0040_00b0);
        assert_eq!(block.bytes, [0x48, 0xc7, 0xc0, 0x01]);
    }

    #[test]
    fn a_malformed_memory_reply_is_rejected_rather_than_half_decoded() {
        use crate::debugger::mi::Value;
        for contents in ["48c7c", "zzzz", ""] {
            let value = Value::List(vec![Value::Tuple(vec![
                ("begin".to_owned(), Value::String("0x1000".to_owned())),
                ("contents".to_owned(), Value::String(contents.to_owned())),
            ])]);
            let decoded = parse_memory_reply(&value);
            if contents.is_empty() {
                assert_eq!(decoded.map(|block| block.len()), Some(0));
            } else {
                assert!(decoded.is_none(), "{contents:?} should not decode");
            }
        }

        assert!(parse_memory_reply(&Value::String("x".to_owned())).is_none());
        assert!(parse_memory_reply(&Value::List(Vec::new())).is_none());
    }
}

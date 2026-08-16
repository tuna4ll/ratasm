//! Register values read from the debugger, and how they are displayed.
//!
//! This module holds *values*; the names, widths and ABI roles come from
//! [`crate::instruction::registers`]. Keeping them apart means the ABI
//! knowledge is testable without a debugger and the value handling is testable
//! without an ABI.
//!
//! # Change tracking
//!
//! The panel highlights registers that changed at the last step, which is the
//! single most useful thing it can do: it turns "what did that instruction
//! do?" into something visible. The file keeps the previous snapshot and
//! compares, rather than trusting GDB's `-data-list-changed-registers`, which
//! reports changes since the last time *it* was asked and so gives wrong
//! answers if any other command intervenes.

use std::collections::BTreeMap;

use crate::debugger::mi::Value;
use crate::instruction::registers::{self, Register, RegisterWidth};
use crate::instruction::Flags;

/// How a register value is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Format {
    /// Hexadecimal, zero-padded to the register's width.
    #[default]
    Hex,
    /// Unsigned decimal.
    Decimal,
    /// Signed decimal, two's complement.
    Signed,
    /// Binary, grouped in bytes.
    Binary,
    /// The bytes as ASCII, with non-printable bytes shown as dots.
    Ascii,
}

impl Format {
    /// Every format, in cycle order.
    pub const ALL: [Format; 5] = [
        Format::Hex,
        Format::Decimal,
        Format::Signed,
        Format::Binary,
        Format::Ascii,
    ];

    /// A short label for the status bar.
    pub const fn label(self) -> &'static str {
        match self {
            Format::Hex => "hex",
            Format::Decimal => "dec",
            Format::Signed => "signed",
            Format::Binary => "bin",
            Format::Ascii => "ascii",
        }
    }

    /// The next format, wrapping around.
    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

/// Renders `value` at `width` in `format`.
///
/// The value is masked to the width first, so displaying `AL` of a register
/// holding `0x1122334455667788` shows `0x88` rather than the whole thing.
pub fn format_value(value: u64, width: RegisterWidth, format: Format) -> String {
    let value = value & width.mask();
    let digits = (width.bits() / 4) as usize;

    match format {
        Format::Hex => format!("0x{value:0digits$x}"),
        Format::Decimal => value.to_string(),
        Format::Signed => signed_value(value, width).to_string(),
        Format::Binary => {
            let bits = width.bits() as usize;
            let text = format!("{value:0bits$b}");
            // Group in bytes so long values stay readable.
            text.as_bytes()
                .chunks(8)
                .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
                .collect::<Vec<_>>()
                .join(" ")
        }
        Format::Ascii => ascii_of(value, width),
    }
}

/// Interprets `value` as a signed integer of `width` bits.
pub fn signed_value(value: u64, width: RegisterWidth) -> i64 {
    let value = value & width.mask();
    match width {
        RegisterWidth::Byte => value as u8 as i8 as i64,
        RegisterWidth::Word => value as u16 as i16 as i64,
        RegisterWidth::Dword => value as u32 as i32 as i64,
        RegisterWidth::Qword => value as i64,
    }
}

/// Renders the bytes of `value` as ASCII, least significant byte first.
///
/// Non-printable bytes become `.` rather than being escaped, so the column
/// stays a fixed width and lines up with the memory view.
pub fn ascii_of(value: u64, width: RegisterWidth) -> String {
    let value = value & width.mask();
    (0..width.bytes())
        .map(|index| {
            let byte = ((value >> (index * 8)) & 0xff) as u8;
            if byte.is_ascii_graphic() || byte == b' ' {
                byte as char
            } else {
                '.'
            }
        })
        .collect()
}

/// One register's current and previous value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterEntry {
    /// The architectural register.
    pub register: Register,
    /// The current value.
    pub value: u64,
    /// The value at the previous stop, if there was one.
    pub previous: Option<u64>,
}

impl RegisterEntry {
    /// Whether the value changed since the previous stop.
    ///
    /// A register with no previous value is not reported as changed: at the
    /// first stop everything would light up, which conveys nothing.
    pub fn has_changed(&self) -> bool {
        self.previous.is_some_and(|previous| previous != self.value)
    }

    /// The value narrowed to one of the register's aliases.
    pub fn alias_value(&self, alias: &str) -> Option<u64> {
        self.register.extract(self.value, alias)
    }

    /// Renders the value in `format` at the register's full width.
    pub fn display(&self, format: Format) -> String {
        format_value(self.value, RegisterWidth::Qword, format)
    }

    /// Every width view of this register, widest first.
    ///
    /// This is what makes the `rax` / `eax` / `ax` / `al` / `ah` relationship
    /// visible rather than something the user has to work out.
    pub fn width_views(&self) -> Vec<(String, u64, RegisterWidth)> {
        let mut views = Vec::new();
        for width in [
            RegisterWidth::Qword,
            RegisterWidth::Dword,
            RegisterWidth::Word,
            RegisterWidth::Byte,
        ] {
            if let Some(alias) = self.register.alias(width) {
                if let Some(value) = self.register.extract(self.value, alias) {
                    views.push((alias.to_ascii_uppercase(), value, width));
                }
            }
        }
        if let Some(high) = self.register.high_byte {
            if let Some(value) = self.register.extract(self.value, high) {
                views.push((high.to_ascii_uppercase(), value, RegisterWidth::Byte));
            }
        }
        views
    }
}

/// The register values at a debugger stop.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegisterFile {
    values: BTreeMap<String, u64>,
    previous: BTreeMap<String, u64>,
}

impl RegisterFile {
    /// Creates an empty file.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the values, moving the current ones to the previous snapshot.
    pub fn update(&mut self, values: BTreeMap<String, u64>) {
        self.previous = std::mem::take(&mut self.values);
        self.values = values;
    }

    /// Forgets all values, as when a session ends.
    pub fn clear(&mut self) {
        self.values.clear();
        self.previous.clear();
    }

    /// Whether any values have been read.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The raw value of a register by any alias.
    pub fn value_of(&self, name: &str) -> Option<u64> {
        let register = registers::lookup(name)?;
        let full = self.values.get(register.name).copied()?;
        register.extract(full, name.trim().trim_start_matches('%'))
    }

    /// The entry for a register, if its value is known.
    pub fn entry(&self, name: &str) -> Option<RegisterEntry> {
        let register = registers::lookup(name)?;
        let value = self.values.get(register.name).copied()?;
        Some(RegisterEntry {
            register,
            value,
            previous: self.previous.get(register.name).copied(),
        })
    }

    /// Every known register, in panel order.
    pub fn entries(&self) -> Vec<RegisterEntry> {
        registers::all()
            .into_iter()
            .filter_map(|register| self.entry(register.name))
            .collect()
    }

    /// The registers whose values changed at the last step.
    pub fn changed(&self) -> Vec<RegisterEntry> {
        self.entries()
            .into_iter()
            .filter(RegisterEntry::has_changed)
            .collect()
    }

    /// The instruction pointer.
    pub fn rip(&self) -> Option<u64> {
        self.value_of("rip")
    }

    /// The stack pointer.
    pub fn rsp(&self) -> Option<u64> {
        self.value_of("rsp")
    }

    /// The frame pointer.
    pub fn rbp(&self) -> Option<u64> {
        self.value_of("rbp")
    }

    /// The flags register, decoded.
    pub fn flags(&self) -> Option<Flags> {
        self.value_of("rflags")
            .or_else(|| self.value_of("eflags"))
            .map(Flags::from_bits)
    }

    /// The flags at the previous stop, for change highlighting.
    pub fn previous_flags(&self) -> Option<Flags> {
        self.previous.get("rflags").copied().map(Flags::from_bits)
    }
}

/// Builds a register map from GDB's `-data-list-register-names` and
/// `-data-list-register-values` replies.
///
/// GDB identifies registers by position in the names list, so the two replies
/// must be correlated by index. Names ratasm does not model — the vector and
/// segment registers — are skipped rather than stored, keeping the file to
/// what the panel can actually explain.
pub fn parse_register_values(names: &Value, values: &Value) -> BTreeMap<String, u64> {
    let mut table = BTreeMap::new();

    let Some(names) = names.as_list() else {
        return table;
    };
    let Some(values) = values.as_list() else {
        return table;
    };

    for entry in values {
        let Some(number) = entry.get_int("number") else {
            continue;
        };
        let Ok(index) = usize::try_from(number) else {
            continue;
        };
        let Some(name) = names.get(index).and_then(Value::as_str) else {
            continue;
        };
        let Some(register) = registers::lookup(name) else {
            continue;
        };
        let Some(raw) = entry.get_str("value") else {
            continue;
        };
        // GDB writes register values as hexadecimal, but a value it cannot
        // render numerically (a vector register, say) comes back as a tuple
        // string; skipping those is better than storing a wrong number.
        if let Some(value) = crate::debugger::mi::parse_address(raw) {
            table.insert(register.name.to_owned(), value);
        }
    }

    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_with(pairs: &[(&str, u64)]) -> RegisterFile {
        let mut file = RegisterFile::new();
        file.update(
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), *value))
                .collect(),
        );
        file
    }

    #[test]
    fn hexadecimal_is_padded_to_the_register_width() {
        assert_eq!(
            format_value(0x3c, RegisterWidth::Qword, Format::Hex),
            "0x000000000000003c"
        );
        assert_eq!(format_value(0x3c, RegisterWidth::Byte, Format::Hex), "0x3c");
        assert_eq!(
            format_value(0x1234, RegisterWidth::Word, Format::Hex),
            "0x1234"
        );
    }

    #[test]
    fn a_narrow_view_masks_the_wider_value() {
        let value = 0x1122_3344_5566_7788u64;
        assert_eq!(
            format_value(value, RegisterWidth::Byte, Format::Hex),
            "0x88"
        );
        assert_eq!(
            format_value(value, RegisterWidth::Dword, Format::Hex),
            "0x55667788"
        );
    }

    #[test]
    fn signed_interpretation_respects_the_width() {
        // The same bits mean different numbers at different widths.
        assert_eq!(signed_value(0xff, RegisterWidth::Byte), -1);
        assert_eq!(signed_value(0xff, RegisterWidth::Word), 255);
        assert_eq!(signed_value(u64::MAX, RegisterWidth::Qword), -1);
        assert_eq!(
            signed_value(0x8000_0000, RegisterWidth::Dword),
            -2_147_483_648
        );
    }

    #[test]
    fn decimal_and_signed_differ_for_negative_values() {
        assert_eq!(
            format_value(u64::MAX, RegisterWidth::Qword, Format::Decimal),
            "18446744073709551615"
        );
        assert_eq!(
            format_value(u64::MAX, RegisterWidth::Qword, Format::Signed),
            "-1"
        );
    }

    #[test]
    fn binary_is_grouped_into_bytes() {
        assert_eq!(
            format_value(0b1010_0101, RegisterWidth::Byte, Format::Binary),
            "10100101"
        );
        assert_eq!(
            format_value(0xff00, RegisterWidth::Word, Format::Binary),
            "11111111 00000000"
        );
    }

    #[test]
    fn ascii_shows_printable_bytes_and_dots_for_the_rest() {
        // "ABCD" little-endian is 0x44434241.
        assert_eq!(
            format_value(0x4443_4241, RegisterWidth::Dword, Format::Ascii),
            "ABCD"
        );
        assert_eq!(format_value(0x00, RegisterWidth::Byte, Format::Ascii), ".");
    }

    #[test]
    fn formats_cycle_through_every_variant() {
        let mut format = Format::Hex;
        for _ in 0..Format::ALL.len() {
            format = format.next();
        }
        assert_eq!(format, Format::Hex);
    }

    #[test]
    fn values_are_read_back_through_any_alias() {
        let file = file_with(&[("rax", 0x1122_3344_5566_7788)]);
        assert_eq!(file.value_of("rax"), Some(0x1122_3344_5566_7788));
        assert_eq!(file.value_of("eax"), Some(0x5566_7788));
        assert_eq!(file.value_of("ax"), Some(0x7788));
        assert_eq!(file.value_of("al"), Some(0x88));
        assert_eq!(file.value_of("ah"), Some(0x77));
    }

    #[test]
    fn an_unknown_register_reads_as_nothing() {
        let file = file_with(&[("rax", 1)]);
        assert_eq!(file.value_of("zmm0"), None);
        assert_eq!(file.value_of("rbx"), None, "not read yet");
        assert!(file.entry("nonsense").is_none());
    }

    #[test]
    fn the_first_stop_reports_nothing_as_changed() {
        // Otherwise every register would light up and mean nothing.
        let file = file_with(&[("rax", 1), ("rbx", 2)]);
        assert!(file.changed().is_empty());
    }

    #[test]
    fn only_registers_that_actually_changed_are_reported() {
        let mut file = file_with(&[("rax", 1), ("rbx", 2)]);
        file.update(
            [("rax".to_owned(), 99u64), ("rbx".to_owned(), 2u64)]
                .into_iter()
                .collect(),
        );
        let changed: Vec<&str> = file.changed().iter().map(|e| e.register.name).collect();
        assert_eq!(changed, ["rax"]);
    }

    #[test]
    fn width_views_expose_the_alias_relationship() {
        let file = file_with(&[("rax", 0x1122_3344_5566_7788)]);
        let entry = file.entry("rax").expect("rax");
        let views = entry.width_views();
        let named: Vec<(&str, u64)> = views
            .iter()
            .map(|(name, value, _)| (name.as_str(), *value))
            .collect();
        assert_eq!(
            named,
            [
                ("RAX", 0x1122_3344_5566_7788),
                ("EAX", 0x5566_7788),
                ("AX", 0x7788),
                ("AL", 0x88),
                ("AH", 0x77),
            ]
        );
    }

    #[test]
    fn a_register_without_a_high_byte_has_no_high_byte_view() {
        let file = file_with(&[("rsi", 0x1122)]);
        let entry = file.entry("rsi").expect("rsi");
        let views = entry.width_views();
        let names: Vec<&str> = views.iter().map(|(name, _, _)| name.as_str()).collect();
        assert_eq!(names, ["RSI", "ESI", "SI", "SIL"]);
    }

    #[test]
    fn the_special_registers_are_exposed_directly() {
        let file = file_with(&[
            ("rip", 0x40_00b0),
            ("rsp", 0x7fff_ffff_e000),
            ("rbp", 0x7fff_ffff_e100),
            ("rflags", 0x246),
        ]);
        assert_eq!(file.rip(), Some(0x40_00b0));
        assert_eq!(file.rsp(), Some(0x7fff_ffff_e000));
        assert_eq!(file.rbp(), Some(0x7fff_ffff_e100));

        let flags = file.flags().expect("flags");
        assert_eq!(flags.summary(), "PF ZF IF");
    }

    #[test]
    fn clearing_forgets_everything() {
        let mut file = file_with(&[("rax", 1)]);
        file.clear();
        assert!(file.is_empty());
        assert!(file.entries().is_empty());
    }

    #[test]
    fn entries_are_returned_in_panel_order() {
        let file = file_with(&[("rax", 1), ("rip", 2), ("r15", 3)]);
        let names: Vec<&str> = file.entries().iter().map(|e| e.register.name).collect();
        assert_eq!(names, ["rax", "r15", "rip"]);
    }

    #[test]
    fn gdb_replies_are_correlated_by_index() {
        // GDB identifies registers only by their position in the names list.
        let names = Value::List(vec![
            Value::String("rax".to_owned()),
            Value::String("rbx".to_owned()),
            Value::String("rcx".to_owned()),
        ]);
        let values = Value::List(vec![
            Value::Tuple(vec![
                ("number".to_owned(), Value::String("2".to_owned())),
                ("value".to_owned(), Value::String("0xcccc".to_owned())),
            ]),
            Value::Tuple(vec![
                ("number".to_owned(), Value::String("0".to_owned())),
                ("value".to_owned(), Value::String("0x3c".to_owned())),
            ]),
        ]);

        let table = parse_register_values(&names, &values);
        assert_eq!(table.get("rax"), Some(&0x3c));
        assert_eq!(table.get("rcx"), Some(&0xcccc));
        assert_eq!(table.get("rbx"), None, "no value was reported for rbx");
    }

    #[test]
    fn unmodelled_registers_are_skipped_rather_than_stored() {
        let names = Value::List(vec![
            Value::String("rax".to_owned()),
            Value::String("ymm0".to_owned()),
        ]);
        let values = Value::List(vec![
            Value::Tuple(vec![
                ("number".to_owned(), Value::String("0".to_owned())),
                ("value".to_owned(), Value::String("0x1".to_owned())),
            ]),
            Value::Tuple(vec![
                ("number".to_owned(), Value::String("1".to_owned())),
                (
                    "value".to_owned(),
                    Value::String("{v8_int32 = {0,0}}".to_owned()),
                ),
            ]),
        ]);

        let table = parse_register_values(&names, &values);
        assert_eq!(table.len(), 1);
        assert_eq!(table.get("rax"), Some(&1));
    }

    #[test]
    fn malformed_gdb_replies_produce_an_empty_table_rather_than_a_panic() {
        let empty = Value::String("not a list".to_owned());
        assert!(parse_register_values(&empty, &empty).is_empty());

        let names = Value::List(vec![Value::String("rax".to_owned())]);
        let bad_values = Value::List(vec![Value::Tuple(vec![(
            "number".to_owned(),
            Value::String("99".to_owned()),
        )])]);
        assert!(parse_register_values(&names, &bad_values).is_empty());
    }
}

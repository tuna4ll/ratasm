//! Decoding machine code back into instructions.
//!
//! Disassembly comes from `iced-x86` rather than from GDB. Two reasons: it
//! works without a debug session, so a built program can be inspected before
//! it is ever run; and it gives structured access to each instruction —
//! length, mnemonic, flow control, operand kinds — where GDB's MI reply is
//! pre-formatted text that would have to be parsed back.
//!
//! Intel syntax is the default because ratasm targets NASM, where reading
//! disassembly in AT&T operand order would mean mentally reversing every
//! instruction. AT&T is available through [`Syntax`] for users who want it.

use iced_x86::{Decoder, DecoderOptions, Formatter, GasFormatter, Instruction, IntelFormatter};

pub mod elf;

pub use elf::{ElfError, ElfImage, Symbol as ElfSymbol};

/// Which operand order to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Syntax {
    /// Intel order: destination first, as NASM writes it.
    #[default]
    Intel,
    /// AT&T order: source first, with sigils.
    Att,
}

impl Syntax {
    /// A short label for the status bar.
    pub const fn label(self) -> &'static str {
        match self {
            Syntax::Intel => "intel",
            Syntax::Att => "att",
        }
    }

    /// The other syntax, for a toggle command.
    pub const fn toggled(self) -> Self {
        match self {
            Syntax::Intel => Syntax::Att,
            Syntax::Att => Syntax::Intel,
        }
    }
}

/// One decoded instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedInstruction {
    /// The address the instruction starts at.
    pub address: u64,
    /// The raw bytes that encode it.
    pub bytes: Vec<u8>,
    /// The mnemonic, lowercase and without operands.
    pub mnemonic: String,
    /// The operands as written, or empty when there are none.
    pub operands: String,
    /// Whether the decoder failed to recognise these bytes.
    pub invalid: bool,
    /// Whether the instruction transfers control.
    pub is_branch: bool,
    /// The target of a direct branch, when it has one.
    pub branch_target: Option<u64>,
}

impl DecodedInstruction {
    /// The length of the instruction in bytes.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the instruction decoded to nothing.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The address just past this instruction.
    pub fn end_address(&self) -> u64 {
        self.address.wrapping_add(self.bytes.len() as u64)
    }

    /// The raw bytes as spaced hexadecimal.
    pub fn bytes_text(&self) -> String {
        self.bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The full instruction text, mnemonic and operands.
    pub fn text(&self) -> String {
        if self.operands.is_empty() {
            self.mnemonic.clone()
        } else {
            format!("{} {}", self.mnemonic, self.operands)
        }
    }
}

/// Decodes `bytes` as instructions starting at `address`.
///
/// # Resynchronising after bad bytes
///
/// Undecodable bytes are emitted as a one-byte instruction marked
/// [`DecodedInstruction::invalid`], and decoding restarts at the very next
/// byte. Left to itself the decoder consumes as much as the opcode's escape
/// and prefix bytes imply — for `06 c3` it swallows both and the `ret` is
/// lost. Advancing a single byte instead matches what `objdump` does and
/// means a stray data byte in a code section costs one line of output rather
/// than everything after it.
pub fn decode(bytes: &[u8], address: u64, syntax: Syntax) -> Vec<DecodedInstruction> {
    if bytes.is_empty() {
        return Vec::new();
    }

    let mut formatter = make_formatter(syntax);
    let mut instruction = Instruction::default();
    let mut decoded = Vec::new();
    let mut offset = 0usize;

    while offset < bytes.len() {
        let start_address = address.wrapping_add(offset as u64);
        let mut decoder =
            Decoder::with_ip(64, &bytes[offset..], start_address, DecoderOptions::NONE);

        while decoder.can_decode() {
            decoder.decode_out(&mut instruction);

            if instruction.is_invalid() {
                decoded.push(DecodedInstruction {
                    address: start_address,
                    bytes: vec![bytes[offset]],
                    mnemonic: "(bad)".to_owned(),
                    operands: String::new(),
                    invalid: true,
                    is_branch: false,
                    branch_target: None,
                });
                offset += 1;
                break;
            }

            let length = instruction.len();
            let end = (offset + length).min(bytes.len());

            let mut text = String::new();
            formatter.format(&instruction, &mut text);
            let (mnemonic, operands) = split_text(&text);

            decoded.push(DecodedInstruction {
                address: instruction.ip(),
                bytes: bytes[offset..end].to_vec(),
                mnemonic,
                operands,
                invalid: false,
                is_branch: is_branch(&instruction),
                branch_target: branch_target(&instruction),
            });
            offset = end;
        }
    }

    decoded
}

/// Decodes at most `count` instructions starting at `address`.
pub fn decode_count(
    bytes: &[u8],
    address: u64,
    count: usize,
    syntax: Syntax,
) -> Vec<DecodedInstruction> {
    let mut decoded = decode(bytes, address, syntax);
    decoded.truncate(count);
    decoded
}

/// Builds the formatter for a syntax.
fn make_formatter(syntax: Syntax) -> Box<dyn Formatter> {
    match syntax {
        Syntax::Intel => {
            let mut formatter = IntelFormatter::new();
            let options = formatter.options_mut();
            // NASM writes lowercase mnemonics and hexadecimal with an 0x
            // prefix, so matching that keeps the two views comparable.
            options.set_uppercase_all(false);
            // `uppercase_all` does not cover hexadecimal digits, which have
            // their own option; without this, immediates render as 0x3C.
            options.set_uppercase_hex(false);
            options.set_hex_prefix("0x");
            options.set_hex_suffix("");
            options.set_space_after_operand_separator(true);
            Box::new(formatter)
        }
        Syntax::Att => {
            let mut formatter = GasFormatter::new();
            let options = formatter.options_mut();
            options.set_uppercase_all(false);
            options.set_uppercase_hex(false);
            options.set_space_after_operand_separator(true);
            Box::new(formatter)
        }
    }
}

/// Splits formatted text into a mnemonic and its operands.
fn split_text(text: &str) -> (String, String) {
    match text.split_once(char::is_whitespace) {
        Some((mnemonic, operands)) => (mnemonic.to_owned(), operands.trim().to_owned()),
        None => (text.to_owned(), String::new()),
    }
}

/// Whether an instruction transfers control.
fn is_branch(instruction: &Instruction) -> bool {
    use iced_x86::FlowControl;
    !matches!(
        instruction.flow_control(),
        FlowControl::Next | FlowControl::Exception
    )
}

/// The target of a direct branch, when the instruction has one.
///
/// Indirect branches through a register or memory have no statically known
/// target, and reporting a number for them would be a fabrication.
fn branch_target(instruction: &Instruction) -> Option<u64> {
    use iced_x86::OpKind;
    for index in 0..instruction.op_count() {
        match instruction.op_kind(index) {
            OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64 => {
                return Some(instruction.near_branch_target())
            }
            _ => {}
        }
    }
    None
}

/// A disassembly line paired with the source line it came from, when known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisassemblyLine {
    /// The decoded instruction.
    pub instruction: DecodedInstruction,
    /// The source file the instruction was generated from.
    pub file: Option<String>,
    /// The one-based source line.
    pub line: Option<usize>,
    /// The symbol the instruction falls within, and its offset.
    pub symbol: Option<(String, u64)>,
}

impl DisassemblyLine {
    /// Creates a line with no source or symbol information.
    pub fn bare(instruction: DecodedInstruction) -> Self {
        Self {
            instruction,
            file: None,
            line: None,
            symbol: None,
        }
    }

    /// The symbol reference in `name+offset` form.
    pub fn symbol_text(&self) -> Option<String> {
        self.symbol.as_ref().map(|(name, offset)| {
            if *offset == 0 {
                name.clone()
            } else {
                format!("{name}+{offset}")
            }
        })
    }
}

/// Annotates decoded instructions with the symbols they fall inside.
pub fn annotate_with_symbols(
    instructions: Vec<DecodedInstruction>,
    symbols: &[ElfSymbol],
) -> Vec<DisassemblyLine> {
    instructions
        .into_iter()
        .map(|instruction| {
            let symbol = elf::symbol_containing(symbols, instruction.address)
                .map(|symbol| (symbol.name.clone(), instruction.address - symbol.address));
            DisassemblyLine {
                instruction,
                file: None,
                line: None,
                symbol,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `mov rax, 60` — the exit syscall number, as NASM encodes it.
    const MOV_RAX_60: [u8; 7] = [0x48, 0xc7, 0xc0, 0x3c, 0x00, 0x00, 0x00];
    /// `xor edi, edi`
    const XOR_EDI_EDI: [u8; 2] = [0x31, 0xff];
    /// `syscall`
    const SYSCALL: [u8; 2] = [0x0f, 0x05];
    /// `ret`
    const RET: [u8; 1] = [0xc3];

    fn program() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MOV_RAX_60);
        bytes.extend_from_slice(&XOR_EDI_EDI);
        bytes.extend_from_slice(&SYSCALL);
        bytes
    }

    #[test]
    fn a_single_instruction_decodes_correctly() {
        let decoded = decode(&MOV_RAX_60, 0x40_00b0, Syntax::Intel);
        assert_eq!(decoded.len(), 1);

        let instruction = &decoded[0];
        assert_eq!(instruction.address, 0x40_00b0);
        assert_eq!(instruction.mnemonic, "mov");
        assert_eq!(instruction.operands, "rax, 0x3c");
        assert_eq!(instruction.len(), 7);
        assert!(!instruction.invalid);
        assert!(!instruction.is_branch);
    }

    #[test]
    fn addresses_advance_by_the_instruction_length() {
        let decoded = decode(&program(), 0x1000, Syntax::Intel);
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0].address, 0x1000);
        assert_eq!(decoded[1].address, 0x1007);
        assert_eq!(decoded[2].address, 0x1009);
        assert_eq!(decoded[2].end_address(), 0x100b);
    }

    #[test]
    fn raw_bytes_are_preserved_for_each_instruction() {
        let decoded = decode(&program(), 0x1000, Syntax::Intel);
        assert_eq!(decoded[0].bytes, MOV_RAX_60);
        assert_eq!(decoded[1].bytes, XOR_EDI_EDI);
        assert_eq!(decoded[2].bytes, SYSCALL);
        assert_eq!(decoded[1].bytes_text(), "31 ff");
    }

    #[test]
    fn intel_syntax_matches_the_order_nasm_writes() {
        let decoded = decode(&XOR_EDI_EDI, 0x1000, Syntax::Intel);
        assert_eq!(decoded[0].text(), "xor edi, edi");
    }

    #[test]
    fn att_syntax_reverses_the_operand_order() {
        // The reason Intel is the default for a NASM editor.
        let decoded = decode(&MOV_RAX_60, 0x1000, Syntax::Att);
        assert_eq!(decoded[0].mnemonic, "mov");
        assert!(
            decoded[0].operands.contains('%'),
            "AT&T uses register sigils: {}",
            decoded[0].operands
        );
        assert!(
            decoded[0].operands.starts_with('$'),
            "immediate comes first"
        );
    }

    #[test]
    fn syntax_toggles_between_the_two() {
        assert_eq!(Syntax::Intel.toggled(), Syntax::Att);
        assert_eq!(Syntax::Att.toggled(), Syntax::Intel);
        assert_eq!(Syntax::default(), Syntax::Intel);
    }

    #[test]
    fn an_instruction_with_no_operands_renders_as_the_mnemonic_alone() {
        let decoded = decode(&SYSCALL, 0x1000, Syntax::Intel);
        assert_eq!(decoded[0].mnemonic, "syscall");
        assert!(decoded[0].operands.is_empty());
        assert_eq!(decoded[0].text(), "syscall");
    }

    #[test]
    fn a_direct_branch_reports_its_target() {
        // jmp rel8 forward by 2 from a 2-byte instruction at 0x1000.
        let bytes = [0xeb, 0x02];
        let decoded = decode(&bytes, 0x1000, Syntax::Intel);
        assert!(decoded[0].is_branch);
        assert_eq!(decoded[0].branch_target, Some(0x1004));
    }

    #[test]
    fn a_conditional_branch_is_recognised() {
        // je rel8
        let bytes = [0x74, 0x05];
        let decoded = decode(&bytes, 0x2000, Syntax::Intel);
        assert!(decoded[0].is_branch);
        assert_eq!(decoded[0].branch_target, Some(0x2007));
        assert_eq!(decoded[0].mnemonic, "je");
    }

    #[test]
    fn an_indirect_branch_reports_no_static_target() {
        // `jmp rax` cannot have its target known without running the program,
        // and inventing one would be a fabrication.
        let bytes = [0xff, 0xe0];
        let decoded = decode(&bytes, 0x1000, Syntax::Intel);
        assert!(decoded[0].is_branch);
        assert_eq!(decoded[0].branch_target, None);
    }

    #[test]
    fn ret_is_recognised_as_control_flow() {
        let decoded = decode(&RET, 0x1000, Syntax::Intel);
        assert_eq!(decoded[0].mnemonic, "ret");
        assert!(decoded[0].is_branch);
    }

    #[test]
    fn a_bad_byte_costs_one_line_and_the_next_instruction_survives() {
        // Verified against objdump: 06 is invalid in 64-bit mode, and the
        // instruction after it must still decode. Letting the decoder consume
        // the escape byte of the following opcode would lose the syscall.
        let mut bytes = vec![0x06];
        bytes.extend_from_slice(&SYSCALL);
        let decoded = decode(&bytes, 0x1000, Syntax::Intel);

        assert_eq!(decoded.len(), 2);
        assert!(decoded[0].invalid);
        assert_eq!(decoded[0].len(), 1, "a bad byte consumes exactly one byte");
        assert_eq!(decoded[1].mnemonic, "syscall");
        assert_eq!(decoded[1].address, 0x1001);
    }

    #[test]
    fn a_bad_byte_before_a_single_byte_instruction_resynchronises() {
        let decoded = decode(&[0x06, 0xc3], 0x1000, Syntax::Intel);
        assert_eq!(decoded.len(), 2);
        assert!(decoded[0].invalid);
        assert_eq!(decoded[1].mnemonic, "ret");
    }

    #[test]
    fn every_byte_is_accounted_for_even_when_undecodable() {
        // Nothing may be silently dropped from the listing.
        for bytes in [
            vec![0x06, 0x0f, 0x05],
            vec![0xff, 0xff, 0xff],
            vec![0x06],
            program(),
        ] {
            let covered: usize = decode(&bytes, 0x1000, Syntax::Intel)
                .iter()
                .map(DecodedInstruction::len)
                .sum();
            assert_eq!(covered, bytes.len(), "coverage failed for {bytes:02x?}");
        }
    }

    #[test]
    fn an_empty_buffer_decodes_to_nothing() {
        assert!(decode(&[], 0x1000, Syntax::Intel).is_empty());
    }

    #[test]
    fn a_truncated_instruction_does_not_read_past_the_buffer() {
        // Only the first two bytes of a seven-byte instruction.
        let decoded = decode(&MOV_RAX_60[..2], 0x1000, Syntax::Intel);
        for instruction in &decoded {
            assert!(
                instruction.len() <= 2,
                "reported {} bytes from a 2-byte buffer",
                instruction.len()
            );
        }
    }

    #[test]
    fn decoding_is_bounded_by_the_requested_count() {
        let decoded = decode_count(&program(), 0x1000, 2, Syntax::Intel);
        assert_eq!(decoded.len(), 2);
        assert_eq!(decode_count(&program(), 0x1000, 0, Syntax::Intel).len(), 0);
        assert_eq!(decode_count(&program(), 0x1000, 99, Syntax::Intel).len(), 3);
    }

    #[test]
    fn instructions_are_annotated_with_the_symbol_they_fall_in() {
        let symbols = vec![
            ElfSymbol {
                name: "_start".to_owned(),
                address: 0x1000,
                size: 11,
                is_function: true,
            },
            ElfSymbol {
                name: "helper".to_owned(),
                address: 0x2000,
                size: 4,
                is_function: true,
            },
        ];
        let lines = annotate_with_symbols(decode(&program(), 0x1000, Syntax::Intel), &symbols);

        assert_eq!(lines[0].symbol_text().as_deref(), Some("_start"));
        assert_eq!(lines[1].symbol_text().as_deref(), Some("_start+7"));
        assert_eq!(lines[2].symbol_text().as_deref(), Some("_start+9"));
    }

    #[test]
    fn an_instruction_outside_every_symbol_is_left_unannotated() {
        let symbols = vec![ElfSymbol {
            name: "elsewhere".to_owned(),
            address: 0x9000,
            size: 4,
            is_function: true,
        }];
        let lines = annotate_with_symbols(decode(&RET, 0x1000, Syntax::Intel), &symbols);
        assert!(lines[0].symbol.is_none());
        assert!(lines[0].symbol_text().is_none());
    }

    #[test]
    fn a_bare_line_carries_no_source_information() {
        let line = DisassemblyLine::bare(decode(&RET, 0x1000, Syntax::Intel).remove(0));
        assert!(line.file.is_none());
        assert!(line.line.is_none());
        assert!(line.symbol.is_none());
    }
}

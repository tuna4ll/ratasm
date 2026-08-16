//! Reading the ELF executables the build pipeline produces.
//!
//! The debugger can answer most of these questions, but only while a session
//! is running. Reading the file directly means the disassembly, symbol list
//! and entry point are available the moment a build finishes — before the
//! program has run even once, and without GDB installed at all.

use std::path::{Path, PathBuf};

use object::{Object, ObjectSection, ObjectSymbol};

/// Errors from reading an ELF file.
#[derive(Debug, thiserror::Error)]
pub enum ElfError {
    /// The file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not a valid object file.
    #[error("{path} is not a valid ELF file: {message}")]
    Parse {
        /// The file involved.
        path: PathBuf,
        /// What the parser objected to.
        message: String,
    },
    /// The file is for an architecture ratasm cannot disassemble.
    #[error("{path} targets {architecture}, but only x86-64 is supported")]
    UnsupportedArchitecture {
        /// The file involved.
        path: PathBuf,
        /// The architecture the file declares.
        architecture: String,
    },
}

/// A symbol defined in an executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The symbol's name.
    pub name: String,
    /// The address it is defined at.
    pub address: u64,
    /// Its size in bytes, or zero when the file does not say.
    pub size: u64,
    /// Whether it names code rather than data.
    pub is_function: bool,
}

impl Symbol {
    /// The address just past this symbol, when its size is known.
    pub fn end_address(&self) -> Option<u64> {
        (self.size > 0).then(|| self.address.saturating_add(self.size))
    }

    /// Whether `address` falls inside this symbol.
    ///
    /// A symbol with no recorded size covers only its own address, because
    /// guessing where it ends would attribute unrelated code to it.
    pub fn contains(&self, address: u64) -> bool {
        match self.end_address() {
            Some(end) => address >= self.address && address < end,
            None => address == self.address,
        }
    }
}

/// An executable section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The section name, such as `.text`.
    pub name: String,
    /// The address the section is loaded at.
    pub address: u64,
    /// The section's contents, empty for sections that occupy no file space.
    pub data: Vec<u8>,
    /// Whether the section holds executable code.
    pub is_executable: bool,
}

impl Section {
    /// The section's size in bytes.
    pub fn size(&self) -> u64 {
        self.data.len() as u64
    }

    /// The address just past the section.
    pub fn end_address(&self) -> u64 {
        self.address.saturating_add(self.size())
    }

    /// Whether `address` falls inside this section.
    pub fn contains(&self, address: u64) -> bool {
        address >= self.address && address < self.end_address()
    }

    /// The bytes starting at `address`, when the section covers it.
    pub fn bytes_at(&self, address: u64) -> Option<&[u8]> {
        if !self.contains(address) {
            return None;
        }
        let offset = usize::try_from(address - self.address).ok()?;
        self.data.get(offset..)
    }
}

/// A parsed executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElfImage {
    /// Where the file was read from.
    pub path: PathBuf,
    /// The address execution begins at.
    pub entry: u64,
    /// Every section that carries an address.
    pub sections: Vec<Section>,
    /// Every named symbol, sorted by address.
    pub symbols: Vec<Symbol>,
}

impl ElfImage {
    /// Reads and parses an executable.
    ///
    /// # Errors
    ///
    /// Returns [`ElfError::Read`] if the file cannot be opened,
    /// [`ElfError::Parse`] if it is not a valid object file, or
    /// [`ElfError::UnsupportedArchitecture`] if it targets something other
    /// than x86-64 — reported explicitly rather than producing nonsense
    /// disassembly from an x86 decoder applied to foreign machine code.
    pub fn load(path: &Path) -> Result<Self, ElfError> {
        let data = std::fs::read(path).map_err(|source| ElfError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(path, &data)
    }

    /// Parses an executable already held in memory.
    ///
    /// # Errors
    ///
    /// As [`ElfImage::load`], minus the read error.
    pub fn parse(path: &Path, data: &[u8]) -> Result<Self, ElfError> {
        let file = object::File::parse(data).map_err(|error| ElfError::Parse {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;

        if file.architecture() != object::Architecture::X86_64 {
            return Err(ElfError::UnsupportedArchitecture {
                path: path.to_path_buf(),
                architecture: format!("{:?}", file.architecture()),
            });
        }

        let mut sections = Vec::new();
        for section in file.sections() {
            let name = section.name().unwrap_or("<unnamed>").to_owned();
            // Sections such as .bss occupy no file space; their contents are
            // not available statically, so they are recorded as empty rather
            // than as zeros that might be mistaken for real data.
            let data = section.data().map(<[u8]>::to_vec).unwrap_or_default();
            let is_executable = section
                .flags()
                .pipe_elf(|flags| flags & u64::from(object::elf::SHF_EXECINSTR) != 0);

            sections.push(Section {
                name,
                address: section.address(),
                data,
                is_executable,
            });
        }

        let mut symbols: Vec<Symbol> = file
            .symbols()
            .filter_map(|symbol| {
                let name = symbol.name().ok()?;
                if name.is_empty() {
                    return None;
                }
                Some(Symbol {
                    name: name.to_owned(),
                    address: symbol.address(),
                    size: symbol.size(),
                    is_function: symbol.kind() == object::SymbolKind::Text,
                })
            })
            .collect();
        symbols.sort_by_key(|symbol| (symbol.address, symbol.name.clone()));

        Ok(Self {
            path: path.to_path_buf(),
            entry: file.entry(),
            sections,
            symbols,
        })
    }

    /// The section holding executable code, conventionally `.text`.
    pub fn text_section(&self) -> Option<&Section> {
        self.sections
            .iter()
            .find(|section| section.name == ".text")
            .or_else(|| {
                self.sections
                    .iter()
                    .find(|section| section.is_executable && !section.data.is_empty())
            })
    }

    /// The section containing `address`, if any.
    pub fn section_at(&self, address: u64) -> Option<&Section> {
        self.sections
            .iter()
            .find(|section| section.contains(address))
    }

    /// The bytes at `address`, from whichever section holds it.
    pub fn bytes_at(&self, address: u64) -> Option<&[u8]> {
        self.section_at(address)?.bytes_at(address)
    }

    /// The symbol containing `address`, if any.
    pub fn symbol_at(&self, address: u64) -> Option<&Symbol> {
        symbol_containing(&self.symbols, address)
    }

    /// Looks up a symbol by name.
    pub fn symbol_named(&self, name: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|symbol| symbol.name == name)
    }

    /// Every function symbol, in address order.
    pub fn functions(&self) -> Vec<&Symbol> {
        self.symbols
            .iter()
            .filter(|symbol| symbol.is_function)
            .collect()
    }
}

/// Finds the symbol whose range covers `address`.
///
/// Prefers the closest preceding symbol when several could match, which is how
/// a disassembly listing attributes an instruction to a function.
pub fn symbol_containing(symbols: &[Symbol], address: u64) -> Option<&Symbol> {
    symbols
        .iter()
        .filter(|symbol| symbol.contains(address))
        .max_by_key(|symbol| symbol.address)
}

/// Extension used to read architecture-specific section flags.
trait FlagsExt {
    /// Applies `f` to the ELF section flags, or returns `false`.
    fn pipe_elf<F: FnOnce(u64) -> bool>(self, f: F) -> bool;
}

impl FlagsExt for object::SectionFlags {
    fn pipe_elf<F: FnOnce(u64) -> bool>(self, f: F) -> bool {
        match self {
            object::SectionFlags::Elf { sh_flags } => f(sh_flags),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a real executable with the system toolchain.
    ///
    /// Returns `None` when nasm or ld is unavailable, so the suite still
    /// passes on a machine without them.
    fn build_program(source: &str) -> Option<(tempfile::TempDir, PathBuf)> {
        use crate::process::is_available;
        if !is_available(Path::new("nasm")) || !is_available(Path::new("ld")) {
            return None;
        }

        let dir = tempfile::tempdir().ok()?;
        let asm = dir.path().join("main.asm");
        let object = dir.path().join("main.o");
        let executable = dir.path().join("main");
        std::fs::write(&asm, source).ok()?;

        let assembled = std::process::Command::new("nasm")
            .args(["-f", "elf64"])
            .arg(&asm)
            .arg("-o")
            .arg(&object)
            .status()
            .ok()?;
        if !assembled.success() {
            return None;
        }

        let linked = std::process::Command::new("ld")
            .arg(&object)
            .arg("-o")
            .arg(&executable)
            .status()
            .ok()?;
        if !linked.success() {
            return None;
        }
        Some((dir, executable))
    }

    const PROGRAM: &str = "\
section .data
    message: db \"hi\", 0

section .text
    global _start
_start:
    mov rax, 60
    xor edi, edi
    syscall
";

    #[test]
    fn a_real_executable_is_parsed() {
        let Some((_dir, path)) = build_program(PROGRAM) else {
            eprintln!("skipping: nasm or ld not installed");
            return;
        };

        let image = ElfImage::load(&path).expect("parse");
        assert!(image.entry > 0, "an entry point must be reported");
        assert!(!image.sections.is_empty());
        assert!(!image.symbols.is_empty());
    }

    #[test]
    fn the_entry_point_matches_the_start_symbol() {
        let Some((_dir, path)) = build_program(PROGRAM) else {
            eprintln!("skipping: nasm or ld not installed");
            return;
        };

        let image = ElfImage::load(&path).expect("parse");
        let start = image.symbol_named("_start").expect("_start is defined");
        assert_eq!(
            image.entry, start.address,
            "ld uses _start as the entry point"
        );
    }

    #[test]
    fn the_text_section_holds_the_entry_point() {
        let Some((_dir, path)) = build_program(PROGRAM) else {
            eprintln!("skipping: nasm or ld not installed");
            return;
        };

        let image = ElfImage::load(&path).expect("parse");
        let text = image.text_section().expect(".text must exist");
        assert!(text.is_executable);
        assert!(!text.data.is_empty());
        assert!(
            text.contains(image.entry),
            ".text must cover the entry point"
        );
    }

    #[test]
    fn the_entry_bytes_decode_to_the_first_instruction() {
        // The end-to-end property: reading the file gives real machine code.
        let Some((_dir, path)) = build_program(PROGRAM) else {
            eprintln!("skipping: nasm or ld not installed");
            return;
        };

        let image = ElfImage::load(&path).expect("parse");
        let bytes = image.bytes_at(image.entry).expect("bytes at the entry");
        let decoded = super::super::decode(bytes, image.entry, super::super::Syntax::Intel);

        // NASM assembles `mov rax, 60` as `mov eax, 60` (b8 3c ...): the
        // immediate fits in 32 bits and writing EAX zeroes the upper half, so
        // the shorter encoding is equivalent. The disassembly shows what was
        // actually emitted rather than what was written.
        assert_eq!(decoded[0].mnemonic, "mov");
        assert_eq!(decoded[0].operands, "eax, 0x3c");
        assert_eq!(decoded[1].mnemonic, "xor");
        assert_eq!(decoded[2].mnemonic, "syscall");
    }

    #[test]
    fn a_data_symbol_is_found_in_its_own_section() {
        let Some((_dir, path)) = build_program(PROGRAM) else {
            eprintln!("skipping: nasm or ld not installed");
            return;
        };

        let image = ElfImage::load(&path).expect("parse");
        let message = image.symbol_named("message").expect("message is defined");
        let section = image
            .section_at(message.address)
            .expect("the symbol lives in a section");
        assert_eq!(section.name, ".data");
    }

    #[test]
    fn symbols_are_sorted_by_address() {
        let Some((_dir, path)) = build_program(PROGRAM) else {
            eprintln!("skipping: nasm or ld not installed");
            return;
        };

        let image = ElfImage::load(&path).expect("parse");
        let mut previous = 0u64;
        for symbol in &image.symbols {
            assert!(symbol.address >= previous, "symbols must be ordered");
            previous = symbol.address;
        }
    }

    #[test]
    fn a_missing_file_is_reported_rather_than_panicking() {
        let error = ElfImage::load(Path::new("/nonexistent/program")).expect_err("must fail");
        assert!(matches!(error, ElfError::Read { .. }));
    }

    #[test]
    fn a_file_that_is_not_an_executable_is_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("not-elf");
        std::fs::write(&path, b"this is plain text, not an object file").expect("write");

        let error = ElfImage::load(&path).expect_err("must fail");
        assert!(matches!(error, ElfError::Parse { .. }));
    }

    #[test]
    fn an_empty_file_is_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("empty");
        std::fs::write(&path, b"").expect("write");
        assert!(ElfImage::load(&path).is_err());
    }

    #[test]
    fn symbol_containment_respects_recorded_sizes() {
        let symbols = vec![
            Symbol {
                name: "first".to_owned(),
                address: 0x1000,
                size: 16,
                is_function: true,
            },
            Symbol {
                name: "second".to_owned(),
                address: 0x2000,
                size: 8,
                is_function: true,
            },
        ];

        assert_eq!(
            symbol_containing(&symbols, 0x1008).map(|s| s.name.as_str()),
            Some("first")
        );
        assert_eq!(
            symbol_containing(&symbols, 0x2000).map(|s| s.name.as_str()),
            Some("second")
        );
        assert_eq!(symbol_containing(&symbols, 0x1010), None, "just past first");
        assert_eq!(symbol_containing(&symbols, 0x500), None, "before them all");
    }

    #[test]
    fn a_sizeless_symbol_covers_only_its_own_address() {
        // Guessing an extent would attribute unrelated code to the symbol.
        let symbols = [Symbol {
            name: "marker".to_owned(),
            address: 0x1000,
            size: 0,
            is_function: false,
        }];
        assert!(symbols[0].contains(0x1000));
        assert!(!symbols[0].contains(0x1001));
        assert_eq!(symbols[0].end_address(), None);
    }

    #[test]
    fn a_section_reports_the_bytes_from_an_offset() {
        let section = Section {
            name: ".text".to_owned(),
            address: 0x1000,
            data: vec![0x11, 0x22, 0x33, 0x44],
            is_executable: true,
        };

        assert_eq!(
            section.bytes_at(0x1000),
            Some(&[0x11, 0x22, 0x33, 0x44][..])
        );
        assert_eq!(section.bytes_at(0x1002), Some(&[0x33, 0x44][..]));
        assert_eq!(section.bytes_at(0x1004), None, "just past the end");
        assert_eq!(section.bytes_at(0x0fff), None, "before the start");
        assert_eq!(section.end_address(), 0x1004);
    }

    #[test]
    fn a_section_with_no_contents_reports_no_bytes() {
        // .bss occupies no file space; it must not appear to hold zeros.
        let section = Section {
            name: ".bss".to_owned(),
            address: 0x2000,
            data: Vec::new(),
            is_executable: false,
        };
        assert_eq!(section.size(), 0);
        assert!(!section.contains(0x2000));
        assert_eq!(section.bytes_at(0x2000), None);
    }
}

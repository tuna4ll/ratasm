# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- NASM editor core: line-oriented buffer with exactly invertible edits,
  grouped undo and redo, save-point tracking, multi-file workspace with atomic
  saves, search and replace, bracket matching over tokens, symbol extraction
  with NASM local-label scoping, and position-aware completion.
- NASM lexer that classifies identifiers by their position in a statement, so
  a label and an instruction are told apart the way the assembler does.
- Build pipeline driving `nasm` and `ld`, with assembler and linker output
  parsed into diagnostics carrying file, line and column.
- Process runner with timeouts, concurrent pipe draining and guaranteed
  reaping; external tools are never launched through a shell.
- Project system: `.ratasm.toml` with validated paths, project discovery,
  scaffolding, and an implicit project for a lone source file.
- GDB machine interface: value grammar parser, typed output records with token
  correlation, injection-safe command builder, and a session driver with
  guaranteed process cleanup.
- Debugger state machine that rejects invalid transitions.
- Register model with change highlighting and hexadecimal, decimal, signed,
  binary and ASCII views across all four register widths.
- Memory model with a hex dump and an address-expression evaluator that never
  panics on malformed input.
- Breakpoints that survive across debug sessions, reconciled with the numbers
  GDB assigns.
- Disassembler built on `iced-x86`, with ELF inspection and `objdump`-style
  resynchronisation after undecodable bytes.
- Instruction semantics database covering data movement, arithmetic, bitwise,
  shift and rotate, comparison, jumps, stack, call and return, string, syscall
  and basic SIMD groups, plus an explainer that substitutes real operands.
- Linux x86-64 syscall database generated from the kernel headers.
- Themes for dark, light, sixteen-colour and colour-blind-safe terminals, with
  ASCII-safe glyphs for terminals without Unicode.
- Terminal interface: editor, register, flag, stack, memory, disassembly,
  breakpoint, output, explanation, syscall and explorer panels, with a
  responsive layout that keeps the focused panel visible at any size.
- Command palette with fuzzy search, and key bindings whose conflicts are
  reported rather than silently shadowing one another.
- Command-line interface: `new`, `build`, `run` and `doctor` work without a
  terminal, so the toolchain composes with Make and CI.

### Fixed

- Register values no longer truncate to 32 bits. GDB lists the narrow
  pseudo-registers (`esp`, `eax`) alongside the full ones, and taking both left
  `RSP` holding only its low half, which broke every stack and memory read.
- A debug session stops at the entry point of a `_start`-only program.
  `-exec-run --start` breaks on `main`, which assembly programs do not have, so
  the program ran to completion instead of stopping.
- The disassembler resynchronises one byte after an undecodable byte, matching
  `objdump`. Letting the decoder consume the following opcode's escape byte
  discarded the next valid instruction.

[Unreleased]: https://github.com/tuna4ll/ratasm/compare/main...HEAD

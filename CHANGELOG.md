# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-09-06

### Fixed

- The program-counter marker is tied to the file execution stopped in. It was
  drawn wherever the stopped line number matched, so in a project with more
  than one source it marked an unrelated line in whatever buffer was showing.
- Document lookup compares whole trailing path components rather than the bare
  file name, so `src/main.asm` and `lib/main.asm` are no longer the same file.
- Stepping into a source that is not open opens it, instead of leaving the
  editor on the previous file as though execution had never left it.
- Jumping to a build error goes to the file the diagnostic names. The cursor
  moved in whatever document was active, which in a multi-source project is
  almost never the right one, and a failed build does this automatically.

## [0.1.0] - 2026-09-03

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
- Call stack panel built from GDB's frame list, which preserves the repeated
  `frame=` keys a map representation would have collapsed.
- Explanations that read live register values: `RAX ← RAX + RBX` becomes
  `0x1 ← 0x1 + 0x2` while the program is stopped. Memory operands are left
  unresolved rather than guessed at.
- Reverse execution: stepping and continuing backwards through GDB's process
  recording, which is enabled once the program is live and can be turned off
  with `debugger.record`.
- Scratchpad: set starting register values, run one instruction, and see which
  registers and flags it actually changed. The snippet is assembled and run
  natively; it is not a sandbox, and the panel says so.
- Learning mode: lessons on the registers, the System V AMD64 ABI, the stack,
  the flags and the Linux syscall convention, followed by questions whose
  stated answers are checked against a real processor by the test suite.
- Copy, cut and paste, with copied text offered to the terminal's clipboard
  through OSC 52 so it works over SSH. Paste uses ratasm's own copy, because a
  terminal that ignores the read request answers with silence.
- Programs run on their own task, so the interface keeps drawing and reading
  keys while one runs, and <kbd>Ctrl</kbd>+<kbd>F5</kbd> stops one that will not
  stop itself.
- Pages. The fourteen panels are grouped into four pages — Code, Debug, Learn
  and Reference — opened with <kbd>Alt</kbd> plus their number. Tab moves
  between the panels of the open page, and a debug session opens the page that
  shows the machine.

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
- The flags panel is no longer blank. Filtering the narrow pseudo-registers by
  name dropped `eflags`, which GDB never calls `rflags`; the widest reported
  alias for each register is kept instead.
- A register name six characters long no longer runs into its value: `RFLAGS`
  filled the whole name column, leaving no separating space.
- The explanation panel describes the file execution stopped in, rather than
  whichever file happens to be open in the editor.
- Stopping scrolls the editor to the line it stopped on. The marker in the
  gutter was usually off-screen, which made the debugger look as though it had
  stopped somewhere else.
- The flag panel fits its nine flags to the panel's width, so the list of
  conditional jumps that would be taken is not pushed off the bottom.
- The system call list is only as tall as it has matches, instead of leaving a
  panel-sized gap above the details, and shows the NASM example that was
  already in the database.

[Unreleased]: https://github.com/tuna4ll/ratasm/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/tuna4ll/ratasm/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/tuna4ll/ratasm/releases/tag/v0.1.0

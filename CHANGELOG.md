# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4] - 2026-09-11

### Added

- Go to definition and search look across the project. A name declared `extern`
  was answered with the `extern` line itself, and a label in another file was
  reported undefined; both now resolve through the other open documents and the
  project's remaining sources. Find next and find previous carry on into the
  next open document.
- The explorer lists the project's files rather than only the open buffers, so
  a second source is something you can see instead of something you have to
  remember the name of. Arrows move, `Enter` opens, `R` re-reads the list.
- `Tab` completes paths in the open and save prompts, and go to definition on
  an `%include` line opens the file it names.
- Typing `(`, `[`, `{`, `"`, `'` or `` ` `` inserts its partner and steps over
  the one already there; `auto_close_pairs = false` turns that off.
- `Ctrl+Backspace` and `Ctrl+Delete` delete by word, clearing a line's
  indentation to the margin in one press.

### Fixed

- The toolchain's paths agree with the directory it runs in. Tools are spawned
  in the project root but their arguments were resolved against the caller, so
  `ratasm build some/project` could not find an output directory it had just
  created.
- The editor draws the selection. Selecting text changed nothing on screen.
- An unclaimed chord no longer types its letter. Terminals send
  `Ctrl+Backspace` as `Ctrl+H`, which the editor took for text and inserted.
- The disassembly view no longer spends 44 columns before the mnemonic; the
  address and byte columns are sized to the listing in hand.
- Every drawn panel gets room it can use, and a compact arrangement fills the
  gap between the wide and medium layouts, where one row short of the wide
  threshold used to drop a page from seven panels to two.
- The page bar keeps the open file visible on a narrow terminal, and the status
  bar gives a long message the room it needs rather than holding a fixed set of
  fields.

## [0.1.3] - 2026-09-09

### Added

- `Ctrl+Alt+A` adds the active file to the project's sources and writes the
  project file back. Opening a second file put it in a buffer and nowhere else,
  so it was never assembled, and the only sign was a linker error about an
  undefined symbol. The explorer marks any open document the build does not
  know about.
- The output panel wraps long tool output instead of cutting it at the right
  edge, which is where the informative half of a `nasm` error lives, and its
  border carries the last build's error and warning counts.

## [0.1.2] - 2026-09-08

### Added

- Every panel scrolls. Only the editor had a viewport, so anything that did not
  fit was never drawn and nothing said so: eighteen registers into the eight
  rows a 120x30 terminal allows, or a build log three lines deep. A scrollbar
  appears when there is more content than room, and the output panel follows
  its newest line until the reader scrolls away from it.
- The mouse works. Capture was enabled from the first release but no event was
  ever read. The wheel scrolls whatever is under the pointer without taking
  focus, a click focuses a panel and places the editor cursor, and clicks on
  the page bar open a page or switch file.
- `ratasm` takes more than one file: the first decides the project, the rest
  are opened alongside it.

### Fixed

- Every edited buffer is written before assembling. The build read files from
  disk and never consulted the workspace, so an unsaved buffer was assembled
  from its previous contents without a word. `Ctrl+Alt+S` saves them on demand.

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

[Unreleased]: https://github.com/tuna4ll/ratasm/compare/v0.1.4...HEAD
[0.1.4]: https://github.com/tuna4ll/ratasm/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/tuna4ll/ratasm/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/tuna4ll/ratasm/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/tuna4ll/ratasm/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/tuna4ll/ratasm/releases/tag/v0.1.0

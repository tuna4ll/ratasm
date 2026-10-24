<div align="center">

<img src="assets/ratasm.png" alt="ratasm" width="160">

# ratasm

**A terminal IDE and debugger for x86-64 assembly.**

[![CI](https://github.com/tuna4ll/ratasm/actions/workflows/ci.yml/badge.svg)](https://github.com/tuna4ll/ratasm/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/ratasm.svg)](https://crates.io/crates/ratasm)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

</div>

<img src="assets/screenshot-debug.png" alt="the debug page: source, disassembly, registers, flags and stack" width="100%">

Assembly is hard mostly because the machine state is invisible. You write
`add rax, rbx` and to know what happened you need two registers, six flags and
the stack, at that instruction rather than before or after it.

ratasm puts them next to your source. The register panel marks what the last
instruction changed. The flag panel lists the conditional jumps that would be
taken as the flags stand right now. The explainer reads your actual operands,
so `add rax, rbx` becomes `RAX ← RAX + RBX` with what it reads, writes and
sets. The syscall finder answers which register the fourth argument goes in
(`R10`, not `RCX`).

Registers and memory come from GDB, the disassembly from a real x86-64 decoder,
the syscall numbers from your kernel's headers. Where ratasm does not know
something it says so.

## Install

```sh
curl -fsSL https://github.com/tuna4ll/ratasm/releases/latest/download/install.sh | sh
```

Downloads the release binary for your machine, checks its checksum and installs
it into `~/.local/bin` (`RATASM_INSTALL_DIR` overrides that). To read the script
first, download it, then run `sh install.sh`.

Or `cargo install ratasm`, or clone and `cargo install --path .`.

ratasm drives the standard Linux toolchain rather than bundling one: `nasm` to
assemble, `ld` to link, `gdb` to debug, and `make` for projects that have a
Makefile. Everything except debugging works without `gdb`.

```sh
sudo apt install nasm binutils gdb make      # Debian, Ubuntu
sudo dnf install nasm binutils gdb make      # Fedora
sudo pacman -S nasm binutils gdb make        # Arch
```

Run `ratasm doctor` to check what is installed. Linux on x86-64 only for now.

## Start

```sh
ratasm new hello
cd hello
ratasm
```

`ratasm new` writes a working, commented `write`/`exit` program rather than a
stub. <kbd>F6</kbd> builds it, <kbd>F5</kbd> runs it, <kbd>F9</kbd> sets a
breakpoint on the cursor's line, <kbd>F12</kbd> starts debugging, and
<kbd>F7</kbd> steps one instruction while you watch the registers move.

| Command | Starts from |
| --- | --- |
| `ratasm new NAME` | `write`/`exit` with syscalls, built by nasm and ld |
| `ratasm new --make NAME` | the same program with a `Makefile` (`all`, `run`, `clean`) |
| `ratasm new --libc NAME` | a `main` that calls `printf`, linked by gcc |

Any folder of assembly is a project too: run `ratasm` in it, or
`ratasm path/to/file.asm`. Without a project file ratasm builds every `.asm`
file in the folder; with a `Makefile` it runs `make`.

Running opens a real terminal in the Output panel. Programs can prompt, read
input, move the cursor and use ANSI colour; the same terminal is attached when
the program runs under GDB. <kbd>Ctrl</kbd>+<kbd>F5</kbd> stops it.

## The IDE

<img src="assets/screenshot-code.png" alt="the code page: editor with a breakpoint and an error, project files, the program's output" width="100%">

- **Editor** with tabs, undo and redo, selection, clipboard, find and replace,
  go to line, go to definition, NASM highlighting and bracket pairing.
  Errors and warnings from the last build sit beside their lines, and a failed
  build puts the cursor on the first one.
- **Project** files in the explorer: create, rename, move and delete them
  there. Files changed on disk by something else are reloaded.
- **Search**: <kbd>Ctrl</kbd>+<kbd>T</kbd> jumps to any label, constant or
  macro in the project, <kbd>Alt</kbd>+<kbd>F</kbd> searches every file.
- **Builds** with nasm and ld, or with your `Makefile`; *Run a make target*
  in the palette runs `clean`, `run` or any other target.
- **Debugger**: breakpoints, conditions, watchpoints, stepping forwards and
  backwards, writable registers, memory, the call stack, and disassembly that
  names branch targets.
- **Sessions**: open files and breakpoints come back when you reopen the
  project.

There are two pages. <kbd>Alt</kbd>+<kbd>1</kbd> is **Code**: editor,
explorer, output. <kbd>Alt</kbd>+<kbd>2</kbd> is **Debug**: editor, registers
and flags, stack, disassembly and an explanation of the current instruction.
<kbd>F1</kbd> switches to the study mode, **Learn** and **Reference**: lessons
and questions checked against the processor, a scratchpad that runs one
instruction, and the system call and instruction reference.

<details>
<summary>Search, Learn and Reference</summary>

<img src="assets/screenshot-search.png" alt="searching every file of the project" width="100%">
<img src="assets/screenshot-learn.png" alt="the learn page with the scratchpad" width="100%">
<img src="assets/screenshot-reference.png" alt="the reference page" width="100%">

</details>

## Keys

| Key | Action |
| --- | --- |
| <kbd>F6</kbd> / <kbd>F5</kbd> | Build / run, or continue when paused |
| <kbd>F12</kbd> / <kbd>Shift</kbd>+<kbd>F12</kbd> | Start / stop debugging |
| <kbd>F9</kbd> | Toggle a breakpoint on the cursor's line |
| <kbd>F7</kbd> / <kbd>F8</kbd> / <kbd>F10</kbd> | Step instruction / over / source line |
| <kbd>Shift</kbd>+<kbd>F7</kbd> | Step one instruction backwards |
| <kbd>Ctrl</kbd>+<kbd>S</kbd> / <kbd>Ctrl</kbd>+<kbd>N</kbd> / <kbd>Ctrl</kbd>+<kbd>O</kbd> | Save / new file / open |
| <kbd>Ctrl</kbd>+<kbd>F</kbd> / <kbd>Alt</kbd>+<kbd>F</kbd> | Find in file / search the project |
| <kbd>Ctrl</kbd>+<kbd>T</kbd> / <kbd>Ctrl</kbd>+<kbd>G</kbd> | Go to symbol / line |
| <kbd>Ctrl</kbd>+<kbd>P</kbd> | Command palette: every command, with its key |
| <kbd>Alt</kbd>+<kbd>1</kbd> / <kbd>2</kbd> | Code / Debug page |
| <kbd>Alt</kbd>+<kbd>←</kbd> / <kbd>→</kbd> | Previous / next panel |
| <kbd>F1</kbd> | Learn and Reference, and back |

Every binding is configurable, and conflicts are reported at start-up.
Full list: [docs/keybindings.md](docs/keybindings.md).

## Configuration

Nothing is required. A `.ratasm.toml` in the project root pins down what would
otherwise be inferred:

```toml
[project]
entry = "src/main.asm"
sources = ["src/util.asm"]

[build]
assembler_args = ["-f", "elf64"]
# make = false            # ignore the Makefile
# executable = "build/x"  # the program a Makefile builds

[run]
timeout_ms = 5000         # 0 disables the limit
```

A misspelled key is an error at load time rather than a setting that quietly
does nothing, and paths may not escape the project directory. Editor, theme,
debugger and key settings live in `~/.config/ratasm/config.toml`. Reference:
[docs/configuration.md](docs/configuration.md).

## Security

**Programs you write in ratasm run natively, with your privileges.** The
scratchpad is a convenience for trying one instruction, not a sandbox. There is
no isolation, no seccomp filter, no container. Do not paste assembly you do not
understand into it and run it.

The run timeout stops a program that loops forever; it is not a security
boundary. External tools are launched through `execve` with separate arguments
and never through a shell.

## Limitations

- Linux x86-64 and NASM syntax only. A project file naming `aarch64` or `gas`
  gets a clear "not supported yet" rather than a confusing failure.
- Vector instructions are named and described but not modelled per lane, and
  the explainer says so.
- Around sixty syscalls carry full argument documentation. All 385 are
  searchable; the rest point at `man 2`.
- Source-level stepping needs debug information. Without it you get
  instruction-level stepping.
- One debug session at a time, no multi-threaded targets.
- With a Makefile, debug information is the Makefile's to ask for
  (`-g -F dwarf` for nasm).
- Stepping backwards needs GDB's recording, which is on by default and costs
  time per instruction. `record = false` turns it off.
- Copying reaches the system clipboard through OSC 52 where the terminal allows
  it, but pasting only sees what ratasm copied: reading the clipboard back is
  not something a terminal reliably permits.

## Contributing

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

All three must pass. Tests needing `nasm`, `ld`, `gdb`, `make` or `gcc` skip
themselves when those are missing. Workflow: [CONTRIBUTING.md](CONTRIBUTING.md). Architecture
notes: [docs/architecture.md](docs/architecture.md).

## Roadmap

AT&T syntax throughout, GAS source support, a C front end for mixed projects,
and AArch64 and RISC-V back ends.

## Licence

MIT. See [LICENSE](LICENSE).

<div align="center">
<sub>Built by <a href="https://github.com/tuna4ll">Tuna Kılıç</a></sub>
</div>

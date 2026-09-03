# Contributing to ratasm

Thanks for taking an interest. This document covers what you need to know to
get a change merged.

## Getting set up

```sh
git clone https://github.com/tuna4ll/ratasm
cd ratasm
cargo build
```

You need a recent stable Rust toolchain — the minimum is `rust-version` in
Cargo.toml, and CI reads it from there — plus the assembly toolchain ratasm
drives:

```sh
sudo apt install nasm binutils gdb      # Debian, Ubuntu
sudo dnf install nasm binutils gdb      # Fedora
sudo pacman -S nasm binutils gdb        # Arch
```

Tests that need those tools skip themselves when they are missing, so a partial
toolchain will not give you false failures — but it will silently reduce what
you are testing. Install all three before trusting a green run.

## Before you open a pull request

These three commands must pass. CI runs exactly the same ones.

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

## What we look for

**Tests that could fail.** A test that passes no matter what the code does is
worse than no test, because it makes the suite look stronger than it is. Prefer
a test that pins down real behaviour: an off-by-one, an encoding detail, an
error path.

Several tests in this repository exist because they caught a genuine bug during
development — the sorted-table check in `instruction/mnemonics.rs`, the octal
exit code in `debugger/mi/record.rs`, the resynchronisation check in
`disassembler`. Those are the useful kind.

**No invented facts.** When ratasm does not know something, it says so. The
syscall database marks entries whose arguments are not modelled; the instruction
explainer returns nothing for an unknown mnemonic rather than a generic
sentence; the disassembler reports no target for an indirect branch. Please keep
that property. A confident wrong answer is worse than a blank.

**Errors, not panics.** Anything a user can type — an address expression, a
project file, a search query — must produce a message rather than a crash.
Production code avoids `unwrap()` and `expect()`; tests may use them freely.

**Public items are documented.** The crate sets `#![warn(missing_docs)]`, and
clippy runs with `-D warnings`, so an undocumented public item fails the build.
Say why something exists, not just what it is.

## How the code is organised

State, rendering, process management and the debugger protocol are kept apart:

- `editor/` is pure text state and never draws.
- `instruction/` is static architectural knowledge with no runtime state.
- `debugger/` speaks GDB/MI and never touches the terminal.
- `ui/` reads state and produces widgets; it never mutates state.

The rule of thumb: if a module needs a terminal or a debugger to be tested, it
is probably doing too much. See [docs/architecture.md](docs/architecture.md).

## Commit messages

Short, imperative, and scoped:

```text
feat(editor): add bracket matching over tokens
fix(mi): decode octal exit codes
docs(readme): describe the install script
```

## Adding an instruction or a syscall

Both are data, not code.

- Instructions live in `assets/instructions.json`. Add an entry with its
  effect template, what it reads and writes, and which flags it touches. The
  tests check that every placeholder names a declared operand and every flag
  name is real.
- Syscalls live in `assets/syscalls-x86_64.json`. The numbers come from the
  kernel headers and should not be edited by hand. To fill in an entry's
  arguments, set `detailed` to `true` and add them in register order — the
  tests enforce that an entry marked detailed actually carries the data.

## Reporting bugs

Include what you ran, what happened, and what you expected. For anything
involving the debugger, `ratasm --log-file /tmp/ratasm.log --log-filter debug`
records the GDB/MI exchange, which is usually enough to see what went wrong.

## Licence

By contributing you agree that your work is licensed under the MIT licence, as
the rest of the project is.

# Development

## Building

```sh
cargo build
cargo run -- new /tmp/demo
```

## The checks CI runs

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo doc --no-deps --all-features
```

`cargo doc` runs with `RUSTDOCFLAGS=-D warnings`, so a broken intra-doc link
fails the build.

## Tests

```sh
cargo test                       # everything
cargo test --lib debugger        # one module
cargo test -- --nocapture        # see skip messages
```

Tests needing `nasm`, `ld` or `gdb` print a skip message and pass when those
tools are missing. That keeps the suite green on a bare machine, but it also
means a green run on such a machine covers less than it appears to. Install all
three before trusting a result.

The toolchain-dependent tests do real work: they assemble and link actual
programs, run them, and drive a real GDB. `disassembler::elf` builds an
executable and decodes its entry point; `assembler::build` checks that a null
dereference is reported as `SIGSEGV` and that an infinite loop is stopped by
the timeout.

## Logging

```sh
cargo run -- --log-file /tmp/ratasm.log --log-filter debug
tail -f /tmp/ratasm.log
```

The TUI owns the terminal, so logs must go to a file. `ratasm::debugger=debug`
records the whole GDB/MI exchange.

## Adding an instruction

`assets/instructions.json`:

```json
{
  "mnemonic": "adc",
  "group": "arithmetic",
  "summary": "Add the source and the carry flag to the destination.",
  "operands": ["dst", "src"],
  "effect": "{dst} ← {dst} + {src} + CF",
  "reads": ["dst", "src"],
  "writes": ["dst"],
  "flags_set": ["CF", "PF", "AF", "ZF", "SF", "OF"],
  "flags_read": ["CF"],
  "flags_cleared": [],
  "flags_undefined": [],
  "notes": "Used to chain additions across registers."
}
```

Tests enforce that every `{placeholder}` names a declared operand, every flag
abbreviation is real, every read and write slot is an operand or a register, and
the mnemonic is recognised by the lexer. Condition-code variants (`je`, `setle`,
`cmovg`) are generated, not listed — add only the canonical spelling.

The four flag lists are distinct on purpose. `flags_set` means computed from the
result; `flags_cleared` means forced to zero regardless (as `and` does to `CF`);
`flags_undefined` means the architecture leaves it unspecified. Do not collapse
them, and do not guess an undefined flag's value.

## Adding a syscall

The numbers in `assets/syscalls-x86_64.json` come from the kernel's
`asm/unistd_64.h`. Do not edit them by hand.

To document an entry, set `detailed` to `true` and fill in `signature`, `args`
in register order (`rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9`), `returns` and
`errors` in `-ERRNO: explanation` form. Tests check that a detailed entry
carries all of it, and that a non-detailed entry invents none of it.

## Style

- Production code avoids `unwrap()` and `expect()`; tests use them freely.
- Public items need documentation — `missing_docs` plus `-D warnings`.
- Comments explain *why*. What the code does is visible; why it does it that
  way, and what breaks otherwise, is not.
- Prefer a test that pins real behaviour over one that restates the
  implementation.

## Releasing

1. Update the version in `Cargo.toml`.
2. Move the `Unreleased` entries in `CHANGELOG.md` under the new version.
3. Commit, tag `vX.Y.Z`, and push the tag.

The release workflow verifies that the tag matches `Cargo.toml`, runs the full
checks, builds musl and glibc binaries, publishes a GitHub release with
checksums and the install script, and pushes to crates.io.

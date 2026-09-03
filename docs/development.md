# Development

## Building

```sh
cargo build
cargo run -- new /tmp/demo
```

## The checks CI runs

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo doc --locked --no-deps --all-features
```

`cargo doc` runs with `RUSTDOCFLAGS=-D warnings`, so a broken intra-doc link
fails the build.

`--locked` is not decoration. `Cargo.lock` is committed, and CI builds exactly
what it pins; without it a release of any transitive crate can change the build
under you. That is not hypothetical — a `clap_lex` release that needed edition
2024 broke the minimum-version job while nothing in this repository had
changed.

A separate job compiles the crate on the oldest supported compiler. It reads
that version from `rust-version` in `Cargo.toml` rather than repeating it, so
raising the minimum is a one-line change. Raise it when the dependency graph
forces it — this finds the number:

```sh
cargo metadata --format-version 1 --all-features \
  | python3 -c 'import json,sys; print(max(p["rust_version"] for p in json.load(sys.stdin)["packages"] if p.get("rust_version")))'
```

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
2. Move the `Unreleased` entries in `CHANGELOG.md` under the new version, with
   the date, and add the two link definitions at the foot of the file.
3. Commit, then tag and push:

```sh
git tag -s vX.Y.Z -m "ratasm X.Y.Z"
git push origin main
git push origin vX.Y.Z
```

The release workflow verifies that the tag matches `Cargo.toml`, runs the full
checks, builds musl and glibc binaries, publishes a GitHub release with
checksums, the changelog entry as its notes and the install script, and pushes
to crates.io.

### The crates.io token

Publishing needs a `CARGO_REGISTRY_TOKEN` repository secret. Without it the
release still happens; only the crates.io step is skipped, with a warning.

1. Sign in at [crates.io](https://crates.io) with GitHub and **verify your
   email address** — the registry refuses to publish without one.
2. Account Settings → API Tokens → New Token. Scope it to the crates named
   `ratasm` and to the `publish-new` and `publish-update` endpoints; nothing
   else is needed, and a token that can only do this is a token worth much
   less if it leaks. Give it an expiry.
3. The token is shown once. Store it as a repository secret named exactly
   `CARGO_REGISTRY_TOKEN`:

```sh
gh secret set CARGO_REGISTRY_TOKEN --repo tuna4ll/ratasm
```

   or through Settings → Secrets and variables → Actions → New repository
   secret.

If a tag was released before the secret existed, re-run that release rather
than cutting a new version:

```sh
gh run rerun <run-id>              # the Release run for the tag
```

Publishing is idempotent: a version already on crates.io is reported and
skipped rather than failing the workflow. A published version can never be
replaced, so a mistake means a new version number, not a re-upload.

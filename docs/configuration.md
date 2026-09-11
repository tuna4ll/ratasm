# Configuration

## Project settings: `.ratasm.toml`

Placed in the project root, beside your source. Every field has a default, so
you only write down what differs from the defaults.

```toml
[project]
name = "hello"
entry = "src/main.asm"
architecture = "x86_64"
syntax = "nasm"

[build]
assembler = "nasm"
assembler_args = ["-f", "elf64"]
linker = "ld"
output_directory = "build"

[run]
args = []
timeout_ms = 5000
```

An empty file, or no file at all, gives exactly the defaults above.

### `[project]`

| Field | Default | Meaning |
| --- | --- | --- |
| `name` | `"ratasm-project"` | Display name. |
| `entry` | `"src/main.asm"` | The main source file. |
| `architecture` | `"x86_64"` | Target architecture. |
| `syntax` | `"nasm"` | Assembly dialect. |
| `sources` | `[]` | Extra sources assembled alongside the entry file. |
| `include_directories` | `[]` | Directories searched by `%include`. |

`architecture` accepts `x86_64` (also spelled `x86-64` or `amd64`), `aarch64`
and `riscv64`. Only `x86_64` is implemented; the others are recognised so that
a project naming one gets a clear "not supported yet" instead of a confusing
failure deep inside the assembler. The same applies to `syntax`, where `gas` is
recognised but not implemented.

### `[build]`

| Field | Default | Meaning |
| --- | --- | --- |
| `assembler` | `"nasm"` | Assembler executable. |
| `assembler_args` | `["-f", "elf64"]` | Arguments before the input file. |
| `linker` | `"ld"` | Linker executable. |
| `linker_args` | `[]` | Extra linker arguments. |
| `objects` | `[]` | Pre-built object files to link in. |
| `output_directory` | `"build"` | Where artefacts are written. |
| `executable` | *(from the entry file's name)* | Explicit output path. |

The default commands are exactly:

```sh
nasm -f elf64 source.asm -o source.o
ld source.o -o source
```

A debug build adds `-g`, unless your `assembler_args` already ask for debug
information — so configuring your own debug flags does not produce a duplicate
that some assembler versions reject.

### `[run]`

| Field | Default | Meaning |
| --- | --- | --- |
| `args` | `[]` | Arguments passed to your program. |
| `timeout_ms` | `5000` | Milliseconds before the program is killed. |
| `working_directory` | *(the project root)* | Where the program runs. |
| `stdin` | *(none)* | Text supplied on standard input. |

`timeout_ms = 0` disables the limit, for a program that is meant to keep going.

## Two rules the loader enforces

**Unknown fields are errors.** Writing `assember_args` by mistake fails at load
time rather than becoming a setting that silently does nothing.

**Paths cannot escape the project.** Absolute paths and paths containing `..`
are rejected, so a project file cannot direct writes to arbitrary locations on
your filesystem.

## Working without a project file

Opening a single `.asm` file with no `.ratasm.toml` anywhere above it works.
`ratasm` builds an implicit project: the file is the entry point, its directory
is the root, and everything else takes its default. Build, run and debug behave
identically either way.

`ratasm` searches for a project file in the file's directory and then upwards,
so a file deep inside a project still finds it.

## User settings: `~/.config/ratasm/config.toml`

Project settings describe *a project*; user settings describe *you*, and follow
you between projects. The file lives at `$XDG_CONFIG_HOME/ratasm/config.toml`,
falling back to `~/.config/ratasm/config.toml`. It is optional — most people
never write one.

```toml
[appearance]
theme = "dark"          # dark, light, ansi16, colorblind
# unicode = false       # omit to detect from the locale

[editor]
indent_width = 4
line_numbers = true
highlight_current_line = true
match_brackets = true
auto_close_pairs = true

[debugger]
gdb = "gdb"
timeout_ms = 10000
memory_window = 256
stack_depth = 16
record = true

[keys]
"F5" = "debugger.continue"
```

### `[appearance]`

| Field | Default | Meaning |
| --- | --- | --- |
| `theme` | `"dark"` | One of `dark`, `light`, `ansi16`, `colorblind`. |
| `unicode` | *(detected)* | Force the Unicode or the ASCII glyph set. |

### `[editor]`

| Field | Default | Meaning |
| --- | --- | --- |
| `indent_width` | `4` | Spaces inserted by one indent. |
| `line_numbers` | `true` | Show line numbers beside the source. |
| `highlight_current_line` | `true` | Tint the line the cursor is on. |
| `match_brackets` | `true` | Mark the bracket matching the one at the cursor. |
| `auto_close_pairs` | `true` | Typing `(`, `[`, `{`, `"`, `'` or `` ` `` inserts its partner. |

### `[debugger]`

| Field | Default | Meaning |
| --- | --- | --- |
| `gdb` | `"gdb"` | The debugger executable, looked up on `PATH`. |
| `timeout_ms` | `10000` | Milliseconds to wait for one GDB command. |
| `memory_window` | `256` | Bytes the memory panel reads at a time. |
| `stack_depth` | `16` | Stack slots shown around the stack pointer. |
| `record` | `true` | Record execution so it can be stepped backwards. |

Recording is what makes <kbd>Shift</kbd>+<kbd>F7</kbd> able to undo a step. It
costs time per instruction, which is invisible for the small programs ratasm is
for and would not be for a large one, so `record = false` turns it off.

### `[keys]`

Each entry rebinds one chord, by the command identifiers listed in
[keybindings.md](keybindings.md). An unknown command name is a load error
rather than a binding that quietly does nothing.

## Command-line options

| Option | Meaning |
| --- | --- |
| `--log-file FILE` | Write diagnostics to a file. |
| `--log-filter FILTER` | Log filter in `RUST_LOG` syntax. Default `info`. |

The TUI owns the terminal, so logs never go to the console. Without
`--log-file`, logging is off.

| Environment variable | Meaning |
| --- | --- |
| `RATASM_LOG` | Overrides `--log-filter`. |
| `RATASM_ASCII` | Set to any value to force ASCII-only glyphs. |
| `NO_COLOR` | Honoured by the install script. |

## Subcommands

| Command | What it does |
| --- | --- |
| `ratasm` | Open the interface on this directory's project. |
| `ratasm FILE...` | Open the interface with those files; the first is the one on screen. |
| `ratasm new NAME` | Create a project with a working hello-world program. |
| `ratasm build [PATH]` | Assemble and link, then exit. |
| `ratasm run [PATH]` | Build and run, then exit. |
| `ratasm doctor` | Report which external tools are installed. |

`build` and `run` exit non-zero when the build fails or the program does, so
they compose with Make and CI.

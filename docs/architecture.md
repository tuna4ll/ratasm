# Architecture

`ratasm` is organised so that four things stay independent: application state,
rendering, process management, and the debugger protocol. Every design choice
below follows from wanting each of them testable on its own.

## The dependency rule

```text
              ┌──────────┐
              │   app    │  owns state, applies commands
              └────┬─────┘
      ┌────────────┼────────────┬─────────────┐
      ▼            ▼            ▼             ▼
 ┌────────┐  ┌──────────┐  ┌─────────┐  ┌──────────┐
 │ editor │  │ debugger │  │assembler│  │ syscall  │
 └───┬────┘  └────┬─────┘  └────┬────┘  └──────────┘
     │            │             │
     └────────────┴──────┬──────┘
                         ▼
                ┌────────────────┐
                │  instruction   │  static ISA knowledge
                └────────────────┘
                         ▲
                    ┌────┴─────┐
                    │ process  │  external programs
                    └──────────┘

 ┌────┐  reads state, produces widgets, mutates nothing
 │ ui │◀──────────────────────────────────────────────
 └────┘
```

Arrows point at what a module depends on. Two rules hold:

1. **`ui` never mutates state.** It reads and draws. Everything that changes
   state goes through a `Command`, which means every state change is reachable
   from the command palette, a key binding and a test alike.
2. **Nothing below `app` knows about a terminal.** The editor tracks a scroll
   offset but has never heard of ratatui; `Document::scroll_into_view` is
   handed a height by the renderer rather than asking for one.

## Why `instruction` sits at the bottom

Register names, mnemonics, flags and condition codes are needed by the editor
(to highlight), the debugger (to display), the explainer (to describe) and the
syscall panel (to name argument registers). Putting that knowledge in one
dependency-free module means those four cannot disagree — and a test asserts
exactly that: the syscall database's argument registers are compared against
the register model's, so the two sources of the same fact cannot drift.

The one consequence worth knowing: `instruction` cannot use the editor's NASM
lexer, because the editor depends on `instruction`. The explainer therefore has
its own small statement splitter. That is a deliberate trade, documented where
it lives.

## Editing

The buffer has exactly one mutating primitive, `replace_range`. Insertion,
deletion, paste and search-and-replace are all expressed through it, and undo is
its exact inverse. If edits could reach the line vector by another route, the
history would drift out of sync with the text; funnelling everything through one
operation makes that impossible rather than merely unlikely.

Undo groups are closed explicitly by `History::seal` — on cursor movement, on
save — rather than by a timer, so grouping is deterministic and testable.
"Modified" is tracked by comparing state ids, not by a boolean, so undoing back
to the save point correctly clears the marker.

## The debugger

GDB is driven over its machine interface. One background task owns GDB's
standard output and parses each line into a typed `Record`; the session issues
one command at a time and waits for the result carrying its token. Records that
arrive meanwhile — a stop, a thread event, program output — are queued rather
than dropped.

Serialising commands removes a whole category of bug for no real cost: a
debugger UI is request/response by nature, so nothing is gained by having
several commands in flight, and correlating a shared pending-map across tasks
would be a source of races.

Legal state transitions live in one function, `state::next_state`. The UI asks
`can_step()` before offering the action, and the same table decides whether the
action succeeds, so the two cannot disagree — a test checks that for every
state and transition pair.

See [debugger.md](debugger.md) for the protocol details.

## Processes

Every external tool goes through `process::run`. Arguments are passed as a
vector to `execve`, never interpolated into a shell string, so a path with a
space or a semicolon is just a path. Both pipes are drained concurrently,
because waiting for exit while a pipe buffer fills is the classic hang. Every
run is bounded by a timeout, and the child is always reaped.

## Assets as data

Instruction semantics and the syscall table are JSON, embedded at build time.
Adding an instruction is a data change, not a code change, and the same data
drives the explainer, the flag panel and the learning mode — so they cannot
describe the same instruction differently.

The syscall numbers were generated from the kernel's own `asm/unistd_64.h`
rather than typed from memory. Entries whose arguments are not modelled are
marked `detailed: false` and carry nothing else; tests enforce that such an
entry invents no arguments, no return value and no example.

## Learning and the scratchpad

Both are modules that know nothing about a terminal. `learning` holds lessons
and questions as data with a `Progress` value describing where the reader is;
`scratchpad` takes some starting register values and one instruction and hands
back what changed.

The scratchpad does not model the instruction. It generates a small program
with the snippet bracketed by two labels, assembles it, and reads the registers
at both labels through GDB — so the answer comes from the processor rather than
from an emulator that could be wrong. That is also what makes the learning
material checkable: a test runs every question through the scratchpad and fails
if a stated answer disagrees with the machine.

## Testing

Around 940 tests, all runnable without a terminal. The parsers are exercised
from fixtures captured from real GDB and NASM output. Tests needing `gdb`,
`nasm` or `ld` skip themselves when those are absent, so the suite passes on a
bare machine while still covering the real thing when it is available.

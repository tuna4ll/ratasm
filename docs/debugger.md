# The debugger

`ratasm` drives GDB over its machine interface (MI), the line-based protocol
GDB exposes for front ends. This document covers what that means in practice
and the details that caused real bugs while it was built.

## Starting a session

```text
gdb --interpreter=mi3 --nx -q
```

`--nx` skips the user's `.gdbinit`. That matters: a personal init file can print
output that desynchronises the protocol, or change settings the front end
depends on.

## The protocol

Every line GDB writes is one record, identified by its first character:

| Prefix | Meaning |
| --- | --- |
| `^` | the answer to a command |
| `*` | the program's execution state changed |
| `=` | GDB's own state changed |
| `+` | progress on a long operation |
| `~` | what an interactive user would have seen |
| `@` | output from the program being debugged |
| `&` | GDB's internal log |
| `(gdb)` | the prompt, ending a response |

Commands carry a numeric token which GDB echoes on the matching result record.
That token is the only reliable way to match an answer to its question:
asynchronous events arrive interleaved with replies, so matching on order breaks
the first time the program hits a breakpoint while a command is in flight.

## Values are parsed, not split

MI values form a small recursive grammar of strings, tuples and lists. They are
parsed properly rather than split on delimiters, because GDB embeds arbitrary
program text inside them — a source line containing a comma, a brace or an
escaped quote is routine, and any approach based on `split(',')` corrupts it.

One shape has no equivalent in most data formats:

```text
stack=[frame={...},frame={...},frame={...}]
```

That is a list whose elements are *results sharing a key*. Represented as a map
it would keep only the last frame. `Value::elements_named` retrieves them all.

## Two details that caused bugs

**Program exit is a stop reason, not a record class.** GDB reports a finished
program as `*stopped,reason="exited-normally"` — an ordinary stop record.
Treating every `*stopped` as "paused and inspectable" leaves the debugger
waiting to step a process that no longer exists.

**Exit codes are octal.** A program exiting with status 9 is reported as
`exit-code="011"`. Reading that as decimal gives 11. Verified against GDB 17.2
and pinned by a test.

## The state machine

```text
 Idle ──build──► Building ──ok──► Ready ──launch──► Starting
                    │               ▲                  │
                  failed            │               started
                    ▼               │                  ▼
                 Failed ──reset─────┘            Running ⇄ Paused
                                                       │       │
                                                     exited────┘
                                                       ▼
                                                    Exited
```

Operations are only valid in particular states. Stepping a program that is not
running, or reading a register while the target executes, produces a confusing
error from GDB rather than a clear one from us. The legal transitions live in
one function; the UI asks `can_step()` before offering the action, and the same
table decides whether it succeeds.

A GDB process can die at any moment, from any state, so `force_failed` always
succeeds — refusing that transition would leave the UI claiming a session that
no longer exists.

## Breakpoints across sessions

Breakpoints are set in the editor before any session exists. GDB assigns numbers
only once it has loaded the program, and may reject or renumber them. So a
breakpoint carries an *optional* GDB number: it is pending until a session
adopts it, and pending breakpoints are replayed when one starts.

When a session ends, every number is forgotten. They belonged to that session
and mean nothing to the next one.

## Cleanup

GDB must never outlive `ratasm`, or it holds the target's process group and the
terminal with it. Three mechanisms cover that: `shutdown` for the orderly path,
`kill_on_drop` for panics and cancellation, and a bounded wait so a GDB that
refuses to exit is killed rather than waited on forever.

## Debugging the debugger

```sh
ratasm --log-file /tmp/ratasm.log --log-filter ratasm::debugger=debug
```

This records every command sent and every record received.

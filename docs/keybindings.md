# Keyboard shortcuts

## Pages

The interface is four pages, each holding the panels for one activity. The
page bar across the top names them, with the number that opens it:

| Key | Page | What is on it |
| --- | --- | --- |
| <kbd>Alt</kbd>+<kbd>1</kbd> | Code | Editor, project files and symbols, build output |
| <kbd>Alt</kbd>+<kbd>2</kbd> | Debug | Editor, disassembly, explanation, registers, flags, stack, output |
| <kbd>Alt</kbd>+<kbd>3</kbd> | Learn | Lessons and questions, the scratchpad, the explanation |
| <kbd>Alt</kbd>+<kbd>4</kbd> | Reference | System calls and instruction semantics |

<kbd>Tab</kbd> moves between the panels of the *current page*, except while a
live terminal has focus, where it is sent to the program. Starting a debug
session opens the Debug page, and so does stopping at a breakpoint — being
stopped is the one moment the machine state is unambiguously what you want.

Panels that share a slot, such as the stack and the memory view, have a small
tab strip above them naming the others.

## Defaults

### Running and debugging

| Key | Action |
| --- | --- |
| <kbd>F5</kbd> | Run, or continue when paused |
| <kbd>F6</kbd> | Build |
| <kbd>F7</kbd> | Step one instruction |
| <kbd>F8</kbd> | Step over |
| <kbd>F9</kbd> | Toggle a breakpoint; asks for the line of the active file |
| <kbd>F10</kbd> | Step one source line |
| <kbd>Ctrl</kbd>+<kbd>F5</kbd> | Stop the running program |
| <kbd>F11</kbd> | Step out of the current call |
| <kbd>F12</kbd> | Start a debug session |
| <kbd>Shift</kbd>+<kbd>F5</kbd> | Continue *backwards* |
| <kbd>Shift</kbd>+<kbd>F7</kbd> | Step one instruction backwards |
| <kbd>Shift</kbd>+<kbd>F8</kbd> | Step backwards over a call |

`F7` and `F10` differ in a way worth internalising: `F7` steps one *machine
instruction*, `F10` steps one *source line*, which may be several instructions.
While learning, `F7` is usually what you want.

The <kbd>Shift</kbd> keys undo a step. They need GDB's process recording, which
ratasm turns on when a session starts (`debugger.record` in `.ratasm.toml`
switches it off). Stepping back past the start of the recording is refused
rather than guessed at.

### The interactive Output terminal

<kbd>F5</kbd> builds and runs the program in the Output panel. While that panel
has focus, ordinary keys, control characters such as <kbd>Ctrl</kbd>+<kbd>C</kbd>,
arrow and function keys, Unicode text and pasted text go to the program. ANSI
colour, cursor movement, alternate-screen programs and terminal resizing are
interpreted as they would be in a standalone terminal.

Application navigation remains available: <kbd>Alt</kbd>+<kbd>1</kbd> through
<kbd>4</kbd> leaves the terminal for another page,
<kbd>Ctrl</kbd>+<kbd>P</kbd> opens the command palette, and
<kbd>Ctrl</kbd>+<kbd>F5</kbd> stops the program. The same terminal is connected
to a program running under GDB, and the interface remains responsive while the
inferior waits for input.

### Learning and experimenting

| Key | Action |
| --- | --- |
| <kbd>F1</kbd> | Learning panel: lessons and questions |
| <kbd>F2</kbd> | Scratchpad: try one instruction |
| <kbd>F3</kbd> | Show the complete status message in Output |

In the learning panel, <kbd>←</kbd> and <kbd>→</kbd> move through the material,
<kbd>?</kbd> jumps to the questions, and on a question you type a value and
press <kbd>Enter</kbd>. A wrong answer stays put and shows the worked
explanation.

In the scratchpad, you type an instruction and press <kbd>Enter</kbd> to run
it. A line of the form `rax=0x10` sets a starting value instead of being
assembled, and `rax=` removes it again. <kbd>Tab</kbd> still leaves the panel.

### Files

| Key | Action |
| --- | --- |
| <kbd>Ctrl</kbd>+<kbd>E</kbd> | Open `$EDITOR` on the active file in the Editor panel |
| <kbd>Ctrl</kbd>+<kbd>N</kbd> | Name a new file and create it in `$EDITOR` |
| <kbd>Ctrl</kbd>+<kbd>O</kbd> | Open |
| <kbd>Ctrl</kbd>+<kbd>W</kbd> | Close the file |
| <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>A</kbd> | Add this file to the project's sources |
| <kbd>Tab</kbd> | Complete the path, inside an open or new-file prompt |
| <kbd>Ctrl</kbd>+<kbd>Q</kbd> | Quit |

### Navigation

| Key | Action |
| --- | --- |
| <kbd>Ctrl</kbd>+<kbd>P</kbd> | Command palette |
| <kbd>Ctrl</kbd>+<kbd>G</kbd> | Show an address in the memory panel |
| <kbd>Ctrl</kbd>+<kbd>K</kbd> | Syscall finder |
| <kbd>Tab</kbd> | Next panel on this page |
| <kbd>Shift</kbd>+<kbd>Tab</kbd> | Previous panel |

### Scrolling a panel

Every panel but the Editor and the scratchpad holds a list that is often taller
than the room it has. A scrollbar down the right edge appears when there is more
than fits.

| Key | Action |
| --- | --- |
| <kbd>↑</kbd> / <kbd>↓</kbd> | Scroll the focused panel |
| <kbd>PgUp</kbd> / <kbd>PgDn</kbd> | Scroll a screenful |
| <kbd>Home</kbd> / <kbd>End</kbd> | First row / last screenful |

Completed output keeps showing its newest line. Scroll up and it stays where
you put it; <kbd>End</kbd> makes it follow again. A live terminal instead uses
the navigation keys itself, then becomes an ordinary scrollable log when the
program exits.

### The explorer

| Key | Action |
| --- | --- |
| <kbd>↑</kbd> / <kbd>↓</kbd> | Move between the project's files |
| <kbd>Enter</kbd> | Open the selected file |
| <kbd>R</kbd> | Re-read the file list from disk |

The list is the project's sources plus any other assembly file under the root,
so a file that exists but is not in the build is visible rather than something
you have to remember. Files the build does not know about are marked; add one
with <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>A</kbd>.

### The mouse

| Action | Result |
| --- | --- |
| Wheel | Scrolls whatever is under the pointer, without taking focus |
| Click a panel | Focuses it |
| Click a page name | Opens that page |
| Click a file name | Switches to that buffer |
| Click a tab | Focuses that panel |
| Click a file in the explorer | Opens it |

Mouse reporting means the terminal's own text selection is off while ratasm is
running. Hold <kbd>Shift</kbd> while dragging to select and copy the way the
terminal normally would; most terminals reserve that for exactly this.

### Editing

ratasm does not edit text itself. The Editor panel runs your own editor inside
it: `$VISUAL`, then `$EDITOR`, then `vi`, started on the project's entry file
as soon as ratasm opens. While the panel has focus every key goes to the
editor, <kbd>Tab</kbd> and <kbd>Ctrl</kbd> chords included, except the
function keys and <kbd>Alt</kbd>+<kbd>1</kbd>..<kbd>4</kbd>, which stay with
ratasm so building, running, debugging and changing page still work. Click
another panel to leave it.

The command goes through `sh -c`, so a value such as `emacsclient -t` works.
Editors known to take `+LINE` (vim, neovim, nano, emacs, micro, kakoune and a
few more) open at the line the listing was scrolled to; any other editor is
given just the file.
When the editor exits, ratasm reads the file back. A build reads every open
file again first, so what is assembled is always what is on disk.

With the editor closed, or during a debug session, the panel shows a plain
listing of the source with its breakpoints and the line the program stopped
on. It has no cursor and nothing in it can be typed into; search, go to line
and the like are your editor's job.

| Key | Action |
| --- | --- |
| <kbd>Enter</kbd> / <kbd>Ctrl</kbd>+<kbd>E</kbd> | Open `$EDITOR` in the panel again |
| <kbd>↑</kbd> / <kbd>↓</kbd>, <kbd>PgUp</kbd> / <kbd>PgDn</kbd> | Scroll the listing |
| <kbd>Home</kbd> / <kbd>End</kbd> | First / last line |

Quitting ratasm while the editor still runs asks for a second Quit, since that
kills the editor and anything it has not saved.

## The command palette

<kbd>Ctrl</kbd>+<kbd>P</kbd> opens a fuzzy-searchable list of every command.
Anything reachable by a key binding is reachable here, so a shortcut you have
not memorised is never a dead end — and every command shows its own binding, so
the palette teaches them.

## Configuring

Bindings live in the `[keys]` section of your configuration file
(`~/.config/ratasm/config.toml`), naming a command by its identifier:

```toml
[keys]
"F5" = "debug.continue"
"ctrl+s" = "file.edit"
"ctrl+shift+p" = "app.palette"
```

Key names are case-insensitive. Modifiers are `ctrl`, `alt` and `shift`, joined
with `+`. Named keys are `F1`-`F12`, `enter`, `tab`, `backspace`, `delete`,
`insert`, `home`, `end`, `pageup`, `pagedown`, `up`, `down`, `left`, `right`
and `esc`.

Conflicting bindings are reported at start-up rather than one silently shadowing
the other.

Run `ratasm` and open the command palette to see the full list of command names.

## Command identifiers

Every identifier accepted in `[keys]`. A test keeps this list in step with the
code, so a command missing here is a build failure rather than a surprise.

| Identifier | Command |
| --- | --- |
| `file.new` | New file |
| `file.edit` | Edit in $EDITOR |
| `file.open` | Open file |
| `file.add-to-project` | Add to project |
| `file.close` | Close file |
| `app.quit` | Quit |
| `navigate.next-panel` | Next panel |
| `navigate.previous-panel` | Previous panel |
| `navigate.next-document` | Next document |
| `navigate.previous-document` | Previous document |
| `navigate.scroll-up` | Scroll up |
| `navigate.scroll-down` | Scroll down |
| `navigate.scroll-page-up` | Scroll up a page |
| `navigate.scroll-page-down` | Scroll down a page |
| `navigate.scroll-to-top` | Scroll to the top |
| `navigate.scroll-to-end` | Scroll to the end |
| `navigate.go-to-address` | Go to address |
| `navigate.go-to-first-error` | Go to first error |
| `build.build` | Build |
| `build.build-debug` | Build with debug info |
| `build.run` | Run |
| `build.stop` | Stop the program |
| `debug.start` | Start debugging |
| `debug.continue` | Continue |
| `debug.interrupt` | Interrupt |
| `debug.step-instruction` | Step instruction |
| `debug.step-over` | Step over |
| `debug.step-line` | Step line |
| `debug.step-out` | Step out |
| `debug.step-back` | Step back |
| `debug.step-back-over` | Step back over |
| `debug.reverse-continue` | Run backwards |
| `debug.stop` | Stop debugging |
| `debug.toggle-breakpoint` | Toggle breakpoint |
| `debug.clear-breakpoints` | Clear all breakpoints |
| `view.cycle-register-format` | Change register format |
| `view.toggle-disassembly-syntax` | Toggle Intel/AT&T syntax |
| `view.cycle-theme` | Change theme |
| `view.toggle-learning-mode` | Toggle learning mode |
| `app.palette` | Command palette |
| `app.syscalls` | Find a system call |
| `app.scratchpad` | Open scratchpad |
| `app.keybindings` | Show keyboard shortcuts |
| `app.status-details` | Show status details |
| `navigate.page.code` | Code page |
| `navigate.page.debug` | Debug page |
| `navigate.page.learn` | Learn page |
| `navigate.page.reference` | Reference page |
| `navigate.focus.editor` | Focus editor |
| `navigate.focus.registers` | Focus registers |
| `navigate.focus.flags` | Focus flags |
| `navigate.focus.stack` | Focus stack |
| `navigate.focus.call-stack` | Focus call stack |
| `navigate.focus.memory` | Focus memory |
| `navigate.focus.disassembly` | Focus disassembly |
| `navigate.focus.breakpoints` | Focus breakpoints |
| `navigate.focus.output` | Focus output |
| `navigate.focus.explain` | Focus explain |
| `navigate.focus.syscalls` | Focus syscalls |
| `navigate.focus.explorer` | Focus explorer |
| `navigate.focus.scratchpad` | Focus scratchpad |
| `navigate.focus.learn` | Focus learn |

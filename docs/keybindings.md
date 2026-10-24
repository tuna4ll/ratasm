# Keyboard shortcuts

## Pages

ratasm has two pages, and a study mode with two more.

| Key | Page | What is on it |
| --- | --- | --- |
| <kbd>Alt</kbd>+<kbd>1</kbd> | Code | Editor, project files and symbols, build output |
| <kbd>Alt</kbd>+<kbd>2</kbd> | Debug | Editor, registers and flags, stack, disassembly, explanation, output |
| <kbd>F1</kbd> | Learn and Reference | Lessons, questions and the scratchpad; system calls and instructions |

In study mode the page bar reads *1 Learn  2 Reference*, and
<kbd>Alt</kbd>+<kbd>1</kbd> and <kbd>2</kbd> move between those two;
<kbd>F1</kbd> again returns to the page you came from.

<kbd>Tab</kbd> moves between the panels of the page, except in the editor, where
it indents. <kbd>Alt</kbd>+<kbd>←</kbd> / <kbd>→</kbd> move between panels from
anywhere, and pressing the number of the page you are on moves to its next
panel. Starting a debug session opens the Debug page, and so does stopping at
a breakpoint.

## Defaults

### Files

| Key | Action |
| --- | --- |
| <kbd>Ctrl</kbd>+<kbd>N</kbd> | New file: name it, and it is created in the project |
| <kbd>Ctrl</kbd>+<kbd>O</kbd> | Open a file; <kbd>Tab</kbd> completes the path |
| <kbd>Ctrl</kbd>+<kbd>S</kbd> / <kbd>Alt</kbd>+<kbd>S</kbd> | Save / save all |
| <kbd>Ctrl</kbd>+<kbd>W</kbd> | Close the file, asking about unsaved changes |
| <kbd>Ctrl</kbd>+<kbd>PgUp</kbd> / <kbd>PgDn</kbd> | Previous / next open file |
| <kbd>Ctrl</kbd>+<kbd>Q</kbd> | Quit, asking about unsaved changes |

Building saves every modified file first. A file changed on disk by something
else is reloaded when it has no unsaved changes; when it has some, they are
kept and the status bar says so.

### Editing

| Key | Action |
| --- | --- |
| <kbd>Ctrl</kbd>+<kbd>Z</kbd> / <kbd>Ctrl</kbd>+<kbd>Y</kbd> | Undo / redo |
| <kbd>Ctrl</kbd>+<kbd>C</kbd> / <kbd>X</kbd> / <kbd>V</kbd> | Copy, cut, paste; with nothing selected, the whole line |
| <kbd>Ctrl</kbd>+<kbd>A</kbd> | Select all |
| <kbd>Shift</kbd>+arrows | Select |
| <kbd>Ctrl</kbd>+<kbd>←</kbd> / <kbd>→</kbd> | Move by word |
| <kbd>Ctrl</kbd>+<kbd>Backspace</kbd> / <kbd>Delete</kbd> | Delete a word |
| <kbd>Tab</kbd> / <kbd>Shift</kbd>+<kbd>Tab</kbd> | Indent / dedent the line or selection |
| <kbd>Enter</kbd> | New line, keeping the indentation and indenting after a label |

Brackets and quotes close themselves; set `editor.auto_close_pairs = false` to
stop that. Copying reaches the system clipboard through OSC 52 where the
terminal allows it.

### Finding your way

| Key | Action |
| --- | --- |
| <kbd>Ctrl</kbd>+<kbd>P</kbd> | Command palette |
| <kbd>Ctrl</kbd>+<kbd>F</kbd> | Find in the file; <kbd>F3</kbd> / <kbd>Shift</kbd>+<kbd>F3</kbd> next / previous |
| <kbd>Ctrl</kbd>+<kbd>R</kbd> | Replace every match |
| <kbd>Alt</kbd>+<kbd>F</kbd> | Search every file of the project |
| <kbd>Ctrl</kbd>+<kbd>T</kbd> | Go to a symbol anywhere in the project |
| <kbd>Ctrl</kbd>+<kbd>D</kbd> | Go to the definition of the symbol under the cursor |
| <kbd>Ctrl</kbd>+<kbd>G</kbd> | Go to a line |

### Building and running

| Key | Action |
| --- | --- |
| <kbd>F6</kbd> | Build: the project's Makefile when it has one, otherwise nasm and ld |
| <kbd>F5</kbd> | Build and run in the Output terminal; continue when paused |
| <kbd>Ctrl</kbd>+<kbd>F5</kbd> | Stop the program or the debug session |

A failed build puts the cursor on the first error, and every error and warning
is shown beside its line. *Run a make target* in the palette lists the
Makefile's targets, such as `clean` or `run`, and runs the one you pick.

### Debugging

| Key | Action |
| --- | --- |
| <kbd>F12</kbd> / <kbd>Shift</kbd>+<kbd>F12</kbd> | Start / stop a debug session |
| <kbd>F9</kbd> | Toggle a breakpoint on the cursor's line |
| <kbd>F7</kbd> | Step one instruction |
| <kbd>F8</kbd> | Step over a call |
| <kbd>F10</kbd> | Step one source line |
| <kbd>F11</kbd> | Step out of the current call |
| <kbd>Shift</kbd>+<kbd>F5</kbd> | Continue backwards |
| <kbd>Shift</kbd>+<kbd>F7</kbd> / <kbd>F8</kbd> | Step backwards |

`F7` steps one *machine instruction*, `F10` one *source line*, which may be
several. The <kbd>Shift</kbd> keys need GDB's process recording, which ratasm
turns on when a session starts (`debugger.record` in the configuration
switches it off).

In the Breakpoints panel, <kbd>Space</kbd> enables or disables,
<kbd>D</kbd> removes, <kbd>C</kbd> sets a condition and <kbd>W</kbd> adds a
watchpoint. *Set a register* in the palette writes one while paused.

### The Output terminal

<kbd>F5</kbd> runs the program in the Output panel. While that panel has focus,
ordinary keys, control characters such as <kbd>Ctrl</kbd>+<kbd>C</kbd>, arrow
and function keys, Unicode text and pasted text go to the program. ANSI colour,
cursor movement, alternate-screen programs and resizing work as in a
standalone terminal. <kbd>Alt</kbd>+<kbd>1</kbd> / <kbd>2</kbd>,
<kbd>Alt</kbd>+arrows, <kbd>Ctrl</kbd>+<kbd>P</kbd> and
<kbd>Ctrl</kbd>+<kbd>F5</kbd> stay with ratasm. The same terminal is attached
to a program running under GDB.

### Study mode

In the Learn panel, <kbd>←</kbd> and <kbd>→</kbd> move through the material,
<kbd>?</kbd> jumps to the questions, and on a question you type a value and
press <kbd>Enter</kbd>. A wrong answer stays put and shows the worked
explanation.

In the scratchpad you type an instruction and press <kbd>Enter</kbd> to run it.
A line such as `rax=0x10` sets a starting value instead, and `rax=` removes it.

### Scrolling a panel

| Key | Action |
| --- | --- |
| <kbd>↑</kbd> / <kbd>↓</kbd> | Scroll the focused panel |
| <kbd>PgUp</kbd> / <kbd>PgDn</kbd> | Scroll a screenful |
| <kbd>Home</kbd> / <kbd>End</kbd> | First row / last screenful |

Completed output keeps showing its newest line. Scroll up and it stays where
you put it; <kbd>End</kbd> makes it follow again.

### The explorer

| Key | Action |
| --- | --- |
| <kbd>↑</kbd> / <kbd>↓</kbd> | Move between the project's files |
| <kbd>Enter</kbd> | Open the selected file |
| <kbd>N</kbd> | Create a file |
| <kbd>M</kbd> | Rename or move the selected file |
| <kbd>D</kbd> / <kbd>Delete</kbd> | Delete the selected file, after typing `yes` |
| <kbd>A</kbd> | Add the selected file to the sources in `.ratasm.toml` |
| <kbd>R</kbd> | Re-read the file list |

The list holds every source, include, Makefile and note in the project, and
follows files created or removed outside ratasm. Renaming or deleting a source
named in `.ratasm.toml` updates that file too, and breakpoints follow the file.

### The mouse

| Action | Result |
| --- | --- |
| Wheel | Scrolls whatever is under the pointer |
| Click a panel | Focuses it |
| Click a page name or file name | Opens it |
| Click in the editor | Moves the cursor |
| Click a file in the explorer | Opens it |

Hold <kbd>Shift</kbd> while dragging to select with the terminal's own
selection.

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
"ctrl+e" = "build.make-target"
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
| `file.open` | Open file |
| `file.save` | Save |
| `file.save-as` | Save as |
| `file.save-all` | Save all |
| `file.add-to-project` | Add to project |
| `file.close` | Close file |
| `file.rename` | Rename file |
| `file.delete` | Delete file |
| `app.quit` | Quit |
| `edit.undo` | Undo |
| `edit.redo` | Redo |
| `edit.select-all` | Select all |
| `edit.copy` | Copy |
| `edit.cut` | Cut |
| `edit.paste` | Paste |
| `edit.indent` | Indent |
| `edit.dedent` | Dedent |
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
| `navigate.go-to-line` | Go to line |
| `navigate.go-to-address` | Go to address |
| `navigate.go-to-definition` | Go to definition |
| `navigate.go-to-first-error` | Go to first error |
| `navigate.go-to-symbol` | Go to symbol |
| `navigate.search` | Search in files |
| `search.find` | Find |
| `search.next` | Find next |
| `search.previous` | Find previous |
| `search.replace` | Replace |
| `build.build` | Build |
| `build.build-debug` | Build with debug info |
| `build.run` | Run |
| `build.stop` | Stop the program |
| `build.make-target` | Run a make target |
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
| `debug.watch` | Watch an expression |
| `debug.condition` | Set breakpoint condition |
| `debug.set-register` | Set a register |
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

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

<kbd>Tab</kbd> moves between the panels of the *current page*, so it stays a
navigation key rather than a search through fourteen panels. Starting a debug
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
| <kbd>F9</kbd> | Toggle breakpoint on the current line |
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

### Learning and experimenting

| Key | Action |
| --- | --- |
| <kbd>F1</kbd> | Learning panel: lessons and questions |
| <kbd>F2</kbd> | Scratchpad: try one instruction |

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
| <kbd>Ctrl</kbd>+<kbd>S</kbd> | Save |
| <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>S</kbd> | Save every modified buffer |
| <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>A</kbd> | Add this file to the project's sources |
| <kbd>Ctrl</kbd>+<kbd>O</kbd> | Open |
| <kbd>Tab</kbd> | Complete the path, inside an open or save prompt |
| <kbd>Ctrl</kbd>+<kbd>Q</kbd> | Quit |

### Navigation and search

| Key | Action |
| --- | --- |
| <kbd>Ctrl</kbd>+<kbd>P</kbd> | Command palette |
| <kbd>Ctrl</kbd>+<kbd>F</kbd> | Search |
| <kbd>Ctrl</kbd>+<kbd>G</kbd> | Go to line, or to an address |
| <kbd>Ctrl</kbd>+<kbd>K</kbd> | Syscall finder |
| <kbd>Ctrl</kbd>+<kbd>D</kbd> | Go to definition, or open the file an `%include` names |
| <kbd>Tab</kbd> | Next panel on this page (indents inside the editor) |
| <kbd>Shift</kbd>+<kbd>Tab</kbd> | Previous panel (dedents inside the editor) |

### Scrolling a panel

Every panel but the editor and the scratchpad holds a list that is often taller
than the room it has. A scrollbar down the right edge appears when there is more
than fits.

| Key | Action |
| --- | --- |
| <kbd>↑</kbd> / <kbd>↓</kbd> | Scroll the focused panel |
| <kbd>PgUp</kbd> / <kbd>PgDn</kbd> | Scroll a screenful |
| <kbd>Home</kbd> / <kbd>End</kbd> | First row / last screenful |

The output panel keeps showing its newest line as a build writes to it. Scroll
up and it stays where you put it; <kbd>End</kbd> makes it follow again.

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
| Click a panel | Focuses it; in the editor, moves the cursor |
| Click a page name | Opens that page |
| Click a file name | Switches to that buffer |
| Click a tab | Focuses that panel |
| Click a file in the explorer | Opens it |

Mouse reporting means the terminal's own text selection is off while ratasm is
running. Hold <kbd>Shift</kbd> while dragging to select and copy the way the
terminal normally would; most terminals reserve that for exactly this.

### Editing

| Key | Action |
| --- | --- |
| <kbd>Ctrl</kbd>+<kbd>Z</kbd> | Undo |
| <kbd>Ctrl</kbd>+<kbd>Y</kbd> | Redo |
| <kbd>Ctrl</kbd>+<kbd>A</kbd> | Select all |
| <kbd>Home</kbd> | First non-blank character, then column zero |
| <kbd>Ctrl</kbd>+<kbd>←</kbd> / <kbd>→</kbd> | Word left / right |

Inside the editor, <kbd>Tab</kbd> indents rather than changing panel — an
editor that cannot insert an indent is not much of an editor. To leave the
editor by keyboard use <kbd>Alt</kbd> plus a digit, which focuses a panel
directly from anywhere.

<kbd>Home</kbd> is deliberately two-stage: pressing it once goes to the start of
the code, pressing it again to the start of the line. Indented assembly makes
the first far more useful than the second.

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
"ctrl+s" = "file.save"
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
| `file.save-all` | Save all |
| `file.add-to-project` | Add to project |
| `file.save-as` | Save as |
| `file.close` | Close file |
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
| `search.find` | Find |
| `search.next` | Find next |
| `search.previous` | Find previous |
| `search.replace` | Replace |
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

# Keyboard shortcuts

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

`F7` and `F10` differ in a way worth internalising: `F7` steps one *machine
instruction*, `F10` steps one *source line*, which may be several instructions.
While learning, `F7` is usually what you want.

### Files

| Key | Action |
| --- | --- |
| <kbd>Ctrl</kbd>+<kbd>S</kbd> | Save |
| <kbd>Ctrl</kbd>+<kbd>O</kbd> | Open |
| <kbd>Ctrl</kbd>+<kbd>Q</kbd> | Quit |

### Navigation and search

| Key | Action |
| --- | --- |
| <kbd>Ctrl</kbd>+<kbd>P</kbd> | Command palette |
| <kbd>Ctrl</kbd>+<kbd>F</kbd> | Search |
| <kbd>Ctrl</kbd>+<kbd>G</kbd> | Go to line, or to an address |
| <kbd>Ctrl</kbd>+<kbd>K</kbd> | Syscall finder |
| <kbd>Tab</kbd> | Next panel (indents inside the editor) |
| <kbd>Shift</kbd>+<kbd>Tab</kbd> | Previous panel (dedents inside the editor) |
| <kbd>Alt</kbd>+<kbd>1</kbd>…<kbd>9</kbd> | Focus a panel directly |

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

Bindings live in the `[keys]` section of your configuration file, naming a
command:

```toml
[keys]
"F5" = "debugger.continue"
"ctrl+s" = "file.save"
"ctrl+shift+p" = "palette.open"
```

Key names are case-insensitive. Modifiers are `ctrl`, `alt` and `shift`, joined
with `+`. Named keys are `F1`-`F12`, `enter`, `tab`, `backspace`, `delete`,
`insert`, `home`, `end`, `pageup`, `pagedown`, `up`, `down`, `left`, `right`
and `esc`.

Conflicting bindings are reported at start-up rather than one silently shadowing
the other.

Run `ratasm` and open the command palette to see the full list of command names.

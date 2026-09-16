//! The in-memory terminal displayed in the Output panel.

use crate::process::pty::PtySize;

/// A VT-compatible screen fed by a program running on a pseudo-terminal.
pub struct TerminalScreen {
    parser: vt100::Parser,
    size: PtySize,
}

impl TerminalScreen {
    /// Creates a blank screen with a bounded scrollback history.
    pub fn new(size: PtySize) -> Self {
        Self {
            parser: vt100::Parser::new(size.rows, size.columns, 10_000),
            size,
        }
    }

    /// Applies bytes emitted by the child process.
    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// Changes the screen geometry while preserving its contents.
    pub fn resize(&mut self, size: PtySize) {
        if self.size == size {
            return;
        }
        self.parser.set_size(size.rows, size.columns);
        self.size = size;
    }

    /// The current geometry.
    pub fn size(&self) -> PtySize {
        self.size
    }

    /// The parsed VT screen used for rendering and input modes.
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Converts the visible screen and its scrollback to plain log lines.
    pub fn into_lines(mut self) -> Vec<String> {
        self.parser.set_scrollback(usize::MAX);
        let history = self.parser.screen().scrollback();
        let mut lines = Vec::with_capacity(history + usize::from(self.size.rows));

        for offset in (1..=history).rev() {
            self.parser.set_scrollback(offset);
            if let Some(line) = self.parser.screen().rows(0, self.size.columns).next() {
                lines.push(line);
            }
        }

        self.parser.set_scrollback(0);
        lines.extend(self.parser.screen().rows(0, self.size.columns));
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_sequences_update_the_screen_instead_of_leaking_into_text() {
        let mut terminal = TerminalScreen::new(PtySize::new(12, 3));
        terminal.process(b"one\r\ntwo\x1b[1A\rTWO");

        let lines = terminal.into_lines();
        assert_eq!(lines, ["TWO", "two"]);
    }

    #[test]
    fn completed_output_keeps_scrollback() {
        let mut terminal = TerminalScreen::new(PtySize::new(8, 2));
        terminal.process(b"one\r\ntwo\r\nthree\r\n");

        let lines = terminal.into_lines();
        assert!(lines.iter().any(|line| line == "one"), "{lines:?}");
        assert!(lines.iter().any(|line| line == "three"), "{lines:?}");
    }
}

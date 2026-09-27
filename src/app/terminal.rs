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

    /// Applies bytes emitted by the child process, returning what a real
    /// terminal would answer to the queries among them.
    ///
    /// Programs such as editors ask for the cursor position and wait for the
    /// reply; left unanswered they time out and quit. Each query is answered
    /// with the state as it stood when the query arrived.
    pub fn process(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut replies = Vec::new();
        let mut rest = bytes;
        while let Some((start, query)) = next_query(rest) {
            self.parser.process(&rest[..start]);
            replies.extend(self.answer(query));
            rest = &rest[start + query.len()..];
        }
        self.parser.process(rest);
        replies
    }

    /// The reply to one recognised query.
    fn answer(&self, query: &[u8]) -> Vec<u8> {
        match query {
            b"\x1b[6n" | b"\x1b[?6n" => {
                let (row, column) = self.parser.screen().cursor_position();
                let private = if query[2] == b'?' { "?" } else { "" };
                format!("\x1b[{private}{};{}R", row + 1, column + 1).into_bytes()
            }
            b"\x1b[5n" => b"\x1b[0n".to_vec(),
            b"\x1b[>c" | b"\x1b[>0c" => b"\x1b[>0;0;0c".to_vec(),
            _ => b"\x1b[?62;22c".to_vec(),
        }
    }

    /// Changes the screen geometry while preserving its contents.
    pub fn resize(&mut self, size: PtySize) {
        if self.size == size {
            return;
        }
        self.parser.screen_mut().set_size(size.rows, size.columns);
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
        self.parser.screen_mut().set_scrollback(usize::MAX);
        let history = self.parser.screen().scrollback();
        let mut lines = Vec::with_capacity(history + usize::from(self.size.rows));

        for offset in (1..=history).rev() {
            self.parser.screen_mut().set_scrollback(offset);
            if let Some(line) = self.parser.screen().rows(0, self.size.columns).next() {
                lines.push(line);
            }
        }

        self.parser.screen_mut().set_scrollback(0);
        lines.extend(self.parser.screen().rows(0, self.size.columns));
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        lines
    }
}

/// Queries answered by [`TerminalScreen::process`]: cursor position, status
/// and primary and secondary device attributes.
const QUERIES: [&[u8]; 7] = [
    b"\x1b[6n",
    b"\x1b[?6n",
    b"\x1b[5n",
    b"\x1b[c",
    b"\x1b[0c",
    b"\x1b[>c",
    b"\x1b[>0c",
];

/// The first query in `bytes` and where it starts.
fn next_query(bytes: &[u8]) -> Option<(usize, &'static [u8])> {
    (0..bytes.len()).find_map(|start| {
        QUERIES
            .iter()
            .find(|query| bytes[start..].starts_with(query))
            .map(|query| (start, *query))
    })
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
    fn a_cursor_position_query_is_answered_where_it_was_asked() {
        let mut terminal = TerminalScreen::new(PtySize::new(20, 5));
        let reply = terminal.process(b"ab\r\ncd\x1b[6nmore text");
        assert_eq!(reply, b"\x1b[2;3R");
        assert_eq!(terminal.screen().contents(), "ab\ncdmore text");
    }

    #[test]
    fn status_and_device_queries_are_answered() {
        let mut terminal = TerminalScreen::new(PtySize::new(20, 5));
        assert_eq!(terminal.process(b"\x1b[5n"), b"\x1b[0n");
        assert!(terminal.process(b"\x1b[c").starts_with(b"\x1b[?"));
        assert!(terminal.process(b"\x1b[>c").starts_with(b"\x1b[>"));
        assert!(terminal.process(b"plain output").is_empty());
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

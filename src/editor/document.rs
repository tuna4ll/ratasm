//! An open file as the source listing shows it: its text and how far it is scrolled.
//!
//! There is no cursor and no selection. Text is edited in `$EDITOR`; this is
//! only what ratasm shows when the editor is closed or a debug session wants
//! the line the program stopped on.

use std::path::{Path, PathBuf};

use super::buffer::TextBuffer;

/// One open file and the part of it on show.
#[derive(Debug, Clone)]
pub struct Document {
    buffer: TextBuffer,
    scroll_line: usize,
    /// A line to bring into view the next time the listing's height is known.
    reveal: Option<usize>,
}

impl Document {
    /// Creates an empty, untitled document.
    pub fn new() -> Self {
        Self {
            buffer: TextBuffer::new(),
            scroll_line: 0,
            reveal: None,
        }
    }

    /// Creates a document from text loaded from `path`.
    pub fn from_file_contents(path: impl Into<PathBuf>, text: &str) -> Self {
        let mut document = Self::new();
        document.buffer = TextBuffer::from_file_contents(path, text);
        document
    }

    /// Creates a document from text with no associated file.
    pub fn from_text(text: &str) -> Self {
        let mut document = Self::new();
        document.buffer = TextBuffer::from_text(text);
        document
    }

    /// The text buffer.
    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    /// The path this document is associated with, if any.
    pub fn path(&self) -> Option<&Path> {
        self.buffer.path()
    }

    /// The name shown in the tab bar.
    pub fn display_name(&self) -> String {
        self.buffer.display_name()
    }

    /// Associates the document with a path.
    pub fn set_path(&mut self, path: impl Into<PathBuf>) {
        self.buffer.set_path(path);
    }

    /// The first visible line, zero-based.
    pub fn scroll_line(&self) -> usize {
        self.scroll_line
    }

    /// Scrolls by `rows`, negative for up, staying inside the text.
    pub fn scroll_by(&mut self, rows: isize) {
        let last = self.buffer.line_count().saturating_sub(1);
        self.scroll_line = self.scroll_line.saturating_add_signed(rows).min(last);
        self.reveal = None;
    }

    /// Asks for the zero-based `line` to be brought into view.
    pub fn reveal(&mut self, line: usize) {
        self.reveal = Some(line.min(self.buffer.line_count().saturating_sub(1)));
    }

    /// Settles the scroll for a listing `height` rows tall, showing a line
    /// asked for with [`Self::reveal`] a third of the way down.
    pub fn fit(&mut self, height: usize) {
        if let Some(line) = self.reveal.take() {
            let visible = self.scroll_line..self.scroll_line + height;
            if height > 0 && !visible.contains(&line) {
                self.scroll_line = line.saturating_sub(height / 3);
            }
        }
        let last = self.buffer.line_count().saturating_sub(1);
        self.scroll_line = self.scroll_line.min(last);
    }

    /// Replaces the text with `text` as it now reads on disk, keeping the
    /// scroll position as close as the new text allows.
    pub fn reload(&mut self, text: &str) {
        let path = self.buffer.path().map(Path::to_path_buf);
        self.buffer = match path {
            Some(path) => TextBuffer::from_file_contents(path, text),
            None => TextBuffer::from_text(text),
        };
        let last = self.buffer.line_count().saturating_sub(1);
        self.scroll_line = self.scroll_line.min(last);
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(count: usize) -> Document {
        let text: Vec<String> = (1..=count).map(|n| n.to_string()).collect();
        Document::from_text(&text.join("\n"))
    }

    #[test]
    fn a_new_document_is_empty() {
        let document = Document::new();
        assert_eq!(document.buffer().to_text(), "");
        assert_eq!(document.scroll_line(), 0);
    }

    #[test]
    fn scrolling_stays_inside_the_text() {
        let mut document = numbered(10);
        document.scroll_by(-5);
        assert_eq!(document.scroll_line(), 0);
        document.scroll_by(500);
        assert_eq!(document.scroll_line(), 9);
    }

    #[test]
    fn a_revealed_line_is_scrolled_into_view_once() {
        let mut document = numbered(200);
        document.reveal(149);
        document.fit(30);
        assert_eq!(document.scroll_line(), 139);

        document.scroll_by(-100);
        document.fit(30);
        assert_eq!(document.scroll_line(), 39, "the request was used up");
    }

    #[test]
    fn a_line_already_on_screen_does_not_move_the_listing() {
        let mut document = numbered(200);
        document.reveal(5);
        document.fit(30);
        assert_eq!(document.scroll_line(), 0);
    }

    #[test]
    fn reloading_keeps_the_scroll_inside_the_new_text() {
        let mut document = Document::from_file_contents("main.asm", "one\ntwo\nthree");
        document.scroll_by(2);
        document.reload("only");
        assert_eq!(document.buffer().to_text(), "only");
        assert_eq!(document.scroll_line(), 0);
        assert_eq!(document.path(), Some(Path::new("main.asm")));
    }
}

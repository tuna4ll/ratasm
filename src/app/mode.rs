//! What the interface is currently doing: editing, or asking a question.
//!
//! Only one thing can be in front of the user at a time — a prompt, the
//! palette, or the editor itself — so it is one enum rather than a set of
//! booleans. A `show_palette` flag next to a `prompt` flag would allow the
//! impossible state where both are open, and something would have to decide
//! which wins at render time.

use crate::command::Command;

/// What a prompt is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// A file to open.
    OpenFile,
    /// A name to save under.
    SaveAs,
    /// A line number.
    GoToLine,
    /// An address expression.
    GoToAddress,
    /// Text to search for.
    Search,
    /// Text to replace matches with.
    Replace,
    /// Confirmation before discarding unsaved changes.
    ConfirmQuit,
}

impl PromptKind {
    /// The label shown before the input.
    pub const fn label(self) -> &'static str {
        match self {
            PromptKind::OpenFile => "Open file",
            PromptKind::SaveAs => "Save as",
            PromptKind::GoToLine => "Go to line",
            PromptKind::GoToAddress => "Go to address",
            PromptKind::Search => "Find",
            PromptKind::Replace => "Replace with",
            PromptKind::ConfirmQuit => "Unsaved changes. Quit anyway? (y/n)",
        }
    }

    /// A hint shown when the input is empty.
    pub const fn hint(self) -> &'static str {
        match self {
            PromptKind::OpenFile => "path to an .asm file",
            PromptKind::SaveAs => "path to write to",
            PromptKind::GoToLine => "line number",
            PromptKind::GoToAddress => "0x4000b0, rsp-0x20, rbp+8",
            PromptKind::Search => "text to find",
            PromptKind::Replace => "replacement text",
            PromptKind::ConfirmQuit => "y or n",
        }
    }

    /// Whether the prompt takes a single keypress rather than a line of text.
    pub const fn is_confirmation(self) -> bool {
        matches!(self, PromptKind::ConfirmQuit)
    }
}

/// A single-line text input.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Prompt {
    /// What is being asked.
    kind: Option<PromptKind>,
    /// The text typed so far.
    text: String,
    /// The cursor position, as a character index.
    cursor: usize,
}

impl Prompt {
    /// Creates a prompt asking for `kind`, pre-filled with `text`.
    pub fn new(kind: PromptKind, text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self {
            kind: Some(kind),
            text,
            cursor,
        }
    }

    /// What is being asked.
    pub fn kind(&self) -> Option<PromptKind> {
        self.kind
    }

    /// The text typed so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The cursor position as a character index.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Inserts a character at the cursor.
    pub fn insert(&mut self, ch: char) {
        let offset = self.byte_offset(self.cursor);
        self.text.insert(offset, ch);
        self.cursor += 1;
    }

    /// Deletes the character before the cursor.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let offset = self.byte_offset(self.cursor - 1);
        self.text.remove(offset);
        self.cursor -= 1;
    }

    /// Deletes the character after the cursor.
    pub fn delete(&mut self) {
        if self.cursor >= self.text.chars().count() {
            return;
        }
        let offset = self.byte_offset(self.cursor);
        self.text.remove(offset);
    }

    /// Moves the cursor one character left.
    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Moves the cursor one character right.
    pub fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.text.chars().count());
    }

    /// Moves the cursor to the start.
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    /// Moves the cursor to the end.
    pub fn move_end(&mut self) {
        self.cursor = self.text.chars().count();
    }

    /// Removes all text.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// Whether nothing has been typed.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Converts a character index to a byte offset.
    fn byte_offset(&self, index: usize) -> usize {
        self.text
            .char_indices()
            .nth(index)
            .map_or(self.text.len(), |(offset, _)| offset)
    }
}

/// The command palette's state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Palette {
    /// The query typed so far.
    prompt: Prompt,
    /// The commands matching the query, best first.
    matches: Vec<Command>,
    /// Which match is highlighted.
    selected: usize,
}

impl Palette {
    /// Opens the palette with every command listed.
    pub fn open() -> Self {
        let mut palette = Self {
            prompt: Prompt::default(),
            matches: Vec::new(),
            selected: 0,
        };
        palette.refresh();
        palette
    }

    /// The query prompt.
    pub fn prompt(&self) -> &Prompt {
        &self.prompt
    }

    /// The query prompt, mutably.
    pub fn prompt_mut(&mut self) -> &mut Prompt {
        &mut self.prompt
    }

    /// The current matches, best first.
    pub fn matches(&self) -> &[Command] {
        &self.matches
    }

    /// The index of the highlighted match.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The highlighted command, if there is one.
    pub fn selected_command(&self) -> Option<&Command> {
        self.matches.get(self.selected)
    }

    /// Recomputes the matches for the current query.
    ///
    /// The selection resets to the top, because after typing another character
    /// the previously highlighted row is usually no longer what the user
    /// meant.
    pub fn refresh(&mut self) {
        let commands = Command::all();
        let texts: Vec<String> = commands.iter().map(Command::search_text).collect();
        let ranked =
            crate::command::palette::rank(self.prompt.text(), texts.iter().map(String::as_str));

        self.matches = ranked
            .into_iter()
            .filter_map(|(index, _)| commands.get(index).cloned())
            .collect();
        self.selected = 0;
    }

    /// Highlights the next match, wrapping around.
    pub fn select_next(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.matches.len();
    }

    /// Highlights the previous match, wrapping around.
    pub fn select_previous(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        self.selected = (self.selected + self.matches.len() - 1) % self.matches.len();
    }
}

/// What the interface is doing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Mode {
    /// Editing, with keys going to the focused panel.
    #[default]
    Normal,
    /// The command palette is open.
    Palette(Palette),
    /// A prompt is open, and what to do with the answer.
    Prompt(Prompt),
}

impl Mode {
    /// Whether an overlay is capturing input.
    pub fn is_overlay(&self) -> bool {
        !matches!(self, Mode::Normal)
    }

    /// The prompt, when one is open.
    pub fn prompt(&self) -> Option<&Prompt> {
        match self {
            Mode::Prompt(prompt) => Some(prompt),
            Mode::Palette(palette) => Some(palette.prompt()),
            Mode::Normal => None,
        }
    }

    /// The palette, when it is open.
    pub fn palette(&self) -> Option<&Palette> {
        match self {
            Mode::Palette(palette) => Some(palette),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_prompt_puts_the_cursor_after_the_prefilled_text() {
        let prompt = Prompt::new(PromptKind::Search, "rax");
        assert_eq!(prompt.text(), "rax");
        assert_eq!(prompt.cursor(), 3);
    }

    #[test]
    fn typing_inserts_at_the_cursor() {
        let mut prompt = Prompt::new(PromptKind::Search, "rax");
        prompt.move_home();
        prompt.insert('e');
        assert_eq!(prompt.text(), "erax");
        assert_eq!(prompt.cursor(), 1);
    }

    #[test]
    fn backspace_and_delete_remove_the_right_character() {
        let mut prompt = Prompt::new(PromptKind::Search, "abc");
        prompt.backspace();
        assert_eq!(prompt.text(), "ab");

        prompt.move_home();
        prompt.delete();
        assert_eq!(prompt.text(), "b");
    }

    #[test]
    fn editing_at_the_edges_does_nothing_rather_than_panicking() {
        let mut prompt = Prompt::default();
        prompt.backspace();
        prompt.delete();
        prompt.move_left();
        prompt.move_right();
        assert!(prompt.is_empty());
        assert_eq!(prompt.cursor(), 0);
    }

    #[test]
    fn multibyte_text_is_edited_by_character_not_byte() {
        // A byte-indexed implementation would split the character and panic.
        let mut prompt = Prompt::new(PromptKind::Search, "ölçüm");
        prompt.backspace();
        assert_eq!(prompt.text(), "ölçü");
        prompt.move_home();
        prompt.delete();
        assert_eq!(prompt.text(), "lçü");
        prompt.insert('ş');
        assert_eq!(prompt.text(), "şlçü");
    }

    #[test]
    fn cursor_movement_is_clamped_to_the_text() {
        let mut prompt = Prompt::new(PromptKind::Search, "ab");
        for _ in 0..10 {
            prompt.move_right();
        }
        assert_eq!(prompt.cursor(), 2);
        for _ in 0..10 {
            prompt.move_left();
        }
        assert_eq!(prompt.cursor(), 0);
    }

    #[test]
    fn every_prompt_kind_has_a_label_and_a_hint() {
        for kind in [
            PromptKind::OpenFile,
            PromptKind::SaveAs,
            PromptKind::GoToLine,
            PromptKind::GoToAddress,
            PromptKind::Search,
            PromptKind::Replace,
            PromptKind::ConfirmQuit,
        ] {
            assert!(!kind.label().is_empty());
            assert!(!kind.hint().is_empty());
        }
    }

    #[test]
    fn only_the_quit_prompt_is_a_confirmation() {
        assert!(PromptKind::ConfirmQuit.is_confirmation());
        assert!(!PromptKind::Search.is_confirmation());
    }

    #[test]
    fn an_open_palette_lists_every_command() {
        let palette = Palette::open();
        assert_eq!(palette.matches().len(), Command::all().len());
        assert_eq!(palette.selected(), 0);
        assert!(palette.selected_command().is_some());
    }

    #[test]
    fn typing_narrows_the_palette() {
        let mut palette = Palette::open();
        for ch in "step over".chars() {
            palette.prompt_mut().insert(ch);
        }
        palette.refresh();

        assert!(palette.matches().len() < Command::all().len());
        assert_eq!(
            palette.selected_command().map(Command::id),
            Some("debug.step-over".to_owned())
        );
    }

    #[test]
    fn a_query_matching_nothing_leaves_no_selection() {
        let mut palette = Palette::open();
        for ch in "zzzznothing".chars() {
            palette.prompt_mut().insert(ch);
        }
        palette.refresh();

        assert!(palette.matches().is_empty());
        assert!(palette.selected_command().is_none());
    }

    #[test]
    fn selection_wraps_in_both_directions() {
        let mut palette = Palette::open();
        let count = palette.matches().len();

        palette.select_previous();
        assert_eq!(palette.selected(), count - 1, "wraps to the end");
        palette.select_next();
        assert_eq!(palette.selected(), 0, "wraps back to the start");
    }

    #[test]
    fn moving_the_selection_in_an_empty_palette_does_nothing() {
        let mut palette = Palette::open();
        for ch in "zzzz".chars() {
            palette.prompt_mut().insert(ch);
        }
        palette.refresh();

        palette.select_next();
        palette.select_previous();
        assert_eq!(palette.selected(), 0);
    }

    #[test]
    fn refreshing_resets_the_selection_to_the_best_match() {
        let mut palette = Palette::open();
        palette.select_next();
        palette.select_next();
        assert_ne!(palette.selected(), 0);

        palette.prompt_mut().insert('r');
        palette.refresh();
        assert_eq!(palette.selected(), 0);
    }

    #[test]
    fn the_default_mode_is_editing() {
        let mode = Mode::default();
        assert_eq!(mode, Mode::Normal);
        assert!(!mode.is_overlay());
        assert!(mode.prompt().is_none());
        assert!(mode.palette().is_none());
    }

    #[test]
    fn overlays_report_themselves_and_expose_their_prompt() {
        let mode = Mode::Prompt(Prompt::new(PromptKind::GoToLine, "42"));
        assert!(mode.is_overlay());
        assert_eq!(mode.prompt().map(Prompt::text), Some("42"));
        assert!(mode.palette().is_none());

        let mode = Mode::Palette(Palette::open());
        assert!(mode.is_overlay());
        assert!(mode.palette().is_some());
        assert!(mode.prompt().is_some(), "the palette has a query prompt");
    }
}

//! Undo and redo.
//!
//! # Model
//!
//! Every change is recorded as an [`Edit`]: the range it replaced, the text it
//! removed and the text it inserted. Because
//! [`TextBuffer::replace_range`](super::buffer::TextBuffer::replace_range) is
//! the buffer's only mutating primitive, an edit is exactly invertible —
//! undoing means replacing the inserted span with the removed text, and there
//! is no way for the history to fall out of step with the buffer.
//!
//! Edits are collected into [`Group`]s so that a burst of typing undoes as one
//! action rather than one character at a time. Groups close when the caller
//! calls [`History::seal`] — on cursor movement, on save, or when the edit kind
//! changes — which keeps grouping explicit rather than time-dependent and
//! therefore deterministic in tests.
//!
//! # Modified tracking
//!
//! Each group carries a unique state id. The history remembers which id was
//! current when the buffer was last saved, so undoing back to the save point
//! correctly reports the buffer as unmodified again — a counter of edits made
//! could not do that.

use super::buffer::TextBuffer;
use super::position::{Position, Range};

/// A single reversible change to a buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The range in the pre-edit text that was replaced.
    pub range: Range,
    /// The text that the edit removed.
    pub removed: String,
    /// The text that the edit inserted.
    pub inserted: String,
    /// Position just past the inserted text, in the post-edit text.
    pub inserted_end: Position,
    /// Cursor position before the edit, restored on undo.
    pub cursor_before: Position,
    /// Cursor position after the edit, restored on redo.
    pub cursor_after: Position,
}

impl Edit {
    /// The span the inserted text occupies in the post-edit buffer.
    pub fn inserted_range(&self) -> Range {
        Range::new(self.range.start, self.inserted_end)
    }

    /// Applies this edit to `buffer`, as used when redoing.
    pub fn apply(&self, buffer: &mut TextBuffer) {
        buffer.replace_range(self.range, &self.inserted);
    }

    /// Applies the inverse of this edit to `buffer`, as used when undoing.
    pub fn revert(&self, buffer: &mut TextBuffer) {
        buffer.replace_range(self.inserted_range(), &self.removed);
    }

    /// Whether this edit is a plain insertion of text with no removal.
    pub fn is_pure_insert(&self) -> bool {
        self.removed.is_empty() && !self.inserted.is_empty()
    }

    /// Whether this edit is a plain removal of text with no insertion.
    pub fn is_pure_delete(&self) -> bool {
        self.inserted.is_empty() && !self.removed.is_empty()
    }
}

/// A batch of edits that undo and redo as a single unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    edits: Vec<Edit>,
    state_id: u64,
}

impl Group {
    /// The edits in this group, in the order they were applied.
    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }

    /// The buffer state id this group produces.
    pub fn state_id(&self) -> u64 {
        self.state_id
    }
}

/// The result of an undo or redo, telling the caller where to put the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Restored {
    /// Where the cursor should be placed after the operation.
    pub cursor: Position,
}

/// The undo and redo stacks for one buffer.
#[derive(Debug, Clone, Default)]
pub struct History {
    undo_stack: Vec<Group>,
    redo_stack: Vec<Group>,
    /// Edits recorded since the last [`History::seal`].
    pending: Vec<Edit>,
    next_state_id: u64,
    saved_state_id: u64,
}

impl History {
    /// Creates an empty history for a buffer considered saved.
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            pending: Vec::new(),
            // State 0 is the pristine buffer; ids for real edits start at 1.
            next_state_id: 1,
            saved_state_id: 0,
        }
    }

    /// Records an edit into the currently open group.
    ///
    /// Recording discards the redo stack: once the user edits after undoing,
    /// the redone future is no longer reachable.
    pub fn record(&mut self, edit: Edit) {
        self.redo_stack.clear();
        self.pending.push(edit);
    }

    /// Closes the current group so the next edit starts a new undo step.
    ///
    /// Sealing an empty group is a no-op, so callers may seal freely — on
    /// every cursor move, for instance — without creating empty undo steps.
    pub fn seal(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let edits = std::mem::take(&mut self.pending);
        let state_id = self.next_state_id;
        self.next_state_id += 1;
        self.undo_stack.push(Group { edits, state_id });
    }

    /// Undoes the most recent group, applying it to `buffer`.
    ///
    /// Returns `None` when there is nothing left to undo.
    pub fn undo(&mut self, buffer: &mut TextBuffer) -> Option<Restored> {
        self.seal();
        let group = self.undo_stack.pop()?;
        for edit in group.edits.iter().rev() {
            edit.revert(buffer);
        }
        let cursor = group
            .edits
            .first()
            .map_or(Position::ORIGIN, |edit| edit.cursor_before);
        self.redo_stack.push(group);
        Some(Restored { cursor })
    }

    /// Redoes the most recently undone group, applying it to `buffer`.
    ///
    /// Returns `None` when there is nothing to redo.
    pub fn redo(&mut self, buffer: &mut TextBuffer) -> Option<Restored> {
        let group = self.redo_stack.pop()?;
        for edit in &group.edits {
            edit.apply(buffer);
        }
        let cursor = group
            .edits
            .last()
            .map_or(Position::ORIGIN, |edit| edit.cursor_after);
        self.undo_stack.push(group);
        Some(Restored { cursor })
    }

    /// Whether an undo step is available.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty() || !self.pending.is_empty()
    }

    /// Whether a redo step is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// The id identifying the buffer's current content state.
    pub fn current_state_id(&self) -> u64 {
        if !self.pending.is_empty() {
            // Uncommitted edits always represent an unsaved state.
            return u64::MAX;
        }
        self.undo_stack.last().map_or(0, Group::state_id)
    }

    /// Marks the current state as saved.
    pub fn mark_saved(&mut self) {
        self.seal();
        self.saved_state_id = self.current_state_id();
    }

    /// Whether the buffer differs from the last saved state.
    ///
    /// Undoing back to the save point clears this again, which a simple
    /// "edited since save" boolean could not do.
    pub fn is_modified(&self) -> bool {
        self.current_state_id() != self.saved_state_id
    }

    /// The number of sealed undo steps available.
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// The number of redo steps available.
    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }

    /// Drops all history, keeping the buffer's saved state marker at the
    /// current content. Used when a buffer is reloaded from disk.
    pub fn reset(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.pending.clear();
        self.next_state_id = 1;
        self.saved_state_id = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Applies `text` at `range` and records the resulting edit, mirroring what
    /// the editor does for a real keystroke.
    fn edit(
        buffer: &mut TextBuffer,
        history: &mut History,
        range: Range,
        text: &str,
        cursor_before: Position,
    ) {
        let result = buffer.replace_range(range, text);
        history.record(Edit {
            range,
            removed: result.removed,
            inserted: text.to_owned(),
            inserted_end: result.end,
            cursor_before,
            cursor_after: result.end,
        });
    }

    #[test]
    fn undo_restores_the_previous_text() {
        let mut buffer = TextBuffer::from_text("mov rax, 1");
        let mut history = History::new();
        let range = Range::new(Position::new(0, 4), Position::new(0, 7));
        edit(&mut buffer, &mut history, range, "rbx", Position::new(0, 7));
        assert_eq!(buffer.to_text(), "mov rbx, 1");

        let restored = history.undo(&mut buffer).expect("undo available");
        assert_eq!(buffer.to_text(), "mov rax, 1");
        assert_eq!(restored.cursor, Position::new(0, 7));
    }

    #[test]
    fn redo_reapplies_an_undone_edit() {
        let mut buffer = TextBuffer::from_text("ret");
        let mut history = History::new();
        edit(
            &mut buffer,
            &mut history,
            Range::empty(Position::new(0, 3)),
            "\nnop",
            Position::new(0, 3),
        );
        history.undo(&mut buffer);
        assert_eq!(buffer.to_text(), "ret");

        let restored = history.redo(&mut buffer).expect("redo available");
        assert_eq!(buffer.to_text(), "ret\nnop");
        assert_eq!(restored.cursor, Position::new(1, 3));
    }

    #[test]
    fn a_sealed_group_undoes_as_one_step() {
        let mut buffer = TextBuffer::new();
        let mut history = History::new();
        for (index, ch) in "mov".chars().enumerate() {
            edit(
                &mut buffer,
                &mut history,
                Range::empty(Position::new(0, index)),
                &ch.to_string(),
                Position::new(0, index),
            );
        }
        history.seal();
        assert_eq!(history.undo_depth(), 1, "typing burst is one undo step");

        history.undo(&mut buffer);
        assert_eq!(buffer.to_text(), "");
    }

    #[test]
    fn sealing_splits_edits_into_separate_steps() {
        let mut buffer = TextBuffer::new();
        let mut history = History::new();
        edit(
            &mut buffer,
            &mut history,
            Range::empty(Position::ORIGIN),
            "mov",
            Position::ORIGIN,
        );
        history.seal();
        edit(
            &mut buffer,
            &mut history,
            Range::empty(Position::new(0, 3)),
            " rax",
            Position::new(0, 3),
        );
        history.seal();
        assert_eq!(history.undo_depth(), 2);

        history.undo(&mut buffer);
        assert_eq!(buffer.to_text(), "mov");
        history.undo(&mut buffer);
        assert_eq!(buffer.to_text(), "");
    }

    #[test]
    fn sealing_an_empty_group_creates_no_undo_step() {
        let mut history = History::new();
        history.seal();
        history.seal();
        assert_eq!(history.undo_depth(), 0);
        assert!(!history.can_undo());
    }

    #[test]
    fn recording_after_undo_discards_the_redo_stack() {
        let mut buffer = TextBuffer::from_text("a");
        let mut history = History::new();
        edit(
            &mut buffer,
            &mut history,
            Range::empty(Position::new(0, 1)),
            "b",
            Position::new(0, 1),
        );
        history.seal();
        history.undo(&mut buffer);
        assert!(history.can_redo());

        edit(
            &mut buffer,
            &mut history,
            Range::empty(Position::new(0, 1)),
            "c",
            Position::new(0, 1),
        );
        assert!(!history.can_redo(), "new edit invalidates the redo future");
    }

    #[test]
    fn undoing_back_to_the_save_point_clears_the_modified_flag() {
        let mut buffer = TextBuffer::from_text("section .text");
        let mut history = History::new();
        assert!(!history.is_modified());

        let end = buffer.end_position();
        edit(
            &mut buffer,
            &mut history,
            Range::empty(end),
            "\nglobal _start",
            end,
        );
        history.seal();
        assert!(history.is_modified());

        history.undo(&mut buffer);
        assert!(
            !history.is_modified(),
            "undoing to the saved content must clear the modified flag"
        );
    }

    #[test]
    fn saving_after_edits_clears_the_modified_flag() {
        let mut buffer = TextBuffer::from_text("nop");
        let mut history = History::new();
        edit(
            &mut buffer,
            &mut history,
            Range::empty(Position::new(0, 3)),
            "\nret",
            Position::new(0, 3),
        );
        history.mark_saved();
        assert!(!history.is_modified());

        history.undo(&mut buffer);
        assert!(
            history.is_modified(),
            "undoing past the save point marks the buffer modified"
        );
    }

    #[test]
    fn unsealed_edits_count_as_modified() {
        let mut buffer = TextBuffer::from_text("nop");
        let mut history = History::new();
        edit(
            &mut buffer,
            &mut history,
            Range::empty(Position::new(0, 3)),
            "!",
            Position::new(0, 3),
        );
        assert!(history.is_modified());
        assert!(history.can_undo());
    }

    #[test]
    fn repeated_undo_and_redo_converge_on_the_same_text() {
        let original = "_start:\n    mov rax, 60\n    xor edi, edi\n    syscall\n";
        let mut buffer = TextBuffer::from_text(original);
        let mut history = History::new();

        edit(
            &mut buffer,
            &mut history,
            Range::new(Position::new(1, 8), Position::new(1, 11)),
            "rdi",
            Position::new(1, 11),
        );
        history.seal();
        edit(
            &mut buffer,
            &mut history,
            Range::new(Position::new(2, 4), Position::new(2, 16)),
            "mov edi, 7",
            Position::new(2, 16),
        );
        history.seal();
        let edited = buffer.to_text();

        for _ in 0..3 {
            while history.undo(&mut buffer).is_some() {}
            assert_eq!(buffer.to_text(), original);
            while history.redo(&mut buffer).is_some() {}
            assert_eq!(buffer.to_text(), edited);
        }
    }

    #[test]
    fn undo_on_empty_history_reports_nothing_to_do() {
        let mut buffer = TextBuffer::from_text("ret");
        let mut history = History::new();
        assert!(history.undo(&mut buffer).is_none());
        assert!(history.redo(&mut buffer).is_none());
        assert_eq!(buffer.to_text(), "ret");
    }

    #[test]
    fn reset_clears_every_stack() {
        let mut buffer = TextBuffer::from_text("a");
        let mut history = History::new();
        edit(
            &mut buffer,
            &mut history,
            Range::empty(Position::new(0, 1)),
            "b",
            Position::new(0, 1),
        );
        history.seal();
        history.reset();
        assert!(!history.can_undo());
        assert!(!history.can_redo());
        assert!(!history.is_modified());
    }

    #[test]
    fn edit_classification_distinguishes_insert_from_delete() {
        let insert = Edit {
            range: Range::empty(Position::ORIGIN),
            removed: String::new(),
            inserted: "x".into(),
            inserted_end: Position::new(0, 1),
            cursor_before: Position::ORIGIN,
            cursor_after: Position::new(0, 1),
        };
        assert!(insert.is_pure_insert());
        assert!(!insert.is_pure_delete());

        let delete = Edit {
            range: Range::new(Position::ORIGIN, Position::new(0, 1)),
            removed: "x".into(),
            inserted: String::new(),
            inserted_end: Position::ORIGIN,
            cursor_before: Position::new(0, 1),
            cursor_after: Position::ORIGIN,
        };
        assert!(delete.is_pure_delete());
        assert!(!delete.is_pure_insert());
    }
}

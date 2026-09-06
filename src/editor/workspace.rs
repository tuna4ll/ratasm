//! The set of open documents and the file operations on them.
//!
//! Saves write a temporary file in the destination's own directory and rename
//! it over the original, so an interrupted save cannot truncate the user's
//! work. The directory has to match: a rename across filesystems fails.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::document::Document;

/// Errors from opening and saving files.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// The file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file could not be written.
    #[error("cannot write {path}: {source}")]
    Write {
        /// The file that could not be written.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid UTF-8.
    #[error("{path} is not valid UTF-8 text")]
    NotUtf8 {
        /// The offending file.
        path: PathBuf,
    },
    /// A save was requested for a document that has no path yet.
    #[error("this buffer has no file name; use 'save as'")]
    NoPath,
    /// The requested document index does not exist.
    #[error("no document at index {index}")]
    NoSuchDocument {
        /// The index that was out of range.
        index: usize,
    },
}

/// Reads a file as UTF-8 text, refusing rather than lossy-decoding it.
pub fn read_file(path: &Path) -> Result<String, FileError> {
    let bytes = std::fs::read(path).map_err(|source| FileError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    String::from_utf8(bytes).map_err(|_| FileError::NotUtf8 {
        path: path.to_path_buf(),
    })
}

/// Writes `contents` to `path` atomically.
pub fn write_file_atomically(path: &Path, contents: &str) -> Result<(), FileError> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let directory = match directory {
        Some(directory) => directory.to_path_buf(),
        None => PathBuf::from("."),
    };

    std::fs::create_dir_all(&directory).map_err(|source| FileError::Write {
        path: path.to_path_buf(),
        source,
    })?;

    let mut temporary =
        tempfile::NamedTempFile::new_in(&directory).map_err(|source| FileError::Write {
            path: path.to_path_buf(),
            source,
        })?;

    temporary
        .write_all(contents.as_bytes())
        .and_then(|()| temporary.flush())
        .map_err(|source| FileError::Write {
            path: path.to_path_buf(),
            source,
        })?;

    temporary.persist(path).map_err(|error| FileError::Write {
        path: path.to_path_buf(),
        source: error.error,
    })?;

    Ok(())
}

/// Renders a path for a message, preferring the file name when it is long.
pub fn display_path(path: &Path) -> String {
    let full = path.display().to_string();
    if full.chars().count() <= 60 {
        return full;
    }
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or(full)
}

/// Whether an open document's path is the file a tool named.
///
/// GDB and the assembler report the path they were given, usually relative to
/// the project root, so an exact comparison misses. Matching whole trailing
/// components accepts `src/main.asm` for a reported `main.asm` while keeping
/// `lib/main.asm` and `src/main.asm` apart — a bare file-name comparison does
/// not, and confuses two files that legitimately share a name.
pub fn same_file(open: &Path, reported: &Path) -> bool {
    if open == reported {
        return true;
    }
    let open: Vec<_> = open.components().collect();
    let reported: Vec<_> = reported.components().collect();
    if reported.is_empty() || reported.len() > open.len() {
        return false;
    }
    open[open.len() - reported.len()..] == reported[..]
}

/// The open documents and which one is active; always holds at least one.
#[derive(Debug)]
pub struct Workspace {
    documents: Vec<Document>,
    active: usize,
    recent_files: Vec<PathBuf>,
}

impl Workspace {
    /// Maximum number of recently opened files remembered.
    pub const MAX_RECENT: usize = 16;

    /// Creates a workspace holding one empty, untitled document.
    pub fn new() -> Self {
        Self {
            documents: vec![Document::new()],
            active: 0,
            recent_files: Vec::new(),
        }
    }

    /// All open documents, in tab order.
    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    /// The number of open documents, always at least one.
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Always `false`; a workspace always holds a document.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The index of the active document.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// The active document.
    pub fn active(&self) -> &Document {
        self.documents
            .get(self.active)
            .unwrap_or(&self.documents[0])
    }

    /// The active document, mutably.
    pub fn active_mut(&mut self) -> &mut Document {
        let index = self.active.min(self.documents.len() - 1);
        self.active = index;
        &mut self.documents[index]
    }

    /// Selects a document by index, ignoring out-of-range values.
    pub fn set_active(&mut self, index: usize) {
        if index < self.documents.len() {
            self.active = index;
        }
    }

    /// Activates the next document, wrapping around.
    pub fn next_document(&mut self) {
        self.active = (self.active + 1) % self.documents.len();
    }

    /// Activates the previous document, wrapping around.
    pub fn previous_document(&mut self) {
        self.active = (self.active + self.documents.len() - 1) % self.documents.len();
    }

    /// Recently opened files, most recent first.
    pub fn recent_files(&self) -> &[PathBuf] {
        &self.recent_files
    }

    /// Records a path in the recent list, moving it to the front if present.
    fn remember(&mut self, path: &Path) {
        self.recent_files.retain(|existing| existing != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(Self::MAX_RECENT);
    }

    /// Opens `path`, activating it; a file already open is activated, not reopened.
    pub fn open(&mut self, path: &Path) -> Result<usize, FileError> {
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.path() == Some(path))
        {
            self.active = index;
            self.remember(path);
            return Ok(index);
        }

        let contents = read_file(path)?;
        let document = Document::from_file_contents(path, &contents);

        // Replace a pristine untitled buffer rather than leaving it behind.
        let index = if self.documents.len() == 1
            && self.documents[0].path().is_none()
            && !self.documents[0].is_modified()
            && self.documents[0].buffer().is_empty()
        {
            self.documents[0] = document;
            0
        } else {
            self.documents.push(document);
            self.documents.len() - 1
        };

        self.active = index;
        self.remember(path);
        Ok(index)
    }

    /// Adds an empty untitled document and activates it.
    pub fn new_document(&mut self) -> usize {
        self.documents.push(Document::new());
        self.active = self.documents.len() - 1;
        self.active
    }

    /// Adds a document with the given contents and activates it.
    pub fn add_document(&mut self, document: Document) -> usize {
        self.documents.push(document);
        self.active = self.documents.len() - 1;
        self.active
    }

    /// Saves the active document to its existing path.
    pub fn save_active(&mut self) -> Result<PathBuf, FileError> {
        let path = self
            .active()
            .path()
            .map(Path::to_path_buf)
            .ok_or(FileError::NoPath)?;
        self.save_active_as(&path)
    }

    /// Saves the active document to `path` and associates it with that path.
    pub fn save_active_as(&mut self, path: &Path) -> Result<PathBuf, FileError> {
        let contents = self.active().buffer().to_text();
        write_file_atomically(path, &contents)?;

        let document = self.active_mut();
        document.set_path(path);
        document.mark_saved();
        self.remember(path);
        Ok(path.to_path_buf())
    }

    /// Closes the document at `index`, returning whether it had unsaved changes.
    pub fn close(&mut self, index: usize) -> Result<bool, FileError> {
        if index >= self.documents.len() {
            return Err(FileError::NoSuchDocument { index });
        }
        let was_modified = self.documents[index].is_modified();
        self.documents.remove(index);
        if self.documents.is_empty() {
            self.documents.push(Document::new());
        }
        self.active = self.active.min(self.documents.len() - 1);
        Ok(was_modified)
    }

    /// Whether any open document has unsaved changes.
    pub fn has_unsaved_changes(&self) -> bool {
        self.documents.iter().any(Document::is_modified)
    }

    /// The documents with unsaved changes, for a quit confirmation prompt.
    pub fn modified_documents(&self) -> Vec<&Document> {
        self.documents
            .iter()
            .filter(|document| document.is_modified())
            .collect()
    }

    /// Finds the index of an open document by path.
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.documents
            .iter()
            .position(|document| document.path() == Some(path))
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[test]
    fn a_reported_path_matches_on_whole_trailing_components() {
        assert!(same_file(
            Path::new("/w/src/main.asm"),
            Path::new("src/main.asm")
        ));
        assert!(same_file(
            Path::new("/w/src/main.asm"),
            Path::new("main.asm")
        ));
        assert!(same_file(Path::new("main.asm"), Path::new("main.asm")));
    }

    #[test]
    fn two_files_sharing_a_name_stay_apart() {
        // The bug this replaces matched on the file name alone.
        assert!(!same_file(
            Path::new("/w/lib/main.asm"),
            Path::new("src/main.asm")
        ));
        assert!(!same_file(Path::new("/w/src/util.asm"), Path::new("m.asm")));
        assert!(!same_file(Path::new("main.asm"), Path::new("")));
    }

    #[test]
    fn a_new_workspace_holds_one_untitled_document() {
        let workspace = Workspace::new();
        assert_eq!(workspace.len(), 1);
        assert_eq!(workspace.active().display_name(), "[untitled]");
        assert!(!workspace.has_unsaved_changes());
    }

    #[test]
    fn opening_a_file_loads_its_contents() {
        let dir = temp_dir();
        let path = dir.path().join("main.asm");
        std::fs::write(&path, "section .text\nglobal _start\n").expect("write");

        let mut workspace = Workspace::new();
        workspace.open(&path).expect("open");
        assert_eq!(
            workspace.active().buffer().to_text(),
            "section .text\nglobal _start\n"
        );
        assert_eq!(workspace.active().display_name(), "main.asm");
        assert!(!workspace.active().is_modified());
    }

    #[test]
    fn opening_replaces_a_pristine_untitled_buffer() {
        let dir = temp_dir();
        let path = dir.path().join("main.asm");
        std::fs::write(&path, "ret\n").expect("write");

        let mut workspace = Workspace::new();
        workspace.open(&path).expect("open");
        assert_eq!(workspace.len(), 1, "the empty buffer should be reused");
    }

    #[test]
    fn opening_keeps_a_modified_untitled_buffer() {
        let dir = temp_dir();
        let path = dir.path().join("main.asm");
        std::fs::write(&path, "ret\n").expect("write");

        let mut workspace = Workspace::new();
        workspace.active_mut().insert_char('x');
        workspace.open(&path).expect("open");
        assert_eq!(workspace.len(), 2, "unsaved work must not be discarded");
    }

    #[test]
    fn opening_the_same_file_twice_activates_the_existing_buffer() {
        // Two buffers over one file would mean two divergent undo histories.
        let dir = temp_dir();
        let path = dir.path().join("main.asm");
        std::fs::write(&path, "ret\n").expect("write");

        let mut workspace = Workspace::new();
        let first = workspace.open(&path).expect("open");
        workspace.new_document();
        let second = workspace.open(&path).expect("reopen");
        assert_eq!(first, second);
        assert_eq!(workspace.len(), 2);
    }

    #[test]
    fn opening_a_missing_file_reports_an_error_without_panicking() {
        let dir = temp_dir();
        let mut workspace = Workspace::new();
        let error = workspace
            .open(&dir.path().join("nope.asm"))
            .expect_err("must fail");
        assert!(matches!(error, FileError::Read { .. }));
        assert_eq!(workspace.len(), 1, "workspace must be unchanged");
    }

    #[test]
    fn opening_a_binary_file_reports_a_utf8_error() {
        let dir = temp_dir();
        let path = dir.path().join("binary.o");
        std::fs::write(&path, [0x7f, b'E', b'L', b'F', 0xff, 0xfe]).expect("write");

        let mut workspace = Workspace::new();
        let error = workspace.open(&path).expect_err("must fail");
        assert!(matches!(error, FileError::NotUtf8 { .. }));
    }

    #[test]
    fn saving_writes_the_buffer_and_clears_the_modified_flag() {
        let dir = temp_dir();
        let path = dir.path().join("out.asm");

        let mut workspace = Workspace::new();
        workspace.active_mut().insert("mov rax, 1\n");
        assert!(workspace.has_unsaved_changes());

        workspace.save_active_as(&path).expect("save");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "mov rax, 1\n"
        );
        assert!(!workspace.has_unsaved_changes());
        assert_eq!(workspace.active().display_name(), "out.asm");
    }

    #[test]
    fn saving_without_a_path_is_an_error_rather_than_a_guess() {
        let mut workspace = Workspace::new();
        workspace.active_mut().insert_char('x');
        assert!(matches!(workspace.save_active(), Err(FileError::NoPath)));
    }

    #[test]
    fn saving_an_opened_file_reuses_its_path() {
        let dir = temp_dir();
        let path = dir.path().join("main.asm");
        std::fs::write(&path, "ret\n").expect("write");

        let mut workspace = Workspace::new();
        workspace.open(&path).expect("open");
        workspace.active_mut().insert("nop\n");
        let saved = workspace.save_active().expect("save");
        assert_eq!(saved, path);
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "nop\nret\n");
    }

    #[test]
    fn a_failed_save_leaves_the_original_file_intact() {
        // The reason saves go through a rename: a write that cannot complete
        // must not destroy what was already on disk.
        let dir = temp_dir();
        let path = dir.path().join("subdir").join("main.asm");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, "original\n").expect("write");

        // Saving into a path whose parent is a file cannot succeed.
        let blocked = dir.path().join("main.asm").join("impossible.asm");
        std::fs::write(dir.path().join("main.asm"), "blocker").expect("write");

        let result = write_file_atomically(&blocked, "new contents");
        assert!(result.is_err(), "expected the write to fail");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "original\n",
            "the untouched file must be unchanged"
        );
    }

    #[test]
    fn saving_creates_missing_parent_directories() {
        let dir = temp_dir();
        let path = dir.path().join("a/b/c/main.asm");
        write_file_atomically(&path, "ret\n").expect("save");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "ret\n");
    }

    #[test]
    fn saving_over_an_existing_file_replaces_it_completely() {
        let dir = temp_dir();
        let path = dir.path().join("main.asm");
        std::fs::write(&path, "a much longer original file\n").expect("write");
        write_file_atomically(&path, "short\n").expect("save");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "short\n");
    }

    #[test]
    fn saving_leaves_no_temporary_files_behind() {
        let dir = temp_dir();
        let path = dir.path().join("main.asm");
        write_file_atomically(&path, "ret\n").expect("save");
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(entries.len(), 1, "found stray files: {entries:?}");
    }

    #[test]
    fn documents_cycle_in_both_directions() {
        let mut workspace = Workspace::new();
        workspace.new_document();
        workspace.new_document();
        assert_eq!(workspace.active_index(), 2);

        workspace.next_document();
        assert_eq!(workspace.active_index(), 0, "must wrap around");
        workspace.previous_document();
        assert_eq!(workspace.active_index(), 2, "must wrap backwards");
    }

    #[test]
    fn setting_an_out_of_range_active_index_is_ignored() {
        let mut workspace = Workspace::new();
        workspace.set_active(99);
        assert_eq!(workspace.active_index(), 0);
    }

    #[test]
    fn closing_the_last_document_leaves_a_fresh_one() {
        let mut workspace = Workspace::new();
        workspace.active_mut().insert_char('x');
        let was_modified = workspace.close(0).expect("close");
        assert!(was_modified);
        assert_eq!(workspace.len(), 1);
        assert!(!workspace.active().is_modified());
    }

    #[test]
    fn closing_keeps_the_active_index_in_range() {
        let mut workspace = Workspace::new();
        workspace.new_document();
        workspace.new_document();
        workspace.set_active(2);
        workspace.close(2).expect("close");
        assert_eq!(workspace.active_index(), 1);
        assert_eq!(workspace.len(), 2);
    }

    #[test]
    fn closing_an_unknown_index_is_an_error() {
        let mut workspace = Workspace::new();
        assert!(matches!(
            workspace.close(9),
            Err(FileError::NoSuchDocument { index: 9 })
        ));
    }

    #[test]
    fn recent_files_are_ordered_most_recent_first_without_duplicates() {
        let dir = temp_dir();
        let first = dir.path().join("a.asm");
        let second = dir.path().join("b.asm");
        std::fs::write(&first, "ret\n").expect("write");
        std::fs::write(&second, "nop\n").expect("write");

        let mut workspace = Workspace::new();
        workspace.open(&first).expect("open a");
        workspace.open(&second).expect("open b");
        workspace.open(&first).expect("reopen a");

        assert_eq!(workspace.recent_files(), [first, second]);
    }

    #[test]
    fn the_recent_list_is_bounded() {
        let dir = temp_dir();
        let mut workspace = Workspace::new();
        for index in 0..(Workspace::MAX_RECENT + 5) {
            let path = dir.path().join(format!("file{index}.asm"));
            std::fs::write(&path, "ret\n").expect("write");
            workspace.open(&path).expect("open");
        }
        assert_eq!(workspace.recent_files().len(), Workspace::MAX_RECENT);
    }

    #[test]
    fn modified_documents_are_reported_for_a_quit_prompt() {
        let mut workspace = Workspace::new();
        workspace.new_document();
        workspace.active_mut().insert_char('x');
        assert_eq!(workspace.modified_documents().len(), 1);
        assert!(workspace.has_unsaved_changes());
    }

    #[test]
    fn a_round_trip_through_disk_preserves_the_text_exactly() {
        let dir = temp_dir();
        let path = dir.path().join("main.asm");
        let source = "section .data\n    msg: db `hi\\n`, 0\n\nsection .text\n_start:\n    ret\n";
        std::fs::write(&path, source).expect("write");

        let mut workspace = Workspace::new();
        workspace.open(&path).expect("open");
        workspace.save_active().expect("save");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), source);
    }
}

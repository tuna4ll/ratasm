//! The set of open documents and reading them from disk.

use std::path::{Path, PathBuf};

use super::document::Document;

/// Errors from opening files.
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
    /// The file is not valid UTF-8.
    #[error("{path} is not valid UTF-8 text")]
    NotUtf8 {
        /// The offending file.
        path: PathBuf,
    },
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

/// Completes `text` as a filesystem path.
pub fn complete_path(text: &str) -> (String, Vec<String>) {
    let (directory, prefix) = match text.rfind('/') {
        Some(index) => (&text[..=index], &text[index + 1..]),
        None => ("", text),
    };
    let search = if directory.is_empty() {
        Path::new(".")
    } else {
        Path::new(directory)
    };

    let Ok(entries) = std::fs::read_dir(search) else {
        return (text.to_owned(), Vec::new());
    };

    let mut candidates: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(prefix) || (name.starts_with('.') && !prefix.starts_with('.')) {
                return None;
            }
            let directory = entry.path().is_dir();
            Some(if directory { format!("{name}/") } else { name })
        })
        .collect();
    candidates.sort();

    let Some(first) = candidates.first() else {
        return (text.to_owned(), candidates);
    };

    let shared = candidates
        .iter()
        .skip(1)
        .fold(first.clone(), |shared, next| {
            let keep = shared
                .chars()
                .zip(next.chars())
                .take_while(|(a, b)| a == b)
                .count();
            shared.chars().take(keep).collect()
        });

    (format!("{directory}{shared}"), candidates)
}

/// Whether an open document's path is the file a tool named.
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

        let index = if self.documents.len() == 1
            && self.documents[0].path().is_none()
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

    /// Adds a document with the given contents and activates it.
    pub fn add_document(&mut self, document: Document) -> usize {
        self.documents.push(document);
        self.active = self.documents.len() - 1;
        self.active
    }

    /// Re-reads every open file from disk, returning how many had changed.
    ///
    /// Documents whose text is unchanged keep their cursor and selection.
    pub fn reload(&mut self) -> Result<usize, FileError> {
        let mut changed = 0;
        for document in &mut self.documents {
            let Some(path) = document.path().map(Path::to_path_buf) else {
                continue;
            };
            let contents = read_file(&path)?;
            if contents != document.buffer().to_text() {
                document.reload(&contents);
                changed += 1;
            }
        }
        Ok(changed)
    }

    /// Closes the document at `index`.
    pub fn close(&mut self, index: usize) -> Result<(), FileError> {
        if index >= self.documents.len() {
            return Err(FileError::NoSuchDocument { index });
        }
        self.documents.remove(index);
        if self.documents.is_empty() {
            self.documents.push(Document::new());
        }
        self.active = self.active.min(self.documents.len() - 1);
        Ok(())
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
    fn completing_a_prefix_extends_it_as_far_as_the_candidates_agree() {
        let dir = temp_dir();
        std::fs::write(dir.path().join("alpha.asm"), "").expect("write");
        std::fs::write(dir.path().join("alps.asm"), "").expect("write");
        std::fs::write(dir.path().join("beta.asm"), "").expect("write");

        let base = format!("{}/", dir.path().display());
        let (completed, candidates) = complete_path(&format!("{base}al"));
        assert_eq!(completed, format!("{base}alp"));
        assert_eq!(candidates.len(), 2);

        let (completed, candidates) = complete_path(&format!("{base}b"));
        assert_eq!(completed, format!("{base}beta.asm"));
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn completing_a_directory_adds_the_separator() {
        let dir = temp_dir();
        std::fs::create_dir(dir.path().join("sources")).expect("mkdir");

        let base = format!("{}/", dir.path().display());
        let (completed, _) = complete_path(&format!("{base}sou"));
        assert_eq!(completed, format!("{base}sources/"));
    }

    #[test]
    fn completing_something_with_no_match_leaves_it_alone() {
        let dir = temp_dir();
        let text = format!("{}/nothing", dir.path().display());
        let (completed, candidates) = complete_path(&text);
        assert_eq!(completed, text);
        assert!(candidates.is_empty());
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
    fn opening_the_same_file_twice_activates_the_existing_buffer() {
        let dir = temp_dir();
        let path = dir.path().join("main.asm");
        std::fs::write(&path, "ret\n").expect("write");

        let mut workspace = Workspace::new();
        let first = workspace.open(&path).expect("open");
        workspace.add_document(Document::new());
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
    fn documents_cycle_in_both_directions() {
        let mut workspace = Workspace::new();
        workspace.add_document(Document::new());
        workspace.add_document(Document::new());
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
        workspace.add_document(Document::from_text("ret"));
        workspace.close(1).expect("close");
        workspace.close(0).expect("close");
        assert_eq!(workspace.len(), 1);
        assert!(workspace.active().buffer().is_empty());
    }

    #[test]
    fn closing_keeps_the_active_index_in_range() {
        let mut workspace = Workspace::new();
        workspace.add_document(Document::new());
        workspace.add_document(Document::new());
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
    fn reloading_picks_up_changes_made_on_disk() {
        let dir = temp_dir();
        let first = dir.path().join("main.asm");
        let second = dir.path().join("util.asm");
        std::fs::write(&first, "ret\n").expect("write");
        std::fs::write(&second, "nop\n").expect("write");

        let mut workspace = Workspace::new();
        workspace.open(&first).expect("open");
        workspace.open(&second).expect("open");
        assert_eq!(workspace.reload().expect("reload"), 0);

        std::fs::write(&first, "mov rax, 60\nsyscall\n").expect("write");
        assert_eq!(workspace.reload().expect("reload"), 1);
        assert_eq!(
            workspace.documents()[0].buffer().to_text(),
            "mov rax, 60\nsyscall\n"
        );
    }

    #[test]
    fn reloading_a_file_that_vanished_reports_it() {
        let dir = temp_dir();
        let path = dir.path().join("main.asm");
        std::fs::write(&path, "ret\n").expect("write");

        let mut workspace = Workspace::new();
        workspace.open(&path).expect("open");
        std::fs::remove_file(&path).expect("remove");
        assert!(matches!(workspace.reload(), Err(FileError::Read { .. })));
    }
}

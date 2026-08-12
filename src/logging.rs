//! File-based diagnostic logging.
//!
//! The TUI owns stdout and stderr, so log records must never be written to the
//! console. All tracing output is redirected to a file whose location the user
//! controls; when no file is configured logging is disabled entirely rather
//! than corrupting the display.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing_subscriber::filter::EnvFilter;

/// Errors that can occur while initialising logging.
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    /// The log file could not be created or opened for appending.
    #[error("cannot open log file {path}: {source}")]
    OpenFile {
        /// Path that failed to open.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A global subscriber had already been installed.
    #[error("a tracing subscriber was already installed")]
    AlreadyInitialised,
}

/// Initialises tracing to append to `path` at the given filter directive.
///
/// `directive` uses the standard `RUST_LOG` syntax, for example `"info"` or
/// `"ratasm::debugger=trace,info"`. The `RATASM_LOG` environment variable takes
/// precedence when set.
///
/// # Errors
///
/// Returns [`LoggingError::OpenFile`] if the log file is not writable, or
/// [`LoggingError::AlreadyInitialised`] if called twice.
pub fn init_file_logging(path: &Path, directive: &str) -> Result<(), LoggingError> {
    let file = open_log_file(path)?;
    let filter = EnvFilter::try_from_env("RATASM_LOG")
        .or_else(|_| EnvFilter::try_new(directive))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(Mutex::new(file))
        .with_ansi(false)
        .with_target(true)
        .try_init()
        .map_err(|_| LoggingError::AlreadyInitialised)
}

fn open_log_file(path: &Path) -> Result<File, LoggingError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|source| LoggingError::OpenFile {
                path: path.to_path_buf(),
                source,
            })?;
        }
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| LoggingError::OpenFile {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deeper/ratasm.log");
        assert!(open_log_file(&path).is_ok());
        assert!(path.exists());
    }

    #[test]
    fn reports_unwritable_paths_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        // A directory can never be opened as a file for appending.
        let err = open_log_file(dir.path()).unwrap_err();
        assert!(matches!(err, LoggingError::OpenFile { .. }));
    }
}

//! What a project had open and which breakpoints it had, kept between runs.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::debugger::breakpoints::Location;

use super::App;

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    files: Vec<PathBuf>,
    active: usize,
    breakpoints: Vec<SavedBreakpoint>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SavedBreakpoint {
    file: Option<PathBuf>,
    line: Option<usize>,
    watch: Option<String>,
    enabled: bool,
    condition: Option<String>,
}

/// Where the session of the project at `root` is kept.
pub fn path_for(root: &Path) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in root.as_os_str().as_encoded_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
    }
    Some(
        base.join("ratasm")
            .join("sessions")
            .join(format!("{hash:016x}.json")),
    )
}

/// Writes what `app` has open to `path`.
pub fn save(app: &App, path: &Path) -> std::io::Result<()> {
    let files: Vec<PathBuf> = app
        .workspace
        .documents()
        .iter()
        .filter_map(|document| document.path())
        .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
        .collect();
    let breakpoints = app
        .breakpoints
        .all()
        .iter()
        .filter_map(|breakpoint| {
            let (file, line, watch) = match &breakpoint.location {
                Location::Line { file, line } => (
                    Some(std::fs::canonicalize(file).unwrap_or_else(|_| file.clone())),
                    Some(*line),
                    None,
                ),
                Location::Watch(expression) => (None, None, Some(expression.clone())),
                _ => return None,
            };
            Some(SavedBreakpoint {
                file,
                line,
                watch,
                enabled: breakpoint.enabled,
                condition: breakpoint.condition.clone(),
            })
        })
        .collect();
    let saved = Saved {
        active: app
            .workspace
            .active_index()
            .min(files.len().saturating_sub(1)),
        files,
        breakpoints,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&saved).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

/// Restores breakpoints from `path` and, when `open_files` is set, the files.
pub fn restore(app: &mut App, path: &Path, open_files: bool) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(saved) = serde_json::from_str::<Saved>(&text) else {
        return;
    };
    for breakpoint in saved.breakpoints {
        let location = match (breakpoint.file, breakpoint.line, breakpoint.watch) {
            (Some(file), Some(line), _) if file.is_file() => Location::Line { file, line },
            (_, _, Some(expression)) => Location::Watch(expression),
            _ => continue,
        };
        app.breakpoints
            .restore(location, breakpoint.enabled, breakpoint.condition);
    }
    if !open_files {
        return;
    }
    let mut opened = Vec::new();
    for file in &saved.files {
        if let Ok(index) = app.workspace.open(file) {
            opened.push(index);
        }
    }
    if let Some(index) = opened.get(saved.active) {
        app.workspace.set_active(*index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::project::Project;

    #[test]
    fn files_and_breakpoints_survive_a_restart() {
        let dir = tempfile::tempdir().expect("temp dir");
        let a = dir.path().join("a.asm");
        let b = dir.path().join("b.asm");
        std::fs::write(&a, "nop\nnop\n").expect("write");
        std::fs::write(&b, "ret\n").expect("write");
        let store = dir.path().join("state/session.json");

        let mut app = App::new(Project::for_file(&a), Settings::default()).expect("app");
        app.workspace.open(&a).expect("open");
        app.workspace.open(&b).expect("open");
        app.toggle_breakpoint_in(&a, 2);
        app.breakpoints.set_condition(0, Some("$rax == 1".into()));
        app.breakpoints.add_watch("$rbx");
        app.workspace.set_active(1);
        save(&app, &store).expect("save");

        let mut fresh = App::new(Project::for_file(&a), Settings::default()).expect("app");
        restore(&mut fresh, &store, true);
        assert_eq!(fresh.workspace.documents().len(), 2);
        assert!(fresh
            .workspace
            .active()
            .path()
            .expect("path")
            .ends_with("b.asm"));
        assert!(fresh.breakpoints.is_set_at(&a, 2));
        assert_eq!(
            fresh.breakpoints.all()[0].condition.as_deref(),
            Some("$rax == 1")
        );
        assert_eq!(fresh.breakpoints.len(), 2);

        let mut files_given = App::new(Project::for_file(&a), Settings::default()).expect("app");
        restore(&mut files_given, &store, false);
        assert_eq!(files_given.workspace.documents().len(), 1);
        assert_eq!(files_given.breakpoints.len(), 2);
    }

    #[test]
    fn each_project_has_its_own_file() {
        let one = path_for(Path::new("/tmp/one"));
        let two = path_for(Path::new("/tmp/two"));
        assert!(one.is_some());
        assert_ne!(one, two);
    }

    #[test]
    fn a_missing_or_broken_file_changes_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = dir.path().join("session.json");
        let mut app = App::new(
            Project::for_file(&dir.path().join("a.asm")),
            Settings::default(),
        )
        .expect("app");
        restore(&mut app, &store, true);
        std::fs::write(&store, "{ not json").expect("write");
        restore(&mut app, &store, true);
        assert!(app.breakpoints.is_empty());
    }
}

//! Building a project that has its own Makefile.

use std::path::{Path, PathBuf};

use super::build::{BuildError, BuildOutcome, BuildStep};
use super::diagnostics::{self, Diagnostic, Producer};
use crate::process::{self, CommandSpec};
use crate::project::Project;

const NAMES: [&str; 3] = ["GNUmakefile", "makefile", "Makefile"];

/// The Makefile the project is built with, unless `build.make = false`.
pub fn makefile(project: &Project) -> Option<PathBuf> {
    if project.config().build.make == Some(false) {
        return None;
    }
    NAMES
        .iter()
        .map(|name| project.root().join(name))
        .find(|path| path.is_file())
}

/// The targets a Makefile defines, with the one-based line of each.
pub fn targets(text: &str) -> Vec<(String, usize)> {
    let mut found: Vec<(String, usize)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.starts_with(['\t', ' ', '#']) {
            continue;
        }
        let Some((names, rest)) = line.split_once(':') else {
            continue;
        };
        if rest.starts_with('=') || names.contains('=') {
            continue;
        }
        for name in names.split_whitespace() {
            let plain = !name.starts_with('.') && !name.contains(['%', '$', '(', '/']);
            if plain && !found.iter().any(|(known, _)| known == name) {
                found.push((name.to_owned(), index + 1));
            }
        }
    }
    found
}

/// The lines of `make`'s output that report a problem.
pub fn parse_output(output: &str) -> Vec<Diagnostic> {
    output
        .lines()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("error")
                || lower.contains("warning")
                || lower.contains("undefined reference")
        })
        .flat_map(|line| {
            let producer =
                if line.contains(".asm:") || line.contains(".s:") || line.contains(".inc:") {
                    Producer::Assembler
                } else {
                    Producer::Linker
                };
            match producer {
                Producer::Assembler => diagnostics::parse_assembler_output(line),
                Producer::Linker => diagnostics::parse_linker_output(line),
            }
        })
        .collect()
}

/// Runs `make`, or `make target`, in the project root.
pub async fn build(project: &Project, target: Option<&str>) -> Result<BuildOutcome, BuildError> {
    let started = std::time::Instant::now();
    if !process::is_available(Path::new("make")) {
        return Err(BuildError::ToolMissing {
            tool: "make".to_owned(),
            setting: "make",
            config: Project::FILE_NAME,
        });
    }
    let mut spec = CommandSpec::new("make").working_directory(project.root());
    if let Some(target) = target {
        spec = spec.arg(target);
    }
    let output = process::run(&spec).await?;
    let diagnostics = parse_output(&output.combined());
    let success = output.is_success();
    let executable = success.then(|| executable(project)).flatten();
    Ok(BuildOutcome {
        success,
        steps: vec![BuildStep {
            label: format!("make {}", target.unwrap_or_default())
                .trim()
                .to_owned(),
            output,
            diagnostics: diagnostics.clone(),
        }],
        diagnostics,
        executable,
        duration: started.elapsed(),
    })
}

/// The program a Makefile produced: `build.executable` when set, otherwise
/// the most recently written ELF executable in the project.
pub fn executable(project: &Project) -> Option<PathBuf> {
    if let Some(path) = &project.config().build.executable {
        let path = project.resolve(path);
        return path.is_file().then_some(path);
    }
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    visit(project.root(), 4, &mut |path| {
        if !is_elf_program(path) {
            return;
        }
        let modified = std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if newest.as_ref().is_none_or(|(time, _)| modified > *time) {
            newest = Some((modified, path.to_path_buf()));
        }
    });
    newest.map(|(_, path)| path)
}

fn visit(directory: &Path, depth: usize, found: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if depth > 0 {
                visit(&path, depth - 1, found);
            }
        } else {
            found(&path);
        }
    }
}

fn is_elf_program(path: &Path) -> bool {
    use std::io::Read as _;
    use std::os::unix::fs::PermissionsExt as _;

    let executable =
        std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0);
    if !executable || path.extension().is_some_and(|extension| extension == "so") {
        return false;
    }
    let mut header = [0u8; 18];
    let read = std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut header));
    read.is_ok() && header[..4] == *b"\x7fELF" && matches!(header[16], 2 | 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_read_in_order_without_patterns_or_variables() {
        let text = "CC := gcc\nall: build/prog\n\nbuild/prog: a.o\n\tld $^ -o $@\n%.o: %.asm\n\
                    run clean: \n.PHONY: all run clean\nx ?= 1\n# test: no\n";
        let names: Vec<_> = targets(text).into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["all", "run", "clean"]);
        assert_eq!(targets(text)[1].1, 7);
    }

    #[test]
    fn only_problem_lines_become_diagnostics() {
        let output = "nasm -f elf64 src/main.asm -o build/main.o\n\
                      src/main.asm:3: error: symbol `x' not defined\n\
                      make: *** [Makefile:9: build/main.o] Error 1\n";
        let found = parse_output(output);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].file.as_deref(), Some(Path::new("src/main.asm")));
        assert_eq!(found[0].line, Some(3));
        assert_eq!(found[0].severity, diagnostics::Severity::Error);
    }

    #[tokio::test]
    async fn a_makefile_project_builds_and_its_program_is_found() {
        for tool in ["make", "nasm", "ld"] {
            if !process::is_available(Path::new(tool)) {
                eprintln!("skipping: {tool} not installed");
                return;
            }
        }
        let dir = tempfile::tempdir().expect("temp dir");
        let project = Project::create_with_make(dir.path(), "made").expect("create");
        assert!(makefile(&project).is_some());

        let outcome = build(&project, None).await.expect("make runs");
        assert!(outcome.success, "{}", outcome.raw_output());
        let program = outcome.executable.expect("an executable");
        assert!(program.ends_with("build/made"), "{}", program.display());

        let cleaned = build(&project, Some("clean")).await.expect("make runs");
        assert!(cleaned.success && cleaned.executable.is_none());
        assert!(!program.exists());

        std::fs::write(
            project.entry_path(),
            "global _start\n_start:\n    mov rax, rbxx\n",
        )
        .expect("write");
        let failed = build(&project, None).await.expect("make runs");
        assert!(!failed.success);
        let first = failed.first_navigable().expect("a located error");
        assert_eq!(first.line, Some(3));
    }
}

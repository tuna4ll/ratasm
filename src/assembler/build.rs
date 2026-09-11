//! The assemble-and-link pipeline.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::diagnostics::{self, Diagnostic};
use crate::process::{self, CommandSpec, ProcessError, ProcessOutput};
use crate::project::Project;

/// Errors that prevent a build from being attempted at all.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// A required tool is not installed or not on `PATH`.
    #[error("{tool} not found: install it or set build.{setting} in {config}")]
    ToolMissing {
        /// The executable that could not be found.
        tool: String,
        /// The configuration key that overrides it.
        setting: &'static str,
        /// The project file name.
        config: &'static str,
    },
    /// A source file listed in the project does not exist.
    #[error("source file {path} does not exist")]
    MissingSource {
        /// The missing file.
        path: PathBuf,
    },
    /// The output directory could not be created.
    #[error("cannot create output directory {path}: {source}")]
    OutputDirectory {
        /// The directory that could not be created.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// A tool could not be launched or supervised.
    #[error(transparent)]
    Process(#[from] ProcessError),
    /// The project lists no sources at all.
    #[error("the project has no source files to build")]
    NothingToBuild,
}

/// Options that vary between builds of the same project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BuildOptions {
    /// Ask the assembler to emit debug information.
    pub debug_info: bool,
}

impl BuildOptions {
    /// Options for an ordinary build.
    pub fn release() -> Self {
        Self { debug_info: false }
    }

    /// Options for a build that will be debugged.
    pub fn debug() -> Self {
        Self { debug_info: true }
    }
}

/// One tool invocation within a build.
#[derive(Debug, Clone)]
pub struct BuildStep {
    /// What this step was doing, for display.
    pub label: String,
    /// What the tool produced.
    pub output: ProcessOutput,
    /// Diagnostics parsed from the tool's output.
    pub diagnostics: Vec<Diagnostic>,
}

impl BuildStep {
    /// Whether this step succeeded.
    pub fn is_success(&self) -> bool {
        self.output.is_success()
    }
}

/// The result of a complete build.
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    /// Whether every step succeeded.
    pub success: bool,
    /// Every step that ran, in order.
    pub steps: Vec<BuildStep>,
    /// Diagnostics from all steps, in order.
    pub diagnostics: Vec<Diagnostic>,
    /// The linked executable, when the build succeeded.
    pub executable: Option<PathBuf>,
    /// Total wall-clock time.
    pub duration: Duration,
}

impl BuildOutcome {
    /// The number of errors and warnings across all steps.
    pub fn counts(&self) -> (usize, usize) {
        diagnostics::counts(&self.diagnostics)
    }

    /// The first diagnostic worth jumping to, if any.
    pub fn first_navigable(&self) -> Option<&Diagnostic> {
        diagnostics::first_navigable(&self.diagnostics)
    }

    /// A one-line summary for the status bar.
    pub fn summary(&self) -> String {
        let (errors, warnings) = self.counts();
        let millis = self.duration.as_millis();
        if self.success {
            if warnings > 0 {
                format!("Build succeeded with {warnings} warning(s) in {millis} ms")
            } else {
                format!("Build succeeded in {millis} ms")
            }
        } else {
            format!("Build failed: {errors} error(s), {warnings} warning(s) in {millis} ms")
        }
    }

    /// All tool output concatenated, for the raw output pane.
    pub fn raw_output(&self) -> String {
        let mut out = String::new();
        for step in &self.steps {
            out.push_str("$ ");
            out.push_str(&step.output.command);
            out.push('\n');
            let text = step.output.combined();
            if !text.is_empty() {
                out.push_str(&text);
                if !text.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
        out
    }
}

/// Assembles and links a project.
pub async fn build(project: &Project, options: BuildOptions) -> Result<BuildOutcome, BuildError> {
    let started = std::time::Instant::now();
    let config = project.config();

    let assembler = PathBuf::from(&config.build.assembler);
    if !process::is_available(&assembler) {
        return Err(BuildError::ToolMissing {
            tool: config.build.assembler.clone(),
            setting: "assembler",
            config: Project::FILE_NAME,
        });
    }

    let sources = project.source_paths();
    if sources.is_empty() {
        return Err(BuildError::NothingToBuild);
    }
    for source in &sources {
        if !source.is_file() {
            return Err(BuildError::MissingSource {
                path: source.clone(),
            });
        }
    }

    let output_directory = project.output_directory();
    std::fs::create_dir_all(&output_directory).map_err(|source| BuildError::OutputDirectory {
        path: output_directory.clone(),
        source,
    })?;

    let mut steps = Vec::new();
    let mut all_diagnostics = Vec::new();
    let mut objects: Vec<PathBuf> = Vec::new();

    for (index, source) in sources.iter().enumerate() {
        let relative = config
            .all_sources()
            .get(index)
            .cloned()
            .unwrap_or_else(|| PathBuf::from(source));
        let object = project.object_path(&relative);

        let spec = assemble_command(project, &assembler, source, &object, options);
        let output = process::run(&spec).await?;
        let parsed = diagnostics::parse_assembler_output(&output.combined());

        let failed = !output.is_success();
        all_diagnostics.extend(parsed.clone());
        steps.push(BuildStep {
            label: format!("Assemble {}", display_path(project, source)),
            output,
            diagnostics: parsed,
        });

        if failed {
            return Ok(finish(steps, all_diagnostics, None, started.elapsed()));
        }
        objects.push(object);
    }

    let linker = PathBuf::from(&config.build.linker);
    if !process::is_available(&linker) {
        return Err(BuildError::ToolMissing {
            tool: config.build.linker.clone(),
            setting: "linker",
            config: Project::FILE_NAME,
        });
    }

    let executable = project.executable_path();
    objects.extend(
        config
            .build
            .objects
            .iter()
            .map(|object| project.resolve(object)),
    );

    let spec = link_command(project, &linker, &objects, &executable);
    let output = process::run(&spec).await?;
    let parsed = diagnostics::parse_linker_output(&output.combined());
    let linked = output.is_success();

    all_diagnostics.extend(parsed.clone());
    steps.push(BuildStep {
        label: "Link".to_owned(),
        output,
        diagnostics: parsed,
    });

    let executable = linked.then_some(executable);
    Ok(finish(
        steps,
        all_diagnostics,
        executable,
        started.elapsed(),
    ))
}

/// Builds the outcome from the steps that ran.
fn finish(
    steps: Vec<BuildStep>,
    diagnostics: Vec<Diagnostic>,
    executable: Option<PathBuf>,
    duration: Duration,
) -> BuildOutcome {
    BuildOutcome {
        success: steps.iter().all(BuildStep::is_success) && executable.is_some(),
        steps,
        diagnostics,
        executable,
        duration,
    }
}

/// Constructs the assembler command for one source file.
pub fn assemble_command(
    project: &Project,
    assembler: &Path,
    source: &Path,
    object: &Path,
    options: BuildOptions,
) -> CommandSpec {
    let config = project.config();
    let mut spec = CommandSpec::new(assembler).args(config.build.assembler_args.clone());

    if options.debug_info && !config.build.requests_debug_info() {
        spec = spec.arg("-g");
    }
    for directory in &config.project.include_directories {
        let mut path = project
            .for_tool(&project.resolve(directory))
            .display()
            .to_string();
        if !path.ends_with('/') {
            path.push('/');
        }
        spec = spec.arg("-i").arg(path);
    }

    spec.arg(project.for_tool(source).display().to_string())
        .arg("-o")
        .arg(project.for_tool(object).display().to_string())
        .working_directory(project.root())
}

/// Constructs the linker command.
pub fn link_command(
    project: &Project,
    linker: &Path,
    objects: &[PathBuf],
    executable: &Path,
) -> CommandSpec {
    CommandSpec::new(linker)
        .args(
            objects
                .iter()
                .map(|object| project.for_tool(object).display().to_string())
                .collect::<Vec<_>>(),
        )
        .args(project.config().build.linker_args.clone())
        .arg("-o")
        .arg(project.for_tool(executable).display().to_string())
        .working_directory(project.root())
}

/// Runs a built executable with the project's run settings.
pub async fn run_executable(
    project: &Project,
    executable: &Path,
) -> Result<ProcessOutput, ProcessError> {
    let config = project.config();
    let mut spec = CommandSpec::new(project.absolute_path(executable))
        .args(config.run.args.clone())
        .working_directory(project.run_directory());

    if let Some(timeout) = config.run.timeout() {
        spec = spec.timeout(timeout);
    }
    if let Some(stdin) = &config.run.stdin {
        spec = spec.stdin(stdin.clone());
    }

    process::run(&spec).await
}

/// Renders a path relative to the project root when possible.
fn display_path(project: &Project, path: &Path) -> String {
    path.strip_prefix(project.root())
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::super::diagnostics::Producer;
    use super::*;
    use crate::project::template;

    fn toolchain_available() -> bool {
        process::is_available(Path::new("nasm")) && process::is_available(Path::new("ld"))
    }

    fn project_with(source: &str) -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = Project::create(dir.path(), "test").expect("create");
        std::fs::write(project.entry_path(), source).expect("write source");
        (dir, project)
    }

    #[test]
    fn the_assemble_command_matches_the_documented_default() {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = Project::create(dir.path(), "demo").expect("create");
        let spec = assemble_command(
            &project,
            Path::new("nasm"),
            &project.resolve(Path::new("source.asm")),
            &project.resolve(Path::new("source.o")),
            BuildOptions::release(),
        );
        assert_eq!(spec.display(), "nasm -f elf64 source.asm -o source.o");
    }

    #[test]
    fn the_link_command_matches_the_documented_default() {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = Project::create(dir.path(), "demo").expect("create");
        let spec = link_command(
            &project,
            Path::new("ld"),
            &[project.resolve(Path::new("source.o"))],
            &project.resolve(Path::new("source")),
        );
        assert_eq!(spec.display(), "ld source.o -o source");
    }

    #[test]
    fn a_debug_build_asks_the_assembler_for_debug_information() {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = Project::create(dir.path(), "demo").expect("create");
        let spec = assemble_command(
            &project,
            Path::new("nasm"),
            Path::new("a.asm"),
            Path::new("a.o"),
            BuildOptions::debug(),
        );
        assert!(spec.args.contains(&"-g".to_owned()));
    }

    #[test]
    fn debug_flags_are_not_duplicated() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut project = Project::create(dir.path(), "demo").expect("create");
        project.config_mut().build.assembler_args.push("-g".into());

        let spec = assemble_command(
            &project,
            Path::new("nasm"),
            Path::new("a.asm"),
            Path::new("a.o"),
            BuildOptions::debug(),
        );
        assert_eq!(spec.args.iter().filter(|arg| *arg == "-g").count(), 1);
    }

    #[tokio::test]
    async fn a_project_builds_from_outside_its_own_directory() {
        if !crate::process::is_available(Path::new("nasm"))
            || !crate::process::is_available(Path::new("ld"))
        {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }

        let parent = tempfile::tempdir().expect("temp dir");
        let root = parent.path().join("project");
        let project = Project::create(&root, "outside").expect("create");

        let outcome = build(&project, BuildOptions::release())
            .await
            .expect("the build runs");
        assert!(outcome.success, "{}", outcome.raw_output());
        assert!(outcome.executable.is_some_and(|path| path.is_file()));
    }

    #[test]
    fn tool_paths_are_written_relative_to_the_root() {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = Project::create(dir.path(), "relative").expect("create");

        let spec = assemble_command(
            &project,
            Path::new("nasm"),
            &project.entry_path(),
            &project.output_directory().join("main.o"),
            BuildOptions::release(),
        );
        let rendered = spec.display();

        assert!(rendered.contains("src/main.asm"), "{rendered}");
        assert!(
            !rendered.contains(&dir.path().display().to_string()),
            "an absolute path here would not survive a move: {rendered}"
        );
    }

    #[test]
    fn include_directories_are_passed_with_a_trailing_separator() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut project = Project::create(dir.path(), "demo").expect("create");
        project
            .config_mut()
            .project
            .include_directories
            .push(PathBuf::from("include"));

        let spec = assemble_command(
            &project,
            Path::new("nasm"),
            Path::new("a.asm"),
            Path::new("a.o"),
            BuildOptions::release(),
        );
        let include = spec
            .args
            .iter()
            .find(|arg| arg.contains("include"))
            .expect("include path passed");
        assert!(include.ends_with('/'), "NASM needs the trailing separator");
    }

    #[test]
    fn custom_linker_arguments_are_forwarded() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut project = Project::create(dir.path(), "demo").expect("create");
        project.config_mut().build.linker_args = vec!["-n".to_owned(), "-static".to_owned()];

        let spec = link_command(
            &project,
            Path::new("ld"),
            &[PathBuf::from("a.o")],
            Path::new("a"),
        );
        assert!(spec.display().contains("-n -static"));
    }

    #[tokio::test]
    async fn a_missing_assembler_is_reported_clearly() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut project = Project::create(dir.path(), "demo").expect("create");
        project.config_mut().build.assembler = "definitely-not-an-assembler".to_owned();

        let error = build(&project, BuildOptions::release())
            .await
            .expect_err("must fail");
        assert!(matches!(error, BuildError::ToolMissing { .. }));
        assert!(error.to_string().contains(".ratasm.toml"));
    }

    #[tokio::test]
    async fn a_missing_source_file_is_reported_before_running_anything() {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = Project::create(dir.path(), "demo").expect("create");
        std::fs::remove_file(project.entry_path()).expect("remove");

        let error = build(&project, BuildOptions::release())
            .await
            .expect_err("must fail");
        assert!(matches!(error, BuildError::MissingSource { .. }));
    }

    #[tokio::test]
    async fn a_working_program_builds_and_runs() {
        if !toolchain_available() {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }
        let (_dir, project) = project_with(template::HELLO_WORLD);

        let outcome = build(&project, BuildOptions::release())
            .await
            .expect("build");
        assert!(outcome.success, "build failed: {}", outcome.raw_output());
        let executable = outcome.executable.clone().expect("executable produced");
        assert!(executable.is_file());
        assert_eq!(outcome.counts(), (0, 0));

        let run = run_executable(&project, &executable).await.expect("run");
        assert!(run.is_success(), "program failed: {}", run.combined());
        assert_eq!(run.stdout, "Hello, world!\n");
    }

    #[tokio::test]
    async fn a_syntax_error_produces_a_navigable_diagnostic() {
        if !toolchain_available() {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }
        let source = "section .text\nglobal _start\n_start:\n    mov rax, notareal_symbol\n";
        let (_dir, project) = project_with(source);

        let outcome = build(&project, BuildOptions::release())
            .await
            .expect("build");
        assert!(!outcome.success, "a bad program must not build");
        assert!(outcome.executable.is_none());

        let first = outcome
            .first_navigable()
            .expect("the error must carry a location");
        assert_eq!(first.line, Some(4), "must point at the offending line");
        assert_eq!(first.producer, Producer::Assembler);
    }

    #[tokio::test]
    async fn a_link_error_is_attributed_to_the_linker() {
        if !toolchain_available() {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }
        let source = "section .text\nglobal main\nmain:\n    ret\n";
        let (_dir, project) = project_with(source);

        let outcome = build(&project, BuildOptions::release())
            .await
            .expect("build");
        let linker_messages: Vec<_> = outcome
            .diagnostics
            .iter()
            .filter(|d| d.producer == Producer::Linker)
            .collect();
        assert!(
            !linker_messages.is_empty(),
            "expected a linker message, got: {}",
            outcome.raw_output()
        );
    }

    #[tokio::test]
    async fn the_build_stops_at_the_first_failing_source() {
        if !toolchain_available() {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }
        let dir = tempfile::tempdir().expect("temp dir");
        let project = Project::create(dir.path(), "demo").expect("create");
        std::fs::write(project.entry_path(), "    this is not assembly\n").expect("write");

        let outcome = build(&project, BuildOptions::release())
            .await
            .expect("build");
        assert!(!outcome.success);
        assert_eq!(outcome.steps.len(), 1, "linking must not be attempted");
    }

    #[tokio::test]
    async fn a_nonzero_exit_status_is_reported_faithfully() {
        if !toolchain_available() {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }
        let source =
            "section .text\nglobal _start\n_start:\n    mov rax, 60\n    mov rdi, 3\n    syscall\n";
        let (_dir, project) = project_with(source);

        let outcome = build(&project, BuildOptions::release())
            .await
            .expect("build");
        assert!(outcome.success, "{}", outcome.raw_output());
        let executable = outcome.executable.clone().expect("executable");

        let run = run_executable(&project, &executable).await.expect("run");
        assert_eq!(run.outcome, crate::process::Outcome::Exited(3));
    }

    #[tokio::test]
    async fn a_crashing_program_reports_its_signal() {
        if !toolchain_available() {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }
        let source =
            "section .text\nglobal _start\n_start:\n    xor rax, rax\n    mov rbx, [rax]\n";
        let (_dir, project) = project_with(source);

        let outcome = build(&project, BuildOptions::release())
            .await
            .expect("build");
        assert!(outcome.success, "{}", outcome.raw_output());
        let executable = outcome.executable.clone().expect("executable");

        let run = run_executable(&project, &executable).await.expect("run");
        assert_eq!(run.outcome, crate::process::Outcome::Signalled(11));
        assert_eq!(run.outcome.signal_name(), Some("SIGSEGV"));
    }

    #[tokio::test]
    async fn an_endless_program_is_stopped_by_the_timeout() {
        if !toolchain_available() {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }
        let source = "section .text\nglobal _start\n_start:\n.spin:\n    jmp .spin\n";
        let (_dir, mut project) = project_with(source);
        project.config_mut().run.timeout_ms = 250;

        let outcome = build(&project, BuildOptions::release())
            .await
            .expect("build");
        let executable = outcome.executable.clone().expect("executable");

        let run = run_executable(&project, &executable).await.expect("run");
        assert_eq!(run.outcome, crate::process::Outcome::TimedOut);
    }

    #[tokio::test]
    async fn program_arguments_and_stdin_are_delivered() {
        if !toolchain_available() {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }
        let source = "\
section .bss
    buffer: resb 32

section .text
    global _start
_start:
    xor eax, eax            ; read
    xor edi, edi            ; stdin
    lea rsi, [rel buffer]
    mov rdx, 32
    syscall

    mov rdx, rax            ; however many bytes we got
    mov eax, 1              ; write
    mov edi, 1              ; stdout
    lea rsi, [rel buffer]
    syscall

    mov eax, 60
    xor edi, edi
    syscall
";
        let (_dir, mut project) = project_with(source);
        project.config_mut().run.stdin = Some("echoed\n".to_owned());

        let outcome = build(&project, BuildOptions::release())
            .await
            .expect("build");
        assert!(outcome.success, "{}", outcome.raw_output());
        let executable = outcome.executable.clone().expect("executable");

        let run = run_executable(&project, &executable).await.expect("run");
        assert_eq!(run.stdout, "echoed\n");
    }

    #[tokio::test]
    async fn raw_output_shows_the_commands_that_ran() {
        if !toolchain_available() {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }
        let (_dir, project) = project_with(template::HELLO_WORLD);
        let outcome = build(&project, BuildOptions::release())
            .await
            .expect("build");
        let raw = outcome.raw_output();
        assert!(raw.contains("nasm"), "expected the assembler command");
        assert!(raw.contains("ld"), "expected the linker command");
    }

    #[tokio::test]
    async fn a_successful_build_summary_reports_the_duration() {
        if !toolchain_available() {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }
        let (_dir, project) = project_with(template::HELLO_WORLD);
        let outcome = build(&project, BuildOptions::release())
            .await
            .expect("build");
        assert!(outcome.summary().starts_with("Build succeeded"));
        assert!(outcome.summary().contains("ms"));
    }

    #[tokio::test]
    async fn the_output_directory_is_created_if_missing() {
        if !toolchain_available() {
            eprintln!("skipping: nasm or ld not installed");
            return;
        }
        let (_dir, project) = project_with(template::HELLO_WORLD);
        assert!(!project.output_directory().exists());
        build(&project, BuildOptions::release())
            .await
            .expect("build");
        assert!(project.output_directory().is_dir());
    }
}

//! Entry point for the `ratasm` binary.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use ratasm::assembler::{self, BuildOptions};
use ratasm::project::Project;

/// A terminal IDE and debugger for x86-64 assembly.
#[derive(Debug, Parser)]
#[command(
    name = "ratasm",
    version,
    about,
    long_about = None,
    disable_help_subcommand = true
)]
struct Cli {
    /// Assembly files, or one project directory, to open.
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// Write diagnostics to this file.
    #[arg(long, value_name = "FILE", global = true)]
    log_file: Option<PathBuf>,

    /// Log filter, in `RUST_LOG` syntax.
    #[arg(long, value_name = "FILTER", default_value = "info", global = true)]
    log_filter: String,

    #[command(subcommand)]
    command: Option<Command>,
}

/// What the binary was asked to do.
#[derive(Debug, Subcommand)]
enum Command {
    /// Create a new project with a working hello-world program.
    New {
        /// Directory to create the project in.
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// Assemble and link the project, then exit.
    Build {
        /// Project directory or source file.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
        /// Ask the assembler for debug information.
        #[arg(long)]
        debug: bool,
    },
    /// Build the project and run the result, then exit.
    Run {
        /// Project directory or source file.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },
    /// Report which external tools are installed.
    Doctor,
}

fn main() -> ExitCode {
    ratasm::ui::install_panic_hook();

    let cli = Cli::parse();

    if let Some(path) = &cli.log_file {
        if let Err(error) = ratasm::logging::init_file_logging(path, &cli.log_filter) {
            eprintln!("ratasm: cannot start logging: {error}");
            return ExitCode::FAILURE;
        }
    }

    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("ratasm: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Dispatches to the requested command.
fn run(cli: Cli) -> Result<ExitCode> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?;

    match cli.command {
        Some(Command::New { name }) => runtime.block_on(async { new_project(&name) }),
        Some(Command::Build { path, debug }) => {
            runtime.block_on(build_project(path.as_deref(), debug))
        }
        Some(Command::Run { path }) => runtime.block_on(run_project(path.as_deref())),
        Some(Command::Doctor) => Ok(doctor()),
        None => runtime.block_on(open_interface(&cli.paths)),
    }
}

/// Creates a project directory and reports what was written.
fn new_project(name: &str) -> Result<ExitCode> {
    let root = PathBuf::from(name);
    if root.exists() && root.read_dir().is_ok_and(|mut dir| dir.next().is_some()) {
        anyhow::bail!("{} already exists and is not empty", root.display());
    }

    let project = Project::create(&root, name)
        .with_context(|| format!("cannot create a project in {}", root.display()))?;

    println!("Created {}", root.display());
    println!("  {}", project.entry_path().display());
    println!("  {}", root.join(Project::FILE_NAME).display());
    println!();
    println!("Next:");
    println!("  cd {name}");
    println!("  ratasm");
    Ok(ExitCode::SUCCESS)
}

/// Resolves the project a path refers to.
fn resolve_project(path: Option<&Path>) -> Result<Project> {
    let path = path.map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    if path.is_file() {
        return Ok(Project::for_file_or_discover(&path));
    }
    Project::discover(&path).map_err(Into::into)
}

/// Builds a project and prints the tool output.
async fn build_project(path: Option<&Path>, debug: bool) -> Result<ExitCode> {
    let project = resolve_project(path)?;
    let options = if debug {
        BuildOptions::debug()
    } else {
        BuildOptions::release()
    };

    let outcome = assembler::build(&project, options)
        .await
        .context("the build could not be run")?;

    print!("{}", outcome.raw_output());
    println!("{}", outcome.summary());

    Ok(if outcome.success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Builds and runs a project, forwarding its exit status.
async fn run_project(path: Option<&Path>) -> Result<ExitCode> {
    let project = resolve_project(path)?;
    let outcome = assembler::build(&project, BuildOptions::release())
        .await
        .context("the build could not be run")?;

    if !outcome.success {
        print!("{}", outcome.raw_output());
        println!("{}", outcome.summary());
        return Ok(ExitCode::FAILURE);
    }

    let executable = outcome
        .executable
        .clone()
        .context("the build reported success but produced no executable")?;

    let result = assembler::run_executable(&project, &executable)
        .await
        .with_context(|| format!("cannot run {}", executable.display()))?;

    print!("{}", result.stdout);
    eprint!("{}", result.stderr);

    match result.outcome {
        ratasm::process::Outcome::Exited(0) => Ok(ExitCode::SUCCESS),
        other => {
            eprintln!("ratasm: program {}", other.description());
            Ok(ExitCode::FAILURE)
        }
    }
}

/// Reports whether the external tools ratasm needs are installed.
fn doctor() -> ExitCode {
    use ratasm::process::is_available;

    let checks: [(&str, &str, bool); 3] = [
        ("nasm", "assembling", true),
        ("ld", "linking", true),
        ("gdb", "debugging", false),
    ];

    let mut missing_required = false;
    println!("ratasm {}", env!("CARGO_PKG_VERSION"));
    println!();

    for (tool, purpose, required) in checks {
        let found = is_available(Path::new(tool));
        let mark = if found {
            "found"
        } else if required {
            missing_required = true;
            "MISSING"
        } else {
            "missing (optional)"
        };
        println!("  {tool:<6} {mark:<20} {purpose}");
    }

    if missing_required {
        println!();
        println!("Install the missing tools:");
        println!("  Debian/Ubuntu:  sudo apt install nasm binutils gdb");
        println!("  Fedora:         sudo dnf install nasm binutils gdb");
        println!("  Arch:           sudo pacman -S nasm binutils gdb");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

/// Opens the terminal interface.
async fn open_interface(paths: &[PathBuf]) -> Result<ExitCode> {
    let settings = ratasm::config::Settings::load_default().unwrap_or_else(|error| {
        eprintln!("ratasm: {error}");
        ratasm::config::Settings::default()
    });

    let project = match paths.first() {
        Some(path) => Project::for_file_or_discover(path),
        None => Project::discover(Path::new("."))
            .unwrap_or_else(|_| Project::for_file(Path::new("main.asm"))),
    };

    let mut app =
        ratasm::app::App::new(project, settings).context("cannot load the built-in databases")?;

    let (keymap, errors) = app.settings.keymap();
    app.keymap = keymap;
    if let Some(error) = errors.first() {
        app.status = ratasm::app::Status::warning(error.to_string());
    }

    let (files, missing): (Vec<&Path>, Vec<&Path>) = paths
        .iter()
        .map(PathBuf::as_path)
        .filter(|path| !path.is_dir())
        .partition(|path| path.is_file());

    if files.is_empty() {
        ratasm::app::run::open_project_entry(&mut app);
    } else {
        for path in files.iter().rev() {
            ratasm::app::run::open_initial_file(&mut app, path);
        }
    }

    if let Some(first) = missing.first() {
        app.status = ratasm::app::Status::warning(match missing.len() {
            1 => format!("{}: no such file", first.display()),
            count => format!("{}: no such file, and {} more", first.display(), count - 1),
        });
    }

    let indent = app.settings.indent_width();
    for index in 0..app.workspace.len() {
        app.workspace.set_active(index);
        app.workspace.active_mut().set_indent_width(indent);
    }
    app.workspace.set_active(0);
    app.refresh_project_files();

    let mut guard = ratasm::ui::TerminalGuard::new()
        .context("cannot set up the terminal; is this running in a real terminal?")?;

    let result = ratasm::app::run(app, guard.terminal_mut()).await;
    drop(guard);
    result?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_line_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn no_arguments_opens_the_interface() {
        let cli = Cli::try_parse_from(["ratasm"]).expect("parse");
        assert!(cli.command.is_none());
        assert!(cli.paths.is_empty());
    }

    #[test]
    fn a_bare_path_is_the_file_to_open() {
        let cli = Cli::try_parse_from(["ratasm", "src/main.asm"]).expect("parse");
        assert_eq!(cli.paths, vec![PathBuf::from("src/main.asm")]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn several_paths_are_all_files_to_open() {
        let cli = Cli::try_parse_from(["ratasm", "src/main.asm", "src/util.asm"]).expect("parse");
        assert_eq!(
            cli.paths,
            vec![PathBuf::from("src/main.asm"), PathBuf::from("src/util.asm")]
        );
    }

    #[test]
    fn subcommands_parse_with_their_arguments() {
        let cli = Cli::try_parse_from(["ratasm", "new", "hello"]).expect("parse");
        assert!(matches!(cli.command, Some(Command::New { name }) if name == "hello"));

        let cli = Cli::try_parse_from(["ratasm", "build", "--debug"]).expect("parse");
        assert!(matches!(
            cli.command,
            Some(Command::Build { debug: true, .. })
        ));

        let cli = Cli::try_parse_from(["ratasm", "run", "demo"]).expect("parse");
        assert!(
            matches!(cli.command, Some(Command::Run { path: Some(p) }) if p == Path::new("demo"))
        );

        let cli = Cli::try_parse_from(["ratasm", "doctor"]).expect("parse");
        assert!(matches!(cli.command, Some(Command::Doctor)));
    }

    #[test]
    fn version_and_help_are_available() {
        let error = Cli::try_parse_from(["ratasm", "--version"]).expect_err("exits");
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
        assert!(error.to_string().contains(env!("CARGO_PKG_VERSION")));

        let error = Cli::try_parse_from(["ratasm", "--help"]).expect_err("exits");
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn a_word_that_is_not_a_subcommand_is_taken_as_a_path() {
        let cli = Cli::try_parse_from(["ratasm", "frobnicate", "x"]).expect("parse");
        assert!(cli.command.is_none());
        assert_eq!(cli.paths.len(), 2);
    }

    #[test]
    fn an_unknown_flag_is_still_rejected() {
        assert!(Cli::try_parse_from(["ratasm", "--frobnicate"]).is_err());
    }

    #[test]
    fn log_options_are_global() {
        let cli =
            Cli::try_parse_from(["ratasm", "build", "--log-file", "/tmp/r.log"]).expect("parse");
        assert_eq!(cli.log_file, Some(PathBuf::from("/tmp/r.log")));
        assert_eq!(cli.log_filter, "info");
    }

    #[test]
    fn resolving_a_project_from_a_lone_file_succeeds() {
        let dir = tempfile::tempdir().expect("temp dir");
        let source = dir.path().join("lone.asm");
        std::fs::write(&source, "ret\n").expect("write");

        let project = resolve_project(Some(&source)).expect("resolve");
        assert_eq!(project.entry_path(), source);
    }

    #[test]
    fn resolving_a_directory_without_a_project_file_reports_why() {
        let dir = tempfile::tempdir().expect("temp dir");
        let error = resolve_project(Some(dir.path())).expect_err("must fail");
        assert!(error.to_string().contains(".ratasm.toml"));
    }

    #[test]
    fn creating_a_project_into_a_non_empty_directory_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("existing.txt"), "keep me").expect("write");

        let previous = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(dir.path()).expect("chdir");
        std::fs::create_dir("taken").expect("mkdir");
        std::fs::write("taken/file.txt", "keep me").expect("write");

        let result = new_project("taken");
        std::env::set_current_dir(previous).expect("restore cwd");

        assert!(result.is_err(), "must not overwrite an occupied directory");
    }
}

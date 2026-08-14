//! Project discovery, configuration and scaffolding.
//!
//! A [`Project`] is a configuration plus the directory it was found in. All
//! path resolution happens here, so no other module has to reason about
//! whether a path in the configuration is relative to the project root, the
//! current directory, or something else.
//!
//! # Working without a project file
//!
//! Opening a single `.asm` file with no `.ratasm.toml` anywhere above it is a
//! completely normal thing to do, and it must work. [`Project::for_file`]
//! builds an implicit project rooted at the file's directory with the default
//! toolchain, so build, run and debug behave identically whether or not the
//! user has written a project file.

pub mod config;
pub mod template;

use std::path::{Path, PathBuf};

pub use config::{
    Architecture, BuildSection, ConfigError, ProjectConfig, ProjectSection, RunSection, Syntax,
};

/// Errors from loading or creating a project.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    /// The project file could not be read or written.
    #[error("cannot access {path}: {source}")]
    Io {
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The project file is invalid.
    #[error("{path}: {source}")]
    Config {
        /// The project file.
        path: PathBuf,
        /// What was wrong with it.
        #[source]
        source: ConfigError,
    },
    /// No project file was found.
    #[error("no {} found in {start} or any parent directory", Project::FILE_NAME)]
    NotFound {
        /// Where the search began.
        start: PathBuf,
    },
    /// A project already exists where one was about to be created.
    #[error("{path} already exists")]
    AlreadyExists {
        /// The path that is already occupied.
        path: PathBuf,
    },
}

/// A loaded project: its configuration and the directory it lives in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    root: PathBuf,
    config: ProjectConfig,
    /// `None` for an implicit project built for a lone source file.
    config_path: Option<PathBuf>,
}

impl Project {
    /// The name of a project file.
    pub const FILE_NAME: &'static str = ".ratasm.toml";

    /// The configuration.
    pub fn config(&self) -> &ProjectConfig {
        &self.config
    }

    /// The configuration, mutably.
    pub fn config_mut(&mut self) -> &mut ProjectConfig {
        &mut self.config
    }

    /// The project root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The project file's path, when the project has one on disk.
    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// Whether this project came from a file rather than being implied.
    pub fn is_explicit(&self) -> bool {
        self.config_path.is_some()
    }

    /// The project's display name.
    pub fn name(&self) -> &str {
        &self.config.project.name
    }

    /// Searches `start` and its parents for a project file and loads it.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::NotFound`] when no project file exists in the
    /// directory or any ancestor, or a [`ProjectError::Config`] when one
    /// exists but is invalid.
    pub fn discover(start: &Path) -> Result<Self, ProjectError> {
        let mut directory = if start.is_dir() {
            start.to_path_buf()
        } else {
            start.parent().unwrap_or(Path::new(".")).to_path_buf()
        };

        loop {
            let candidate = directory.join(Self::FILE_NAME);
            if candidate.is_file() {
                return Self::load(&candidate);
            }
            if !directory.pop() {
                return Err(ProjectError::NotFound {
                    start: start.to_path_buf(),
                });
            }
        }
    }

    /// Loads a project from an explicit project file path.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::Io`] if the file cannot be read, or
    /// [`ProjectError::Config`] if it is invalid.
    pub fn load(path: &Path) -> Result<Self, ProjectError> {
        let text = std::fs::read_to_string(path).map_err(|source| ProjectError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        let config = ProjectConfig::from_toml(&text).map_err(|source| ProjectError::Config {
            path: path.to_path_buf(),
            source,
        })?;
        config.validate().map_err(|source| ProjectError::Config {
            path: path.to_path_buf(),
            source,
        })?;

        let root = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf();

        Ok(Self {
            root,
            config,
            config_path: Some(path.to_path_buf()),
        })
    }

    /// Builds an implicit project for a single source file.
    ///
    /// Used when a user opens a `.asm` file directly. The file becomes the
    /// entry point, its directory becomes the root, and everything else takes
    /// its default.
    pub fn for_file(source: &Path) -> Self {
        let root = source
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf();
        let file_name = source
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("main.asm"));
        let name = source
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "program".to_owned());

        let mut config = ProjectConfig::default();
        config.project.name = name;
        config.project.entry = file_name;

        Self {
            root,
            config,
            config_path: None,
        }
    }

    /// Loads the project containing `source`, falling back to an implicit one.
    ///
    /// This is what the application calls when a file is opened: use a real
    /// project file if there is one, otherwise still provide a usable project.
    pub fn for_file_or_discover(source: &Path) -> Self {
        Self::discover(source).unwrap_or_else(|_| Self::for_file(source))
    }

    /// Creates a new project directory with a working hello-world program.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::AlreadyExists`] if a project file is already
    /// there, so an existing project is never overwritten, or
    /// [`ProjectError::Io`] if the files cannot be written.
    pub fn create(root: &Path, name: &str) -> Result<Self, ProjectError> {
        let config_path = root.join(Self::FILE_NAME);
        if config_path.exists() {
            return Err(ProjectError::AlreadyExists { path: config_path });
        }

        let name = if name.trim().is_empty() {
            "ratasm-project"
        } else {
            name.trim()
        };

        let source_directory = root.join("src");
        write_new(&source_directory.join("main.asm"), template::HELLO_WORLD)?;
        write_new(&config_path, &template::project_file(name))?;
        write_new(&root.join(".gitignore"), template::GITIGNORE)?;

        Self::load(&config_path)
    }

    /// Writes the configuration back to the project file.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError::Io`] when the file cannot be written, or
    /// [`ProjectError::Config`] when the configuration cannot be serialised.
    pub fn save(&self) -> Result<PathBuf, ProjectError> {
        let path = self
            .config_path
            .clone()
            .unwrap_or_else(|| self.root.join(Self::FILE_NAME));
        let text = self
            .config
            .to_toml()
            .map_err(|source| ProjectError::Config {
                path: path.clone(),
                source,
            })?;
        std::fs::write(&path, text).map_err(|source| ProjectError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }

    /// Resolves a project-relative path against the root.
    ///
    /// An absolute path is returned unchanged, which matters for paths that
    /// come from somewhere other than the configuration file — the
    /// configuration's own paths are validated to be relative.
    pub fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }

    /// The absolute path of the entry source file.
    pub fn entry_path(&self) -> PathBuf {
        self.resolve(&self.config.project.entry)
    }

    /// The absolute paths of every source file.
    pub fn source_paths(&self) -> Vec<PathBuf> {
        self.config
            .all_sources()
            .iter()
            .map(|source| self.resolve(source))
            .collect()
    }

    /// The absolute path of the build output directory.
    pub fn output_directory(&self) -> PathBuf {
        self.resolve(&self.config.build.output_directory)
    }

    /// The object file produced for `source`.
    ///
    /// Object files are named after the source's stem and placed in the
    /// output directory, so two sources in different directories with the
    /// same name would collide; the name includes the parent directory when
    /// there is one to keep them apart.
    pub fn object_path(&self, source: &Path) -> PathBuf {
        let stem = source
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "output".to_owned());
        let parent = source
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned());

        let file_name = match parent {
            Some(parent) if !parent.is_empty() && parent != "." => format!("{parent}_{stem}.o"),
            _ => format!("{stem}.o"),
        };
        self.output_directory().join(file_name)
    }

    /// The absolute path of the linked executable.
    pub fn executable_path(&self) -> PathBuf {
        match &self.config.build.executable {
            Some(path) => self.resolve(path),
            None => {
                let stem = self
                    .config
                    .project
                    .entry
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_else(|| self.config.project.name.clone());
                self.output_directory().join(stem)
            }
        }
    }

    /// The working directory a built program should run in.
    pub fn run_directory(&self) -> PathBuf {
        match &self.config.run.working_directory {
            Some(path) => self.resolve(path),
            None => self.root.clone(),
        }
    }
}

/// Writes `contents` to `path`, creating parent directories.
///
/// Refuses to overwrite an existing file: project creation must never destroy
/// something the user already has.
fn write_new(path: &Path, contents: &str) -> Result<(), ProjectError> {
    if path.exists() {
        return Err(ProjectError::AlreadyExists {
            path: path.to_path_buf(),
        });
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ProjectError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(path, contents).map_err(|source| ProjectError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[test]
    fn creating_a_project_writes_a_working_hello_world() {
        let dir = temp_dir();
        let project = Project::create(dir.path(), "hello").expect("create");

        assert_eq!(project.name(), "hello");
        assert!(project.entry_path().is_file(), "entry source must exist");
        assert!(dir.path().join(".ratasm.toml").is_file());
        assert!(dir.path().join(".gitignore").is_file());

        let source = std::fs::read_to_string(project.entry_path()).expect("read");
        assert!(source.contains("global _start"));
    }

    #[test]
    fn creating_over_an_existing_project_is_refused() {
        // Scaffolding must never overwrite someone's work.
        let dir = temp_dir();
        Project::create(dir.path(), "first").expect("create");
        let error = Project::create(dir.path(), "second").expect_err("must refuse");
        assert!(matches!(error, ProjectError::AlreadyExists { .. }));

        let project = Project::discover(dir.path()).expect("discover");
        assert_eq!(project.name(), "first", "the original must survive");
    }

    #[test]
    fn creating_refuses_to_overwrite_an_existing_source_file() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.path().join("src")).expect("mkdir");
        std::fs::write(dir.path().join("src/main.asm"), "; my work\n").expect("write");

        Project::create(dir.path(), "hello").expect_err("must refuse");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/main.asm")).expect("read"),
            "; my work\n"
        );
    }

    #[test]
    fn discovery_walks_up_to_the_project_root() {
        let dir = temp_dir();
        Project::create(dir.path(), "hello").expect("create");
        let deep = dir.path().join("src/nested/deeper");
        std::fs::create_dir_all(&deep).expect("mkdir");

        let project = Project::discover(&deep).expect("discover");
        assert_eq!(project.name(), "hello");
        assert_eq!(
            std::fs::canonicalize(project.root()).expect("canonicalize"),
            std::fs::canonicalize(dir.path()).expect("canonicalize")
        );
    }

    #[test]
    fn discovery_starting_from_a_file_uses_its_directory() {
        let dir = temp_dir();
        let project = Project::create(dir.path(), "hello").expect("create");
        let found = Project::discover(&project.entry_path()).expect("discover");
        assert_eq!(found.name(), "hello");
    }

    #[test]
    fn discovery_reports_when_there_is_no_project() {
        let dir = temp_dir();
        let error = Project::discover(dir.path()).expect_err("must fail");
        assert!(matches!(error, ProjectError::NotFound { .. }));
    }

    #[test]
    fn an_invalid_project_file_is_reported_with_its_path() {
        let dir = temp_dir();
        let path = dir.path().join(Project::FILE_NAME);
        std::fs::write(&path, "[project]\narchitecture = \"aarch64\"\n").expect("write");

        let error = Project::discover(dir.path()).expect_err("must fail");
        assert!(matches!(error, ProjectError::Config { .. }));
        assert!(error.to_string().contains("not supported yet"));
    }

    #[test]
    fn a_lone_source_file_gets_an_implicit_project() {
        // Opening one file with no project file must still build and run.
        let dir = temp_dir();
        let source = dir.path().join("scratch.asm");
        std::fs::write(&source, "ret\n").expect("write");

        let project = Project::for_file(&source);
        assert!(!project.is_explicit());
        assert_eq!(project.name(), "scratch");
        assert_eq!(project.entry_path(), source);
        assert_eq!(project.config().build.assembler, "nasm");
    }

    #[test]
    fn opening_a_file_prefers_a_real_project_over_an_implicit_one() {
        let dir = temp_dir();
        let project = Project::create(dir.path(), "hello").expect("create");
        let found = Project::for_file_or_discover(&project.entry_path());
        assert!(found.is_explicit());
        assert_eq!(found.name(), "hello");
    }

    #[test]
    fn opening_a_file_outside_any_project_falls_back_cleanly() {
        let dir = temp_dir();
        let source = dir.path().join("lone.asm");
        std::fs::write(&source, "ret\n").expect("write");
        let project = Project::for_file_or_discover(&source);
        assert!(!project.is_explicit());
        assert_eq!(project.name(), "lone");
    }

    #[test]
    fn paths_resolve_against_the_project_root() {
        let dir = temp_dir();
        let project = Project::create(dir.path(), "hello").expect("create");

        assert_eq!(project.entry_path(), dir.path().join("src/main.asm"));
        assert_eq!(project.output_directory(), dir.path().join("build"));
        assert_eq!(project.executable_path(), dir.path().join("build/main"));
        assert_eq!(project.run_directory(), dir.path());
    }

    #[test]
    fn an_explicit_executable_path_overrides_the_default() {
        let dir = temp_dir();
        let path = dir.path().join(Project::FILE_NAME);
        std::fs::write(&path, "[build]\nexecutable = \"bin/program\"\n").expect("write");

        let project = Project::load(&path).expect("load");
        assert_eq!(project.executable_path(), dir.path().join("bin/program"));
    }

    #[test]
    fn object_paths_land_in_the_output_directory() {
        let dir = temp_dir();
        let project = Project::create(dir.path(), "hello").expect("create");
        let object = project.object_path(Path::new("src/main.asm"));
        assert_eq!(object, dir.path().join("build/src_main.o"));
    }

    #[test]
    fn sources_in_different_directories_do_not_collide() {
        // Two files both called main.asm must produce different objects.
        let dir = temp_dir();
        let project = Project::create(dir.path(), "hello").expect("create");
        let first = project.object_path(Path::new("boot/main.asm"));
        let second = project.object_path(Path::new("kernel/main.asm"));
        assert_ne!(first, second);
    }

    #[test]
    fn source_paths_include_every_configured_source() {
        let dir = temp_dir();
        let path = dir.path().join(Project::FILE_NAME);
        std::fs::write(
            &path,
            "[project]\nentry = \"src/main.asm\"\nsources = [\"src/util.asm\"]\n",
        )
        .expect("write");

        let project = Project::load(&path).expect("load");
        assert_eq!(
            project.source_paths(),
            vec![
                dir.path().join("src/main.asm"),
                dir.path().join("src/util.asm")
            ]
        );
    }

    #[test]
    fn saving_round_trips_a_modified_configuration() {
        let dir = temp_dir();
        let mut project = Project::create(dir.path(), "hello").expect("create");
        project.config_mut().run.timeout_ms = 12_345;
        project.save().expect("save");

        let reloaded = Project::discover(dir.path()).expect("reload");
        assert_eq!(reloaded.config().run.timeout_ms, 12_345);
    }

    #[test]
    fn saving_an_implicit_project_writes_a_project_file() {
        let dir = temp_dir();
        let source = dir.path().join("lone.asm");
        std::fs::write(&source, "ret\n").expect("write");

        let project = Project::for_file(&source);
        let written = project.save().expect("save");
        assert_eq!(written, dir.path().join(Project::FILE_NAME));
        assert!(Project::discover(dir.path()).is_ok());
    }

    #[test]
    fn a_created_project_builds_a_valid_configuration() {
        let dir = temp_dir();
        let project = Project::create(dir.path(), "hello").expect("create");
        project.config().validate().expect("must be valid");
        assert_eq!(project.config().project.architecture, Architecture::X86_64);
        assert_eq!(project.config().project.syntax, Syntax::Nasm);
    }

    #[test]
    fn an_empty_project_name_falls_back_to_a_default() {
        let dir = temp_dir();
        let project = Project::create(dir.path(), "   ").expect("create");
        assert_eq!(project.name(), "ratasm-project");
    }
}

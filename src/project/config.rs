//! The `.ratasm.toml` schema.
//!
//! Every field has a default, so an empty file — or no file at all — yields a
//! working configuration for the common case: one NASM source assembled with
//! `nasm -f elf64` and linked with `ld`. Users only write down what differs
//! from that.
//!
//! # Forward compatibility
//!
//! [`Architecture`] and [`Syntax`] are enumerations rather than free strings.
//! Only x86-64 and NASM are implemented today, but a project file naming
//! `aarch64` gets a precise "not supported yet" message instead of a confusing
//! failure deep inside the assembler. Adding a backend later means adding a
//! variant and its toolchain defaults, not finding every place a string was
//! compared.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The target architecture of a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Architecture {
    /// 64-bit x86, the only architecture currently implemented.
    #[default]
    #[serde(rename = "x86_64", alias = "x86-64", alias = "amd64")]
    X86_64,
    /// 64-bit ARM. Recognised but not yet implemented.
    #[serde(rename = "aarch64", alias = "arm64")]
    Aarch64,
    /// 64-bit RISC-V. Recognised but not yet implemented.
    #[serde(rename = "riscv64")]
    RiscV64,
}

impl Architecture {
    /// Whether ratasm can build and debug this architecture today.
    pub const fn is_supported(self) -> bool {
        matches!(self, Architecture::X86_64)
    }

    /// The identifier used in configuration files.
    pub const fn id(self) -> &'static str {
        match self {
            Architecture::X86_64 => "x86_64",
            Architecture::Aarch64 => "aarch64",
            Architecture::RiscV64 => "riscv64",
        }
    }

    /// The default NASM object format for this architecture.
    pub const fn object_format(self) -> &'static str {
        match self {
            Architecture::X86_64 => "elf64",
            Architecture::Aarch64 | Architecture::RiscV64 => "elf64",
        }
    }
}

/// The assembly dialect a project is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Syntax {
    /// NASM syntax, the only dialect currently implemented.
    #[default]
    Nasm,
    /// GNU assembler syntax. Recognised but not yet implemented.
    Gas,
}

impl Syntax {
    /// Whether ratasm can assemble this dialect today.
    pub const fn is_supported(self) -> bool {
        matches!(self, Syntax::Nasm)
    }

    /// The identifier used in configuration files.
    pub const fn id(self) -> &'static str {
        match self {
            Syntax::Nasm => "nasm",
            Syntax::Gas => "gas",
        }
    }

    /// The assembler this dialect is normally built with.
    pub const fn default_assembler(self) -> &'static str {
        match self {
            Syntax::Nasm => "nasm",
            Syntax::Gas => "as",
        }
    }
}

/// Identity and layout of the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectSection {
    /// Human-readable project name.
    pub name: String,
    /// The main source file, relative to the project root.
    pub entry: PathBuf,
    /// Target architecture.
    pub architecture: Architecture,
    /// Assembly dialect.
    pub syntax: Syntax,
    /// Additional sources assembled alongside the entry file.
    pub sources: Vec<PathBuf>,
    /// Directories searched for `%include` files.
    pub include_directories: Vec<PathBuf>,
}

impl Default for ProjectSection {
    fn default() -> Self {
        Self {
            name: "ratasm-project".to_owned(),
            entry: PathBuf::from("src/main.asm"),
            architecture: Architecture::default(),
            syntax: Syntax::default(),
            sources: Vec::new(),
            include_directories: Vec::new(),
        }
    }
}

/// How the project is assembled and linked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BuildSection {
    /// The assembler executable.
    pub assembler: String,
    /// Arguments passed to the assembler before the input file.
    pub assembler_args: Vec<String>,
    /// The linker executable.
    pub linker: String,
    /// Extra arguments passed to the linker.
    pub linker_args: Vec<String>,
    /// Pre-built object files to link in.
    pub objects: Vec<PathBuf>,
    /// Directory for intermediate and final build artefacts.
    pub output_directory: PathBuf,
    /// Explicit path for the linked executable.
    ///
    /// When absent the executable is named after the entry file and placed in
    /// [`BuildSection::output_directory`].
    pub executable: Option<PathBuf>,
}

impl Default for BuildSection {
    fn default() -> Self {
        Self {
            assembler: "nasm".to_owned(),
            assembler_args: vec!["-f".to_owned(), "elf64".to_owned()],
            linker: "ld".to_owned(),
            linker_args: Vec::new(),
            objects: Vec::new(),
            output_directory: PathBuf::from("build"),
            executable: None,
        }
    }
}

impl BuildSection {
    /// Whether the assembler arguments already request debug information.
    ///
    /// Checked before adding `-g`, so a user who configured their own debug
    /// flags does not get a duplicate that some assembler versions reject.
    pub fn requests_debug_info(&self) -> bool {
        self.assembler_args
            .iter()
            .any(|arg| arg == "-g" || arg.starts_with("-F") || arg.starts_with("-gdwarf"))
    }
}

/// How the built program is run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RunSection {
    /// Arguments passed to the program.
    pub args: Vec<String>,
    /// Maximum run time in milliseconds before the program is killed.
    pub timeout_ms: u64,
    /// Working directory, relative to the project root.
    pub working_directory: Option<PathBuf>,
    /// Text supplied on the program's standard input.
    pub stdin: Option<String>,
}

impl Default for RunSection {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            timeout_ms: 5_000,
            working_directory: None,
            stdin: None,
        }
    }
}

impl RunSection {
    /// The timeout as a [`Duration`], or `None` when disabled with zero.
    ///
    /// Zero meaning "no limit" gives users an explicit way to run a program
    /// that is supposed to keep going, without inventing a second field.
    pub fn timeout(&self) -> Option<Duration> {
        if self.timeout_ms == 0 {
            None
        } else {
            Some(Duration::from_millis(self.timeout_ms))
        }
    }
}

/// A complete `.ratasm.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectConfig {
    /// Identity and layout.
    pub project: ProjectSection,
    /// Toolchain settings.
    pub build: BuildSection,
    /// Run settings.
    pub run: RunSection,
}

/// Problems found while validating a configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The configuration file is not valid TOML or has unknown fields.
    #[error("invalid project file: {message}")]
    Parse {
        /// The parser's message.
        message: String,
    },
    /// The architecture is recognised but not implemented.
    #[error("architecture '{0}' is recognised but not supported yet; only x86_64 is implemented")]
    UnsupportedArchitecture(String),
    /// The dialect is recognised but not implemented.
    #[error("syntax '{0}' is recognised but not supported yet; only nasm is implemented")]
    UnsupportedSyntax(String),
    /// A required field was left empty.
    #[error("{field} must not be empty")]
    EmptyField {
        /// The offending field.
        field: &'static str,
    },
    /// A path escaped the project root.
    #[error("{field} must stay inside the project directory, but '{path}' does not")]
    EscapesRoot {
        /// The offending field.
        field: &'static str,
        /// The path as written.
        path: PathBuf,
    },
}

impl ProjectConfig {
    /// Parses a configuration from TOML text.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Parse`] when the text is not valid TOML or names
    /// a field that does not exist. Rejecting unknown fields turns a silent
    /// typo — `assember_args` — into a message at load time.
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|error| ConfigError::Parse {
            message: error.message().to_owned(),
        })
    }

    /// Renders the configuration back to TOML.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Parse`] if serialisation fails, which cannot
    /// happen for the shapes in this module but is surfaced rather than
    /// panicking.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(|error| ConfigError::Parse {
            message: error.to_string(),
        })
    }

    /// Checks the configuration for problems that would break a build.
    ///
    /// # Errors
    ///
    /// Returns the first problem found. Paths are checked for escaping the
    /// project root so a configuration file cannot direct writes to arbitrary
    /// locations on the filesystem.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !self.project.architecture.is_supported() {
            return Err(ConfigError::UnsupportedArchitecture(
                self.project.architecture.id().to_owned(),
            ));
        }
        if !self.project.syntax.is_supported() {
            return Err(ConfigError::UnsupportedSyntax(
                self.project.syntax.id().to_owned(),
            ));
        }
        if self.project.name.trim().is_empty() {
            return Err(ConfigError::EmptyField {
                field: "project.name",
            });
        }
        if self.build.assembler.trim().is_empty() {
            return Err(ConfigError::EmptyField {
                field: "build.assembler",
            });
        }
        if self.build.linker.trim().is_empty() {
            return Err(ConfigError::EmptyField {
                field: "build.linker",
            });
        }
        if self.project.entry.as_os_str().is_empty() {
            return Err(ConfigError::EmptyField {
                field: "project.entry",
            });
        }

        check_contained("project.entry", &self.project.entry)?;
        for source in &self.project.sources {
            check_contained("project.sources", source)?;
        }
        check_contained("build.output_directory", &self.build.output_directory)?;
        if let Some(executable) = &self.build.executable {
            check_contained("build.executable", executable)?;
        }

        Ok(())
    }

    /// Every source file, with the entry point first and duplicates removed.
    pub fn all_sources(&self) -> Vec<PathBuf> {
        let mut sources = vec![self.project.entry.clone()];
        for source in &self.project.sources {
            if !sources.contains(source) {
                sources.push(source.clone());
            }
        }
        sources
    }
}

/// Rejects absolute paths and paths that climb out of the project root.
fn check_contained(field: &'static str, path: &Path) -> Result<(), ConfigError> {
    use std::path::Component;
    let escapes = path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir));
    if escapes {
        return Err(ConfigError::EscapesRoot {
            field,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example from the documentation, which must always parse.
    const EXAMPLE: &str = r#"
[project]
name = "hello"
entry = "src/main.asm"
architecture = "x86_64"
syntax = "nasm"

[build]
assembler = "nasm"
assembler_args = ["-f", "elf64"]
linker = "ld"

[run]
args = []
timeout_ms = 5000
"#;

    #[test]
    fn the_documented_example_parses() {
        let config = ProjectConfig::from_toml(EXAMPLE).expect("parse");
        assert_eq!(config.project.name, "hello");
        assert_eq!(config.project.entry, PathBuf::from("src/main.asm"));
        assert_eq!(config.project.architecture, Architecture::X86_64);
        assert_eq!(config.project.syntax, Syntax::Nasm);
        assert_eq!(config.build.assembler, "nasm");
        assert_eq!(config.build.assembler_args, ["-f", "elf64"]);
        assert_eq!(config.build.linker, "ld");
        assert_eq!(config.run.timeout_ms, 5000);
        config.validate().expect("valid");
    }

    #[test]
    fn an_empty_file_yields_working_defaults() {
        let config = ProjectConfig::from_toml("").expect("parse");
        assert_eq!(config.build.assembler, "nasm");
        assert_eq!(config.build.assembler_args, ["-f", "elf64"]);
        assert_eq!(config.build.linker, "ld");
        assert_eq!(config.project.entry, PathBuf::from("src/main.asm"));
        config.validate().expect("defaults must be valid");
    }

    #[test]
    fn a_partial_section_keeps_the_other_defaults() {
        let config = ProjectConfig::from_toml("[build]\nlinker = \"ld.lld\"\n").expect("parse");
        assert_eq!(config.build.linker, "ld.lld");
        assert_eq!(
            config.build.assembler, "nasm",
            "untouched field keeps default"
        );
    }

    #[test]
    fn a_misspelled_field_is_rejected_rather_than_ignored() {
        // Silently ignoring this would leave the user debugging a build that
        // never picked up their setting.
        let error =
            ProjectConfig::from_toml("[build]\nassember = \"nasm\"\n").expect_err("must fail");
        assert!(matches!(error, ConfigError::Parse { .. }));
    }

    #[test]
    fn malformed_toml_is_reported_not_panicked() {
        let error = ProjectConfig::from_toml("[project\nname =").expect_err("must fail");
        assert!(matches!(error, ConfigError::Parse { .. }));
    }

    #[test]
    fn architecture_aliases_are_accepted() {
        for spelling in ["x86_64", "x86-64", "amd64"] {
            let text = format!("[project]\narchitecture = \"{spelling}\"\n");
            let config = ProjectConfig::from_toml(&text)
                .unwrap_or_else(|error| panic!("{spelling}: {error}"));
            assert_eq!(config.project.architecture, Architecture::X86_64);
        }
    }

    #[test]
    fn an_unimplemented_architecture_gives_a_clear_message() {
        let config =
            ProjectConfig::from_toml("[project]\narchitecture = \"aarch64\"\n").expect("parse");
        let error = config.validate().expect_err("must not validate");
        assert!(matches!(error, ConfigError::UnsupportedArchitecture(_)));
        assert!(error.to_string().contains("not supported yet"));
    }

    #[test]
    fn an_unimplemented_syntax_gives_a_clear_message() {
        let config = ProjectConfig::from_toml("[project]\nsyntax = \"gas\"\n").expect("parse");
        let error = config.validate().expect_err("must not validate");
        assert!(matches!(error, ConfigError::UnsupportedSyntax(_)));
    }

    #[test]
    fn an_unknown_architecture_fails_to_parse() {
        assert!(ProjectConfig::from_toml("[project]\narchitecture = \"pdp11\"\n").is_err());
    }

    #[test]
    fn empty_required_fields_are_rejected() {
        for (text, field) in [
            ("[project]\nname = \"  \"\n", "project.name"),
            ("[build]\nassembler = \"\"\n", "build.assembler"),
            ("[build]\nlinker = \"\"\n", "build.linker"),
            ("[project]\nentry = \"\"\n", "project.entry"),
        ] {
            let config = ProjectConfig::from_toml(text).expect("parse");
            let error = config.validate().expect_err("must not validate");
            assert_eq!(error, ConfigError::EmptyField { field });
        }
    }

    #[test]
    fn a_path_escaping_the_project_root_is_rejected() {
        // A project file must not be able to direct writes outside its own
        // directory.
        for text in [
            "[project]\nentry = \"../../etc/passwd\"\n",
            "[project]\nentry = \"/etc/passwd\"\n",
            "[build]\noutput_directory = \"../elsewhere\"\n",
            "[build]\nexecutable = \"/usr/bin/evil\"\n",
        ] {
            let config = ProjectConfig::from_toml(text).expect("parse");
            let error = config.validate().expect_err("must reject {text}");
            assert!(
                matches!(error, ConfigError::EscapesRoot { .. }),
                "wrong error for {text}: {error}"
            );
        }
    }

    #[test]
    fn a_nested_relative_path_is_allowed() {
        let config =
            ProjectConfig::from_toml("[project]\nentry = \"src/boot/main.asm\"\n").expect("parse");
        config.validate().expect("nested paths are fine");
    }

    #[test]
    fn a_zero_timeout_means_no_limit() {
        let config = ProjectConfig::from_toml("[run]\ntimeout_ms = 0\n").expect("parse");
        assert_eq!(config.run.timeout(), None);
    }

    #[test]
    fn a_positive_timeout_converts_to_a_duration() {
        let config = ProjectConfig::from_toml("[run]\ntimeout_ms = 250\n").expect("parse");
        assert_eq!(config.run.timeout(), Some(Duration::from_millis(250)));
    }

    #[test]
    fn sources_start_with_the_entry_point_and_are_deduplicated() {
        let config = ProjectConfig::from_toml(
            "[project]\nentry = \"src/main.asm\"\nsources = [\"src/util.asm\", \"src/main.asm\"]\n",
        )
        .expect("parse");
        assert_eq!(
            config.all_sources(),
            vec![PathBuf::from("src/main.asm"), PathBuf::from("src/util.asm")]
        );
    }

    #[test]
    fn configuration_round_trips_through_toml() {
        let original = ProjectConfig::from_toml(EXAMPLE).expect("parse");
        let rendered = original.to_toml().expect("serialise");
        let reparsed = ProjectConfig::from_toml(&rendered).expect("reparse");
        assert_eq!(original, reparsed);
    }

    #[test]
    fn debug_flags_are_detected_so_they_are_not_added_twice() {
        let mut build = BuildSection::default();
        assert!(!build.requests_debug_info());
        build.assembler_args.push("-g".to_owned());
        assert!(build.requests_debug_info());

        let mut build = BuildSection::default();
        build.assembler_args.push("-gdwarf".to_owned());
        assert!(build.requests_debug_info());
    }

    #[test]
    fn architecture_and_syntax_identifiers_are_stable() {
        assert_eq!(Architecture::X86_64.id(), "x86_64");
        assert_eq!(Architecture::X86_64.object_format(), "elf64");
        assert_eq!(Syntax::Nasm.id(), "nasm");
        assert_eq!(Syntax::Nasm.default_assembler(), "nasm");
        assert_eq!(Syntax::Gas.default_assembler(), "as");
    }
}

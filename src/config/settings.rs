//! User settings, loaded from a configuration file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ui::theme::ThemeKind;

/// Editor preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EditorSettings {
    /// Spaces inserted by one indent.
    pub indent_width: usize,
    /// Whether to show line numbers.
    pub line_numbers: bool,
    /// Whether to highlight the line the cursor is on.
    pub highlight_current_line: bool,
    /// Whether to show a marker on the matching bracket.
    pub match_brackets: bool,
    /// Whether typing an opening bracket or quote inserts its partner.
    pub auto_close_pairs: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            indent_width: 4,
            line_numbers: true,
            highlight_current_line: true,
            match_brackets: true,
            auto_close_pairs: true,
        }
    }
}

/// Debugger preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DebuggerSettings {
    /// The debugger executable.
    pub gdb: String,
    /// How long to wait for a command before giving up.
    pub timeout_ms: u64,
    /// How many bytes the memory panel reads at a time.
    pub memory_window: usize,
    /// How many stack slots to show around the stack pointer.
    pub stack_depth: usize,
    /// Record execution so it can be stepped backwards.
    pub record: bool,
}

impl Default for DebuggerSettings {
    fn default() -> Self {
        Self {
            gdb: "gdb".to_owned(),
            timeout_ms: 10_000,
            memory_window: 256,
            stack_depth: 16,
            record: true,
        }
    }
}

/// Appearance preferences.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppearanceSettings {
    /// The colour theme.
    pub theme: ThemeKind,
    /// Force Unicode glyphs on or off.
    pub unicode: Option<bool>,
}

/// Everything a user can configure.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Appearance.
    pub appearance: AppearanceSettings,
    /// Editor behaviour.
    pub editor: EditorSettings,
    /// Debugger behaviour.
    pub debugger: DebuggerSettings,
    /// Key binding overrides, keyed by chord.
    pub keys: BTreeMap<String, String>,
}

/// Errors from loading settings.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// The file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid TOML, or names a field that does not exist.
    #[error("{path}: {message}")]
    Parse {
        /// The file involved.
        path: PathBuf,
        /// What the parser objected to.
        message: String,
    },
    /// The file could not be written.
    #[error("cannot write {path}: {source}")]
    Write {
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
}

impl Settings {
    /// The file name settings are stored in.
    pub const FILE_NAME: &'static str = "config.toml";

    /// Parses settings from TOML text.
    pub fn from_toml(path: &Path, text: &str) -> Result<Self, SettingsError> {
        toml::from_str(text).map_err(|error| SettingsError::Parse {
            path: path.to_path_buf(),
            message: error.message().to_owned(),
        })
    }

    /// Loads settings from `path`, or the defaults when it does not exist.
    pub fn load(path: &Path) -> Result<Self, SettingsError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path).map_err(|source| SettingsError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml(path, &text)
    }

    /// Loads settings from the default location.
    pub fn load_default() -> Result<Self, SettingsError> {
        match default_path() {
            Some(path) => Self::load(&path),
            None => Ok(Self::default()),
        }
    }

    /// Writes settings to `path`, creating parent directories.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        let text = toml::to_string_pretty(self).map_err(|error| SettingsError::Parse {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| SettingsError::Write {
                path: path.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(path, text).map_err(|source| SettingsError::Write {
            path: path.to_path_buf(),
            source,
        })
    }

    /// The indent width, clamped to something usable.
    pub fn indent_width(&self) -> usize {
        self.editor.indent_width.clamp(1, 16)
    }

    /// The debugger command timeout.
    pub fn debugger_timeout(&self) -> std::time::Duration {
        let millis = if self.debugger.timeout_ms == 0 {
            DebuggerSettings::default().timeout_ms
        } else {
            self.debugger.timeout_ms
        };
        std::time::Duration::from_millis(millis)
    }

    /// How many bytes the memory panel should read, clamped to a sane range.
    pub fn memory_window(&self) -> usize {
        self.debugger.memory_window.clamp(16, 4096)
    }

    /// How many stack slots to show, clamped to a sane range.
    pub fn stack_depth(&self) -> usize {
        self.debugger.stack_depth.clamp(4, 128)
    }

    /// Builds the theme these settings describe.
    pub fn theme(&self) -> crate::ui::Theme {
        match self.appearance.unicode {
            Some(unicode) => crate::ui::Theme::new(self.appearance.theme, unicode),
            None => crate::ui::Theme::detect(self.appearance.theme),
        }
    }

    /// Builds the keymap these settings describe.
    pub fn keymap(&self) -> (super::Keymap, Vec<super::KeymapError>) {
        let mut keymap = super::Keymap::defaults();
        let errors = keymap.apply_overrides(&self.keys);
        (keymap, errors)
    }
}

/// The default settings file location.
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("ratasm").join(Settings::FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Settings, SettingsError> {
        Settings::from_toml(Path::new("test.toml"), text)
    }

    #[test]
    fn an_empty_file_gives_the_defaults() {
        let settings = parse("").expect("parses");
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.appearance.theme, ThemeKind::Dark);
        assert_eq!(settings.editor.indent_width, 4);
        assert_eq!(settings.debugger.gdb, "gdb");
    }

    #[test]
    fn a_partial_file_keeps_the_other_defaults() {
        let settings = parse("[appearance]\ntheme = \"light\"\n").expect("parses");
        assert_eq!(settings.appearance.theme, ThemeKind::Light);
        assert_eq!(settings.editor.indent_width, 4, "untouched field");
    }

    #[test]
    fn every_section_parses() {
        let settings = parse(
            r#"
[appearance]
theme = "colorblind"
unicode = false

[editor]
indent_width = 8
line_numbers = false

[debugger]
gdb = "gdb-multiarch"
timeout_ms = 30000
stack_depth = 32

[keys]
"ctrl+b" = "build.build"
"#,
        )
        .expect("parses");

        assert_eq!(settings.appearance.theme, ThemeKind::Colorblind);
        assert_eq!(settings.appearance.unicode, Some(false));
        assert_eq!(settings.editor.indent_width, 8);
        assert!(!settings.editor.line_numbers);
        assert_eq!(settings.debugger.gdb, "gdb-multiarch");
        assert_eq!(settings.debugger.stack_depth, 32);
        assert_eq!(
            settings.keys.get("ctrl+b").map(String::as_str),
            Some("build.build")
        );
    }

    #[test]
    fn a_misspelled_field_is_rejected_rather_than_ignored() {
        let error = parse("[editor]\nindent_with = 8\n").expect_err("must fail");
        assert!(matches!(error, SettingsError::Parse { .. }));
    }

    #[test]
    fn malformed_toml_is_reported_not_panicked() {
        assert!(parse("[appearance\ntheme =").is_err());
    }

    #[test]
    fn an_unknown_theme_name_is_rejected() {
        assert!(parse("[appearance]\ntheme = \"solarized\"\n").is_err());
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = Settings::load(&dir.path().join("absent.toml")).expect("defaults");
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn a_present_but_broken_file_is_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[editor]\nnonsense = 1\n").expect("write");
        assert!(Settings::load(&path).is_err());
    }

    #[test]
    fn settings_round_trip_through_a_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested/config.toml");

        let mut settings = Settings::default();
        settings.appearance.theme = ThemeKind::Light;
        settings.editor.indent_width = 2;
        settings
            .keys
            .insert("ctrl+b".to_owned(), "build.build".to_owned());

        settings.save(&path).expect("save");
        let reloaded = Settings::load(&path).expect("load");
        assert_eq!(reloaded, settings);
    }

    #[test]
    fn out_of_range_values_are_clamped_rather_than_rejected() {
        let settings = parse(
            "[editor]\nindent_width = 0\n\n[debugger]\nmemory_window = 1\nstack_depth = 9999\n",
        )
        .expect("parses");

        assert_eq!(settings.indent_width(), 1);
        assert_eq!(settings.memory_window(), 16);
        assert_eq!(settings.stack_depth(), 128);
    }

    #[test]
    fn a_zero_debugger_timeout_falls_back_to_the_default() {
        let settings = parse("[debugger]\ntimeout_ms = 0\n").expect("parses");
        assert_eq!(
            settings.debugger_timeout(),
            std::time::Duration::from_millis(DebuggerSettings::default().timeout_ms)
        );
    }

    #[test]
    fn the_theme_honours_an_explicit_unicode_choice() {
        let settings = parse("[appearance]\nunicode = false\n").expect("parses");
        assert!(settings.theme().symbols().breakpoint.is_ascii());

        let settings = parse("[appearance]\nunicode = true\n").expect("parses");
        assert!(!settings.theme().symbols().breakpoint.is_ascii());
    }

    #[test]
    fn key_overrides_are_applied_and_problems_reported() {
        let settings =
            parse("[keys]\n\"ctrl+b\" = \"build.build\"\n\"ctrl+j\" = \"no.such.command\"\n")
                .expect("parses");

        let (keymap, errors) = settings.keymap();
        assert_eq!(errors.len(), 1, "the bad binding is reported");
        assert_eq!(
            keymap.command_for(super::super::parse_binding("ctrl+b").expect("parses")),
            Some(&crate::command::Command::Build),
            "the good binding took effect"
        );
    }

    #[test]
    fn the_default_path_follows_the_xdg_specification() {
        let path = default_path().expect("a path");
        let text = path.display().to_string();
        assert!(text.ends_with("ratasm/config.toml"), "{text}");
    }
}

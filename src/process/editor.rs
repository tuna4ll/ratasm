//! Launching the user's own editor, and driving Neovim through its server.
//!
//! `$VISUAL` and `$EDITOR` are shell words by convention (`code --wait`,
//! `emacsclient -t`), so the command goes through `sh -c` like git's does.
//! The file is passed as a positional parameter, never spliced into the
//! script, so its name is still only a name.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::CommandSpec;
use crate::config::settings::EditorSettings;

const TAKES_LINE: [&str; 13] = [
    "vi",
    "vim",
    "nvim",
    "gvim",
    "view",
    "nano",
    "pico",
    "emacs",
    "emacsclient",
    "micro",
    "kak",
    "joe",
    "mg",
];

/// The editor used when nothing else names one.
pub const FALLBACK: &str = "vi";

const REMOTE_TIMEOUT: Duration = Duration::from_secs(3);

/// The editor to run and how it takes a line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    /// The shell words that start it.
    pub command: String,
    line_argument: Option<String>,
}

/// The editor from the settings, else `$VISUAL`, then `$EDITOR`, then [`FALLBACK`].
pub fn resolve(settings: &EditorSettings) -> Editor {
    let command = settings
        .command
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .map_or_else(
            || choose(std::env::var_os("VISUAL"), std::env::var_os("EDITOR")),
            str::to_owned,
        );
    Editor {
        command,
        line_argument: settings.line_argument.clone(),
    }
}

fn choose(visual: Option<OsString>, editor: Option<OsString>) -> String {
    [visual, editor]
        .into_iter()
        .flatten()
        .map(|value| value.to_string_lossy().trim().to_owned())
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| FALLBACK.to_owned())
}

fn program_name(command: &str) -> &str {
    let program = command.split_whitespace().next().unwrap_or_default();
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

impl Editor {
    /// An editor run as `command`, given lines the way it is known to take them.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            line_argument: None,
        }
    }

    /// The name shown in messages.
    pub fn name(&self) -> &str {
        program_name(&self.command)
    }

    /// Whether this is Neovim, which ratasm can drive through `--listen`.
    pub fn is_neovim(&self) -> bool {
        self.name() == "nvim"
    }

    fn takes_line(&self) -> bool {
        TAKES_LINE.contains(&self.name())
    }

    fn file_arguments(&self, path: &Path, line: usize) -> Vec<String> {
        let line = line.max(1).to_string();
        let file = path.display().to_string();
        match self.line_argument.as_deref() {
            Some(template) if template.contains("{file}") => template
                .split_whitespace()
                .map(|word| word.replace("{file}", &file).replace("{line}", &line))
                .collect(),
            Some(template) => vec![template.replace("{line}", &line), file],
            None if self.takes_line() => vec![format!("+{line}"), file],
            None => vec![file],
        }
    }

    /// The command that runs the editor on `path` at one-based `line`,
    /// listening on `server` when one is given.
    ///
    /// The screen is parsed as an xterm, so `TERM` says so.
    pub fn spec(&self, path: &Path, line: usize, server: Option<&Path>) -> CommandSpec {
        let mut spec = CommandSpec::new("sh")
            .arg("-c")
            .arg(format!("{} \"$@\"", self.command))
            .arg(self.command.clone())
            .env("TERM", "xterm-256color");
        if let Some(server) = server {
            spec = spec.arg("--listen").arg(server.display().to_string());
        }
        spec.args(self.file_arguments(path, line))
    }
}

/// A mark ratasm shows in the editor's sign column.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sign {
    /// The file, as an absolute path.
    pub path: PathBuf,
    /// The one-based line.
    pub line: usize,
    /// What the mark stands for.
    pub kind: SignKind,
}

/// The kinds of mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SignKind {
    /// A breakpoint.
    Breakpoint,
    /// A build error.
    Error,
    /// A build warning.
    Warning,
}

impl SignKind {
    fn name(self) -> &'static str {
        match self {
            Self::Breakpoint => "RatasmBreakpoint",
            Self::Error => "RatasmError",
            Self::Warning => "RatasmWarning",
        }
    }
}

/// A running Neovim reached through its server socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    program: String,
    socket: PathBuf,
}

/// Quotes `text` as a Vim script string literal.
pub fn vim_string(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

impl Remote {
    /// A server for `editor` on a fresh socket, if it is Neovim.
    pub fn for_editor(editor: &Editor) -> Option<Self> {
        if !editor.is_neovim() {
            return None;
        }
        let program = editor.command.split_whitespace().next()?.to_owned();
        let directory = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let socket = directory.join(format!("ratasm-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        Some(Self { program, socket })
    }

    /// The socket Neovim listens on.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Removes the socket file.
    pub fn close(&self) {
        let _ = std::fs::remove_file(&self.socket);
    }

    async fn eval(&self, expression: &str) -> Result<String, String> {
        let spec = CommandSpec::new(&self.program)
            .arg("--server")
            .arg(self.socket.display().to_string())
            .arg("--remote-expr")
            .arg(expression)
            .timeout(REMOTE_TIMEOUT);
        let output = super::run(&spec).await.map_err(|error| error.to_string())?;
        if output.is_success() {
            Ok(output.stdout.trim_end_matches('\n').to_owned())
        } else {
            Err(output.combined().trim().to_owned())
        }
    }

    /// Writes every modified buffer.
    pub async fn save_all(&self) -> Result<(), String> {
        self.eval("execute('silent! wall')").await.map(drop)
    }

    /// The file and one-based line under the cursor.
    pub async fn cursor(&self) -> Result<(PathBuf, usize), String> {
        let reply = self.eval("expand('%:p') .. \"\\n\" .. line('.')").await?;
        let (path, line) = reply
            .rsplit_once('\n')
            .ok_or_else(|| format!("unexpected reply {reply:?}"))?;
        let line = line
            .trim()
            .parse()
            .map_err(|_| format!("unexpected line {line:?}"))?;
        if path.is_empty() {
            return Err("the editor has no file open".to_owned());
        }
        Ok((PathBuf::from(path), line))
    }

    /// Opens `path` at one-based `line`.
    pub async fn open(&self, path: &Path, line: usize) -> Result<(), String> {
        let path = vim_string(&path.display().to_string());
        self.eval(&format!(
            "execute('edit ' .. fnameescape({path})) .. execute('{}') .. execute('normal! zz')",
            line.max(1)
        ))
        .await
        .map(drop)
    }

    /// Replaces ratasm's marks with `signs`.
    pub async fn place_signs(&self, signs: &[Sign]) -> Result<(), String> {
        self.eval(&signs_expression(signs)).await.map(drop)
    }
}

fn signs_expression(signs: &[Sign]) -> String {
    let mut parts = vec![
        "sign_define('RatasmBreakpoint', {'text': '●', 'texthl': 'DiagnosticError'})".to_owned(),
        "sign_define('RatasmError', {'text': '✗', 'texthl': 'DiagnosticError'})".to_owned(),
        "sign_define('RatasmWarning', {'text': '!', 'texthl': 'DiagnosticWarn'})".to_owned(),
        "sign_unplace('ratasm')".to_owned(),
    ];
    for sign in signs {
        let path = vim_string(&sign.path.display().to_string());
        parts.push(format!(
            "bufloaded({path}) ? sign_place(0, 'ratasm', '{}', {path}, {{'lnum': {}}}) : 0",
            sign.kind.name(),
            sign.line
        ));
    }
    format!("len([{}])", parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_wins_over_editor() {
        let chosen = choose(Some("nvim".into()), Some("nano".into()));
        assert_eq!(chosen, "nvim");
    }

    #[test]
    fn a_blank_variable_is_skipped() {
        assert_eq!(choose(Some("  ".into()), Some("nano".into())), "nano");
        assert_eq!(choose(None, Some(String::new().into())), FALLBACK);
        assert_eq!(choose(None, None), FALLBACK);
    }

    #[test]
    fn the_settings_name_the_editor_first() {
        let settings = EditorSettings {
            command: Some("micro".into()),
            ..EditorSettings::default()
        };
        assert_eq!(resolve(&settings).command, "micro");
    }

    #[test]
    fn only_editors_that_take_a_line_are_given_one() {
        assert!(Editor::new("nvim").takes_line());
        assert!(Editor::new("/usr/bin/vim -p").takes_line());
        assert!(Editor::new("emacsclient -t").takes_line());
        assert!(!Editor::new("termi").takes_line());
        assert!(!Editor::new("code --wait").takes_line());
        assert!(!Editor::new("").takes_line());
    }

    fn arguments(editor: &Editor, path: &Path, line: usize) -> Vec<String> {
        editor.spec(path, line, None).args[3..].to_vec()
    }

    #[test]
    fn the_line_and_path_reach_the_editor_as_arguments() {
        let path = Path::new("dir with space/it's; main.asm");
        assert_eq!(
            arguments(&Editor::new("vim"), path, 7),
            ["+7", "dir with space/it's; main.asm"]
        );
    }

    #[test]
    fn line_zero_is_treated_as_the_first_line() {
        assert_eq!(
            arguments(&Editor::new("nano"), Path::new("a.asm"), 0),
            ["+1", "a.asm"]
        );
    }

    #[test]
    fn an_unknown_editor_gets_only_the_file() {
        assert_eq!(
            arguments(&Editor::new("termi"), Path::new("a.asm"), 9),
            ["a.asm"]
        );
    }

    #[test]
    fn a_line_template_is_used_as_written() {
        let mut editor = Editor::new("termi");
        editor.line_argument = Some("{file}:{line}".into());
        assert_eq!(arguments(&editor, Path::new("a.asm"), 9), ["a.asm:9"]);
        editor.line_argument = Some("--line {line}".into());
        assert_eq!(
            arguments(&editor, Path::new("a.asm"), 9),
            ["--line 9", "a.asm"]
        );
    }

    #[test]
    fn neovim_is_told_where_to_listen() {
        let spec = Editor::new("nvim").spec(Path::new("a.asm"), 2, Some(Path::new("/s")));
        assert_eq!(spec.args[3..], ["--listen", "/s", "+2", "a.asm"]);
        assert!(Remote::for_editor(&Editor::new("vim")).is_none());
        assert!(Remote::for_editor(&Editor::new("nvim -u NONE")).is_some());
    }

    #[test]
    fn vim_strings_double_their_quotes() {
        assert_eq!(vim_string("it's"), "'it''s'");
    }

    #[test]
    fn signs_are_placed_only_in_loaded_buffers() {
        let expression = signs_expression(&[Sign {
            path: PathBuf::from("/p/it's.asm"),
            line: 4,
            kind: SignKind::Breakpoint,
        }]);
        assert!(expression.contains("sign_unplace('ratasm')"));
        assert!(expression.contains(
            "bufloaded('/p/it''s.asm') ? sign_place(0, 'ratasm', 'RatasmBreakpoint', '/p/it''s.asm', {'lnum': 4}) : 0"
        ));
    }

    #[tokio::test]
    async fn the_script_hands_its_arguments_to_the_editor_untouched() {
        let spec = Editor::new("printf '%s|'").spec(Path::new("it's; a.asm"), 3, None);
        let output = tokio::process::Command::new(&spec.program)
            .args(&spec.args)
            .output()
            .await
            .expect("sh runs");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "it's; a.asm|");
    }

    #[tokio::test]
    async fn neovim_is_driven_through_its_server() {
        if !crate::process::is_available(Path::new("nvim")) {
            eprintln!("skipping: nvim not installed");
            return;
        }
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("a.asm");
        std::fs::write(&file, "one\ntwo\nthree\n").expect("write");
        let remote = Remote {
            program: "nvim".into(),
            socket: dir.path().join("nvim.sock"),
        };
        let mut child = tokio::process::Command::new("nvim")
            .args(["--headless", "--clean", "--listen"])
            .arg(remote.socket())
            .arg(&file)
            .kill_on_drop(true)
            .spawn()
            .expect("nvim starts");
        for _ in 0..50 {
            if remote.socket().exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        remote.open(&file, 3).await.expect("open");
        assert_eq!(remote.cursor().await.expect("cursor"), (file.clone(), 3));
        remote
            .eval("execute('normal! ggOzero')")
            .await
            .expect("type");
        remote.save_all().await.expect("save");
        assert!(std::fs::read_to_string(&file)
            .expect("read")
            .starts_with("zero\n"));
        remote
            .place_signs(&[Sign {
                path: file.clone(),
                line: 2,
                kind: SignKind::Error,
            }])
            .await
            .expect("signs");
        let placed = remote
            .eval("len(sign_getplaced('%', {'group': 'ratasm'})[0].signs)")
            .await
            .expect("count");
        assert_eq!(placed, "1");
        let _ = child.kill().await;
    }
}

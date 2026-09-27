//! Launching the user's own editor.
//!
//! `$VISUAL` and `$EDITOR` are shell words by convention (`code --wait`,
//! `emacsclient -t`), so the command goes through `sh -c` like git's does,
//! on the pseudo-terminal the Editor panel draws.
//! The file is passed as a positional parameter, never spliced into the
//! script, so its name is still only a name.

use std::ffi::OsString;
use std::path::Path;

use super::CommandSpec;

/// Editors known to accept `+LINE` before the file name.
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

/// The editor used when neither `$VISUAL` nor `$EDITOR` is set.
pub const FALLBACK: &str = "vi";

/// The editor to run: `$VISUAL`, then `$EDITOR`, then [`FALLBACK`].
pub fn from_env() -> String {
    choose(std::env::var_os("VISUAL"), std::env::var_os("EDITOR"))
}

/// Picks the first of `visual` and `editor` that holds something.
fn choose(visual: Option<OsString>, editor: Option<OsString>) -> String {
    [visual, editor]
        .into_iter()
        .flatten()
        .map(|value| value.to_string_lossy().trim().to_owned())
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| FALLBACK.to_owned())
}

/// Whether `editor` understands `+LINE`; any other editor gets the file alone.
fn takes_line(editor: &str) -> bool {
    let program = editor.split_whitespace().next().unwrap_or_default();
    let name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    TAKES_LINE.contains(&name)
}

/// The command that runs `editor` on `path`, at the one-based `line` when it can.
///
/// It runs on the pseudo-terminal behind the Editor panel, whose screen is
/// parsed as an xterm, so `TERM` says so whatever the outer terminal is.
pub fn spec(editor: &str, path: &Path, line: usize) -> CommandSpec {
    let mut spec = CommandSpec::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$@\""))
        .arg(editor)
        .env("TERM", "xterm-256color");
    if takes_line(editor) {
        spec = spec.arg(format!("+{}", line.max(1)));
    }
    spec.arg(path.display().to_string())
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
    fn only_editors_that_take_a_line_are_given_one() {
        assert!(takes_line("nvim"));
        assert!(takes_line("/usr/bin/vim -p"));
        assert!(takes_line("emacsclient -t"));
        assert!(!takes_line("termi"));
        assert!(!takes_line("code --wait"));
        assert!(!takes_line(""));
    }

    /// The arguments `sh` receives after its script and `$0`.
    fn arguments(editor: &str, path: &Path, line: usize) -> Vec<String> {
        spec(editor, path, line).args[3..].to_vec()
    }

    #[test]
    fn the_line_and_path_reach_the_editor_as_arguments() {
        let path = Path::new("dir with space/it's; main.asm");
        assert_eq!(
            arguments("vim", path, 7),
            ["+7", "dir with space/it's; main.asm"]
        );
    }

    #[test]
    fn line_zero_is_treated_as_the_first_line() {
        assert_eq!(arguments("nano", Path::new("a.asm"), 0), ["+1", "a.asm"]);
    }

    #[test]
    fn an_unknown_editor_gets_only_the_file() {
        assert_eq!(arguments("termi", Path::new("a.asm"), 9), ["a.asm"]);
    }

    #[tokio::test]
    async fn the_script_hands_its_arguments_to_the_editor_untouched() {
        let spec = spec("printf '%s|'", Path::new("it's; a.asm"), 3);
        let output = tokio::process::Command::new(&spec.program)
            .args(&spec.args)
            .output()
            .await
            .expect("sh runs");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "it's; a.asm|");
    }
}

//! Key bindings, and validating that they do not conflict.
//!
//! A binding is written the way people say it out loud: `ctrl+s`, `F5`,
//! `shift+tab`. Parsing is deliberately forgiving about case and about
//! `control`/`ctrl`, because a configuration file that rejects `Ctrl+S` for
//! being capitalised is annoying for no benefit.
//!
//! # Conflicts are reported, not resolved
//!
//! Two commands bound to the same chord is a mistake the user wants to know
//! about. Silently letting one shadow the other produces a key that
//! mysteriously does the wrong thing, so [`Keymap::conflicts`] finds them and
//! start-up reports them.
//!
//! The one intentional exception is a binding that *replaces* a default: that
//! is a user overriding, not a conflict, and it is how customisation works.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::panel::Panel;
use crate::command::Command;

/// A key plus its modifiers.
///
/// Not ordered: crossterm's `KeyCode` has no `Ord`, so keymaps are hashed and
/// sorted by their rendered text when an order is needed for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    /// The key itself.
    pub code: KeyCode,
    /// The modifiers held with it.
    pub modifiers: KeyModifiers,
}

impl KeyBinding {
    /// Creates a binding.
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    /// A binding with no modifiers.
    pub const fn plain(code: KeyCode) -> Self {
        Self::new(code, KeyModifiers::NONE)
    }

    /// A binding with control held.
    pub const fn ctrl(code: KeyCode) -> Self {
        Self::new(code, KeyModifiers::CONTROL)
    }

    /// Builds a binding from a terminal key event.
    ///
    /// Runs the same normalisation as [`parse_binding`], so a chord written in
    /// a configuration file matches the event the terminal actually sends.
    pub fn from_event(event: KeyEvent) -> Self {
        let (code, modifiers) = normalise(event.code, event.modifiers);
        Self::new(code, modifiers)
    }
}

/// Puts a key and its modifiers into the one canonical form.
///
/// Terminals encode shift into the character itself — `shift+p` arrives as
/// `P` *and* the shift modifier — so a binding written either way has to end
/// up identical or it can never match. Two rules do that:
///
/// - shift is dropped for character keys, because the character already
///   carries it;
/// - a character combined with control or alt is lowercased, so `ctrl+shift+p`
///   and the `Ctrl+P` event the terminal sends are the same binding.
///
/// A plain character keeps its case, because in the editor `A` and `a` are
/// genuinely different input.
fn normalise(code: KeyCode, modifiers: KeyModifiers) -> (KeyCode, KeyModifiers) {
    let mut modifiers = modifiers;

    // Terminals send shift+tab as BackTab with no modifier, so both spellings
    // canonicalise to that. Without this, a configuration file saying
    // "shift+tab" would never match the key the terminal actually reports.
    if code == KeyCode::BackTab || (code == KeyCode::Tab && modifiers.contains(KeyModifiers::SHIFT))
    {
        modifiers.remove(KeyModifiers::SHIFT);
        return (KeyCode::BackTab, modifiers);
    }

    let KeyCode::Char(ch) = code else {
        return (code, modifiers);
    };

    let shifted = modifiers.contains(KeyModifiers::SHIFT);
    modifiers.remove(KeyModifiers::SHIFT);

    let is_shortcut = modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    let ch = if is_shortcut || shifted && is_shortcut {
        ch.to_ascii_lowercase()
    } else {
        ch
    };

    (KeyCode::Char(ch), modifiers)
}

impl fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            f.write_str("ctrl+")?;
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            f.write_str("alt+")?;
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            f.write_str("shift+")?;
        }
        f.write_str(&code_name(self.code))
    }
}

/// Renders a key code as the name used in configuration files.
fn code_name(code: KeyCode) -> String {
    match code {
        KeyCode::Char(' ') => "space".to_owned(),
        KeyCode::Char(ch) => ch.to_string(),
        KeyCode::F(number) => format!("F{number}"),
        KeyCode::Enter => "enter".to_owned(),
        KeyCode::Tab => "tab".to_owned(),
        KeyCode::BackTab => "shift+tab".to_owned(),
        KeyCode::Backspace => "backspace".to_owned(),
        KeyCode::Delete => "delete".to_owned(),
        KeyCode::Insert => "insert".to_owned(),
        KeyCode::Home => "home".to_owned(),
        KeyCode::End => "end".to_owned(),
        KeyCode::PageUp => "pageup".to_owned(),
        KeyCode::PageDown => "pagedown".to_owned(),
        KeyCode::Up => "up".to_owned(),
        KeyCode::Down => "down".to_owned(),
        KeyCode::Left => "left".to_owned(),
        KeyCode::Right => "right".to_owned(),
        KeyCode::Esc => "esc".to_owned(),
        other => format!("{other:?}").to_lowercase(),
    }
}

/// Why a key binding could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyParseError {
    /// The text was empty.
    #[error("a key binding cannot be empty")]
    Empty,
    /// A modifier was not recognised.
    #[error("'{modifier}' is not a modifier (expected ctrl, alt or shift)")]
    UnknownModifier {
        /// The offending word.
        modifier: String,
    },
    /// The key itself was not recognised.
    #[error("'{key}' is not a key name")]
    UnknownKey {
        /// The offending word.
        key: String,
    },
}

/// Parses a binding such as `ctrl+shift+p`.
///
/// # Errors
///
/// Returns [`KeyParseError`] naming the part that was not understood, so a
/// typo in a configuration file points at itself.
pub fn parse_binding(text: &str) -> Result<KeyBinding, KeyParseError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(KeyParseError::Empty);
    }

    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    // A lone "+" is the plus key, not an empty separator.
    let (key_part, modifier_parts) = match parts.split_last() {
        Some((last, rest)) if !last.is_empty() => (*last, rest),
        _ => ("+", &parts[..parts.len().saturating_sub(2)]),
    };

    let mut modifiers = KeyModifiers::NONE;
    for part in modifier_parts {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "c" => modifiers |= KeyModifiers::CONTROL,
            "alt" | "meta" | "m" => modifiers |= KeyModifiers::ALT,
            "shift" | "s" => modifiers |= KeyModifiers::SHIFT,
            other => {
                return Err(KeyParseError::UnknownModifier {
                    modifier: other.to_owned(),
                })
            }
        }
    }

    let code = parse_code(key_part)?;
    let (code, modifiers) = normalise(code, modifiers);
    Ok(KeyBinding::new(code, modifiers))
}

/// Parses the key part of a binding.
fn parse_code(text: &str) -> Result<KeyCode, KeyParseError> {
    let lowered = text.to_ascii_lowercase();

    if let Some(number) = lowered.strip_prefix('f') {
        if let Ok(number) = number.parse::<u8>() {
            if (1..=12).contains(&number) {
                return Ok(KeyCode::F(number));
            }
        }
    }

    Ok(match lowered.as_str() {
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "backspace" | "bs" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "esc" | "escape" => KeyCode::Esc,
        "space" => KeyCode::Char(' '),
        _ => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                // The character as written; normalise decides whether case
                // is significant.
                (Some(ch), None) => KeyCode::Char(ch),
                _ => {
                    return Err(KeyParseError::UnknownKey {
                        key: text.to_owned(),
                    })
                }
            }
        }
    })
}

/// A binding bound to two different commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The chord that is bound twice.
    pub binding: KeyBinding,
    /// The commands competing for it.
    pub commands: Vec<Command>,
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self.commands.iter().map(Command::id).collect();
        write!(f, "{} is bound to {}", self.binding, names.join(" and "))
    }
}

/// The mapping from key chords to commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: HashMap<KeyBinding, Command>,
}

impl Keymap {
    /// The built-in bindings.
    pub fn defaults() -> Self {
        let mut bindings = HashMap::new();
        let mut bind = |binding: KeyBinding, command: Command| {
            bindings.insert(binding, command);
        };

        // Running and debugging, on the function keys.
        bind(KeyBinding::plain(KeyCode::F(5)), Command::Run);
        bind(KeyBinding::plain(KeyCode::F(6)), Command::Build);
        bind(KeyBinding::plain(KeyCode::F(7)), Command::StepInstruction);
        bind(KeyBinding::plain(KeyCode::F(8)), Command::StepOver);
        bind(KeyBinding::plain(KeyCode::F(9)), Command::ToggleBreakpoint);
        bind(KeyBinding::plain(KeyCode::F(10)), Command::StepLine);
        bind(KeyBinding::plain(KeyCode::F(11)), Command::StepOut);
        // Shift with a step key reverses it, which is the mapping people
        // already expect from step-into and step-over.
        bind(
            KeyBinding::new(KeyCode::F(7), KeyModifiers::SHIFT),
            Command::StepBack,
        );
        bind(
            KeyBinding::new(KeyCode::F(8), KeyModifiers::SHIFT),
            Command::StepBackOver,
        );
        bind(
            KeyBinding::new(KeyCode::F(5), KeyModifiers::SHIFT),
            Command::ReverseContinue,
        );
        bind(KeyBinding::plain(KeyCode::F(12)), Command::DebugStart);

        // Files.
        bind(KeyBinding::ctrl(KeyCode::Char('s')), Command::SaveFile);
        bind(KeyBinding::ctrl(KeyCode::Char('o')), Command::OpenFile);
        bind(KeyBinding::ctrl(KeyCode::Char('n')), Command::NewFile);
        bind(KeyBinding::ctrl(KeyCode::Char('w')), Command::CloseFile);
        bind(KeyBinding::ctrl(KeyCode::Char('q')), Command::Quit);

        // Editing.
        bind(KeyBinding::ctrl(KeyCode::Char('z')), Command::Undo);
        bind(KeyBinding::ctrl(KeyCode::Char('y')), Command::Redo);
        bind(KeyBinding::ctrl(KeyCode::Char('a')), Command::SelectAll);
        bind(KeyBinding::ctrl(KeyCode::Char('c')), Command::Copy);
        bind(KeyBinding::ctrl(KeyCode::Char('x')), Command::Cut);
        bind(KeyBinding::ctrl(KeyCode::Char('v')), Command::Paste);

        // Navigation and search.
        bind(KeyBinding::ctrl(KeyCode::Char('p')), Command::OpenPalette);
        bind(KeyBinding::ctrl(KeyCode::Char('f')), Command::Search);
        bind(KeyBinding::ctrl(KeyCode::Char('g')), Command::GoToLine);
        bind(
            KeyBinding::ctrl(KeyCode::Char('k')),
            Command::OpenSyscallFinder,
        );
        bind(
            KeyBinding::ctrl(KeyCode::Char('d')),
            Command::GoToDefinition,
        );
        // Learning and the scratchpad. F1 is where a reader looks for help,
        // and the material is what ratasm has instead of a help screen.
        bind(
            KeyBinding::plain(KeyCode::F(1)),
            Command::ToggleLearningMode,
        );
        bind(KeyBinding::plain(KeyCode::F(2)), Command::OpenScratchpad);
        bind(KeyBinding::plain(KeyCode::Tab), Command::NextPanel);
        bind(KeyBinding::plain(KeyCode::BackTab), Command::PreviousPanel);
        bind(KeyBinding::ctrl(KeyCode::PageDown), Command::NextDocument);
        bind(KeyBinding::ctrl(KeyCode::PageUp), Command::PreviousDocument);

        // Alt plus a digit focuses a panel directly.
        for (index, panel) in Panel::ALL.iter().enumerate().take(9) {
            let digit = char::from_digit(index as u32 + 1, 10).unwrap_or('1');
            bindings.insert(
                KeyBinding::new(KeyCode::Char(digit), KeyModifiers::ALT),
                Command::FocusPanel(*panel),
            );
        }

        Self { bindings }
    }

    /// An empty keymap.
    pub fn empty() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }

    /// The command bound to a chord, if any.
    pub fn command_for(&self, binding: KeyBinding) -> Option<&Command> {
        self.bindings.get(&binding)
    }

    /// The command bound to a terminal key event, if any.
    pub fn command_for_event(&self, event: KeyEvent) -> Option<&Command> {
        self.command_for(KeyBinding::from_event(event))
    }

    /// The chord bound to a command, if any.
    ///
    /// Used to show the shortcut beside a command in the palette.
    pub fn binding_for(&self, command: &Command) -> Option<KeyBinding> {
        self.sorted_bindings()
            .into_iter()
            .find(|(_, bound)| bound == command)
            .map(|(binding, _)| binding)
    }

    /// Binds a chord, replacing whatever was there.
    pub fn bind(&mut self, binding: KeyBinding, command: Command) {
        self.bindings.insert(binding, command);
    }

    /// Removes a binding.
    pub fn unbind(&mut self, binding: KeyBinding) -> Option<Command> {
        self.bindings.remove(&binding)
    }

    /// Every binding, in no particular order.
    pub fn bindings(&self) -> impl Iterator<Item = (&KeyBinding, &Command)> {
        self.bindings.iter()
    }

    /// Every binding, sorted by its rendered chord.
    ///
    /// Hash order is not stable between runs, so anything the user sees — the
    /// shortcut list, the palette — goes through this instead.
    pub fn sorted_bindings(&self) -> Vec<(KeyBinding, Command)> {
        let mut all: Vec<(KeyBinding, Command)> = self
            .bindings
            .iter()
            .map(|(binding, command)| (*binding, command.clone()))
            .collect();
        all.sort_by_key(|(binding, _)| binding.to_string());
        all
    }

    /// The number of bindings.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Whether nothing is bound.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Applies user overrides on top of these bindings.
    ///
    /// Returns the parse errors encountered. A bad entry is skipped rather
    /// than aborting the whole file, so one typo does not cost the user every
    /// other binding they configured — but it is reported.
    pub fn apply_overrides(&mut self, overrides: &BTreeMap<String, String>) -> Vec<KeymapError> {
        let mut errors = Vec::new();

        for (key, command_id) in overrides {
            let binding = match parse_binding(key) {
                Ok(binding) => binding,
                Err(error) => {
                    errors.push(KeymapError::BadKey {
                        key: key.clone(),
                        source: error,
                    });
                    continue;
                }
            };

            // An empty command name unbinds the chord.
            if command_id.trim().is_empty() {
                self.unbind(binding);
                continue;
            }

            match Command::from_id(command_id) {
                Some(command) => self.bind(binding, command),
                None => errors.push(KeymapError::UnknownCommand {
                    key: key.clone(),
                    command: command_id.clone(),
                }),
            }
        }

        errors
    }

    /// Finds commands bound to more than one chord, and chords bound twice.
    ///
    /// A chord can only map to one command by construction, so what this
    /// actually detects is the reverse: a command reachable from several
    /// chords. That is usually deliberate, so only genuine duplicates — the
    /// same command bound twice with no other command displaced — are
    /// reported.
    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut by_command: BTreeMap<String, Vec<KeyBinding>> = BTreeMap::new();
        for (binding, command) in &self.bindings {
            by_command.entry(command.id()).or_default().push(*binding);
        }

        by_command
            .into_iter()
            .filter(|(_, bindings)| bindings.len() > 1)
            .filter_map(|(id, bindings)| {
                Command::from_id(&id).map(|command| Conflict {
                    binding: bindings[0],
                    commands: vec![command],
                })
            })
            .collect()
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::defaults()
    }
}

/// A problem found while applying user key bindings.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    /// The key could not be parsed.
    #[error("key binding '{key}': {source}")]
    BadKey {
        /// The key as written.
        key: String,
        /// Why it could not be parsed.
        #[source]
        source: KeyParseError,
    },
    /// The command name is not recognised.
    #[error("key binding '{key}': there is no command called '{command}'")]
    UnknownCommand {
        /// The key as written.
        key: String,
        /// The command name as written.
        command: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_keys_parse() {
        assert_eq!(parse_binding("F5"), Ok(KeyBinding::plain(KeyCode::F(5))));
        assert_eq!(parse_binding("tab"), Ok(KeyBinding::plain(KeyCode::Tab)));
        assert_eq!(
            parse_binding("a"),
            Ok(KeyBinding::plain(KeyCode::Char('a')))
        );
    }

    #[test]
    fn modifiers_parse_in_any_case_and_spelling() {
        let expected = KeyBinding::ctrl(KeyCode::Char('s'));
        for text in ["ctrl+s", "Ctrl+S", "CONTROL+s", "  ctrl + s  ", "c+s"] {
            assert_eq!(parse_binding(text), Ok(expected), "failed for {text:?}");
        }
    }

    #[test]
    fn several_modifiers_combine() {
        let binding = parse_binding("ctrl+alt+delete").expect("parses");
        assert_eq!(binding.code, KeyCode::Delete);
        assert!(binding.modifiers.contains(KeyModifiers::CONTROL));
        assert!(binding.modifiers.contains(KeyModifiers::ALT));
    }

    #[test]
    fn function_keys_parse_within_range() {
        assert_eq!(parse_binding("F12"), Ok(KeyBinding::plain(KeyCode::F(12))));
        // F13 is not a key we bind; it falls through to the unknown-key error.
        assert!(parse_binding("F13").is_err());
        assert!(parse_binding("F0").is_err());
    }

    #[test]
    fn shift_is_dropped_for_characters() {
        // The terminal reports shift+a as 'A', so keeping the modifier would
        // make the binding unmatchable.
        let binding = parse_binding("ctrl+shift+p").expect("parses");
        assert!(!binding.modifiers.contains(KeyModifiers::SHIFT));
        assert!(binding.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn shift_tab_is_normalised_to_backtab() {
        // Terminals report shift+tab as BackTab with no modifier, so both
        // spellings must land on that or the binding could never fire.
        let expected = KeyBinding::plain(KeyCode::BackTab);
        assert_eq!(parse_binding("shift+tab"), Ok(expected));
        assert_eq!(parse_binding("backtab"), Ok(expected));
        assert_eq!(parse_binding("Shift+Tab"), Ok(expected));

        let event = KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE);
        assert_eq!(KeyBinding::from_event(event), expected);
    }

    #[test]
    fn bad_bindings_report_which_part_was_wrong() {
        assert_eq!(parse_binding(""), Err(KeyParseError::Empty));
        assert!(matches!(
            parse_binding("hyper+s"),
            Err(KeyParseError::UnknownModifier { .. })
        ));
        assert!(matches!(
            parse_binding("ctrl+notakey"),
            Err(KeyParseError::UnknownKey { .. })
        ));
    }

    #[test]
    fn malformed_bindings_never_panic() {
        for text in ["+", "++", "ctrl+", "+s", "ctrl++", "   ", "\u{1F600}"] {
            let _ = parse_binding(text);
        }
    }

    #[test]
    fn bindings_render_back_to_readable_text() {
        assert_eq!(KeyBinding::plain(KeyCode::F(5)).to_string(), "F5");
        assert_eq!(KeyBinding::ctrl(KeyCode::Char('s')).to_string(), "ctrl+s");
        assert_eq!(KeyBinding::plain(KeyCode::BackTab).to_string(), "shift+tab");
    }

    #[test]
    fn rendered_bindings_parse_back() {
        for (binding, _) in Keymap::defaults().bindings() {
            let text = binding.to_string();
            assert_eq!(
                parse_binding(&text).as_ref(),
                Ok(binding),
                "{text} did not round trip"
            );
        }
    }

    #[test]
    fn the_documented_defaults_are_bound() {
        // These are the shortcuts the README promises.
        let keymap = Keymap::defaults();
        for (text, expected) in [
            ("F5", Command::Run),
            ("F6", Command::Build),
            ("F7", Command::StepInstruction),
            ("F8", Command::StepOver),
            ("F9", Command::ToggleBreakpoint),
            ("F10", Command::StepLine),
            ("F1", Command::ToggleLearningMode),
            ("F2", Command::OpenScratchpad),
            ("shift+F5", Command::ReverseContinue),
            ("shift+F7", Command::StepBack),
            ("shift+F8", Command::StepBackOver),
            ("ctrl+s", Command::SaveFile),
            ("ctrl+o", Command::OpenFile),
            ("ctrl+p", Command::OpenPalette),
            ("ctrl+f", Command::Search),
            ("ctrl+g", Command::GoToLine),
            ("ctrl+k", Command::OpenSyscallFinder),
            ("ctrl+q", Command::Quit),
            ("tab", Command::NextPanel),
            ("shift+tab", Command::PreviousPanel),
        ] {
            let binding = parse_binding(text).expect("parses");
            assert_eq!(
                keymap.command_for(binding),
                Some(&expected),
                "{text} is not bound to {expected}"
            );
        }
    }

    #[test]
    fn no_default_binding_is_used_twice() {
        // A chord bound twice would silently do the wrong thing.
        assert!(
            Keymap::defaults().conflicts().is_empty(),
            "the defaults conflict: {:?}",
            Keymap::defaults().conflicts()
        );
    }

    #[test]
    fn a_key_event_finds_its_command() {
        let keymap = Keymap::defaults();
        let event = KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE);
        assert_eq!(keymap.command_for_event(event), Some(&Command::Run));

        let event = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert_eq!(keymap.command_for_event(event), Some(&Command::SaveFile));
    }

    #[test]
    fn an_unbound_key_finds_nothing() {
        let keymap = Keymap::defaults();
        let event = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(keymap.command_for_event(event), None);
    }

    #[test]
    fn a_shifted_character_event_matches_the_binding_written_for_it() {
        // The terminal sends 'P' with SHIFT for what a user writes as
        // ctrl+shift+p. Both must normalise to the same binding or the chord
        // could never fire.
        let mut keymap = Keymap::empty();
        keymap.bind(
            parse_binding("ctrl+shift+p").expect("parses"),
            Command::OpenPalette,
        );

        let event = KeyEvent::new(
            KeyCode::Char('P'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert_eq!(keymap.command_for_event(event), Some(&Command::OpenPalette));
    }

    #[test]
    fn control_shortcuts_ignore_the_case_of_the_character() {
        let keymap = Keymap::defaults();
        for ch in ['s', 'S'] {
            let event = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL);
            assert_eq!(
                keymap.command_for_event(event),
                Some(&Command::SaveFile),
                "ctrl+{ch} should save"
            );
        }
    }

    #[test]
    fn a_plain_character_keeps_its_case() {
        // In the editor, A and a are different input; only shortcuts fold.
        let upper = parse_binding("A").expect("parses");
        let lower = parse_binding("a").expect("parses");
        assert_ne!(upper, lower);
    }

    #[test]
    fn a_command_reports_the_chord_that_runs_it() {
        let keymap = Keymap::defaults();
        assert_eq!(
            keymap
                .binding_for(&Command::SaveFile)
                .map(|b| b.to_string()),
            Some("ctrl+s".to_owned())
        );
        assert_eq!(keymap.binding_for(&Command::ClearBreakpoints), None);
    }

    #[test]
    fn user_overrides_replace_defaults() {
        let mut keymap = Keymap::defaults();
        let overrides = BTreeMap::from([("ctrl+b".to_owned(), "build.build".to_owned())]);

        let errors = keymap.apply_overrides(&overrides);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            keymap.command_for(parse_binding("ctrl+b").expect("parses")),
            Some(&Command::Build)
        );
    }

    #[test]
    fn an_empty_command_name_unbinds_a_chord() {
        let mut keymap = Keymap::defaults();
        let overrides = BTreeMap::from([("ctrl+q".to_owned(), String::new())]);

        keymap.apply_overrides(&overrides);
        assert_eq!(
            keymap.command_for(parse_binding("ctrl+q").expect("parses")),
            None
        );
    }

    #[test]
    fn a_bad_override_is_reported_without_discarding_the_good_ones() {
        // One typo must not cost the user every other binding.
        let mut keymap = Keymap::empty();
        let overrides = BTreeMap::from([
            ("ctrl+b".to_owned(), "build.build".to_owned()),
            ("nonsense+z".to_owned(), "build.run".to_owned()),
            ("ctrl+j".to_owned(), "no.such.command".to_owned()),
        ]);

        let errors = keymap.apply_overrides(&overrides);
        assert_eq!(errors.len(), 2);
        assert!(errors
            .iter()
            .any(|e| matches!(e, KeymapError::BadKey { .. })));
        assert!(errors
            .iter()
            .any(|e| matches!(e, KeymapError::UnknownCommand { .. })));

        assert_eq!(
            keymap.command_for(parse_binding("ctrl+b").expect("parses")),
            Some(&Command::Build),
            "the valid binding survived"
        );
    }

    #[test]
    fn error_messages_name_the_offending_key() {
        let mut keymap = Keymap::empty();
        let overrides = BTreeMap::from([("ctrl+j".to_owned(), "no.such.command".to_owned())]);
        let errors = keymap.apply_overrides(&overrides);
        let text = errors[0].to_string();
        assert!(text.contains("ctrl+j"), "{text}");
        assert!(text.contains("no.such.command"), "{text}");
    }

    #[test]
    fn binding_one_command_to_two_chords_is_reported() {
        let mut keymap = Keymap::empty();
        keymap.bind(KeyBinding::plain(KeyCode::F(5)), Command::Run);
        keymap.bind(KeyBinding::ctrl(KeyCode::Char('r')), Command::Run);

        let conflicts = keymap.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].to_string().contains("build.run"));
    }

    #[test]
    fn every_default_binding_names_a_real_command() {
        for (_, command) in Keymap::defaults().bindings() {
            assert!(
                Command::from_id(&command.id()).is_some(),
                "{} is bound but does not exist",
                command.id()
            );
        }
    }
}

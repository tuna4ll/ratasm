//! Theming: colour palettes, glyph sets and the styles widgets consume.
//!
//! A [`Theme`] pairs a [`Palette`] with a [`SymbolSet`]. Widgets ask the theme
//! for ready-made [`Style`] values instead of assembling colours themselves,
//! which keeps two accessibility guarantees enforceable in one place:
//!
//! - state is never signalled by colour alone, because the style helpers add a
//!   modifier (bold, reversed, underlined) alongside the colour, and callers
//!   pair them with a glyph from the symbol set;
//! - terminals without true colour or without Unicode remain fully usable by
//!   selecting a different palette and symbol set.

pub mod palette;
pub mod symbols;

use std::fmt;
use std::str::FromStr;

use ratatui::style::{Modifier, Style};

pub use palette::Palette;
pub use symbols::{detect_unicode_support, SymbolSet};

/// The built-in themes a user can select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeKind {
    /// Dark palette for true-colour terminals.
    #[default]
    Dark,
    /// Light palette for true-colour terminals.
    Light,
    /// Sixteen-colour palette for limited terminals.
    Ansi16,
    /// Palette that avoids red/green contrasts.
    Colorblind,
}

impl ThemeKind {
    /// All selectable themes, in presentation order.
    pub const ALL: [ThemeKind; 4] = [
        ThemeKind::Dark,
        ThemeKind::Light,
        ThemeKind::Ansi16,
        ThemeKind::Colorblind,
    ];

    /// The stable identifier used in configuration files.
    pub const fn id(self) -> &'static str {
        match self {
            ThemeKind::Dark => "dark",
            ThemeKind::Light => "light",
            ThemeKind::Ansi16 => "ansi16",
            ThemeKind::Colorblind => "colorblind",
        }
    }

    /// A short human-readable description.
    pub const fn description(self) -> &'static str {
        match self {
            ThemeKind::Dark => "Dark, true colour",
            ThemeKind::Light => "Light, true colour",
            ThemeKind::Ansi16 => "Sixteen ANSI colours only",
            ThemeKind::Colorblind => "Blue/orange, avoids red-green pairs",
        }
    }

    /// The palette backing this theme.
    pub const fn palette(self) -> Palette {
        match self {
            ThemeKind::Dark => Palette::dark(),
            ThemeKind::Light => Palette::light(),
            ThemeKind::Ansi16 => Palette::ansi16(),
            ThemeKind::Colorblind => Palette::colorblind(),
        }
    }

    /// Returns the next theme, wrapping around, for cycle-through shortcuts.
    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

impl fmt::Display for ThemeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// Error returned when a theme name is not recognised.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown theme '{name}' (expected one of: dark, light, ansi16, colorblind)")]
pub struct UnknownTheme {
    /// The unrecognised name as supplied by the user.
    pub name: String,
}

impl FromStr for ThemeKind {
    type Err = UnknownTheme;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalised = s.trim().to_ascii_lowercase().replace(['_', ' '], "-");
        match normalised.as_str() {
            "dark" => Ok(ThemeKind::Dark),
            "light" => Ok(ThemeKind::Light),
            "ansi16" | "ansi-16" | "16-color" | "16-colour" => Ok(ThemeKind::Ansi16),
            "colorblind" | "colourblind" | "color-blind" => Ok(ThemeKind::Colorblind),
            _ => Err(UnknownTheme { name: s.to_owned() }),
        }
    }
}

/// A palette and glyph set, plus the styles derived from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    kind: ThemeKind,
    palette: Palette,
    symbols: SymbolSet,
}

impl Theme {
    /// Builds a theme from a kind, choosing glyphs based on Unicode support.
    pub const fn new(kind: ThemeKind, unicode: bool) -> Self {
        Self {
            kind,
            palette: kind.palette(),
            symbols: SymbolSet::for_unicode(unicode),
        }
    }

    /// Builds a theme, detecting Unicode support from the environment.
    pub fn detect(kind: ThemeKind) -> Self {
        Self::new(kind, detect_unicode_support())
    }

    /// The kind this theme was built from.
    pub const fn kind(&self) -> ThemeKind {
        self.kind
    }

    /// The underlying colour palette.
    pub const fn palette(&self) -> &Palette {
        &self.palette
    }

    /// The glyph set matching the terminal's Unicode support.
    pub const fn symbols(&self) -> &SymbolSet {
        &self.symbols
    }

    /// Replaces the palette while keeping the glyph set.
    pub fn set_kind(&mut self, kind: ThemeKind) {
        self.kind = kind;
        self.palette = kind.palette();
    }

    /// Base style for panel content.
    pub fn base(&self) -> Style {
        Style::default()
            .fg(self.palette.foreground)
            .bg(self.palette.background)
    }

    /// Style for de-emphasised text such as line numbers.
    pub fn dim(&self) -> Style {
        Style::default().fg(self.palette.dim)
    }

    /// Style for emphasised text.
    pub fn bright(&self) -> Style {
        Style::default()
            .fg(self.palette.bright)
            .add_modifier(Modifier::BOLD)
    }

    /// Border style for a panel, varying with focus.
    ///
    /// The focused border differs in both colour and weight so that focus is
    /// perceivable without colour.
    pub fn border(&self, focused: bool) -> Style {
        if focused {
            Style::default()
                .fg(self.palette.border_focused)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.palette.border)
        }
    }

    /// Title style for a panel, varying with focus.
    pub fn title(&self, focused: bool) -> Style {
        if focused {
            Style::default()
                .fg(self.palette.bright)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.palette.dim)
        }
    }

    /// Style for the selected row of a list.
    pub fn selection(&self) -> Style {
        Style::default()
            .bg(self.palette.selection)
            .fg(self.palette.bright)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for the line containing the editor cursor.
    pub fn cursor_line(&self) -> Style {
        Style::default().bg(self.palette.cursor_line)
    }

    /// Style for a text selection range.
    pub fn text_selection(&self) -> Style {
        Style::default().bg(self.palette.selection)
    }

    /// Style for error text.
    pub fn error(&self) -> Style {
        Style::default()
            .fg(self.palette.error)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for warning text.
    pub fn warning(&self) -> Style {
        Style::default().fg(self.palette.warning)
    }

    /// Style for success text.
    pub fn success(&self) -> Style {
        Style::default().fg(self.palette.success)
    }

    /// Style for informational text.
    pub fn info(&self) -> Style {
        Style::default().fg(self.palette.info)
    }

    /// Style for the accent colour.
    pub fn accent(&self) -> Style {
        Style::default().fg(self.palette.accent)
    }

    /// Style for a value that changed since the previous debugger stop.
    ///
    /// Bold is applied in addition to the colour so the change is visible on
    /// monochrome terminals.
    pub fn changed(&self) -> Style {
        Style::default()
            .fg(self.palette.changed)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for a breakpoint marker.
    pub fn breakpoint(&self) -> Style {
        Style::default()
            .fg(self.palette.breakpoint)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for the line the program counter points at.
    pub fn current_line(&self) -> Style {
        Style::default()
            .fg(self.palette.current)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for memory and instruction addresses.
    pub fn address(&self) -> Style {
        Style::default().fg(self.palette.address)
    }

    /// Style for raw instruction bytes.
    pub fn bytes(&self) -> Style {
        Style::default().fg(self.palette.bytes)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(ThemeKind::default(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_names_round_trip() {
        for kind in ThemeKind::ALL {
            let parsed: ThemeKind = kind.id().parse().expect("id must parse");
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn theme_parsing_is_forgiving_about_case_and_separators() {
        assert_eq!("Dark".parse::<ThemeKind>(), Ok(ThemeKind::Dark));
        assert_eq!("  LIGHT ".parse::<ThemeKind>(), Ok(ThemeKind::Light));
        assert_eq!("ansi_16".parse::<ThemeKind>(), Ok(ThemeKind::Ansi16));
        assert_eq!(
            "colourblind".parse::<ThemeKind>(),
            Ok(ThemeKind::Colorblind)
        );
    }

    #[test]
    fn unknown_theme_reports_the_original_name() {
        let err = "solarized".parse::<ThemeKind>().unwrap_err();
        assert_eq!(err.name, "solarized");
        assert!(err.to_string().contains("solarized"));
    }

    #[test]
    fn cycling_themes_visits_every_theme_and_returns() {
        let mut kind = ThemeKind::Dark;
        let mut seen = Vec::new();
        for _ in 0..ThemeKind::ALL.len() {
            seen.push(kind);
            kind = kind.next();
        }
        assert_eq!(kind, ThemeKind::Dark, "cycle must wrap around");
        assert_eq!(seen.len(), ThemeKind::ALL.len());
        for theme in ThemeKind::ALL {
            assert!(seen.contains(&theme));
        }
    }

    #[test]
    fn focused_panels_differ_from_unfocused_by_more_than_colour() {
        let theme = Theme::default();
        let focused = theme.border(true);
        let idle = theme.border(false);
        assert_ne!(focused.fg, idle.fg);
        assert!(focused.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn changed_values_carry_a_modifier_for_monochrome_terminals() {
        let theme = Theme::default();
        assert!(theme.changed().add_modifier.contains(Modifier::BOLD));
        assert!(theme.error().add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn ascii_theme_uses_ascii_glyphs() {
        let theme = Theme::new(ThemeKind::Ansi16, false);
        assert!(theme.symbols().breakpoint.is_ascii());
    }

    #[test]
    fn setting_kind_swaps_the_palette_but_keeps_glyphs() {
        let mut theme = Theme::new(ThemeKind::Dark, false);
        let glyphs = *theme.symbols();
        theme.set_kind(ThemeKind::Light);
        assert_eq!(theme.kind(), ThemeKind::Light);
        assert_eq!(*theme.palette(), Palette::light());
        assert_eq!(*theme.symbols(), glyphs);
    }
}

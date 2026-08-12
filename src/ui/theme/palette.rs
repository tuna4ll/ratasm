//! Semantic colour slots and the built-in palettes that fill them.
//!
//! Widgets never name a colour directly; they ask the palette for a *role*
//! such as [`Palette::error`] or [`Palette::syntax_register`]. Swapping a
//! theme therefore cannot leave a widget with a hardcoded colour, and adding a
//! terminal-capability tier only means adding one more palette here.

use ratatui::style::Color;

/// The complete set of colour roles a theme must define.
///
/// Every field is a semantic role rather than a colour name, so a palette
/// built for a 16-colour terminal and one built for true colour remain
/// interchangeable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Default window background.
    pub background: Color,
    /// Default foreground for body text.
    pub foreground: Color,
    /// De-emphasised text such as line numbers and hints.
    pub dim: Color,
    /// Emphasised text such as active headings.
    pub bright: Color,
    /// Border of an unfocused panel.
    pub border: Color,
    /// Border of the focused panel.
    pub border_focused: Color,
    /// Background of a selected list row or text selection.
    pub selection: Color,
    /// Background of the line holding the editor cursor.
    pub cursor_line: Color,
    /// Primary accent used sparingly for emphasis.
    pub accent: Color,
    /// Errors and failed operations.
    pub error: Color,
    /// Warnings and recoverable problems.
    pub warning: Color,
    /// Successful operations.
    pub success: Color,
    /// Informational notices.
    pub info: Color,
    /// A value that changed since the previous debugger stop.
    pub changed: Color,
    /// An enabled breakpoint marker.
    pub breakpoint: Color,
    /// The line or instruction the program counter points at.
    pub current: Color,
    /// Memory and instruction addresses.
    pub address: Color,
    /// Raw instruction bytes in the disassembly view.
    pub bytes: Color,
    /// Assembly comments.
    pub syntax_comment: Color,
    /// String and character literals.
    pub syntax_string: Color,
    /// Numeric literals.
    pub syntax_number: Color,
    /// Label definitions and references.
    pub syntax_label: Color,
    /// Assembler directives such as `section` or `global`.
    pub syntax_directive: Color,
    /// Instruction mnemonics.
    pub syntax_instruction: Color,
    /// Register names.
    pub syntax_register: Color,
    /// Size specifiers and other reserved words.
    pub syntax_keyword: Color,
    /// Macro names and preprocessor directives.
    pub syntax_macro: Color,
    /// Operators, commas and brackets.
    pub syntax_punctuation: Color,
}

impl Palette {
    /// The default dark palette, tuned for true-colour terminals.
    pub const fn dark() -> Self {
        Self {
            background: Color::Rgb(0x16, 0x18, 0x1d),
            foreground: Color::Rgb(0xc9, 0xcc, 0xd4),
            dim: Color::Rgb(0x63, 0x6b, 0x7a),
            bright: Color::Rgb(0xe8, 0xea, 0xf0),
            border: Color::Rgb(0x33, 0x38, 0x42),
            border_focused: Color::Rgb(0x6d, 0x9d, 0xd8),
            selection: Color::Rgb(0x2c, 0x3a, 0x4d),
            cursor_line: Color::Rgb(0x1e, 0x22, 0x2a),
            accent: Color::Rgb(0x6d, 0x9d, 0xd8),
            error: Color::Rgb(0xd8, 0x6b, 0x6b),
            warning: Color::Rgb(0xd8, 0xaa, 0x5f),
            success: Color::Rgb(0x77, 0xb5, 0x7f),
            info: Color::Rgb(0x6d, 0x9d, 0xd8),
            changed: Color::Rgb(0xd8, 0xaa, 0x5f),
            breakpoint: Color::Rgb(0xd8, 0x6b, 0x6b),
            current: Color::Rgb(0x77, 0xb5, 0x7f),
            address: Color::Rgb(0x7d, 0x8a, 0x9c),
            bytes: Color::Rgb(0x8a, 0x7f, 0xa8),
            syntax_comment: Color::Rgb(0x5f, 0x69, 0x78),
            syntax_string: Color::Rgb(0x9d, 0xb8, 0x7f),
            syntax_number: Color::Rgb(0xc9, 0x9d, 0x6b),
            syntax_label: Color::Rgb(0xe0, 0xc0, 0x7a),
            syntax_directive: Color::Rgb(0xb8, 0x8a, 0xc8),
            syntax_instruction: Color::Rgb(0x7f, 0xb0, 0xd8),
            syntax_register: Color::Rgb(0x6f, 0xc0, 0xb8),
            syntax_keyword: Color::Rgb(0xb8, 0x8a, 0xc8),
            syntax_macro: Color::Rgb(0xc8, 0x8a, 0x9d),
            syntax_punctuation: Color::Rgb(0x8a, 0x92, 0xa0),
        }
    }

    /// The default light palette, tuned for true-colour terminals.
    pub const fn light() -> Self {
        Self {
            background: Color::Rgb(0xfa, 0xfa, 0xf7),
            foreground: Color::Rgb(0x2b, 0x2f, 0x38),
            dim: Color::Rgb(0x7c, 0x83, 0x8f),
            bright: Color::Rgb(0x11, 0x14, 0x1a),
            border: Color::Rgb(0xd0, 0xd4, 0xdb),
            border_focused: Color::Rgb(0x1f, 0x63, 0xa8),
            selection: Color::Rgb(0xd6, 0xe4, 0xf5),
            cursor_line: Color::Rgb(0xef, 0xf1, 0xf4),
            accent: Color::Rgb(0x1f, 0x63, 0xa8),
            error: Color::Rgb(0xa8, 0x27, 0x27),
            warning: Color::Rgb(0x8a, 0x5d, 0x00),
            success: Color::Rgb(0x1f, 0x6b, 0x35),
            info: Color::Rgb(0x1f, 0x63, 0xa8),
            changed: Color::Rgb(0x8a, 0x5d, 0x00),
            breakpoint: Color::Rgb(0xa8, 0x27, 0x27),
            current: Color::Rgb(0x1f, 0x6b, 0x35),
            address: Color::Rgb(0x5c, 0x66, 0x75),
            bytes: Color::Rgb(0x5f, 0x4b, 0x8a),
            syntax_comment: Color::Rgb(0x77, 0x7f, 0x8c),
            syntax_string: Color::Rgb(0x2f, 0x6b, 0x33),
            syntax_number: Color::Rgb(0x9a, 0x53, 0x1f),
            syntax_label: Color::Rgb(0x8a, 0x60, 0x00),
            syntax_directive: Color::Rgb(0x6b, 0x37, 0x99),
            syntax_instruction: Color::Rgb(0x1a, 0x54, 0x8f),
            syntax_register: Color::Rgb(0x0f, 0x6b, 0x66),
            syntax_keyword: Color::Rgb(0x6b, 0x37, 0x99),
            syntax_macro: Color::Rgb(0x9a, 0x33, 0x5c),
            syntax_punctuation: Color::Rgb(0x55, 0x5c, 0x68),
        }
    }

    /// A palette restricted to the sixteen ANSI colours.
    ///
    /// Uses only named colours so it renders correctly on terminals such as
    /// the Linux console, where RGB sequences are ignored or approximated
    /// badly.
    pub const fn ansi16() -> Self {
        Self {
            background: Color::Reset,
            foreground: Color::Gray,
            dim: Color::DarkGray,
            bright: Color::White,
            border: Color::DarkGray,
            border_focused: Color::Cyan,
            selection: Color::Blue,
            cursor_line: Color::Black,
            accent: Color::Cyan,
            error: Color::Red,
            warning: Color::Yellow,
            success: Color::Green,
            info: Color::Cyan,
            changed: Color::Yellow,
            breakpoint: Color::Red,
            current: Color::Green,
            address: Color::DarkGray,
            bytes: Color::Magenta,
            syntax_comment: Color::DarkGray,
            syntax_string: Color::Green,
            syntax_number: Color::Yellow,
            syntax_label: Color::LightYellow,
            syntax_directive: Color::Magenta,
            syntax_instruction: Color::LightBlue,
            syntax_register: Color::LightCyan,
            syntax_keyword: Color::Magenta,
            syntax_macro: Color::LightMagenta,
            syntax_punctuation: Color::Gray,
        }
    }

    /// A palette that avoids red/green contrasts.
    ///
    /// Deuteranopia and protanopia make the conventional red-for-error,
    /// green-for-success pairing hard to separate. This palette substitutes
    /// the blue/orange axis, which stays distinguishable under all three
    /// common forms of colour vision deficiency. Glyphs from
    /// [`super::SymbolSet`] carry the same information independently, so no
    /// state depends on colour alone.
    pub const fn colorblind() -> Self {
        Self {
            background: Color::Rgb(0x16, 0x18, 0x1d),
            foreground: Color::Rgb(0xc9, 0xcc, 0xd4),
            dim: Color::Rgb(0x67, 0x6f, 0x7d),
            bright: Color::Rgb(0xf0, 0xf2, 0xf6),
            border: Color::Rgb(0x35, 0x3a, 0x44),
            border_focused: Color::Rgb(0x64, 0xa8, 0xe8),
            selection: Color::Rgb(0x2b, 0x3c, 0x52),
            cursor_line: Color::Rgb(0x1e, 0x22, 0x2a),
            accent: Color::Rgb(0x64, 0xa8, 0xe8),
            error: Color::Rgb(0xe8, 0x8a, 0x1f),
            warning: Color::Rgb(0xe8, 0xc4, 0x4f),
            success: Color::Rgb(0x64, 0xa8, 0xe8),
            info: Color::Rgb(0x9d, 0xb4, 0xc8),
            changed: Color::Rgb(0xe8, 0xc4, 0x4f),
            breakpoint: Color::Rgb(0xe8, 0x8a, 0x1f),
            current: Color::Rgb(0x64, 0xa8, 0xe8),
            address: Color::Rgb(0x8d, 0x96, 0xa4),
            bytes: Color::Rgb(0xa8, 0x9b, 0xc8),
            syntax_comment: Color::Rgb(0x67, 0x6f, 0x7d),
            syntax_string: Color::Rgb(0x9d, 0xc4, 0xe8),
            syntax_number: Color::Rgb(0xe8, 0xc4, 0x4f),
            syntax_label: Color::Rgb(0xe8, 0xa8, 0x5f),
            syntax_directive: Color::Rgb(0xb8, 0xa8, 0xe8),
            syntax_instruction: Color::Rgb(0x7f, 0xb8, 0xe8),
            syntax_register: Color::Rgb(0xc8, 0xd4, 0xe0),
            syntax_keyword: Color::Rgb(0xb8, 0xa8, 0xe8),
            syntax_macro: Color::Rgb(0xe8, 0xb8, 0x9d),
            syntax_punctuation: Color::Rgb(0x94, 0x9c, 0xa8),
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles(p: &Palette) -> Vec<Color> {
        vec![
            p.foreground,
            p.dim,
            p.border,
            p.border_focused,
            p.error,
            p.warning,
            p.success,
            p.accent,
        ]
    }

    #[test]
    fn ansi_palette_uses_no_rgb_colours() {
        let p = Palette::ansi16();
        for color in roles(&p) {
            assert!(
                !matches!(color, Color::Rgb(..)),
                "ansi16 palette must not contain RGB colours, found {color:?}"
            );
        }
    }

    #[test]
    fn colorblind_palette_separates_error_from_success() {
        let p = Palette::colorblind();
        assert_ne!(p.error, p.success);
        // The point of the palette: success must not read as a green hue.
        if let Color::Rgb(r, g, b) = p.success {
            assert!(
                b > g || r > g,
                "success colour should not be green-dominant"
            );
        }
    }

    #[test]
    fn light_and_dark_differ_in_background() {
        assert_ne!(Palette::light().background, Palette::dark().background);
        assert_ne!(Palette::light().foreground, Palette::dark().foreground);
    }

    #[test]
    fn focused_border_is_distinct_from_idle_border() {
        for p in [
            Palette::dark(),
            Palette::light(),
            Palette::ansi16(),
            Palette::colorblind(),
        ] {
            assert_ne!(p.border, p.border_focused);
        }
    }
}

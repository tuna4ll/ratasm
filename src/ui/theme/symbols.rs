//! Glyph sets that convey state without relying on colour, each with an

/// A complete set of status glyphs; build one with [`SymbolSet::for_unicode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolSet {
    /// Marks a line carrying an enabled breakpoint.
    pub breakpoint: &'static str,
    /// Marks a line carrying a disabled breakpoint.
    pub breakpoint_disabled: &'static str,
    /// Marks the instruction the program counter currently points at.
    pub current_instruction: &'static str,
    /// Marks a value that changed since the previous stop.
    pub changed: &'static str,
    /// Marks a value that did not change.
    pub unchanged: &'static str,
    /// Prefix for error diagnostics.
    pub error: &'static str,
    /// Prefix for warning diagnostics.
    pub warning: &'static str,
    /// Prefix for informational messages.
    pub info: &'static str,
    /// Indicates a successful operation.
    pub success: &'static str,
    /// Indicates a buffer with unsaved modifications.
    pub modified: &'static str,
    /// Points at the stack slot referenced by RSP.
    pub stack_pointer: &'static str,
    /// Points at the stack slot referenced by RBP.
    pub base_pointer: &'static str,
    /// Separates segments in the status bar.
    pub separator: &'static str,
    /// Indicates content continues beyond the right edge.
    pub ellipsis: &'static str,
    /// Marks the selected entry in a list.
    pub selection: &'static str,
    /// Shown next to a set CPU flag.
    pub flag_set: &'static str,
    /// Shown next to a cleared CPU flag.
    pub flag_clear: &'static str,
    /// The unfilled part of a scrollbar track.
    pub scroll_track: &'static str,
    /// The part of a scrollbar showing what is on screen.
    pub scroll_thumb: &'static str,
}

impl SymbolSet {
    /// Glyphs for terminals with full Unicode support.
    pub const fn unicode() -> Self {
        Self {
            breakpoint: "\u{25cf}",
            breakpoint_disabled: "\u{25cb}",
            current_instruction: "\u{25b6}",
            changed: "\u{2022}",
            unchanged: " ",
            error: "\u{2717}",
            warning: "\u{26a0}",
            info: "\u{2139}",
            success: "\u{2713}",
            modified: "\u{25cf}",
            stack_pointer: "\u{2192}",
            base_pointer: "\u{21b3}",
            separator: "\u{2502}",
            ellipsis: "\u{2026}",
            selection: "\u{276f}",
            flag_set: "\u{25a0}",
            flag_clear: "\u{25a1}",
            scroll_track: "\u{2502}",
            scroll_thumb: "\u{2588}",
        }
    }

    /// Glyphs restricted to printable ASCII.
    pub const fn ascii() -> Self {
        Self {
            breakpoint: "*",
            breakpoint_disabled: "o",
            current_instruction: ">",
            changed: "!",
            unchanged: " ",
            error: "E",
            warning: "W",
            info: "i",
            success: "+",
            modified: "*",
            stack_pointer: "->",
            base_pointer: "=>",
            separator: "|",
            ellipsis: "...",
            selection: ">",
            flag_set: "[x]",
            flag_clear: "[ ]",
            scroll_track: "|",
            scroll_thumb: "#",
        }
    }

    /// Chooses a set based on whether Unicode output is permitted.
    pub const fn for_unicode(enabled: bool) -> Self {
        if enabled {
            Self::unicode()
        } else {
            Self::ascii()
        }
    }

    /// Returns the glyph representing a boolean flag state.
    pub const fn flag(&self, set: bool) -> &'static str {
        if set {
            self.flag_set
        } else {
            self.flag_clear
        }
    }
}

impl Default for SymbolSet {
    fn default() -> Self {
        Self::unicode()
    }
}

/// Detects whether the environment appears to support Unicode output.
pub fn detect_unicode_support() -> bool {
    if std::env::var_os("RATASM_ASCII").is_some() {
        return false;
    }
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .find(|value| !value.is_empty())
        .map(|value| {
            let value = value.to_ascii_lowercase();
            value.contains("utf-8") || value.contains("utf8")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_set_is_pure_ascii() {
        let set = SymbolSet::ascii();
        for glyph in [
            set.breakpoint,
            set.breakpoint_disabled,
            set.current_instruction,
            set.changed,
            set.error,
            set.warning,
            set.info,
            set.success,
            set.modified,
            set.stack_pointer,
            set.base_pointer,
            set.separator,
            set.ellipsis,
            set.selection,
            set.flag_set,
            set.flag_clear,
        ] {
            assert!(glyph.is_ascii(), "glyph {glyph:?} is not ASCII");
        }
    }

    #[test]
    fn distinct_states_use_distinct_glyphs() {
        for set in [SymbolSet::unicode(), SymbolSet::ascii()] {
            assert_ne!(set.breakpoint, set.breakpoint_disabled);
            assert_ne!(set.flag_set, set.flag_clear);
            assert_ne!(set.changed, set.unchanged);
        }
    }

    #[test]
    fn flag_selects_by_state() {
        let set = SymbolSet::unicode();
        assert_eq!(set.flag(true), set.flag_set);
        assert_eq!(set.flag(false), set.flag_clear);
    }
}

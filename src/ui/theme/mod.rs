//! Theming: colour palettes and glyph sets.
//!
//! Widgets never name a colour or a glyph directly. They resolve both through
//! this module, which is what keeps two accessibility guarantees enforceable
//! in one place: terminals without true colour or without Unicode stay fully
//! usable, and no state is signalled by colour alone.

pub mod palette;
pub mod symbols;

pub use palette::Palette;
pub use symbols::{detect_unicode_support, SymbolSet};

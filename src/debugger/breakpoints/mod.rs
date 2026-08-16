//! Breakpoints, and keeping ratasm's view of them in step with GDB's.
//!
//! # Two sources of truth, reconciled
//!
//! The user sets breakpoints in the editor before a session exists; GDB
//! assigns them numbers only once it has loaded the program, and may move,
//! renumber or reject them. So a breakpoint here has an optional GDB number:
//! it is *pending* until a session adopts it, and pending breakpoints are
//! replayed when one starts.
//!
//! That is what makes it possible to set a breakpoint, build, and run without
//! the breakpoint quietly disappearing — a failure mode that is invisible
//! until the program blows past the line the user was waiting on.

use std::path::{Path, PathBuf};

use crate::debugger::mi::Value;

/// Where a breakpoint is placed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Location {
    /// A one-based line in a source file.
    Line {
        /// The source file.
        file: PathBuf,
        /// The one-based line number.
        line: usize,
    },
    /// A raw address.
    Address(u64),
    /// A symbol name.
    Symbol(String),
}

impl Location {
    /// The location in the form GDB's `-break-insert` expects.
    pub fn to_gdb_location(&self) -> String {
        match self {
            Location::Line { file, line } => {
                let name = file
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| file.display().to_string());
                format!("{name}:{line}")
            }
            Location::Address(address) => format!("*0x{address:x}"),
            Location::Symbol(name) => name.clone(),
        }
    }

    /// A short rendering for the breakpoint list.
    pub fn display(&self) -> String {
        match self {
            Location::Line { file, line } => {
                let name = file
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| file.display().to_string());
                format!("{name}:{line}")
            }
            Location::Address(address) => format!("0x{address:x}"),
            Location::Symbol(name) => name.clone(),
        }
    }

    /// The source line this location refers to, if it names one.
    pub fn source_line(&self) -> Option<(&Path, usize)> {
        match self {
            Location::Line { file, line } => Some((file.as_path(), *line)),
            _ => None,
        }
    }
}

/// One breakpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakpoint {
    /// GDB's number, once a session has adopted it.
    pub number: Option<u32>,
    /// Where it is placed.
    pub location: Location,
    /// Whether it is currently active.
    pub enabled: bool,
    /// The address GDB resolved it to, once known.
    pub address: Option<u64>,
    /// The function it landed in, once known.
    pub function: Option<String>,
    /// How many times it has been hit.
    pub hit_count: u32,
}

impl Breakpoint {
    /// Creates a breakpoint that no session has adopted yet.
    pub fn pending(location: Location) -> Self {
        Self {
            number: None,
            location,
            enabled: true,
            address: None,
            function: None,
            hit_count: 0,
        }
    }

    /// Whether GDB has adopted this breakpoint.
    pub fn is_active(&self) -> bool {
        self.number.is_some()
    }

    /// A one-line description for the breakpoint list.
    pub fn describe(&self) -> String {
        let mut text = match self.number {
            Some(number) => format!("{number}: "),
            None => "-: ".to_owned(),
        };
        text.push_str(&self.location.display());
        if let Some(function) = &self.function {
            text.push_str(&format!(" in {function}"));
        }
        if !self.enabled {
            text.push_str(" (disabled)");
        }
        if self.hit_count > 0 {
            text.push_str(&format!(" hit {}", self.hit_count));
        }
        text
    }
}

/// The set of breakpoints the user has asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BreakpointSet {
    breakpoints: Vec<Breakpoint>,
}

impl BreakpointSet {
    /// Creates an empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every breakpoint, in the order they were added.
    pub fn all(&self) -> &[Breakpoint] {
        &self.breakpoints
    }

    /// The number of breakpoints.
    pub fn len(&self) -> usize {
        self.breakpoints.len()
    }

    /// Whether there are no breakpoints.
    pub fn is_empty(&self) -> bool {
        self.breakpoints.is_empty()
    }

    /// The breakpoint at a source line, if there is one.
    pub fn at_line(&self, file: &Path, line: usize) -> Option<&Breakpoint> {
        self.breakpoints.iter().find(|breakpoint| {
            breakpoint
                .location
                .source_line()
                .is_some_and(|(path, number)| same_file(path, file) && number == line)
        })
    }

    /// Whether an enabled breakpoint sits on a source line.
    ///
    /// Used by the editor gutter, which shows disabled breakpoints with a
    /// different glyph rather than hiding them.
    pub fn is_set_at(&self, file: &Path, line: usize) -> bool {
        self.at_line(file, line).is_some()
    }

    /// Adds a breakpoint, or removes the one already at that line.
    ///
    /// Returns `true` when a breakpoint was added and `false` when one was
    /// removed, so the caller knows which GDB command to send.
    pub fn toggle_line(&mut self, file: &Path, line: usize) -> bool {
        if let Some(index) = self.index_of_line(file, line) {
            self.breakpoints.remove(index);
            false
        } else {
            self.breakpoints.push(Breakpoint::pending(Location::Line {
                file: file.to_path_buf(),
                line,
            }));
            true
        }
    }

    /// Adds a breakpoint at an address, if one is not already there.
    pub fn add_address(&mut self, address: u64) -> bool {
        if self
            .breakpoints
            .iter()
            .any(|breakpoint| breakpoint.location == Location::Address(address))
        {
            return false;
        }
        self.breakpoints
            .push(Breakpoint::pending(Location::Address(address)));
        true
    }

    /// Adds a breakpoint at a symbol, if one is not already there.
    pub fn add_symbol(&mut self, name: &str) -> bool {
        let location = Location::Symbol(name.to_owned());
        if self
            .breakpoints
            .iter()
            .any(|breakpoint| breakpoint.location == location)
        {
            return false;
        }
        self.breakpoints.push(Breakpoint::pending(location));
        true
    }

    /// Removes the breakpoint with a GDB number.
    pub fn remove_number(&mut self, number: u32) -> bool {
        let before = self.breakpoints.len();
        self.breakpoints
            .retain(|breakpoint| breakpoint.number != Some(number));
        self.breakpoints.len() != before
    }

    /// Removes the breakpoint at an index in the list.
    pub fn remove_index(&mut self, index: usize) -> Option<Breakpoint> {
        if index < self.breakpoints.len() {
            Some(self.breakpoints.remove(index))
        } else {
            None
        }
    }

    /// Enables or disables a breakpoint by index.
    pub fn set_enabled(&mut self, index: usize, enabled: bool) -> bool {
        match self.breakpoints.get_mut(index) {
            Some(breakpoint) => {
                breakpoint.enabled = enabled;
                true
            }
            None => false,
        }
    }

    /// Removes every breakpoint.
    pub fn clear(&mut self) {
        self.breakpoints.clear();
    }

    /// Forgets every GDB number, marking all breakpoints pending again.
    ///
    /// Called when a session ends: the numbers belonged to that session and
    /// mean nothing to the next one.
    pub fn detach(&mut self) {
        for breakpoint in &mut self.breakpoints {
            breakpoint.number = None;
            breakpoint.address = None;
            breakpoint.hit_count = 0;
        }
    }

    /// The breakpoints that still need to be sent to GDB.
    pub fn pending(&self) -> Vec<&Breakpoint> {
        self.breakpoints
            .iter()
            .filter(|breakpoint| breakpoint.number.is_none())
            .collect()
    }

    /// Records the number and resolved details GDB assigned to a location.
    ///
    /// Matching is by location rather than by order, because GDB may reject
    /// one breakpoint and still accept later ones.
    pub fn adopt(&mut self, location: &Location, record: &Value) -> bool {
        let Some(index) = self
            .breakpoints
            .iter()
            .position(|breakpoint| &breakpoint.location == location)
        else {
            return false;
        };

        let breakpoint = &mut self.breakpoints[index];
        breakpoint.number = record.get_int("number").and_then(|n| u32::try_from(n).ok());
        breakpoint.address = record.get_address("addr");
        breakpoint.function = record.get_str("func").map(str::to_owned);
        if let Some(enabled) = record.get_str("enabled") {
            breakpoint.enabled = enabled == "y";
        }
        true
    }

    /// Records a hit on the breakpoint GDB reported.
    pub fn record_hit(&mut self, number: u32) {
        if let Some(breakpoint) = self
            .breakpoints
            .iter_mut()
            .find(|breakpoint| breakpoint.number == Some(number))
        {
            breakpoint.hit_count = breakpoint.hit_count.saturating_add(1);
        }
    }

    fn index_of_line(&self, file: &Path, line: usize) -> Option<usize> {
        self.breakpoints.iter().position(|breakpoint| {
            breakpoint
                .location
                .source_line()
                .is_some_and(|(path, number)| same_file(path, file) && number == line)
        })
    }
}

/// Whether two paths refer to the same source file.
///
/// GDB reports the file name it was given, which may be relative where the
/// editor holds an absolute path. Comparing the file names as a fallback keeps
/// breakpoints attached in the common single-directory project; comparing only
/// the full paths would silently fail to match.
fn same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.file_name(), right.file_name()) {
        (Some(left_name), Some(right_name)) => left_name == right_name,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn main_asm() -> PathBuf {
        PathBuf::from("/project/src/main.asm")
    }

    #[test]
    fn a_new_set_is_empty() {
        let set = BreakpointSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn toggling_adds_then_removes() {
        let mut set = BreakpointSet::new();
        assert!(set.toggle_line(&main_asm(), 12), "first toggle adds");
        assert_eq!(set.len(), 1);
        assert!(set.is_set_at(&main_asm(), 12));

        assert!(!set.toggle_line(&main_asm(), 12), "second toggle removes");
        assert!(set.is_empty());
        assert!(!set.is_set_at(&main_asm(), 12));
    }

    #[test]
    fn breakpoints_on_different_lines_coexist() {
        let mut set = BreakpointSet::new();
        set.toggle_line(&main_asm(), 5);
        set.toggle_line(&main_asm(), 12);
        assert_eq!(set.len(), 2);
        assert!(set.is_set_at(&main_asm(), 5));
        assert!(set.is_set_at(&main_asm(), 12));
        assert!(!set.is_set_at(&main_asm(), 7));
    }

    #[test]
    fn a_relative_path_matches_an_absolute_one() {
        // GDB reports "main.asm" where the editor holds the full path.
        let mut set = BreakpointSet::new();
        set.toggle_line(&main_asm(), 9);
        assert!(set.is_set_at(Path::new("main.asm"), 9));
        assert!(!set.is_set_at(Path::new("other.asm"), 9));
    }

    #[test]
    fn a_new_breakpoint_starts_pending_and_enabled() {
        let mut set = BreakpointSet::new();
        set.toggle_line(&main_asm(), 3);
        let breakpoint = &set.all()[0];
        assert!(!breakpoint.is_active(), "no session has adopted it");
        assert!(breakpoint.enabled);
        assert_eq!(breakpoint.hit_count, 0);
        assert_eq!(set.pending().len(), 1);
    }

    #[test]
    fn adopting_records_the_details_gdb_resolved() {
        let mut set = BreakpointSet::new();
        let location = Location::Line {
            file: main_asm(),
            line: 9,
        };
        set.toggle_line(&main_asm(), 9);

        let record = Value::Tuple(vec![
            ("number".to_owned(), Value::String("1".to_owned())),
            ("enabled".to_owned(), Value::String("y".to_owned())),
            ("addr".to_owned(), Value::String("0x004000b0".to_owned())),
            ("func".to_owned(), Value::String("_start".to_owned())),
        ]);
        assert!(set.adopt(&location, &record));

        let breakpoint = &set.all()[0];
        assert_eq!(breakpoint.number, Some(1));
        assert_eq!(breakpoint.address, Some(0x0040_00b0));
        assert_eq!(breakpoint.function.as_deref(), Some("_start"));
        assert!(breakpoint.is_active());
        assert!(set.pending().is_empty());
    }

    #[test]
    fn adopting_an_unknown_location_changes_nothing() {
        let mut set = BreakpointSet::new();
        set.toggle_line(&main_asm(), 9);
        let elsewhere = Location::Line {
            file: PathBuf::from("other.asm"),
            line: 1,
        };
        assert!(!set.adopt(&elsewhere, &Value::Tuple(Vec::new())));
        assert!(!set.all()[0].is_active());
    }

    #[test]
    fn a_disabled_breakpoint_is_recorded_as_disabled() {
        let mut set = BreakpointSet::new();
        let location = Location::Symbol("_start".to_owned());
        set.add_symbol("_start");
        let record = Value::Tuple(vec![
            ("number".to_owned(), Value::String("1".to_owned())),
            ("enabled".to_owned(), Value::String("n".to_owned())),
        ]);
        set.adopt(&location, &record);
        assert!(!set.all()[0].enabled);
        assert!(set.all()[0].describe().contains("disabled"));
    }

    #[test]
    fn ending_a_session_returns_every_breakpoint_to_pending() {
        // GDB's numbers belong to one session and must not be reused.
        let mut set = BreakpointSet::new();
        set.toggle_line(&main_asm(), 9);
        set.adopt(
            &Location::Line {
                file: main_asm(),
                line: 9,
            },
            &Value::Tuple(vec![("number".to_owned(), Value::String("1".to_owned()))]),
        );
        set.record_hit(1);
        assert_eq!(set.all()[0].hit_count, 1);

        set.detach();
        let breakpoint = &set.all()[0];
        assert!(!breakpoint.is_active());
        assert_eq!(breakpoint.hit_count, 0);
        assert_eq!(breakpoint.address, None);
        assert_eq!(set.pending().len(), 1, "it must be replayed next time");
    }

    #[test]
    fn hits_are_counted_for_the_right_breakpoint() {
        let mut set = BreakpointSet::new();
        set.add_symbol("first");
        set.add_symbol("second");
        set.adopt(
            &Location::Symbol("first".to_owned()),
            &Value::Tuple(vec![("number".to_owned(), Value::String("1".to_owned()))]),
        );
        set.adopt(
            &Location::Symbol("second".to_owned()),
            &Value::Tuple(vec![("number".to_owned(), Value::String("2".to_owned()))]),
        );

        set.record_hit(2);
        set.record_hit(2);
        assert_eq!(set.all()[0].hit_count, 0);
        assert_eq!(set.all()[1].hit_count, 2);
    }

    #[test]
    fn recording_a_hit_on_an_unknown_number_is_ignored() {
        let mut set = BreakpointSet::new();
        set.add_symbol("_start");
        set.record_hit(99);
        assert_eq!(set.all()[0].hit_count, 0);
    }

    #[test]
    fn address_and_symbol_breakpoints_are_not_duplicated() {
        let mut set = BreakpointSet::new();
        assert!(set.add_address(0x4000));
        assert!(!set.add_address(0x4000), "the same address twice");
        assert!(set.add_address(0x4008));
        assert!(set.add_symbol("_start"));
        assert!(!set.add_symbol("_start"));
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn gdb_locations_use_the_expected_syntax() {
        assert_eq!(
            Location::Line {
                file: main_asm(),
                line: 12
            }
            .to_gdb_location(),
            "main.asm:12"
        );
        assert_eq!(Location::Address(0x4000b0).to_gdb_location(), "*0x4000b0");
        assert_eq!(
            Location::Symbol("_start".into()).to_gdb_location(),
            "_start"
        );
    }

    #[test]
    fn removing_by_number_and_index_both_work() {
        let mut set = BreakpointSet::new();
        set.add_symbol("a");
        set.add_symbol("b");
        set.adopt(
            &Location::Symbol("a".to_owned()),
            &Value::Tuple(vec![("number".to_owned(), Value::String("7".to_owned()))]),
        );

        assert!(set.remove_number(7));
        assert_eq!(set.len(), 1);
        assert!(!set.remove_number(7), "already gone");

        assert!(set.remove_index(0).is_some());
        assert!(set.is_empty());
        assert!(set.remove_index(0).is_none());
    }

    #[test]
    fn enabling_and_disabling_by_index_is_bounds_checked() {
        let mut set = BreakpointSet::new();
        set.add_symbol("_start");
        assert!(set.set_enabled(0, false));
        assert!(!set.all()[0].enabled);
        assert!(!set.set_enabled(9, true), "out of range is refused");
    }

    #[test]
    fn clearing_removes_everything() {
        let mut set = BreakpointSet::new();
        set.add_symbol("a");
        set.toggle_line(&main_asm(), 1);
        set.clear();
        assert!(set.is_empty());
    }

    #[test]
    fn descriptions_are_readable_in_every_state() {
        let mut set = BreakpointSet::new();
        set.toggle_line(&main_asm(), 9);
        assert_eq!(set.all()[0].describe(), "-: main.asm:9");

        set.adopt(
            &Location::Line {
                file: main_asm(),
                line: 9,
            },
            &Value::Tuple(vec![
                ("number".to_owned(), Value::String("1".to_owned())),
                ("func".to_owned(), Value::String("_start".to_owned())),
            ]),
        );
        set.record_hit(1);
        assert_eq!(set.all()[0].describe(), "1: main.asm:9 in _start hit 1");
    }
}

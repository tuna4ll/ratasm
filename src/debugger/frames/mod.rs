//! The call stack.
//!
//! Distinct from the *stack memory* panel, which shows the bytes around `RSP`.
//! This is the chain of calls that got the program where it is: who called
//! whom, and from which line. In hand-written assembly the two are easy to
//! confuse, and the call stack is the one that answers "how did I get here?"
//! after a `ret` lands somewhere unexpected.

use crate::debugger::mi::Value;

/// One frame of the call stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Distance from the innermost frame; zero is where execution is now.
    pub level: u32,
    /// The address of the instruction in this frame.
    pub address: Option<u64>,
    /// The function the address falls in, when known.
    pub function: Option<String>,
    /// The source file, when the program carries debug information.
    pub file: Option<String>,
    /// The one-based source line.
    pub line: Option<usize>,
}

impl Frame {
    /// A one-line description for the panel.
    ///
    /// Degrades gracefully: an address is always available, a function name
    /// usually is, and a source line only when the program was built with
    /// debug information.
    pub fn describe(&self) -> String {
        let mut text = format!("#{}", self.level);

        match self.address {
            Some(address) => text.push_str(&format!("  {address:#018x}")),
            None => text.push_str("  ?"),
        }
        if let Some(function) = &self.function {
            text.push_str(&format!("  {function}"));
        }
        if let (Some(file), Some(line)) = (&self.file, self.line) {
            text.push_str(&format!("  {file}:{line}"));
        }
        text
    }

    /// Whether this is the frame execution is stopped in.
    pub fn is_innermost(&self) -> bool {
        self.level == 0
    }

    /// The source location, when the frame has one.
    pub fn source(&self) -> Option<(&str, usize)> {
        match (&self.file, self.line) {
            (Some(file), Some(line)) => Some((file.as_str(), line)),
            _ => None,
        }
    }
}

/// Decodes the `stack` payload of a `-stack-list-frames` reply.
///
/// GDB returns a repeated-key list — `stack=[frame={...},frame={...}]` — so
/// the frames are read out by key rather than as plain elements; treating it
/// as a map would keep only the last one.
pub fn parse_frames(value: &Value) -> Vec<Frame> {
    value
        .items("frame")
        .into_iter()
        .filter_map(|frame| {
            let level = frame.get_int("level").and_then(|n| u32::try_from(n).ok())?;
            Some(Frame {
                level,
                address: frame.get_address("addr"),
                function: frame.get_str("func").map(str::to_owned),
                file: frame.get_str("file").map(str::to_owned),
                line: frame
                    .get_int("line")
                    .and_then(|line| usize::try_from(line).ok()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::mi::parse_line;

    /// A real reply, captured from GDB.
    const REPLY: &str = r#"^done,stack=[frame={level="0",addr="0x0000000000401006",func="helper",file="main.asm",fullname="/tmp/p/main.asm",line="12",arch="i386:x86-64"},frame={level="1",addr="0x0000000000401015",func="_start",file="main.asm",fullname="/tmp/p/main.asm",line="20",arch="i386:x86-64"}]"#;

    fn frames() -> Vec<Frame> {
        let record = parse_line(REPLY).expect("parses").expect("a record");
        parse_frames(record.get("stack").expect("stack payload"))
    }

    #[test]
    fn every_frame_is_kept() {
        // The repeated-key list is the trap: a map would keep only the last.
        let frames = frames();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].level, 0);
        assert_eq!(frames[1].level, 1);
    }

    #[test]
    fn frame_details_are_decoded() {
        let frames = frames();
        assert_eq!(frames[0].function.as_deref(), Some("helper"));
        assert_eq!(frames[0].address, Some(0x0040_1006));
        assert_eq!(frames[0].source(), Some(("main.asm", 12)));

        assert_eq!(frames[1].function.as_deref(), Some("_start"));
        assert_eq!(frames[1].line, Some(20));
    }

    #[test]
    fn the_innermost_frame_is_where_execution_is() {
        let frames = frames();
        assert!(frames[0].is_innermost());
        assert!(!frames[1].is_innermost());
    }

    #[test]
    fn a_frame_without_source_information_still_describes_itself() {
        // Without -g there is no file or line, but the address is always there
        // and that is the part the user needs.
        let frame = Frame {
            level: 0,
            address: Some(0x401000),
            function: Some("_start".to_owned()),
            file: None,
            line: None,
        };
        let text = frame.describe();
        assert!(text.contains("0x0000000000401000"));
        assert!(text.contains("_start"));
        assert!(!text.contains(':'), "no source location to show: {text}");
    }

    #[test]
    fn a_frame_with_nothing_but_a_level_does_not_panic() {
        let frame = Frame {
            level: 3,
            address: None,
            function: None,
            file: None,
            line: None,
        };
        assert_eq!(frame.describe(), "#3  ?");
    }

    #[test]
    fn descriptions_include_the_source_location_when_present() {
        assert_eq!(
            frames()[0].describe(),
            "#0  0x0000000000401006  helper  main.asm:12"
        );
    }

    #[test]
    fn a_malformed_reply_yields_no_frames_rather_than_a_panic() {
        assert!(parse_frames(&Value::String("nonsense".to_owned())).is_empty());
        assert!(parse_frames(&Value::List(Vec::new())).is_empty());

        // A frame with no level cannot be placed in the chain, so it is
        // dropped rather than guessed at.
        let value = Value::List(vec![Value::Tuple(vec![(
            "frame".to_owned(),
            Value::Tuple(vec![("addr".to_owned(), Value::String("0x1".to_owned()))]),
        )])]);
        assert!(parse_frames(&value).is_empty());
    }
}

//! GDB/MI output records.
//!
//! Every line GDB writes on its machine interface is one of a handful of
//! record types, distinguished by the first character after an optional
//! numeric token:
//!
//! ```text
//! ^done,key=value      result record: the answer to a command
//! *stopped,reason=...  exec-async: the program's execution state changed
//! =thread-created,...  notify-async: something in GDB's own state changed
//! +download,...        status-async: progress on a long operation
//! ~"text"              console stream: what a human user would have seen
//! @"text"              target stream: output from the program itself
//! &"text"              log stream: GDB's internal logging
//! (gdb)                the prompt, marking the end of a response
//! ```
//!
//! # Why tokens matter
//!
//! Commands may be sent before earlier ones have replied, and async events
//! arrive interleaved with replies at any time. The numeric token that a
//! client puts in front of a command is echoed on its result record, and it is
//! the *only* reliable way to match an answer to its question. Matching on
//! order instead breaks the first time the program hits a breakpoint while a
//! command is in flight.

use super::value::{ParseError, Parser, Value};

/// The class of a result record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultClass {
    /// The command completed.
    Done,
    /// The command completed; a synonym for `done` used by some commands.
    Running,
    /// A remote connection was established.
    Connected,
    /// The command failed.
    Error,
    /// GDB is exiting.
    Exit,
}

impl ResultClass {
    /// Parses a result class name.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "done" => Some(ResultClass::Done),
            "running" => Some(ResultClass::Running),
            "connected" => Some(ResultClass::Connected),
            "error" => Some(ResultClass::Error),
            "exit" => Some(ResultClass::Exit),
            _ => None,
        }
    }

    /// Whether this class reports failure.
    pub fn is_error(self) -> bool {
        matches!(self, ResultClass::Error)
    }

    /// The name as it appears in the protocol.
    pub const fn as_str(self) -> &'static str {
        match self {
            ResultClass::Done => "done",
            ResultClass::Running => "running",
            ResultClass::Connected => "connected",
            ResultClass::Error => "error",
            ResultClass::Exit => "exit",
        }
    }
}

/// Which asynchronous channel a record arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncKind {
    /// `*` — the program's execution state changed.
    Exec,
    /// `+` — progress on a long-running operation.
    Status,
    /// `=` — GDB's own state changed.
    Notify,
}

/// Which stream a piece of text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// `~` — what an interactive user would have seen.
    Console,
    /// `@` — output from the program being debugged.
    Target,
    /// `&` — GDB's internal log.
    Log,
}

/// One line of GDB/MI output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    /// The answer to a command.
    Result {
        /// The token echoed from the command, when it had one.
        token: Option<u32>,
        /// Whether the command succeeded.
        class: ResultClass,
        /// The payload.
        results: Vec<(String, Value)>,
    },
    /// An asynchronous notification.
    Async {
        /// The token, when the event was caused by a tokenised command.
        token: Option<u32>,
        /// Which channel it arrived on.
        kind: AsyncKind,
        /// The event name, such as `stopped` or `thread-group-added`.
        class: String,
        /// The payload.
        results: Vec<(String, Value)>,
    },
    /// A line of text on one of the streams.
    Stream {
        /// Which stream it came from.
        kind: StreamKind,
        /// The text, with escapes decoded.
        text: String,
    },
    /// The `(gdb)` prompt marking the end of a response.
    Prompt,
}

impl Record {
    /// The token, when the record carries one.
    pub fn token(&self) -> Option<u32> {
        match self {
            Record::Result { token, .. } | Record::Async { token, .. } => *token,
            _ => None,
        }
    }

    /// The payload, for records that carry one.
    pub fn results(&self) -> &[(String, Value)] {
        match self {
            Record::Result { results, .. } | Record::Async { results, .. } => results,
            _ => &[],
        }
    }

    /// Looks up a key in the record's payload.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.results()
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// Looks up a key and returns it as a string.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    /// The error message, for a failed result record.
    ///
    /// GDB puts the message in a `msg` field; a missing one is reported as a
    /// generic message rather than an empty string so the user always sees
    /// something.
    pub fn error_message(&self) -> Option<String> {
        match self {
            Record::Result {
                class: ResultClass::Error,
                ..
            } => Some(
                self.get_str("msg")
                    .unwrap_or("the debugger reported an error with no message")
                    .to_owned(),
            ),
            _ => None,
        }
    }

    /// Whether this record is a completed answer to a command.
    pub fn is_result(&self) -> bool {
        matches!(self, Record::Result { .. })
    }

    /// Whether this record reports that the program stopped.
    pub fn is_stopped(&self) -> bool {
        matches!(
            self,
            Record::Async {
                kind: AsyncKind::Exec,
                class,
                ..
            } if class == "stopped"
        )
    }

    /// The `reason` field of a stop record.
    pub fn stop_reason(&self) -> Option<&str> {
        if self.is_stopped() {
            self.get_str("reason")
        } else {
            None
        }
    }

    /// Whether this record reports that the program finished.
    ///
    /// Program exit is not a record class of its own: GDB reports it as an
    /// ordinary `*stopped` whose reason is `exited`, `exited-normally` or
    /// `exited-signalled`. Treating "stopped" as "paused and inspectable"
    /// without checking the reason would leave the debugger waiting to step a
    /// process that no longer exists.
    pub fn is_program_exit(&self) -> bool {
        self.stop_reason()
            .is_some_and(|reason| reason.starts_with("exited"))
    }

    /// The program's exit status, when this record reports an exit.
    ///
    /// GDB writes this field in **octal**: a program exiting with status 9 is
    /// reported as `exit-code="011"`. Reading it as decimal is a silent
    /// off-by-a-lot, so the conversion happens here once.
    pub fn exit_code(&self) -> Option<i32> {
        if !self.is_program_exit() {
            return None;
        }
        match self.get_str("exit-code") {
            Some(text) => i32::from_str_radix(text.trim(), 8).ok(),
            // `exited-normally` carries no code and means zero.
            None => (self.stop_reason() == Some("exited-normally")).then_some(0),
        }
    }

    /// Whether this record reports that the program resumed.
    pub fn is_running(&self) -> bool {
        matches!(
            self,
            Record::Async {
                kind: AsyncKind::Exec,
                class,
                ..
            } if class == "running"
        )
    }
}

/// Parses one line of GDB/MI output.
///
/// Returns `Ok(None)` for a blank line, which GDB emits freely and which
/// carries no information.
///
/// # Errors
///
/// Returns [`ParseError`] when the line has a recognised prefix but a
/// malformed payload.
pub fn parse_line(line: &str) -> Result<Option<Record>, ParseError> {
    let line = line.trim_end_matches(['\r', '\n']);
    let trimmed = line.trim();

    if trimmed.is_empty() {
        return Ok(None);
    }
    // The prompt is written as "(gdb)" and often padded with a space.
    if trimmed == "(gdb)" || trimmed == "(gdb) " {
        return Ok(Some(Record::Prompt));
    }

    // A stream record is never preceded by a token.
    if let Some(kind) = stream_kind(trimmed.as_bytes()[0]) {
        let mut parser = Parser::new(&trimmed[1..]);
        let text = match parser.parse_value()? {
            Value::String(text) => text,
            other => other.to_string(),
        };
        return Ok(Some(Record::Stream { kind, text }));
    }

    let (token, rest) = split_token(trimmed);
    let Some(marker) = rest.as_bytes().first().copied() else {
        return Ok(None);
    };
    let body = &rest[1..];

    match marker {
        b'^' => {
            let (class_name, results) = split_class(body);
            let class = ResultClass::parse(class_name).ok_or(ParseError::Unexpected {
                found: class_name.chars().next().unwrap_or('?'),
                expected: "a result class",
                position: 0,
            })?;
            Ok(Some(Record::Result {
                token,
                class,
                results: parse_payload(results)?,
            }))
        }
        b'*' | b'+' | b'=' => {
            let kind = match marker {
                b'*' => AsyncKind::Exec,
                b'+' => AsyncKind::Status,
                _ => AsyncKind::Notify,
            };
            let (class_name, results) = split_class(body);
            Ok(Some(Record::Async {
                token,
                kind,
                class: class_name.to_owned(),
                results: parse_payload(results)?,
            }))
        }
        // GDB occasionally emits plain text, for instance its startup banner
        // when it is not in pure MI mode. Treating it as console output keeps
        // it visible rather than throwing it away.
        _ => Ok(Some(Record::Stream {
            kind: StreamKind::Console,
            text: line.to_owned(),
        })),
    }
}

/// Parses a payload, tolerating an empty one.
fn parse_payload(text: &str) -> Result<Vec<(String, Value)>, ParseError> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let mut parser = Parser::new(text);
    parser.parse_results()
}

/// Splits the leading numeric token from a line.
fn split_token(line: &str) -> (Option<u32>, &str) {
    let digits = line
        .as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 {
        return (None, line);
    }
    let token = line[..digits].parse().ok();
    (token, &line[digits..])
}

/// Splits the class name from the results that follow it.
fn split_class(body: &str) -> (&str, &str) {
    match body.find(',') {
        Some(index) => (&body[..index], &body[index + 1..]),
        None => (body, ""),
    }
}

/// Maps a stream marker byte to its kind.
fn stream_kind(marker: u8) -> Option<StreamKind> {
    match marker {
        b'~' => Some(StreamKind::Console),
        b'@' => Some(StreamKind::Target),
        b'&' => Some(StreamKind::Log),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Record {
        parse_line(line)
            .unwrap_or_else(|error| panic!("{line:?} failed to parse: {error}"))
            .unwrap_or_else(|| panic!("{line:?} produced no record"))
    }

    #[test]
    fn a_bare_done_is_a_result_record() {
        let record = parse("^done");
        assert!(record.is_result());
        assert_eq!(record.token(), None);
        assert!(record.results().is_empty());
        match record {
            Record::Result { class, .. } => assert_eq!(class, ResultClass::Done),
            other => panic!("unexpected record: {other:?}"),
        }
    }

    #[test]
    fn a_token_is_carried_through() {
        // Without this, replies cannot be matched to their commands.
        let record = parse("0042^done");
        assert_eq!(record.token(), Some(42));
    }

    #[test]
    fn a_result_payload_is_parsed() {
        let record = parse(r#"^done,value="0x3c""#);
        assert_eq!(record.get_str("value"), Some("0x3c"));
    }

    #[test]
    fn an_error_record_exposes_its_message() {
        let record = parse(r#"^error,msg="No symbol \"nope\" in current context.""#);
        assert_eq!(
            record.error_message().as_deref(),
            Some("No symbol \"nope\" in current context.")
        );
        match record {
            Record::Result { class, .. } => assert!(class.is_error()),
            other => panic!("unexpected record: {other:?}"),
        }
    }

    #[test]
    fn an_error_without_a_message_still_reports_something() {
        let record = parse("^error");
        assert!(record.error_message().is_some());
        assert!(!record.error_message().unwrap_or_default().is_empty());
    }

    #[test]
    fn a_successful_record_has_no_error_message() {
        assert!(parse("^done").error_message().is_none());
        assert!(parse("*stopped").error_message().is_none());
    }

    #[test]
    fn the_running_class_is_recognised() {
        match parse("^running") {
            Record::Result { class, .. } => assert_eq!(class, ResultClass::Running),
            other => panic!("unexpected record: {other:?}"),
        }
    }

    #[test]
    fn an_unknown_result_class_is_an_error() {
        assert!(parse_line("^nonsense").is_err());
    }

    #[test]
    fn a_stopped_event_is_recognised_with_its_payload() {
        let line = r#"*stopped,reason="breakpoint-hit",bkptno="1",frame={addr="0x004000b0",func="_start",file="main.asm",line="9"},thread-id="1""#;
        let record = parse(line);
        assert!(record.is_stopped());
        assert!(!record.is_running());
        assert_eq!(record.get_str("reason"), Some("breakpoint-hit"));

        let frame = record.get("frame").expect("frame");
        assert_eq!(frame.get_int("line"), Some(9));
        assert_eq!(frame.get_address("addr"), Some(0x0040_00b0));
    }

    #[test]
    fn a_running_event_is_recognised() {
        let record = parse(r#"*running,thread-id="all""#);
        assert!(record.is_running());
        assert!(!record.is_stopped());
    }

    #[test]
    fn notify_and_status_records_are_distinguished_from_exec() {
        match parse(r#"=thread-group-added,id="i1""#) {
            Record::Async { kind, class, .. } => {
                assert_eq!(kind, AsyncKind::Notify);
                assert_eq!(class, "thread-group-added");
            }
            other => panic!("unexpected record: {other:?}"),
        }
        match parse(r#"+download,section=".text""#) {
            Record::Async { kind, .. } => assert_eq!(kind, AsyncKind::Status),
            other => panic!("unexpected record: {other:?}"),
        }
    }

    #[test]
    fn a_notify_record_is_not_treated_as_a_stop() {
        // `=` records describe GDB's state, never the program's execution.
        let record = parse(r#"=breakpoint-modified,bkpt={number="1"}"#);
        assert!(!record.is_stopped());
        assert!(!record.is_result());
    }

    #[test]
    fn the_three_stream_kinds_are_distinguished() {
        for (line, expected) in [
            (r#"~"console text\n""#, StreamKind::Console),
            (r#"@"program output\n""#, StreamKind::Target),
            (r#"&"log line\n""#, StreamKind::Log),
        ] {
            match parse(line) {
                Record::Stream { kind, text } => {
                    assert_eq!(kind, expected, "wrong kind for {line}");
                    assert!(text.ends_with('\n'), "escape not decoded in {line}");
                }
                other => panic!("unexpected record: {other:?}"),
            }
        }
    }

    #[test]
    fn stream_text_keeps_embedded_punctuation() {
        // Program output legitimately contains commas, braces and quotes.
        let record = parse(r#"@"mov rax, {1} \"quoted\"\n""#);
        match record {
            Record::Stream { text, .. } => {
                assert_eq!(text, "mov rax, {1} \"quoted\"\n");
            }
            other => panic!("unexpected record: {other:?}"),
        }
    }

    #[test]
    fn the_prompt_is_recognised() {
        assert_eq!(parse("(gdb)"), Record::Prompt);
        assert_eq!(parse("(gdb) "), Record::Prompt);
    }

    #[test]
    fn blank_lines_produce_nothing() {
        assert_eq!(parse_line(""), Ok(None));
        assert_eq!(parse_line("   "), Ok(None));
        assert_eq!(parse_line("\n"), Ok(None));
    }

    #[test]
    fn trailing_line_endings_are_ignored() {
        assert!(parse("^done\r\n").is_result());
        assert!(parse("^done\n").is_result());
    }

    #[test]
    fn unrecognised_text_is_kept_as_console_output() {
        // GDB's startup banner, for instance. Discarding it would hide
        // messages the user may need.
        match parse("GNU gdb (GDB) 17.2") {
            Record::Stream { kind, text } => {
                assert_eq!(kind, StreamKind::Console);
                assert!(text.contains("GNU gdb"));
            }
            other => panic!("unexpected record: {other:?}"),
        }
    }

    #[test]
    fn a_malformed_payload_is_an_error_not_a_panic() {
        assert!(parse_line(r#"^done,value="unterminated"#).is_err());
        assert!(parse_line("*stopped,=").is_err());
    }

    #[test]
    fn a_real_session_transcript_parses_completely() {
        // A verbatim exchange from starting a program and hitting a breakpoint.
        const TRANSCRIPT: &str = r#"=thread-group-added,id="i1"
~"GNU gdb (GDB) 17.2\n"
(gdb)
-break-insert _start
^done,bkpt={number="1",type="breakpoint",disp="keep",enabled="y",addr="0x00000000004000b0",func="_start",file="main.asm",fullname="/tmp/p/main.asm",line="9",thread-groups=["i1"],times="0",original-location="_start"}
(gdb)
=thread-created,id="1",group-id="i1"
^running
*running,thread-id="all"
(gdb)
@"Hello, world!\n"
*stopped,reason="breakpoint-hit",disp="keep",bkptno="1",frame={addr="0x00000000004000b0",func="_start",args=[],file="main.asm",line="9"},thread-id="1",stopped-threads="all",core="2"
(gdb)
"#;

        let mut results = 0;
        let mut stops = 0;
        let mut prompts = 0;
        let mut target_output = String::new();

        for line in TRANSCRIPT.lines() {
            // The command echo is not MI output; skip it as GDB would not
            // send it back.
            if line.starts_with('-') {
                continue;
            }
            let record =
                parse_line(line).unwrap_or_else(|error| panic!("{line:?} failed: {error}"));
            match record {
                Some(Record::Result { .. }) => results += 1,
                Some(Record::Prompt) => prompts += 1,
                Some(ref record) if record.is_stopped() => stops += 1,
                Some(Record::Stream {
                    kind: StreamKind::Target,
                    ref text,
                }) => target_output.push_str(text),
                _ => {}
            }
        }

        assert_eq!(results, 2, "^done and ^running");
        assert_eq!(stops, 1);
        assert_eq!(prompts, 4);
        assert_eq!(target_output, "Hello, world!\n");
    }

    #[test]
    fn a_breakpoint_record_exposes_its_fields() {
        let line = r#"^done,bkpt={number="2",type="breakpoint",enabled="y",addr="0x004000b4",func="_start",file="main.asm",line="12",times="0"}"#;
        let record = parse(line);
        let bkpt = record.get("bkpt").expect("bkpt");
        assert_eq!(bkpt.get_int("number"), Some(2));
        assert_eq!(bkpt.get_str("enabled"), Some("y"));
        assert_eq!(bkpt.get_address("addr"), Some(0x0040_00b4));
        assert_eq!(bkpt.get_int("line"), Some(12));
    }

    #[test]
    fn a_normal_exit_is_recognised_as_an_exit_not_a_pause() {
        // Verbatim from GDB 17.2.
        let record = parse(r#"*stopped,reason="exited-normally""#);
        assert!(record.is_stopped(), "it is still a stop record");
        assert!(record.is_program_exit(), "but the program has finished");
        assert_eq!(record.exit_code(), Some(0));
    }

    #[test]
    fn an_exit_code_is_decoded_from_octal() {
        // GDB writes the status in octal: 011 is 9, not 11. Verified against
        // GDB 17.2 with a program exiting with status 9.
        let record = parse(r#"*stopped,reason="exited",exit-code="011""#);
        assert!(record.is_program_exit());
        assert_eq!(record.exit_code(), Some(9));
    }

    #[test]
    fn a_breakpoint_stop_is_not_an_exit() {
        let record = parse(r#"*stopped,reason="breakpoint-hit",bkptno="1""#);
        assert!(record.is_stopped());
        assert!(!record.is_program_exit());
        assert_eq!(record.exit_code(), None);
        assert_eq!(record.stop_reason(), Some("breakpoint-hit"));
    }

    #[test]
    fn a_signalled_exit_is_recognised() {
        let record = parse(
            r#"*stopped,reason="exited-signalled",signal-name="SIGSEGV",signal-meaning="Segmentation fault""#,
        );
        assert!(record.is_program_exit());
        assert_eq!(record.get_str("signal-name"), Some("SIGSEGV"));
    }

    #[test]
    fn a_signal_stop_is_a_pause_not_an_exit() {
        // A caught SIGSEGV stops the program without ending it, which is
        // exactly the case a debugger exists to inspect.
        let record = parse(
            r#"*stopped,reason="signal-received",signal-name="SIGSEGV",frame={addr="0x004000b4"}"#,
        );
        assert!(!record.is_program_exit());
        assert_eq!(record.stop_reason(), Some("signal-received"));
        assert_eq!(
            record.get("frame").and_then(|f| f.get_address("addr")),
            Some(0x0040_00b4)
        );
    }

    #[test]
    fn a_non_stop_record_has_no_stop_reason() {
        assert_eq!(parse("^done").stop_reason(), None);
        assert!(!parse("^done").is_program_exit());
    }

    #[test]
    fn result_class_names_round_trip() {
        for class in [
            ResultClass::Done,
            ResultClass::Running,
            ResultClass::Connected,
            ResultClass::Error,
            ResultClass::Exit,
        ] {
            assert_eq!(ResultClass::parse(class.as_str()), Some(class));
        }
        assert_eq!(ResultClass::parse("nonsense"), None);
    }
}

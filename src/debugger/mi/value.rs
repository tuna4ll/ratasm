//! The GDB/MI value grammar.
//!
//! MI values form a small recursive language:
//!
//! ```text
//! value  → const | tuple | list
//! const  → c-string
//! tuple  → "{}" | "{" result ("," result)* "}"
//! list   → "[]" | "[" value ("," value)* "]" | "[" result ("," result)* "]"
//! result → variable "=" value
//! ```
//!
//! Parsing this into typed values, rather than pattern-matching the raw text,
//! is what keeps the rest of the debugger honest. GDB embeds arbitrary program
//! text in these strings — a source line containing `,` or `"` or `\n` is
//! routine — and any approach based on splitting on delimiters corrupts it.
//!
//! # Lists with repeated keys
//!
//! MI lists come in two shapes, and the second has no equivalent in most data
//! formats: `stack=[frame={...},frame={...}]` is a list whose elements are
//! *results* that all share the key `frame`. A map would lose all but the last
//! one. Such elements are represented as single-entry tuples, and
//! [`Value::elements_named`] retrieves them by key.

use std::collections::BTreeMap;
use std::fmt;

/// A parsed MI value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A C-string constant, with escapes already decoded.
    String(String),
    /// A `{key=value,...}` tuple.
    Tuple(Vec<(String, Value)>),
    /// A `[...]` list.
    List(Vec<Value>),
}

impl Value {
    /// The string contents, when this is a constant.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(text) => Some(text),
            _ => None,
        }
    }

    /// The tuple entries, when this is a tuple.
    pub fn as_tuple(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Tuple(entries) => Some(entries),
            _ => None,
        }
    }

    /// The elements, when this is a list.
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(items) => Some(items),
            _ => None,
        }
    }

    /// Looks up `key` in a tuple, returning the first match.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_tuple()?
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// Looks up `key` and returns it as a string.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    /// Looks up `key` and parses it as a decimal integer.
    pub fn get_int(&self, key: &str) -> Option<i64> {
        self.get_str(key)?.trim().parse().ok()
    }

    /// Looks up `key` and parses it as an address.
    ///
    /// Accepts the `0x`-prefixed form GDB uses everywhere, and plain decimal.
    pub fn get_address(&self, key: &str) -> Option<u64> {
        parse_address(self.get_str(key)?)
    }

    /// The elements of a list that are single-entry tuples keyed by `name`.
    ///
    /// This is how repeated-key lists such as `stack=[frame={...},frame={...}]`
    /// are read back.
    pub fn elements_named(&self, name: &str) -> Vec<&Value> {
        let Some(items) = self.as_list() else {
            return Vec::new();
        };
        items
            .iter()
            .filter_map(|item| match item {
                Value::Tuple(entries) if entries.len() == 1 && entries[0].0 == name => {
                    Some(&entries[0].1)
                }
                _ => None,
            })
            .collect()
    }

    /// List elements, treating a repeated-key list as a plain list.
    ///
    /// GDB is inconsistent about which of the two list shapes it uses for the
    /// same conceptual data across versions, so callers that just want "the
    /// items" should use this rather than choosing a shape and being wrong on
    /// some GDB release.
    pub fn items(&self, name: &str) -> Vec<&Value> {
        let Some(items) = self.as_list() else {
            return Vec::new();
        };
        let named = self.elements_named(name);
        if named.len() == items.len() && !items.is_empty() {
            named
        } else {
            items.iter().collect()
        }
    }

    /// Flattens a tuple into a map, keeping the first value for repeated keys.
    pub fn to_map(&self) -> BTreeMap<String, Value> {
        let mut map = BTreeMap::new();
        for (key, value) in self.as_tuple().unwrap_or_default() {
            map.entry(key.clone()).or_insert_with(|| value.clone());
        }
        map
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::String(text) => f.write_str(text),
            Value::Tuple(entries) => {
                f.write_str("{")?;
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{key}={value}")?;
                }
                f.write_str("}")
            }
            Value::List(items) => {
                f.write_str("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
        }
    }
}

/// Parses an address in GDB's usual notation.
///
/// GDB writes addresses as `0x4000b0`, sometimes with a trailing symbolic
/// suffix such as `0x4000b0 <_start+4>`, which is discarded here.
pub fn parse_address(text: &str) -> Option<u64> {
    let text = text.trim();
    let text = text.split_whitespace().next()?;
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        text.parse::<u64>()
            .ok()
            .or_else(|| u64::from_str_radix(text, 16).ok())
    }
}

/// Errors from parsing MI text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The input ended in the middle of a value.
    #[error("unexpected end of input at byte {position}")]
    UnexpectedEnd {
        /// Where the input ran out.
        position: usize,
    },
    /// A character appeared where the grammar did not allow it.
    #[error("unexpected character '{found}' at byte {position}, expected {expected}")]
    Unexpected {
        /// The character that was found.
        found: char,
        /// What the grammar allowed there.
        expected: &'static str,
        /// Where it was found.
        position: usize,
    },
    /// Text remained after a complete value was parsed.
    #[error("trailing input at byte {position}")]
    TrailingInput {
        /// Where the unconsumed text starts.
        position: usize,
    },
}

/// A cursor over MI text.
pub struct Parser<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> Parser<'a> {
    /// Creates a parser over `input`.
    pub fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            position: 0,
        }
    }

    /// The current byte offset.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Whether all input has been consumed.
    pub fn is_at_end(&self) -> bool {
        self.position >= self.input.len()
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        Some(byte)
    }

    fn expect(&mut self, byte: u8, expected: &'static str) -> Result<(), ParseError> {
        match self.peek() {
            Some(found) if found == byte => {
                self.position += 1;
                Ok(())
            }
            Some(found) => Err(ParseError::Unexpected {
                found: found as char,
                expected,
                position: self.position,
            }),
            None => Err(ParseError::UnexpectedEnd {
                position: self.position,
            }),
        }
    }

    /// Parses one value.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when the text does not match the MI grammar.
    pub fn parse_value(&mut self) -> Result<Value, ParseError> {
        match self.peek() {
            Some(b'"') => self.parse_string().map(Value::String),
            Some(b'{') => self.parse_tuple(),
            Some(b'[') => self.parse_list(),
            Some(found) => Err(ParseError::Unexpected {
                found: found as char,
                expected: "a value",
                position: self.position,
            }),
            None => Err(ParseError::UnexpectedEnd {
                position: self.position,
            }),
        }
    }

    /// Parses a `variable=value` result.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when the text is not a well-formed result.
    pub fn parse_result(&mut self) -> Result<(String, Value), ParseError> {
        let name = self.parse_variable()?;
        self.expect(b'=', "'=' after a variable name")?;
        let value = self.parse_value()?;
        Ok((name, value))
    }

    /// Parses a comma-separated run of results to the end of the input.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when any result is malformed.
    pub fn parse_results(&mut self) -> Result<Vec<(String, Value)>, ParseError> {
        let mut results = Vec::new();
        if self.is_at_end() {
            return Ok(results);
        }
        loop {
            results.push(self.parse_result()?);
            if self.peek() == Some(b',') {
                self.position += 1;
            } else {
                break;
            }
        }
        Ok(results)
    }

    fn parse_variable(&mut self) -> Result<String, ParseError> {
        let start = self.position;
        while let Some(byte) = self.peek() {
            // A variable name runs up to the '='; GDB uses identifier
            // characters plus '-' and '_'.
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
                self.position += 1;
            } else {
                break;
            }
        }
        if start == self.position {
            return match self.peek() {
                Some(found) => Err(ParseError::Unexpected {
                    found: found as char,
                    expected: "a variable name",
                    position: self.position,
                }),
                None => Err(ParseError::UnexpectedEnd {
                    position: self.position,
                }),
            };
        }
        Ok(String::from_utf8_lossy(&self.input[start..self.position]).into_owned())
    }

    /// Parses a C-string, decoding escapes.
    fn parse_string(&mut self) -> Result<String, ParseError> {
        self.expect(b'"', "an opening quote")?;
        let mut out = Vec::new();

        loop {
            let byte = self.bump().ok_or(ParseError::UnexpectedEnd {
                position: self.position,
            })?;
            match byte {
                b'"' => break,
                b'\\' => {
                    let escape = self.bump().ok_or(ParseError::UnexpectedEnd {
                        position: self.position,
                    })?;
                    match escape {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'f' => out.push(0x0c),
                        b'b' => out.push(0x08),
                        b'a' => out.push(0x07),
                        b'v' => out.push(0x0b),
                        b'0'..=b'7' => {
                            // Octal escape: up to three digits including this
                            // one. GDB uses these for non-printable bytes in
                            // program output and memory contents.
                            let mut code = u32::from(escape - b'0');
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(digit @ b'0'..=b'7') => {
                                        code = code * 8 + u32::from(digit - b'0');
                                        self.position += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push((code & 0xff) as u8);
                        }
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }

        // GDB may emit bytes that are not valid UTF-8 when echoing program
        // memory; replacing them keeps the debugger usable instead of failing.
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    fn parse_tuple(&mut self) -> Result<Value, ParseError> {
        self.expect(b'{', "an opening brace")?;
        let mut entries = Vec::new();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(Value::Tuple(entries));
        }
        loop {
            entries.push(self.parse_result()?);
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b'}') => {
                    self.position += 1;
                    break;
                }
                Some(found) => {
                    return Err(ParseError::Unexpected {
                        found: found as char,
                        expected: "',' or '}'",
                        position: self.position,
                    })
                }
                None => {
                    return Err(ParseError::UnexpectedEnd {
                        position: self.position,
                    })
                }
            }
        }
        Ok(Value::Tuple(entries))
    }

    fn parse_list(&mut self) -> Result<Value, ParseError> {
        self.expect(b'[', "an opening bracket")?;
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(Value::List(items));
        }
        loop {
            // A list element is either a bare value or a `name=value` result;
            // the latter becomes a single-entry tuple.
            let item = if matches!(self.peek(), Some(b'"') | Some(b'{') | Some(b'[')) {
                self.parse_value()?
            } else {
                let (name, value) = self.parse_result()?;
                Value::Tuple(vec![(name, value)])
            };
            items.push(item);

            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    break;
                }
                Some(found) => {
                    return Err(ParseError::Unexpected {
                        found: found as char,
                        expected: "',' or ']'",
                        position: self.position,
                    })
                }
                None => {
                    return Err(ParseError::UnexpectedEnd {
                        position: self.position,
                    })
                }
            }
        }
        Ok(Value::List(items))
    }
}

/// Parses a complete list of results, requiring all input to be consumed.
///
/// # Errors
///
/// Returns [`ParseError`] when the text is malformed or has trailing content.
pub fn parse_results(text: &str) -> Result<Vec<(String, Value)>, ParseError> {
    let mut parser = Parser::new(text);
    let results = parser.parse_results()?;
    if !parser.is_at_end() {
        return Err(ParseError::TrailingInput {
            position: parser.position(),
        });
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> Value {
        let mut parser = Parser::new(text);
        let value = parser.parse_value().expect("parse");
        assert!(parser.is_at_end(), "input not fully consumed: {text}");
        value
    }

    #[test]
    fn parses_a_plain_string() {
        assert_eq!(value(r#""hello""#), Value::String("hello".to_owned()));
    }

    #[test]
    fn parses_an_empty_string() {
        assert_eq!(value(r#""""#), Value::String(String::new()));
    }

    #[test]
    fn decodes_standard_escapes() {
        assert_eq!(
            value(r#""line\none\ttab""#).as_str(),
            Some("line\none\ttab")
        );
        assert_eq!(value(r#""quote\"inside""#).as_str(), Some("quote\"inside"));
        assert_eq!(value(r#""back\\slash""#).as_str(), Some("back\\slash"));
    }

    #[test]
    fn decodes_octal_escapes() {
        // GDB writes non-printable bytes this way in memory dumps.
        assert_eq!(value(r#""\101\102\103""#).as_str(), Some("ABC"));
        assert_eq!(value(r#""\0""#).as_str(), Some("\0"));
    }

    #[test]
    fn a_string_containing_delimiters_is_not_split() {
        // The reason this is a parser and not a `split(',')`.
        let parsed = value(r#""mov rax, [rbx+8] ; a {tricky} \"line\"""#);
        assert_eq!(
            parsed.as_str(),
            Some(r#"mov rax, [rbx+8] ; a {tricky} "line""#)
        );
    }

    #[test]
    fn parses_an_empty_tuple_and_list() {
        assert_eq!(value("{}"), Value::Tuple(Vec::new()));
        assert_eq!(value("[]"), Value::List(Vec::new()));
    }

    #[test]
    fn parses_a_flat_tuple() {
        let parsed = value(r#"{number="1",addr="0x004000b0"}"#);
        assert_eq!(parsed.get_str("number"), Some("1"));
        assert_eq!(parsed.get_address("addr"), Some(0x0040_00b0));
    }

    #[test]
    fn parses_nested_structures() {
        let parsed = value(r#"{frame={level="0",func="main",args=[]}}"#);
        let frame = parsed.get("frame").expect("frame");
        assert_eq!(frame.get_str("func"), Some("main"));
        assert_eq!(frame.get("args"), Some(&Value::List(Vec::new())));
    }

    #[test]
    fn parses_a_list_of_values() {
        let parsed = value(r#"["a","b","c"]"#);
        let items = parsed.as_list().expect("list");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].as_str(), Some("a"));
    }

    #[test]
    fn parses_a_list_of_tuples() {
        let parsed = value(r#"[{name="rax",value="1"},{name="rbx",value="2"}]"#);
        let items = parsed.as_list().expect("list");
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].get_str("name"), Some("rbx"));
    }

    #[test]
    fn a_repeated_key_list_keeps_every_element() {
        // A map-based representation would keep only the last frame.
        let parsed = value(r#"[frame={level="0"},frame={level="1"},frame={level="2"}]"#);
        let frames = parsed.elements_named("frame");
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].get_str("level"), Some("0"));
        assert_eq!(frames[2].get_str("level"), Some("2"));
    }

    #[test]
    fn items_handles_both_list_shapes() {
        let repeated = value(r#"[frame={level="0"},frame={level="1"}]"#);
        assert_eq!(repeated.items("frame").len(), 2);

        let plain = value(r#"[{level="0"},{level="1"}]"#);
        assert_eq!(plain.items("frame").len(), 2);
        assert_eq!(plain.items("frame")[0].get_str("level"), Some("0"));
    }

    #[test]
    fn parses_a_real_stopped_event_payload() {
        // Copied from an actual GDB session.
        let text = r#"reason="breakpoint-hit",disp="keep",bkptno="1",frame={addr="0x00000000004000b0",func="_start",args=[],file="main.asm",fullname="/tmp/p/main.asm",line="9",arch="i386:x86-64"},thread-id="1",stopped-threads="all",core="3""#;
        let results = parse_results(text).expect("parse");
        let map: BTreeMap<_, _> = results.into_iter().collect();

        assert_eq!(map["reason"].as_str(), Some("breakpoint-hit"));
        assert_eq!(map["bkptno"].as_str(), Some("1"));
        let frame = &map["frame"];
        assert_eq!(frame.get_str("func"), Some("_start"));
        assert_eq!(frame.get_int("line"), Some(9));
        assert_eq!(frame.get_address("addr"), Some(0x0040_00b0));
        assert_eq!(frame.get_str("file"), Some("main.asm"));
    }

    #[test]
    fn parses_a_real_register_values_payload() {
        let text =
            r#"register-values=[{number="0",value="0x3c"},{number="4",value="0x7ffd8f2a1b40"}]"#;
        let results = parse_results(text).expect("parse");
        let values = &results[0].1;
        let items = values.as_list().expect("list");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get_address("value"), Some(0x3c));
        assert_eq!(items[1].get_address("value"), Some(0x7ffd_8f2a_1b40));
    }

    #[test]
    fn parses_an_empty_result_list() {
        assert_eq!(parse_results("").expect("parse"), Vec::new());
    }

    #[test]
    fn addresses_parse_in_every_form_gdb_uses() {
        assert_eq!(parse_address("0x4000b0"), Some(0x0040_00b0));
        assert_eq!(parse_address("0X4000B0"), Some(0x0040_00b0));
        assert_eq!(parse_address("  0x10  "), Some(16));
        assert_eq!(parse_address("0x4000b0 <_start+4>"), Some(0x0040_00b0));
        assert_eq!(parse_address("4194480"), Some(4_194_480));
        assert_eq!(parse_address("not an address"), None);
        assert_eq!(parse_address(""), None);
    }

    #[test]
    fn a_truncated_value_is_an_error_not_a_panic() {
        for text in [r#""unterminated"#, "{number=", "[", "{a=\"1\"", "[\"a\","] {
            let mut parser = Parser::new(text);
            assert!(parser.parse_value().is_err(), "{text:?} should not parse");
        }
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        for text in ["=", ",", "}", "]", "{=\"1\"}", "{1=\"x\"", "@"] {
            assert!(parse_results(text).is_err(), "{text:?} should not parse");
        }
    }

    #[test]
    fn trailing_input_is_rejected() {
        let error = parse_results(r#"a="1" leftover"#).expect_err("must fail");
        assert!(matches!(error, ParseError::TrailingInput { .. }));
    }

    #[test]
    fn errors_report_where_they_occurred() {
        let error = parse_results("a=@").expect_err("must fail");
        match error {
            ParseError::Unexpected { position, .. } => assert_eq!(position, 2),
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn invalid_utf8_bytes_are_replaced_rather_than_failing() {
        // GDB echoes raw program memory; it is not always valid UTF-8.
        let parsed = value(r#""\377\376""#);
        assert!(parsed.as_str().is_some(), "must still produce a string");
    }

    #[test]
    fn display_renders_a_readable_form_without_quotes() {
        // Display is for logs and the raw-protocol pane, so string contents
        // are shown as-is rather than re-escaped.
        let parsed = value(r#"{a="1",b=["x",{c="2"}]}"#);
        assert_eq!(parsed.to_string(), "{a=1,b=[x,{c=2}]}");
        assert_eq!(Value::String("plain".to_owned()).to_string(), "plain");
    }

    #[test]
    fn accessors_return_none_for_the_wrong_shape() {
        let string = Value::String("x".to_owned());
        assert!(string.get("anything").is_none());
        assert!(string.as_list().is_none());
        assert!(string.as_tuple().is_none());
        assert!(string.elements_named("frame").is_empty());

        let tuple = value(r#"{a="not a number"}"#);
        assert_eq!(tuple.get_int("a"), None);
        assert_eq!(tuple.get_int("missing"), None);
    }
}

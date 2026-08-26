//! Handing text to the terminal's clipboard with OSC 52.
//!
//! ratasm runs inside a terminal that may be on another machine over SSH, so
//! it cannot talk to an X or Wayland selection directly. OSC 52 is the escape
//! sequence terminals accept for "put this on the clipboard", and it travels
//! down the same connection the interface already uses.
//!
//! Reading the clipboard back is deliberately not attempted. The reply form of
//! OSC 52 is disabled by default in most terminals for good reason, and one
//! that ignores the query returns nothing at all rather than an error, so a
//! paste built on it would fail silently. Paste uses ratasm's own clipboard.

/// The largest payload sent to the terminal.
///
/// Terminals bound the length of an escape sequence, and a truncated one
/// leaves the clipboard holding half a selection. Refusing is better.
pub const MAX_BYTES: usize = 64 * 1024;

/// Builds the OSC 52 sequence that sets the clipboard to `text`.
///
/// Returns `None` when the text is longer than [`MAX_BYTES`], so the caller
/// can say why nothing happened.
pub fn set_sequence(text: &str) -> Option<String> {
    if text.len() > MAX_BYTES {
        return None;
    }
    Some(format!("\x1b]52;c;{}\x07", base64(text.as_bytes())))
}

/// Encodes bytes as standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).copied().map_or(0, u32::from);
        let b2 = chunk.get(2).copied().map_or(0, u32::from);
        let triple = (b0 << 16) | (b1 << 8) | b2;

        for index in 0..4 {
            if index <= chunk.len() {
                let shift = 18 - index * 6;
                out.push(char::from(ALPHABET[((triple >> shift) & 0x3f) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_matches_the_standard_vectors() {
        // From RFC 4648 section 10.
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn non_ascii_text_survives_the_encoding() {
        assert_eq!(base64("→".as_bytes()), "4oaS");
    }

    #[test]
    fn the_sequence_is_addressed_to_the_clipboard_selection() {
        let sequence = set_sequence("foo").expect("short enough");
        assert!(sequence.starts_with("\x1b]52;c;"));
        assert!(sequence.ends_with('\x07'));
        assert!(sequence.contains("Zm9v"));
    }

    #[test]
    fn an_oversized_payload_is_refused_rather_than_truncated() {
        let huge = "x".repeat(MAX_BYTES + 1);
        assert!(set_sequence(&huge).is_none());
        assert!(set_sequence(&huge[..MAX_BYTES]).is_some());
    }
}

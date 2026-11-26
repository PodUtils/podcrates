//! Text that came from a peer, made safe to print on a terminal.
//!
//! A relay's error body, a WebSocket close reason, a proxy's status line or an
//! SSH banner can carry ESC, BEL or a bare CR, which repaint the line, ring the
//! bell or overwrite what the user is reading. Everything a crate shows from a
//! peer goes through one of these two functions, so there is one definition of
//! "safe" (a sibling project ended up with two that disagreed). A field that
//! keeps such text takes a [`PeerText`], which is clean by its type.

/// For a one-line message: control characters (C0, DEL, C1) and bidirectional
/// formatting characters are dropped, and every run of whitespace, line breaks
/// included, becomes one space. Leading and trailing whitespace is removed.
pub fn one_line(text: &str) -> String {
    let text = &without_escapes(text);
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            space = true;
        } else if c.is_control() || is_bidi_control(c) {
            continue;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    out
}

/// For text that is meant to span lines (an SSH banner, a keyboard-interactive
/// instruction): CR LF and a lone CR become LF, tabs stay, and every other
/// control or bidirectional formatting character is dropped.
pub fn multi_line(text: &str) -> String {
    let text = &without_escapes(text);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\n' | '\t' => out.push(c),
            c if c.is_control() || is_bidi_control(c) => {}
            c => out.push(c),
        }
    }
    out
}

/// `text` without each ` (os error N)` that Rust's `io::Error` adds to the
/// system's words: a line says what happened once, in the system's words.
pub fn plain(text: &str) -> String {
    const MARK: &str = " (os error ";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(MARK) {
        let tail = &rest[at + MARK.len()..];
        let digits = tail.bytes().take_while(|b| b.is_ascii_digit() || *b == b'-').count();
        if digits > 0 && tail[digits..].starts_with(')') {
            out.push_str(&rest[..at]);
            rest = &tail[digits + 1..];
        } else {
            out.push_str(&rest[..at + 1]);
            rest = &rest[at + 1..];
        }
    }
    out.push_str(rest);
    out
}

/// What a user line shows of an `io::Error`: its words with no
/// ` (os error N)` ([`plain`]). The error stays the source of a failure;
/// only the line changes.
pub fn io_words(e: &std::io::Error) -> String {
    plain(&e.to_string())
}

/// `text` with each escape sequence of a terminal removed whole, so that
/// its parameters do not stay as text: a CSI (ESC `[`, or U+009B) up to its
/// final byte, an OSC (ESC `]`, or U+009D) up to BEL or ST (ESC `\`, or
/// U+009C), and another escape (ESC, its intermediates and its final
/// character). The controls that
/// stay are for [`one_line`] and [`multi_line`].
fn without_escapes(text: &str) -> String {
    if !text.contains(['\u{1b}', '\u{9b}', '\u{9d}']) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let csi = match c {
            '\u{1b}' => match chars.next() {
                Some('[') => true,
                Some(']') => false,
                // Another escape: ESC, its intermediates (0x20 to 0x2f), then
                // its final character.
                Some(c) if ('\u{20}'..='\u{2f}').contains(&c) => {
                    while chars.next_if(|c| ('\u{20}'..='\u{2f}').contains(c)).is_some() {}
                    chars.next();
                    continue;
                }
                _ => continue,
            },
            '\u{9b}' => true,
            '\u{9d}' => false,
            c => {
                out.push(c);
                continue;
            }
        };
        if csi {
            // The parameters and the intermediates, up to the final byte.
            for c in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    break;
                }
            }
        } else {
            while let Some(c) = chars.next() {
                if c == '\u{7}' || c == '\u{9c}' {
                    break;
                }
                if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                    chars.next();
                    break;
                }
            }
        }
    }
    out
}

/// The most characters of a [`PeerText`]: a peer that sends a page of
/// text in an error field fills no screen.
pub const PEER_TEXT_MOST: usize = 512;

/// Text of a peer, cleaned and cut when it is made: a field of this type
/// cannot hold text that is not safe to print. It has no `Deref` to `str`
/// and no `From<String>`, so raw text does not go in by a slip. It goes out
/// as a `String`:
///
/// ```
/// let text: String = pod_core::text::PeerText::one_line("bad\x1b[2J token").into();
/// assert_eq!(text, "bad token");
/// ```
///
/// ```compile_fail
/// let raw = String::from("\x1b[2J");
/// let field: pod_core::text::PeerText = raw.into();
/// ```
///
/// ```compile_fail
/// let field = pod_core::text::PeerText::one_line("ok");
/// let shown: &str = &field;
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct PeerText(String);

impl PeerText {
    /// One line of a peer ([`one_line`]), cut to [`PEER_TEXT_MOST`].
    pub fn one_line(text: &str) -> PeerText {
        PeerText(cut(one_line(text)))
    }

    /// Lines of a peer ([`multi_line`]), cut to [`PEER_TEXT_MOST`].
    pub fn multi_line(text: &str) -> PeerText {
        PeerText(cut(multi_line(text)))
    }

    /// The clean text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for PeerText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Clean text goes where a `String` goes (the text of a `Failure`).
impl From<PeerText> for String {
    fn from(text: PeerText) -> String {
        text.0
    }
}

/// `text` cut to [`PEER_TEXT_MOST`] characters; a cut ends with "...".
fn cut(mut text: String) -> String {
    if let Some((at, _)) = text.char_indices().nth(PEER_TEXT_MOST) {
        let keep = text[..at].char_indices().nth(PEER_TEXT_MOST - 3).map_or(at, |(i, _)| i);
        text.truncate(keep);
        text.push_str("...");
    }
    text
}

/// `text` in at most `max` bytes: a cut ends with "..." on a character
/// boundary, so a line of an error keeps its bound.
pub fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut at = max.saturating_sub(3);
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    format!("{}...", &text[..at])
}

/// One line of the body of a peer for the text of an error: the value of
/// each JSON key of `secret` is `<redacted>`, the text is safe for a
/// terminal ([`one_line`]), and it has at most `max` bytes ([`clip`]). A
/// proxy or a gateway on the way can answer with its own page: its words
/// tell the user what broke, and the credentials of a true answer stay out.
pub fn body_line(body: &str, secret: &[&str], max: usize) -> String {
    clip(&one_line(&redacted(body, secret)), max)
}

/// `body` with the string value of each key of `secret` replaced by a
/// marker. A value with no closing quote ends the text: what follows it can
/// be the secret.
fn redacted(body: &str, secret: &[&str]) -> String {
    const REDACTED: &str = "<redacted>";
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        let Some(key) = secret.iter().find(|key| {
            body[i..].strip_prefix('"').and_then(|r| r.strip_prefix(**key)).is_some_and(|r| r.starts_with('"'))
        }) else {
            let c = body[i..].chars().next().unwrap_or_default();
            out.push(c);
            i += c.len_utf8().max(1);
            continue;
        };
        let after_key = i + key.len() + 2;
        let Some(colon) = body[after_key..].find(':') else { break };
        let after_colon = after_key + colon + 1;
        let Some(open) = body[after_colon..].find('"').map(|p| after_colon + p + 1) else { break };
        let Some(close) = body[open..].find('"').map(|p| open + p) else { break };
        out.push_str(&body[i..open]);
        out.push_str(REDACTED);
        i = close;
    }
    out
}

/// Characters that reorder how a terminal displays the text around them
/// (the "Trojan Source" set), plus the Arabic letter mark.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(one_line("403 missing or wrong token"), "403 missing or wrong token");
        assert_eq!(multi_line("Welcome.\nNo shell here.\n"), "Welcome.\nNo shell here.\n");
        assert_eq!(one_line("naïve — ok"), "naïve — ok");
    }

    #[test]
    fn terminal_controls_are_removed() {
        // ESC sequences, BEL, a bare CR that would overwrite the line, and the
        // 8-bit CSI (U+009B). The CR is whitespace, so one line shows a space.
        let hostile = "ok\x1b[2J\x1b]0;title\x07\rFAKE\u{9b}31m";
        assert_eq!(one_line(hostile), "ok FAKE", "each sequence goes whole");
        assert!(!multi_line(hostile).contains('\x1b'));
        assert!(!multi_line(hostile).contains('\x07'));
        assert!(!multi_line(hostile).contains('\u{9b}'));
    }

    /// The sequences of the three copies that this rule replaces
    /// (`pod_cloudflare::text::clean`, podnet's `report::clean`): a colour, a
    /// title ended by ST, a 7-bit and an 8-bit form, and an ESC at the end.
    #[test]
    fn each_escape_sequence_goes_with_its_parameters() {
        assert_eq!(multi_line("\x1b[1;31mred\x1b[0m"), "red");
        assert_eq!(multi_line("a\x1b]8;;https://x.test\x1b\\link\x1b]8;;\x1b\\b"), "alinkb");
        assert_eq!(multi_line("a\u{9d}0;title\u{9c}b\u{9b}2Kc"), "abc");
        assert_eq!(multi_line("a\x1b(Bb\x1b"), "ab", "ESC, an intermediate and a final; a last ESC");
        assert_eq!(multi_line("a\x1b7b\x1bMc"), "abc", "ESC and one character");
        assert_eq!(one_line("x\x1b[999999999999999999m"), "x", "long parameters");
        assert_eq!(multi_line("\x1b["), "", "a sequence that the text cuts");
    }

    #[test]
    fn bidirectional_overrides_are_removed() {
        assert_eq!(one_line("abc\u{202E}fed\u{202C}"), "abcfed");
        assert_eq!(multi_line("a\u{2066}b\u{2069}c"), "abc");
    }

    #[test]
    fn one_line_folds_whitespace_and_line_breaks() {
        assert_eq!(one_line("  a \r\n\t b\n\nc  "), "a b c");
        assert_eq!(one_line("\n\n"), "");
    }

    #[test]
    fn peer_text_is_clean_and_cut() {
        let long = format!("\x1b[2J{}", "é".repeat(2 * PEER_TEXT_MOST));
        let text = PeerText::one_line(&long);
        assert_eq!(text.as_str().chars().count(), PEER_TEXT_MOST);
        assert!(text.as_str().ends_with("...") && !text.as_str().contains('\x1b'));
        let short = PeerText::multi_line("a\r\nb\x07");
        assert_eq!((short.to_string(), String::from(short)), ("a\nb".into(), "a\nb".into()));
        assert_eq!(PeerText::one_line(&"x".repeat(PEER_TEXT_MOST)).as_str().len(), PEER_TEXT_MOST);
    }

    /// A cut keeps its bound in bytes and lands on a character boundary.
    #[test]
    fn a_clip_keeps_its_bound_on_a_character_boundary() {
        assert_eq!(clip("short", 10), "short");
        let out = clip(&"é".repeat(400), 200);
        assert!(out.len() <= 200 && out.ends_with("..."), "{out}");
        assert_eq!(clip("abcdef", 5), "ab...");
    }

    /// The line of a body: a page of a stand-in as it is, the values of
    /// the secret keys replaced, no control character, a bound.
    #[test]
    fn the_line_of_a_body_hides_its_secrets_and_keeps_its_words() {
        assert_eq!(body_line("pod-lab stand-in\n", &["token"], 200), "pod-lab stand-in");
        let json = r#"{"id":"dev-1","token":"t0ps3cret","name":"x"}"#;
        let line = body_line(json, &["token", "id"], 200);
        assert!(!line.contains("t0ps3cret") && !line.contains("dev-1"), "{line}");
        assert!(line.contains(r#""token":"<redacted>""#) && line.contains(r#""name":"x""#), "{line}");
        let cut = body_line(r#"{"token":"unterminated secret"#, &["token"], 200);
        assert!(!cut.contains("unterminated"), "{cut}");
        assert_eq!(body_line("a\x1b[2Jb", &[], 200), "ab");
        assert!(body_line(&"x".repeat(500), &[], 100).len() <= 100);
        for odd in ["", "{", "\"token\":", r#"{"token":"a","token":"b"}"#, "é\"token\"é"] {
            let _ = body_line(odd, &["token"], 50);
        }
    }

    #[test]
    fn multi_line_normalises_line_breaks() {
        assert_eq!(multi_line("a\r\nb\rc\nd\te"), "a\nb\nc\nd\te");
    }

    #[test]
    fn io_words_drop_the_code_of_the_system_and_keep_its_words() {
        use std::io::{Error, ErrorKind};
        let table = [
            (Error::from_raw_os_error(2), "No such file or directory"),
            (Error::from_raw_os_error(104), "Connection reset by peer"),
            (Error::from(ErrorKind::NotFound), "entity not found"),
            (Error::new(ErrorKind::Other, "the pair is stale"), "the pair is stale"),
            (Error::new(ErrorKind::Other, "kept (os error x) text"), "kept (os error x) text"),
        ];
        for (e, want) in table {
            assert_eq!(io_words(&e), want, "{e:?}");
        }
    }

    #[test]
    fn plain_drops_each_code_in_the_text_and_no_other_bracket() {
        assert_eq!(plain("a (os error 2): b (os error 104) c"), "a: b c");
        assert_eq!(plain("x (os error -1)"), "x");
        assert_eq!(plain("x (os error )"), "x (os error )");
        assert_eq!(plain("x (other 2)"), "x (other 2)");
    }
}

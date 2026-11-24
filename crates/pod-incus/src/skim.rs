//! A JSON text too large to hold, made small as it comes: each string keeps
//! its first bytes and drops the rest, so that serde reads the result. The
//! error of incusd on a full storage pool is the stderr of `tar`, one line
//! for each file: 100 MB in one `error` field, whose start says why. The
//! skim also notes the `type` of the envelope and where its `error` starts,
//! so that a start of an answer gives the words of incus up to a cut.

/// The bytes of each string that a skim keeps.
pub(crate) const KEPT_TEXT: usize = 64 * 1024;

/// An escape that has its backslash and no next byte.
const ESCAPE_START: u8 = u8::MAX;

/// A skim of one JSON text.
#[derive(Debug)]
pub(crate) struct Skim {
    out: Vec<u8>,
    keep: usize,
    /// The open containers: true for an object, false for an array.
    open: Vec<bool>,
    /// In an object, the next string is a key. A key is never cut.
    expect_key: bool,
    in_string: bool,
    is_key: bool,
    /// The bytes of an escape that are still to come.
    escape: u8,
    kept: usize,
    dropped: u64,
    cutting: bool,
    /// The offset in `out` of the text of the current string.
    start: usize,
    /// The last key of the top level.
    key: Vec<u8>,
    top_type: Option<String>,
    /// The text of the top-level `error` in `out`: its start, and its end
    /// once the string closed.
    error: Option<(usize, Option<usize>)>,
}

impl Skim {
    /// A skim that keeps `keep` bytes of each string.
    pub(crate) fn new(keep: usize) -> Skim {
        Skim {
            out: Vec::new(),
            keep,
            open: Vec::new(),
            expect_key: false,
            in_string: false,
            is_key: false,
            escape: 0,
            kept: 0,
            dropped: 0,
            cutting: false,
            start: 0,
            key: Vec::new(),
            top_type: None,
            error: None,
        }
    }

    /// The bytes of the skimmed text so far.
    pub(crate) fn len(&self) -> usize {
        self.out.len()
    }

    /// The `type` of the envelope, once its string closed.
    pub(crate) fn top_type(&self) -> Option<&str> {
        self.top_type.as_deref()
    }

    /// The text can still be an envelope of type `error`.
    pub(crate) fn may_be_error(&self) -> bool {
        self.top_type().is_none_or(|kind| kind == "error")
    }

    /// The skimmed JSON text.
    pub(crate) fn finish(self) -> Vec<u8> {
        self.out
    }

    /// The top-level `error`, unescaped, as far as the text holds it: an
    /// escape or a character that the end cuts goes.
    pub(crate) fn partial_error(&self) -> Option<String> {
        let (start, end) = self.error?;
        let raw = &self.out[start..end.unwrap_or(self.out.len())];
        // An escape is 12 bytes at most (a surrogate pair).
        (0..=raw.len().min(12)).find_map(|cut| {
            let mut quoted = Vec::with_capacity(raw.len() + 2);
            quoted.push(b'"');
            quoted.extend_from_slice(&raw[..raw.len() - cut]);
            quoted.push(b'"');
            serde_json::from_slice::<String>(&quoted).ok()
        })
    }

    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.in_string {
                self.string_byte(byte);
            } else {
                self.structure_byte(byte);
            }
        }
    }

    fn structure_byte(&mut self, byte: u8) {
        match byte {
            b'"' => {
                self.in_string = true;
                self.is_key = self.expect_key;
                (self.escape, self.kept, self.dropped, self.cutting) = (0, 0, 0, false);
                self.out.push(byte);
                self.start = self.out.len();
                if !self.is_key && self.at_top() && self.key == b"error" {
                    self.error = Some((self.start, None));
                }
            }
            b'{' | b'[' => {
                self.open.push(byte == b'{');
                self.expect_key = byte == b'{';
                self.out.push(byte);
            }
            b'}' | b']' => {
                self.open.pop();
                self.expect_key = false;
                self.out.push(byte);
            }
            b',' => {
                self.expect_key = self.open.last() == Some(&true);
                self.out.push(byte);
            }
            b' ' | b'\t' | b'\r' | b'\n' => {}
            _ => self.out.push(byte),
        }
    }

    fn string_byte(&mut self, byte: u8) {
        if self.escape > 0 {
            self.escape = if self.escape == ESCAPE_START && byte == b'u' {
                4
            } else if self.escape == ESCAPE_START {
                0
            } else {
                self.escape - 1
            };
            return self.take(byte);
        }
        if byte == b'"' {
            return self.close();
        }
        // A cut falls before a character or an escape, never in one.
        let continues = byte & 0xC0 == 0x80;
        if !self.cutting && !continues && !self.is_key && self.kept >= self.keep {
            self.cutting = true;
        }
        if byte == b'\\' {
            self.escape = ESCAPE_START;
        }
        self.take(byte);
    }

    /// In the value of a key of the top-level object.
    fn at_top(&self) -> bool {
        self.open.len() == 1 && self.open[0]
    }

    fn take(&mut self, byte: u8) {
        if self.cutting {
            self.dropped += 1;
        } else {
            self.out.push(byte);
            self.kept += 1;
        }
    }

    fn close(&mut self) {
        self.in_string = false;
        if self.cutting {
            // A high surrogate whose low half went is not JSON.
            let tail = &self.out[self.start..];
            if tail.len() >= 6 && is_high_surrogate(&tail[tail.len() - 6..]) {
                self.out.truncate(self.out.len() - 6);
                self.dropped += 6;
            }
            self.out.extend_from_slice(format!("… ({} bytes more)", self.dropped).as_bytes());
        }
        let end = self.out.len();
        self.out.push(b'"');
        if self.is_key {
            if self.open.len() == 1 {
                self.key = self.out[self.start..end].to_vec();
            }
            self.expect_key = false;
        } else if self.at_top() {
            if self.key == b"type" {
                self.top_type = serde_json::from_slice::<String>(&self.out[self.start - 1..=end]).ok();
            }
            if let Some((start, None)) = self.error {
                self.error = Some((start, Some(end)));
            }
        }
    }
}

/// `\uD800` to `\uDBFF`.
fn is_high_surrogate(escape: &[u8]) -> bool {
    escape[0] == b'\\'
        && escape[1] == b'u'
        && escape[2].eq_ignore_ascii_case(&b'd')
        && matches!(escape[3].to_ascii_lowercase(), b'8'..=b'9' | b'a'..=b'b')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skim(text: &[u8], keep: usize, piece: usize) -> Skim {
        let mut skim = Skim::new(keep);
        for part in text.chunks(piece) {
            skim.feed(part);
        }
        skim
    }

    #[test]
    fn a_long_string_keeps_its_start_and_the_rest_stays_json() {
        let error = format!("Unpack failed: {}", "tar: rootfs/a: Cannot open: No space left on device\n".repeat(1000));
        let text =
            serde_json::json!({"type": "error", "error_code": 500, "error": error, "metadata": null}).to_string();
        for piece in [1, 7, 4096] {
            let skim = skim(text.as_bytes(), 100, piece);
            assert_eq!(skim.top_type(), Some("error"));
            let value: serde_json::Value = serde_json::from_slice(&skim.finish()).expect("JSON");
            let kept = value["error"].as_str().unwrap();
            assert!(kept.starts_with(&error[..90]), "{kept}");
            assert!(kept.contains("… (") && kept.ends_with(" bytes more)"), "{kept}");
            assert_eq!(value["error_code"], 500);
        }
    }

    #[test]
    fn a_cut_falls_before_an_escape_or_a_character() {
        // Each cut point in an escape, a pair of surrogates and a UTF-8
        // character gives JSON that serde reads.
        let text = r#"{"type":"error","error":"ab\"\n\u00e9\ud83d\ude00\u0447x"}"#;
        for keep in 0..30 {
            let skim = skim(text.as_bytes(), keep, 1);
            let value: serde_json::Value =
                serde_json::from_slice(&skim.finish()).unwrap_or_else(|e| panic!("keep {keep}: {e}"));
            assert!(value["error"].is_string(), "keep {keep}");
        }
    }

    #[test]
    fn the_start_of_an_envelope_gives_the_error_up_to_the_cut() {
        let text =
            r#"{"type":"error","status":"","error_code":500,"error":"Failed: tar: a\nCannot open: No sp\u00e9ace"#;
        // The whole start, a cut in the escape of a character, a cut in the escape of a line end.
        let in_line_end = text.find("a\\nC").unwrap() + 2;
        for end in [text.len(), text.len() - 6, in_line_end] {
            let skim = skim(&text.as_bytes()[..end], KEPT_TEXT, 5);
            assert_eq!(skim.top_type(), Some("error"));
            let partial = skim.partial_error().expect("the error so far");
            let want = if end == in_line_end { "Failed: tar: a" } else { "Failed: tar: a\nCannot open: No sp" };
            assert!(partial.starts_with(want), "{partial:?} at {end}");
        }
        let sync = skim(br#"{"type":"sync","metadata":{"error":"x"}}"#, 10, 3);
        assert!(!sync.may_be_error());
        assert_eq!(sync.partial_error(), None, "an error that is not of the top level");
    }
}

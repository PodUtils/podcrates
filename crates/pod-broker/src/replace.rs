//! The replacement of texts in a stream of bytes: the swap of a placeholder
//! on the way out and the scrub of a secret on the way back. A text that
//! two pieces of the stream split is found: the end of a piece that can
//! start a text waits for the next piece.

/// Each pair `(from, to)`, in a stream: each `from` becomes its `to`, the
/// longest first where two start at one byte.
#[derive(Debug, Clone, Default)]
pub struct Replace {
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
    carry: Vec<u8>,
    count: u64,
}

impl Replace {
    /// The pairs with an empty `from` are left out.
    pub fn new(pairs: impl IntoIterator<Item = (Vec<u8>, Vec<u8>)>) -> Replace {
        let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = pairs.into_iter().filter(|(from, _)| !from.is_empty()).collect();
        pairs.sort_by_key(|p| std::cmp::Reverse(p.0.len()));
        Replace { pairs, carry: Vec::new(), count: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// The replacements so far.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// The next piece of the stream: the bytes that are sure. A part of a
    /// text at the end waits in the carry.
    pub fn push(&mut self, piece: &[u8]) -> Vec<u8> {
        if self.pairs.is_empty() {
            return piece.to_vec();
        }
        let mut data = std::mem::take(&mut self.carry);
        data.extend_from_slice(piece);
        self.run(&data, false)
    }

    /// The end of the stream: the bytes of the carry.
    pub fn finish(&mut self) -> Vec<u8> {
        let data = std::mem::take(&mut self.carry);
        self.run(&data, true)
    }

    /// Each pair in `data`, all at once.
    pub fn all(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = self.push(data);
        out.extend(self.finish());
        out
    }

    fn run(&mut self, data: &[u8], last: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        let mut i = 0;
        'bytes: while i < data.len() {
            let rest = &data[i..];
            for (from, to) in &self.pairs {
                if rest.starts_with(from) {
                    out.extend_from_slice(to);
                    self.count += 1;
                    i += from.len();
                    continue 'bytes;
                }
                if !last && rest.len() < from.len() && from.starts_with(rest) {
                    self.carry = rest.to_vec();
                    return out;
                }
            }
            out.push(data[i]);
            i += 1;
        }
        out
    }
}

impl Drop for Replace {
    // The pairs hold the values of secrets, and the carry a part of one.
    fn drop(&mut self) {
        use zeroize::Zeroize as _;
        for (from, to) in &mut self.pairs {
            from.zeroize();
            to.zeroize();
        }
        self.carry.zeroize();
    }
}

/// `text` with each pair replaced, and the count.
pub fn replace_text(pairs: &[(Vec<u8>, Vec<u8>)], text: &str) -> (String, u64) {
    let mut replace = Replace::new(pairs.iter().cloned());
    if replace.is_empty() {
        return (text.to_string(), 0);
    }
    let out = replace.all(text.as_bytes());
    (String::from_utf8_lossy(&out).into_owned(), replace.count())
}

/// `value` percent-encoded: each byte but `A-Z a-z 0-9 - . _ ~` as `%XX`.
pub fn percent(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `value` as a string of JSON writes it, with no quotes.
pub fn json_escaped(value: &str) -> String {
    let quoted = serde_json::to_string(value).unwrap_or_default();
    quoted.get(1..quoted.len().saturating_sub(1)).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(Vec<u8>, Vec<u8>)> {
        list.iter().map(|(a, b)| (a.as_bytes().to_vec(), b.as_bytes().to_vec())).collect()
    }

    #[test]
    fn a_text_split_over_two_pieces_is_found() {
        for cut in 0..=22 {
            let text = b"xx abcdefgh-secret yy z";
            let mut r = Replace::new(pairs(&[("abcdefgh-secret", "PH")]));
            let mut out = r.push(&text[..cut]);
            out.extend(r.push(&text[cut..]));
            out.extend(r.finish());
            assert_eq!(String::from_utf8(out).unwrap(), "xx PH yy z", "cut at {cut}");
            assert_eq!(r.count(), 1);
        }
    }

    #[test]
    fn a_start_of_a_text_at_the_end_comes_out_at_the_finish() {
        let mut r = Replace::new(pairs(&[("secret", "PH")]));
        assert_eq!(r.push(b"a secr"), b"a ");
        assert_eq!(r.finish(), b"secr");
        assert_eq!(r.count(), 0);
    }

    #[test]
    fn the_longest_text_wins_and_the_forms_are_written() {
        let (out, n) = replace_text(&pairs(&[("ab", "1"), ("abc", "2")]), "abcab");
        assert_eq!((out.as_str(), n), ("21", 2));
        assert_eq!(percent("a b/+~"), "a%20b%2F%2B~");
        assert_eq!(json_escaped(r#"a"b\c/"#), r#"a\"b\\c/"#);
    }
}

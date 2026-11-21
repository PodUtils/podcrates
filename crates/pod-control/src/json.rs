//! The reader of one JSON text of the wire into a [`Value`]. A line can
//! hold a token, so the reader leaves no copy of a string that it does not
//! wipe:
//!
//! * A string with an escape is decoded into a buffer of its own, of the
//!   length of its text in the line: the decoded string is not longer, so
//!   the buffer does not grow, and the buffer is the string of the value.
//!   `serde_json` decodes it into a scratch buffer that it frees with no
//!   wipe.
//! * A part of a value that fails to read is wiped at its drop, and so is
//!   a value that a later key of the same name replaces (the last value is
//!   kept, as in `serde_json`). `serde_json` drops each of them with no
//!   wipe.
//!
//! The grammar is the one of `serde_json`: RFC 8259, the whitespace
//! `' '`, `\t`, `\n` and `\r`, a surrogate pair for each code point past
//! U+FFFF and no lone surrogate, and at most 127 arrays and objects in each
//! other. `serde_json` reads each number from its text, so a number has the
//! value that `serde_json` gives. The fuzz target `control_line` holds the
//! reader to `serde_json`: the same value for each text, or a refusal of
//! both.

use serde_json::{Map, Value};
use zeroize::Zeroize;

use crate::wire::Wiped;

/// `serde_json` refuses the 128th array or object in each other.
const DEPTH: usize = 127;

/// The value of `text`, or `None` when `text` is not one JSON value. The
/// caller wipes the value ([`crate::wire::wipe`]).
pub fn read(text: &str) -> Option<Value> {
    let mut reader = Reader { text, at: 0 };
    let value = Wiped(reader.value(0)?);
    reader.space();
    (reader.at == text.len()).then(|| value.take())
}

struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    /// Step over `byte` after the whitespace before it.
    fn eat(&mut self, byte: u8) -> bool {
        self.space();
        let found = self.peek() == Some(byte);
        self.at += usize::from(found);
        found
    }

    fn word(&mut self, word: &str, value: Value) -> Option<Value> {
        self.text[self.at..].starts_with(word).then(|| {
            self.at += word.len();
            value
        })
    }

    /// The value at `self.at`, inside `depth` arrays and objects.
    fn value(&mut self, depth: usize) -> Option<Value> {
        self.space();
        match self.peek()? {
            b'n' => self.word("null", Value::Null),
            b't' => self.word("true", Value::Bool(true)),
            b'f' => self.word("false", Value::Bool(false)),
            b'"' => self.string().map(Value::String),
            b'[' if depth < DEPTH => self.array(depth + 1),
            b'{' if depth < DEPTH => self.object(depth + 1),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn array(&mut self, depth: usize) -> Option<Value> {
        self.at += 1;
        let mut items = Wiped(Value::Array(Vec::new()));
        if self.eat(b']') {
            return Some(items.take());
        }
        loop {
            let item = self.value(depth)?;
            if let Value::Array(list) = &mut items.0 {
                list.push(item);
            }
            if self.eat(b']') {
                return Some(items.take());
            }
            if !self.eat(b',') {
                return None;
            }
        }
    }

    fn object(&mut self, depth: usize) -> Option<Value> {
        self.at += 1;
        let mut fields = Wiped(Value::Object(Map::new()));
        if self.eat(b'}') {
            return Some(fields.take());
        }
        loop {
            if !self.eat(b'"') {
                return None;
            }
            self.at -= 1;
            let mut key = Wiped(Value::String(self.string()?));
            if !self.eat(b':') {
                return None;
            }
            let item = self.value(depth)?;
            if let (Value::Object(map), Value::String(key)) = (&mut fields.0, &mut key.0) {
                match map.get_mut(key.as_str()) {
                    Some(old) => drop(Wiped(std::mem::replace(old, item))),
                    None => drop(map.insert(std::mem::take(key), item)),
                }
            }
            if self.eat(b'}') {
                return Some(fields.take());
            }
            if !self.eat(b',') {
                return None;
            }
        }
    }

    /// The string at `self.at`, at its quote.
    fn string(&mut self) -> Option<String> {
        let bytes = self.text.as_bytes();
        let start = self.at + 1;
        let mut end = start;
        let mut escaped = false;
        loop {
            match *bytes.get(end)? {
                b'"' => break,
                b'\\' => {
                    escaped = true;
                    end += 2;
                }
                0..=0x1f => return None,
                _ => end += 1,
            }
        }
        self.at = end + 1;
        let raw = &self.text[start..end];
        if !escaped {
            return Some(raw.to_string());
        }
        // Each escape gives fewer bytes than its text: the string does not
        // grow past this capacity, so no old buffer is freed.
        let mut out = String::with_capacity(raw.len());
        if decode(raw, &mut out).is_none() {
            out.zeroize();
            return None;
        }
        Some(out)
    }

    /// The number at `self.at`, read by `serde_json` from its text. A
    /// number is not a token: with the feature `float_roundtrip`,
    /// `serde_json` keeps its digits in a scratch buffer with no wipe.
    fn number(&mut self) -> Option<Value> {
        let bytes = self.text.as_bytes();
        let start = self.at;
        let digits = |at: &mut usize| {
            let from = *at;
            while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
                *at += 1;
            }
            *at > from
        };
        let mut at = start + usize::from(bytes[start] == b'-');
        match bytes.get(at)? {
            b'0' => at += 1,
            b'1'..=b'9' => _ = digits(&mut at),
            _ => return None,
        }
        if bytes.get(at) == Some(&b'.') {
            at += 1;
            digits(&mut at).then_some(())?;
        }
        if matches!(bytes.get(at), Some(b'e' | b'E')) {
            at += 1;
            at += usize::from(matches!(bytes.get(at), Some(b'+' | b'-')));
            digits(&mut at).then_some(())?;
        }
        self.at = at;
        serde_json::from_str::<serde_json::Number>(&self.text[start..at]).ok().map(Value::Number)
    }
}

/// Decode the text of a string with escapes into `out`.
fn decode(raw: &str, out: &mut String) -> Option<()> {
    let bytes = raw.as_bytes();
    let mut i = 0;
    let mut run = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        out.push_str(&raw[run..i]);
        let ch = match *bytes.get(i + 1)? {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let (ch, len) = unicode(&bytes[i + 2..])?;
                i += len;
                ch
            }
            _ => return None,
        };
        out.push(ch);
        i += 2;
        run = i;
    }
    out.push_str(&raw[run..]);
    Some(())
}

/// The code point of `XXXX` or of a pair `XXXX\uYYYY` at the start of
/// `bytes`, and the count of bytes that it takes.
fn unicode(bytes: &[u8]) -> Option<(char, usize)> {
    let hex = |at: usize| -> Option<u32> {
        let digits = std::str::from_utf8(bytes.get(at..at + 4)?).ok()?;
        digits.bytes().all(|b| b.is_ascii_hexdigit()).then(|| u32::from_str_radix(digits, 16).ok())?
    };
    let first = hex(0)?;
    if !(0xd800..=0xdbff).contains(&first) {
        return char::from_u32(first).map(|c| (c, 4));
    }
    if bytes.get(4..6)? != b"\\u" {
        return None;
    }
    let second = hex(6)?;
    if !(0xdc00..=0xdfff).contains(&second) {
        return None;
    }
    char::from_u32(0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00)).map(|c| (c, 10))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The texts of the grammar, each read by the reader and by `serde_json`.
    const TEXTS: &[&str] = &[
        "null",
        "true",
        "false",
        " \t\r\n1 ",
        "nul",
        "truex",
        "",
        " ",
        "x",
        "0",
        "-0",
        "-0.0",
        "1.5e3",
        "1E+2",
        "2e-2",
        "01",
        "-",
        "1.",
        ".5",
        "1e",
        "1e+",
        "+1",
        "0x1",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775808",
        "-9223372036854775809",
        "1e308",
        "1e309",
        "-1e400",
        "1e-400",
        "0.1000000000000000055511151231257827",
        "123456789012345678901234567890e-10",
        "3.141592653589793238462643383279",
        r#""""#,
        r#""a""#,
        r#""\n\"\\\/\b\f\r\t""#,
        r#""\u0041\u00e9\u20ac""#,
        r#""\ud83d\ude00""#,
        r#""\ud83d""#,
        r#""\ude00""#,
        r#""\ud83d\u0041""#,
        r#""\ud83dx""#,
        r#""\ud83d\n""#,
        r#""\u004""#,
        r#""\u+041""#,
        r#""\x""#,
        r#""\"#,
        r#""a"#,
        "\"\u{1}\"",
        "\"\u{7f}\u{e9}\u{10348}\"",
        r#""\u0000""#,
        r#""a\u0000b\ud800\udc00c""#,
        "\"\\\u{e9}\"",
        "[]",
        "[ ]",
        "[1,2]",
        "[1,]",
        "[,1]",
        "[1 2]",
        "[",
        "]",
        "[[]]",
        "[1]]",
        "{}",
        "{ }",
        r#"{"a":1}"#,
        r#"{"a":1,}"#,
        r#"{"a"}"#,
        r#"{"a":}"#,
        r#"{a:1}"#,
        r#"{"a":1"#,
        r#"{"a":1,"a":2}"#,
        r#"{"a":1,"b":[true,null,{"c":"\n"}],"a":{"d":2}}"#,
        r#"{"\n":1,"\u000a":2}"#,
        r#"{"schema":1,"id":7,"method":"version","params":{"t":"x\n"}}"#,
        "{1:2}",
        r#"{"a":1 "b":2}"#,
    ];

    #[test]
    fn each_text_reads_as_serde_json_reads_it() {
        for text in TEXTS {
            assert_eq!(read(text), serde_json::from_str::<Value>(text).ok(), "{text:?}");
        }
    }

    #[test]
    fn the_depth_of_arrays_and_objects_is_the_one_of_serde_json() {
        for depth in [126, 127, 128, 129] {
            let arrays = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
            let objects = format!("{}1{}", r#"{"a":"#.repeat(depth), "}".repeat(depth));
            for text in [arrays, objects] {
                assert_eq!(read(&text), serde_json::from_str::<Value>(&text).ok(), "depth {depth}");
                assert_eq!(read(&text).is_some(), depth <= DEPTH, "depth {depth}");
            }
        }
    }

    #[test]
    fn a_string_with_an_escape_does_not_grow_its_buffer() {
        for raw in [r"\n", r"made-up\\\u0041\ud83d\ude00\t", r#"\"a\"b"#] {
            let text = format!("\"{raw}\"");
            let Some(Value::String(s)) = read(&text) else { panic!("{text}") };
            assert_eq!(s.capacity(), raw.len(), "{text}: the buffer grew");
            assert_eq!(Some(s), serde_json::from_str::<String>(&text).ok());
        }
    }
}

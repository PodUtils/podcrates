//! The JSON of `toJSON` and `fromJSON`: a writer in the runner's form (two
//! spaces of indent, `"key": value`, the number text of GitHub) and a
//! strict reader of RFC 8259 into a [`Value`].

use crate::number;
use crate::value::{Object, Value};

/// The deepest document that `fromJSON` reads, as the default of the JSON
/// reader of the runner.
pub const MAX_JSON_DEPTH: usize = 64;

/// The JSON text of a value. `pretty` gives the form of `toJSON`; else one
/// line with no space.
pub(crate) fn write(value: &Value, pretty: bool) -> String {
    let mut out = String::new();
    put(value, pretty, 0, &mut out);
    out
}

fn put(value: &Value, pretty: bool, depth: usize, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number::format(*n)),
        Value::String(s) => quote(s, out),
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Object(o) if o.is_empty() => out.push_str("{}"),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                gap(i, pretty, depth + 1, out);
                put(item, pretty, depth + 1, out);
            }
            end(pretty, depth, out);
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            for (i, (key, item)) in o.iter().enumerate() {
                gap(i, pretty, depth + 1, out);
                quote(key, out);
                out.push_str(if pretty { ": " } else { ":" });
                put(item, pretty, depth + 1, out);
            }
            end(pretty, depth, out);
            out.push('}');
        }
    }
}

fn gap(i: usize, pretty: bool, depth: usize, out: &mut String) {
    if i > 0 {
        out.push(',');
    }
    if pretty {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', depth * 2));
    }
}

fn end(pretty: bool, depth: usize, out: &mut String) {
    if pretty {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', depth * 2));
    }
}

fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The value of a JSON text. A key that comes again replaces the value of
/// the first one, in its place. A document deeper than
/// [`MAX_JSON_DEPTH`] is an error.
pub(crate) fn read(text: &str) -> Result<Value, String> {
    let mut reader = Reader { b: text.as_bytes(), i: 0, text };
    reader.space();
    if reader.i == reader.b.len() {
        return Err("the text is empty".into());
    }
    let value = reader.value(0)?;
    reader.space();
    if reader.i != reader.b.len() {
        return Err(reader.fault("text after the value"));
    }
    Ok(value)
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
    text: &'a str,
}

impl Reader<'_> {
    fn fault(&self, why: &str) -> String {
        format!("{why} at byte {}", self.i)
    }

    fn space(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.space();
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth >= MAX_JSON_DEPTH {
            return Err(format!("the document is deeper than {MAX_JSON_DEPTH} levels"));
        }
        self.space();
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut object = Object::new();
                if self.eat(b'}') {
                    return Ok(object.into());
                }
                loop {
                    self.space();
                    if self.b.get(self.i) != Some(&b'"') {
                        return Err(self.fault("a key must be a string"));
                    }
                    let key = self.string()?;
                    if !self.eat(b':') {
                        return Err(self.fault("a `:` is necessary"));
                    }
                    let item = self.value(depth + 1)?;
                    object.insert(key, item);
                    if self.eat(b',') {
                        continue;
                    }
                    if self.eat(b'}') {
                        return Ok(object.into());
                    }
                    return Err(self.fault("a `,` or a `}` is necessary"));
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                if self.eat(b']') {
                    return Ok(items.into());
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    if self.eat(b',') {
                        continue;
                    }
                    if self.eat(b']') {
                        return Ok(items.into());
                    }
                    return Err(self.fault("a `,` or a `]` is necessary"));
                }
            }
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.fault("a value is necessary")),
        }
    }

    fn word(&mut self, word: &str, value: Value) -> Result<Value, String> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(value)
        } else {
            Err(self.fault("an unknown word"))
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        let digits = |r: &mut Reader<'_>| {
            let s = r.i;
            while r.i < r.b.len() && r.b[r.i].is_ascii_digit() {
                r.i += 1;
            }
            r.i - s
        };
        if self.b[self.i] == b'-' {
            self.i += 1;
        }
        let whole = digits(self);
        if whole == 0 || (whole > 1 && self.b[self.i - whole] == b'0') {
            return Err(self.fault("a bad number"));
        }
        if self.b.get(self.i) == Some(&b'.') {
            self.i += 1;
            if digits(self) == 0 {
                return Err(self.fault("a bad number"));
            }
        }
        if matches!(self.b.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.b.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if digits(self) == 0 {
                return Err(self.fault("a bad number"));
            }
        }
        self.text[start..self.i].parse().map(Value::Number).map_err(|_| self.fault("a bad number"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.i < self.b.len() && !matches!(self.b[self.i], b'"' | b'\\') && self.b[self.i] >= 0x20 {
                self.i += 1;
            }
            out.push_str(&self.text[start..self.i]);
            match self.b.get(self.i) {
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    let c = self.escape()?;
                    out.push(c);
                }
                Some(_) => return Err(self.fault("a control character in a string")),
                None => return Err(self.fault("a string with no end")),
            }
        }
    }

    fn escape(&mut self) -> Result<char, String> {
        let c = self.b.get(self.i).copied().ok_or_else(|| self.fault("a string with no end"))?;
        self.i += 1;
        Ok(match c {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let high = self.hex4()?;
                if (0xD800..0xDC00).contains(&high) && self.b[self.i..].starts_with(b"\\u") {
                    let save = self.i;
                    self.i += 2;
                    let low = self.hex4()?;
                    if (0xDC00..0xE000).contains(&low) {
                        let code = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                        return Ok(char::from_u32(code).unwrap_or('\u{FFFD}'));
                    }
                    self.i = save;
                }
                char::from_u32(high).unwrap_or('\u{FFFD}')
            }
            _ => return Err(self.fault("a bad escape")),
        })
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self.b.get(self.i..self.i + 4).ok_or_else(|| self.fault("a bad escape"))?;
        let text = std::str::from_utf8(digits).map_err(|_| self.fault("a bad escape"))?;
        let code = u32::from_str_radix(text, 16).map_err(|_| self.fault("a bad escape"))?;
        if !digits.iter().all(u8::is_ascii_hexdigit) {
            return Err(self.fault("a bad escape"));
        }
        self.i += 4;
        Ok(code)
    }
}

//! The DER of a key file (X.690, clause 10), by hand: the few elements of
//! one structure. The reader is strict, as the file is untrusted: tags of
//! one octet, definite lengths in their fewest octets, each length checked
//! against the bytes that remain, integers in their fewest octets.

use super::Refusal;

pub const BOOLEAN: u8 = 0x01;
pub const INTEGER: u8 = 0x02;
pub const OCTET_STRING: u8 = 0x04;
pub const OID: u8 = 0x06;
pub const UTF8_STRING: u8 = 0x0C;
pub const SEQUENCE: u8 = 0x30;

/// `[n] EXPLICIT`: context-specific and constructed.
pub const fn explicit(n: u8) -> u8 {
    0xA0 | n
}

/// The number of an `[n] EXPLICIT` tag, or `None` for another tag.
pub fn explicit_number(tag: u8) -> Option<u8> {
    (tag & 0xE0 == 0xA0).then_some(tag & 0x1F)
}

fn malformed(why: impl Into<String>) -> Refusal {
    Refusal::Malformed(why.into())
}

/// A reader of the elements in the contents of one element.
pub struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Reader { rest: bytes }
    }

    pub fn is_empty(&self) -> bool {
        self.rest.is_empty()
    }

    /// The tag of the next element, if one remains.
    pub fn peek(&self) -> Option<u8> {
        self.rest.first().copied()
    }

    /// The contents of the next element, which must have the tag `tag`.
    pub fn element(&mut self, tag: u8, what: &str) -> Result<&'a [u8], Refusal> {
        let Some(&found) = self.rest.first() else {
            return Err(malformed(format!("{what} is missing")));
        };
        if found != tag {
            return Err(malformed(format!("{what}: the tag {found:#04x}, where {tag:#04x} is due")));
        }
        let (contents, rest) = split(self.rest)?;
        self.rest = rest;
        Ok(contents)
    }

    /// The contents of the one element of `tag` that `bytes` holds, with no
    /// byte after it.
    pub fn only(bytes: &'a [u8], tag: u8, what: &'static str) -> Result<&'a [u8], Refusal> {
        let mut reader = Reader::new(bytes);
        let contents = reader.element(tag, what)?;
        reader.finish(what)?;
        Ok(contents)
    }

    /// The end of the contents: a byte after the last element is refused.
    pub fn finish(&self, after: &'static str) -> Result<(), Refusal> {
        match self.rest.len() {
            0 => Ok(()),
            count => Err(Refusal::Trailing { after, count }),
        }
    }
}

/// The contents of the element at the start of `input`, and the bytes
/// after it.
fn split(input: &[u8]) -> Result<(&[u8], &[u8]), Refusal> {
    let [tag, first, rest @ ..] = input else {
        return Err(malformed("an element is cut short"));
    };
    if tag & 0x1F == 0x1F {
        return Err(malformed("a tag of more than one octet, which the structure does not have"));
    }
    let cut = || malformed("an element is cut short");
    let fewest = || malformed("a length not in its fewest octets, which DER does not allow");
    let (length, rest) = match *first {
        0..=0x7F => (usize::from(*first), rest),
        0x80 => return Err(malformed("an indefinite length, which DER does not allow")),
        0x81 => match rest {
            [n, ..] if *n < 0x80 => return Err(fewest()),
            [n, rest @ ..] => (usize::from(*n), rest),
            [] => return Err(cut()),
        },
        0x82 => match rest {
            [0, _, ..] => return Err(fewest()),
            [a, b, rest @ ..] => (usize::from(*a) << 8 | usize::from(*b), rest),
            _ => return Err(cut()),
        },
        _ => return Err(malformed("a length of more than two octets: more than a key file holds")),
    };
    if length > rest.len() {
        return Err(malformed(format!("a length of {length} bytes, past the end ({} remain)", rest.len())));
    }
    Ok(rest.split_at(length))
}

/// The value of the contents of a BOOLEAN. Each octet other than zero is
/// true, as the draft (section 6) tells a reader.
pub fn boolean(contents: &[u8], what: &str) -> Result<bool, Refusal> {
    match contents {
        [octet] => Ok(*octet != 0),
        _ => Err(malformed(format!("{what}: a BOOLEAN of {} octets", contents.len()))),
    }
}

/// The value of the contents of an INTEGER that is not negative and fits
/// 32 bits, in its fewest octets.
pub fn unsigned(contents: &[u8], what: &str) -> Result<u32, Refusal> {
    let digits = match contents {
        [] => return Err(malformed(format!("{what}: an INTEGER of no octet"))),
        [first, ..] if first & 0x80 != 0 => return Err(malformed(format!("{what}: a negative INTEGER"))),
        [0, next, ..] if next & 0x80 == 0 => {
            return Err(malformed(format!("{what}: an INTEGER not in its fewest octets")));
        }
        [0, rest @ ..] if !rest.is_empty() => rest,
        _ => contents,
    };
    if digits.len() > 4 {
        return Err(malformed(format!("{what}: an INTEGER of more than 32 bits")));
    }
    Ok(digits.iter().fold(0u32, |n, b| n << 8 | u32::from(*b)))
}

/// The text of a UTF8String.
pub fn text(contents: &[u8], what: &str) -> Result<String, Refusal> {
    String::from_utf8(contents.to_vec()).map_err(|_| malformed(format!("{what} is not UTF-8")))
}

/// The dotted form of an OID, for the words of a refusal.
pub fn dotted(oid: &[u8]) -> String {
    let mut arcs: Vec<u64> = Vec::new();
    let mut value = 0u64;
    for byte in oid.iter().take(32) {
        value = (value << 7) | u64::from(byte & 0x7F);
        if byte & 0x80 == 0 {
            if arcs.is_empty() {
                let first = (value / 40).min(2);
                arcs.extend([first, value - 40 * first]);
            } else {
                arcs.push(value);
            }
            value = 0;
        }
    }
    let words: Vec<String> = arcs.iter().map(u64::to_string).collect();
    words.join(".")
}

/// Append the element of `tag` and `contents` to `out`, its length in its
/// fewest octets.
pub fn put(out: &mut Vec<u8>, tag: u8, contents: &[u8]) {
    out.push(tag);
    let length = contents.len();
    if length < 0x80 {
        out.push(length as u8);
    } else {
        let octets = length.to_be_bytes();
        let start = octets.iter().position(|b| *b != 0).unwrap_or(octets.len() - 1);
        out.push(0x80 | (octets.len() - start) as u8);
        out.extend_from_slice(&octets[start..]);
    }
    out.extend_from_slice(contents);
}

/// The element of `tag` and `contents`.
pub fn element(tag: u8, contents: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(contents.len() + 4);
    put(&mut out, tag, contents);
    out
}

/// The INTEGER of `n`, in its fewest octets.
pub fn integer(n: u32) -> Vec<u8> {
    let octets = n.to_be_bytes();
    let start = octets.iter().position(|b| *b != 0).unwrap_or(3);
    let mut contents = Vec::with_capacity(5);
    if octets[start] & 0x80 != 0 {
        contents.push(0);
    }
    contents.extend_from_slice(&octets[start..]);
    element(INTEGER, &contents)
}

/// The `[n] EXPLICIT BOOLEAN` of true, as DER writes it (0xFF).
pub fn explicit_true(n: u8) -> Vec<u8> {
    element(explicit(n), &element(BOOLEAN, &[0xFF]))
}

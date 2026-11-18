//! The head of a request, as a server reads it. A line ends with CRLF or a
//! bare LF (RFC 9112 section 2.2); a bare CR, a space before a header's
//! colon, a folded line and a control byte are refused.

use super::{Bounds, MAX_HEAD, head_bounds, leading_empty};

/// A request's head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// As sent: a method is case-sensitive (RFC 9110 section 9.1).
    pub method: String,
    /// The request-target, as sent.
    pub target: String,
    /// The minor version of HTTP/1: 0 or 1.
    pub minor: u8,
    /// Header names lower-cased, values trimmed, in arrival order.
    pub headers: Vec<(String, String)>,
}

impl Request {
    /// The first value of a header, by case-insensitive name.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }

    /// Whether a header of `name` is present at all.
    pub fn has(&self, name: &str) -> bool {
        self.header(name).is_some()
    }

    /// Whether the lists of the headers of `name` hold `token`, with any case:
    /// `Connection: keep-alive, Upgrade` holds `upgrade` (RFC 9110 section 5.6.1).
    pub fn has_token(&self, name: &str, token: &str) -> bool {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .filter(|(n, _)| *n == name)
            .flat_map(|(_, v)| v.split(','))
            .any(|item| item.trim().eq_ignore_ascii_case(token))
    }
}

/// Why a request's head was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    /// The connection ended, or failed, before the head did: nobody to answer.
    Gone(String),
    /// No end of the head within its bounds: 431.
    TooLarge,
    /// Not a request of HTTP/1: 400, or 505 for another version of HTTP.
    Malformed { status: u16, why: String },
}

impl RequestError {
    /// The status that answers it, when a client is there to read one.
    pub fn status(&self) -> Option<u16> {
        match self {
            RequestError::Gone(_) => None,
            RequestError::TooLarge => Some(431),
            RequestError::Malformed { status, .. } => Some(*status),
        }
    }
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestError::Gone(why) => write!(f, "the client left before its request ended: {why}"),
            RequestError::TooLarge => write!(f, "no end of the request's headers within {} KiB", MAX_HEAD / 1024),
            RequestError::Malformed { why, .. } => write!(f, "not a request of HTTP/1.1: {why}"),
        }
    }
}

impl std::error::Error for RequestError {}

fn malformed(why: impl Into<String>) -> RequestError {
    RequestError::Malformed { status: 400, why: why.into() }
}

/// Where the head of a request is in `buf`, as [`head_bounds`] finds it:
/// `None` while it has not ended within `bounds`, and
/// [`RequestError::TooLarge`] past them. A reader reads more bytes into
/// `buf` after a `None`.
pub fn find_request(buf: &[u8], bounds: Bounds) -> Result<Option<(usize, usize)>, RequestError> {
    let lines = |bytes: &[u8]| bytes.iter().filter(|&&b| b == b'\n').count();
    match head_bounds(buf) {
        // The empty line at the end is one LF more than the lines.
        Some((start, end)) => {
            if end - start > bounds.bytes || lines(&buf[start..end]) > bounds.lines.saturating_add(1) {
                return Err(RequestError::TooLarge);
            }
            Ok(Some((start, end)))
        }
        None if buf.len() > bounds.bytes || lines(&buf[leading_empty(buf)..]) > bounds.lines => {
            Err(RequestError::TooLarge)
        }
        None => Ok(None),
    }
}

/// A request's head: its request line, then its headers, each line ended by
/// CRLF or LF, and the empty line.
pub fn parse_request(head: &[u8]) -> Result<Request, RequestError> {
    let mut lines = Vec::new();
    for raw in head.split(|&b| b == b'\n') {
        let line = raw.strip_suffix(b"\r").unwrap_or(raw);
        if line.contains(&b'\r') {
            return Err(malformed("a bare CR in a line"));
        }
        // A path goes into logs and onto a terminal: no escape sequence.
        if line.iter().any(|&b| (b < 0x20 && b != b'\t') || b == 0x7f) {
            return Err(malformed("a control byte in a line"));
        }
        lines.push(line);
    }
    // The split leaves the empty line, and what follows its LF.
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let Some((first, fields)) = lines.split_first() else { return Err(malformed("no request line")) };
    let (method, target, minor) = request_line(first)?;
    let mut headers = Vec::with_capacity(fields.len());
    for field in fields {
        headers.push(header_field(field)?);
    }
    Ok(Request { method, target, minor, headers })
}

/// `METHOD SP TARGET SP HTTP/1.x`, one space each (RFC 9112 section 3).
fn request_line(line: &[u8]) -> Result<(String, String, u8), RequestError> {
    let text = std::str::from_utf8(line).map_err(|_| malformed("the request line is not ASCII"))?;
    let parts: Vec<&str> = text.split(' ').collect();
    let [method, target, version] = parts[..] else {
        return Err(malformed("the request line is not METHOD TARGET HTTP/1.1"));
    };
    if method.is_empty() || !method.bytes().all(is_tchar) {
        return Err(malformed("the method is not a token"));
    }
    if target.is_empty() || !target.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(malformed("the request-target is empty or not ASCII"));
    }
    let minor = match version {
        "HTTP/1.1" => 1,
        "HTTP/1.0" => 0,
        other if other.starts_with("HTTP/") => {
            return Err(RequestError::Malformed { status: 505, why: format!("{other}: this server speaks HTTP/1.1") });
        }
        _ => return Err(malformed("the request line names no version of HTTP")),
    };
    Ok((method.to_string(), target.to_string(), minor))
}

/// `NAME: VALUE`, with no space before the colon (RFC 9112 section 5.1) and
/// no folded line (section 5.2).
fn header_field(line: &[u8]) -> Result<(String, String), RequestError> {
    if line.first().is_some_and(|b| *b == b' ' || *b == b'\t') {
        return Err(malformed("a folded header line"));
    }
    let Some(colon) = line.iter().position(|&b| b == b':') else {
        return Err(malformed("a header line with no colon"));
    };
    let name = &line[..colon];
    if name.is_empty() || !name.iter().copied().all(is_tchar) {
        return Err(malformed("a header's name is not a token, or a space comes before its colon"));
    }
    let value = String::from_utf8_lossy(&line[colon + 1..]);
    let name = String::from_utf8_lossy(name).to_ascii_lowercase();
    Ok((name, value.trim_matches([' ', '\t']).to_string()))
}

/// A character of a token (RFC 9110 section 5.6.2).
fn is_tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

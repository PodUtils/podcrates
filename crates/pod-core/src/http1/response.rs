//! The head of a response, as a client reads it: the status line, the
//! header fields, and how its body ends.

use crate::text::one_line;

/// A response head: the status, the reason, and the headers, names in lower
/// case.
pub type Head = (u16, String, Vec<(String, String)>);

/// Parse a response head into status, reason and lower-cased headers. A
/// line with no colon is passed over.
pub fn parse_head(head: &str) -> Result<Head, String> {
    let (status, reason) = parse_status_line(head)?;
    let headers = head
        .split("\r\n")
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Ok((status, reason, headers))
}

/// `HTTP/1.1 200 Connection Established` → `(200, "Connection Established")`.
pub fn parse_status_line(head: &str) -> Result<(u16, String), String> {
    let line = head.lines().next().unwrap_or_default();
    let mut parts = line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/") {
        return Err(format!("not an HTTP status line: {line:?}"));
    }
    let status =
        parts.next().and_then(|s| s.parse::<u16>().ok()).ok_or_else(|| format!("no status code in {line:?}"))?;
    // The reason phrase is the peer's text and ends up on the user's terminal.
    Ok((status, one_line(parts.next().unwrap_or_default())))
}

/// How the body of an answer ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    Length(u64),
    Chunked,
    UntilClose,
}

impl Framing {
    /// The framing of the body of an answer of `status` with `headers`
    /// (lower-cased names): none after 1xx, 204 and 304; chunks before a
    /// length (RFC 9112 section 6.3); else to the close.
    pub fn of(status: u16, headers: &[(String, String)]) -> Framing {
        let header = |name: &str| headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.to_ascii_lowercase());
        if status < 200 || status == 204 || status == 304 {
            Framing::Length(0)
        } else if header("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
            Framing::Chunked
        } else if let Some(length) = header("content-length").and_then(|v| v.trim().parse::<u64>().ok()) {
            Framing::Length(length)
        } else {
            Framing::UntilClose
        }
    }
}

/// Whether the connection of an answer can carry the next request: an
/// answer of HTTP/1.1 (`http11`), with no `Connection: close` and a body
/// whose end is known before the close.
pub fn keeps(http11: bool, headers: &[(String, String)], framing: Framing) -> bool {
    let close = headers.iter().any(|(n, v)| n == "connection" && v.to_ascii_lowercase().contains("close"));
    http11 && !close && framing != Framing::UntilClose
}

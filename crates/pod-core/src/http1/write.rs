//! The write of a head: a request of a client, and the answers of a server
//! that answers one request on a connection.

use crate::text::one_line;

/// The reason phrase of each status that the servers of the consumers send,
/// by RFC 9110's names; podssh's relay role sends 404, 409, 413 and 429
/// through [`refusal`] too. The phrase of 200 is a CONNECT's.
pub fn reason(status: u16) -> &'static str {
    match status {
        101 => "Switching Protocols",
        200 => "Connection Established",
        201 => "Created",
        202 => "Accepted",
        206 => "Partial Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        413 => "Content Too Large",
        416 => "Range Not Satisfiable",
        426 => "Upgrade Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        _ => "Error",
    }
}

/// `first`, then each header on its line, then the empty line.
fn lines<'a>(first: &str, headers: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut out = format!("{first}\r\n");
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str("\r\n");
    out
}

/// Refuse a part with CR, LF or NUL: it would end a line, or the head, where
/// the caller did not.
fn check<'a>(parts: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
    match parts.into_iter().any(|part| part.contains(['\r', '\n', '\0'])) {
        true => Err("a request line or header contains CR or LF, or NUL".to_string()),
        false => Ok(()),
    }
}

/// The head of a request: `METHOD TARGET HTTP/1.1`, each header in its
/// order, and the empty line. A part with CR, LF or NUL is refused.
pub fn request_head(method: &str, target: &str, headers: &[(&str, &str)]) -> Result<String, String> {
    check([method, target].into_iter().chain(headers.iter().flat_map(|(n, v)| [*n, *v])))?;
    Ok(lines(&format!("{method} {target} HTTP/1.1"), headers.iter().copied()))
}

/// The head of a response: `HTTP/1.1 STATUS PHRASE`, each header in its
/// order, and the empty line. A part with CR, LF or NUL is refused.
pub fn response_head(status: u16, phrase: &str, headers: &[(&str, &str)]) -> Result<String, String> {
    check(std::iter::once(phrase).chain(headers.iter().flat_map(|(n, v)| [*n, *v])))?;
    Ok(lines(&format!("HTTP/1.1 {status} {phrase}"), headers.iter().copied()))
}

/// A refusal of `status`: its reason phrase, with `words` after it when there
/// are some, `headers`, and `words` again as a body of text. Each part is
/// made one line, as the words may be a peer's. The connection closes after
/// it: these servers answer one request on a connection.
pub fn refusal(status: u16, words: &str, headers: &[(&str, &str)]) -> Vec<u8> {
    let words = one_line(words);
    let mut phrase = reason(status).to_string();
    if !words.is_empty() {
        phrase.push_str(": ");
        phrase.push_str(&cut(&words, 200));
    }
    let body = if words.is_empty() { String::new() } else { format!("{words}\n") };
    let mut fields: Vec<(String, String)> = headers.iter().map(|(n, v)| (one_line(n), one_line(v))).collect();
    if !body.is_empty() {
        fields.push(("Content-Type".into(), "text/plain; charset=utf-8".into()));
    }
    fields.push(("Content-Length".into(), body.len().to_string()));
    fields.push(("Connection".into(), "close".into()));
    let head = lines(&format!("HTTP/1.1 {status} {phrase}"), fields.iter().map(|(n, v)| (n.as_str(), v.as_str())));
    format!("{head}{body}").into_bytes()
}

/// A whole answer of `status` whose body `body` is of the type `kind`, after
/// which the connection closes, as after [`refusal`]: the pages of
/// podssh's `ws-listen:` `--cover`. With `head_only`, the answer to a HEAD:
/// the same head, its length the body's, and no body (RFC 9110 section
/// 9.3.2). `nosniff`, so that a browser takes the type that the name gave.
pub fn answer(status: u16, kind: &str, body: &[u8], head_only: bool) -> Vec<u8> {
    // [`reason`]'s 200 is a CONNECT's; a page's is OK.
    let phrase = if status == 200 { "OK" } else { reason(status) };
    let (kind, length) = (one_line(kind), body.len().to_string());
    let fields = [
        ("Content-Type", kind.as_str()),
        ("Content-Length", length.as_str()),
        ("X-Content-Type-Options", "nosniff"),
        ("Connection", "close"),
    ];
    let mut out = lines(&format!("HTTP/1.1 {status} {phrase}"), fields).into_bytes();
    if !head_only {
        out.extend_from_slice(body);
    }
    out
}

/// The answer to a CONNECT whose tunnel is open: no header, and no length,
/// which a 2xx answer to CONNECT must not have (RFC 9110 section 9.3.6).
pub fn established() -> Vec<u8> {
    lines(&format!("HTTP/1.1 200 {}", reason(200)), []).into_bytes()
}

/// `text` cut to `max` bytes on a character boundary.
fn cut(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

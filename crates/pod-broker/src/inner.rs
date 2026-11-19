//! One request inside a tunnel that the TLS end reads (T-100), with no I/O:
//! the checks of its head, the swap of the placeholders in its head, and
//! the head of its answer with each value scrubbed. [`crate::tls_end`] moves
//! the bytes.
//!
//! The checks: a request-target in origin form with a path in its normal
//! form (400), one `Host` that names the CONNECT target (421), a path that
//! a rule allows (403), and a body with one framing (400).

use base64::Engine as _;
use pod_proxy::http::{Framing, Request, Response};

use crate::credentials::Place;
use crate::policy::Compiled;
use crate::replace::replace_text;
use crate::target::{Authority, parse_authority, parse_origin};
use crate::vault::Vault;

pub(crate) type Pairs = Vec<(Vec<u8>, Vec<u8>)>;

/// A request that the TLS end refuses: its status, a word for the audit
/// line, and the words of the answer. No word holds a value of the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub status: u16,
    pub note: &'static str,
    pub words: String,
}

fn refused(status: u16, note: &'static str, words: impl Into<String>) -> Refused {
    Refused { status, note, words: words.into() }
}

/// A request that passed the checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inner {
    /// The path in its normal form.
    pub path: String,
    /// The query with no `?`, as sent.
    pub query: Option<String>,
    pub framing: Framing,
    /// The rule that allows the path.
    pub rule: String,
}

/// The checks of `request` in the tunnel to `authority` under `policy`.
pub fn check(request: &Request, authority: &Authority, policy: &Compiled) -> Result<Inner, Refused> {
    if request.method == "CONNECT" {
        return Err(refused(400, "target-bad", "no CONNECT inside a tunnel"));
    }
    let (path, query) = parse_origin(&request.target).map_err(|e| refused(400, "target-bad", e.0))?;
    let hosts: Vec<&str> = request.headers.iter().filter(|(n, _)| n == "host").map(|(_, v)| v.as_str()).collect();
    let [host] = hosts[..] else { return Err(refused(400, "host-bad", "a request has one Host")) };
    let named = parse_authority(host, Some(authority.port)).map_err(|e| refused(400, "host-bad", e.0))?;
    if named != *authority {
        return Err(refused(421, "host-mismatch", "the Host is not the target of the CONNECT"));
    }
    let framing = crate::forward::request_framing(request).map_err(|why| refused(400, "body-bad", why))?;
    let Some(rule) = policy.allows_path(authority, &path) else {
        let why = crate::policy::NoRule::Path.words(authority);
        return Err(refused(403, "path-denied", format!("denied by the rule policy:no-rule: {why}")));
    };
    Ok(Inner { path, query, framing, rule })
}

/// The head of `head` read and checked, for the fuzz target of the TLS
/// end: a refusal of the reader is a 400.
pub fn read(head: &[u8], authority: &Authority, policy: &Compiled) -> Result<(Request, Inner), Refused> {
    let request = pod_proxy::http::request::parse_request(head)
        .map_err(|e| refused(e.status().unwrap_or(400), "head-bad", e.to_string()))?;
    let inner = check(&request, authority, policy)?;
    Ok((request, inner))
}

/// The pairs of a swap for one host: each placeholder and its value, by
/// place. All empty for a host that no credential names.
#[derive(Debug, Clone, Default)]
pub(crate) struct Outgoing {
    pub header: Pairs,
    pub basic: Pairs,
    pub query: Pairs,
    pub body: Pairs,
    /// The job has a secret: each answer is scrubbed, so it asks for no
    /// compression.
    pub scrub: bool,
}

impl Outgoing {
    pub fn of(vault: &Vault, authority: &Authority) -> Outgoing {
        let host = &authority.host;
        Outgoing {
            header: vault.outgoing(host, Place::Header),
            basic: vault.outgoing(host, Place::Basic),
            query: vault.outgoing(host, Place::Query),
            body: vault.outgoing(host, Place::Body),
            scrub: vault.has_secrets(),
        }
    }
}

/// The headers that the TLS end does not pass: the framing and the
/// expectation, which it writes itself, and the credential of the broker.
const DROPPED: &[&str] =
    &["content-length", "transfer-encoding", "expect", "trailer", "proxy-authorization", "proxy-connection"];

/// The head of a request for the far end, with no framing line and no end:
/// the request line and each header, the placeholders swapped. The count of
/// the swaps, and for each `Basic` value that changed, the pair that the
/// scrub of the answer takes back.
pub(crate) fn request_head(request: &Request, inner: &Inner, out: &Outgoing) -> (String, u64, Pairs) {
    let mut swaps = 0;
    let mut target = inner.path.clone();
    if let Some(query) = &inner.query {
        let (query, n) = replace_text(&out.query, query);
        swaps += n;
        target.push('?');
        target.push_str(&query);
    }
    let mut head = format!("{} {target} HTTP/1.1\r\n", request.method);
    let mut basic_back = Pairs::new();
    for (name, value) in &request.headers {
        if DROPPED.contains(&name.as_str()) || (out.scrub && name == "accept-encoding") {
            continue;
        }
        let (mut value, n) = replace_text(&out.header, value);
        swaps += n;
        if name == "authorization"
            && let Some((swapped, n)) = swap_basic(&value, &out.basic)
        {
            basic_back.push((swapped.clone().into_bytes(), value.clone().into_bytes()));
            value = swapped;
            swaps += n;
        }
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if out.scrub {
        head.push_str("accept-encoding: identity\r\n");
    }
    (head, swaps, basic_back)
}

/// `Basic CREDENTIALS` with each placeholder of `pairs` swapped in the user
/// and the password; `None` when nothing changed.
fn swap_basic(value: &str, pairs: &Pairs) -> Option<(String, u64)> {
    let (scheme, encoded) = value.split_once(' ')?;
    if pairs.is_empty() || !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let b64 = base64::engine::general_purpose::STANDARD;
    let raw = zeroize::Zeroizing::new(b64.decode(encoded.trim()).ok()?);
    let text = std::str::from_utf8(&raw).ok()?;
    let (swapped, n) = replace_text(pairs, text);
    let swapped = zeroize::Zeroizing::new(swapped);
    (n > 0).then(|| (format!("{scheme} {}", b64.encode(swapped.as_bytes())), n))
}

/// How the body of an answer goes to the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Out {
    /// No body.
    Empty,
    /// Read whole, scrubbed, with its new length.
    Whole(u64),
    /// As it is, by its length: no scrub.
    Length(u64),
    /// Scrubbed as it comes, in chunks.
    Chunked,
    /// Scrubbed as it comes, to the close.
    ToClose,
}

/// The way of the body of `answer` with the framing `framing`, to a job of
/// HTTP/1.`minor`; `scrub` when the job has a secret.
pub(crate) fn out_of(framing: Framing, scrub: bool, max_body: u64, minor: u8) -> Out {
    let stream = if minor == 1 { Out::Chunked } else { Out::ToClose };
    match framing {
        Framing::Empty => Out::Empty,
        Framing::Length(n) if !scrub => Out::Length(n),
        Framing::Length(n) if n <= max_body => Out::Whole(n),
        Framing::Length(_) | Framing::Chunked => stream,
        Framing::ToEnd => Out::ToClose,
    }
}

/// The headers of the answer that the TLS end writes itself.
const ANSWER_FRAMING: &[&str] = &["content-length", "transfer-encoding", "trailer", "connection"];

/// The head of `answer` for the job, each value scrubbed, with the framing
/// of `out` (`length`: the new length of [`Out::Whole`]); `keep` when the
/// connection carries a next request. The count of the scrubs.
pub(crate) fn answer_head(answer: &Response, scrub: &Pairs, out: Out, length: u64, keep: bool) -> (String, u64) {
    let (reason, mut count) = replace_text(scrub, &answer.reason);
    let reason = if reason.is_empty() { pod_core::http1::reason(answer.status).to_string() } else { reason };
    let mut head = format!("HTTP/1.1 {} {reason}\r\n", answer.status);
    let switching = answer.status == 101;
    for (name, value) in &answer.headers {
        let framing = ANSWER_FRAMING.contains(&name.as_str());
        if framing && !(switching && name == "connection") && !(out == Out::Empty && name == "content-length") {
            continue;
        }
        let (value, n) = replace_text(scrub, value);
        count += n;
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    match out {
        Out::Whole(_) => head.push_str(&format!("content-length: {length}\r\n")),
        Out::Length(n) => head.push_str(&format!("content-length: {n}\r\n")),
        Out::Chunked => head.push_str("transfer-encoding: chunked\r\n"),
        Out::Empty | Out::ToClose => {}
    }
    if !switching && !keep {
        head.push_str("connection: close\r\n");
    }
    head.push_str("\r\n");
    (head, count)
}

/// Whether the answer has a coding that the scrub cannot read.
pub(crate) fn encoded(headers: &[(String, String)]) -> bool {
    headers
        .iter()
        .any(|(n, v)| n == "content-encoding" && !v.trim().is_empty() && !v.trim().eq_ignore_ascii_case("identity"))
}

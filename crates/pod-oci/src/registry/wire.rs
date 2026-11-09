//! The small parts of a request: the key of a token, the host of a URL,
//! the class of a status, the retry and its wait, the message of an error
//! body, the parse of a challenge, and the redaction of a URL.

use super::*;
use crate::net::Transport;

pub(super) enum Attempt {
    // Boxed. A streamed answer holds its connection and its head, and an
    // enum with no box makes each return of the small variant as large as
    // the large one.
    Done(Box<Body>),
    Retry {
        after: Option<Duration>,
        failure: Failure,
        /// The cause of a transport failure (`connection failed: ...`), when
        /// the try got no answer; `None` for a status.
        transport: Option<String>,
    },
}

pub(super) fn key(endpoint: &str, scope: &str) -> String {
    format!("{endpoint}|{scope}")
}

pub(super) fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    Some(rest.split('/').next()?.to_string())
}

/// Split `scheme://host/path` into its host and its path. Any scheme, and a
/// bare `host/path` with none: the policy sends some endpoints to plain
/// HTTP, and the error names the same host. For errors only; the path of
/// the credentials keeps the `https://` form of [`host_of`].
pub(super) fn split_host_path(url: &str) -> (String, String) {
    let after = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    match after.split_once('/') {
        Some((host, path)) => (host.to_string(), format!("/{path}")),
        None => (after.to_string(), "/".to_string()),
    }
}

/// The host that failed the transport: the one that the transport names
/// first, as a redirect can fail on a host that the request did not name;
/// else the host of the request URL.
pub(super) fn transport_host(url: &str, t: &Transport) -> String {
    if !t.host.is_empty() {
        // The port stays as the URL wrote it: two registries on one host
        // are two hosts for this message.
        return t.host.clone();
    }
    split_host_path(url).0
}

/// One host, one cause: the line reads `GET /v2/...: <host>: dns failed:
/// ...`, with the host of the URL once and the chain after the cause on the
/// same line. The failure keeps the stage and the class of `pod-proxy`, and
/// the failure of `pod-proxy` as its cause.
pub(super) fn transport_error(url: &str, t: Transport) -> Failure {
    let host = transport_host(url, &t);
    let text = format!("GET {}: {host}: {}", redact(&split_host_path(url).1), t.why);
    Failure::caused(t.failure.stage, t.failure.class, text, t.failure)
}

/// A URL in a message loses its query first: no credential gets to a log,
/// and some registries put one in a query parameter of a signed URL.
pub fn redact(url: &str) -> String {
    match url.split_once('?') {
        Some((head, _)) => format!("{head}?<redacted>"),
        None => url.to_string(),
    }
}

pub(super) fn retryable(code: u16) -> bool {
    matches!(code, 429 | 500 | 502 | 503 | 504)
}

/// The class of a status of 400 or more: a refused credential, a missing
/// manifest or blob, a limit of the rate, a registry that is down, or a
/// request that the registry does not take.
pub(super) fn class_of(code: u16) -> Class {
    match code {
        401 | 403 => Class::Refused,
        404 => Class::NotFound,
        429 => Class::TempFail,
        500..=599 => Class::Unavailable,
        _ => Class::Protocol,
    }
}

/// Seconds only. The HTTP-date form is legal, and it gets the usual backoff,
/// not a parsed date: a date read wrong is a wait of an unknown length.
pub(super) fn parse_retry_after(v: &str) -> Option<Duration> {
    v.trim().parse::<u64>().ok().map(Duration::from_secs)
}

pub(super) fn backoff(attempt: u32, asked: Option<Duration>) -> Duration {
    let ours = BACKOFF_BASE * 2u32.saturating_pow(attempt.saturating_sub(1));
    // The number of the server when it gave one, else ours, and the cap
    // over both.
    asked.unwrap_or(ours).min(BACKOFF_CAP)
}

pub(super) fn status_error(url: &str, e: Refusal) -> Failure {
    match e {
        Refusal::Status(mut resp) => {
            let code = resp.status();
            // The error body of a registry is JSON with `errors[].message`.
            // It is read for the message only; a body that does not parse
            // gives the status alone, not the body.
            let detail = crate::net::text(&mut resp, TOKEN_CEILING)
                .ok()
                .and_then(|b| registry_message(&b))
                .map(|m| format!("HTTP {code}: {m}"))
                .unwrap_or_else(|| format!("HTTP {code}"));
            http(class_of(code), &format!("GET {}", redact(url)), detail)
        }
        Refusal::Transport(t) => transport_error(url, t),
        Refusal::Refused(why) => Failure::caused(why.stage, why.class, format!("GET {}: {why}", redact(url)), why),
    }
}

pub(super) fn registry_message(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Errors {
        errors: Vec<Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        code: Option<String>,
        message: Option<String>,
    }
    let parsed: Errors = serde_json::from_str(body).ok()?;
    let first = parsed.errors.first()?;
    match (&first.code, &first.message) {
        (Some(c), Some(m)) => Some(format!("{c}: {m}")),
        (_, Some(m)) => Some(m.clone()),
        (Some(c), _) => Some(c.clone()),
        _ => None,
    }
}

/// `Bearer realm="...",service="...",scope="..."` into its parameters.
pub(super) fn parse_challenge(challenge: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let rest = match challenge.split_once(char::is_whitespace) {
        Some((_, rest)) => rest,
        None => return out,
    };
    let mut key = String::new();
    let mut value = String::new();
    let mut in_key = true;
    let mut quoted = false;
    for ch in rest.chars() {
        match ch {
            '=' if in_key && !quoted => in_key = false,
            '"' if !in_key => quoted = !quoted,
            // A comma in quotes is part of the value: a scope can hold
            // several, and a split at each comma keeps only the first, which
            // authorises less than the request needs.
            ',' if !quoted => {
                if !key.trim().is_empty() {
                    out.insert(key.trim().to_string(), value.clone());
                }
                key.clear();
                value.clear();
                in_key = true;
            }
            c if in_key => key.push(c),
            c => value.push(c),
        }
    }
    if !key.trim().is_empty() {
        out.insert(key.trim().to_string(), value);
    }
    out
}

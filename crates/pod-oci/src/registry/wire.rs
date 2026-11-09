use super::*;

pub(super) enum Attempt {
    // ⚠ Boxed. A streamed answer holds its connection and its head, and
    // `Retry` is a pointer and a duration, so the unboxed enum makes every
    // return of the small variant carry the large one's footprint.
    Done(Box<Body>),
    Retry {
        after: Option<Duration>,
        why: String,
        /// The host the transport failed against, when `why` is a transport
        /// failure rather than a status. `get` renders those without the URL:
        /// the host names it once, and the full chain stays in `why` on the
        /// same line (TODO/image.md T-1420).
        transport_host: Option<String>,
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
/// bare `host/path` without one: the transport policy downgrades some
/// endpoints to plain HTTP, and the error still has to name the same host.
/// Error rendering only; the auth path keeps [`host_of`]'s `https://` shape.
pub(super) fn split_host_path(url: &str) -> (String, String) {
    let after = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    match after.split_once('/') {
        Some((host, path)) => (host.to_string(), format!("/{path}")),
        None => (after.to_string(), "/".to_string()),
    }
}

/// TODO/image.md T-1420. The host that failed the transport: the one that
/// `net` names first, because a redirect can fail on a host the request did
/// not name, and the request URL where it names none.
pub(super) fn transport_host(url: &str, t: &crate::net::Transport) -> String {
    if !t.host.is_empty() {
        // ⚠ The port stays where the URL wrote it: two registries on one
        // host are two hosts for this message's purpose.
        return t.host.clone();
    }
    split_host_path(url).0
}

/// TODO/image.md T-1420. The cause in plain lowercase words with the full
/// chain after it on the same line: `net` names the cause by the stage that
/// broke (`dns failed`, `connection failed`, `tls failed`), and the words of
/// `pod-proxy` follow, so nothing is dropped until `-v` exists to hold them
/// (T-1416).
pub(super) fn transport_why(t: &crate::net::Transport) -> String {
    t.why.clone()
}

/// TODO/image.md T-1420. One host, one cause: the rendered line reads
/// `GET /v2/...: <host>: dns failed: ...`, with the URL's host exactly once
/// and the chain kept after the cause on the same line.
pub(super) fn transport_error(url: &str, t: &crate::net::Transport) -> Error {
    let host = transport_host(url, t);
    Error::Http {
        what: format!("GET {}", redact(&split_host_path(url).1)),
        detail: format!("{host}: {}", transport_why(t)),
    }
}

/// ⛔ A URL in a message loses its query first. `docs/security/secrets.md`: a
/// credential never reaches a log, and some registries put one in a query
/// parameter of a signed URL.
pub fn redact(url: &str) -> String {
    match url.split_once('?') {
        Some((head, _)) => format!("{head}?<redacted>"),
        None => url.to_string(),
    }
}

pub(super) fn retryable(code: u16) -> bool {
    matches!(code, 429 | 500 | 502 | 503 | 504)
}

/// ⚠ Seconds only. The HTTP-date form is legal and is answered with the
/// ordinary backoff rather than a parsed date, because a mis-parsed date is a
/// wait of unknown length and `RULES.md` section 8 forbids one.
pub(super) fn parse_retry_after(v: &str) -> Option<Duration> {
    v.trim().parse::<u64>().ok().map(Duration::from_secs)
}

pub(super) fn backoff(attempt: u32, asked: Option<Duration>) -> Duration {
    let ours = BACKOFF_BASE * 2u32.saturating_pow(attempt.saturating_sub(1));
    // The server's number when it gave one, ours otherwise, and the cap over
    // both.
    asked.unwrap_or(ours).min(BACKOFF_CAP)
}

pub(super) fn status_error(url: &str, e: Refusal) -> Error {
    match e {
        Refusal::Status(mut resp) => {
            let code = resp.status();
            // A registry's error body is JSON with a `errors[].message`. It is
            // read for the message and nothing else; an unparseable body is
            // reported as the status alone rather than dumped.
            let detail = crate::net::text(&mut resp, TOKEN_CEILING)
                .ok()
                .and_then(|b| registry_message(&b))
                .map(|m| format!("HTTP {code}: {m}"))
                .unwrap_or_else(|| format!("HTTP {code}"));
            Error::Http {
                what: format!("GET {}", redact(url)),
                detail,
            }
        }
        Refusal::Transport(t) => transport_error(url, &t),
        Refusal::Refused(why) => Error::Http {
            what: format!("GET {}", redact(url)),
            detail: why,
        },
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
            // ⚠ A comma inside quotes is part of the value: a scope legally
            // carries several, and splitting on every comma truncates it to the
            // first, which then authorises less than the request needs.
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

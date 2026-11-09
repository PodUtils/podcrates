//! The HTTP transport of the registry client: the streamed blocking client
//! of `pod-proxy`, which the caller makes with its proxy settings, its TLS
//! context and its user agent.
//!
//! The proxy rules are the rules of `pod-proxy`: a loopback target never
//! goes through a proxy, and the `NO_PROXY` list of the caller's
//! `pod_proxy::Environment` takes a host around it.
//!
//! `pod-proxy` follows each redirect of a GET or a HEAD (a blob to a CDN),
//! up to [`REDIRECTS`]. The `Authorization` header does not go to another
//! origin, and a redirect from `https://` to `http://` is refused.

use std::io::Read;
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use pod_proxy::blocking;
use pod_proxy::client::Request;
use pod_tls::Trust;

/// The body of an answer: `status()`, `header(..)` and `std::io::Read`.
pub(crate) use pod_proxy::blocking::Streamed as Body;

/// The redirects that one request follows.
pub const REDIRECTS: u8 = 5;

/// The time limits of one request.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    /// The dial, the handshake, the request and the head of the answer.
    pub head: Duration,
    /// Each wait for the next bytes of the body. A limit of a stall, not of
    /// a large transfer: a body is streamed.
    pub idle: Duration,
}

/// Why a request gave no answer to read.
pub(crate) enum Refusal {
    /// A status of 400 or more, with the answer, its body not read.
    Status(Box<Body>),
    /// No answer: the host that failed, why in plain lower-case words, and
    /// the failure of `pod-proxy`.
    Transport(Transport),
    /// A request that the client does not make, and why.
    Refused(Failure),
}

/// A request that got no answer.
#[derive(Debug)]
pub(crate) struct Transport {
    /// `host` or `host:port`, as the URL wrote it.
    pub host: String,
    /// The cause first (`dns failed`, `connection failed`), then the chain.
    pub why: String,
    /// The failure of `pod-proxy`, with its stage and its class.
    pub failure: Failure,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Status(body) => write!(f, "HTTP {}", body.status()),
            Refusal::Transport(t) => write!(f, "{}: {}", t.host, t.why),
            Refusal::Refused(why) => write!(f, "{why}"),
        }
    }
}

/// The transport of one client: the client of `pod-proxy` and the two
/// trusts. A request with no check of the certificate goes only to a
/// registry that the policy names insecure, and only with the trust that
/// the caller gave for it.
pub(crate) struct Net {
    pub client: blocking::Client,
    pub verified: Trust,
    pub unverified: Option<Trust>,
}

impl Net {
    /// One `method` (a GET or a HEAD) to `url` with `headers`, and each
    /// redirect after it. A status of 400 or more is [`Refusal::Status`].
    pub fn send(
        &self,
        method: &'static str,
        url: &str,
        headers: &[(&str, String)],
        verify: bool,
        limits: Limits,
    ) -> Result<Body, Refusal> {
        let trust = if verify {
            self.verified.clone()
        } else {
            self.unverified.clone().ok_or_else(|| {
                Refusal::Refused(Failure::new(
                    Stage::Tls,
                    Class::Config,
                    format!(
                        "{} is named insecure, and the caller gave no configuration for a registry \
                         whose certificate is not checked",
                        authority(url)
                    ),
                ))
            })?
        };
        let mut request =
            Request::new(method, url).timeout(limits.head).body_idle(limits.idle).trust(trust).redirects(REDIRECTS);
        for (name, value) in headers {
            request = request.header(name, value.clone());
        }
        let body = self.client.send_streamed(request).map_err(|f| Refusal::Transport(transport(method, url, f)))?;
        if body.status() >= 400 {
            return Err(Refusal::Status(Box::new(body)));
        }
        Ok(body)
    }
}

/// The text of a body, at most `ceiling` bytes, for an error or a token.
pub(crate) fn text(body: &mut Body, ceiling: u64) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    body.take(ceiling).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The cause of a failure in plain lower-case words, by the stage that
/// broke, then the words of `pod-proxy`. The host comes once, in front: the
/// method, the URL and each repeat of the host leave the words of
/// `pod-proxy` (`could not connect to H: H: refused`).
fn transport(method: &str, url: &str, failure: Failure) -> Transport {
    let cause = match failure.stage {
        Stage::Dns => "dns failed",
        Stage::Proxy => "proxy failed",
        Stage::Tls => "tls failed",
        _ if failure.class == Class::Timeout => "timed out",
        _ => "connection failed",
    };
    let chain = failure.chain();
    let prefix = format!("{method} {}: ", url.split(['?', '#']).next().unwrap_or(url));
    let host = authority(url);
    let chain = chain
        .strip_prefix(&prefix)
        .unwrap_or(&chain)
        .replace(&format!(" to {host}"), "")
        .replace(&format!("{host}: "), "");
    Transport { host: host.to_string(), why: format!("{cause}: {chain}"), failure }
}

/// The `host` or `host:port` of a URL, as the URL wrote it.
pub(crate) fn authority(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

/// `value` for a query: each byte but the unreserved ones as `%XX`.
pub(crate) fn query_value(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_value_is_escaped() {
        assert_eq!(query_value("repository:library/alpine:pull"), "repository%3Alibrary%2Falpine%3Apull");
        assert_eq!(query_value("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(query_value("registry.docker.io"), "registry.docker.io");
    }

    #[test]
    fn the_authority_is_the_host_as_the_url_wrote_it() {
        assert_eq!(authority("https://R.example:5000/v2/x?q#f"), "R.example:5000");
        assert_eq!(authority("r.example"), "r.example");
    }
}

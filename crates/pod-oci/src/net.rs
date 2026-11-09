//! The HTTP transport of podvm: the registry, the token realm, the Windows
//! guest image fetch and the mirror probe of `podvm-complete`.
//!
//! ⭐ `pod-proxy` (PodUtils/podcrates) carries each request: its blocking
//! client, on a private runtime of one thread, so podvm's own code stays
//! synchronous. The proxy is the environment's, by the rules of `pod-proxy`:
//! `https_proxy`, `HTTPS_PROXY`, `all_proxy`, `ALL_PROXY`, and the `NO_PROXY`
//! list. A loopback target never goes through a proxy. A variable cannot name
//! a proxy at a command (`+exec:`): `pod-proxy` takes one only from a value
//! that the user gave on purpose, and podvm gives none.
//!
//! ⚠ Two rules of podvm stay on top of those of `pod-proxy`: both spellings of
//! `NO_PROXY` count (curl reads the lower-case one and most tooling sets the
//! upper), and `ip6-localhost` is loopback.
//!
//! ⚠ `pod-proxy` follows no redirect. This module follows a redirect of a GET
//! or a HEAD (a blob to a CDN), up to [`REDIRECTS`], as podvm did before. The
//! `Authorization` header does not go to another host, and a redirect from
//! `https://` to `http://` is refused (TODO/image.md T-0201).

use std::ffi::OsString;
use std::io::Read;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Failure, Stage};
use pod_proxy::blocking;
use pod_proxy::client::Request;
use pod_proxy::resolve::System;
use pod_proxy::{Environment, ProxyChoice, Settings};
use pod_tls::Trust;
use rustls::ClientConfig;

/// The body of an answer: `status()`, `header(..)` and `std::io::Read`.
pub use pod_proxy::blocking::Streamed as Body;

/// The redirects that one request follows, as many as podvm followed before.
pub const REDIRECTS: usize = 5;

/// The `User-Agent` of each request.
pub const USER_AGENT: &str = concat!("podvm/", env!("CARGO_PKG_VERSION"));

/// The time limits of one request.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The dial, the handshake, the request and the head of the answer.
    pub head: Duration,
    /// Each wait for the next bytes of the body. ⚠ A bound on a stall, not on
    /// a large transfer: a body is streamed.
    pub idle: Duration,
}

/// Why a request gave no answer to read.
pub enum Refusal {
    /// A status of 400 or more, with the answer, its body not read.
    Status(Box<Body>),
    /// No answer: the host that failed (after a redirect, the host of the
    /// redirect), and why in plain lowercase words.
    Transport(Transport),
    /// A request that podvm does not make, and why: a redirect that it does
    /// not follow, a trust that it cannot build.
    Refused(String),
}

/// A request that got no answer.
#[derive(Debug, Clone)]
pub struct Transport {
    /// `host` or `host:port`, as the URL wrote it.
    pub host: String,
    /// The cause first (`dns failed`, `connection failed`), then the chain.
    pub why: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Status(body) => write!(f, "HTTP {}", body.status()),
            Refusal::Transport(t) => write!(f, "{}: {}", t.host, t.why),
            Refusal::Refused(why) => f.write_str(why),
        }
    }
}

/// The proxy settings of the environment that `var` reads, by the rules of
/// `pod-proxy`, with both spellings of `NO_PROXY` joined.
pub fn environment(var: &dyn Fn(&str) -> Option<OsString>) -> Environment {
    let joined = |name: &str| -> Option<OsString> {
        if !name.eq_ignore_ascii_case("no_proxy") {
            return var(name);
        }
        let lists: Vec<String> = ["NO_PROXY", "no_proxy"]
            .iter()
            .filter_map(|n| var(n)?.into_string().ok())
            .filter(|v| !v.trim().is_empty())
            .collect();
        (!lists.is_empty()).then(|| OsString::from(lists.join(",")))
    };
    Environment::read(&joined)
}

/// The blocking dialer of `env`, with the system resolver.
/// ⚠ `ip6-localhost` is pinned to `::1`, so that `pod-proxy` reads it as
/// loopback: a name pinned to loopback alone goes directly.
pub fn dialer(env: Environment) -> std::io::Result<blocking::Dialer> {
    let pins = vec![("ip6-localhost".to_string(), IpAddr::V6(Ipv6Addr::LOCALHOST))];
    blocking::Dialer::new(
        Settings::new("podvm", ProxyChoice::FromEnvironment(env)),
        Arc::new(System::with_pins(pins)),
    )
}

/// The transport of one caller: the client of `pod-proxy` and the two
/// trusts. ⛔ Each verifies with podvm's own configuration
/// ([`crate::tls::client_config`]); the one that does not is reached only for
/// a registry that the caller named insecure ([`crate::transport::Policy`]).
pub struct Net {
    client: blocking::Client,
    verified: Trust,
    unverified: Option<Trust>,
}

impl Net {
    /// A transport over the proxy of the process environment.
    pub fn from_environment(verified: Arc<ClientConfig>) -> Result<Net, String> {
        Net::with_environment(verified, environment(&|n| std::env::var_os(n)))
    }

    /// A transport over the proxy settings `env`.
    pub fn with_environment(verified: Arc<ClientConfig>, env: Environment) -> Result<Net, String> {
        let dialer = dialer(env).map_err(|e| format!("the runtime of the HTTP client: {e}"))?;
        let context = Arc::new(pod_tls::Context::new(
            Arc::new(rustls::crypto::ring::default_provider()),
            pod_tls::Settings::new(pod_tls::Words::new("podvm")),
        ));
        let client = blocking::Client::new(context, &dialer).with_user_agent(USER_AGENT);
        let verified = Trust::caller(verified).map_err(|e| e.to_string())?;
        Ok(Net {
            client,
            verified,
            unverified: None,
        })
    }

    /// The trust of a registry whose certificate is not checked.
    fn unverified(&mut self) -> Result<Trust, String> {
        if let Some(t) = &self.unverified {
            return Ok(t.clone());
        }
        let (config, _roots) = crate::tls::client_config_unverified();
        let t = Trust::caller(config).map_err(|e| e.to_string())?;
        self.unverified = Some(t.clone());
        Ok(t)
    }

    /// One `method` (a GET or a HEAD) to `url` with `headers`, and each
    /// redirect after it. A status of 400 or more is [`Refusal::Status`].
    pub fn send(
        &mut self,
        method: &'static str,
        url: &str,
        headers: &[(&str, String)],
        verify: bool,
        limits: Limits,
    ) -> Result<Body, Refusal> {
        let trust = if verify {
            self.verified.clone()
        } else {
            self.unverified().map_err(Refusal::Refused)?
        };
        let first = authority(url).to_ascii_lowercase();
        let mut url = url.to_string();
        for _ in 0..=REDIRECTS {
            let mut request = Request::new(method, url.as_str())
                .timeout(limits.head)
                .body_idle(limits.idle)
                .trust(trust.clone());
            // ⚠ A credential stays with the host that it was given for: a
            // CDN that a registry redirects to never sees the token.
            let same_host = authority(&url).eq_ignore_ascii_case(&first);
            for (name, value) in headers {
                if same_host || !name.eq_ignore_ascii_case("authorization") {
                    request = request.header(name, value.clone());
                }
            }
            let body = self
                .client
                .send_streamed(request)
                .map_err(|f| Refusal::Transport(transport(method, &url, &f)))?;
            let status = body.status();
            if matches!(status, 301 | 302 | 303 | 307 | 308)
                && let Some(location) = body.header("location")
            {
                let next = join(&url, location);
                if url.starts_with("https://") && !next.starts_with("https://") {
                    return Err(Refusal::Refused(format!(
                        "{} redirected to {}, which is not HTTPS, and podvm does not \
                         follow a redirect down to plain HTTP (TODO/image.md T-0201)",
                        crate::registry::redact(&url),
                        crate::registry::redact(&next)
                    )));
                }
                url = next;
                continue;
            }
            if status >= 400 {
                return Err(Refusal::Status(Box::new(body)));
            }
            return Ok(body);
        }
        Err(Refusal::Refused(format!(
            "{} redirected more than {REDIRECTS} times",
            crate::registry::redact(&url)
        )))
    }
}

/// The text of a body, at most `ceiling` bytes, for an error or a token.
pub fn text(body: &mut Body, ceiling: u64) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    body.take(ceiling).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The cause of a failure in plain lowercase words, by the stage that broke,
/// then the words of `pod-proxy`. ⭐ TODO/image.md T-1420: the host is named
/// once, in front, so the method, the URL and each repeat of the host leave
/// the words of `pod-proxy` (`could not connect to H: H: refused`).
fn transport(method: &str, url: &str, failure: &Failure) -> Transport {
    let cause = match failure.stage {
        Stage::Dns => "dns failed",
        Stage::Proxy => "proxy failed",
        Stage::Tls => "tls failed",
        _ if failure.class == pod_core::Class::Timeout => "timed out",
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
    Transport {
        host: host.to_string(),
        why: format!("{cause}: {chain}"),
    }
}

/// The `host` or `host:port` of a URL, as the URL wrote it.
pub fn authority(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

/// The URL that `location` names, from the URL `base` that answered with it.
pub fn join(base: &str, location: &str) -> String {
    let location = location.trim();
    if location.starts_with("https://") || location.starts_with("http://") {
        return location.to_string();
    }
    let (scheme, rest) = base.split_once("://").unwrap_or(("https", base));
    if let Some(far) = location.strip_prefix("//") {
        return format!("{scheme}://{far}");
    }
    let host = authority(base);
    if location.starts_with('/') {
        return format!("{scheme}://{host}{location}");
    }
    let path = rest[host.len()..].split(['?', '#']).next().unwrap_or("/");
    let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    format!("{scheme}://{host}{dir}/{location}")
}

/// `value` for a query: each byte but the unreserved ones as `%XX`.
pub fn query_value(value: &str) -> String {
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
mod tests;

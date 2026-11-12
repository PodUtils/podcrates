//! The settings of a client and its way out. The crate reads no variable
//! of the environment and keeps no process state: the consumer fills these
//! once and passes them.

use std::fmt;
use std::time::Duration;

use pod_core::{Class, Failure};

use crate::failure::{Step, fail};
use crate::service;

/// The base of the management API: `https://HOST[:PORT]`, or `http://` to
/// a loopback host for a test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Base {
    pub tls: bool,
    /// A name or an IP address, an IPv6 one with no brackets.
    pub host: String,
    pub port: u16,
}

/// True for a host of the loopback, the only one that a plain `http://` or
/// `ws://` may name.
pub fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

impl Base {
    /// `https://HOST[:PORT]` with no path, no query and no user; `http://`
    /// for a loopback host only.
    pub fn parse(text: &str) -> Result<Base, Failure> {
        let bad = |why: &str| fail(Step::Management, Class::Config, format!("the base {text:?}: {why}"));
        let trimmed = text.trim().trim_end_matches('/');
        let (tls, rest) = if let Some(rest) = trimmed.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = trimmed.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err(bad("use https://HOST[:PORT]"));
        };
        if rest.is_empty() || rest.contains(['/', '?', '#', '@', ' ']) {
            return Err(bad("use https://HOST[:PORT], with no path and no user"));
        }
        let default = if tls { 443 } else { 80 };
        let (host, port) = if let Some(inner) = rest.strip_prefix('[') {
            let (host, after) = inner.split_once(']').ok_or_else(|| bad("an IPv6 address with no ]"))?;
            match after.strip_prefix(':') {
                Some(port) => (host, port.parse().map_err(|_| bad("no port"))?),
                None if after.is_empty() => (host, default),
                None => return Err(bad("text after the IPv6 address")),
            }
        } else {
            match rest.rsplit_once(':') {
                Some((host, port)) => (host, port.parse().map_err(|_| bad("no port"))?),
                None => (rest, default),
            }
        };
        if host.is_empty() || port == 0 {
            return Err(bad("no host or no port"));
        }
        if !tls && !is_loopback(host) {
            return Err(bad("http:// is for a loopback host only"));
        }
        Ok(Base { tls, host: host.to_ascii_lowercase(), port })
    }

    /// `scheme://HOST[:PORT]` for `host`, the port left out when it is the
    /// scheme's own.
    pub fn url_of(&self, host: &str) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        let host = if host.contains(':') { format!("[{host}]") } else { host.to_string() };
        let default = if self.tls { 443 } else { 80 };
        if self.port == default { format!("{scheme}://{host}") } else { format!("{scheme}://{host}:{}", self.port) }
    }

    /// The host of the cluster `cluster`: its id in the place of the start
    /// of the global host (the fact `cluster-host-rule`), as the vendor's
    /// client builds it. Another host (a stand-in, a custom domain) stays.
    pub fn cluster_host(&self, cluster: &str) -> String {
        let rule = service::cluster_host_rule();
        match self.host.strip_prefix(rule) {
            Some(rest) if !rest.is_empty() => format!("{cluster}.{rest}"),
            _ => self.host.clone(),
        }
    }
}

/// The settings of a client of Dev Tunnels. [`Settings::default`] takes
/// the management API and its version from `pod-services`.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The management API.
    pub management: Base,
    /// The value of the query `api-version`.
    pub api_version: String,
    /// The bound of each step: one request of the management API, the
    /// dial, TLS and the upgrade of the relay, the SSH handshake and its
    /// authentication, the open of a channel.
    pub timeout: Duration,
    /// The tries of a reconnect, each with a fresh connect token. At least
    /// one.
    pub reconnect_tries: u32,
    /// The wait before try `n` of a reconnect (1 is the second try):
    /// `pod_core::backoff` by default.
    pub backoff: fn(u32) -> Duration,
    /// An SSH keepalive after this much silence of the host, and the end of
    /// the session after `keepalive_max` keepalives with no answer: a dead
    /// relay is found with no traffic of the caller.
    pub keepalive: Option<Duration>,
    pub keepalive_max: usize,
    /// The frame and the message that the relay WebSocket takes in.
    pub limits: pod_ws::Limits,
    /// The SSH identification of the client (RFC 4253 section 4.2): the
    /// consumer's name.
    pub ssh_id: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            management: Base::parse(&service::management()).expect("the endpoint management is a base"),
            api_version: service::api_version().to_string(),
            timeout: pod_core::backoff::CONNECT_TIMEOUT,
            reconnect_tries: 4,
            backoff: pod_core::backoff::backoff,
            keepalive: Some(Duration::from_secs(30)),
            keepalive_max: 3,
            limits: pod_ws::Limits::RELAY,
            ssh_id: "SSH-2.0-pod-devtunnel".into(),
        }
    }
}

/// The way out of a client: the HTTP client of the caller for the
/// management API (its dial, its proxy, its roots, its user agent), and
/// its WebSocket way and trust for the relay (`pod-ws`, `pod-tls`).
#[derive(Clone)]
pub struct Way {
    pub http: pod_proxy::Client,
    pub ws: pod_ws::Way,
    pub trust: pod_tls::Trust,
}

impl fmt::Debug for Way {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Way").field("http", &self.http).field("ws", &self.ws.label()).finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_base_is_https_or_plain_to_the_loopback() {
        let global = Base::parse("https://global.rel.tunnels.api.visualstudio.com/").unwrap();
        assert_eq!((global.tls, global.port), (true, 443));
        assert_eq!(global.url_of(&global.host), "https://global.rel.tunnels.api.visualstudio.com");
        assert_eq!(global.cluster_host("usw2"), "usw2.rel.tunnels.api.visualstudio.com");
        let local = Base::parse("http://127.0.0.1:8080").unwrap();
        assert_eq!(local.url_of("127.0.0.1"), "http://127.0.0.1:8080");
        assert_eq!(local.cluster_host("usw2"), "127.0.0.1");
        assert_eq!(Base::parse("https://[::1]:9").unwrap().url_of("::1"), "https://[::1]:9");
        for bad in ["http://tunnels.example", "ftp://x", "https://x/api", "https://u@x", "https://x:0", "https://"] {
            assert_eq!(Base::parse(bad).unwrap_err().class, Class::Config, "{bad}");
        }
    }

    #[test]
    fn the_default_settings_read_the_data() {
        let settings = Settings::default();
        assert_eq!(settings.management.host, "global.rel.tunnels.api.visualstudio.com");
        assert_eq!(settings.api_version, "2023-09-27-preview");
        assert!(settings.reconnect_tries >= 1);
    }
}

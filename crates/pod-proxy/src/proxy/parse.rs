//! The forms of a proxy URL:
//!
//! * `[SCHEME://][USER[:PASSWORD]@]HOST[:PORT][/]`, with the schemes `http`
//!   (the default; [`HTTP_PORT`] with no port, or the port that the caller
//!   gives), `socks5` and `socks5h` ([`SOCKS_PORT`]). The user and the
//!   password are percent-decoded; a bad escape is an error.
//! * `SCHEME+unix:/PATH` and `SCHEME://unix:/PATH`: the proxy on a Unix
//!   socket, at an absolute path with no `@` and no control character. A
//!   host or a user (`SCHEME+unix://USER@HOST/PATH`) is an error: the label
//!   shows the path, and a socket takes no credentials. `///PATH` is a path.
//! * `SCHEME+exec:COMMAND`: the proxy at the stdin and the stdout of a
//!   command, taken only when the caller says that the user gave the value
//!   on purpose; a variable that other programs read must not start a
//!   program.

use std::path::PathBuf;

use super::{Carrier, Proxy, ProxyKind};
use crate::host::split_host_port;

/// The port of an `http://` proxy URL with no port, as Go and Python's
/// requests read it. A consumer can give another ([`Proxy::parse_with`]).
pub const HTTP_PORT: u16 = 80;

/// The port of a `socks5://` or `socks5h://` URL with no port.
pub const SOCKS_PORT: u16 = 1080;

impl Proxy {
    /// Parse a proxy URL of a variable that other programs read too: no
    /// command, and [`HTTP_PORT`] for an `http://` URL with no port.
    pub fn parse(url: &str) -> Result<Proxy, String> {
        Proxy::parse_with(url, false, HTTP_PORT)
    }

    /// Parse a value that the user gave on purpose (an option, a variable of
    /// the consumer's own): also `SCHEME+exec:COMMAND`.
    pub fn parse_trusted(spec: &str) -> Result<Proxy, String> {
        Proxy::parse_with(spec, true, HTTP_PORT)
    }

    /// Parse `spec` (see the module): `commands` takes `SCHEME+exec:COMMAND`,
    /// and `http_port` is the port of an `http://` URL with no port. SOCKS4
    /// is refused by name: it has no authentication, and its first form no
    /// host names.
    pub fn parse_with(spec: &str, commands: bool, http_port: u16) -> Result<Proxy, String> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err("the proxy URL is empty".into());
        }
        // A scheme comes first: a `+exec:` or `+unix:` later in the URL is
        // a part of its user or its password.
        let scheme_only = |scheme: &str| !scheme.is_empty() && !scheme.contains(['/', ':', '@']);
        if let Some((scheme, command)) = spec.split_once("+exec:").filter(|(s, _)| scheme_only(s)) {
            if !commands {
                return Err(
                    "a proxy command (+exec:) is taken only from a value that the user gave on purpose, never \
                            from a variable that other programs read"
                        .into(),
                );
            }
            if command.trim().is_empty() {
                return Err(format!("{scheme}+exec: has no command"));
            }
            return Proxy::elsewhere(scheme, Carrier::Exec(command.trim().to_string()), http_port);
        }
        let unix = spec.split_once("+unix:").filter(|(s, _)| scheme_only(s));
        let unix = unix.or_else(|| spec.split_once("://unix:").filter(|(s, p)| scheme_only(s) && p.starts_with('/')));
        if let Some((scheme, path)) = unix {
            if !path.starts_with('/') {
                return Err(format!("the Unix socket of a proxy is an absolute path: {scheme}+unix:/PATH"));
            }
            // The label shows the path whole. In the URL form, `//` starts a
            // host and `@` ends a user and a password, which a socket has not.
            // `///PATH` has an empty host, as `unix:///PATH` of other tools.
            if (path.starts_with("//") && !path.starts_with("///")) || path.contains('@') {
                return Err(format!(
                    "a proxy on a Unix socket has no host, no user and no password (no // and no @ after unix:): \
                     {scheme}+unix:/PATH"
                ));
            }
            if path.chars().any(char::is_control) {
                return Err("the Unix socket path of a proxy holds a control character".into());
            }
            return Proxy::elsewhere(scheme, Carrier::Unix(PathBuf::from(path)), http_port);
        }
        let (scheme, rest) = match spec.split_once("://") {
            Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
            None => ("http".to_string(), spec),
        };
        let (kind, default_port) = match scheme.as_str() {
            "http" => (ProxyKind::Http, http_port),
            "socks5" => (ProxyKind::Socks5 { remote_dns: false }, SOCKS_PORT),
            "socks5h" => (ProxyKind::Socks5 { remote_dns: true }, SOCKS_PORT),
            "https" => {
                return Err("https:// proxies (TLS to the proxy) are not supported; HTTP CONNECT goes to an http:// \
                            proxy, and SOCKS5 to a socks5:// or socks5h:// one"
                    .into());
            }
            "socks4" | "socks4a" => {
                return Err(format!(
                    "SOCKS4 is not supported ({scheme}://): it has no authentication, no IPv6 and, in its first \
                     form, no host names; SOCKS5 goes to a socks5:// or socks5h:// proxy, and HTTP CONNECT to an \
                     http:// one"
                ));
            }
            other => {
                return Err(format!(
                    "the proxy scheme {other}:// is not supported; HTTP CONNECT goes to an http:// proxy, and SOCKS5 \
                     to a socks5:// or socks5h:// one"
                ));
            }
        };
        let rest = rest.trim_end_matches('/');
        if rest.contains(['/', '?', '#']) {
            return Err("a proxy URL has no path, query or fragment (percent-encode / ? # @ : in the user and the \
                        password, for example %2F for /)"
                .into());
        }
        let (credentials, hostport) = match rest.rsplit_once('@') {
            // Split before decoding: a `:` escaped in the user is the user's.
            Some((userinfo, hostport)) => {
                let (user, password) = userinfo.split_once(':').unwrap_or((userinfo, ""));
                (Some((percent_decode(user)?, percent_decode(password)?)), hostport)
            }
            None => (None, rest),
        };
        let (host, port) = split_host_port(hostport)?;
        let port = port.unwrap_or(default_port);
        if host.is_empty() {
            return Err("the proxy URL names no host".into());
        }
        if port == 0 {
            return Err("the proxy port is 0".into());
        }
        // A name or an address holds none of these, and `url()` could not
        // write such a host back in a form that parses (the fuzz target
        // `proxy_url` found `]_::`). A `%` is an IPv6 zone only.
        let v6 = host.contains(':');
        let odd = |c: char| {
            matches!(c, '[' | ']' | '@' | '/' | '?' | '#' | '\\')
                || (c == '%' && !v6)
                || c.is_whitespace()
                || c.is_control()
        };
        if let Some(c) = host.chars().find(|c| odd(*c)) {
            return Err(format!("the proxy URL's host holds {c:?}, which no host name or address holds"));
        }
        let address = host.split('%').next().unwrap_or_default();
        if hostport.starts_with('[') && address.parse::<std::net::Ipv6Addr>().is_err() {
            return Err("the proxy host in brackets is not an IPv6 address".into());
        }
        // Only an IPv6 address has a colon. A name with one took the default
        // port, and `url()` wrote it in brackets, which do not parse back
        // (the fuzz target `proxy_url` found `pr:xy:8080`).
        if v6 && address.parse::<std::net::Ipv6Addr>().is_err() {
            return Err("the proxy host has a colon but is not an IPv6 address; write HOST:PORT or [IPV6]:PORT".into());
        }
        if let (ProxyKind::Socks5 { .. }, Some((user, password))) = (kind, &credentials) {
            // RFC 1929 gives each one a length byte, and 0 is no length.
            for (what, value) in [("user", user), ("password", password)] {
                if value.is_empty() || value.len() > 255 {
                    return Err(format!("the SOCKS5 {what} has {} bytes; RFC 1929 takes 1 to 255", value.len()));
                }
            }
        }
        Ok(Proxy { host, port, kind, credentials, carrier: Carrier::Tcp })
    }

    /// A proxy of `scheme` that `carrier` reaches: no host, no port.
    fn elsewhere(scheme: &str, carrier: Carrier, http_port: u16) -> Result<Proxy, String> {
        let mut proxy = Proxy::parse_with(&format!("{scheme}://elsewhere.invalid"), false, http_port)?;
        proxy.host.clear();
        proxy.port = 0;
        proxy.carrier = carrier;
        Ok(proxy)
    }

    /// The parts of the proxy URL `spec` that are secret, for a consumer
    /// that hides them in each line that it writes: the user and password
    /// text, the password as written and percent-decoded. A URL with no user
    /// part has no secret, so that the URL itself stays in a message. The
    /// command of a `+exec:` proxy is secret as a whole.
    pub fn secret_parts(spec: &str) -> Vec<String> {
        let spec = spec.trim();
        if let Some((_, command)) = spec.split_once("+exec:") {
            return vec![spec.to_string(), command.trim().to_string()];
        }
        let rest = spec.split_once("://").map_or(spec, |(_, rest)| rest);
        let authority = rest.split('/').next().unwrap_or(rest);
        let Some((userinfo, _)) = authority.rsplit_once('@') else {
            return Vec::new();
        };
        let mut parts = vec![userinfo.to_string()];
        if let Some((_, password)) = userinfo.split_once(':') {
            parts.push(password.to_string());
            if let Ok(decoded) = percent_decode(password) {
                parts.push(decoded);
            }
        }
        parts
    }
}

/// Each byte but the unreserved ones of RFC 3986 as a `%` escape.
pub(super) fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Decode `%XX` escapes in proxy credentials, and in the user of a second
/// proxy's URL. A bad escape, or bytes that are not UTF-8, are an error: a
/// password changed by a bad escape would get a 407 with no cause.
pub fn percent_decode(s: &str) -> Result<String, String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3).ok_or("a % escape is cut short in the proxy credentials")?;
            // Two hex digits, and nothing that `from_str_radix` also takes
            // (a sign).
            let digit = |b: u8| (b as char).to_digit(16);
            let (Some(high), Some(low)) = (digit(hex[0]), digit(hex[1])) else {
                return Err("a % escape in the proxy credentials is not hex".into());
            };
            out.push((high * 16 + low) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "the proxy credentials are not UTF-8".to_string())
}

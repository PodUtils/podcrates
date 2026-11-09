//! The targets of a request, read strictly. A target that two readers could
//! read two ways is refused (400): a check reads one host and one path, and
//! the far end must read the same. The broker refuses userinfo, a host in
//! percent-encoding, an IPv4 address in another spelling (`2130706433`,
//! `0177.0.0.1`, `127.1`), an IPv6 address with no brackets, a port out of
//! range, an encoded dot or slash in a path (`%2e`, `%2f`), a dot segment
//! and an empty segment (`//`).

use std::fmt;
use std::net::{IpAddr, Ipv6Addr};

/// Why a target was refused: the words of a 400.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bad(pub String);

impl fmt::Display for Bad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn bad<T>(words: impl Into<String>) -> Result<T, Bad> {
    Err(Bad(words.into()))
}

/// A host: a name in lower case with no dot at its end, or an address.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Host {
    Name(String),
    Ip(IpAddr),
}

impl Host {
    /// The host as a URL or a CONNECT line writes it: an IPv6 address in
    /// brackets.
    pub fn uri_text(&self) -> String {
        match self {
            Host::Name(name) => name.clone(),
            Host::Ip(IpAddr::V6(ip)) => format!("[{ip}]"),
            Host::Ip(ip) => ip.to_string(),
        }
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Host::Name(name) => f.write_str(name),
            Host::Ip(ip) => write!(f, "{ip}"),
        }
    }
}

/// A host and a port.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Authority {
    pub host: Host,
    pub port: u16,
}

impl fmt::Display for Authority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host.uri_text(), self.port)
    }
}

/// The target of a request in absolute form (`http://HOST[:PORT]/PATH`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpTarget {
    pub authority: Authority,
    /// The path in its normal form: each unreserved character decoded, each
    /// other escape in upper case.
    pub path: String,
    /// The query with no `?`, as sent; `None`: no `?`.
    pub query: Option<String>,
}

impl HttpTarget {
    /// The request-target in origin form for the far end: the checked path
    /// and the query.
    pub fn origin_form(&self) -> String {
        match &self.query {
            Some(query) => format!("{}?{query}", self.path),
            None => self.path.clone(),
        }
    }
}

/// A host of a target or of a rule: a name, an IPv4 address in its one
/// spelling, or an IPv6 address in brackets. A name is made lower case, and
/// one dot at its end is taken away.
pub fn parse_host(text: &str) -> Result<Host, Bad> {
    if text.is_empty() {
        return bad("the host is empty");
    }
    if let Some(inner) = text.strip_prefix('[') {
        let Some(inner) = inner.strip_suffix(']') else { return bad(format!("{text:?}: '[' is not closed")) };
        return match inner.parse::<Ipv6Addr>() {
            Ok(ip) => Ok(Host::Ip(IpAddr::V6(ip))),
            Err(_) => bad(format!("{text:?}: brackets hold an IPv6 address, with no zone")),
        };
    }
    if text.contains('@') {
        return bad("a target has no user information (@)");
    }
    if text.contains('%') {
        return bad("a host in percent-encoding is ambiguous");
    }
    if text.contains(':') {
        return bad(format!("{text:?}: an IPv6 address goes in brackets"));
    }
    if !text.is_ascii() {
        return bad(format!("{text:?}: give the name in its ASCII form (xn--)"));
    }
    let lower = text.to_ascii_lowercase();
    let name = lower.strip_suffix('.').unwrap_or(&lower);
    if name.is_empty() || name.len() > 253 {
        return bad(format!("{text:?} is not a host name"));
    }
    if let Some(c) = name.chars().find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))) {
        return bad(format!("{text:?} is not a host name ({c:?} is not allowed)"));
    }
    let labels: Vec<&str> = name.split('.').collect();
    if labels.iter().any(|l| l.is_empty() || l.len() > 63) {
        return bad(format!("{text:?} has an empty or a long label"));
    }
    // A URL reader takes a host whose last label is a number as IPv4
    // (WHATWG URL), in decimal, octal or hex: one spelling only is let in.
    let last = labels[labels.len() - 1];
    if is_number(last) {
        return match name.parse::<std::net::Ipv4Addr>() {
            Ok(ip) => Ok(Host::Ip(IpAddr::V4(ip))),
            Err(_) => bad(format!("{text:?}: an IPv4 address is four decimal parts, with no leading zero")),
        };
    }
    Ok(Host::Name(name.to_string()))
}

fn is_number(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    match lower.strip_prefix("0x") {
        Some(hex) => hex.chars().all(|c| c.is_ascii_hexdigit()),
        None => lower.chars().all(|c| c.is_ascii_digit()),
    }
}

/// A port: one to five digits with no leading zero, from 1 to 65535.
pub fn parse_port(text: &str) -> Result<u16, Bad> {
    let fine =
        !text.is_empty() && text.len() <= 5 && text.bytes().all(|b| b.is_ascii_digit()) && !text.starts_with('0');
    match text.parse::<u16>() {
        Ok(port) if fine && port != 0 => Ok(port),
        _ => bad(format!("{text:?} is not a port from 1 to 65535")),
    }
}

/// `HOST:PORT` or `[V6]:PORT`; with `default`, the port may be left out.
pub fn parse_authority(text: &str, default: Option<u16>) -> Result<Authority, Bad> {
    if text.contains('@') {
        return bad("a target has no user information (@)");
    }
    let (host, port) = if text.starts_with('[') {
        match text.find(']') {
            Some(end) => (&text[..=end], text[end + 1..].strip_prefix(':')),
            None => return bad(format!("{text:?}: '[' is not closed")),
        }
    } else {
        match text.rsplit_once(':') {
            Some((host, _)) if host.contains(':') => return bad(format!("{text:?}: an IPv6 address goes in brackets")),
            Some((host, port)) => (host, Some(port)),
            None => (text, None),
        }
    };
    if text.starts_with('[') && port.is_none() && text.len() > host.len() {
        return bad(format!("{text:?}: a port comes after ':'"));
    }
    let port = match (port, default) {
        (Some(port), _) => parse_port(port)?,
        (None, Some(port)) => port,
        (None, None) => return bad(format!("{text:?} names no port: CONNECT takes HOST:PORT")),
    };
    Ok(Authority { host: parse_host(host)?, port })
}

/// A request-target in absolute form, `http://` only. The port is 80 when
/// the URL names none.
pub fn parse_absolute(text: &str) -> Result<HttpTarget, Bad> {
    let Some((scheme, rest)) = text.split_once("://") else {
        return bad("a proxy takes a request-target in absolute form (http://HOST/PATH), or CONNECT");
    };
    if !scheme.eq_ignore_ascii_case("http") {
        return bad(format!("{scheme:?}: plain requests are http://; use CONNECT for another scheme"));
    }
    if text.contains('#') {
        return bad("a request-target has no fragment (#)");
    }
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let authority = parse_authority(&rest[..end], Some(80))?;
    let rest = &rest[end..];
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query.to_string())),
        None => (rest, None),
    };
    let path = if path.is_empty() { "/".to_string() } else { normal_path(path)? };
    Ok(HttpTarget { authority, path, query })
}

/// A path in its normal form: it starts with `/`, has no empty segment, no
/// dot segment and no backslash; an escape of an unreserved character is
/// decoded, an escape of a dot, a slash, a backslash or a control byte is
/// refused, and each other escape is written in upper case.
pub fn normal_path(path: &str) -> Result<String, Bad> {
    if !path.starts_with('/') {
        return bad(format!("{path:?}: a path starts with '/'"));
    }
    if path.contains('\\') {
        return bad("a backslash in a path is ambiguous");
    }
    let bytes = path.as_bytes();
    let mut out = String::with_capacity(path.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b != b'%' {
            out.push(char::from(b));
            i += 1;
            continue;
        }
        let hex = path.get(i + 1..i + 3).filter(|h| h.bytes().all(|c| c.is_ascii_hexdigit()));
        let Some(hex) = hex else { return bad("a '%' in a path is not followed by two hex digits") };
        let byte = u8::from_str_radix(hex, 16).map_err(|_| Bad("a bad escape".into()))?;
        match byte {
            b'.' | b'/' | b'\\' => return bad(format!("%{hex} in a path is ambiguous")),
            0..=0x1f | 0x7f => return bad(format!("%{hex}: a control byte in a path")),
            b if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'~') => out.push(char::from(b)),
            _ => out.push_str(&format!("%{}", hex.to_ascii_uppercase())),
        }
        i += 3;
    }
    for segment in out.split('/').skip(1) {
        if segment == "." || segment == ".." {
            return bad("a dot segment in a path is ambiguous");
        }
    }
    if out.contains("//") {
        return bad("an empty segment (//) in a path is ambiguous");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_made_lower_case_with_no_last_dot() {
        assert_eq!(parse_host("API.Example.COM."), Ok(Host::Name("api.example.com".into())));
        assert_eq!(parse_host("10.1.2.3"), Ok(Host::Ip("10.1.2.3".parse().unwrap())));
        assert_eq!(parse_host("[::1]"), Ok(Host::Ip("::1".parse().unwrap())));
        for bad in ["", "a..b", ".a", "a b", "[::1", "[fe80::1%25eth0]", "::1", "a@b", "a%2eb", "é.example"] {
            assert!(parse_host(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_port_has_one_spelling() {
        assert_eq!(parse_port("443"), Ok(443));
        for bad in ["", "0", "0443", "+443", "65536", "123456", "4 3"] {
            assert!(parse_port(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_origin_form_keeps_the_query() {
        let target = parse_absolute("http://a.example/x%7e?q=%2e").unwrap();
        assert_eq!((target.path.as_str(), target.origin_form().as_str()), ("/x~", "/x~?q=%2e"));
        assert_eq!(parse_absolute("http://a.example?q").unwrap().origin_form(), "/?q");
    }
}

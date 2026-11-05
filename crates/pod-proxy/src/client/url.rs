//! The parts of an `http://` or `https://` URL that a request needs.

/// The parts of a URL that a request needs.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub tls: bool,
    pub host: String,
    pub port: u16,
    /// The path and the query, from `/`.
    pub path: String,
}

/// The parts of `url`. A user part is refused (the client sends none), and
/// so is a space or a control character, as the host and the path go into
/// the request head as they are.
pub(crate) fn target(url: &str) -> Result<Target, String> {
    let (tls, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err("the URL is not http:// or https://".into());
    };
    // The fragment is not sent, and it can come before a path.
    let rest = rest.split('#').next().unwrap_or(rest);
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_string()),
        Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
        None => (rest, "/".to_string()),
    };
    let path = path.split('#').next().unwrap_or("/").to_string();
    if authority.contains('@') {
        return Err("the URL has a user part, which the client does not send".into());
    }
    let default = if tls { 443 } else { 80 };
    let port_of = |p: &str| p.parse::<u16>().map_err(|_| format!("the port of the URL is not a port: {p:?}"));
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (host, tail) = v6.split_once(']').ok_or("no ] in the URL")?;
        let port = match tail.strip_prefix(':') {
            Some(p) => port_of(p)?,
            None => default,
        };
        (host.to_string(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), port_of(p)?),
            None => (authority.to_string(), default),
        }
    };
    if host.is_empty() {
        return Err("the URL has no host".into());
    }
    // The host and the path go in the request head as they are.
    if !host.bytes().chain(path.bytes()).all(|b| b > b' ' && b != 0x7f) {
        return Err("the URL has a space or a control character".into());
    }
    Ok(Target { tls, host, port, path })
}

/// The URL with no query and no fragment, for the words of a failure: a
/// query can hold a token.
pub(crate) fn without_query(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(
            target("https://api.trycloudflare.com/tunnel").unwrap(),
            Target { tls: true, host: "api.trycloudflare.com".into(), port: 443, path: "/tunnel".into() }
        );
        let t = target("http://[::1]:8080?x=1#frag").unwrap();
        assert_eq!((t.host.as_str(), t.port, t.path.as_str()), ("::1", 8080, "/?x=1"));
        assert!(target("ftp://x").is_err());
        assert!(target("https://user:pw@x/").is_err());
    }

    /// A fragment after the host is not part of the host, and a control
    /// character or a space in the URL is an error.
    #[test]
    fn a_fragment_is_not_part_of_the_host() {
        let t = target("https://host#frag").unwrap();
        assert_eq!((t.host.as_str(), t.port, t.path.as_str()), ("host", 443, "/"));
        let t = target("https://host:8443#f/x").unwrap();
        assert_eq!((t.host.as_str(), t.port, t.path.as_str()), ("host", 8443, "/"));
        for bad in ["https://ho\rst/", "https://host/a b", "https://host/a\r\nX: y", "http://host/\x7f"] {
            assert!(target(bad).is_err(), "{bad:?}");
        }
    }
}

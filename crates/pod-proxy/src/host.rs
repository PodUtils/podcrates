//! A host and a port as a request line and a URL write them, and the check
//! of a name before it goes into one.

use crate::error::DialError;

/// `host:port`, with brackets around an IPv6 literal.
pub fn authority(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') { format!("[{host}]:{port}") } else { format!("{host}:{port}") }
}

/// The value of the `Host` header of an HTTPS request (RFC 9110, section
/// 7.2): the host with brackets around an IPv6 literal, and the port when it
/// is not 443.
pub fn host_header(host: &str, port: u16) -> String {
    if port != 443 {
        return authority(host, port);
    }
    if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() }
}

/// Refuse a name that would break a request line or a header.
pub fn check_name(host: &str) -> Result<(), DialError> {
    if host.is_empty() {
        return Err(DialError::InvalidTarget("the host name is empty".into()));
    }
    if host.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '?' | '#' | '@')) {
        return Err(DialError::InvalidTarget(format!("{host:?} is not a host name")));
    }
    Ok(())
}

/// Split `host`, `host:port`, `[v6]` or `[v6]:port`. A bare IPv6 literal with
/// no brackets is returned whole, with no port.
pub fn split_host_port(s: &str) -> Result<(String, Option<u16>), String> {
    if let Some(rest) = s.strip_prefix('[') {
        let (host, after) = rest.split_once(']').ok_or_else(|| format!("unterminated [ in {s:?}"))?;
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p.parse::<u16>().map_err(|_| format!("bad port in {s:?}"))?),
            None if after.is_empty() => None,
            None => return Err(format!("unexpected text after ] in {s:?}")),
        };
        return Ok((host.to_string(), port));
    }
    match s.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            let port = port.parse::<u16>().map_err(|_| format!("bad port in {s:?}"))?;
            Ok((host.to_string(), Some(port)))
        }
        _ => Ok((s.to_string(), None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_header_brackets_an_ipv6_literal_on_each_port() {
        assert_eq!(host_header("::1", 443), "[::1]");
        assert_eq!(host_header("::1", 8443), "[::1]:8443");
        assert_eq!(host_header("[::1]", 443), "[::1]");
        assert_eq!(host_header("relay.example", 443), "relay.example");
        assert_eq!(host_header("relay.example", 8443), "relay.example:8443");
        assert_eq!(host_header("192.0.2.1", 443), "192.0.2.1");
        assert_eq!(host_header("2001:db8::1", 443), "[2001:db8::1]");
        assert_eq!(host_header("2001:db8::1", 8443), "[2001:db8::1]:8443");
    }
}

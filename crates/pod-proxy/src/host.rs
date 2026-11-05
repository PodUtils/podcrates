//! A host and a port as a request line and a URL write them, and the check
//! of a name before it goes into one.

use crate::error::DialError;

/// `host:port`, with brackets around an IPv6 literal.
pub fn authority(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') { format!("[{host}]:{port}") } else { format!("{host}:{port}") }
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

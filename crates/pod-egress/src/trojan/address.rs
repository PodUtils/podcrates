//! The address of Trojan's request, as SOCKS5 writes it (RFC 1928, section
//! 5): `0x01` and four bytes, `0x03`, a length and a name, or `0x04` and
//! sixteen bytes; then the port in network order. The client's request
//! ([`super::request`]) writes it, and the accept side ([`super::accept`])
//! reads it: one codec for both.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// The type byte of an IPv4 address.
pub(super) const V4: u8 = 0x01;
/// The type byte of a name.
pub(super) const NAME: u8 = 0x03;
/// The type byte of an IPv6 address.
pub(super) const V6: u8 = 0x04;

/// Why the bytes are no address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Bad {
    /// The type byte is none of the three.
    Type(u8),
    /// A name of no byte, or one that is not UTF-8.
    Name,
}

/// `host` and `port` after `out`: a literal in brackets is an address, as a
/// link writes IPv6. A name has 1 to 255 bytes.
pub(super) fn write(out: &mut Vec<u8>, host: &str, port: u16) -> Result<(), String> {
    let bare = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    match bare.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            out.push(V4);
            out.extend_from_slice(&v4.octets());
        }
        Ok(IpAddr::V6(v6)) => {
            out.push(V6);
            out.extend_from_slice(&v6.octets());
        }
        Err(_) => {
            let len = u8::try_from(bare.len()).map_err(|_| "a name of more than 255 bytes")?;
            if len == 0 {
                return Err("an empty name".into());
            }
            out.push(NAME);
            out.push(len);
            out.extend_from_slice(bare.as_bytes());
        }
    }
    out.extend_from_slice(&port.to_be_bytes());
    Ok(())
}

/// The address at the start of `buf`: the host, the port and the bytes that
/// it took; `None` while `buf` is too short. An IPv6 host has no brackets.
pub(super) fn read(buf: &[u8]) -> Result<Option<(String, u16, usize)>, Bad> {
    let Some(&kind) = buf.first() else { return Ok(None) };
    let (host, at) = match kind {
        V4 => match buf.get(1..5) {
            Some(b) => (Ipv4Addr::from(<[u8; 4]>::try_from(b).unwrap_or_default()).to_string(), 5),
            None => return Ok(None),
        },
        V6 => match buf.get(1..17) {
            Some(b) => (Ipv6Addr::from(<[u8; 16]>::try_from(b).unwrap_or_default()).to_string(), 17),
            None => return Ok(None),
        },
        NAME => {
            let Some(&len) = buf.get(1) else { return Ok(None) };
            if len == 0 {
                return Err(Bad::Name);
            }
            let at = 2 + usize::from(len);
            let Some(name) = buf.get(2..at) else { return Ok(None) };
            (std::str::from_utf8(name).map_err(|_| Bad::Name)?.to_string(), at)
        }
        other => return Err(Bad::Type(other)),
    };
    match buf.get(at..at + 2) {
        Some(port) => Ok(Some((host, u16::from_be_bytes([port[0], port[1]]), at + 2))),
        None => Ok(None),
    }
}

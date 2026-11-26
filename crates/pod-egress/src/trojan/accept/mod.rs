//! The accept side of Trojan: the parser of a request that a server reads
//! from a client (trojan-gfw `docs/protocol.md`; trojan-gfw's
//! `src/session/serversession.cpp`). The request is the 56 bytes of
//! `hex(SHA224(password))`, CRLF, the command, the address as SOCKS5 writes
//! it, the port, CRLF. The server feeds the bytes that came to [`parse`]
//! until it gives a request; an error says whether the fallback answer is for
//! a wrong password or for bytes that no Trojan client wrote. The codec of
//! the address is the client's (`address`).

use std::fmt;

use crate::trojan::address::{self, Bad};
use crate::trojan::link::Password;
use crate::trojan::{CONNECT, UDP_ASSOCIATE};

/// The longest request: the hash, CRLF, the command, a type, a length, a
/// name of 255 bytes, the port, CRLF. A server needs no larger buffer to
/// tell a request from other bytes.
pub const MAX_REQUEST: usize = 56 + 2 + 1 + 1 + 1 + 255 + 2 + 2;

/// What the client asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// A stream to the target (`0x01`).
    Connect,
    /// UDP associate (`0x03`): the datagrams of the client go to the target.
    UdpAssociate,
}

/// A request that the password of the server opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub command: Command,
    /// A name, or an address as text; an IPv6 address has no brackets.
    pub host: String,
    pub port: u16,
    /// The bytes of the request: the payload starts after them.
    pub used: usize,
}

/// Why the bytes are not a request that the password opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestError {
    /// The bytes are a Trojan request of another password.
    WrongPassword,
    /// The bytes are no Trojan request: what is wrong with them.
    NotTrojan(Malformed),
}

/// What is wrong with bytes that are no Trojan request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformed {
    /// The first 56 bytes are not the hex digits of a hash.
    Hash,
    /// No CRLF after the hash.
    HashEnd,
    /// The command is none of `0x01` and `0x03`.
    Command(u8),
    /// The type of the address is none of `0x01`, `0x03` and `0x04`.
    AddressType(u8),
    /// A name of no byte, or one that is not UTF-8.
    Name,
    /// No CRLF after the port.
    End,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RequestError::WrongPassword => f.write_str("the request holds the hash of another password"),
            RequestError::NotTrojan(Malformed::Hash) => f.write_str("the first 56 bytes are not a hash"),
            RequestError::NotTrojan(Malformed::HashEnd) => f.write_str("no CRLF after the hash"),
            RequestError::NotTrojan(Malformed::Command(c)) => {
                write!(f, "the command {c:#04x} is not CONNECT or UDP ASSOCIATE")
            }
            RequestError::NotTrojan(Malformed::AddressType(t)) => {
                write!(f, "the address type {t:#04x} is not 1, 3 or 4")
            }
            RequestError::NotTrojan(Malformed::Name) => f.write_str("the name of the address is empty or not UTF-8"),
            RequestError::NotTrojan(Malformed::End) => f.write_str("no CRLF after the port"),
        }
    }
}

impl std::error::Error for RequestError {}

/// The request at the front of `buf`: `Ok(None)` while the bytes are too
/// short to tell, an error as soon as they are not a request of `password`.
/// The hash is compared in constant time. A request is at most
/// [`MAX_REQUEST`] bytes, so a server that holds that many bytes with no
/// request holds an error.
pub fn parse(buf: &[u8], password: &Password) -> Result<Option<Request>, RequestError> {
    let Some(hash) = buf.get(..56) else { return Ok(None) };
    if !password.matches(hash) {
        let hex = hash.iter().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b));
        return Err(if hex { RequestError::WrongPassword } else { RequestError::NotTrojan(Malformed::Hash) });
    }
    let rest = &buf[56..];
    match crlf(rest) {
        Some(true) => {}
        Some(false) => return Ok(None),
        None => return Err(RequestError::NotTrojan(Malformed::HashEnd)),
    }
    let Some(&command) = rest.get(2) else { return Ok(None) };
    let command = match command {
        CONNECT => Command::Connect,
        UDP_ASSOCIATE => Command::UdpAssociate,
        other => return Err(RequestError::NotTrojan(Malformed::Command(other))),
    };
    let after = &rest[3..];
    let (host, port, taken) = match address::read(after) {
        Ok(Some(address)) => address,
        Ok(None) => return Ok(None),
        Err(Bad::Type(t)) => return Err(RequestError::NotTrojan(Malformed::AddressType(t))),
        Err(Bad::Name) => return Err(RequestError::NotTrojan(Malformed::Name)),
    };
    match crlf(&after[taken..]) {
        Some(true) => Ok(Some(Request { command, host, port, used: 56 + 2 + 1 + taken + 2 })),
        Some(false) => Ok(None),
        None => Err(RequestError::NotTrojan(Malformed::End)),
    }
}

/// Whether `bytes` start with CRLF: `Some(true)` when they do, `Some(false)`
/// while they are a part of it, `None` when they are not.
fn crlf(bytes: &[u8]) -> Option<bool> {
    match bytes {
        [b'\r', b'\n', ..] => Some(true),
        [b'\r'] | [] => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

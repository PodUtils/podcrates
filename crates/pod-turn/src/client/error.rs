//! The errors and the events of a TURN client. A decode error names the
//! type of the message and the offset; a server's error names the method
//! and the code.

use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use pod_core::{Class, Failure, Stage};

use crate::stun::{
    ALLOCATE, CHANNEL_BIND, CONNECT, CONNECTION_ATTEMPT, CONNECTION_BIND, CREATE_PERMISSION, DATA_METHOD, DecodeError,
    IntegrityError, REFRESH, SEND,
};

/// The name of a method of TURN, for a line of an error.
pub fn method_name(method: u16) -> &'static str {
    match method {
        0x0001 => "Binding",
        ALLOCATE => "Allocate",
        REFRESH => "Refresh",
        SEND => "Send",
        DATA_METHOD => "Data",
        CREATE_PERMISSION => "CreatePermission",
        CHANNEL_BIND => "ChannelBind",
        CONNECT => "Connect",
        CONNECTION_BIND => "ConnectionBind",
        CONNECTION_ATTEMPT => "ConnectionAttempt",
        _ => "unknown",
    }
}

/// Why a TURN client failed.
#[derive(Debug)]
pub enum Error {
    /// A setting or an argument that cannot work.
    Usage(String),
    /// The dial, the TLS handshake or the UDP socket failed.
    Connect(Failure),
    /// A read or a write of the transport failed.
    Io(std::io::Error),
    /// The server ended the TLS stream.
    Closed,
    /// No answer to a request of `method` within `waited`.
    Timeout { method: u16, waited: Duration },
    /// A message of the server that is no STUN message: its type and the
    /// offset where the reading stopped are in the decode error.
    Decode(DecodeError),
    /// The bytes of the TLS stream are neither STUN nor ChannelData: the
    /// stream cannot be read further.
    Frame { offset: usize, why: String },
    /// An answer of the type `kind` that lacks an attribute, or has one
    /// that does not read.
    Protocol { kind: u16, why: String },
    /// An answer of the type `kind` whose integrity does not hold.
    Integrity { kind: u16, error: IntegrityError },
    /// The server answered a request of `method` with an error.
    Server { method: u16, code: u16, reason: String },
    /// The server answered each send of a request of `method` with 438
    /// (Stale Nonce).
    StaleNonce { method: u16, tries: u32 },
    /// The server's realm is not the one of the settings.
    Realm { expected: String, offered: String },
    /// The entropy gave no transaction id.
    NoRandom,
    /// The allocation is closed, or its lifetime ended.
    NoAllocation,
    /// No channel of this number is bound.
    NoChannel(u16),
    /// No permission and no channel for the address of this peer.
    NoPermission(IpAddr),
    /// Each channel number (0x4000 to 0x4FFF) is in use.
    NoChannelNumber,
    /// Data over the 65535 bytes of a DATA attribute or of ChannelData.
    TooLarge(usize),
}

impl Error {
    /// The class of the failure, for the exit code of the consumer.
    pub fn class(&self) -> Class {
        match self {
            Error::Usage(_) | Error::NoChannel(_) | Error::NoPermission(_) | Error::TooLarge(_) => Class::Usage,
            Error::NoChannelNumber => Class::Usage,
            Error::Connect(f) => f.class,
            Error::Io(e) => Class::of_io(e.kind()),
            Error::Closed | Error::NoRandom | Error::NoAllocation => Class::Unavailable,
            Error::Timeout { .. } => Class::Timeout,
            Error::Decode(_) | Error::Frame { .. } | Error::Protocol { .. } | Error::Integrity { .. } => {
                Class::Protocol
            }
            Error::StaleNonce { .. } => Class::Protocol,
            Error::Realm { .. } => Class::Config,
            Error::Server { code, .. } => match code {
                401 | 441 => Class::Refused,
                403 => Class::Policy,
                420 | 440 | 442 | 443 => Class::Unsupported,
                437 => Class::Unavailable,
                486 | 508 => Class::TempFail,
                500..=599 => Class::TempFail,
                _ => Class::Protocol,
            },
        }
    }

    /// The code of a server's error, when it is one.
    pub fn code(&self) -> Option<u16> {
        match self {
            Error::Server { code, .. } => Some(*code),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage(why) => write!(f, "TURN: {why}"),
            Error::Connect(failure) => write!(f, "TURN: {}", failure.chain()),
            Error::Io(e) => write!(f, "TURN: the transport failed: {e}"),
            Error::Closed => f.write_str("TURN: the server ended the stream"),
            Error::Timeout { method, waited } => {
                write!(f, "TURN: no answer to the {} request within {} ms", method_name(*method), waited.as_millis())
            }
            Error::Decode(e) => write!(f, "TURN: the server sent no STUN message: {e}"),
            Error::Frame { offset, why } => write!(f, "TURN: the stream holds no message at byte {offset}: {why}"),
            Error::Protocol { kind, why } => write!(f, "TURN: the answer of type 0x{kind:04x}: {why}"),
            Error::Integrity { kind, error } => write!(f, "TURN: the answer of type 0x{kind:04x}: {error}"),
            Error::Server { method, code, reason } => {
                write!(f, "TURN: the server answered the {} request with {code}", method_name(*method))?;
                if reason.is_empty() { Ok(()) } else { write!(f, " ({})", pod_core::text::one_line(reason)) }
            }
            Error::StaleNonce { method, tries } => {
                write!(
                    f,
                    "TURN: the server answered the {} request {tries} times with 438 (Stale Nonce)",
                    method_name(*method)
                )
            }
            Error::Realm { expected, offered } => write!(
                f,
                "TURN: the server's realm is {:?}, not {:?}",
                pod_core::text::one_line(offered),
                pod_core::text::one_line(expected)
            ),
            Error::NoRandom => f.write_str("TURN: the system gave no random bytes for a transaction id"),
            Error::NoAllocation => f.write_str("TURN: the allocation is closed or expired"),
            Error::NoChannel(n) => write!(f, "TURN: no channel 0x{n:04x} is bound"),
            Error::NoPermission(ip) => write!(f, "TURN: no permission for {ip}; create one first"),
            Error::NoChannelNumber => f.write_str("TURN: each channel number is in use"),
            Error::TooLarge(len) => write!(f, "TURN: {len} bytes of data, more than 65535"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Connect(failure) => Some(failure),
            Error::Io(e) => Some(e),
            Error::Decode(e) => Some(e),
            Error::Integrity { error, .. } => Some(error),
            _ => None,
        }
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        let class = e.class();
        match e {
            Error::Connect(failure) => failure,
            other => Failure::caused(Stage::Relay, class, other.to_string(), other),
        }
    }
}

/// What the client keeps alive with a refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kept {
    /// The allocation (a Refresh request).
    Allocation,
    /// The permission of a peer's address (a CreatePermission request).
    Permission(IpAddr),
    /// A channel (a ChannelBind request again).
    Channel(u16),
}

/// What [`super::Client::recv`] gives.
#[derive(Debug)]
pub enum Event {
    /// Data of a peer: in a Data indication (`channel` is `None`) or in
    /// ChannelData.
    Data { peer: SocketAddr, channel: Option<u16>, data: Vec<u8> },
    /// A refresh that the client made on its timer failed. A permission or
    /// a channel that failed is no longer refreshed; the allocation is
    /// tried again until its lifetime ends.
    RefreshFailed { kept: Kept, error: Error },
    /// An error answer of the server that no request of this client waits
    /// for.
    ServerError { method: u16, code: u16, reason: String },
}

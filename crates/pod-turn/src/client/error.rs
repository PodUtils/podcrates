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
    /// The server ended the control stream.
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
    /// The server's realm is not the one of the settings, or after a
    /// redirect not the one of the first server.
    Realm { expected: String, offered: String },
    /// The server redirected a request of `method` to `to` (300 Try
    /// Alternate), and the client did not follow: the client follows one
    /// redirect of the Allocate alone, to another server. `domain` is the
    /// ALTERNATE-DOMAIN of an answer signed with MESSAGE-INTEGRITY-SHA256.
    Alternate { method: u16, to: SocketAddr, domain: Option<String> },
    /// The Allocate that followed the redirect to `to` failed: the way out
    /// of the caller refused it, or the server there failed it.
    Redirected { to: SocketAddr, error: Box<Error> },
    /// The nonce of a challenge to a request of `method` announces
    /// PASSWORD-ALGORITHMS, and the challenge holds none (RFC 8489, 9.2.5): a
    /// party on the path took the attribute out to force MD5.
    Downgrade { method: u16 },
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
    /// The allocation relays UDP, not TCP: RFC 6062 needs a TCP allocation.
    NoTcpAllocation,
    /// The server refused a Connect request with `code`.
    ConnectRefused { code: u16, reason: String },
    /// No ConnectionAttempt indication came within `waited`.
    ConnectionAttemptTimeout { waited: Duration },
    /// A success or an indication that lacks a whole 32-bit CONNECTION-ID.
    BadConnectionId { why: String },
    /// The server refused a ConnectionBind request with `code`.
    BindRefused { code: u16, reason: String },
    /// A frame of the relayed stream does not read: the length is cut, or
    /// the stream ended inside the data that it names.
    MalformedFrame { why: String },
    /// The server closed the relayed stream between two frames.
    RelayedClosed,
}

impl Error {
    /// The class of the failure, for the exit code of the consumer.
    pub fn class(&self) -> Class {
        match self {
            Error::Usage(_) | Error::NoChannel(_) | Error::NoPermission(_) | Error::TooLarge(_) => Class::Usage,
            Error::NoChannelNumber | Error::NoTcpAllocation => Class::Usage,
            Error::Connect(f) => f.class,
            Error::Io(e) => Class::of_io(e.kind()),
            Error::Closed | Error::NoRandom | Error::NoAllocation | Error::RelayedClosed => Class::Unavailable,
            Error::Timeout { .. } | Error::ConnectionAttemptTimeout { .. } => Class::Timeout,
            Error::Decode(_)
            | Error::Frame { .. }
            | Error::Protocol { .. }
            | Error::Integrity { .. }
            | Error::MalformedFrame { .. }
            | Error::BadConnectionId { .. } => Class::Protocol,
            Error::StaleNonce { .. } | Error::Downgrade { .. } => Class::Protocol,
            Error::Alternate { .. } => Class::Unavailable,
            Error::Redirected { error, .. } => error.class(),
            Error::Realm { .. } => Class::Config,
            Error::Server { code, .. } | Error::ConnectRefused { code, .. } | Error::BindRefused { code, .. } => {
                server_class(*code)
            }
        }
    }

    /// The code of a server's error, when it is one.
    pub fn code(&self) -> Option<u16> {
        match self {
            Error::Server { code, .. } | Error::ConnectRefused { code, .. } | Error::BindRefused { code, .. } => {
                Some(*code)
            }
            Error::Alternate { .. } => Some(300),
            Error::Redirected { error, .. } => error.code(),
            _ => None,
        }
    }
}

/// The class of a server's error code. 447 is a timeout of the server's
/// connection to the peer; 446 says the connection exists already.
fn server_class(code: u16) -> Class {
    match code {
        401 | 441 => Class::Refused,
        403 => Class::Policy,
        420 | 440 | 442 | 443 | 446 => Class::Unsupported,
        437 => Class::Unavailable,
        447 => Class::Timeout,
        486 | 508 => Class::TempFail,
        500..=599 => Class::TempFail,
        _ => Class::Protocol,
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
                // coturn sends no reason phrase by default: the RFC's name says what the code is.
                match (reason.is_empty(), code_name(*code)) {
                    (false, _) => write!(f, " ({})", pod_core::text::one_line(reason)),
                    (true, Some(name)) => write!(f, " ({name})"),
                    (true, None) => Ok(()),
                }
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
            Error::Alternate { method, to, .. } => write!(
                f,
                "TURN: the server redirected the {} request to {to} (300 Try Alternate); \
                 the client follows one redirect of an Allocate, to another server",
                method_name(*method)
            ),
            Error::Redirected { to, error } => {
                let inner = error.to_string();
                write!(f, "TURN: the redirect to {to}: {}", inner.strip_prefix("TURN: ").unwrap_or(&inner))
            }
            Error::Downgrade { method } => write!(
                f,
                "TURN: the nonce of the challenge to the {} request announces PASSWORD-ALGORITHMS, \
                 and the challenge holds none: a downgrade on the path",
                method_name(*method)
            ),
            Error::NoRandom => f.write_str("TURN: the system gave no random bytes for a transaction id"),
            Error::NoAllocation => f.write_str("TURN: the allocation is closed or expired"),
            Error::NoChannel(n) => write!(f, "TURN: no channel 0x{n:04x} is bound"),
            Error::NoPermission(ip) => write!(f, "TURN: no permission for {ip}; create one first"),
            Error::NoChannelNumber => f.write_str("TURN: each channel number is in use"),
            Error::TooLarge(len) => write!(f, "TURN: {len} bytes of data, more than 65535"),
            Error::NoTcpAllocation => f.write_str("TURN: the allocation relays UDP, not TCP (RFC 6062)"),
            Error::ConnectRefused { code, reason } => {
                write!(f, "TURN: the server refused the Connect request with {code}")?;
                error_reason(f, *code, reason)
            }
            Error::ConnectionAttemptTimeout { waited } => {
                write!(f, "TURN: no ConnectionAttempt indication within {} ms", waited.as_millis())
            }
            Error::BadConnectionId { why } => write!(f, "TURN: CONNECTION-ID: {why}"),
            Error::BindRefused { code, reason } => {
                write!(f, "TURN: the server refused the ConnectionBind request with {code}")?;
                error_reason(f, *code, reason)
            }
            Error::MalformedFrame { why } => write!(f, "TURN: the relayed stream holds no frame: {why}"),
            Error::RelayedClosed => f.write_str("TURN: the server closed the relayed stream"),
        }
    }
}

/// The reason of an error answer after its code, or the RFC's name when
/// the server sent no reason.
fn error_reason(f: &mut fmt::Formatter<'_>, code: u16, reason: &str) -> fmt::Result {
    match (reason.is_empty(), code_name(code)) {
        (false, _) => write!(f, " ({})", pod_core::text::one_line(reason)),
        (true, Some(name)) => write!(f, " ({name})"),
        (true, None) => Ok(()),
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Connect(failure) => Some(failure),
            Error::Io(e) => Some(e),
            Error::Decode(e) => Some(e),
            Error::Integrity { error, .. } => Some(error),
            Error::Redirected { error, .. } => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        let class = e.class();
        match e {
            Error::Connect(failure) => failure,
            // The stage of the way out stays: a refused redirect is a dial's failure.
            Error::Redirected { ref error, .. } => {
                let stage = if let Error::Connect(failure) = error.as_ref() { failure.stage } else { Stage::Relay };
                Failure::caused(stage, class, e.to_string(), e)
            }
            other => Failure::caused(Stage::Relay, class, other.to_string(), other),
        }
    }
}

/// The name of an error code of RFC 8489 (14.8) and RFC 8656 (18), for an
/// answer with no reason phrase.
fn code_name(code: u16) -> Option<&'static str> {
    Some(match code {
        300 => "Try Alternate",
        400 => "Bad Request",
        401 => "Unauthenticated",
        403 => "Forbidden",
        420 => "Unknown Attribute",
        437 => "Allocation Mismatch",
        438 => "Stale Nonce",
        440 => "Address Family not Supported",
        441 => "Wrong Credentials",
        446 => "Connection Already Exists",
        447 => "Connection Timeout or Failure",
        442 => "Unsupported Transport Protocol",
        443 => "Peer Address Family Mismatch",
        486 => "Allocation Quota Reached",
        500 => "Server Error",
        508 => "Insufficient Capacity",
        _ => return None,
    })
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

/// What [`crate::Client::recv`] gives.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_error_with_no_reason_names_its_code() {
        let e = |code: u16, reason: &str| Error::Server { method: ALLOCATE, code, reason: reason.into() }.to_string();
        assert_eq!(e(437, ""), "TURN: the server answered the Allocate request with 437 (Allocation Mismatch)");
        assert_eq!(e(437, "In use"), "TURN: the server answered the Allocate request with 437 (In use)");
        assert_eq!(e(499, ""), "TURN: the server answered the Allocate request with 499");
        let refresh = Error::Server { method: REFRESH, code: 441, reason: String::new() };
        assert!(refresh.to_string().ends_with("with 441 (Wrong Credentials)"), "{refresh}");
    }

    /// A refused redirect keeps the words and the stage of the way out.
    #[test]
    fn a_refused_redirect_keeps_the_words_and_the_stage_of_the_way_out() {
        let to: SocketAddr = "192.0.2.7:3478".parse().unwrap();
        let refused = Failure::new(Stage::Connect, Class::Policy, "the policy refuses 192.0.2.7:3478");
        let e = Error::Redirected { to, error: Box::new(Error::Connect(refused)) };
        assert_eq!(e.to_string(), "TURN: the redirect to 192.0.2.7:3478: the policy refuses 192.0.2.7:3478");
        assert_eq!(e.class(), Class::Policy);
        let failure = Failure::from(e);
        assert_eq!((failure.stage, failure.class), (Stage::Connect, Class::Policy));
        let again = Error::Alternate { method: ALLOCATE, to, domain: None };
        assert_eq!(again.code(), Some(300));
        let twice = Error::Redirected { to, error: Box::new(again) };
        assert!(
            twice.to_string().contains(": the server redirected the Allocate request to 192.0.2.7:3478"),
            "{twice}"
        );
        assert_eq!(twice.code(), Some(300));
    }
}

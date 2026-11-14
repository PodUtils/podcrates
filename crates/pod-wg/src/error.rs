//! The errors of the handshake, the cookie and the session. Each [`Error`]
//! converts to a [`Failure`] of `pod-core` at the stage `Warp` (WireGuard
//! carries WARP for each consumer now); a caller of another tunnel moves it
//! with [`Failure::at`]. The texts are podnet's.

use std::fmt;
use std::time::Duration;

use pod_core::{Class, Failure, Stage};

/// What went wrong.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Not a handshake response: another type or another length.
    NotResponse,
    /// A response to another initiation (its receiver index).
    OtherHandshake,
    /// The mac1 of a response is not the MAC of its bytes with this key.
    Mac1,
    /// The empty payload of a response does not authenticate.
    ResponseNotAuthentic,
    /// Not a cookie reply: another type or another length.
    NotCookie,
    /// The cookie of a reply does not authenticate: it is not for the
    /// mac1 of this initiation, or not from this peer.
    CookieNotAuthentic,
    /// Not a transport message: another type, or shorter than its header
    /// and its tag.
    NotTransport,
    /// A transport message for another session (its receiver index).
    OtherSession,
    /// A transport message that does not authenticate.
    NotAuthentic,
    /// A transport message whose counter came before, or is older than the
    /// window.
    Replayed(u64),
    /// A rekey had no answer in this time: the path is dead.
    Dead(Duration),
    /// The system gave no random bytes for a key or an index.
    NoRandom,
    /// An initiation of another length.
    NotInitiation,
    /// A message that a responder does not take: a response, a cookie
    /// reply, another type.
    NotForResponder,
    /// The mac1 of an initiation is not the MAC of its bytes with the key of
    /// this responder.
    InitiationMac1,
    /// The static key or the timestamp of an initiation does not
    /// authenticate.
    InitiationNotAuthentic,
    /// An initiation of a static key that is not a peer of the responder.
    UnknownPeer,
    /// An initiation whose timestamp is not after the last one of its peer:
    /// a replay.
    OldTimestamp,
    /// An initiation that came too soon after the last one of its peer.
    Flood,
    /// A session past REJECT_AFTER_TIME, or past its count of messages.
    Expired,
    /// No session to the peer: it must make a handshake.
    NoSession,
}

impl Error {
    /// What a caller can do.
    pub fn class(&self) -> Class {
        match self {
            Error::NotResponse
            | Error::OtherHandshake
            | Error::Mac1
            | Error::ResponseNotAuthentic
            | Error::NotCookie
            | Error::CookieNotAuthentic
            | Error::NotTransport
            | Error::OtherSession
            | Error::NotAuthentic
            | Error::Replayed(_)
            | Error::NotInitiation
            | Error::NotForResponder
            | Error::InitiationMac1
            | Error::InitiationNotAuthentic
            | Error::OldTimestamp => Class::Protocol,
            Error::UnknownPeer => Class::Refused,
            Error::Flood => Class::TempFail,
            Error::Dead(_) | Error::Expired => Class::Timeout,
            Error::NoRandom | Error::NoSession => Class::Unavailable,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotResponse => f.write_str("not a WireGuard handshake response"),
            Error::OtherHandshake => f.write_str("a WireGuard response to another handshake"),
            Error::Mac1 => f.write_str("the mac1 of the WireGuard response is wrong"),
            Error::ResponseNotAuthentic => {
                f.write_str("the WireGuard handshake response: a WireGuard message that does not authenticate")
            }
            Error::NotCookie => f.write_str("not a WireGuard cookie reply"),
            Error::CookieNotAuthentic => f.write_str("the WireGuard cookie reply does not authenticate"),
            Error::NotTransport => f.write_str("not a WireGuard transport message"),
            Error::OtherSession => f.write_str("a WireGuard message for another session"),
            Error::NotAuthentic => f.write_str("a WireGuard message that does not authenticate"),
            Error::Replayed(counter) => write!(f, "a WireGuard message that came before (counter {counter})"),
            Error::Dead(after) => write!(f, "no WireGuard handshake answer in {} s", after.as_secs()),
            Error::NoRandom => f.write_str("the system gave no random bytes for WireGuard"),
            Error::NotInitiation => f.write_str("not a WireGuard handshake initiation"),
            Error::NotForResponder => f.write_str("a WireGuard message that a responder does not take"),
            Error::InitiationMac1 => f.write_str("the mac1 of the WireGuard initiation is wrong"),
            Error::InitiationNotAuthentic => f.write_str("a WireGuard initiation that does not authenticate"),
            Error::UnknownPeer => f.write_str("a WireGuard initiation of a key that is not a peer"),
            Error::OldTimestamp => f.write_str("a WireGuard initiation that came before (its timestamp)"),
            Error::Flood => f.write_str("a WireGuard initiation too soon after the last one of its peer"),
            Error::Expired => f.write_str("the WireGuard session is too old"),
            Error::NoSession => f.write_str("no WireGuard session to the peer"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        Failure::caused(Stage::Warp, e.class(), e.to_string(), e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_error_has_the_stage_of_warp_and_a_class() {
        let cases = [
            (Error::NotResponse, Class::Protocol),
            (Error::Mac1, Class::Protocol),
            (Error::CookieNotAuthentic, Class::Protocol),
            (Error::Replayed(5), Class::Protocol),
            (Error::Dead(Duration::from_secs(20)), Class::Timeout),
            (Error::NoRandom, Class::Unavailable),
            (Error::OldTimestamp, Class::Protocol),
            (Error::UnknownPeer, Class::Refused),
            (Error::Flood, Class::TempFail),
            (Error::Expired, Class::Timeout),
            (Error::NoSession, Class::Unavailable),
        ];
        for (e, want) in cases {
            let text = e.to_string();
            let failure: Failure = e.into();
            assert_eq!((failure.stage, failure.class), (Stage::Warp, want), "{text}");
            assert_eq!(failure.chain(), text);
        }
    }
}

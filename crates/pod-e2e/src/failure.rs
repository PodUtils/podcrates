//! Each error of the channel as a [`Failure`] of `pod-core`: the stage
//! `Session`, as for the resumable layer under it, and a class that tells
//! the caller what it can do. The error stays the cause, so that a caller
//! that knows the channel reads its kind.

use pod_core::{Class, Failure, Stage};

use crate::{Error, Refusal};

/// The class of an error of the channel.
pub(crate) fn class(e: &Error) -> Class {
    match e {
        Error::Io(io) => Class::of_io(io.kind()),
        // An end with no channel, or with the channel off: no try with the
        // same two ends works.
        Error::NotChannel(_) => Class::Unsupported,
        Error::Handshake(_) | Error::Tampered | Error::Malformed(_) => Class::Protocol,
        // The road's own reason ended the stream: a new link can work.
        Error::Cut => Class::Unavailable,
        Error::Refused(Refusal::NotAllowed) | Error::Refused(Refusal::Other(_)) => Class::Refused,
        Error::Refused(Refusal::NoTarget(_)) => Class::Unavailable,
        // The identity of the far end failed this end's check (a pin).
        Error::NodeKey(_) => Class::Refused,
        Error::Timeout(_) => Class::Timeout,
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        Failure::caused(Stage::Session, class(&e), e.to_string(), e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_error_has_the_stage_of_the_session_and_a_class() {
        let cases = [
            (Error::NotChannel("\"SSH-2.0\"".into()), Class::Unsupported),
            (Error::Tampered, Class::Protocol),
            (Error::Cut, Class::Unavailable),
            (Error::Refused(Refusal::NotAllowed), Class::Refused),
            (Error::Refused(Refusal::NoTarget("connection refused".into())), Class::Unavailable),
            (Error::NodeKey("a changed key".into()), Class::Refused),
            (Error::Timeout(crate::VERDICT_STEP), Class::Timeout),
            (Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionReset)), Class::Unavailable),
        ];
        for (e, want) in cases {
            let text = e.to_string();
            let failure: Failure = e.into();
            assert_eq!((failure.stage, failure.class), (Stage::Session, want), "{text}");
        }
        let failure: Failure = Error::Io(std::io::Error::from(std::io::ErrorKind::BrokenPipe)).into();
        assert!(failure.io_cause().is_some(), "the stream's error stays the cause");
    }
}

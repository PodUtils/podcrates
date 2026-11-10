//! Each error of the layer as a [`Failure`] of `pod-core`: the stage
//! `Session`, and a class that tells the caller what it can do. The error
//! stays the cause, so that a caller that knows the layer reads its kind.

use pod_core::{Class, Failure, Stage};

use crate::client::ClientError;
use crate::decode::DecodeError;
use crate::far::FarError;
use crate::handover::Unreadable;
use crate::handshake::HandshakeError;
use crate::link::LinkError;
use crate::offset::OffsetError;
use crate::record::{EncodeError, RefuseCode};
use crate::replay::NotKept;
use crate::udp::FrameError;
use crate::udp::flow::NoFlow;

/// The class of a refusal by its code.
fn of_refusal(code: RefuseCode) -> Class {
    match code {
        RefuseCode::BUSY => Class::TempFail,
        RefuseCode::BAD_PROOF => Class::Refused,
        RefuseCode::VERSION | RefuseCode::ROLE => Class::Unsupported,
        RefuseCode::PROTOCOL => Class::Protocol,
        // The far end forgot the session, or no longer keeps its bytes: a
        // new session is the only way on.
        RefuseCode::UNKNOWN_SESSION | RefuseCode::NOT_KEPT => Class::Unavailable,
        _ => Class::Refused,
    }
}

fn of_handshake(e: &HandshakeError) -> Class {
    match e {
        HandshakeError::Refused { code, .. } => of_refusal(*code),
        HandshakeError::Closed(_) => Class::Unavailable,
        HandshakeError::Unexpected(_) | HandshakeError::WeakKey => Class::Protocol,
        HandshakeError::Version(_) | HandshakeError::Role(_) => Class::Unsupported,
        HandshakeError::BadProof => Class::Refused,
        HandshakeError::NoRandom => Class::Unavailable,
    }
}

fn of_link(e: &LinkError) -> Class {
    match e {
        LinkError::Refused { code, .. } => of_refusal(*code),
        LinkError::Decode(_) | LinkError::Offset(_) | LinkError::Unexpected(_) | LinkError::End(_) => Class::Protocol,
        LinkError::Full(_) => Class::Usage,
    }
}

fn session(class: Class, e: impl std::error::Error + Send + Sync + 'static) -> Failure {
    Failure::caused(Stage::Session, class, e.to_string(), e)
}

impl From<HandshakeError> for Failure {
    fn from(e: HandshakeError) -> Failure {
        session(of_handshake(&e), e)
    }
}

impl From<LinkError> for Failure {
    fn from(e: LinkError) -> Failure {
        session(of_link(&e), e)
    }
}

impl From<ClientError> for Failure {
    fn from(e: ClientError) -> Failure {
        let class = match &e {
            ClientError::Io(io) => Class::of_io(io.kind()),
            ClientError::Decode(_) => Class::Protocol,
            ClientError::Handshake(h) => of_handshake(h),
            ClientError::Timeout => Class::Timeout,
        };
        session(class, e)
    }
}

impl From<FarError> for Failure {
    fn from(e: FarError) -> Failure {
        let class = match &e {
            FarError::Io(io) => Class::of_io(io.kind()),
            FarError::Decode(_) => Class::Protocol,
            FarError::Handshake(h) => of_handshake(h),
            FarError::Timeout => Class::Timeout,
            FarError::NoRoom(_) => Class::TempFail,
        };
        session(class, e)
    }
}

impl From<DecodeError> for Failure {
    fn from(e: DecodeError) -> Failure {
        session(Class::Protocol, e)
    }
}

impl From<OffsetError> for Failure {
    fn from(e: OffsetError) -> Failure {
        session(Class::Protocol, e)
    }
}

impl From<NotKept> for Failure {
    fn from(e: NotKept) -> Failure {
        session(Class::Unavailable, e)
    }
}

impl From<EncodeError> for Failure {
    fn from(e: EncodeError) -> Failure {
        session(Class::Usage, e)
    }
}

impl From<Unreadable> for Failure {
    fn from(e: Unreadable) -> Failure {
        session(Class::Config, e)
    }
}

impl From<FrameError> for Failure {
    fn from(e: FrameError) -> Failure {
        session(Class::Protocol, e)
    }
}

impl From<NoFlow> for Failure {
    fn from(e: NoFlow) -> Failure {
        match e {
            // The resolver's own failure, with its stage and its class.
            NoFlow::Resolve(failure) => failure,
            NoFlow::Refused(io) => {
                Failure::caused(Stage::Host, Class::Policy, format!("this host refuses UDP: {io}"), io)
            }
            NoFlow::Socket(io) => {
                let class = Class::of_io(io.kind());
                Failure::caused(Stage::Connect, class, format!("no UDP socket to it: {io}"), io)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_busy_far_end_is_a_later_try_and_a_bad_proof_a_refusal() {
        let busy: Failure = HandshakeError::Refused { code: RefuseCode::BUSY, reason: "full".into() }.into();
        assert_eq!((busy.stage, busy.class), (Stage::Session, Class::TempFail));
        let proof: Failure = HandshakeError::BadProof.into();
        assert_eq!(proof.class, Class::Refused);
        let timeout: Failure = ClientError::Timeout.into();
        assert_eq!(timeout.class, Class::Timeout);
        let gone: Failure = LinkError::Refused { code: RefuseCode::UNKNOWN_SESSION, reason: String::new() }.into();
        assert_eq!(gone.class, Class::Unavailable);
    }

    #[test]
    fn a_host_that_refuses_udp_is_a_policy_of_the_host() {
        let e = NoFlow::Refused(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        let failure: Failure = e.into();
        assert_eq!((failure.stage, failure.class), (Stage::Host, Class::Policy));
        assert!(failure.io_cause().is_some());
    }
}

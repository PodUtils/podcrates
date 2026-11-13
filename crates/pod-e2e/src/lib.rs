//! pod-e2e: the end-to-end channel between two ends of a relay.
//!
//! The channel is Noise XX over the bytes of a session, so that the relay
//! carries ciphertext only, and each end proves its key of `pod-identity`.
//! It runs above the resumable layer of `pod-session`: one handshake for
//! each session, which a resume carries on, as the layer gives each byte
//! once and in order.
//!
//! On the wire, each end first sends [`MAGIC`]. Then come frames: each is a
//! 2-byte length and one Noise message. The handshake is `-> e`,
//! `<- e, ee, s, es` and `-> s, se`. The second and the third messages carry
//! the Ed25519 key of their sender and its signature of the Noise static
//! key, encrypted. The responder (a node) then sends its verdict: let in,
//! or refused with the reason. Each message after the verdict is data, or
//! the clean end of its direction. A stream that ends with no such end is a
//! cut, never a clean end, as the relay can drop the last frames. A message
//! that fails its check (changed, dropped, repeated or out of order) ends
//! the session, as nothing can be skipped.
//!
//! - [`Ask`]: what the operator's proof asks of a node: its target, a
//!   route, or the target of an exit. The word travels encrypted.
//! - [`initiate_with`] and [`respond_with`]: the handshake, sans transport
//!   of its own; [`Channel`]: the channel after it, and its carry.
//! - [`ends`]: the two ends as a session runs them: the node's door, its
//!   grant used once ([`ends::Redeem`]), its person ([`person`]), its
//!   target; the operator's check of the node's key before its own proof.
//! - [`net`]: two ends that hold the net's document give each other the
//!   newer one inside the channel.
//! - [`legs`]: the seed of the session's tokens of one leg, in the verdict.
//! - [`hold`]: a node's channel stops between two messages for the binary
//!   that takes its place, which carries it on from the same keys and
//!   nonces.
//! - [`Exporter`]: keys for a label from the finished handshake, which only
//!   the two ends hold; no key of the transport leaves the crate.
//! - [`record`]: the records of a suite that the two ends agree on, with a
//!   rekey, a padding to buckets, a replay window and a RESUME record used
//!   once, sealed with keys of the exporter, in place of the carry.
//!
//! The crate reads no environment variable and keeps no process state but
//! task-local values: the caller gives the net's documents
//! ([`ends::Node::docs`]) and the holds of a handover
//! ([`ends::Node::holds`]); a node's lines go to its
//! [`pod_core::Events`] as events of the kinds `e2e.*`; each [`Error`]
//! converts to a [`pod_core::Failure`] of the stage `Session`.

mod ask;
mod channel;
pub mod ends;
pub mod exporter;
mod failure;
mod frame;
#[cfg(feature = "fuzzing")]
pub mod fuzzing;
mod handshake;
pub mod hold;
pub mod legs;
pub mod net;
pub mod person;
pub mod record;

use std::fmt;
use std::time::Duration;

pub use ask::{Ask, MAX_WORD, exit_word};
pub use channel::{Carried, Channel, Held, Start};
pub use exporter::Exporter;
pub use handshake::{Asked, Offered, initiate, initiate_with, respond, respond_with};

/// What each end sends first: the channel and its version. An end with no
/// channel is then told apart at once, before any byte of a session. The
/// name is the name of the protocol, the same for each consumer.
pub const MAGIC: &[u8; 13] = b"podssh-e2e/1\n";
/// The Noise protocol.
pub const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_SHA256";
/// The prologue, which binds the handshake to this channel and version.
pub const PROLOGUE: &[u8] = b"podssh-e2e/1";
/// The largest Noise message.
pub const MAX_MESSAGE: usize = 65535;
/// Noise's tag on each message of the transport.
pub const TAG: usize = 16;
/// The most bytes of a session in one message: its type byte and the tag
/// go with them.
pub const MAX_DATA: usize = MAX_MESSAGE - TAG - 1;
/// The limit of each step of the handshake, and of the verdict, in which the
/// node can dial its target.
pub const HANDSHAKE_LIMIT: Duration = Duration::from_secs(30);
/// The largest grant that the operator's proof carries: an Ed25519
/// certificate with a few principals is about 300 bytes.
pub const MAX_GRANT: usize = 8 * 1024;

/// Why a node refused a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The node's allowlist does not hold this end's key.
    NotAllowed,
    /// The node could not reach its target: why.
    NoTarget(String),
    /// Another reason.
    Other(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NotAllowed => f.write_str("this end's key is not in the node's allowlist"),
            Refusal::NoTarget(why) => write!(f, "the node could not reach its target: {why}"),
            Refusal::Other(why) => f.write_str(why),
        }
    }
}

/// The limit of the operator's wait for the node's verdict: a step's, and
/// the question of a node's person before it ([`person`]), so that an
/// operator waits for the answer of a node that asks its person.
pub const VERDICT_LIMIT: Duration =
    Duration::from_secs(HANDSHAKE_LIMIT.as_secs() + person::Times::DEFAULT.question.as_secs());
/// The step of [`Error::Timeout`] that [`VERDICT_LIMIT`] limits.
pub const VERDICT_STEP: &str = "the node's verdict";

/// Why the channel failed.
#[derive(Debug)]
pub enum Error {
    /// The stream under the channel failed.
    Io(std::io::Error),
    /// The peer does not speak the channel: what it sent first, made safe.
    NotChannel(String),
    /// The handshake failed: a message that is not Noise's, or a key that
    /// does not sign the peer's Noise key.
    Handshake(String),
    /// A message failed its check: changed, dropped, repeated or out of
    /// order on its way.
    Tampered,
    /// The stream ended before the clean end of its direction.
    Cut,
    /// The node refused this end.
    Refused(Refusal),
    /// This end refused the node's key: why (a changed key, with both
    /// fingerprints).
    NodeKey(String),
    /// A step of the handshake took longer than [`HANDSHAKE_LIMIT`]: which.
    Timeout(&'static str),
    /// A frame or a message that the channel does not have.
    Malformed(String),
    /// The two ends share no suite of records: the offers.
    NoSuite(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "the stream under the channel failed: {e}"),
            // The words name the protocol, as MAGIC does, and no binary or
            // flag of a consumer.
            Error::NotChannel(saw) => write!(
                f,
                "the other end does not speak podssh's end-to-end channel (it sent {saw}): an end of an older \
                 release, or an end with the channel off, which must then be off at both ends"
            ),
            Error::Handshake(why) => write!(f, "the channel's handshake failed: {why}"),
            Error::Tampered => f.write_str(
                "a message of the channel failed its check: the relay, or something on the way, changed, \
                 dropped or repeated it",
            ),
            Error::Cut => f.write_str("the channel was cut before its end: the last bytes can be missing"),
            Error::Refused(why) => write!(f, "the node refused the session: {why}"),
            Error::NodeKey(why) => write!(f, "the node's key is not the one expected: {why}"),
            Error::Timeout(step) => {
                let limit = if *step == VERDICT_STEP { VERDICT_LIMIT } else { HANDSHAKE_LIMIT };
                write!(f, "{step} did not come within {} s", limit.as_secs())
            }
            Error::Malformed(why) => write!(f, "the channel carried a message that it does not have: {why}"),
            Error::NoSuite(offers) => write!(f, "the two ends share no suite of records: {offers}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::UnexpectedEof { Error::Cut } else { Error::Io(e) }
    }
}

//! Why a use of a mailbox gave nothing. No variant holds the code.

use std::fmt;
use std::time::Duration;

use pod_core::{Class, Failure, Stage};

/// Why a pairing by code gave nothing. No variant holds the code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failed {
    /// No mailbox could be reached: why, for each.
    Unreachable(String),
    /// A mailbox refused this end, in its words.
    Refused(String),
    /// Nobody took the code within its life: the giver's end.
    Expired(Duration),
    /// No giver waits at the code: it expired, it was used, or it never was.
    Nobody,
    /// The two ends' keys differ: a code typed wrong, or a guess. The code
    /// is spent at both ends.
    WrongCode,
    /// The other end broke the exchange, or is no pairing of the caller's.
    Peer(String),
    /// This end refused what the other end gave: why.
    Mine(String),
    /// The connection to the mailbox failed or stalled.
    Broken(String),
    /// The system gave no random bytes.
    NoRandom,
}

impl Failed {
    /// The stage and the class of the failure, for a caller that maps them
    /// to an exit code. The stage is always [`Stage::Rendezvous`], but for a
    /// system with no random bytes, which is [`Stage::Host`].
    pub fn stage_and_class(&self) -> (Stage, Class) {
        match self {
            Failed::Unreachable(_) | Failed::Broken(_) => (Stage::Rendezvous, Class::Unavailable),
            Failed::Refused(_) | Failed::WrongCode => (Stage::Rendezvous, Class::Refused),
            Failed::Expired(_) => (Stage::Rendezvous, Class::Timeout),
            Failed::Nobody => (Stage::Rendezvous, Class::NotFound),
            Failed::Peer(_) => (Stage::Rendezvous, Class::Protocol),
            Failed::Mine(_) => (Stage::Rendezvous, Class::Policy),
            Failed::NoRandom => (Stage::Host, Class::Unavailable),
        }
    }
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failed::Unreachable(why) => write!(f, "no mailbox could be reached: {why}"),
            Failed::Refused(why) | Failed::Peer(why) | Failed::Mine(why) | Failed::Broken(why) => f.write_str(why),
            Failed::Expired(life) => {
                write!(f, "nobody took the code within its {} minutes; it is spent", life.as_secs().div_ceil(60))
            }
            Failed::Nobody => f.write_str("no node waits at this code: it expired, it was used, or it was typed wrong"),
            Failed::WrongCode => f.write_str(
                "the code did not match at the other end: it was typed wrong, or someone guessed; the code is spent",
            ),
            Failed::NoRandom => f.write_str("the system gave no random bytes"),
        }
    }
}

impl std::error::Error for Failed {}

impl From<Failed> for Failure {
    fn from(failed: Failed) -> Failure {
        let (stage, class) = failed.stage_and_class();
        Failure::caused(stage, class, failed.to_string(), failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_failure_says_more_than_its_words() {
        assert!(Failed::Expired(Duration::from_secs(600)).to_string().contains("within its 10 minutes"));
        assert!(Failed::WrongCode.to_string().contains("the code is spent"));
        assert!(Failed::Nobody.to_string().starts_with("no node waits at this code"));
    }

    #[test]
    fn each_failure_has_a_stage_and_a_class_and_becomes_a_failure() {
        let all = [
            (Failed::Unreachable("x".into()), Stage::Rendezvous, Class::Unavailable),
            (Failed::Broken("x".into()), Stage::Rendezvous, Class::Unavailable),
            (Failed::Refused("x".into()), Stage::Rendezvous, Class::Refused),
            (Failed::WrongCode, Stage::Rendezvous, Class::Refused),
            (Failed::Expired(Duration::from_secs(1)), Stage::Rendezvous, Class::Timeout),
            (Failed::Nobody, Stage::Rendezvous, Class::NotFound),
            (Failed::Peer("x".into()), Stage::Rendezvous, Class::Protocol),
            (Failed::Mine("x".into()), Stage::Rendezvous, Class::Policy),
            (Failed::NoRandom, Stage::Host, Class::Unavailable),
        ];
        for (failed, stage, class) in all {
            assert_eq!(failed.stage_and_class(), (stage, class), "{failed:?}");
            let text = failed.to_string();
            let failure = Failure::from(failed);
            assert_eq!((failure.stage, failure.class, failure.to_string()), (stage, class, text));
        }
    }
}

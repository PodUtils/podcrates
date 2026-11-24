//! Why a use of a mailbox gave nothing. No variant holds the code.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Failure, Stage};

/// Why a pairing by code gave nothing. No variant holds the code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failed {
    /// No mailbox, or no broker of [`crate::mqtt`], could be reached: why,
    /// for each, with the failure of each as the source.
    Unreachable(Missed),
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
            // The words of each name its mailbox or its broker.
            Failed::Unreachable(why) => write!(f, "no meeting point could be reached: {why}"),
            Failed::Refused(why) | Failed::Peer(why) | Failed::Mine(why) | Failed::Broken(why) => f.write_str(why),
            Failed::Expired(life) => write!(f, "nobody took the code within its {}; it is spent", exact_time(*life)),
            Failed::Nobody => {
                f.write_str("no giver waits at this code: it expired, it was used, or it was typed wrong")
            }
            Failed::WrongCode => f.write_str(
                "the code did not match at the other end: it was typed wrong, or someone guessed; the code is spent",
            ),
            Failed::NoRandom => f.write_str("the system gave no random bytes"),
        }
    }
}

impl std::error::Error for Failed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Failed::Unreachable(missed) => missed.causes.first().map(|c| &**c as &(dyn std::error::Error + 'static)),
            _ => None,
        }
    }
}

/// A time as a person reads it, with no rounding: whole minutes as
/// "N minute(s)", else "N second(s)".
fn exact_time(time: Duration) -> String {
    let secs = time.as_secs();
    let (n, unit) = if secs >= 60 && secs.is_multiple_of(60) { (secs / 60, "minute") } else { (secs, "second") };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// Why each meeting point could not be reached: the words, and the
/// failure of each point with its stage (a proxy, a name, the far end).
/// Two are equal when their words are equal.
#[derive(Debug, Clone, Default)]
pub struct Missed {
    text: String,
    causes: Vec<Arc<Failure>>,
}

impl Missed {
    /// The words `text`, with no failure.
    pub fn new(text: impl Into<String>) -> Missed {
        Missed { text: text.into(), causes: Vec::new() }
    }

    /// The words `text` of one point, and its failure `cause`.
    pub fn caused(text: impl Into<String>, cause: impl Into<Failure>) -> Missed {
        Missed { text: text.into(), causes: vec![Arc::new(cause.into())] }
    }

    /// The failure of each point, in the order of the points. The first is
    /// the source of [`Failed::Unreachable`].
    pub fn causes(&self) -> &[Arc<Failure>] {
        &self.causes
    }

    /// The words and the failures of one point that failed with `failed`.
    pub(crate) fn of(failed: Failed) -> Missed {
        match failed {
            Failed::Unreachable(missed) => missed,
            other => Missed::new(other.to_string()),
        }
    }

    /// The words of each of `each`, joined by "; ", and each failure.
    pub fn join(each: Vec<Missed>) -> Missed {
        let text = each.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join("; ");
        Missed { text, causes: each.into_iter().flat_map(|m| m.causes).collect() }
    }
}

impl std::ops::Deref for Missed {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for Missed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialEq for Missed {
    fn eq(&self, other: &Missed) -> bool {
        self.text == other.text
    }
}

impl Eq for Missed {}

impl From<String> for Missed {
    fn from(text: String) -> Missed {
        Missed::new(text)
    }
}

impl From<&str> for Missed {
    fn from(text: &str) -> Missed {
        Missed::new(text)
    }
}

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
        let rows = [(1, "1 second"), (5, "5 seconds"), (60, "1 minute"), (90, "90 seconds"), (600, "10 minutes")];
        for (secs, time) in rows {
            let text = Failed::Expired(Duration::from_secs(secs)).to_string();
            assert_eq!(text, format!("nobody took the code within its {time}; it is spent"), "{secs} s");
        }
        assert!(Failed::WrongCode.to_string().contains("the code is spent"));
        assert!(Failed::Nobody.to_string().starts_with("no giver waits at this code"));
        // A broker is no mailbox: the words fit both (T-261).
        let unreachable = Failed::Unreachable("wss://b.example/mqtt: refused".into()).to_string();
        assert_eq!(unreachable, "no meeting point could be reached: wss://b.example/mqtt: refused");
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

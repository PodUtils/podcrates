//! Early data (0-RTT) of a VLESS session, behind an explicit option. The
//! link's `ed` query names the size that the server reserves for the first
//! flight, and this client announces it only where the session's nonce is
//! fresh. A nonce that the replay window has seen within 10 minutes is
//! refused, and the same session goes on at 1-RTT, on the connection that it
//! already has: the refusal costs a round trip and opens no other session.
//!
//! pod-tls sends no early data today. The option therefore decides whether
//! the session announces the size at all, which is the byte that a server
//! reads before it reserves the buffer; the replay check and the fallback are
//! the same ones that a session with 0-RTT takes.

use std::sync::Arc;

use crate::replay::{Admitted, ReplayWindow};

/// The nonce of one session's early data, and the window that keeps it.
#[derive(Clone)]
pub struct Early {
    pub window: Arc<ReplayWindow>,
    /// The bytes that say this flight is not a replay. They go in no line
    /// and in no log.
    pub nonce: Vec<u8>,
}

impl std::fmt::Debug for Early {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Early(<redacted>)")
    }
}

impl Early {
    pub fn new(window: Arc<ReplayWindow>, nonce: impl Into<Vec<u8>>) -> Early {
        Early { window, nonce: nonce.into() }
    }

    /// The window's verdict for this nonce: the first use is fresh, each
    /// use inside the window a repeat.
    pub fn admit(&self) -> Admitted {
        self.window.admit(&self.nonce)
    }
}

/// What a session offers of 0-RTT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// The option is set and the window took the nonce: the link's size goes
    /// in the path.
    ZeroRtt,
    /// The option is set and the nonce is a repeat: the session keeps 1-RTT.
    OneRtt,
    /// The option is absent: the session sends no early data and says so.
    Absent,
}

impl Offer {
    /// The offer of `early`, which the window rules on here and nowhere else.
    pub fn of(early: Option<&Early>) -> Offer {
        match early {
            None => Offer::Absent,
            Some(early) => match early.admit() {
                Admitted::Fresh => Offer::ZeroRtt,
                Admitted::Repeat => Offer::OneRtt,
            },
        }
    }

    pub fn zero_rtt(self) -> bool {
        self == Offer::ZeroRtt
    }

    /// The words of the offer, for a line of `-v`.
    pub fn words(self) -> &'static str {
        match self {
            Offer::ZeroRtt => "early data announced; the nonce is new to the window",
            Offer::OneRtt => "early data refused as a replay of a nonce of the last 10 minutes: 1-RTT",
            Offer::Absent => "no early data",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::WINDOW;
    use std::time::Instant;

    #[test]
    fn a_nonce_is_offered_once_and_a_repeat_keeps_the_session_at_one_round_trip() {
        let window = Arc::new(ReplayWindow::new());
        let early = Early::new(window.clone(), b"the-session-s-nonce");
        assert_eq!(format!("{early:?}"), "Early(<redacted>)");
        assert_eq!(Offer::of(Some(&early)), Offer::ZeroRtt);
        assert_eq!(Offer::of(Some(&early)), Offer::OneRtt);
        assert_eq!(Offer::of(None), Offer::Absent);
        assert!(!Offer::Absent.zero_rtt() && Offer::ZeroRtt.zero_rtt());
        assert!(Offer::OneRtt.words().contains("1-RTT"));
        // Another nonce is another session's, and the window is full of the
        // first: the two do not stand for each other.
        let other = Early::new(window.clone(), b"another-session-s-nonce");
        assert_eq!(Offer::of(Some(&other)), Offer::ZeroRtt);
        assert_eq!(window.len(), 2);
        // The first nonce is free once the window has aged it.
        let start = Instant::now();
        let aged = ReplayWindow::new();
        assert_eq!(aged.admit_at(b"n", start), Admitted::Fresh);
        assert_eq!(aged.admit_at(b"n", start + WINDOW + std::time::Duration::from_secs(1)), Admitted::Fresh);
    }
}

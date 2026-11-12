//! The two legs of a session (PodUtils/podnet issues 76 and 126): the
//! direct path (the dial of [`crate::Way`]) and a relay of the caller (a
//! second dial, podnet's WebSocket relay of `pod-relay`). The direct path
//! starts first; the relay starts after a delay, or at once when the direct
//! path failed; the first session that opens wins
//! ([`crate::Warp::open_session`]).
//!
//! The leg that worked is a value of the caller: [`relay_first_from`] reads
//! the text of a leg file (podnet's `warp-leg.json`) and [`leg_text`]
//! writes it. The file is the caller's.

use std::sync::Arc;
use std::time::Duration;

use pod_core::Dial;

use crate::session::Meter;

/// The delay of the relay after the start of the direct path.
pub const RELAY_DELAY: Duration = Duration::from_secs(3);

/// The delay of the relay when the relay worked last: a direct path that
/// works opens in less.
pub const RELAY_DELAY_KNOWN: Duration = Duration::from_millis(500);

/// A leg in a leg file is used for this time.
pub const LEG_TTL: Duration = Duration::from_secs(6 * 3600);

/// The leg of a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leg {
    /// The dial of [`crate::Way`].
    Direct,
    /// The dial of [`Relay`].
    Relay,
}

impl Leg {
    /// The word of the leg in a leg file.
    pub fn as_str(self) -> &'static str {
        match self {
            Leg::Direct => "direct",
            Leg::Relay => "relay",
        }
    }
}

/// The relay leg of a session: the caller's dial through its relay.
#[derive(Clone)]
pub struct Relay {
    /// The dial through the relay. A session through it goes to the same
    /// front doors, with the TLS settings of [`crate::Way`].
    pub dial: Arc<dyn Dial>,
    /// The name of the relay in a leg file (podnet: the URL of the relay).
    pub name: String,
    /// The meter of a stream of the relay: its budget goes to the session
    /// ([`crate::MasqueSession::relay_budget`]) and to the
    /// [`crate::WarpNet`] over it ([`crate::WarpNet::renew_due`]). None:
    /// no budget.
    pub meter: Option<Meter>,
    /// The delay of the relay after the start of the direct path.
    pub delay: Duration,
    /// The delay when the relay worked last ([`crate::Warp::relay_first`]).
    pub delay_known: Duration,
}

impl Relay {
    /// The relay of `dial` named `name`, with no meter, [`RELAY_DELAY`] and
    /// [`RELAY_DELAY_KNOWN`].
    pub fn new(dial: Arc<dyn Dial>, name: impl Into<String>) -> Relay {
        Relay { dial, name: name.into(), meter: None, delay: RELAY_DELAY, delay_known: RELAY_DELAY_KNOWN }
    }

    /// The delay of the relay: [`Relay::delay_known`] when the relay worked
    /// last, else [`Relay::delay`].
    pub fn delay_for(&self, relay_first: bool) -> Duration {
        if relay_first { self.delay_known } else { self.delay }
    }
}

impl std::fmt::Debug for Relay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Relay")
            .field("dial", &self.dial.label())
            .field("name", &self.name)
            .field("meter", &self.meter.is_some())
            .field("delay", &self.delay)
            .field("delay_known", &self.delay_known)
            .finish()
    }
}

/// True when the text of a leg file says that the relay `relay` worked
/// less than [`LEG_TTL`] before `now` (Unix seconds) (PodUtils/podnet issue
/// 126). An old entry, another relay, an entry of the future or a bad text
/// is false.
pub fn relay_first_from(text: &str, now: u64, relay: &str) -> bool {
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    doc["leg"] == Leg::Relay.as_str()
        && doc["relay"] == relay
        && doc["at"].as_u64().is_some_and(|at| at <= now && now - at < LEG_TTL.as_secs())
}

/// The text of a leg file: `leg` worked at `now` (Unix seconds) with the
/// relay `relay` (podnet's `warp-leg.json`).
pub fn leg_text(leg: Leg, now: u64, relay: &str) -> String {
    serde_json::json!({"leg": leg.as_str(), "at": now, "relay": relay}).to_string()
}

/// The state of the legs of a [`crate::Warp`].
#[derive(Debug, Default)]
pub(crate) struct Legs {
    /// The relay worked last: its delay is short.
    pub(crate) relay_first: bool,
    /// The leg of the last session.
    pub(crate) last: Option<Leg>,
    /// The leg for the leg file, after a session that changed
    /// `relay_first`.
    pub(crate) change: Option<Leg>,
}

impl Legs {
    /// Note that `leg` opened a session.
    pub(crate) fn won(&mut self, leg: Leg) {
        self.last = Some(leg);
        let relay_first = leg == Leg::Relay;
        if relay_first != self.relay_first {
            self.relay_first = relay_first;
            self.change = Some(leg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // PodUtils/podnet issue 126: a relay that worked in the last hours goes
    // first; an old entry, another relay or a bad file does not.
    #[test]
    fn the_leg_file_puts_a_recent_relay_first() {
        let relay = "https://relay.example";
        let doc = |leg: &str, at: u64, r: &str| serde_json::json!({"leg": leg, "at": at, "relay": r}).to_string();
        let now = 1_800_000_000;
        assert!(relay_first_from(&doc("relay", now - 60, relay), now, relay));
        assert!(!relay_first_from(&doc("direct", now - 60, relay), now, relay));
        assert!(!relay_first_from(&doc("relay", now - 7 * 3600, relay), now, relay));
        assert!(!relay_first_from(&doc("relay", now - 60, "https://other"), now, relay));
        assert!(!relay_first_from(&doc("relay", now + 60, relay), now, relay));
        assert!(!relay_first_from("not json", now, relay));
    }

    // The text that `leg_text` writes reads back, in podnet's form.
    #[test]
    fn the_leg_text_reads_back() {
        let relay = "https://relay.example";
        let now = 1_800_000_000;
        assert!(relay_first_from(&leg_text(Leg::Relay, now - 5, relay), now, relay));
        assert!(!relay_first_from(&leg_text(Leg::Direct, now - 5, relay), now, relay));
        assert!(!relay_first_from(&leg_text(Leg::Relay, now - LEG_TTL.as_secs(), relay), now, relay));
        let doc: serde_json::Value = serde_json::from_str(&leg_text(Leg::Relay, now, relay)).unwrap();
        assert_eq!(doc, serde_json::json!({"leg": "relay", "at": now, "relay": relay}));
    }

    // The leg file changes only when the order of the legs changes, as
    // podnet writes it.
    #[test]
    fn a_change_of_the_order_is_kept_for_the_leg_file() {
        let mut legs = Legs::default();
        legs.won(Leg::Direct);
        assert_eq!((legs.last, legs.change, legs.relay_first), (Some(Leg::Direct), None, false));
        legs.won(Leg::Relay);
        assert_eq!((legs.change.take(), legs.relay_first), (Some(Leg::Relay), true));
        legs.won(Leg::Relay);
        assert_eq!(legs.change, None);
        legs.won(Leg::Direct);
        assert_eq!((legs.change, legs.relay_first), (Some(Leg::Direct), false));
    }

    #[test]
    fn the_delays_of_the_relay() {
        assert_eq!((RELAY_DELAY, RELAY_DELAY_KNOWN), (Duration::from_secs(3), Duration::from_millis(500)));
    }
}

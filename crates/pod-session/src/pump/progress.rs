//! Whether the session moves while a `PING` waits for its `PONG`
//! (PodUtils/podssh#283, ask 4). The far end reads each record as it comes
//! and answers a `PING` at once, while its application can hold its
//! acknowledgements. A `PING` that waits with no move of the session tells
//! that this side's bytes no longer reach the far end, also while the far
//! end's own records still come.

use std::sync::Mutex;

use tokio::time::Instant;

use crate::beside::{Position, position};
use crate::link::Link;

/// The position of the session, and since when it stands there.
pub(super) struct Stood {
    at: Position,
    since: Instant,
}

impl Stood {
    pub(super) fn new(state: &Mutex<Link>) -> Stood {
        Stood { at: position(state), since: Instant::now() }
    }

    /// Since when a `PING` that the heartbeat asked for at `asked` waits for
    /// its `PONG` with no move of the session, at `now`: the later of the
    /// two. `None` while each `PING` is answered.
    pub(super) fn unanswered(&mut self, state: &Mutex<Link>, asked: Option<Instant>, now: Instant) -> Option<Instant> {
        let at = position(state);
        if at != self.at {
            self.at = at;
            self.since = now;
        }
        asked.map(|asked| asked.max(self.since))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A wait of a `PING` counts from the last move of the session: a byte
    /// that came, or an acknowledgement, starts it again.
    #[tokio::test(start_paused = true)]
    async fn a_wait_counts_from_the_ping_or_the_last_move_whichever_is_later() {
        let state = Mutex::new(Link::new(0, 0).with_replay(1 << 20));
        let mut stood = Stood::new(&state);
        let start = Instant::now();
        assert_eq!(stood.unanswered(&state, None, start), None, "no PING waits");
        tokio::time::sleep(Duration::from_secs(10)).await;
        let asked = Instant::now();
        assert_eq!(stood.unanswered(&state, Some(asked), asked), Some(asked), "the session stood since the start");
        tokio::time::sleep(Duration::from_secs(5)).await;
        let mut records = Vec::new();
        state.lock().unwrap().send(b"moved", &mut records).unwrap();
        let ack = crate::record::Record::Ack { offset: 5 };
        state.lock().unwrap().on_record(ack).unwrap();
        let moved = Instant::now();
        assert_eq!(stood.unanswered(&state, Some(asked), moved), Some(moved), "the far end acknowledged a byte");
        tokio::time::sleep(Duration::from_secs(20)).await;
        let later = Instant::now();
        assert_eq!(stood.unanswered(&state, Some(asked), later), Some(moved), "no move since");
        assert_eq!(stood.unanswered(&state, None, later), None, "the PONG came");
    }
}

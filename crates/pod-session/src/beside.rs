//! A new link beside the one that carries the session: a move before the
//! relay's limits, or a spare beside a link that went silent.
//!
//! When the client's address changes, its TCP link through the proxy and
//! the relay often goes silent with no error, and the heartbeat finds it
//! dead only after 30 s. A spare link opens sooner: when a question of this
//! side (`DATA` that the far end acknowledges, a `PING`) has waited three
//! round trips of the link, 1 s to 3 s, with nothing from the far
//! end, or nothing at all came for two heartbeats. The old link is asked
//! too, so that one that only lagged closes the spare within a round trip.
//! The spare reads its far end's `GREETING` and sends nothing: when the old
//! link's bytes come first, it closes before its handshake, and the far end
//! never dialled a target for it; when it greets first, its resume takes the
//! session, as after a loss, and the far end lets the newest resume win. One
//! new link at a time, and one spare each 10 s at most. Measured through the
//! stand-in proxy's `--move` with a fixed 3 s: a resume 3.0 to 3.2 s
//! after a new address, where the heartbeat alone took 35 s.
//!
//! A road that holds the session's bytes for longer than a spare waits
//! (a third party's tunnel of HTTP) would get a spare on each link, each
//! before the replayed bytes come through. So the wait doubles after a
//! spare that took the session when the session moved no byte before the
//! next silence, up to [`DEAD_AFTER`], and the first byte that moves sets
//! it back ([`SpareWait`]).

use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::io::AsyncRead;
use tokio::sync::mpsc::UnboundedSender;
// tokio's clock, which a test can pause.
use tokio::time::Instant;

use super::client;
use super::hearing::Hearing;
use super::link::Link;
use super::pump::{DEAD_AFTER, End, Ended, Watch};
use super::resume::{Greeted, Next, Note, greeted};

/// The most that a question of this side waits, with nothing from the far
/// end, before a spare link opens; all of it before the round trip is known.
pub const SPARE_AFTER: Duration = Duration::from_secs(3);
/// How long nothing at all may come before a spare link opens: two
/// heartbeats (`pump::PING_EVERY`).
pub const SPARE_QUIET: Duration = Duration::from_secs(20);
/// At most one spare link each this long.
pub const SPARE_EVERY: Duration = Duration::from_secs(10);
/// The least that a question waits before a spare link opens, on a link of
/// any round trip: osewa's floor, under which a link that only lagged would
/// get a spare.
pub const SPARE_SOONEST: Duration = Duration::from_secs(1);
/// How many smoothed round trips of the link a question waits before a
/// spare link opens: osewa's three.
pub const SPARE_ROUND_TRIPS: u32 = 3;
/// The longest wait between two looks at the silence of the link.
const LOOK_EVERY: Duration = Duration::from_millis(250);
/// Why a spare closes when the old link's bytes came first.
const SPOKE: &str = "the link spoke again";

/// Why a new link opened beside the one that carries the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beside {
    /// The link nears the relay's limits.
    Move,
    /// The link said nothing for this long.
    Spare(Duration),
}

/// What the driver gives to look for a new link beside the current one.
pub(super) struct Plan<'a> {
    /// Whether the session moves before the relay's limits.
    pub moves: bool,
    /// Whether a silent link gets a spare beside it.
    pub spares: bool,
    /// Whether the link answers `PING`s, so that a long quiet is silence.
    pub heartbeat: bool,
    /// When the link is old enough to move.
    pub move_at: Instant,
    /// Whether the session was closing already: no link opens beside it.
    pub closing: bool,
    /// The wait of a spare after the spares that took the session.
    pub wait: SpareWait,
    pub watch: &'a Watch,
    pub state: &'a Mutex<Link>,
    pub hearing: &'a Hearing,
    /// Where the steps go, as they come, for the caller's lines.
    pub said: &'a UnboundedSender<Note>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The next new link beside the current one, once its far end greeted, and
/// why it opened. A move that fails says so and is not tried again on this
/// link; a spare that the old link outspeaks, or that fails, closes, and the
/// watch goes on. Never, while the session closes.
pub(super) async fn next<L, C, F>(plan: Plan<'_>, connect: &mut C) -> (Beside, Greeted<L>)
where
    L: AsyncRead + Unpin,
    C: FnMut(&Ended) -> F,
    F: Future<Output = Next<L>>,
{
    let mut moves = plan.moves;
    let mut last_spare: Option<Instant> = None;
    loop {
        let why = tokio::select! {
            // The pump tells when the link passes its bytes.
            _ = plan.watch.due.notified(), if moves => Beside::Move,
            () = tokio::time::sleep_until(plan.move_at), if moves => Beside::Move,
            quiet = silence(&plan, last_spare), if plan.spares => Beside::Spare(quiet),
            else => std::future::pending::<Beside>().await,
        };
        // A session whose application ended is closing: its `CLOSE` goes on
        // the link that it has.
        if plan.closing || plan.watch.app_ended.load(Ordering::SeqCst) {
            return std::future::pending().await;
        }
        // One guard: a second lock in the same expression would wait for
        // the first, which lives to the end of the statement.
        let (received, sent) = {
            let link = lock(plan.state);
            (link.received(), link.sent())
        };
        if let Beside::Spare(quiet) = why {
            last_spare = Some(Instant::now());
            let _ = plan.said.send(Note::Spare { quiet });
            // A `PING` on the old link: one that only lagged, as behind a
            // far end that holds its `ACK`, answers it within a round trip,
            // and the answer closes the spare before it greets.
            plan.hearing.ask_now();
        }
        // The far end's bytes after this mark close a spare, while it
        // connects too: the old link is back, and a spare costs a resume.
        let mark = plan.hearing.words();
        let at = Ended { end: End::Moving, received, sent };
        let next = match why {
            Beside::Move => connect(&at).await,
            Beside::Spare(_) => tokio::select! {
                biased;
                () = plan.hearing.spoke_after(mark) => {
                    let _ = plan.said.send(failure(why, SPOKE.to_string()));
                    continue;
                }
                next = connect(&at) => next,
            },
        };
        let new = match next {
            Next::Link(new) => new,
            Next::Retry(why_not) | Next::Stop(why_not) => {
                let _ = plan.said.send(failure(why, why_not));
                moves &= why != Beside::Move;
                continue;
            }
        };
        let greeting = tokio::time::timeout(client::HANDSHAKE_LIMIT, greeted(new));
        let why_not = match why {
            Beside::Move => match greeting.await {
                Ok(Ok(greeted)) => return (why, greeted),
                Ok(Err(fault)) => fault.reason(),
                Err(_) => "the new link's far end did not greet in time".to_string(),
            },
            // The spare closes, with nothing sent, when the old link speaks
            // first; when both came, the old link goes on.
            Beside::Spare(_) => tokio::select! {
                biased;
                () = plan.hearing.spoke_after(mark) => SPOKE.to_string(),
                got = greeting => match got {
                    Ok(Ok(greeted)) => return (why, greeted),
                    Ok(Err(fault)) => fault.reason(),
                    Err(_) => "its far end did not greet in time".to_string(),
                },
            },
        };
        let _ = plan.said.send(failure(why, why_not));
        moves &= why != Beside::Move;
    }
}

/// The step that says that a new link beside the old one failed.
pub(super) fn failure(why: Beside, why_not: String) -> Note {
    match why {
        Beside::Move => Note::MoveFailed { why: why_not },
        Beside::Spare(_) => Note::SpareClosed { why: why_not },
    }
}

/// How long a question waits, with nothing from the far end, before a spare
/// link opens, on a link whose smoothed round trip is `round_trip`: three of
/// them, [`SPARE_SOONEST`] at least and [`SPARE_AFTER`] at most; all of
/// [`SPARE_AFTER`] until a question of the link was answered.
pub fn spare_after(round_trip: Option<Duration>) -> Duration {
    match round_trip {
        Some(round_trip) => round_trip.saturating_mul(SPARE_ROUND_TRIPS).clamp(SPARE_SOONEST, SPARE_AFTER),
        None => SPARE_AFTER,
    }
}

/// Where the session is: the bytes that this side received, and the bytes
/// of this side that the far end acknowledged. Both only grow.
pub type Position = (u64, u64);

/// The position of the session that `state` carries.
pub(super) fn position(state: &Mutex<Link>) -> Position {
    let link = lock(state);
    (link.received(), link.acknowledged())
}

/// The wait of a spare across the links of a session. A spare that took the
/// session at a position that the session has not left since doubles the
/// wait; the first byte that moves the session sets it back.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SpareWait {
    /// Where the last spare took the session, with no move of it since the
    /// spare before.
    taken_at: Option<Position>,
    /// How many times the wait doubles at that position.
    doubled: u32,
}

impl SpareWait {
    /// A spare took the session at `at`: the wait doubles once more when the
    /// spare before took it at the same position, else once.
    pub fn took(&mut self, at: Position) {
        self.doubled = if self.taken_at == Some(at) { self.doubled.saturating_add(1) } else { 1 };
        self.taken_at = Some(at);
    }

    /// A new link took the session, for `why`: a spare counts.
    pub(super) fn after(&mut self, why: Beside, state: &Mutex<Link>) {
        if let Beside::Spare(_) = why {
            self.took(position(state));
        }
    }

    /// How long a question waits before a spare opens, at `now`, on a link
    /// whose own wait is `base`: `base` doubled for each spare that took the
    /// session at `now` with no move, [`DEAD_AFTER`] at most; `base` once
    /// the session moved.
    pub fn wait(&self, base: Duration, now: Position) -> Duration {
        if self.taken_at != Some(now) {
            return base;
        }
        let times = 1u32.checked_shl(self.doubled).unwrap_or(u32::MAX);
        base.saturating_mul(times).min(DEAD_AFTER).max(base)
    }
}

/// Until the link is silent: a question waited [`spare_after`] the link's
/// round trip, or, with a heartbeat, nothing came for [`SPARE_QUIET`]; each
/// as long at least as the [`SpareWait`] of the session; and [`SPARE_EVERY`]
/// after the last spare. How long nothing came.
async fn silence(plan: &Plan<'_>, last_spare: Option<Instant>) -> Duration {
    let hearing = plan.hearing;
    if let Some(last) = last_spare {
        tokio::time::sleep_until(last + SPARE_EVERY).await;
    }
    loop {
        let now = Instant::now();
        let quiet = hearing.quiet(now);
        let after = plan.wait.wait(spare_after(hearing.round_trip()), position(plan.state));
        if hearing.silent(now, after).is_some() || (plan.heartbeat && quiet >= SPARE_QUIET.max(after)) {
            return quiet;
        }
        // The next look comes at the instant that a waiting question reaches
        // its bound, so the spare is not late by a look; the bound itself
        // moves with each answer, so it is read again at least each look.
        let mut next = now + LOOK_EVERY;
        if let Some(asked) = hearing.asked_at() {
            next = next.min(asked + after);
        }
        tokio::time::sleep_until(next.max(now + Duration::from_millis(1))).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_question_waits_three_round_trips_of_the_link_1_s_at_least_and_3_s_at_most() {
        assert_eq!(spare_after(None), SPARE_AFTER, "no answer yet: the wait of before");
        assert_eq!(spare_after(Some(MS * 50)), SPARE_SOONEST, "a fast link: the floor");
        assert_eq!(spare_after(Some(MS * 250)), SPARE_SOONEST, "an ACK held 200 ms: still the floor");
        assert_eq!(spare_after(Some(MS * 500)), MS * 1500);
        assert_eq!(spare_after(Some(MS * 1100)), SPARE_AFTER, "a slow link: the ceiling");
        assert_eq!(spare_after(Some(Duration::MAX)), SPARE_AFTER, "no overflow");
    }

    #[test]
    fn the_wait_doubles_after_each_spare_that_moved_nothing_up_to_dead_after_and_a_move_sets_it_back() {
        let mut wait = SpareWait::default();
        let base = SPARE_AFTER;
        assert_eq!(wait.wait(base, (0, 0)), base, "no spare took the session yet");
        wait.took((10, 20));
        assert_eq!(wait.wait(base, (10, 20)), base * 2, "the session did not move since the spare");
        assert_eq!(wait.wait(base, (11, 20)), base, "a byte came");
        assert_eq!(wait.wait(base, (10, 21)), base, "the far end acknowledged a byte");
        wait.took((10, 20));
        assert_eq!(wait.wait(base, (10, 20)), base * 4);
        wait.took((10, 20));
        assert_eq!(wait.wait(base, (10, 20)), base * 8);
        wait.took((10, 20));
        assert_eq!(wait.wait(base, (10, 20)), DEAD_AFTER, "the heartbeat's limit at most");
        for _ in 0..40 {
            wait.took((10, 20));
        }
        assert_eq!(wait.wait(base, (10, 20)), DEAD_AFTER, "no overflow");
        assert_eq!(wait.wait(SPARE_SOONEST, (10, 20)), DEAD_AFTER);
        assert_eq!(wait.wait(DEAD_AFTER * 2, (10, 20)), DEAD_AFTER * 2, "never under the link's own wait");
        // A spare that takes the session after a move starts again at two.
        wait.took((10, 30));
        assert_eq!(wait.wait(base, (10, 30)), base * 2);
        wait.took((10, 30));
        assert_eq!(wait.wait(SPARE_SOONEST, (10, 30)), SPARE_SOONEST * 4, "the wait of a fast link doubles too");
    }
}

//! The tests of the new link beside the old one: the hold of a move, the
//! wait of a spare.

use super::*;

use std::cell::RefCell;

use tokio::io::DuplexStream;

const MS: Duration = Duration::from_millis(1);
/// The mark of a move in the tests: the bytes of the link that make it
/// due.
const MARK: u64 = 100;

/// The plan of a link that moves, with no spare, whose age never makes a
/// move due.
fn plan<'a>(
    watch: &'a Watch,
    state: &'a Mutex<Link>,
    hearing: &'a Hearing,
    said: &'a UnboundedSender<Note>,
) -> Plan<'a> {
    Plan {
        moves: true,
        spares: false,
        heartbeat: true,
        move_at: Instant::now() + Duration::from_secs(3600),
        closing: false,
        wait: SpareWait::default(),
        watch,
        state,
        hearing,
        said,
    }
}

/// A new link that never comes; `held` gets the hold of the old link at
/// each attempt.
fn no_link(held: &RefCell<Vec<bool>>, watch: &Watch) -> impl Future<Output = Next<DuplexStream>> + use<> {
    held.borrow_mut().push(watch.hold.on());
    async { Next::Retry("no link".into()) }
}

/// A move holds the old link while its new link opens, and a move that
/// fails lifts the hold at once: the old link goes on (PodUtils/podssh#325).
#[tokio::test(start_paused = true)]
async fn a_move_holds_the_old_link_and_a_failed_move_lifts_the_hold() {
    let (watch, state, hearing) = (Watch::default(), Mutex::new(Link::new(0, 0)), Hearing::default());
    let (said, mut notes) = tokio::sync::mpsc::unbounded_channel();
    let held = RefCell::default();
    let mut connect = |_: &Ended| no_link(&held, &watch);
    assert!(!watch.hold.on(), "no hold before the move is due");
    watch.due.notify_one();
    let looked =
        tokio::time::timeout(Duration::from_secs(5), next(plan(&watch, &state, &hearing, &said), &mut connect)).await;
    assert!(looked.is_err(), "no new link, and no second move on this link");
    assert_eq!(*held.borrow(), [true], "the old link holds while the new one opens");
    assert_eq!(notes.try_recv(), Ok(Note::MoveFailed { why: "no link".into() }));
    assert!(!watch.hold.on(), "the failed move lifted the hold");
}

/// A move at the link's age, before the mark, holds the old link while its
/// new link opens. When it fails, the hold lifts, and the bytes that pass
/// the mark later start no hold: no task would lift it, as the link moves no
/// more (PodUtils/podssh#327).
#[tokio::test(start_paused = true)]
async fn a_failed_move_ends_the_hold_and_the_mark_of_the_link() {
    let (watch, state, hearing) = (Watch::moving_at(MARK), Mutex::new(Link::new(0, 0)), Hearing::default());
    let (said, mut notes) = tokio::sync::mpsc::unbounded_channel();
    let held = RefCell::default();
    let mut connect = |_: &Ended| no_link(&held, &watch);
    watch.count(MARK - 1);
    assert!(!watch.hold.on(), "no hold under the mark");
    let plan = Plan { move_at: Instant::now(), ..plan(&watch, &state, &hearing, &said) };
    let looked = tokio::time::timeout(Duration::from_secs(5), next(plan, &mut connect)).await;
    assert!(looked.is_err(), "no new link, and no second move on this link");
    assert_eq!(*held.borrow(), [true], "the old link holds while the new one opens");
    assert_eq!(notes.try_recv(), Ok(Note::MoveFailed { why: "no link".into() }));
    assert!(!watch.hold.on(), "the failed move lifted the hold");
    watch.count(2);
    assert!(!watch.hold.on(), "the mark passed after the failed move: no hold");
}

/// A session that closes, or whose application ended, lifts the hold that
/// the mark started and opens no new link: its `CLOSE` goes on the old link,
/// with no wait. When the link's age comes first, the bytes that pass the
/// mark later start no hold (PodUtils/podssh#327).
#[tokio::test(start_paused = true)]
async fn a_closing_session_lifts_the_hold_of_the_mark_and_does_not_move() {
    for (closing, marked) in [(true, true), (false, true), (true, false), (false, false)] {
        let (watch, state, hearing) = (Watch::moving_at(MARK), Mutex::new(Link::new(0, 0)), Hearing::default());
        let (said, _notes) = tokio::sync::mpsc::unbounded_channel();
        watch.app_ended.store(!closing, Ordering::SeqCst);
        if marked {
            watch.count(MARK);
            assert!(watch.hold.on(), "the mark starts the hold");
        }
        let held = RefCell::default();
        let mut connect = |_: &Ended| no_link(&held, &watch);
        let move_at = if marked { Instant::now() + Duration::from_secs(3600) } else { Instant::now() };
        let plan = Plan { closing, move_at, ..plan(&watch, &state, &hearing, &said) };
        let looked = tokio::time::timeout(Duration::from_secs(5), next(plan, &mut connect)).await;
        let case = format!("closing {closing}, at the mark {marked}");
        assert!(looked.is_err(), "{case}: nothing beside a closing session");
        assert!(held.borrow().is_empty(), "{case}: no new link");
        assert!(!watch.hold.on(), "{case}: the hold lifted");
        watch.count(MARK);
        assert!(!watch.hold.on(), "{case}: no hold after the moves of the link ended");
    }
}

/// A spare holds nothing. When the mark passes while the spare opens and
/// the spare fails, the hold of the mark stays, and the move that is due
/// opens its new link with the hold on (PodUtils/podssh#327).
#[tokio::test(start_paused = true)]
async fn a_failed_spare_keeps_the_hold_of_the_mark_and_the_move_follows() {
    let (watch, state, hearing) = (Watch::moving_at(MARK), Mutex::new(Link::new(0, 0)), Hearing::default());
    let (said, mut notes) = tokio::sync::mpsc::unbounded_channel();
    let held = RefCell::default();
    let mut connect = |_: &Ended| {
        let link = no_link(&held, &watch);
        // The mark passes while the first new link, the spare, opens.
        watch.count(MARK);
        link
    };
    let plan = Plan { spares: true, ..plan(&watch, &state, &hearing, &said) };
    // The spare opens after `SPARE_QUIET`; the next one only `SPARE_EVERY` after it.
    let looked = tokio::time::timeout(SPARE_QUIET + SPARE_EVERY / 2, next(plan, &mut connect)).await;
    assert!(looked.is_err(), "no new link");
    assert_eq!(*held.borrow(), [false, true], "the spare holds nothing; the move holds");
    assert!(matches!(notes.try_recv(), Ok(Note::Spare { .. })));
    assert_eq!(notes.try_recv(), Ok(Note::SpareClosed { why: "no link".into() }));
    assert_eq!(notes.try_recv(), Ok(Note::MoveFailed { why: "no link".into() }));
    assert!(!watch.hold.on(), "the failed move lifted the hold");
}

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

//! The one-POST form of the HTTP road (podssh T-622, PodUtils/podssh#239),
//! between the client and a stand-in of the relay role in memory, on a
//! paused clock: straight and through a hop that passes bytes as they come,
//! the form carries the bytes each way with equal digests on one
//! connection; through a hop that holds a body in chunks to its end, the
//! client takes the split form within the bound on the mark, and the run
//! keeps that; an answer cut with no last chunk is an error; the form asked
//! alone through such a hop fails with words that say why.

mod http_road_stand_in;

use std::sync::atomic::Ordering;
use std::time::Duration;

use http_road_stand_in::hop::{Hop, through};
use http_road_stand_in::{One, Plan, StandIn, pattern};
use pod_relay::http_road::client::{End, Opened, Road, Target, open};
use pod_relay::http_road::{Form, Forms, Tuning};
use pod_ws::ConnectError;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;

const MIB: usize = 1 << 20;
/// Far longer than a session that moves: a session that never ends fails
/// the test here.
const BOUND: Duration = Duration::from_secs(600);
/// The bound on the mark that the ask names (PodUtils/podssh#239).
const MARK_BOUND: Duration = Duration::from_secs(2);
const TARGET: Target<'static> = Target { host: "target.test", port: 22, family: None };

async fn opened(road: &Road, form: Option<Form>) -> Result<Opened, ConnectError> {
    open(road, &TARGET, "token", form, Tuning::default()).await
}

/// Send `up` through the session, then its end, and read the bytes down to
/// their end: the digest of the bytes down, their count, and the session's
/// end.
async fn carry(opened: Opened, up: Vec<u8>) -> ([u8; 32], usize, End) {
    let carried = tokio::time::timeout(BOUND, async move {
        let (mut from, mut to) = tokio::io::split(opened.stream);
        // A session that failed drops the pipe: the writes fail then.
        let writer = tokio::spawn(async move {
            for piece in up.chunks(48 * 1024) {
                if to.write_all(piece).await.is_err() {
                    return;
                }
            }
            let _ = to.shutdown().await;
        });
        let (mut down, mut count) = (Sha256::new(), 0);
        let mut buf = vec![0u8; 64 * 1024];
        while let Ok(n) = from.read(&mut buf).await {
            if n == 0 {
                break;
            }
            down.update(&buf[..n]);
            count += n;
        }
        writer.await.unwrap();
        (down.finalize().into(), count, opened.ended.get().expect("the session's end"))
    });
    carried.await.expect("the session's end within the bound")
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// 4 MiB each way, straight and through a hop that streams: the one-POST
/// form carries them with no form asked, on one request, with the token,
/// and the body's last chunk ends the target.
#[tokio::test(start_paused = true)]
async fn the_one_form_carries_4_mib_each_way_straight_and_through_a_hop_that_streams() {
    for hop in [None, Some(Hop::Streams)] {
        let (up, down) = (pattern(4 * MIB, 3), pattern(4 * MIB, 5));
        let stand_in = StandIn::new(Plan { down: down.clone(), ..Plan::default() });
        let road = match hop {
            None => stand_in.road(),
            Some(how) => through(stand_in.road(), how).0,
        };
        let session = opened(&road, None).await.expect("the open");
        assert_eq!((session.form, session.form.name()), (Form::One, "one"), "{hop:?}");
        let (got, count, end) = carry(session, up.clone()).await;
        assert_eq!(end, End::Closed { code: 1000, reason: "target closed".into() }, "{hop:?}");
        assert_eq!((got, count), (digest(&down), down.len()), "the bytes down, {hop:?}");
        let (up_digest, up_bytes, ended, ones, token, batches) = stand_in.record(|r| {
            let up_digest: [u8; 32] = r.up.clone().finalize().into();
            (up_digest, r.up_bytes, r.one_up_ended, r.ones, r.one_token.clone(), r.sent.len())
        });
        assert_eq!((up_digest, up_bytes), (digest(&up), up.len() as u64), "the bytes up, {hop:?}");
        assert!(ended, "the body's last chunk came, {hop:?}");
        assert_eq!((ones, token.as_deref(), batches), (1, Some("token"), 0), "{hop:?}");
    }
}

/// Through a hop that holds a body in chunks to its end, no mark comes: the
/// client takes the split form after the bound, which carries the session,
/// and the run's next open goes to the split form at once.
#[tokio::test(start_paused = true)]
async fn through_a_holding_hop_the_client_takes_the_split_form_within_the_bound_and_keeps_it() {
    let (up, down) = (pattern(MIB, 7), pattern(MIB, 11));
    let stand_in = StandIn::new(Plan { down: down.clone(), ..Plan::default() });
    let (road, chunked) = through(stand_in.road(), Hop::Holds);
    let started = Instant::now();
    let session = opened(&road, None).await.expect("the open");
    let took = started.elapsed();
    assert_eq!(session.form, Form::Split);
    assert!(took >= MARK_BOUND && took < MARK_BOUND + Duration::from_secs(1), "took the split form after {took:?}");
    let held = road.learned.held("relay.test").expect("the run keeps the held form");
    assert!(held.contains("no mark"), "{held}");
    let (got, count, end) = carry(session, up.clone()).await;
    assert_eq!(end, End::Closed { code: 1000, reason: "target closed".into() });
    assert_eq!((got, count), (digest(&down), down.len()));
    assert_eq!(stand_in.record(|r| (r.up_bytes, r.ones)), (up.len() as u64, 0), "no request of the form passed");
    // The run's next session: a new stand-in behind the same hop's kind, and
    // the same memory.
    let next = StandIn::new(Plan::default());
    let (mut again, again_chunked) = through(next.road(), Hop::Holds);
    again.learned = road.learned.clone();
    let started = Instant::now();
    assert_eq!(opened(&again, None).await.expect("the open").form, Form::Split);
    assert!(started.elapsed() < MARK_BOUND, "the next open waited {:?} for a mark", started.elapsed());
    assert_eq!(chunked.load(Ordering::SeqCst) + again_chunked.load(Ordering::SeqCst), 1, "one try of the form");
}

/// An answer that ends with no last chunk is a cut: the session fails, and
/// the caller's stream ends.
#[tokio::test(start_paused = true)]
async fn an_answer_of_the_one_form_cut_with_no_last_chunk_is_an_error() {
    let plan = Plan { down: pattern(MIB, 9), one: One::CutAfter(256 * 1024), ..Plan::default() };
    let stand_in = StandIn::new(plan);
    let session = opened(&stand_in.road(), None).await.expect("the open");
    assert_eq!(session.form, Form::One);
    let (_, count, end) = carry(session, pattern(64 * 1024, 1)).await;
    assert_eq!(count, 256 * 1024, "the bytes before the cut");
    assert!(matches!(&end, End::Failed(why) if why.contains("cut")), "{end:?}");
}

/// The form asked alone through a holding hop fails after the bound, with
/// words that name the mark and the hop; no other form goes.
#[tokio::test(start_paused = true)]
async fn the_one_form_alone_fails_with_a_clear_error_through_a_holding_hop() {
    let stand_in = StandIn::new(Plan::default());
    let (road, _) = through(stand_in.road(), Hop::Holds);
    let started = Instant::now();
    let failed = opened(&road, Some(Form::One)).await.err().expect("no session");
    assert!(started.elapsed() < MARK_BOUND + Duration::from_secs(1), "{:?}", started.elapsed());
    let words = failed.to_string();
    assert!(words.contains("one-POST form") && words.contains("no mark") && words.contains("holds"), "{words}");
    assert_eq!(stand_in.record(|r| (r.ones, r.sent.len())), (0, 0), "no form went");
}

/// A role that serves no such form (`404`), and a host whose forms do not
/// name it, take the split form; the split and the serial forms asked
/// leave it out.
#[tokio::test(start_paused = true)]
async fn a_host_with_no_one_form_and_the_other_forms_asked_take_no_one_form() {
    let absent = StandIn::new(Plan { one: One::Absent, ..Plan::default() });
    let road = absent.road();
    assert_eq!(opened(&road, None).await.expect("the open").form, Form::Split);
    assert!(road.learned.held("relay.test").is_some_and(|why| why.contains("404")));
    assert_eq!(absent.record(|r| r.ones), 1);
    for (forms, form, carried) in [
        (Some(Forms::of(["split", "serial"])), None, Form::Split),
        (None, Some(Form::Split), Form::Split),
        (None, Some(Form::Serial), Form::Serial),
    ] {
        let stand_in = StandIn::new(Plan::default());
        let road = stand_in.road().with_forms(forms);
        assert_eq!(opened(&road, form).await.expect("the open").form, carried, "{form:?}");
        assert_eq!(stand_in.record(|r| r.ones), 0, "{forms:?} {form:?}");
    }
    let named = StandIn::new(Plan::default());
    let road = Road { forms: Some(Forms::of(["split", "one"])), ..named.road() };
    assert_eq!(opened(&road, None).await.expect("the open").form, Form::One);
}

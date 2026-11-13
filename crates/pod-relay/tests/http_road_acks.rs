//! The invariants 6 to 10 of the HTTP road (PodUtils/podssh#221), between
//! the client and a stand-in of the relay role in memory, on a paused
//! clock: acknowledgements that are lost after the batch is stored, a full
//! window, the refusals and their reasons, the counts at the end of each
//! direction, and the bytes down that the caller's pipe holds.

mod http_road_stand_in;

use std::time::Duration;

use http_road_stand_in::{ID, Plan, StandIn};
use pod_relay::http_road::client::{End, Opened, Target, open};
use pod_relay::http_road::window::Refusal;
use pod_relay::http_road::{Form, Tuning};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MIB: usize = 1 << 20;

/// `n` bytes of a pattern that `seed` picks.
fn pattern(n: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

async fn opened(stand_in: &std::sync::Arc<StandIn>, form: Form) -> Opened {
    let target = Target { host: "target.test", port: 22, family: None };
    open(&stand_in.road(), &target, "token", Some(form), Tuning::default()).await.expect("the open")
}

/// Send `up` through the session and read its bytes down to their end: the
/// digest of the bytes down, and the session's end.
async fn carry(opened: Opened, up: Vec<u8>) -> ([u8; 32], End) {
    let (mut from, mut to) = tokio::io::split(opened.stream);
    // A session that failed drops the pipe: the writes fail then.
    let writer = tokio::spawn(async move {
        if to.write_all(&up).await.is_ok() {
            let _ = to.shutdown().await;
        }
    });
    let mut down = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match from.read(&mut buf).await.expect("the bytes down") {
            0 => break,
            n => down.update(&buf[..n]),
        }
    }
    writer.await.unwrap();
    (down.finalize().into(), opened.ended.get().expect("the session's end"))
}

/// The test of the ask: every third acknowledgement is dropped after its
/// batch is stored; 20 MiB each way arrive with equal digests, each lost
/// acknowledgement costs one resend at most, and each resend carries the
/// key (session, `seq`) and the body of its first send.
async fn lost_acknowledgements(form: Form) {
    let (up, down) = (pattern(20 * MIB, 7), pattern(20 * MIB, 11));
    let stand_in = StandIn::new(Plan { down: down.clone(), drop_every: 3, ..Plan::default() });
    let session = opened(&stand_in, form).await;
    assert_eq!(session.form, form);
    let (digest_down, end) = carry(session, up.clone()).await;
    assert_eq!(end, End::Closed { code: 1000, reason: "target closed".into() }, "{form:?}");
    assert_eq!(digest_down, <[u8; 32]>::from(Sha256::digest(&down)), "the bytes down, {form:?}");
    let (dropped, resent) = (stand_in.record(|r| r.dropped), stand_in.resent());
    stand_in.record(|r| {
        let digest_up: [u8; 32] = std::mem::take(&mut r.up).finalize().into();
        assert_eq!(digest_up, <[u8; 32]>::from(Sha256::digest(&up)), "the bytes up, {form:?}");
        assert_eq!((r.up_bytes, r.total), (up.len() as u64, Some(up.len() as u64)), "{form:?}");
        for ((id, seq), bodies) in &r.sent {
            assert_eq!(id, ID);
            assert!(bodies.windows(2).all(|w| w[0] == w[1]), "the batch {seq} went again with another body");
        }
    });
    assert!(dropped >= 10, "{form:?}: {dropped} acknowledgements dropped");
    assert!(resent <= dropped, "{form:?}: {resent} batches sent again for {dropped} lost acknowledgements");
    assert!(resent > 0, "{form:?}: a lost acknowledgement sends its batch again");
}

#[tokio::test(start_paused = true)]
async fn the_split_form_carries_20_mib_with_every_third_acknowledgement_lost() {
    lost_acknowledgements(Form::Split).await;
}

#[tokio::test(start_paused = true)]
async fn the_serial_form_carries_20_mib_with_every_third_acknowledgement_lost() {
    lost_acknowledgements(Form::Serial).await;
}

/// A window that the target does not read refuses `full`: the batch after
/// it goes again, the same number, and no later number goes; past the
/// bound of the tries the session fails and names the reason.
#[tokio::test(start_paused = true)]
async fn a_full_window_is_a_refusal_and_the_sender_s_window_does_not_move() {
    for form in [Form::Split, Form::Serial] {
        let plan = Plan { room: 1, target_reads: false, ..Plan::default() };
        let stand_in = StandIn::new(plan);
        let (_, end) = carry(opened(&stand_in, form).await, pattern(3 * 256 * 1024, 3)).await;
        assert!(matches!(&end, End::Failed(why) if why.contains("(full)")), "{form:?}: {end:?}");
        stand_in.record(|r| {
            let mut seqs: Vec<u64> = r.sent.keys().map(|(_, seq)| *seq).collect();
            seqs.sort_unstable();
            assert_eq!(seqs, [0, 1], "{form:?}");
            assert!(r.sent[&(ID.to_string(), 1)].len() > 2, "{form:?}: the refused batch went again");
            assert_eq!(r.acks, 1, "{form:?}: only the stored batch is acknowledged");
        });
    }
}

/// `not allowed` and `unknown session` stop the session at the first
/// refusal, with the reason in its end.
#[tokio::test(start_paused = true)]
async fn a_refusal_names_its_reason_and_stops_the_sender_at_once() {
    for (reason, said) in [(Refusal::NotAllowed, "(not allowed)"), (Refusal::UnknownSession, "no longer knows")] {
        for form in [Form::Split, Form::Serial] {
            let stand_in = StandIn::new(Plan { refuse_up: Some(reason), ..Plan::default() });
            let (_, end) = carry(opened(&stand_in, form).await, pattern(1000, 5)).await;
            assert!(matches!(&end, End::Failed(why) if why.contains(said)), "{reason:?} {form:?}: {end:?}");
            assert_eq!(stand_in.resent(), 0, "{reason:?} {form:?}: a refused batch went again");
        }
    }
}

/// A target's end whose count of the bytes down is one more than the bytes
/// that came fails the session, never a clean end.
#[tokio::test(start_paused = true)]
async fn an_end_whose_count_differs_fails_the_session() {
    for form in [Form::Split, Form::Serial] {
        let stand_in = StandIn::new(Plan { down: pattern(5000, 9), total_skew: 1, ..Plan::default() });
        let (_, end) = carry(opened(&stand_in, form).await, pattern(3000, 4)).await;
        assert!(matches!(&end, End::Failed(why) if why.contains("the counts differ")), "{form:?}: {end:?}");
    }
}

/// The offset of a request down acknowledges bytes that the caller's pipe
/// holds, never bytes that wait for room in it.
#[tokio::test(start_paused = true)]
async fn the_bytes_down_are_acknowledged_only_once_the_caller_s_pipe_holds_them() {
    let pipe = 256 * 1024;
    let stand_in = StandIn::new(Plan { down: pattern(4 * MIB, 13), ..Plan::default() });
    let session = opened(&stand_in, Form::Split).await;
    let mut stream = session.stream;
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(stand_in.record(|r| r.most_at) <= pipe as u64);
    let mut half = vec![0u8; 2 * MIB];
    stream.read_exact(&mut half).await.unwrap();
    tokio::time::sleep(Duration::from_secs(30)).await;
    let most = stand_in.record(|r| r.most_at);
    assert!(most <= (2 * MIB + pipe) as u64, "{most} acknowledged, {} read", 2 * MIB);
    assert!(most >= MIB as u64, "{most}: the bytes read are acknowledged");
}

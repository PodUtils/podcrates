//! The flow of the HTTP road with a caller whose bytes up wait for its
//! bytes down (PodUtils/podssh#222), between the client and a stand-in of
//! the relay role in memory, on a paused clock. SSH's client sends a window
//! adjustment up after each window of bytes down, and reads no more until
//! it went; its own bytes up share that stream. A client that stops reading
//! the bytes up while it writes bytes down then deadlocks.

mod http_road_stand_in;

use std::sync::Arc;
use std::time::Duration;

use http_road_stand_in::{Plan, StandIn, pattern};
use pod_relay::http_road::client::{End, Opened, Target, open};
use pod_relay::http_road::{Form, Tuning};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, oneshot};

const MIB: usize = 1 << 20;
/// The bytes down after which the caller sends a window adjustment up.
const WINDOW: usize = 256 * 1024;
/// A window adjustment.
const ADJUST: &[u8] = b"window adjusted\n";
/// The caller's writes up.
const CHUNK: usize = 64 * 1024;
/// The pipe in memory between the caller and the client, each way.
const PIPE: u64 = 256 * 1024;
/// Far longer than a session that moves: a deadlock fails the test here.
const BOUND: Duration = Duration::from_secs(900);

async fn opened(stand_in: &Arc<StandIn>, form: Form) -> Opened {
    let target = Target { host: "target.test", port: 22, family: None };
    open(&stand_in.road(), &target, "token", Some(form), Tuning::default()).await.expect("the open")
}

/// A caller as SSH's: `up` in chunks, and after each window of bytes down,
/// an adjustment up on the same stream before it reads on. Its stream up
/// ends after the bytes down of `down_len` came. The digest of the bytes
/// down, the adjustments sent, and the session's end.
async fn ssh_like(opened: Opened, up: Vec<u8>, down_len: usize) -> ([u8; 32], usize, End) {
    let (mut from, to) = tokio::io::split(opened.stream);
    let to = Arc::new(Mutex::new(to));
    let (read_all, all_read) = oneshot::channel::<()>();
    let writer = tokio::spawn({
        let to = to.clone();
        async move {
            for chunk in up.chunks(CHUNK) {
                // A session that failed drops the pipe: the writes fail then.
                if to.lock().await.write_all(chunk).await.is_err() {
                    return;
                }
            }
            if all_read.await.is_ok() {
                let _ = to.lock().await.shutdown().await;
            }
        }
    });
    let mut digest = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let (mut came, mut adjusts, mut read_all) = (0usize, 0usize, Some(read_all));
    loop {
        let n = match from.read(&mut buf).await.expect("the bytes down") {
            0 => break,
            n => n,
        };
        digest.update(&buf[..n]);
        came += n;
        while came >= (adjusts + 1) * WINDOW {
            if to.lock().await.write_all(ADJUST).await.is_err() {
                break;
            }
            adjusts += 1;
        }
        if came >= down_len
            && let Some(tx) = read_all.take()
        {
            let _ = tx.send(());
        }
    }
    drop(read_all);
    writer.await.unwrap();
    (digest.finalize().into(), adjusts, opened.ended.get().expect("the session's end"))
}

/// 20 MiB each way end cleanly with a caller as SSH's: the bytes down
/// arrive whole, and the stand-in's target reads each byte up and each
/// adjustment.
async fn twenty_mib_each_way(form: Form) {
    let (up, down) = (pattern(20 * MIB, 7), pattern(20 * MIB, 11));
    let stand_in = StandIn::new(Plan { down: down.clone(), ..Plan::default() });
    let session = opened(&stand_in, form).await;
    assert_eq!(session.form, form);
    let carried = tokio::time::timeout(BOUND, ssh_like(session, up.clone(), down.len())).await;
    let Ok((digest_down, adjusts, end)) = carried else {
        let (read, at) = stand_in.record(|r| (r.up_bytes, r.most_at));
        panic!("{form:?}: no end in {BOUND:?}: {read} bytes up read, {at} bytes down acknowledged");
    };
    assert_eq!(end, End::Closed { code: 1000, reason: "target closed".into() }, "{form:?}");
    assert_eq!(digest_down, <[u8; 32]>::from(Sha256::digest(&down)), "the bytes down, {form:?}");
    assert_eq!(adjusts, down.len() / WINDOW, "{form:?}");
    let expected = (up.len() + adjusts * ADJUST.len()) as u64;
    stand_in.record(|r| assert_eq!((r.up_bytes, r.total), (expected, Some(expected)), "{form:?}"));
}

#[tokio::test(start_paused = true)]
async fn the_serial_form_carries_20_mib_each_way_for_a_caller_as_ssh_s() {
    twenty_mib_each_way(Form::Serial).await;
}

#[tokio::test(start_paused = true)]
async fn the_split_form_carries_20_mib_each_way_for_a_caller_as_ssh_s() {
    twenty_mib_each_way(Form::Split).await;
}

/// A caller that reads nothing: the serial form still takes each of its
/// bytes up, by `up` alone once it holds its bound of bytes down, and holds
/// no more than that bound; the caller then reads each byte down and the
/// end.
#[tokio::test(start_paused = true)]
async fn the_serial_form_takes_the_bytes_up_while_the_caller_reads_nothing() {
    let (up, down) = (pattern(4 * MIB, 3), pattern(16 * MIB, 5));
    let stand_in = StandIn::new(Plan { down: down.clone(), ..Plan::default() });
    let session = opened(&stand_in, Form::Serial).await;
    let (mut from, mut to) = tokio::io::split(session.stream);
    let sent = tokio::time::timeout(BOUND, async {
        to.write_all(&up).await.unwrap();
        to.shutdown().await.unwrap();
    })
    .await;
    assert!(sent.is_ok(), "the client stopped reading the bytes up");
    tokio::time::sleep(Duration::from_secs(60)).await;
    let (read, at, ups) = stand_in.record(|r| (r.up_bytes, r.most_at, r.ups));
    assert_eq!(read, up.len() as u64, "the bytes up that the target read");
    assert!(ups > 0, "no batch went up alone");
    // The pipe, the answers of the channel, and the answer in the writer's
    // hands; the stand-in answers 1 MiB at most.
    let most = PIPE + 5 * MIB as u64;
    assert!(at <= most, "{at} bytes down acknowledged, {most} at most");
    let mut got = Vec::with_capacity(down.len());
    tokio::time::timeout(BOUND, from.read_to_end(&mut got)).await.expect("the end").unwrap();
    assert!(got == down, "the bytes down differ");
    assert_eq!(session.ended.get(), Some(End::Closed { code: 1000, reason: "target closed".into() }));
}

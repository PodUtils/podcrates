//! A link over a resumable session of `pod-session` (call C120, T-311;
//! PodUtils/podnet#466, row E0-2): the session's carrier is cut twice with
//! no word to either end while two streams of the link move 1 MiB each
//! way; the session resumes on a new carrier each time, and each stream
//! goes on with no loss.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_link::{Link, Role, Settings};
use pod_session::client::{self, Outcome};
use pod_session::keep::Keeper;
use pod_session::resume::{self, Next, Note};
use pod_session::{Ask, OsEntropy, Role as SessionRole, far};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::sync::mpsc;

const PIPE: usize = 256 * 1024;
const MIB: usize = 1 << 20;
const LIMIT: Duration = Duration::from_secs(120);

/// The bound of each wait: a guard of a hang, five times longer under an
/// emulator (T-535).
fn limit() -> Duration {
    pod_testkit::host::scaled(LIMIT)
}

fn pattern(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

/// Copy `from` into `to` until `left` (both ways together) runs out.
async fn copy_until<R, W>(from: &mut R, to: &mut W, left: &AtomicI64)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = match from.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        let before = left.fetch_sub(n as i64, Ordering::SeqCst);
        if before <= n as i64 {
            let _ = to.write_all(&buf[..before.max(0) as usize]).await;
            return;
        }
        if to.write_all(&buf[..n]).await.is_err() {
            return;
        }
    }
}

/// A carrier of the session: both ways until `cut` bytes have passed, then
/// both ends are dropped with no word. `None` never cuts.
fn carrier(cut: Option<usize>) -> (DuplexStream, DuplexStream) {
    let (client, relay_client) = tokio::io::duplex(PIPE);
    let (relay_far, far) = tokio::io::duplex(PIPE);
    tokio::spawn(async move {
        let left = AtomicI64::new(cut.map_or(i64::MAX, |c| c as i64));
        let (mut cr, mut cw) = tokio::io::split(relay_client);
        let (mut fr, mut fw) = tokio::io::split(relay_far);
        tokio::select! {
            _ = copy_until(&mut cr, &mut fw, &left) => {}
            _ = copy_until(&mut fr, &mut cw, &left) => {}
        }
    });
    (client, far)
}

/// The far end of the session: a new session gives its application side to
/// `apps`; a resume goes on through the keeper.
fn far_end(
    keeper: Arc<Keeper<DuplexStream>>,
    settings: pod_session::Settings,
    mut carriers: mpsc::UnboundedReceiver<DuplexStream>,
    apps: mpsc::UnboundedSender<DuplexStream>,
) {
    tokio::spawn(async move {
        while let Some(link) = carriers.recv().await {
            let (keeper, apps) = (keeper.clone(), apps.clone());
            tokio::spawn(async move {
                let Ok(accepted) =
                    far::accept(link, SessionRole::NODE, settings, &keeper.sessions, &mut OsEntropy).await
                else {
                    return;
                };
                if accepted.established().resumed {
                    let _ = keeper.run_resumed(accepted).await;
                } else {
                    let (app, link_side) = tokio::io::duplex(PIPE);
                    let _ = apps.send(link_side);
                    keeper.run_new(accepted, app).await;
                }
            });
        }
    });
}

fn digest(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

/// Send `out` on `stream`, end it, and read the bytes of the far end.
async fn both_ways(stream: pod_link::Stream, out: Vec<u8>) -> Vec<u8> {
    let (mut r, mut w) = tokio::io::split(stream);
    let writer = tokio::spawn(async move {
        w.write_all(&out).await.unwrap();
        w.shutdown().await.unwrap();
        w
    });
    let mut back = Vec::new();
    r.read_to_end(&mut back).await.unwrap();
    let _w = writer.await.unwrap();
    back
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_streams_cross_two_resumes_of_the_session_whole() {
    let settings = pod_session::Settings { resume_deadline: Duration::from_secs(30), ..Default::default() };
    let keeper = Arc::new(Keeper::new(Duration::from_secs(30)));
    let (carriers_tx, carriers_rx) = mpsc::unbounded_channel();
    let (apps_tx, mut apps_rx) = mpsc::unbounded_channel();
    far_end(keeper, settings, carriers_rx, apps_tx);

    // Two cuts inside the 4 MiB of the streams; the third carrier holds.
    let cuts = Arc::new(Mutex::new(vec![Some(700_000), Some(1_300_000), None].into_iter()));
    let (client_carrier, far_carrier) = carrier(cuts.lock().unwrap().next().flatten());
    carriers_tx.send(far_carrier).unwrap();
    let client = client::start(client_carrier, Ask::New, settings, &mut OsEntropy).await.unwrap();
    let connect = {
        let (cuts, carriers_tx) = (cuts.clone(), carriers_tx.clone());
        move |_: &pod_session::Ended| {
            let (client_carrier, far_carrier) = carrier(cuts.lock().unwrap().next().flatten());
            carriers_tx.send(far_carrier).unwrap();
            async move { Next::Link(client_carrier) }
        }
    };
    let notes: Arc<Mutex<Vec<Note>>> = Arc::default();
    let note = {
        let notes = notes.clone();
        move |n: Note| notes.lock().unwrap().push(n)
    };
    let (dialer_side, app_end) = tokio::io::duplex(PIPE);
    let session = tokio::spawn(async move { resume::run(client, app_end, connect, note, &mut OsEntropy).await });
    let listener_side = tokio::time::timeout(limit(), apps_rx.recv()).await.unwrap().unwrap();

    let dialer = Link::new(dialer_side, Role::Dialer, Settings::default()).unwrap();
    let listener = Link::new(listener_side, Role::Listener, Settings::default()).unwrap();
    let served = tokio::spawn(async move {
        let mut got = Vec::new();
        for n in 0..2u32 {
            let stream = listener.accept().await.unwrap();
            got.push(tokio::spawn(both_ways(stream, pattern(MIB, 100 + n))));
        }
        let mut digests = Vec::new();
        for g in got {
            digests.push(digest(&g.await.unwrap()));
        }
        (listener, digests)
    });
    let sent: Vec<Vec<u8>> = (0..2u32).map(|n| pattern(MIB, 1 + n)).collect();
    let streams: Vec<_> = sent.iter().map(|_| dialer.open().unwrap()).collect();
    let tasks: Vec<_> =
        streams.into_iter().zip(sent.iter().cloned()).map(|(s, out)| tokio::spawn(both_ways(s, out))).collect();
    let mut back = Vec::new();
    for t in tasks {
        back.push(tokio::time::timeout(limit(), t).await.expect("the streams end within the limit").unwrap());
    }
    let (listener, at_listener) = tokio::time::timeout(limit(), served).await.unwrap().unwrap();

    // The listener accepts in the order of the frames, so compare as sets.
    let mut sent_digests: Vec<_> = sent.iter().map(|b| digest(b)).collect();
    let mut got_digests = at_listener;
    sent_digests.sort();
    got_digests.sort();
    assert_eq!(sent_digests, got_digests, "the dialer's bytes reached the listener whole");
    let mut back_digests: Vec<_> = back.iter().map(|b| digest(b)).collect();
    let mut answered: Vec<_> = (0..2u32).map(|n| digest(&pattern(MIB, 100 + n))).collect();
    back_digests.sort();
    answered.sort();
    assert_eq!(back_digests, answered, "the listener's bytes reached the dialer whole");

    let notes = notes.lock().unwrap().clone();
    let lost = notes.iter().filter(|n| matches!(n, Note::Lost { .. })).count();
    let resumed = notes.iter().filter(|n| matches!(n, Note::Resumed { .. })).count();
    assert_eq!((lost, resumed), (2, 2), "{notes:?}");
    assert!(dialer.ended().is_none() && listener.ended().is_none(), "the link lives across each resume");

    dialer.close().await;
    let _ = tokio::time::timeout(limit(), listener.closed()).await.expect("the link ends");
    let outcome = tokio::time::timeout(limit(), session).await.expect("the session ends").unwrap();
    assert!(matches!(outcome, Outcome::Layer(_)), "{outcome:?}");
}

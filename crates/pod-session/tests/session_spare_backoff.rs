//! A spare link on a road that answers the handshake at once and holds each
//! other byte for 5 s or 13 s (a third party's tunnel of HTTP), on tokio's
//! paused clock. A question on each link through the road waits longer
//! than a spare, so a spare opens and takes the session before the bytes
//! come through, the road drops the bytes of the old link, and the new link
//! is silent again: with a fixed wait, no byte ever moves. The wait of the
//! next spare doubles while the session moves no byte, so a later link
//! lives long enough to carry the bytes.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::sync::mpsc;
use tokio::time::Instant;

use pod_session::client;
use pod_session::keep::Keeper;
use pod_session::record::{self, kind};
use pod_session::resume::{self, Next, Note};
use pod_session::{Ask, Ended, OsEntropy, Role, Settings, far};

const PIPE: usize = 256 * 1024;
const LEN: usize = 1 << 20;
const LIMIT: Duration = Duration::from_secs(120);

/// Copy the records of `from` into `to` in order: those of the handshake at
/// once, each other one `hold` after it came.
async fn held<R, W>(mut from: R, mut to: W, hold: Duration)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<(Instant, Vec<u8>)>();
    let read = async move {
        let mut due = Instant::now();
        loop {
            let mut record = vec![0u8; record::HEADER];
            if from.read_exact(&mut record).await.is_err() {
                return;
            }
            let len = u32::from_be_bytes(record[1..5].try_into().unwrap()) as usize;
            record.resize(record::HEADER + len, 0);
            if from.read_exact(&mut record[record::HEADER..]).await.is_err() {
                return;
            }
            let handshake = matches!(record[0], kind::GREETING | kind::OPEN | kind::ACCEPT | kind::PROOF);
            due = due.max(Instant::now() + if handshake { Duration::ZERO } else { hold });
            let _ = tx.send((due, record));
        }
    };
    let write = async move {
        while let Some((due, bytes)) = rx.recv().await {
            tokio::time::sleep_until(due).await;
            if to.write_all(&bytes).await.is_err() {
                return;
            }
        }
    };
    // The road ends with either half: the bytes that it holds are lost.
    tokio::select! {
        () = read => {}
        () = write => {}
    }
}

/// A link through the road. When one end drops it, the road drops the other
/// end and the bytes that it holds.
fn road(hold: Duration) -> (DuplexStream, DuplexStream) {
    let (client, relay_client) = tokio::io::duplex(PIPE);
    let (relay_far, far) = tokio::io::duplex(PIPE);
    let (cr, cw) = tokio::io::split(relay_client);
    let (fr, fw) = tokio::io::split(relay_far);
    tokio::spawn(async move {
        tokio::select! {
            () = held(cr, fw, hold) => {}
            () = held(fr, cw, hold) => {}
        }
    });
    (client, far)
}

/// A far end that keeps its sessions, as a node does; a new session's
/// target goes to `targets`.
fn far_end(mut links: mpsc::UnboundedReceiver<DuplexStream>, targets: mpsc::UnboundedSender<DuplexStream>) {
    let keeper: Arc<Keeper<DuplexStream>> = Arc::new(Keeper::new(Duration::from_secs(600)));
    let settings = Settings::default();
    tokio::spawn(async move {
        while let Some(link) = links.recv().await {
            let (keeper, targets) = (keeper.clone(), targets.clone());
            tokio::spawn(async move {
                let Ok(accepted) = far::accept(link, Role::NODE, settings, &keeper.sessions, &mut OsEntropy).await
                else {
                    return;
                };
                if accepted.established().resumed {
                    let _ = keeper.run_resumed(accepted).await;
                } else {
                    let (target, app) = tokio::io::duplex(PIPE);
                    let _ = targets.send(app);
                    keeper.run_new(accepted, target).await;
                }
            });
        }
    });
}

fn pattern(seed: u8) -> Vec<u8> {
    (0..LEN).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

/// Write `bytes` to `to` and read as many from `from`, both at once; the
/// digest of what came.
async fn both_ways<R, W>(mut from: R, mut to: W, bytes: Vec<u8>) -> [u8; 32]
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let write = async move { to.write_all(&bytes).await.unwrap() };
    let read = async move {
        let mut got = vec![0u8; LEN];
        from.read_exact(&mut got).await.unwrap();
        Sha256::digest(&got).into()
    };
    tokio::join!(write, read).1
}

/// 1 MiB each way, at once, through links of a road that holds each byte
/// for `hold`, with spare links. The time that the bytes took, and the
/// session's notes with their times.
async fn through(hold: Duration) -> (Duration, Vec<(Duration, Note)>) {
    let (links_tx, links_rx) = mpsc::unbounded_channel();
    let (targets_tx, mut targets_rx) = mpsc::unbounded_channel();
    far_end(links_rx, targets_tx);
    let (client_link, far_link) = road(hold);
    links_tx.send(far_link).unwrap();
    let settings = Settings { spare_links: true, ..Settings::default() };
    let client = client::start(client_link, Ask::New, settings, &mut OsEntropy).await.unwrap();
    let connect = move |_: &Ended| {
        let links_tx = links_tx.clone();
        async move {
            let (client_link, far_link) = road(hold);
            links_tx.send(far_link).unwrap();
            Next::Link(client_link)
        }
    };
    let notes: Arc<Mutex<Vec<(Duration, Note)>>> = Arc::default();
    let start = Instant::now();
    let note = {
        let notes = notes.clone();
        move |n: Note| notes.lock().unwrap().push((start.elapsed(), n))
    };
    let (app, app_end) = tokio::io::duplex(PIPE);
    let session = tokio::spawn(async move { resume::run(client, app_end, connect, note, &mut OsEntropy).await });
    let target = targets_rx.recv().await.unwrap();
    let (target_r, target_w) = tokio::io::split(target);
    let (app_r, app_w) = tokio::io::split(app);
    let (up, down) = (pattern(1), pattern(2));
    let digests: ([u8; 32], [u8; 32]) = (Sha256::digest(&up).into(), Sha256::digest(&down).into());
    let carried = async { tokio::join!(both_ways(target_r, app_w, up), both_ways(app_r, target_w, down)) };
    let got = tokio::time::timeout(LIMIT, carried).await;
    let took = start.elapsed();
    session.abort();
    let notes = notes.lock().unwrap().clone();
    let got = got.unwrap_or_else(|_| panic!("the 1 MiB each way did not come in {LIMIT:?}: {notes:?}"));
    assert_eq!(got, digests, "the bytes came whole, each way");
    (took, notes)
}

/// The quiet of each spare that opened.
fn spares(notes: &[(Duration, Note)]) -> Vec<Duration> {
    notes.iter().filter_map(|(_, n)| if let Note::Spare { quiet } = n { Some(*quiet) } else { None }).collect()
}

#[tokio::test(start_paused = true)]
async fn a_spare_on_a_road_that_holds_each_byte_for_5_s_lets_the_session_move() {
    let (took, notes) = through(Duration::from_secs(5)).await;
    // The first spare waits 3 s; the next waits 6 s, and the bytes come
    // through its link at 5 s.
    assert_eq!(spares(&notes), [Duration::from_secs(3)], "{notes:?}");
    assert!(took < Duration::from_secs(10), "{took:?}: {notes:?}");
}

#[tokio::test(start_paused = true)]
async fn the_wait_of_a_spare_doubles_until_a_link_carries_bytes_held_for_13_s() {
    let (took, notes) = through(Duration::from_secs(13)).await;
    let s = Duration::from_secs;
    assert_eq!(spares(&notes), [s(3), s(6), s(12)], "{notes:?}");
    assert!(took < s(40), "{took:?}: {notes:?}");
}

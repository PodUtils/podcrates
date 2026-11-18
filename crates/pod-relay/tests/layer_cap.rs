//! A session of the resumable layer under a small cap of the relay
//! (PodUtils/podssh#314): a stand-in relay publishes
//! `limits.max_session_bytes` of 2 MiB in its `/relays.json`, and ends each
//! link at 2 MiB both ways, as the relay ends a session with `1009 session
//! byte cap`. The client reads the offer, keeps it, and takes it again from
//! the cache with no read; the kept offer moves the session at three
//! quarters of the cap. 8 MiB go through an echo, with equal SHA-256
//! digests, over more than 4 links, and no link meets its cap.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_relay::cache::Cache;
use pod_relay::forward_layer::{Offer, kept, offer};
use pod_relay::relay::Relay;
use pod_session::client::{self, Outcome};
use pod_session::keep::Keeper;
use pod_session::resume::{self, Next, Note};
use pod_session::{Ask, End, Ended, OsEntropy, Role, Settings, far};
use pod_testkit::host::scaled;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};

const KIB: usize = 1 << 10;
const MIB: usize = 1 << 20;
const PIPE: usize = 256 * KIB;
/// The cap of the stand-in relay on each link, both ways together.
const CAP: usize = 2 * MIB;
const LIMIT: Duration = Duration::from_secs(120);

/// The stand-in relay: its document, and links that end at [`CAP`].
#[derive(Default)]
struct SmallRelay {
    /// The links that met their cap.
    capped: AtomicUsize,
    /// The links that it carried.
    links: AtomicUsize,
}

impl SmallRelay {
    /// Answer one `GET /relays.json` on `stream` with the offer of the layer
    /// and the cap.
    async fn document(mut stream: DuplexStream) {
        let (head, _) = pod_proxy::http::request::read_request(&mut stream, Vec::new()).await.expect("a head");
        assert_eq!(head.target, "/relays.json");
        let body = format!(r#"{{"forward":{{"layer":true}},"limits":{{"max_session_bytes":{CAP}}}}}"#);
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        stream.write_all(format!("{head}{body}").as_bytes()).await.unwrap();
    }

    /// A new link to `keeper`'s far end: the client's end. The relay ends it
    /// when [`CAP`] bytes went through, both ways together.
    fn link(self: &Arc<Self>, keeper: &Arc<Keeper<DuplexStream>>) -> DuplexStream {
        let (client, client_side) = tokio::io::duplex(PIPE);
        let (far_side, far_end) = tokio::io::duplex(PIPE);
        tokio::spawn(far_end_of(keeper.clone(), far_end));
        self.links.fetch_add(1, Ordering::SeqCst);
        let relay = self.clone();
        tokio::spawn(async move {
            let carried = Arc::new(AtomicUsize::new(0));
            let (client_r, client_w) = tokio::io::split(client_side);
            let (far_r, far_w) = tokio::io::split(far_side);
            let met = tokio::select! {
                met = one_way(client_r, far_w, carried.clone()) => met,
                met = one_way(far_r, client_w, carried) => met,
            };
            if met {
                relay.capped.fetch_add(1, Ordering::SeqCst);
            }
        });
        client
    }
}

/// Carry `from` to `to` while the bytes of both ways, in `carried`, are
/// under [`CAP`]; `true` when the cap ended it.
async fn one_way(mut from: impl AsyncRead + Unpin, mut to: impl AsyncWrite + Unpin, carried: Arc<AtomicUsize>) -> bool {
    let mut chunk = vec![0u8; 16 * KIB];
    while let Ok(n @ 1..) = from.read(&mut chunk).await {
        let before = carried.fetch_add(n, Ordering::SeqCst);
        let room = CAP.saturating_sub(before).min(n);
        let sent = to.write_all(&chunk[..room]).await.is_ok();
        if before + n >= CAP {
            return true;
        }
        if !sent {
            return false;
        }
    }
    false
}

/// The far end of the layer on one link, as the relay's role holds it: a
/// new session gets an echo as its target.
async fn far_end_of(keeper: Arc<Keeper<DuplexStream>>, link: DuplexStream) {
    let Ok(accepted) = far::accept(link, Role::SERVE, Settings::default(), &keeper.sessions, &mut OsEntropy).await
    else {
        return;
    };
    if !accepted.established().resumed {
        let (target, echo) = tokio::io::duplex(PIPE);
        tokio::spawn(async move {
            let (mut r, mut w) = tokio::io::split(echo);
            tokio::io::copy(&mut r, &mut w).await
        });
        keeper.run_new(accepted, target).await;
    } else {
        let _ = keeper.run_resumed(accepted).await;
    }
}

/// The offer of the stand-in's document, read, kept in a cache, and taken
/// again from the cache.
async fn offer_from_the_cache(asked: &Relay) -> Offer {
    let (mut ours, theirs) = tokio::io::duplex(64 * KIB);
    tokio::spawn(SmallRelay::document(theirs));
    let read = offer::over(&mut ours, "ua", "relay.test", asked).await.expect("the document");
    let dir = std::env::temp_dir().join(format!("pod-relay-layer-cap-{}", std::process::id()));
    let cache = Cache::in_dirs(vec![dir.clone()]);
    kept::keep_now(&cache, asked, &read);
    let from_the_cache = kept::kept_now(&cache, asked).expect("the offer is kept");
    let _ = std::fs::remove_dir_all(&dir);
    from_the_cache
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_session_moves_before_each_cap_of_the_relay() {
    let asked = Relay { host: "relay.test".into(), port: 443 };
    let offered = offer_from_the_cache(&asked).await;
    assert_eq!((offered.layer, offered.max_session_bytes), (true, Some(CAP as u64)), "{offered:?}");
    let settings = Settings { move_bytes: offered.move_bytes(), ..Settings::default() };

    let relay = Arc::new(SmallRelay::default());
    let keeper = Arc::new(Keeper::new(Duration::from_secs(30)));
    let first = relay.link(&keeper);
    let client = client::start(first, Ask::New, settings, &mut OsEntropy).await.expect("the session");
    let notes: Arc<Mutex<Vec<Note>>> = Arc::default();
    let connect = {
        let (relay, keeper) = (relay.clone(), keeper.clone());
        move |_: &Ended| {
            let link = relay.link(&keeper);
            async move { Next::Link(link) }
        }
    };
    let heard = notes.clone();
    let (app, app_end) = tokio::io::duplex(PIPE);
    let note = move |n: Note| heard.lock().unwrap().push(n);
    let session = tokio::spawn(async move { resume::run(client, app_end, connect, note, &mut OsEntropy).await });

    // At about 16 MiB/s each way, as through a relay; an emulator slows the
    // pace with the handshake of each new link, so the margin keeps its time.
    let sent: Vec<u8> = (0..4 * MIB).map(|i| (i * 7 % 253) as u8).collect();
    let (mut r, mut w) = tokio::io::split(app);
    let writer = {
        let (sent, pace) = (sent.clone(), scaled(Duration::from_millis(2)));
        tokio::spawn(async move {
            for chunk in sent.chunks(32 * KIB) {
                w.write_all(chunk).await.unwrap();
                tokio::time::sleep(pace).await;
            }
            w
        })
    };
    let mut back = vec![0u8; sent.len()];
    tokio::time::timeout(scaled(LIMIT), r.read_exact(&mut back)).await.expect("the echo in time").unwrap();
    assert_eq!(Sha256::digest(&back), Sha256::digest(&sent), "the echo came back whole");
    writer.await.unwrap().shutdown().await.unwrap();
    let outcome = tokio::time::timeout(scaled(LIMIT), session).await.expect("the end in time").unwrap();
    assert!(matches!(&outcome, Outcome::Layer(e) if matches!(e.end, End::LocalEnd | End::Closed(_))), "{outcome:?}");

    let notes = notes.lock().unwrap().clone();
    let moves = notes.iter().filter(|n| matches!(n, Note::Moved { .. })).count();
    let losses: Vec<&Note> = notes.iter().filter(|n| !matches!(n, Note::Moved { .. })).collect();
    assert_eq!(relay.capped.load(Ordering::SeqCst), 0, "no link met its cap; notes {notes:?}");
    assert!(losses.is_empty(), "no loss, no failed move: {losses:?}");
    assert!(moves >= 4, "{moves} moves for 8 MiB under a cap of 2 MiB");
    assert!(relay.links.load(Ordering::SeqCst) > moves, "a link for each move, and the first");
}

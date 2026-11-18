//! A new address of the client on the iroh road with no UDP
//! (PodUtils/podssh#283, ask 4): the iroh relay carries each byte, the
//! client's tunnels through its proxy go silent in the middle of an echo of
//! 8 MiB, and the new address has no network for 45 s, longer than the
//! layer's heartbeat waits. The session resumes and loses no byte, and the
//! client says when its connection to the relay ended and when it came back,
//! as podssh's `m6_exit` asks.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use iroh::EndpointAddr;
use pod_iroh::Dialer;
use pod_session::client::Outcome;
use pod_session::resume::Note;
use pod_session::{End, Hearing, Settings};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::mpsc;
use tokio::time::Instant;

use common::{limit, moving_proxy, options, relay};

const PIPE: usize = 256 * 1024;
const MIB: usize = 1 << 20;
/// How long the new address has no network.
const AWAY: Duration = Duration::from_secs(45);

fn pseudo_random(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

/// A target that echoes each byte, as the target of podssh's `m6_exit`.
async fn echo() -> Result<DuplexStream, String> {
    let (target, mut far) = tokio::io::duplex(PIPE);
    tokio::spawn(async move {
        let (mut r, mut w) = tokio::io::split(&mut far);
        let _ = tokio::io::copy(&mut r, &mut w).await;
        let _ = w.shutdown().await;
    });
    Ok(target)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_session_whose_client_moves_on_the_relay_alone_resumes_whole() {
    let (relay_url, _relay) = relay().await;
    let port = relay_url.port().unwrap();
    let (node_proxy, _, _) = moving_proxy(port).await;
    let (client_proxy, tunnels, moving) = moving_proxy(port).await;

    let far = pod_iroh::bind(&options(&relay_url, node_proxy, true)).await.expect("the far end");
    tokio::time::timeout(limit(), far.online()).await.expect("the far end has its home relay");
    let near = pod_iroh::bind(&options(&relay_url, client_proxy, false)).await.expect("the client");
    tokio::time::timeout(limit(), near.online()).await.expect("the client has its home relay");
    let client_key = near.id();
    let admit = move |key: &iroh::PublicKey| *key == client_key;
    tokio::spawn(pod_iroh::far::serve(far.clone(), Settings::default(), 64 * MIB, echo, admit));

    // The client's lines, the layer's and the relay's, in their order.
    let (said_tx, mut said_rx) = mpsc::unbounded_channel::<(Duration, String)>();
    let started = Instant::now();
    let relay_said = said_tx.clone();
    pod_iroh::home_relay::say_changes(&near, move |line| {
        let _ = relay_said.send((started.elapsed(), line));
    });
    let note = move |note: Note| {
        let _ = said_tx.send((started.elapsed(), format!("{note:?}")));
    };
    let addr = EndpointAddr::new(far.id()).with_relay_url(relay_url.clone());
    let dialer = Dialer::new(near.clone(), addr);
    // podssh's settings of the iroh road: no spare link.
    let settings = Settings { spare_links: false, ..Settings::default() };
    let (app, app_end) = tokio::io::duplex(PIPE);
    let carried = pod_iroh::client::carry_heard(&dialer, app_end, settings, note, Arc::new(Hearing::default()));

    let up = pseudo_random(8 * MIB, 0x0283);
    let talk = async {
        let (mut r, mut w) = tokio::io::split(app);
        let out = up.clone();
        let writer = tokio::spawn(async move {
            w.write_all(&out).await.unwrap();
            w
        });
        let mut got = vec![0u8; up.len()];
        // The address changes once 2 MiB came back, with bytes in flight
        // both ways on the old tunnels.
        r.read_exact(&mut got[..2 * MIB]).await.unwrap();
        moving.now_away(AWAY);
        r.read_exact(&mut got[2 * MIB..]).await.unwrap();
        let mut w = writer.await.unwrap();
        w.shutdown().await.unwrap();
        got
    };
    let bound = pod_testkit::host::scaled(AWAY + Duration::from_secs(105));
    let joined = tokio::time::timeout(bound, async { tokio::join!(carried, talk) }).await;
    let mut said = Vec::new();
    while let Ok((at, line)) = said_rx.try_recv() {
        said.push(format!("{:.1} s: {line}", at.as_secs_f64()));
    }
    let said = said.join("\n");
    let (outcome, back) =
        joined.unwrap_or_else(|_| panic!("the session hung after the move; the client said:\n{said}"));
    assert_eq!(Sha256::digest(&back), Sha256::digest(&up), "the bytes came back whole\n{said}");
    let outcome = outcome.expect("a session");
    assert!(
        matches!(&outcome, Outcome::Layer(ended) if matches!(ended.end, End::LocalEnd | End::Closed(_))),
        "{outcome:?}\n{said}"
    );
    assert!(said.contains("Resumed {"), "the layer resumed the session:\n{said}");
    let lost = "the connection to the iroh relay relay.podssh.test ended";
    assert!(said.contains(lost) && said.contains("; dialling it again"), "the relay's loss:\n{said}");
    assert!(said.contains("the connection to the iroh relay relay.podssh.test is back after"), "{said}");
    assert!(tunnels.load(Ordering::SeqCst) >= 2, "a new tunnel after the move:\n{said}");
    eprintln!("the client said:\n{said}");
    near.close().await;
    far.close().await;
}

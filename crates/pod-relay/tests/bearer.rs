//! Bearers bound to one end-to-end channel (PodUtils/podssh#355), through
//! the channel and a stand-in relay: the operator makes the bearer from the
//! binding that its end gets before its proof, the node checks it with the
//! binding of its end, and the same bearer on another channel is refused by
//! its binding, before the node dials. The relay never sees the bearer.
#![cfg(feature = "grant")]

mod e2e_harness;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use e2e_harness::{Fault, LIMIT, PIPE, echo, echo_back, holds, identity, stand_in};
use pod_e2e::ends::{self, GRANT_REFUSED, Redeem};
use pod_e2e::{Ask, Binding, Error, Refusal};
use pod_identity::PublicKey;
use pod_relay::grant::{Bearers, bearer};

/// The seed of the key that signs the bearers: a key of the user's identity
/// file.
const SIGNER: u8 = 0x55;
/// The seed of the operator's key, which the channel proves.
const OPERATOR: u8 = 0x11;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A node's check of a grant that takes a bearer of [`SIGNER`] with the
/// binding of the channel that carries it.
fn redeem_of(bearers: Arc<Bearers>) -> Arc<Redeem> {
    let signer = identity(SIGNER).public();
    Arc::new(move |peer: &PublicKey, _: &Ask, blob: &[u8]| {
        let binding = ends::binding();
        let taken = |key: &PublicKey| *key == signer;
        bearers.verify(blob, binding.as_ref(), peer, taken, now()).map(|v| v.to_string()).map_err(|e| e.to_string())
    })
}

/// What one session gave.
struct Ran {
    result: Result<PublicKey, Error>,
    echoed: bool,
    opened: usize,
    carried: Vec<u8>,
    lines: Vec<String>,
}

/// How the operator's end gets its grant.
enum Grant {
    /// A bearer of [`SIGNER`] for the binding of this channel, with the
    /// nonce of this byte; `change` flips one byte of it first. The bearer
    /// goes to `kept`.
    Bound { nonce: u8, change: Option<usize>, kept: Arc<Mutex<Vec<u8>>> },
    /// These bytes, as they are.
    Fixed(Vec<u8>),
}

/// One session of the operator with `grant`, to a node whose door refuses
/// each key and whose check of grants takes bearers of `bearers`, through a
/// stand-in relay that keeps each byte.
async fn session(grant: Grant, bearers: Arc<Bearers>) -> Ran {
    let (mut node, lines) = e2e_harness::node(identity(0x22), |_: &PublicKey| Err("the allowlist names nobody".into()));
    node.redeem = Some(redeem_of(bearers));
    let opened = Arc::new(AtomicUsize::new(0));
    let (mut app, app_end) = tokio::io::duplex(PIPE);
    let check = |_: &PublicKey| -> Result<(), String> { Ok(()) };
    let mint = move |_node: PublicKey, binding: Binding| {
        let blob = match grant {
            Grant::Bound { nonce, change, kept } => {
                let holder = identity(OPERATOR).public();
                let mut blob = bearer::mint(&identity(SIGNER), holder, &binding, now(), [nonce; 32]);
                if let Some(at) = change {
                    blob[at] ^= 1;
                }
                *kept.lock().unwrap() = blob.clone();
                blob
            }
            Grant::Fixed(blob) => blob,
        };
        std::future::ready(Some(blob))
    };
    let (cipher, channel) = ends::operator_bound(app_end, identity(OPERATOR), check, None, mint, None, |_| {});
    let (node_link, relay_end) = tokio::io::duplex(PIPE);
    let target = echo(opened.clone());
    let node_task = tokio::spawn(async move {
        ends::serve_asked(node_link, &node, move |_, _: Ask| async move { target().await.map_err(Refusal::NoTarget) })
            .await
    });
    let carried = Arc::new(Mutex::new(Vec::new()));
    stand_in(cipher, relay_end, (Fault::None, Arc::default()), Fault::None, carried.clone());
    let channel = tokio::spawn(channel);
    let said = b"through the bearer";
    let echoed = echo_back(&mut app, said).await.is_ok_and(|back| back == said);
    drop(app);
    let result = tokio::time::timeout(LIMIT, channel).await.expect("the channel ends").unwrap();
    let _ = tokio::time::timeout(LIMIT, node_task).await;
    let carried = carried.lock().unwrap().clone();
    let lines = lines.lock().unwrap().clone();
    Ran { result, echoed, opened: opened.load(Ordering::SeqCst), carried, lines }
}

fn refused_by(ran: &Ran, class: &str) -> bool {
    let refused = matches!(&ran.result, Err(Error::Refused(Refusal::Other(why))) if why == GRANT_REFUSED);
    refused && ran.opened == 0 && ran.lines.iter().any(|l| l.contains(&format!("a bearer refused ({class})")))
}

/// A bearer of its channel lets the operator in; the same bearer on another
/// channel is refused by its binding, by the same node (whose record holds
/// its `jti`) and by a node that never saw it.
#[tokio::test]
async fn a_bearer_holds_on_its_channel_and_is_refused_by_its_binding_on_another() {
    let bearers = Arc::new(Bearers::default());
    let kept = Arc::new(Mutex::new(Vec::new()));
    let first = session(Grant::Bound { nonce: 1, change: None, kept: kept.clone() }, bearers.clone()).await;
    assert!(first.result.is_ok() && first.echoed, "{:?} {:?}", first.result, first.lines);
    assert!(first.lines.iter().any(|l| l.contains("came with the bearer of")), "{:?}", first.lines);
    let blob = kept.lock().unwrap().clone();
    assert_eq!(blob.len(), bearer::LEN);
    assert!(!holds(&first.carried, &blob[blob.len() - 64..]), "the relay saw the bearer");
    let again = session(Grant::Fixed(blob.clone()), bearers).await;
    assert!(refused_by(&again, "binding"), "{:?} {:?}", again.result, again.lines);
    let elsewhere = session(Grant::Fixed(blob), Arc::new(Bearers::default())).await;
    assert!(refused_by(&elsewhere, "binding"), "{:?} {:?}", elsewhere.result, elsewhere.lines);
}

/// A bearer whose `jti` changed on its way fails its signature, and the
/// node opens nothing.
#[tokio::test]
async fn a_changed_bearer_is_refused_by_its_signature() {
    let jti = bearer::MAGIC.len() + 32 + 32 + 8 + 8 + 32;
    let grant = Grant::Bound { nonce: 2, change: Some(jti), kept: Arc::default() };
    let ran = session(grant, Arc::new(Bearers::default())).await;
    assert!(refused_by(&ran, "signature"), "{:?} {:?}", ran.result, ran.lines);
}

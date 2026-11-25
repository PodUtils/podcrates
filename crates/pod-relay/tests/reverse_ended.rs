//! The node's run loop on a carrier's wire whose stream ended for good
//! (PodUtils/podssh#359): `ConnectError::Ended` ends the node as unusable
//! with its own words, and no second socket is asked, where a failed dial
//! connects again.
#![cfg(feature = "pair")]

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pod_core::BoxFuture;
use pod_relay::pair::{self, Pair};
use pod_relay::relay::Relay;
use pod_relay::reverse::wire::{Carrier, Door};
use pod_relay::reverse::{self, Exit, Handler, NodeConfig, Opening, SessionId, Settings, Wire};
use pod_ws::Trust;
use pod_ws::client::ConnectError;
use pod_ws::session::RelaySession;
use tokio::io::DuplexStream;

const LIMIT: Duration = Duration::from_secs(10);
/// How long the node has to end: one that connected again would be past
/// its second try of the backoff by then, and never end.
const ENDS_WITHIN: Duration = Duration::from_secs(8);
const NAME: &str = "podssh-test-pair-0123456789abcdef0";
const WORDS: &str = "the carrier dns:FILE ended, and a stream that this end did not start has one life";

/// A handler that no session reaches.
struct Nobody;

impl Handler for Nobody {
    type Stream = DuplexStream;

    fn open(&self, _id: SessionId) -> Opening<DuplexStream> {
        Box::pin(async { Err("no session in this test".to_string()) })
    }
}

/// A carrier's door whose stream ended: each open is [`WORDS`], counted.
struct Ended(Arc<AtomicUsize>);

impl Door for Ended {
    fn open<'a>(&'a self, _: &'a str, _: Duration) -> BoxFuture<'a, Result<RelaySession<DuplexStream>, ConnectError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(ConnectError::Ended(WORDS.into())) })
    }

    fn label(&self) -> String {
        "an ended carrier".into()
    }
}

/// A pair on a relay that no test reaches, for 72 hours.
fn pair() -> Pair {
    let relay = Relay { host: "127.0.0.1".into(), port: 9 };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let token = |c: &str| format!("testonly{}", c.repeat(56));
    let body = serde_json::json!({
        "name": NAME, "node_token": token("n"), "connect_token": token("c"), "stop_token": token("s"),
        "expires": now + 72 * 3600 * 1000,
    });
    pair::parse(&relay, body.to_string().as_bytes(), now).expect("a test pair")
}

#[tokio::test]
async fn an_ended_stream_ends_the_node_with_its_own_words_and_no_second_socket() {
    let opens = Arc::new(AtomicUsize::new(0));
    let (trust, ctx) = (Trust::Default, support::context());
    let mut config = NodeConfig {
        pair: pair(),
        label: None,
        trust: &trust,
        ctx: &ctx,
        timeout: LIMIT,
        settings: Settings::default(),
        repair: None,
        wire: Wire::Carrier(Carrier::of(Ended(opens.clone()))),
        say: None,
    };
    let ran = reverse::run(&mut config, Arc::new(Nobody), std::future::pending::<()>());
    let exit =
        tokio::time::timeout(ENDS_WITHIN, ran).await.expect("the node ended; one that connected again never ends");
    match exit {
        Exit::Unusable(why) => assert_eq!(why, WORDS),
        other => panic!("{other:?}"),
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(opens.load(Ordering::SeqCst), 1, "an ended stream is never asked again");
}

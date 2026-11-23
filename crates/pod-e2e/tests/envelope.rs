//! The destination envelope, the forwarder that carries it, the relay's
//! view, and the check of the suites: a destination seals to one opaque
//! size, opens at the far end of the same session and pair, fails by name at
//! another session or another pair, and crosses a relay without a key.

mod common;

use std::time::Instant;

use common::{PIPE, pair};
use pod_e2e::envelope::{Destination, EnvelopeError, Forwarder, SIZE};
use pod_e2e::record::{self, Settings, Suite};
use pod_e2e::{Channel, Error, Hop, Inspect, Pair};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

/// One handshake, with both ends bound to the pair that the relay
/// advertises.
async fn bound(relay: &str, pair_id: &str) -> (Channel<DuplexStream>, Channel<DuplexStream>) {
    let (mut operator, mut node) = pair().await;
    let advertised = Pair::named(relay, pair_id);
    operator.bind(advertised).unwrap();
    node.bind(advertised).unwrap();
    (operator, node)
}

#[tokio::test]
async fn a_destination_seals_to_one_size_and_opens_at_the_far_end() {
    let (operator, node) = bound("relay-a", "pair-1").await;
    let sealed = Destination::seal(&operator, "db.internal", 5432, "postgres").unwrap();
    assert_eq!(sealed.bytes().len(), SIZE);
    assert!(!sealed.bytes().windows(11).any(|w| w == b"db.internal"), "no host on the wire");
    assert!(!sealed.bytes().windows(8).any(|w| w == b"postgres"), "no service on the wire");
    let destination = Destination::open(&node, sealed.bytes()).unwrap();
    assert_eq!((destination.host(), destination.port(), destination.service()), ("db.internal", 5432, "postgres"));
    assert_eq!(destination.authority(), "db.internal:5432");
    assert_eq!(Destination::open(&node, sealed.bytes()).unwrap(), destination);
    // One destination of one session seals to the same bytes: the identifier
    // is stable for the relay, and the far end recomputes it.
    assert_eq!(Destination::seal(&operator, "db.internal", 5432, "postgres").unwrap(), sealed);
    let other = Destination::seal(&operator, "other.internal", 22, "ssh").unwrap();
    assert_eq!(other.bytes().len(), SIZE, "another destination, the same size");
    assert_ne!(other.id(), sealed.id());
    assert_eq!(Destination::open(&node, other.bytes()).unwrap().authority(), "other.internal:22");
}

#[tokio::test]
async fn an_envelope_of_another_session_or_another_pair_is_not_for_this_session() {
    // A relay splices the prelude of one session into another session: the
    // key of the second session does not open it.
    let (one, _) = bound("relay-a", "pair-1").await;
    let (_, two) = bound("relay-a", "pair-1").await;
    let sealed = Destination::seal(&one, "db.internal", 5432, "postgres").unwrap();
    let refused = Destination::open(&two, sealed.bytes()).unwrap_err();
    assert!(matches!(refused, EnvelopeError::NotForThisSession), "{refused}");
    assert!(refused.to_string().contains("not for this session"), "{refused}");
    // A relay that advertises one pair to one end and another pair to the
    // other end: the two ends derive two keys, and the envelope fails.
    let (mut operator, mut node) = pair().await;
    operator.bind(Pair::named("relay-a", "pair-1")).unwrap();
    node.bind(Pair::named("relay-a", "pair-2")).unwrap();
    let sealed = Destination::seal(&operator, "db.internal", 5432, "postgres").unwrap();
    let refused = Destination::open(&node, sealed.bytes()).unwrap_err();
    assert!(matches!(refused, EnvelopeError::NotForThisSession), "{refused}");
}

#[tokio::test]
async fn a_channel_without_a_pair_has_no_envelope() {
    let (unbound, _) = pair().await;
    let refused = Destination::seal(&unbound, "db.internal", 5432, "postgres").unwrap_err();
    assert!(matches!(refused, EnvelopeError::Unbound), "{refused}");
    let (operator, _) = bound("relay-a", "pair-1").await;
    let sealed = Destination::seal(&operator, "db.internal", 5432, "postgres").unwrap();
    let (_, other) = pair().await;
    let refused = Destination::open(&other, sealed.bytes()).unwrap_err();
    assert!(matches!(refused, EnvelopeError::Unbound), "{refused}");
}

#[tokio::test]
async fn a_forwarder_carries_the_prelude_and_the_far_end_opens_it() {
    let (operator, node) = bound("relay-a", "pair-1").await;
    let sealed = Destination::seal(&operator, "db.internal", 5432, "postgres").unwrap();
    let (mut client, mut relay_in) = tokio::io::duplex(PIPE);
    let (mut relay_out, mut far) = tokio::io::duplex(PIPE);
    client.write_all(sealed.bytes()).await.unwrap();
    client.flush().await.unwrap();
    let forwarder = Forwarder::new();
    let seen = forwarder.forward(&mut relay_in, &mut relay_out).await.unwrap();
    assert_eq!(seen.bytes(), sealed.bytes(), "the relay carries the bytes whole");
    assert_eq!(seen.id(), sealed.id());
    assert_eq!(forwarder.route(&seen), sealed.id());
    assert!(!seen.bytes().windows(11).any(|w| w == b"db.internal"));
    let mut got = [0u8; SIZE];
    far.read_exact(&mut got).await.unwrap();
    let destination = Destination::open(&node, &got).unwrap();
    assert_eq!(destination.authority(), "db.internal:5432");
    assert_eq!(destination.service(), "postgres");
}

#[tokio::test]
async fn the_relay_view_shows_no_destination_no_plaintext_and_no_key() {
    let (operator, _) = bound("relay-a", "pair-1").await;
    let sealed = Destination::seal(&operator, "db.internal", 5432, "postgres").unwrap();
    let now = Instant::now();
    let mut records = operator.exporter().unwrap().records(Suite::ChaCha20Poly1305, Settings::DEFAULT, now).unwrap();
    let wire = records.seal(now, 1, 0, 0, b"the secret payload").unwrap();
    // A key of the same session, which the relay must not see either.
    let mut key = [0u8; 32];
    operator.exporter().unwrap().export("test key", b"relay view", &mut key).unwrap();
    let key_hex = pod_e2e::legs::hex(&key);
    let view = Inspect::relay(&sealed.prelude(), &wire);
    let rendered = view.render();
    let debug = format!("{view:?}");
    for text in [&rendered, &debug] {
        for needle in ["db.internal", "postgres", "the secret payload", key_hex.as_str()] {
            assert!(!text.contains(needle), "{needle} in {text}");
        }
        assert!(!text.to_lowercase().contains("key"), "a key in {text}");
    }
    assert_eq!(view.hop(), Hop::Relay);
    assert_eq!(view.fields().iter().map(|f| f.name).collect::<Vec<_>>(), ["prelude id", "prelude", "wire"]);
    assert!(rendered.contains("tunnel --inspect, relay:"));
    assert!(rendered.contains(&pod_e2e::legs::hex(&sealed.id())), "the relay routes by the identifier");
}

#[test]
fn a_suite_that_does_not_match_is_the_config_class_before_a_record() {
    let refused = record::check_suites(&[Suite::Aes256Gcm], &[Suite::ChaCha20Poly1305]).unwrap_err();
    assert!(matches!(refused, Error::NoSuite(_)), "{refused}");
    let failure: pod_core::Failure = refused.into();
    assert_eq!(failure.class, pod_core::Class::Config);
    assert_eq!(failure.class.exit_code(), 78);
    assert_eq!(record::check_suites(&Suite::ALL, &[Suite::Aes256Gcm]).unwrap(), Suite::Aes256Gcm);
}

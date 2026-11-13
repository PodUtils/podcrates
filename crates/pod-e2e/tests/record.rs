//! The exporter and the records over a real handshake: both ends derive
//! the same key for a label, another label gives another key, the two ends
//! agree on a suite (or share none), and records go both ways over the
//! stream that the channel gives back. A document of the net that the node
//! sends after its verdict comes before its suites, and goes to the
//! operator's documents.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use pod_e2e::net::{Docs, Held};
use pod_e2e::record::{self, HEADER, Opened, Settings, Suite};
use pod_e2e::{self as e2e, Channel, Error};
use pod_identity::{Identity, PublicKey};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

const PIPE: usize = 64 * 1024;

/// The two channels of one handshake: the operator's, then the node's.
async fn pair() -> (Channel<DuplexStream>, Channel<DuplexStream>) {
    let (a, b) = tokio::io::duplex(PIPE);
    let operator = tokio::spawn(async move {
        let me = Identity::from_seed(&[1; 32]);
        e2e::initiate(a, &me).await?.accept(None).await
    });
    let node = Identity::from_seed(&[2; 32]);
    let node = e2e::respond(b, &node).await.unwrap().admit().await.unwrap();
    (operator.await.unwrap().unwrap(), node)
}

fn export(channel: &Channel<DuplexStream>, label: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    channel.exporter().unwrap().export(label, b"context", &mut out).unwrap();
    out
}

#[tokio::test]
async fn both_ends_derive_the_same_key_and_another_label_another_key() {
    let (operator, node) = pair().await;
    assert_eq!(operator.handshake_hash(), node.handshake_hash());
    assert_eq!(export(&operator, "podnet link"), export(&node, "podnet link"));
    assert_ne!(export(&operator, "podnet link"), export(&operator, "podnet other"));
    // The key is not the hash, which the relay can compute.
    assert_ne!(Some(export(&operator, "podnet link")), operator.handshake_hash());
    let (other, _) = pair().await;
    assert_ne!(export(&operator, "podnet link"), export(&other, "podnet link"), "another handshake");
    assert!(operator.exporter().unwrap().is_initiator() && !node.exporter().unwrap().is_initiator());
}

#[tokio::test]
async fn the_ends_take_the_initiator_s_first_suite_that_both_offer() {
    let (mut operator, mut node) = pair().await;
    let asked = tokio::spawn(async move { operator.agree(&[Suite::Aes256Gcm, Suite::ChaCha20Poly1305]).await });
    let suite = node.agree(&[Suite::ChaCha20Poly1305, Suite::Aes256Gcm]).await.unwrap();
    assert_eq!((asked.await.unwrap().unwrap(), suite), (Suite::Aes256Gcm, Suite::Aes256Gcm));
    let (mut operator, mut node) = pair().await;
    let asked = tokio::spawn(async move { operator.agree(&[Suite::Aes256Gcm]).await });
    let refused = node.agree(&[Suite::ChaCha20Poly1305]).await.unwrap_err();
    assert!(matches!(refused, Error::NoSuite(_)), "{refused}");
    assert!(matches!(asked.await.unwrap(), Err(Error::NoSuite(_))));
    let failure: pod_core::Failure = refused.into();
    assert_eq!(failure.class, pod_core::Class::Config, "a suite that does not match is a setting");
}

/// One record from `stream`, by the length in its header.
async fn read_record(stream: &mut DuplexStream) -> Vec<u8> {
    let mut record = vec![0u8; HEADER];
    stream.read_exact(&mut record).await.unwrap();
    record.resize(record::record_len(&record).unwrap(), 0);
    stream.read_exact(&mut record[HEADER..]).await.unwrap();
    record
}

#[tokio::test]
async fn records_of_the_agreed_suite_cross_the_stream_both_ways() {
    let (mut operator, mut node) = pair().await;
    let now = Instant::now();
    let settings = Settings { pad: true, ..Settings::DEFAULT };
    let agreed = tokio::spawn(async move {
        let suite = operator.agree(&Suite::ALL).await?;
        let records = operator.exporter().unwrap().records(suite, settings, now)?;
        Ok::<_, Error>((records, operator.into_transport()))
    });
    let suite = node.agree(&Suite::ALL).await.unwrap();
    assert_eq!(suite, Suite::ChaCha20Poly1305, "the default first");
    let mut theirs = node.exporter().unwrap().records(suite, settings, now).unwrap();
    let mut node = node.into_transport();
    let (mut mine, mut operator) = agreed.await.unwrap().unwrap();
    for i in 0..3u32 {
        let sealed = mine.seal(now, 1, 0, i, format!("operator {i}").as_bytes()).unwrap();
        operator.write_all(&sealed).await.unwrap();
        let got = theirs.open(now, &read_record(&mut node).await).unwrap();
        let Opened::Record(r) = got else { panic!("{got:?}") };
        assert_eq!((r.stream, r.payload), (i, format!("operator {i}").into_bytes()));
        let sealed = theirs.seal(now, 1, 0, i, b"node").unwrap();
        assert_eq!(sealed.len(), HEADER + 128 + 16, "padded to its bucket");
        node.write_all(&sealed).await.unwrap();
        assert!(matches!(mine.open(now, &read_record(&mut operator).await), Ok(Opened::Record(_))));
    }
}

/// A document of one owner whose bytes are its serial; each member may have
/// it.
struct Serial(u64, Mutex<Vec<Vec<u8>>>);

impl Docs for Serial {
    fn held(&self) -> Option<(Held, Vec<u8>)> {
        Some((Held { owner: [7; 32], serial: self.0 }, self.0.to_string().into_bytes()))
    }

    fn shares_with(&self, _: &PublicKey) -> bool {
        true
    }

    fn take(&self, _: &PublicKey, bytes: &[u8]) -> Result<(), String> {
        self.1.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }
}

#[tokio::test]
async fn the_node_s_newer_document_before_its_suites_goes_to_the_operator_s_documents() {
    let (a, b) = tokio::io::duplex(PIPE);
    let mine = Arc::new(Serial(1, Mutex::default()));
    let docs = mine.clone();
    let operator = tokio::spawn(async move {
        let me = Identity::from_seed(&[1; 32]);
        let mut channel = e2e::initiate_with(a, &me, Some(docs)).await?.accept(None).await?;
        channel.agree(&Suite::ALL).await
    });
    let node = Identity::from_seed(&[2; 32]);
    let theirs: Arc<dyn Docs> = Arc::new(Serial(2, Mutex::default()));
    let mut node = e2e::respond_with(b, &node, Some(theirs)).await.unwrap().admit().await.unwrap();
    assert_eq!(node.agree(&Suite::ALL).await.unwrap(), Suite::ChaCha20Poly1305);
    assert_eq!(operator.await.unwrap().unwrap(), Suite::ChaCha20Poly1305);
    assert_eq!(*mine.1.lock().unwrap(), vec![b"2".to_vec()]);
}

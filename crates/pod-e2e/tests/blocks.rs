//! The gate of no blocking call on a seal and an open of the record layer,
//! in a test binary of its own (T-1082): the other test threads of a
//! binary share the lock of the address space and of the allocator, and a
//! wait of this thread on one of them counts as a voluntary switch that
//! the layer did not make. This binary has one test.

use std::time::Instant;

use pod_e2e::record::{Opened, Settings, Suite};
use pod_e2e::{self as e2e};
use pod_identity::Identity;

const RECORDS: u64 = 1000;

/// A seal and an open block on nothing (T-838, VF-3): 1000 of each make no
/// voluntary switch of context of the thread.
#[test]
fn a_seal_and_an_open_block_on_nothing() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (mut mine, mut theirs) = runtime.block_on(async {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let operator = tokio::spawn(async move {
            let me = Identity::from_seed(&[1; 32]);
            e2e::initiate(a, &me).await?.accept(None).await
        });
        let node = Identity::from_seed(&[2; 32]);
        let node = e2e::respond(b, &node).await.unwrap().admit().await.unwrap();
        let operator = operator.await.unwrap().unwrap();
        let (now, suite) = (Instant::now(), Suite::ChaCha20Poly1305);
        let mine = operator.exporter().unwrap().records(suite, Settings::DEFAULT, now).unwrap();
        let theirs = node.exporter().unwrap().records(suite, Settings::DEFAULT, now).unwrap();
        (mine, theirs)
    });
    let (now, payload) = (Instant::now(), vec![7u8; 1200]);
    let (opened, switches) = pod_testkit::switches::count(|| {
        (0..RECORDS as u32)
            .filter(|i| {
                let sealed = mine.seal(now, 1, 0, *i, &payload).unwrap();
                matches!(theirs.open(now, &sealed), Ok(Opened::Record(_)))
            })
            .count()
    });
    assert_eq!(opened as u64, RECORDS);
    if let Some(switches) = switches {
        assert_eq!(switches, 0, "{switches} voluntary switches in {RECORDS} seals and opens");
    }
}

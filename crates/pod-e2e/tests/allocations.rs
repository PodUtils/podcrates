//! The allocations of the record layer (T-824, VF-1 of the ingest study):
//! a seal makes one allocation, the `Vec` of the record that it gives, and
//! an open one, the `Vec` of the payload; nothing else for each record. The
//! counter is the allocator of this test binary alone.

use std::time::Instant;

use pod_e2e::record::{Opened, Settings, Suite};
use pod_e2e::{self as e2e};
use pod_identity::Identity;

#[global_allocator]
static ALLOC: pod_testkit::alloc::Counting = pod_testkit::alloc::Counting;

const RECORDS: u64 = 1000;

#[tokio::test(flavor = "current_thread")]
async fn a_seal_and_an_open_each_make_one_allocation_for_each_record() {
    let (a, b) = tokio::io::duplex(64 * 1024);
    let operator = tokio::spawn(async move {
        let me = Identity::from_seed(&[1; 32]);
        e2e::initiate(a, &me).await?.accept(None).await
    });
    let node = Identity::from_seed(&[2; 32]);
    let node = e2e::respond(b, &node).await.unwrap().admit().await.unwrap();
    let operator = operator.await.unwrap().unwrap();
    let now = Instant::now();
    let suite = Suite::ChaCha20Poly1305;
    for pad in [false, true] {
        let settings = Settings { pad, ..Settings::DEFAULT };
        let mut mine = operator.exporter().unwrap().records(suite, settings, now).unwrap();
        let mut theirs = node.exporter().unwrap().records(suite, settings, now).unwrap();
        let payload = vec![7u8; 1200];
        // The first records grow what the layer keeps.
        for i in 0..8 {
            let sealed = mine.seal(now, 1, 0, i, &payload).unwrap();
            theirs.open(now, &sealed).unwrap();
        }
        let mut sealed = Vec::with_capacity(RECORDS as usize);
        let ((), seals) = pod_testkit::alloc::count(|| {
            for i in 0..RECORDS {
                sealed.push(mine.seal(now, 1, 0, i as u32, &payload).unwrap());
            }
        });
        let (opened, opens) = pod_testkit::alloc::count(|| {
            sealed.iter().filter(|r| matches!(theirs.open(now, r), Ok(Opened::Record(_)))).count()
        });
        assert_eq!(opened as u64, RECORDS);
        let (seals, opens) = (seals.expect("the counter"), opens.expect("the counter"));
        assert_eq!(seals, RECORDS, "pad {pad}: {seals} allocations for {RECORDS} seals");
        assert_eq!(opens, RECORDS, "pad {pad}: {opens} allocations for {RECORDS} opens");
    }
}

#[test]
fn the_counter_counts_this_thread_only() {
    let ((), made) = pod_testkit::alloc::count(|| {
        drop(std::hint::black_box(vec![1u8; 10]));
        std::thread::spawn(|| drop(std::hint::black_box(vec![2u8; 10]))).join().unwrap();
    });
    // The vector, and the thread's own start on this thread.
    let made = made.expect("the counter");
    assert!((1..=8).contains(&made), "{made}");
}

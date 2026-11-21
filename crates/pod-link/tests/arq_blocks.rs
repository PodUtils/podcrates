//! The gate of no blocking call on a turn of the ARQ, in a test binary of
//! its own (T-1082): the other test threads of a binary share the lock of
//! the address space and of the allocator, and a wait of this thread on
//! one of them counts as a voluntary switch that the ARQ did not make.
//! This binary has one test.

mod arq_common;

use std::time::Instant;

use pod_link::arq::ArqSettings;

use arq_common::{arq, drain, give, pattern};

/// A turn of the ARQ blocks on nothing (T-838, VF-3): 1000 records of 5000
/// bytes go across, with the acknowledgements, and the thread makes no
/// voluntary switch of context.
#[test]
fn a_turn_of_the_arq_blocks_on_nothing() {
    let now = Instant::now();
    let (mut a, mut b) = (arq(ArqSettings::default()), arq(ArqSettings::default()));
    let record = pattern(5000, 3);
    let (got, switches) = pod_testkit::switches::count(|| {
        let mut got = 0;
        for _ in 0..1000 {
            assert!(a.send_record(&record).unwrap());
            let out = drain(&mut a, now);
            give(&mut b, now, &out);
            while let Some(r) = b.recv_record().unwrap() {
                got += usize::from(r == record);
            }
            let acks = drain(&mut b, now);
            give(&mut a, now, &acks);
        }
        got
    });
    assert_eq!(got, 1000);
    if let Some(switches) = switches {
        assert_eq!(switches, 0, "{switches} voluntary switches in 1000 turns");
    }
}

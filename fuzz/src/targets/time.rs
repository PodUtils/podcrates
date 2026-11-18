//! The reader of the zone files of `pod-time` (T-907).

use pod_time::{Cron, Local, Zone};

/// A TZif file from untrusted bytes: no panic. A zone that the reader
/// takes gives an offset at each instant, and each instant that a local
/// time gives has that local time, or ends its gap.
pub fn tzif_zone(data: &[u8]) {
    let Ok(zone) = Zone::from_tzif("fuzz", data) else { return };
    let cron = Cron::parse("30 2 * * *").expect("a cron");
    for utc in [i64::MIN, -(1 << 40), -1, 0, 1_790_000_000, 1 << 40, i64::MAX] {
        let _ = (zone.offset_at(utc), zone.next_change(utc), zone.local(utc));
    }
    for local in [0i64, 1_774_751_400, 1_793_068_200] {
        match zone.instants(local) {
            Local::One(t) => assert_eq!(zone.local(t), local),
            Local::Two(a, b) => assert!(a < b && zone.local(a) == local && zone.local(b) == local),
            Local::Gap(_) => {}
        }
        let _ = cron.next_in(&zone, local);
    }
}

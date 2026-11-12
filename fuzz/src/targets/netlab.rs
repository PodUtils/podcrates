//! The counters of `pod-netlab` (T-207).

/// The output of `nft -j list counters` from a guest
/// (`pod_netlab::parse_counters`; new in `pod-netlab`, T-207), for the
/// table `lab`: the same answer twice; no more counters than bytes; and the
/// judge of the counters against themselves (nothing moved, as after a
/// flushed chain) fails each counter that must move.
pub fn netlab_counters(data: &[u8]) {
    use pod_netlab::{Lab, Must, judge, parse_counters, scenario};
    let Some(text) = crate::text(data) else { return };
    let first = parse_counters(text, "lab");
    assert_eq!(first, parse_counters(text, "lab"));
    let Ok(counters) = first else { return };
    assert!(counters.len() <= text.len());
    for name in counters.names() {
        assert!(counters.get(name).is_some());
    }
    let lab = Lab::new("sn", "dual-nat", scenario("dual-nat").expect("a scenario").knobs).expect("a lab");
    let e = lab.expect();
    let report = judge(&e, &counters, &counters, &Default::default());
    for (counter, must) in &e.counters {
        let subject = format!("counter {}", counter.name());
        let row = report.rows.iter().find(|r| r.subject == subject).expect("a row for each counter");
        if *must == Must::Move {
            assert!(!row.pass, "a counter that did not move passed: {row}");
        }
    }
    assert!(!report.passed());
}

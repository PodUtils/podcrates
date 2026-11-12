//! The judge: a run that agrees passes; a flushed policy chain, a table
//! that is gone, a counter that went back and a wrong answer fail.

use std::collections::BTreeMap;

use pod_netlab::{
    Bound, Count, Counter, Counters, Expectations, Lab, Measured, Must, Probe, Seen, Verdict, judge, scenario,
    scenarios,
};

fn exp(name: &str) -> Expectations {
    Lab::new("sn", name, scenario(name).unwrap().knobs).unwrap().expect()
}

/// The measurement of a run that agrees with each expectation.
fn agreeing(e: &Expectations) -> BTreeMap<Probe, Measured> {
    e.probes
        .iter()
        .map(|x| {
            let mut m = Measured::verdict(x.verdicts[0]);
            match x.seen {
                Some(Seen::Addr(addr)) => m.addr = Some(addr),
                Some(Seen::PortKept) => m.port = Some(40001),
                Some(Seen::PortMoved) => m.port = Some(51234),
                None => {}
            }
            m.number = x.bound.map(|b| match b {
                Bound::AtLeastMs(ms) => ms + 15,
                Bound::Between(_, hi) => u64::from(hi),
            });
            (x.probe, m)
        })
        .collect()
}

fn all(packets: u64) -> Counters {
    let mut c = Counters::default();
    for counter in Counter::ALL {
        c.insert(counter.name(), Count { packets, bytes: packets * 60 });
    }
    c
}

/// The counters after a run that agrees: each counter that must move moved.
fn moved(e: &Expectations, before: &Counters) -> Counters {
    let mut after = before.clone();
    for (counter, must) in &e.counters {
        if *must == Must::Move {
            let b = before.get(counter.name()).unwrap();
            after.insert(counter.name(), Count { packets: b.packets + 4, bytes: b.bytes + 240 });
        }
    }
    after
}

#[test]
fn a_run_that_agrees_passes_in_each_scenario() {
    for s in scenarios() {
        let e = Lab::new("sn", s.name, s.knobs).unwrap().expect();
        let before = all(3);
        let report = judge(&e, &before, &moved(&e, &before), &agreeing(&e));
        assert!(report.passed(), "{}:\n{report}", s.name);
        assert_eq!(report.rows.len(), e.probes.len() + Counter::ALL.len());
    }
}

/// `nft flush chain inet lab forward_policy`: the counters stay, and the
/// chain's policy drops each forwarded flow. Both runs fail on the
/// counters, also where the guest measured what the lab expects.
#[test]
fn a_flushed_chain_fails_the_check() {
    for name in ["dual-nat", "web-only", "hairpin-web", "cgnat"] {
        let e = exp(name);
        let before = all(9);
        let report = judge(&e, &before, &before, &agreeing(&e));
        assert!(!report.passed(), "{name}");
        let failed: Vec<&str> = report.failures().map(|r| r.subject.as_str()).collect();
        assert!(failed.contains(&"counter forwarded"), "{name}: {failed:?}");
        assert!(report.to_string().contains("FAIL counter forwarded: expected it moves, got +0 packets"), "{report}");
        let mut dropped = agreeing(&e);
        for m in dropped.values_mut().filter(|m| m.verdict == Verdict::Allow) {
            *m = Measured::verdict(Verdict::Dropped);
        }
        assert!(!judge(&e, &before, &before, &dropped).passed(), "{name}");
    }
}

#[test]
fn a_table_that_is_gone_fails() {
    let e = exp("dual-nat");
    let before = all(0);
    let report = judge(&e, &before, &Counters::default(), &agreeing(&e));
    assert!(!report.passed());
    assert_eq!(report.failures().count(), Counter::ALL.len());
    assert!(report.to_string().contains("FAIL counter return: expected it moves, got no counter: the table is gone"));
}

#[test]
fn a_counter_that_went_back_fails() {
    let e = exp("dual-nat");
    let before = all(50);
    let mut after = moved(&e, &before);
    after.insert("broken", Count { packets: 1, bytes: 60 });
    let report = judge(&e, &before, &after, &agreeing(&e));
    let failed: Vec<String> = report.failures().map(|r| r.to_string()).collect();
    assert_eq!(
        failed,
        ["FAIL counter broken: expected it stays, got 50 packets before, 1 after: the counter went back"]
    );
}

#[test]
fn a_counter_that_must_stay_and_moved_fails() {
    let e = exp("pmtu-ok");
    let before = all(0);
    let mut after = moved(&e, &before);
    after.insert("dropped", Count { packets: 2, bytes: 120 });
    let report = judge(&e, &before, &after, &agreeing(&e));
    let failed: Vec<&str> = report.failures().map(|r| r.subject.as_str()).collect();
    assert_eq!(failed, ["counter dropped"]);
}

#[test]
fn a_wrong_answer_fails_its_row() {
    let e = exp("dual-nat");
    let before = all(0);
    let after = moved(&e, &before);
    let check = |change: &dyn Fn(&mut BTreeMap<Probe, Measured>), subject: &str| {
        let mut m = agreeing(&e);
        change(&mut m);
        let report = judge(&e, &before, &after, &m);
        let failed: Vec<&str> = report.failures().map(|r| r.subject.as_str()).collect();
        assert_eq!(failed, [subject], "{report}");
    };
    let v4 = pod_netlab::Family::V4;
    check(&|m| m.get_mut(&Probe::Tcp(v4, 443)).unwrap().verdict = Verdict::Refused, "tcp4-443");
    check(&|m| m.get_mut(&Probe::Whoami(v4)).unwrap().addr = Some("10.77.0.2".parse().unwrap()), "whoami4");
    check(&|m| m.get_mut(&Probe::Srcport(v4)).unwrap().port = Some(40002), "srcport4");
    check(&|m| m.get_mut(&Probe::UdpBurst(v4)).unwrap().number = Some(99), "udp-burst4");
    check(&|m| m.get_mut(&Probe::Whoami(v4)).unwrap().addr = None, "whoami4");
    check(&|m| _ = m.remove(&Probe::Resolve), "resolve");
    check(&|m| _ = m.insert(Probe::IspWhoami, Measured::verdict(Verdict::Allow)), "isp-whoami");
}

#[test]
fn a_bound_of_time_is_a_floor() {
    let e = exp("satellite");
    let before = all(0);
    let after = moved(&e, &before);
    let mut m = agreeing(&e);
    m.get_mut(&Probe::Rtt(pod_netlab::Family::V6)).unwrap().number = Some(599);
    let report = judge(&e, &before, &after, &m);
    let failed: Vec<String> = report.failures().map(|r| r.to_string()).collect();
    assert_eq!(failed.len(), 1);
    assert!(failed[0].starts_with("FAIL rtt6: expected ALLOW, at least 600 ms (passes), got ALLOW, 599"), "{failed:?}");
}

#[test]
fn an_empty_report_does_not_pass() {
    assert!(!pod_netlab::Report::default().passed());
}

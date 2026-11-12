//! The check: the guest only measures; the judge compares each measured
//! verdict with its expectation, and the router's counters after the
//! probes with the counters before them. A flushed policy chain leaves its
//! counters still, and the judge fails it.

use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;

use crate::counters::Counters;
use crate::expect::{Bound, Expect, Expectations, Must, Seen, Verdict};
use crate::plan::ruleset::Counter;
use crate::probe::{Probe, SRCPORT};

/// What the guest (or the world) measured for one probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measured {
    pub verdict: Verdict,
    /// The address that the far end reported, or that the resolver gave.
    pub addr: Option<IpAddr>,
    /// The source port that the far end reported (`srcport`).
    pub port: Option<u16>,
    /// `rtt` and `bulk`: milliseconds; `udp-burst`: echoed datagrams.
    pub number: Option<u64>,
}

impl Measured {
    pub fn verdict(verdict: Verdict) -> Measured {
        Measured { verdict, addr: None, port: None, number: None }
    }
}

/// One row of a report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub pass: bool,
    /// A probe's name, or `counter NAME`.
    pub subject: String,
    pub expected: String,
    pub got: String,
}

impl fmt::Display for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mark = if self.pass { "PASS" } else { "FAIL" };
        write!(f, "{mark} {}: expected {}, got {}", self.subject, self.expected, self.got)
    }
}

/// The rows of one check, the probes first, then the counters.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Report {
    pub rows: Vec<Row>,
}

impl Report {
    pub fn passed(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|r| r.pass)
    }

    pub fn failures(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|r| !r.pass)
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in &self.rows {
            writeln!(f, "{row}")?;
        }
        Ok(())
    }
}

fn verdicts(list: &[Verdict]) -> String {
    list.iter().map(|v| v.word()).collect::<Vec<_>>().join(" or ")
}

fn seen_text(seen: Seen) -> String {
    match seen {
        Seen::Addr(addr) => format!("from {addr}"),
        Seen::PortKept => format!("port {SRCPORT}"),
        Seen::PortMoved => format!("a port other than {SRCPORT}"),
    }
}

fn bound_text(bound: Bound) -> String {
    match bound {
        Bound::AtLeastMs(ms) => format!("at least {ms} ms"),
        Bound::Between(lo, hi) => format!("{lo} to {hi} echoes"),
    }
}

/// Whether `m` agrees with `e`, and what it got.
fn probe_row(e: &Expect, m: Option<&Measured>) -> Row {
    let mut expected = verdicts(&e.verdicts);
    let pass_verdict = m.is_some_and(|m| e.verdicts.contains(&m.verdict));
    let mut pass = pass_verdict;
    let mut got = m.map_or("no measurement".to_string(), |m| m.verdict.word().to_string());
    if let Some(m) = m.filter(|m| pass_verdict && m.verdict == Verdict::Allow) {
        if let Some(seen) = e.seen {
            expected.push_str(&format!(" {}", seen_text(seen)));
            let ok = match seen {
                Seen::Addr(addr) => m.addr == Some(addr),
                Seen::PortKept => m.port == Some(SRCPORT),
                Seen::PortMoved => m.port.is_some_and(|p| p != SRCPORT),
            };
            got.push_str(&match (m.addr, m.port) {
                (_, Some(port)) if matches!(seen, Seen::PortKept | Seen::PortMoved) => format!(" port {port}"),
                (Some(addr), _) => format!(" from {addr}"),
                _ => " with no address".to_string(),
            });
            pass &= ok;
        }
        if let Some(bound) = e.bound {
            expected.push_str(&format!(", {}", bound_text(bound)));
            let ok = match (bound, m.number) {
                (Bound::AtLeastMs(ms), Some(n)) => n >= ms,
                (Bound::Between(lo, hi), Some(n)) => n >= u64::from(lo) && n <= u64::from(hi),
                (_, None) => false,
            };
            got.push_str(&m.number.map_or(", no number".to_string(), |n| format!(", {n}")));
            pass &= ok;
        }
    }
    expected.push_str(&format!(" ({})", e.why));
    Row { pass, subject: e.probe.name(), expected, got }
}

fn counter_row(counter: Counter, must: Option<Must>, before: &Counters, after: &Counters) -> Row {
    let subject = format!("counter {}", counter.name());
    let expected = match must {
        Some(Must::Move) => "it moves",
        Some(Must::Stay) => "it stays",
        None => "it is present",
    }
    .to_string();
    let (Some(b), Some(a)) = (before.get(counter.name()), after.get(counter.name())) else {
        return Row { pass: false, subject, expected, got: "no counter: the table is gone".into() };
    };
    if a.packets < b.packets || a.bytes < b.bytes {
        let got = format!("{} packets before, {} after: the counter went back", b.packets, a.packets);
        return Row { pass: false, subject, expected, got };
    }
    let delta = a.packets - b.packets;
    let pass = match must {
        Some(Must::Move) => delta > 0,
        Some(Must::Stay) => delta == 0,
        None => true,
    };
    Row { pass, subject, expected, got: format!("+{delta} packets") }
}

/// The report of one check: a row for each expected probe, a row for each
/// measured probe with no expectation, and a row for each counter.
pub fn judge(
    expectations: &Expectations,
    before: &Counters,
    after: &Counters,
    measured: &BTreeMap<Probe, Measured>,
) -> Report {
    let mut rows: Vec<Row> = expectations.probes.iter().map(|e| probe_row(e, measured.get(&e.probe))).collect();
    for probe in measured.keys().filter(|p| expectations.get(**p).is_none()) {
        rows.push(Row {
            pass: false,
            subject: probe.name(),
            expected: "no probe of this name".into(),
            got: "a measurement".into(),
        });
    }
    for counter in Counter::ALL {
        let must = expectations.counters.iter().find(|(c, _)| *c == counter).map(|(_, m)| *m);
        rows.push(counter_row(counter, must, before, after));
    }
    Report { rows }
}

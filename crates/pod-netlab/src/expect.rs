//! The derivation (issue 178, "sand net check"): from the knobs alone, the
//! expected verdict of each probe, what it must see, its bounds, and which
//! named counters must move and which must not.

use std::collections::BTreeSet;
use std::fmt;
use std::net::IpAddr;

use crate::lab::Lab;
use crate::plan::ruleset::Counter;
use crate::probe::{Probe, probes};

mod path;

/// The verdict of one probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    /// An answer.
    Allow,
    /// No answer.
    Dropped,
    /// A reset or a port-unreachable.
    Refused,
    /// A local errno: the guest has no route or no address of the family.
    Deny,
    /// The resolver gave no address.
    Unresolved,
    /// Connected, then stalled.
    Timeout,
}

impl Verdict {
    pub const ALL: [Verdict; 6] =
        [Verdict::Allow, Verdict::Dropped, Verdict::Refused, Verdict::Deny, Verdict::Unresolved, Verdict::Timeout];

    pub fn word(self) -> &'static str {
        match self {
            Verdict::Allow => "ALLOW",
            Verdict::Dropped => "DROPPED",
            Verdict::Refused => "REFUSED",
            Verdict::Deny => "DENY",
            Verdict::Unresolved => "UNRESOLVED",
            Verdict::Timeout => "TIMEOUT",
        }
    }

    pub fn from_word(word: &str) -> Option<Verdict> {
        Verdict::ALL.into_iter().find(|v| v.word() == word)
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// What a probe that passes must see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// The address that the far end reports (`you=ADDR`), or the address
    /// that the resolver gives.
    Addr(IpAddr),
    /// `srcport`: the far end reports port 40001.
    PortKept,
    /// `srcport`: the far end reports another port (`random`,
    /// `fully-random`).
    PortMoved,
}

/// A bound on the number of a probe that passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    /// `rtt` and `bulk`: at least this many milliseconds.
    AtLeastMs(u64),
    /// `udp-burst`: between these counts of echoed datagrams, both in.
    Between(u32, u32),
}

/// The expectation of one probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expect {
    pub probe: Probe,
    /// Each verdict that passes; more than one only where loss makes the
    /// verdict random.
    pub verdicts: Vec<Verdict>,
    pub seen: Option<Seen>,
    pub bound: Option<Bound>,
    /// The counters that this probe moves.
    pub moves: Vec<Counter>,
    /// The counters that this probe can move, by chance.
    pub may_move: Vec<Counter>,
    /// The rule that gives the verdict, for a report.
    pub why: String,
}

/// Whether a counter must move between the two reads of the check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Must {
    Move,
    Stay,
}

/// The expectations of a lab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expectations {
    pub probes: Vec<Expect>,
    /// Each counter of the table: it must move when a probe moves it, and
    /// it must stay when no probe can move it.
    pub counters: Vec<(Counter, Must)>,
}

impl Expectations {
    pub fn get(&self, probe: Probe) -> Option<&Expect> {
        self.probes.iter().find(|e| e.probe == probe)
    }
}

impl Lab {
    /// The expectation of each probe of `probe::probes` and of each counter.
    pub fn expect(&self) -> Expectations {
        let knobs = self.knobs();
        let layout = self.layout();
        let all: Vec<Expect> = probes(knobs).into_iter().map(|p| path::expect(knobs, layout, p)).collect();
        let moves: BTreeSet<Counter> = all.iter().flat_map(|e| e.moves.iter().copied()).collect();
        let may: BTreeSet<Counter> = all.iter().flat_map(|e| e.may_move.iter().copied()).collect();
        let counters = Counter::ALL
            .into_iter()
            .filter_map(|c| {
                if moves.contains(&c) {
                    Some((c, Must::Move))
                } else if may.contains(&c) {
                    None
                } else {
                    Some((c, Must::Stay))
                }
            })
            .collect();
        Expectations { probes: all, counters }
    }
}

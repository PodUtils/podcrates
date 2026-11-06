//! What a resolver remembers of where names were: the addresses that
//! carried a session, which the caller keeps between runs and gives with
//! [`Memory::set_remembered`] (podssh keeps them in a file of its cache), and
//! the source of the address of each connection that the dialer made lately,
//! which a doctor names and which tells the caller that TLS refused an
//! address of the cache. A field of the [`crate::Resolver`], keyed by the
//! name, and off with [`crate::Settings::memory`] `false`.

use std::collections::VecDeque;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Mutex, PoisonError, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::order::Source;
use crate::settings::key;

/// An address that carried a session to `host`, and when it last did, in
/// milliseconds since the epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remembered {
    pub host: String,
    pub ip: IpAddr,
    pub at_ms: i64,
}

/// A remembered address younger than this is asked before DNS over HTTPS;
/// an older one after it, last. The relay's names keep their addresses for
/// months, and a week lets a host that runs podssh weekly find its own.
pub const FRESH: Duration = Duration::from_secs(7 * 24 * 3600);

/// The connections of the dialer that [`Memory::dialed`] can name: a run
/// reaches a few hosts, and each new one pushes out the oldest.
const DIALED_KEPT: usize = 32;

/// The cache of a resolver: see the module.
#[derive(Debug, Default)]
pub struct Memory {
    off: bool,
    remembered: RwLock<Vec<Remembered>>,
    dialed: Mutex<VecDeque<(String, SocketAddr, Source)>>,
}

impl Memory {
    /// A cache that keeps what it is given, or, `on` false, nothing.
    pub fn new(on: bool) -> Memory {
        Memory { off: !on, ..Memory::default() }
    }

    /// Whether the cache keeps anything.
    pub fn is_on(&self) -> bool {
        !self.off
    }

    /// Replace the remembered addresses.
    pub fn set_remembered(&self, list: Vec<Remembered>) {
        let list = if self.off { Vec::new() } else { list };
        let list = list.into_iter().map(|r| Remembered { host: key(&r.host), ..r }).collect();
        *self.remembered.write().unwrap_or_else(PoisonError::into_inner) = list;
    }

    /// The remembered addresses of `host`, the newest first.
    pub fn remembered(&self, host: &str) -> Vec<Remembered> {
        let host = key(host);
        let all = self.remembered.read().unwrap_or_else(PoisonError::into_inner);
        let mut found: Vec<Remembered> = all.iter().filter(|r| r.host == host).cloned().collect();
        found.sort_by_key(|r| std::cmp::Reverse(r.at_ms));
        found
    }

    /// The dialer connected to `addr` for `host`, from `source`.
    pub fn note_dialed(&self, host: &str, addr: SocketAddr, source: Source) {
        if self.off {
            return;
        }
        let mut dialed = self.dialed.lock().unwrap_or_else(PoisonError::into_inner);
        let host = key(host);
        dialed.retain(|(h, a, _)| !(*h == host && *a == addr));
        if dialed.len() == DIALED_KEPT {
            dialed.pop_front();
        }
        dialed.push_back((host, addr, source));
    }

    /// The source of `addr`, when the dialer connected to it for `host`
    /// lately.
    pub fn dialed(&self, host: &str, addr: SocketAddr) -> Option<Source> {
        let host = key(host);
        let dialed = self.dialed.lock().unwrap_or_else(PoisonError::into_inner);
        dialed.iter().rev().find(|(h, a, _)| *h == host && *a == addr).map(|(_, _, source)| *source)
    }

    /// The address of the last connection that the dialer made for `host`,
    /// and its source.
    pub fn last_dialed(&self, host: &str) -> Option<(SocketAddr, Source)> {
        let host = key(host);
        let dialed = self.dialed.lock().unwrap_or_else(PoisonError::into_inner);
        dialed.iter().rev().find(|(h, _, _)| *h == host).map(|(_, addr, source)| (*addr, *source))
    }
}

/// The fresh addresses and the old ones of `entries` at `now_ms`, each the
/// newest first and each address once. One from the future, of a clock that
/// moved, is old: DNS over HTTPS is asked before it.
pub fn split(entries: &[Remembered], now_ms: i64) -> (Vec<IpAddr>, Vec<IpAddr>) {
    let window = FRESH.as_millis() as i64;
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|r| std::cmp::Reverse(r.at_ms));
    let (mut fresh, mut old) = (Vec::new(), Vec::new());
    for r in sorted {
        let list = if r.at_ms <= now_ms && now_ms - r.at_ms < window { &mut fresh } else { &mut old };
        if !list.contains(&r.ip) {
            list.push(r.ip);
        }
    }
    old.retain(|ip| !fresh.contains(ip));
    (fresh, old)
}

/// This host's clock, in milliseconds since the epoch.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_790_000_000_000;

    fn seen(ip: &str, ago_ms: i64) -> Remembered {
        Remembered { host: "relay.example".into(), ip: ip.parse().unwrap(), at_ms: NOW - ago_ms }
    }

    /// Fresh within the window, old past it and from the future; the newest
    /// first in each, and an address in one list only.
    #[test]
    fn addresses_are_fresh_for_a_week_and_old_after_it() {
        let week = FRESH.as_millis() as i64;
        let entries = [
            seen("192.0.2.1", week + 1),
            seen("192.0.2.2", 1000),
            seen("192.0.2.3", week - 1),
            seen("192.0.2.4", -60_000),
            seen("192.0.2.2", week * 2),
        ];
        let (fresh, old) = split(&entries, NOW);
        let ips = |list: &[&str]| list.iter().map(|s| s.parse::<IpAddr>().unwrap()).collect::<Vec<_>>();
        assert_eq!(fresh, ips(&["192.0.2.2", "192.0.2.3"]));
        assert_eq!(old, ips(&["192.0.2.4", "192.0.2.1"]), "the future's first, by its time; the old 192.0.2.2 gone");
        assert_eq!(split(&[], NOW), (Vec::new(), Vec::new()));
    }

    /// The dialer's record names the source of an address for its host only,
    /// the latest connection first, and keeps a bounded number.
    #[test]
    fn the_dialer_s_record_names_each_address_s_source() {
        let memory = Memory::new(true);
        let a: SocketAddr = "192.0.2.10:443".parse().unwrap();
        let b: SocketAddr = "192.0.2.11:443".parse().unwrap();
        memory.note_dialed("Memory-Test.example.", a, Source::System);
        memory.note_dialed("memory-test.example", b, Source::Cache);
        assert_eq!(memory.dialed("memory-test.example", a), Some(Source::System));
        assert_eq!(memory.dialed("other.example", a), None);
        assert_eq!(memory.last_dialed("MEMORY-TEST.example"), Some((b, Source::Cache)));
        for i in 0..DIALED_KEPT as u16 {
            memory.note_dialed("filler.example", SocketAddr::from(([192, 0, 2, 200], i)), Source::Doh);
        }
        assert_eq!(memory.dialed("memory-test.example", b), None, "pushed out by newer connections");
    }

    /// A cache that is off keeps no address and no connection.
    #[test]
    fn a_cache_that_is_off_keeps_nothing() {
        let memory = Memory::new(false);
        memory.set_remembered(vec![seen("192.0.2.1", 1000)]);
        assert!(memory.remembered("relay.example").is_empty());
        let a: SocketAddr = "192.0.2.10:443".parse().unwrap();
        memory.note_dialed("relay.example", a, Source::System);
        assert_eq!((memory.dialed("relay.example", a), memory.last_dialed("relay.example")), (None, None));
        let on = Memory::new(true);
        on.set_remembered(vec![seen("192.0.2.1", 1000)]);
        assert_eq!(on.remembered("RELAY.example.").len(), 1);
        assert!(on.is_on() && !memory.is_on());
    }
}

//! The order in which a [`crate::Resolver`] asks the sources of a name's
//! addresses, as the crate's comment gives it, apart from the asking of the
//! two sources that need the network, so that a test stands in for them.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};

/// Where an address came from, in the order that a resolver asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// The name is an IP address.
    Literal,
    /// An address pinned for the name ([`crate::Settings::pins`]).
    Pin,
    /// The system resolver.
    System,
    /// An address that carried a session, younger than [`crate::FRESH`].
    Cache,
    /// DNS over HTTPS.
    Doh,
    /// An address that carried a session, older than [`crate::FRESH`].
    OldCache,
}

impl Source {
    /// The sources, in the order asked.
    pub const ORDER: [Source; 6] =
        [Source::Literal, Source::Pin, Source::System, Source::Cache, Source::Doh, Source::OldCache];

    /// The source in a few words, as a doctor names it.
    pub fn words(self) -> &'static str {
        match self {
            Source::Literal => "an IP address in place of the name",
            Source::Pin => "a pinned address",
            Source::System => "the system resolver",
            Source::Cache => "the cache of addresses that carried a session",
            Source::Doh => "DNS over HTTPS",
            Source::OldCache => "an old address of the cache, tried last",
        }
    }

    /// The source in one word, for a log line and a counter (podnet's words
    /// of its steps: `pin`, `system`, `doh`).
    pub fn name(self) -> &'static str {
        match self {
            Source::Literal => "literal",
            Source::Pin => "pin",
            Source::System => "system",
            Source::Cache => "cache",
            Source::Doh => "doh",
            Source::OldCache => "old-cache",
        }
    }

    /// Whether a dial asks the sources after this one when none of its
    /// addresses connects: the cache's may be stale, and DNS over HTTPS
    /// leaves the old addresses their turn. The system resolver's answer is
    /// the system's, and a pin or an IP address is the user's.
    pub fn falls_back(self) -> bool {
        matches!(self, Source::Cache | Source::Doh)
    }

    /// The source as `pod-core` names it: its place in [`Source::ORDER`],
    /// and its [`Source::name`]. The first three are those of `pod-proxy`'s
    /// system resolver.
    pub fn core(self) -> pod_core::Source {
        let rank = Source::ORDER.iter().position(|s| *s == self).unwrap_or_default() as u8;
        pod_core::Source { rank, name: self.name() }
    }

    /// The source of a `pod-core` source, by its place in the order.
    pub fn of(source: pod_core::Source) -> Option<Source> {
        Source::ORDER.get(usize::from(source.rank)).copied()
    }
}

/// The addresses of one source, and whether a source after it may give
/// more when none of them connects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub addrs: Vec<SocketAddr>,
    pub source: Source,
    pub more: bool,
}

/// What is known of a name before the network is asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Known {
    pub literal: Option<IpAddr>,
    pub pins: Vec<IpAddr>,
    pub fresh: Vec<IpAddr>,
    pub old: Vec<IpAddr>,
    /// Whether DNS over HTTPS may be asked: the name is safe in a query.
    pub asks: bool,
}

impl Known {
    fn has(&self, source: Source) -> bool {
        match source {
            Source::Literal => self.literal.is_some(),
            Source::Pin => !self.pins.is_empty(),
            Source::System => true,
            Source::Cache => !self.fresh.is_empty(),
            Source::Doh => self.asks,
            Source::OldCache => !self.old.is_empty(),
        }
    }
}

/// The first source after `after` (from the start with none) that has
/// something to give. An IP address and a pin end the order: nothing is
/// asked after them.
pub(crate) fn next(after: Option<Source>, known: &Known) -> Option<Source> {
    if matches!(after, Some(Source::Literal | Source::Pin)) {
        return None;
    }
    let start = after.and_then(|s| Source::ORDER.iter().position(|o| *o == s)).map_or(0, |i| i + 1);
    Source::ORDER[start..].iter().copied().find(|s| known.has(*s))
}

/// The addresses of the first source after `after` over what is `known`,
/// with the asking of the system resolver and of DNS over HTTPS given: each
/// is awaited at most once, and only when the order reaches it. The caller
/// names the host ("could not resolve HOST: ..."), so the error says only
/// what each source answered.
pub(crate) async fn resolve_in<S, D>(
    known: &Known,
    port: u16,
    after: Option<Source>,
    system: S,
    doh: D,
) -> Result<Found, String>
where
    S: Future<Output = Result<Vec<SocketAddr>, String>>,
    D: Future<Output = Result<Vec<IpAddr>, String>>,
{
    let (mut system, mut doh) = (Some(system), Some(doh));
    let at = |ips: &[IpAddr]| ips.iter().map(|ip| SocketAddr::new(*ip, port)).collect::<Vec<_>>();
    let mut reasons: Vec<String> = Vec::new();
    let mut source = after;
    while let Some(asked) = next(source, known) {
        source = Some(asked);
        let got = match asked {
            Source::Literal => Ok(at(known.literal.as_slice())),
            Source::Pin => Ok(at(&known.pins)),
            Source::Cache => Ok(at(&known.fresh)),
            Source::OldCache => Ok(at(&known.old)),
            Source::System => match system.take() {
                Some(ask) => ask.await,
                None => continue,
            },
            Source::Doh => match doh.take() {
                Some(ask) => ask.await.map(|ips| at(&ips)).map_err(|e| format!("DNS over HTTPS: {e}")),
                None => continue,
            },
        };
        match got {
            Ok(addrs) if !addrs.is_empty() => {
                let more = asked.falls_back() && next(Some(asked), known).is_some();
                return Ok(Found { addrs, source: asked, more });
            }
            Ok(_) => reasons.push(format!("{}: no addresses", asked.words())),
            Err(e) => reasons.push(e),
        }
    }
    Err(if reasons.is_empty() { "no source gave an address".into() } else { reasons.join("; ") })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|s| s.parse().unwrap()).collect()
    }

    fn at(list: &[&str]) -> Vec<SocketAddr> {
        ips(list).into_iter().map(|ip| SocketAddr::new(ip, 443)).collect()
    }

    /// The order over `known`, with the answers of the system resolver and
    /// of DNS over HTTPS given; and the sources of the network that it asked,
    /// in turn.
    async fn run(
        known: &Known,
        after: Option<Source>,
        system: Result<Vec<SocketAddr>, String>,
        doh: Result<Vec<IpAddr>, String>,
    ) -> (Result<Found, String>, Vec<&'static str>) {
        let asked = RefCell::new(Vec::new());
        let found = resolve_in(
            known,
            443,
            after,
            async {
                asked.borrow_mut().push("system");
                system
            },
            async {
                asked.borrow_mut().push("doh");
                doh
            },
        )
        .await;
        (found, asked.into_inner())
    }

    fn cached() -> Known {
        Known { fresh: ips(&["192.0.2.7"]), old: ips(&["192.0.2.8"]), asks: true, ..Known::default() }
    }

    /// The system resolver first; after its failure the cache's fresh
    /// addresses, before DNS over HTTPS, which is not asked then.
    #[tokio::test]
    async fn the_cache_comes_after_the_system_resolver_and_before_dns_over_https() {
        let (found, asked) = run(&cached(), None, Ok(at(&["198.51.100.1"])), Ok(ips(&["203.0.113.1"]))).await;
        let found = found.unwrap();
        assert_eq!((found.source, found.addrs, found.more), (Source::System, at(&["198.51.100.1"]), false));
        assert_eq!(asked, ["system"], "the system's answer ends the order");
        let (found, asked) = run(&cached(), None, Err("no such host".into()), Ok(ips(&["203.0.113.1"]))).await;
        let found = found.unwrap();
        assert_eq!((found.source, found.addrs, found.more), (Source::Cache, at(&["192.0.2.7"]), true));
        assert_eq!(asked, ["system"], "DNS over HTTPS is not asked before the cache");
    }

    /// When the cache's addresses did not connect: DNS over HTTPS, then the
    /// old addresses when it fails; and with nothing fresh, DNS over HTTPS
    /// right after the system resolver.
    #[tokio::test]
    async fn dns_over_https_comes_next_and_the_old_addresses_last() {
        let (found, asked) = run(&cached(), Some(Source::Cache), Err("x".into()), Ok(ips(&["203.0.113.1"]))).await;
        let found = found.unwrap();
        assert_eq!((found.source, found.addrs, found.more), (Source::Doh, at(&["203.0.113.1"]), true));
        assert_eq!(asked, ["doh"]);
        let (found, asked) = run(&cached(), Some(Source::Cache), Err("x".into()), Err("timed out".into())).await;
        let found = found.unwrap();
        assert_eq!((found.source, found.addrs, found.more), (Source::OldCache, at(&["192.0.2.8"]), false));
        assert_eq!(asked, ["doh"]);
        let stale = Known { old: ips(&["192.0.2.8"]), asks: true, ..Known::default() };
        let (found, asked) = run(&stale, None, Err("x".into()), Ok(ips(&["203.0.113.1"]))).await;
        assert_eq!(found.unwrap().source, Source::Doh);
        assert_eq!(asked, ["system", "doh"]);
        let (found, _) = run(&stale, Some(Source::OldCache), Err("x".into()), Err("y".into())).await;
        assert_eq!(found.unwrap_err(), "no source gave an address", "nothing after the last");
    }

    /// An IP address and a pin ask nothing; a failure names each source in
    /// turn, as the dialer's error did before the cache; a name unsafe in a
    /// query asks no resolver of DNS over HTTPS.
    #[tokio::test]
    async fn literals_and_pins_ask_nothing_and_a_failure_names_each_source() {
        let literal = Known { literal: Some("192.0.2.1".parse().unwrap()), asks: true, ..Known::default() };
        let (found, asked) = run(&literal, None, Err("x".into()), Err("y".into())).await;
        assert_eq!((found.unwrap().source, asked.len()), (Source::Literal, 0));
        let pinned = Known { pins: ips(&["192.0.2.9"]), fresh: ips(&["192.0.2.7"]), asks: true, ..Known::default() };
        let (found, asked) = run(&pinned, None, Err("x".into()), Err("y".into())).await;
        let found = found.unwrap();
        assert_eq!((found.source, found.addrs, found.more, asked.len()), (Source::Pin, at(&["192.0.2.9"]), false, 0));
        let nothing = Known { asks: true, ..Known::default() };
        let (found, _) = run(&nothing, None, Err("Name or service not known".into()), Err("no answer".into())).await;
        assert_eq!(found.unwrap_err(), "Name or service not known; DNS over HTTPS: no answer");
        let unsafe_name = Known::default();
        let (found, asked) = run(&unsafe_name, None, Err("bad name".into()), Ok(ips(&["203.0.113.1"]))).await;
        assert_eq!((found.unwrap_err(), asked), ("bad name".to_string(), vec!["system"]));
    }
}

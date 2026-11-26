//! The direct connection to a name: the addresses of the first source of the
//! resolver that gives any, raced as RFC 8305 says; when none of them
//! connects and the source says that more may follow ([`Found::more`]: the
//! addresses of a cache, those of DNS over HTTPS), the addresses of the
//! sources after it. A source with another after it has [`SOURCE_WAIT`] to
//! connect, so that a remembered address that no longer answers does not
//! hold the dial.

use std::net::SocketAddr;
use std::time::Duration;

use pod_core::{Found, Resolve, SOURCE_WAIT, Stage};
use tokio::net::TcpStream;
use tokio::time::Instant;

use crate::cut::Cut;
use crate::error::DialError;
use crate::host::authority;
use crate::mptcp::Multipath;

/// What each step of one dial takes: the resolvers, the delay between two
/// attempts of the race, and the bounds.
#[derive(Clone, Copy)]
pub(crate) struct Leg<'a> {
    pub resolve: &'a dyn Resolve,
    /// The resolver of a proxy's own host: one that asks no source through
    /// that proxy.
    pub proxy_resolve: &'a dyn Resolve,
    pub attempt_delay: Duration,
    /// The plan by family, when the settings ask for it.
    pub families: Option<crate::FamilyPlan>,
    pub deadline: Instant,
    /// The whole bound, for the words of a timeout.
    pub budget: Duration,
    /// The TLS of an `https://` proxy, when the caller gave one.
    pub proxy_tls: Option<&'a crate::tls_hop::ProxyTls>,
}

/// Resolve and connect, with plain TCP: [`connect_race`].
pub(crate) async fn connect_direct(leg: Leg<'_>, host: &str, port: u16) -> Result<TcpStream, DialError> {
    connect_race(leg, host, port, None).await.map(|(stream, _)| stream)
}

/// One attempt of the race: plain TCP, or with `ask`, the probe's answer,
/// Multipath TCP where the host offers it.
async fn attempt(addr: SocketAddr, ask: Option<Result<(), String>>) -> std::io::Result<(TcpStream, Multipath)> {
    match ask {
        Some(offer) => crate::mptcp::connect(addr, offer).await,
        None => Ok((TcpStream::connect(addr).await?, Multipath::Plain("Multipath TCP was not asked for".into()))),
    }
}

/// [`connect_direct`], each attempt as `ask` says. The resolver hears the
/// source of the address that connected ([`Resolve::note_dialed`]), for a
/// doctor and for its cache.
pub(crate) async fn connect_race(
    leg: Leg<'_>,
    host: &str,
    port: u16,
    ask: Option<Result<(), String>>,
) -> Result<(TcpStream, Multipath), DialError> {
    let target = authority(host, port);
    let resolve_error = |e: pod_core::Failure| DialError::Resolve { host: host.to_string(), detail: e.chain() };
    // The resolver's order: pins, the system resolver, the addresses that
    // carried a session, DNS over HTTPS. A host with a broken resolver but
    // working TCP egress still gets there.
    // The deadline bounds the resolver too: a resolver that does not keep
    // to it does not hold the dial past its time limit.
    let timed_out = || DialError::Timeout { step: format!("resolving {host}"), after: leg.budget, stage: Stage::Dns };
    let first = tokio::time::timeout_at(leg.deadline, leg.resolve.resolve_from(host, port, leg.deadline, None));
    let mut found: Found = first.await.map_err(|_| timed_out())?.map_err(resolve_error)?;
    let mut tried: Vec<SocketAddr> = Vec::new();
    loop {
        if Instant::now() >= leg.deadline {
            return Err(timed_out());
        }
        let addrs: Vec<SocketAddr> = found.addrs.iter().copied().filter(|a| !tried.contains(a)).collect();
        tried.extend(addrs.iter().copied());
        let until = if found.more { leg.deadline.min(Instant::now() + SOURCE_WAIT) } else { leg.deadline };
        match race(leg, &target, addrs, until, &ask).await {
            Ok((stream, how, _)) => {
                if let Ok(peer) = stream.peer_addr() {
                    leg.resolve.note_dialed(host, peer, found.source);
                }
                return Ok((stream, how));
            }
            Err(failed) if found.more && Instant::now() < leg.deadline => {
                let next = leg.resolve.resolve_from(host, port, leg.deadline, Some(found.source));
                match tokio::time::timeout_at(leg.deadline, next).await {
                    Ok(Ok(next)) => found = next,
                    Ok(Err(_)) | Err(_) => return Err(failed),
                }
            }
            Err(failed) => return Err(failed),
        }
    }
}

/// The addresses in the order of [`interleave`], each started
/// `leg.attempt_delay` after the one before, or at once when the one before
/// failed; the first that connects wins, with its address, and the others
/// stop. Past `until`, short of the deadline, they failed, and another
/// source may have more. The errors of the addresses that failed are
/// grouped by their words ([`group_errors`]).
pub(crate) async fn race(
    leg: Leg<'_>,
    target: &str,
    addrs: Vec<SocketAddr>,
    until: Instant,
    ask: &Option<Result<(), String>>,
) -> Result<(TcpStream, Multipath, SocketAddr), DialError> {
    // A resolver's answer can hold an address twice (a pin and the system's
    // answer): each is dialed once, in its first place.
    let addrs = distinct(addrs);
    if let Some(plan) = leg.families {
        let order = crate::family::plan(&addrs, crate::family::Local::cached());
        let each = |addr| attempt(addr, ask.clone());
        let held =
            crate::family::race_held(order, plan, leg.attempt_delay, target, until, leg.deadline, leg.budget, each);
        return held.await.map(|((stream, how), addr)| (stream, how, addr));
    }
    let mut waiting = interleave(addrs).into_iter();
    // Dropped at the return: each attempt that still runs stops.
    let mut running = tokio::task::JoinSet::new();
    let mut next_at = Instant::now();
    let mut errors: Vec<(SocketAddr, String)> = Vec::new();
    let mut last = String::from("each address of this source was tried already");
    // The kind of the last attempt's error, beside its words.
    let (mut cut, mut kind) = (None, None);
    loop {
        if running.is_empty() || Instant::now() >= next_at {
            if let Some(addr) = waiting.next() {
                let ask = ask.clone();
                running.spawn(async move { (addr, attempt(addr, ask).await) });
                next_at = Instant::now() + leg.attempt_delay;
                continue;
            }
            if running.is_empty() {
                let detail = match errors.as_slice() {
                    [] => last,
                    // The one address is the target: the words name it once.
                    [(addr, text)] if addr.to_string() == target => text.clone(),
                    _ => group_errors(&errors),
                };
                return Err(DialError::Connect { target: target.to_string(), detail, cut, kind });
            }
        }
        let wake = if waiting.len() > 0 { next_at.min(until) } else { until };
        tokio::select! {
            joined = running.join_next() => match joined {
                Some(Ok((addr, Ok((stream, how))))) => return Ok((stream, how, addr)),
                Some(Ok((addr, Err(e)))) => {
                    errors.push((addr, crate::hint::open_files(&e).unwrap_or_else(|| e.to_string())));
                    (cut, kind) = (Some(Cut::of(e.kind())), Some(e.kind()));
                    next_at = Instant::now();
                }
                // An attempt that panicked counts as one that failed.
                Some(Err(e)) => {
                    errors.clear();
                    last = e.to_string();
                    (cut, kind) = (None, None);
                }
                None => {}
            },
            _ = tokio::time::sleep_until(wake) => {
                if Instant::now() >= leg.deadline {
                    let step = format!("connecting to {target}");
                    return Err(DialError::Timeout { step, after: leg.budget, stage: Stage::Connect });
                }
                if Instant::now() >= until {
                    let detail = format!("no address of this source answered within {} s", SOURCE_WAIT.as_secs());
                    let cut = Some(Cut::TimedOut);
                    return Err(DialError::Connect { target: target.to_string(), detail, cut, kind: None });
                }
            }
        }
    }
}

/// The errors of a dial to more than one address, one entry for each
/// text: `198.41.192.7:7844: refused`, or `20 addresses (198.41.192.7:7844,
/// 198.41.192.27:7844, ...): Permission denied (os error 13)`.
pub fn group_errors(errors: &[(SocketAddr, String)]) -> String {
    group_texts(errors, "addresses")
}

/// The errors of more than one key (an address, a host), one entry for
/// each text: `KEY: TEXT`, or `N NOUN (KEY, KEY, ...): TEXT`. A key is
/// counted once.
pub fn group_texts<K: std::fmt::Display + PartialEq>(errors: &[(K, String)], noun: &str) -> String {
    let mut groups: Vec<(&str, Vec<&K>)> = Vec::new();
    for (key, text) in errors {
        match groups.iter_mut().find(|(t, _)| *t == text.as_str()) {
            Some((_, keys)) if keys.contains(&key) => {}
            Some((_, keys)) => keys.push(key),
            None => groups.push((text, vec![key])),
        }
    }
    groups
        .iter()
        .map(|(text, keys)| match keys.as_slice() {
            [one] => format!("{one}: {text}"),
            many => {
                let shown: Vec<String> = many.iter().take(2).map(|k| k.to_string()).collect();
                let more = if many.len() > 2 { ", ..." } else { "" };
                format!("{} {noun} ({}{more}): {text}", many.len(), shown.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Each address of `addrs` once, in the place where it first comes.
pub fn distinct(addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let mut out: Vec<SocketAddr> = Vec::with_capacity(addrs.len());
    for a in addrs {
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

/// Connect to the first of `addrs` that answers, with no proxy and no
/// resolver: the attempts start [`crate::ATTEMPT_DELAY`] apart, or at once
/// when the attempt before fails, the families taking turns. Gives the
/// index of the address in `addrs`, and the stream. The whole is bounded by
/// `timeout`.
pub async fn dial_addrs(addrs: &[SocketAddr], timeout: Duration) -> Result<(usize, TcpStream), DialError> {
    if addrs.is_empty() {
        return Err(DialError::Connect {
            target: "no address".into(),
            detail: "no address to connect to".into(),
            cut: None,
            kind: None,
        });
    }
    let system = crate::resolve::System::new();
    let deadline = Instant::now() + timeout;
    let leg = Leg {
        resolve: &system,
        proxy_resolve: &system,
        attempt_delay: crate::ATTEMPT_DELAY,
        families: None,
        deadline,
        budget: timeout,
        proxy_tls: None,
    };
    let target = match addrs {
        [one] => one.to_string(),
        [first, ..] => format!("{first} and {} more", addrs.len() - 1),
        [] => unreachable!(),
    };
    let (stream, _, won) = race(leg, &target, addrs.to_vec(), deadline, &None).await?;
    let _ = stream.set_nodelay(true);
    Ok((addrs.iter().position(|a| *a == won).unwrap_or_default(), stream))
}

/// The addresses with the families taking turns, the first family first
/// (RFC 8305, section 4): a family that does not work costs one attempt
/// delay, not each of its addresses in a row.
pub fn interleave(addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let Some(first) = addrs.first().map(|a| a.is_ipv6()) else { return addrs };
    let (mut lead, mut other): (Vec<_>, Vec<_>) = addrs.into_iter().partition(|a| a.is_ipv6() == first);
    let mut out = Vec::with_capacity(lead.len() + other.len());
    lead.reverse();
    other.reverse();
    loop {
        match (lead.pop(), other.pop()) {
            (None, None) => return out,
            (a, b) => out.extend(a.into_iter().chain(b)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_error_of_many_addresses_is_one_entry() {
        let a = |s: &str| s.parse::<SocketAddr>().unwrap();
        let denied = "Permission denied (os error 13)".to_string();
        let mut errors: Vec<(SocketAddr, String)> =
            (1..=20).map(|i| (a(&format!("198.41.192.{i}:7844")), denied.clone())).collect();
        errors.push((a("1.2.3.4:7844"), "refused".into()));
        assert_eq!(
            group_errors(&errors),
            "20 addresses (198.41.192.1:7844, 198.41.192.2:7844, ...): \
             Permission denied (os error 13); 1.2.3.4:7844: refused"
        );
        assert_eq!(group_errors(&errors[..1]), format!("198.41.192.1:7844: {denied}"));
    }

    /// An address that an answer holds twice is dialed once and said once
    /// (PodUtils/podssh#315).
    #[test]
    fn an_address_twice_is_dialed_once_and_said_once() {
        let a = |s: &str| s.parse::<SocketAddr>().unwrap();
        let addrs = vec![a("127.0.0.1:9443"), a("[::1]:9443"), a("127.0.0.1:9443"), a("10.0.0.1:1")];
        assert_eq!(distinct(addrs), [a("127.0.0.1:9443"), a("[::1]:9443"), a("10.0.0.1:1")]);
        let refused = "Connection refused (os error 111)".to_string();
        let twice = [(a("127.0.0.1:9443"), refused.clone()), (a("127.0.0.1:9443"), refused.clone())];
        assert_eq!(group_errors(&twice), format!("127.0.0.1:9443: {refused}"));
    }

    #[tokio::test]
    async fn a_dial_of_one_refused_address_twice_names_it_once() {
        let (_held, at) = pod_testkit::refused::refused();
        let e = dial_addrs(&[at, at], Duration::from_secs(10)).await.expect_err("a refused address");
        let text = e.to_string();
        assert!(!text.contains("2 addresses"), "{text}");
        assert!(text.contains(&at.to_string()), "{text}");
    }

    #[test]
    fn interleave_alternates_the_families() {
        let a: Vec<SocketAddr> =
            ["[::1]:1", "[::2]:1", "1.1.1.1:1", "2.2.2.2:1"].iter().map(|s| s.parse().unwrap()).collect();
        let got: Vec<String> = interleave(a).iter().map(|a| a.to_string()).collect();
        assert_eq!(got, ["[::1]:1", "1.1.1.1:1", "[::2]:1", "2.2.2.2:1"]);
    }
}

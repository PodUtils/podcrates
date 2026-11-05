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
    pub deadline: Instant,
    /// The whole bound, for the words of a timeout.
    pub budget: Duration,
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
    let mut found: Found = leg.resolve.resolve_from(host, port, leg.deadline, None).await.map_err(resolve_error)?;
    let mut tried: Vec<SocketAddr> = Vec::new();
    loop {
        if Instant::now() >= leg.deadline {
            return Err(DialError::Timeout { step: format!("resolving {host}"), after: leg.budget, stage: Stage::Dns });
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
                match leg.resolve.resolve_from(host, port, leg.deadline, Some(found.source)).await {
                    Ok(next) => found = next,
                    Err(_) => return Err(failed),
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
    let mut waiting = interleave(addrs).into_iter();
    // Dropped at the return: each attempt that still runs stops.
    let mut running = tokio::task::JoinSet::new();
    let mut next_at = Instant::now();
    let mut errors: Vec<(SocketAddr, String)> = Vec::new();
    let mut last = String::from("each address of this source was tried already");
    // The kind of the last attempt's error, beside its words.
    let mut cut = None;
    loop {
        if running.is_empty() || Instant::now() >= next_at {
            if let Some(addr) = waiting.next() {
                let ask = ask.clone();
                running.spawn(async move { (addr, attempt(addr, ask).await) });
                next_at = Instant::now() + leg.attempt_delay;
                continue;
            }
            if running.is_empty() {
                let detail = if errors.is_empty() { last } else { group_errors(&errors) };
                return Err(DialError::Connect { target: target.to_string(), detail, cut });
            }
        }
        let wake = if waiting.len() > 0 { next_at.min(until) } else { until };
        tokio::select! {
            joined = running.join_next() => match joined {
                Some(Ok((addr, Ok((stream, how))))) => return Ok((stream, how, addr)),
                Some(Ok((addr, Err(e)))) => {
                    errors.push((addr, crate::hint::open_files(&e).unwrap_or_else(|| e.to_string())));
                    cut = Some(Cut::of(e.kind()));
                    next_at = Instant::now();
                }
                // An attempt that panicked counts as one that failed.
                Some(Err(e)) => {
                    errors.clear();
                    last = e.to_string();
                    cut = None;
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
                    return Err(DialError::Connect { target: target.to_string(), detail, cut: Some(Cut::TimedOut) });
                }
            }
        }
    }
}

/// The errors of a dial to more than one address, one entry for each
/// text: `198.41.192.7:7844: refused`, or `20 addresses (198.41.192.7:7844,
/// 198.41.192.27:7844, ...): Permission denied (os error 13)`.
pub fn group_errors(errors: &[(SocketAddr, String)]) -> String {
    let mut groups: Vec<(&str, Vec<SocketAddr>)> = Vec::new();
    for (addr, text) in errors {
        match groups.iter_mut().find(|(t, _)| *t == text.as_str()) {
            Some((_, addrs)) => addrs.push(*addr),
            None => groups.push((text, vec![*addr])),
        }
    }
    groups
        .iter()
        .map(|(text, addrs)| match addrs.as_slice() {
            [one] => format!("{one}: {text}"),
            many => {
                let shown: Vec<String> = many.iter().take(2).map(|a| a.to_string()).collect();
                let more = if many.len() > 2 { ", ..." } else { "" };
                format!("{} addresses ({}{more}): {text}", many.len(), shown.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
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
        });
    }
    let system = crate::resolve::System::new();
    let deadline = Instant::now() + timeout;
    let leg = Leg {
        resolve: &system,
        proxy_resolve: &system,
        attempt_delay: crate::ATTEMPT_DELAY,
        deadline,
        budget: timeout,
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

    #[test]
    fn interleave_alternates_the_families() {
        let a: Vec<SocketAddr> =
            ["[::1]:1", "[::2]:1", "1.1.1.1:1", "2.2.2.2:1"].iter().map(|s| s.parse().unwrap()).collect();
        let got: Vec<String> = interleave(a).iter().map(|a| a.to_string()).collect();
        assert_eq!(got, ["[::1]:1", "1.1.1.1:1", "[::2]:1", "2.2.2.2:1"]);
    }
}

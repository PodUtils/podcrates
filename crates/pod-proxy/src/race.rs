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

/// What each step of one dial takes: the resolver, the delay between two
/// attempts of the race, and the bounds.
#[derive(Clone, Copy)]
pub(crate) struct Leg<'a> {
    pub resolve: &'a dyn Resolve,
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
            Ok((stream, how)) => {
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
/// failed; the first that connects wins, and the others stop. Past `until`,
/// short of the deadline, they failed, and another source may have more.
pub(crate) async fn race(
    leg: Leg<'_>,
    target: &str,
    addrs: Vec<SocketAddr>,
    until: Instant,
    ask: &Option<Result<(), String>>,
) -> Result<(TcpStream, Multipath), DialError> {
    let mut waiting = interleave(addrs).into_iter();
    // Dropped at the return: each attempt that still runs stops.
    let mut running = tokio::task::JoinSet::new();
    let mut next_at = Instant::now();
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
                return Err(DialError::Connect { target: target.to_string(), detail: last, cut });
            }
        }
        let wake = if waiting.len() > 0 { next_at.min(until) } else { until };
        tokio::select! {
            joined = running.join_next() => match joined {
                Some(Ok((_, Ok(stream)))) => return Ok(stream),
                Some(Ok((addr, Err(e)))) => {
                    last = format!("{addr}: {e}");
                    cut = Some(Cut::of(e.kind()));
                    next_at = Instant::now();
                }
                // An attempt that panicked counts as one that failed.
                Some(Err(e)) => {
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

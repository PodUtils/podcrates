//! Edge discovery that works with no system resolver.
//!
//! `cloudflared` finds the tunnel edge with a lookup of
//! `_v2-origintunneld._tcp.argotunnel.com`. The answer gives the regional
//! names and port 7844. A closed sandbox usually has no resolver that works:
//! no nameserver in `/etc/resolv.conf` answers, and DNS over TLS on 853 is
//! also blocked. Thus the lookup uses HTTPS: the DNS-over-HTTPS endpoint of
//! Cloudflare answers the same SRV and address records on TCP 443. That is
//! the one egress of such a host.
//!
//! The order of the attempts:
//!
//! 1. DoH of the resolver of the host ([`crate::Host::resolver`]), through
//!    the proxy when the host has one.
//! 2. The system resolver of the consumer ([`crate::host::SystemSrv`]).
//! 3. A short list of anycast addresses, for a host where no DNS works. This
//!    list is the last option, and its label tells so.
//!
//! The whole discovery has [`DISCOVERY_LIMIT`], and DoH has [`DOH_LIMIT`] in
//! it: a proxy that connects each resolver and gives no answer does not
//! hold the legs. A source that takes longer than [`SLOW_SOURCE`] gives one
//! event `edge.discovery` with its time and its cause.
//!
//! The port comes from the SRV record, not from a constant. Thus the crate
//! follows a change at Cloudflare and does not guess. An SRV answer of another
//! shape (more than two targets, a target outside `argotunnel.com`, two
//! ports) is not used: the crate takes [`fixed_targets`] and says why.
//!
//! The record, the zone of its targets, the fixed targets and the static
//! addresses are data of the edge of `pod-services`.

use std::future::Future;
use std::net::IpAddr;
use std::time::Duration;

use tokio::time::Instant;

use crate::Host;
use crate::cloudflare::{EdgeAddr, EdgeIpVersion, IpVersionFilter};
use crate::fail::{Context, Result, bail};
use crate::services::{self, EDGE};
use pod_core::{Class, Event, Stage};

mod doh;

pub use doh::discover_doh;

/// The limit of the whole discovery: DNS over HTTPS, then the system
/// resolver in the rest of the time. The static list follows it, so the
/// first leg starts in this time.
pub const DISCOVERY_LIMIT: Duration = Duration::from_secs(12);

/// The limit of the source DNS over HTTPS in the discovery.
pub const DOH_LIMIT: Duration = Duration::from_secs(8);

/// The limit of one DoH question to one host of a resolver: a connection
/// that succeeds and gives no answer costs this, then the next host.
pub const DOH_EACH: Duration = Duration::from_secs(3);

/// A source of the discovery that takes longer gives one event when it
/// ends.
pub const SLOW_SOURCE: Duration = Duration::from_secs(3);

/// The SRV record that cloudflared resolves: the fact `srv-name`.
pub fn edge_srv() -> &'static str {
    services::text(EDGE, "srv-name")
}

/// The anycast addresses of the last option, only for a failure of the two
/// resolvers: the fact `static-addresses`.
///
/// These are the published anycast addresses of the edge of Cloudflare. They
/// do not replace the discovery: the SRV record is the authority. This list
/// lets a host with no DNS get to the service.
fn static_edge_ips() -> Vec<&'static str> {
    let ips = services::get(EDGE).texts("static-addresses").unwrap_or_default();
    ips.iter().map(|ip| ip.as_ref()).collect()
}

/// The SRV targets when the SRV answer has an unexpected shape: the
/// endpoints of the role `h2`.
pub fn fixed_targets() -> Vec<(&'static str, u16)> {
    let endpoints = services::endpoints_of(EDGE, "h2");
    endpoints.iter().map(|e| (e.host(), e.port())).collect()
}

/// [`checked_targets_with`] with no report.
pub fn checked_targets(targets: Vec<(String, u16)>) -> Vec<(String, u16)> {
    checked_targets_with(targets, &pod_core::Report::default())
}

/// The SRV targets to use: `targets` when they have the known shape, else
/// [`fixed_targets`], with an event for `report` that gives the reason.
pub fn checked_targets_with(targets: Vec<(String, u16)>, report: &pod_core::Report) -> Vec<(String, u16)> {
    let mut ports: Vec<u16> = targets.iter().map(|(_, p)| *p).collect();
    ports.sort();
    ports.dedup();
    let reason = if targets.is_empty() {
        Some("it has no target".to_string())
    } else if targets.len() > 2 {
        Some(format!("it has {} targets, not 1 or 2", targets.len()))
    } else if let Some((name, _)) =
        targets.iter().find(|(name, _)| !name.to_ascii_lowercase().ends_with(&format!(".{}", zone())))
    {
        Some(format!("the target {name} is not under {}", zone()))
    } else if ports.len() > 1 {
        Some(format!("it has the ports {ports:?}, not one port"))
    } else {
        None
    };
    match reason {
        None => {
            // A stable order: region1 is region 0 at each lookup.
            let mut targets = targets;
            targets.sort();
            targets
        }
        Some(reason) => {
            let fixed = fixed_targets();
            let names: Vec<&str> = fixed.iter().map(|(n, _)| *n).collect();
            let text = format!(
                "the SRV answer of {} has an unexpected shape: {reason}; the client uses {} on port {}",
                edge_srv(),
                names.join(" and "),
                services::edge_port()
            );
            report.event(Event::warn("edge.discovery", text));
            fixed.into_iter().map(|(n, p)| (n.to_string(), p)).collect()
        }
    }
}

/// The zone of the targets of the SRV record: the fact `srv-target-zone`.
fn zone() -> &'static str {
    services::text(EDGE, "srv-target-zone")
}

/// Resolve the edge: DoH first, then the system resolver, then the static
/// list, in [`DISCOVERY_LIMIT`]. Gives at least one address, or an error
/// that names each attempt. A source that takes longer than
/// [`SLOW_SOURCE`] gives one event `edge.discovery`.
pub async fn discover(host: &Host, filter: IpVersionFilter) -> Result<Vec<EdgeAddr>> {
    let start = Instant::now();
    // Each failure, and its short form when an event said its cause.
    let mut failures: Vec<(String, Option<String>)> = Vec::new();
    let after_doh = if host.system_srv.is_some() { "the system resolver" } else { "the static list" };

    let doh_end = start + DOH_LIMIT;
    let doh = doh::discover_doh_until(host, filter, doh_end);
    let (found, said) = timed(host, ("DoH", &doh::way_words(host)), after_doh, doh_end, doh).await;
    match found {
        Ok(edges) if !edges.is_empty() => return Ok(edges),
        Ok(_) => failures.push(("DoH returned no addresses".to_string(), said)),
        Err(err) => failures.push((format!("DoH: {}", err.chain()), said)),
    }

    let system = discover_system(host, filter);
    let words = ("system resolver", "the system resolver");
    let (found, said) = timed(host, words, "the static list", start + DISCOVERY_LIMIT, system).await;
    match found {
        Ok(edges) if !edges.is_empty() => return Ok(edges),
        Ok(_) => failures.push(("the system resolver returned no addresses".to_string(), said)),
        Err(err) => failures.push((format!("system resolver: {}", err.chain()), said)),
    }

    let edges = static_edges(filter);
    if !edges.is_empty() {
        let short: Vec<&str> = failures.iter().map(|(full, said)| said.as_deref().unwrap_or(full)).collect();
        let text = format!("DNS discovery of the edge failed ({}); the static edge list is in use", short.join("; "));
        host.event(Event::warn("edge.discovery", text));
        return Ok(edges);
    }
    let mut full: Vec<String> = failures.into_iter().map(|(full, _)| full).collect();
    full.push("the static list has no address for this IP version".to_string());
    bail!(Stage::Dns, Class::Unavailable, "edge discovery failed: {}", full.join("; "))
}

/// Run a source of the discovery until `deadline`; `words` are its short
/// name and its name in a line. A source that takes longer than
/// [`SLOW_SOURCE`] gives one event when it ends: its time, and its result,
/// or its cause and `next`. With the event, the short form of a failure
/// for a later line comes back.
async fn timed(
    host: &Host,
    words: (&str, &str),
    next: &str,
    deadline: Instant,
    source: impl Future<Output = Result<Vec<EdgeAddr>>>,
) -> (Result<Vec<EdgeAddr>>, Option<String>) {
    let start = Instant::now();
    let limit = deadline.saturating_duration_since(start);
    let found = tokio::time::timeout_at(deadline, source)
        .await
        .unwrap_or_else(|_| Err(crate::fail::new(Stage::Dns, Class::Timeout, "no answer")));
    let took = start.elapsed();
    if took < SLOW_SOURCE {
        return (found, None);
    }
    host.event(slow_event(words.1, next, took, limit, &found));
    (found, Some(format!("{}: the line before gives the cause", words.0)))
}

/// The event of a source that took `took` of its `limit`.
fn slow_event(what: &str, next: &str, took: Duration, limit: Duration, found: &Result<Vec<EdgeAddr>>) -> Event {
    let waited = format!("{:.1} s", took.as_secs_f64());
    match found {
        Ok(edges) if !edges.is_empty() => {
            Event::info("edge.discovery", format!("the DNS of the edge took {waited} on {what}"))
        }
        Ok(_) => Event::warn(
            "edge.discovery",
            format!("the DNS of the edge waited {waited} on {what}, which gave no address; the client takes {next}"),
        ),
        Err(err) => {
            let at = if took >= limit { format!(" (the limit of {} s)", limit.as_secs()) } else { String::new() };
            let cause = err.chain();
            let text = format!("the DNS of the edge waited {waited}{at} on {what}; {cause}; the client takes {next}");
            Event::warn("edge.discovery", text)
        }
    }
}

/// Resolve the SRV record with the system resolver of the consumer, and its
/// targets with the lookup of addresses of the system.
pub async fn discover_system(host: &Host, filter: IpVersionFilter) -> Result<Vec<EdgeAddr>> {
    let Some(system) = &host.system_srv else {
        bail!(Stage::Dns, Class::Unsupported, "the consumer gives no system resolver of SRV records");
    };
    let targets = system.srv(edge_srv()).await.with_context(|| format!("SRV lookup of {}", edge_srv()))?;
    let targets: Vec<(String, u16)> =
        targets.into_iter().map(|(target, port)| (target.trim_end_matches('.').to_string(), port)).collect();
    let mut edges = Vec::new();
    for (region, (target, port)) in (0u8..).zip(checked_targets_with(targets, &host.report)) {
        let Ok(addrs) = tokio::net::lookup_host((target.as_str(), port)).await else {
            continue;
        };
        for addr in addrs {
            let edge = EdgeAddr::new(addr).in_region(region);
            if filter.allows(edge.version) && !edges.contains(&edge) {
                edges.push(edge);
            }
        }
    }
    edges.sort_by_key(|e| e.version != EdgeIpVersion::V4);
    Ok(edges)
}

/// Parse an SRV `data` field: `priority weight port target`.
pub(crate) fn parse_srv(data: &str) -> Option<(String, u16)> {
    let mut parts = data.split_whitespace();
    let _priority = parts.next()?;
    let _weight = parts.next()?;
    let port: u16 = parts.next()?.parse().ok()?;
    let target = parts.next()?.trim_end_matches('.').to_string();
    if target.is_empty() {
        return None;
    }
    Some((target, port))
}

fn static_edges(filter: IpVersionFilter) -> Vec<EdgeAddr> {
    let mut out = Vec::new();
    for text in static_edge_ips() {
        let Ok(ip) = text.parse::<IpAddr>() else {
            continue;
        };
        let version = if ip.is_ipv4() { EdgeIpVersion::V4 } else { EdgeIpVersion::V6 };
        let keep = match filter {
            IpVersionFilter::Auto => true,
            IpVersionFilter::V4Only => version == EdgeIpVersion::V4,
            IpVersionFilter::V6Only => version == EdgeIpVersion::V6,
        };
        if keep {
            out.push(EdgeAddr {
                ip,
                port: services::edge_port(),
                version,
                // The first two are in region1, the others in region2.
                region: u8::from(!text.starts_with("198.41.192.")),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests;

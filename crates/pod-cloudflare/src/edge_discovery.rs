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
//! The port comes from the SRV record, not from a constant. Thus the crate
//! follows a change at Cloudflare and does not guess. An SRV answer of another
//! shape (more than two targets, a target outside `argotunnel.com`, two
//! ports) is not used: the crate takes [`fixed_targets`] and says why.
//!
//! The record, the zone of its targets, the fixed targets and the static
//! addresses are data of the edge of `pod-services`.

use std::net::IpAddr;

use std::time::Duration;

use crate::Host;
use crate::cloudflare::{EdgeAddr, EdgeIpVersion, IpVersionFilter};
use crate::fail::{Context, OrFail, Result, bail};
use crate::services::{self, EDGE};
use pod_core::{Class, Stage};

/// The time of one step of the chain of resolvers.
const STEP_TIMEOUT: Duration = Duration::from_secs(5);

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
            report.event(pod_core::Event::warn("edge.discovery", text));
            fixed.into_iter().map(|(n, p)| (n.to_string(), p)).collect()
        }
    }
}

/// The zone of the targets of the SRV record: the fact `srv-target-zone`.
fn zone() -> &'static str {
    services::text(EDGE, "srv-target-zone")
}

/// Resolve the edge: DoH first, then the system resolver, then the static
/// list. Gives at least one address, or an error that names each attempt.
pub async fn discover(host: &Host, filter: IpVersionFilter) -> Result<Vec<EdgeAddr>> {
    let mut failures = Vec::new();

    match discover_doh(host, filter).await {
        Ok(edges) if !edges.is_empty() => return Ok(edges),
        Ok(_) => failures.push("DoH returned no addresses".to_string()),
        Err(err) => failures.push(format!("DoH: {}", err.chain())),
    }

    match discover_system(host, filter).await {
        Ok(edges) if !edges.is_empty() => return Ok(edges),
        Ok(_) => failures.push("the system resolver returned no addresses".to_string()),
        Err(err) => failures.push(format!("system resolver: {}", err.chain())),
    }

    let edges = static_edges(filter);
    if edges.is_empty() {
        failures.push("the static list has no address for this IP version".to_string());
    } else {
        let text =
            format!("DNS discovery of the edge failed ({}); the static edge list is in use", failures.join("; "));
        host.event(pod_core::Event::warn("edge.discovery", text));
        return Ok(edges);
    }

    bail!(Stage::Dns, Class::Unavailable, "edge discovery failed: {}", failures.join("; "))
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

/// Resolve through the DNS-over-HTTPS JSON API of Cloudflare.
pub async fn discover_doh(host: &Host, filter: IpVersionFilter) -> Result<Vec<EdgeAddr>> {
    let srv = query(host, edge_srv(), "SRV").await?;
    let mut targets: Vec<(String, u16)> = Vec::new();
    for answer in &srv.records {
        if answer.kind != 33 {
            continue;
        }
        if let Some((target, port)) = parse_srv(&answer.data) {
            targets.push((target, port));
        }
    }
    let targets = checked_targets_with(targets, &host.report);

    let mut edges = Vec::new();
    for (region, (target, port)) in (0u8..).zip(targets) {
        let records: &[(&str, u64)] = match filter {
            IpVersionFilter::V4Only => &[("A", 1)],
            IpVersionFilter::V6Only => &[("AAAA", 28)],
            IpVersionFilter::Auto => &[("A", 1), ("AAAA", 28)],
        };
        for (rtype, wanted) in records {
            let answer = match query(host, &target, rtype).await {
                Ok(answer) => answer,
                Err(err) => {
                    tracing::debug!(%target, rtype, error = %err.chain(), "address lookup failed");
                    continue;
                }
            };
            for entry in &answer.records {
                if entry.kind != *wanted {
                    continue;
                }
                let Ok(ip) = entry.data.trim().parse::<IpAddr>() else {
                    continue;
                };
                edges.push(EdgeAddr {
                    ip,
                    port,
                    version: if ip.is_ipv4() { EdgeIpVersion::V4 } else { EdgeIpVersion::V6 },
                    region,
                });
            }
        }
    }
    Ok(edges)
}

/// Ask the DoH resolvers of the host for `rtype` records of `name`, one
/// resolver after the other, until one answers.
async fn query(host: &Host, name: &str, rtype: &str) -> Result<pod_dns::json::Answer> {
    let resolver = &host.resolver;
    let limit = STEP_TIMEOUT * (1 + 2 * resolver.settings().resolvers.len() as u32);
    let deadline = tokio::time::Instant::now() + limit;
    let way = resolver.way().or_fail(Stage::Dns, Class::Unsupported, "the resolver asks no DNS over HTTPS")?;
    let answer =
        way.query(name, rtype, deadline).await.map_err(|e| crate::fail::new(Stage::Dns, Class::Unavailable, e))?;
    if answer.status != Some(0) {
        let status = answer.status.map_or("missing".into(), |s| s.to_string());
        bail!(Stage::Dns, Class::Unavailable, "DoH status {status} for {name} {rtype}");
    }
    Ok(answer)
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
mod tests {
    use super::*;

    #[test]
    fn parses_an_srv_data_field() {
        assert_eq!(
            parse_srv("2 1 7844 region2.v2.argotunnel.com."),
            Some(("region2.v2.argotunnel.com".to_string(), 7844))
        );
        assert_eq!(parse_srv("garbage"), None);
    }

    #[test]
    fn decodes_a_cloudflare_doh_answer() {
        let body = r#"{"Status":0,"Answer":[{"name":"_v2-origintunneld._tcp.argotunnel.com","type":33,"TTL":300,"data":"2 1 7844 region2.v2.argotunnel.com"}]}"#;
        let parsed = pod_dns::json::parse(body.as_bytes()).unwrap();
        assert_eq!(parsed.status, Some(0));
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(parsed.records[0].kind, 33);
    }

    #[test]
    fn an_srv_answer_of_another_shape_gives_the_fixed_names() {
        let t =
            |list: &[(&str, u16)]| -> Vec<(String, u16)> { list.iter().map(|(n, p)| (n.to_string(), *p)).collect() };
        let fixed = t(&fixed_targets());
        let good = t(&[("region1.v2.argotunnel.com", 7844), ("region2.v2.argotunnel.com", 7844)]);
        assert_eq!(checked_targets(good.clone()), good);
        let three = t(&[("a.argotunnel.com", 7844), ("b.argotunnel.com", 7844), ("c.argotunnel.com", 7844)]);
        assert_eq!(checked_targets(three), fixed);
        assert_eq!(checked_targets(t(&[("evil.example.com", 7844)])), fixed);
        assert_eq!(checked_targets(t(&[("a.argotunnel.com", 7844), ("b.argotunnel.com", 443)])), fixed);
        assert_eq!(checked_targets(Vec::new()), fixed);
    }

    /// The reason of the fixed names is an event for the user.
    #[test]
    fn an_srv_answer_of_another_shape_is_an_event() {
        let (report, seen) = crate::testing::recorded();
        let three = ["a", "b", "c"].map(|n| (format!("{n}.argotunnel.com"), 7844)).to_vec();
        checked_targets_with(three, &report);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!((seen[0].level, seen[0].kind), (pod_core::Level::Warn, "edge.discovery"));
        assert!(seen[0].text.contains("3 targets"), "{}", seen[0].text);
    }

    #[test]
    fn the_static_list_respects_the_filter() {
        assert!(static_edges(IpVersionFilter::V4Only).iter().all(|e| e.version == EdgeIpVersion::V4));
        assert!(static_edges(IpVersionFilter::V6Only).is_empty());
    }
}

//! The source DNS over HTTPS of the edge discovery: the SRV record, then
//! the A and AAAA records of its targets, all in one deadline.
//!
//! One question to one host of a resolver has [`DOH_EACH`], shorter than
//! [`pod_dns::EACH`]: behind a proxy, a connection to a resolver can succeed
//! and give no answer, and each such host costs this limit.

use std::net::IpAddr;

use tokio::time::Instant;

use super::{DOH_EACH, DOH_LIMIT, checked_targets_with, edge_srv};
use crate::Host;
use crate::cloudflare::{EdgeAddr, EdgeIpVersion, IpVersionFilter};
use crate::fail::{OrFail, Result, bail};
use pod_core::{Class, Stage};

/// Resolve through the DNS-over-HTTPS JSON API of the resolvers of the
/// host, in [`DOH_LIMIT`].
pub async fn discover_doh(host: &Host, filter: IpVersionFilter) -> Result<Vec<EdgeAddr>> {
    discover_doh_until(host, filter, Instant::now() + DOH_LIMIT).await
}

/// [`discover_doh`] until `deadline`. The A and AAAA questions of the
/// targets run at the same time. When no address comes and a question
/// failed, the failures are the error.
pub(super) async fn discover_doh_until(
    host: &Host,
    filter: IpVersionFilter,
    deadline: Instant,
) -> Result<Vec<EdgeAddr>> {
    let way = host.resolver.way().or_fail(Stage::Dns, Class::Unsupported, "the resolver asks no DNS over HTTPS")?;
    let mut settings = way.settings.clone();
    settings.each = settings.each.min(DOH_EACH);
    let way = pod_dns::Way { settings: &settings, ..way };

    let srv = query(&way, edge_srv(), "SRV", deadline).await?;
    let mut targets: Vec<(String, u16)> = Vec::new();
    for answer in &srv.records {
        if answer.kind != 33 {
            continue;
        }
        if let Some((target, port)) = super::parse_srv(&answer.data) {
            targets.push((target, port));
        }
    }
    let targets = checked_targets_with(targets, &host.report);

    let records: &[(&str, u64)] = match filter {
        IpVersionFilter::V4Only => &[("A", 1)],
        IpVersionFilter::V6Only => &[("AAAA", 28)],
        IpVersionFilter::Auto => &[("A", 1), ("AAAA", 28)],
    };
    let asks = (0u8..).zip(&targets).flat_map(|(region, (target, port))| {
        let way = &way;
        records.iter().map(move |(rtype, wanted)| async move {
            (region, *port, *wanted, query(way, target, rtype, deadline).await)
        })
    });
    let mut edges = Vec::new();
    let mut failures = Vec::new();
    for (region, port, wanted, answer) in futures::future::join_all(asks).await {
        let answer = match answer {
            Ok(answer) => answer,
            Err(err) => {
                failures.push(err.chain());
                continue;
            }
        };
        for entry in answer.records.iter().filter(|entry| entry.kind == wanted) {
            let Ok(ip) = entry.data.trim().parse::<IpAddr>() else {
                continue;
            };
            let version = if ip.is_ipv4() { EdgeIpVersion::V4 } else { EdgeIpVersion::V6 };
            edges.push(EdgeAddr { ip, port, version, region });
        }
    }
    if edges.is_empty() && !failures.is_empty() {
        bail!(Stage::Dns, class_at(deadline), "each address query failed: {}", failures.join("; "));
    }
    Ok(edges)
}

/// Ask the DoH resolvers of `way` for `rtype` records of `name`, one
/// resolver after the other, until one answers or `deadline` comes. A
/// failure goes to the log at debug with each resolver and its cause.
async fn query(way: &pod_dns::Way<'_>, name: &str, rtype: &str, deadline: Instant) -> Result<pod_dns::json::Answer> {
    let answer = match way.query(name, rtype, deadline).await {
        Ok(answer) => answer,
        Err(failures) => {
            tracing::debug!(%name, rtype, %failures, "a DoH query failed");
            bail!(Stage::Dns, class_at(deadline), "{failures}");
        }
    };
    if answer.status != Some(0) {
        let status = answer.status.map_or("missing".into(), |s| s.to_string());
        tracing::debug!(%name, rtype, %status, "a DoH query failed");
        bail!(Stage::Dns, Class::Unavailable, "DoH status {status} for {name} {rtype}");
    }
    Ok(answer)
}

/// The class of a failure at `deadline`: a timeout when it came.
fn class_at(deadline: Instant) -> Class {
    if Instant::now() >= deadline { Class::Timeout } else { Class::Unavailable }
}

/// The way of the questions, for a person: "DNS over HTTPS through the
/// proxy P", or with no proxy.
pub(super) fn way_words(host: &Host) -> String {
    let proxy = host.resolver.way().and_then(|way| {
        let first = way.settings.resolvers.first()?;
        way.dial.proxy_label(&pod_core::Target::new(&first.name, first.port))
    });
    match proxy {
        Some(proxy) => format!("DNS over HTTPS through the proxy {proxy}"),
        None => "DNS over HTTPS with no proxy".to_string(),
    }
}

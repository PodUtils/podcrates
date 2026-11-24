//! IP pooling disambiguator separating `Paired`, `Arbitrary`, `PerDestinationEgress` (ECMP)
//! and endpoint-dependent mapping.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, SocketAddr};

use super::alloc::AllocSample;
use crate::report::{IpPooling, Unknown, UnknownReason};

/// One observation on the single mapping socket `M` to a specific destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DestObs {
    pub dest: SocketAddr,
    pub mapped: SocketAddr,
}

/// High-level mapping view derived from comparing ports and IPs separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapView {
    /// All mapped ports equal across destinations.
    EndpointIndependentPort { arbitrary_ip: bool },
    /// At least one IP group has multiple destinations with differing mapped ports.
    NotEndpointIndependent,
    /// Ports differ, but every external IP appeared for only one destination (confounded).
    ConfoundedByPooling,
}

/// Combined result of IP pooling analysis.
#[derive(Clone, Debug, PartialEq)]
pub struct PoolingAnalysis {
    pub pooling: Result<IpPooling, Unknown>,
    pub confidence: f32,
    pub map_view: Option<MapView>,
}

/// Disambiguate IP pooling from destination-dependent mapping using two orthogonal sample sets:
/// - `fresh_same_dest`: fresh sockets to ONE destination (holds destination constant).
/// - `per_dest`: ONE socket `M` to multiple destinations (holds socket constant).
pub fn classify_pooling(fresh_same_dest: &[AllocSample], per_dest: &[DestObs]) -> PoolingAnalysis {
    let ips_fresh: BTreeSet<IpAddr> = fresh_same_dest.iter().map(|s| s.mapped.ip()).collect();
    let ips_dest: BTreeSet<IpAddr> = per_dest.iter().map(|o| o.mapped.ip()).collect();
    let k = fresh_same_dest.len();

    let (pooling, confidence) = if ips_fresh.len() > 1 {
        (Ok(IpPooling::Arbitrary), 1.0)
    } else if k >= 5 && ips_fresh.len() == 1 && ips_dest.len() > 1 {
        (Ok(IpPooling::PerDestinationEgress), 0.90)
    } else if k >= 5 && ips_fresh.len() == 1 && ips_dest.len() <= 1 {
        let conf = 1.0 - 0.5f32.powi((k - 1) as i32);
        (Ok(IpPooling::Paired), conf)
    } else {
        (Err(Unknown { reason: UnknownReason::TooFewSamples, constraint: crate::report::Constraint::None }), 0.0)
    };

    let map_view = if per_dest.len() >= 2 {
        let ports: BTreeSet<u16> = per_dest.iter().map(|o| o.mapped.port()).collect();
        if ports.len() == 1 {
            Some(MapView::EndpointIndependentPort { arbitrary_ip: ips_dest.len() > 1 })
        } else {
            let mut by_ip: BTreeMap<IpAddr, Vec<u16>> = BTreeMap::new();
            for o in per_dest {
                by_ip.entry(o.mapped.ip()).or_default().push(o.mapped.port());
            }
            let same_ip_port_diff = by_ip.values().any(|ps| {
                let unique: BTreeSet<u16> = ps.iter().copied().collect();
                unique.len() > 1
            });
            if same_ip_port_diff { Some(MapView::NotEndpointIndependent) } else { Some(MapView::ConfoundedByPooling) }
        }
    } else {
        None
    };

    PoolingAnalysis { pooling, confidence, map_view }
}

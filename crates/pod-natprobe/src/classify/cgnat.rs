//! CGNAT and double-NAT noisy-OR evidence combiner (signals E1 through E12).

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use crate::report::{EvidenceItem, IpPooling, LayerCount, LayerKind, NatLayers, PortAllocation};
use crate::wire::addr::{AddrClass, AddrPolicy, classify_ip, is_rfc1918, is_shared_cgnat};

/// Collected local, STUN, portmapper and behavioural signals for [`cgnat_verdict`].
#[derive(Clone, Debug, Default)]
pub struct CgnatInput {
    pub direct_public: bool,
    pub route_source_v4: Option<Ipv4Addr>,
    pub route_iface_is_tunnel: bool,
    pub has_v6_gua: bool,
    pub stun_mapped_ip: Option<IpAddr>,
    pub portmap_external_v4: Option<Ipv4Addr>,
    pub pcp_address_mismatch: bool,
    pub pooling: Option<IpPooling>,
    pub allocation: Option<PortAllocation>,
    pub lifetime_upper_bound: Option<Duration>,
    pub reachability_echo_failed: bool,
    pub policy: AddrPolicy,
}

/// Combine all available signals into a [`NatLayers`] verdict.
pub fn cgnat_verdict(input: &CgnatInput) -> NatLayers {
    if input.direct_public {
        return NatLayers {
            layers: Some(LayerCount::Zero),
            kind: LayerKind::None,
            confidence: 0.95,
            evidence: vec![EvidenceItem::E11MappedEqualsLocal],
            contradictions: Vec::new(),
        };
    }

    let mut evidence: Vec<(EvidenceItem, f32)> = Vec::new();
    let mut e1 = false;
    let mut e2 = false;
    let mut e3_cgnat = false;
    let mut e3_private = false;
    let mut e4 = false;
    let mut e5 = false;
    let mut e6 = false;
    let mut e10 = false;

    if let Some(src) = input.route_source_v4 {
        let cls = classify_ip(IpAddr::V4(src), input.policy);
        if cls == AddrClass::SharedCgnat && !input.route_iface_is_tunnel {
            e1 = true;
            evidence.push((EvidenceItem::E1RouteSourceSharedSpace, 0.90));
        } else if cls == AddrClass::DsLite && input.has_v6_gua {
            e2 = true;
            evidence.push((EvidenceItem::E2DsLiteAftr, 0.90));
        }
    }

    if let Some(pm_ip) = input.portmap_external_v4
        && !pm_ip.is_unspecified()
    {
        let pm_cls = classify_ip(IpAddr::V4(pm_ip), input.policy);
        if pm_cls != AddrClass::Global {
            if is_shared_cgnat(pm_ip) {
                e3_cgnat = true;
            } else if is_rfc1918(pm_ip) || pm_cls == AddrClass::Ula || pm_cls == AddrClass::Documentation {
                e3_private = true;
            }
            evidence.push((EvidenceItem::E3PortmapIpNotGlobal, 0.95));
        } else if let Some(stun_ip) = input.stun_mapped_ip {
            if IpAddr::V4(pm_ip) == stun_ip {
                e5 = true;
            } else {
                let w = match input.pooling {
                    Some(IpPooling::PerDestinationEgress) => 0.0,
                    Some(IpPooling::Arbitrary) => 0.15,
                    _ => 0.40,
                };
                if w > 0.0 {
                    e4 = true;
                    evidence.push((EvidenceItem::E4PortmapIpDiffersFromStun, w));
                }
            }
        }
    }

    if let Some(stun_ip) = input.stun_mapped_ip
        && classify_ip(stun_ip, input.policy) != AddrClass::Global
    {
        e6 = true;
    }
    if input.pooling == Some(IpPooling::Arbitrary) {
        evidence.push((EvidenceItem::E7ArbitraryPooling, 0.20));
    }
    if matches!(input.allocation, Some(PortAllocation::GlobalCounter { .. } | PortAllocation::Random { pool: Some(_) }))
    {
        evidence.push((EvidenceItem::E8CounterOrNarrowPool, 0.20));
    }
    if input.lifetime_upper_bound.is_some_and(|d| d < Duration::from_secs(120)) {
        evidence.push((EvidenceItem::E9ShortLifetime, 0.15));
    }
    if input.pcp_address_mismatch {
        e10 = true;
        evidence.push((EvidenceItem::E10PcpAddressMismatch, 0.60));
    }
    if input.reachability_echo_failed {
        evidence.push((EvidenceItem::E12ReachabilityEcho, 1.0));
    }

    let prod_one_minus: f32 = evidence.iter().fold(1.0f32, |acc, &(_, w)| acc * (1.0 - w));
    let mut conf = 1.0 - prod_one_minus;

    let kind = if e1 || e2 || e3_cgnat {
        LayerKind::Cgnat
    } else if e3_private || e10 {
        LayerKind::DoubleNat
    } else if e4 {
        LayerKind::UpstreamNatLikely
    } else if e5 {
        conf = 0.85;
        LayerKind::SingleNat
    } else {
        conf = conf.min(0.40);
        LayerKind::SingleNatAssumed
    };

    let mut ev_items: Vec<EvidenceItem> = evidence.into_iter().map(|(item, _)| item).collect();
    if e5 {
        ev_items.push(EvidenceItem::E5PortmapIpEqualsStun);
    }
    if e6 {
        ev_items.push(EvidenceItem::E6StunIpNotGlobal);
    }
    let mut contradictions = Vec::new();
    if e5 && (e1 || e2 || e3_cgnat || e3_private || e10) {
        conf *= 0.5;
        contradictions.push(EvidenceItem::E5PortmapIpEqualsStun);
    }

    let layers = if e3_cgnat || e3_private || e1 || e2 || e10 {
        Some(LayerCount::AtLeastTwo)
    } else if e5 && !e6 {
        Some(LayerCount::One)
    } else {
        None
    };

    NatLayers { layers, kind, confidence: conf, evidence: ev_items, contradictions }
}

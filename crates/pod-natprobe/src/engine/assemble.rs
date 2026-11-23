//! Pure report assembler converting [`ProbeMachine`] state into a validated [`Report`].

use std::net::IpAddr;

use super::assemble_aux::{
    build_allocation, build_hairpin, build_ipv6, build_lifetime, build_nat64, build_portmap, ev_from_slice,
};
use super::io::{SOCK_F, SOCK_M};
use super::machine::ProbeMachine;
use crate::classify::{
    CgnatInput, DestObs, Verdict, alg_from_responses, cgnat_verdict, classify_pooling, filtering_verdict,
    mapping_verdict, mapping_verdict_multi,
};
use crate::report::{
    Certainty, Connectivity, Evidence, Family, FamilyParts, FamilyReport, FilteringBehavior, Flag, Ipv6Status,
    LayerKind, MappingBehavior, NaReason, NonEmpty, Obs, ObsId, PortMappingSupport, Profile, QuestionId, REPORT_SCHEMA,
    Report, ServerRecord, UdpPathStatus, UnknownReason,
};
use crate::wire::addr::is_tunnel_iface;

pub(crate) fn assemble_report(m: &ProbeMachine) -> Report {
    let default_obs = m.observations.first().map_or(ObsId(0), |o| o.id);
    let portmap = build_portmap(m, default_obs);
    let ipv6 = build_ipv6(m, default_obs);
    let nat64 = build_nat64(m, default_obs);

    let (v4, layers, mut pending) = if !m.settings.servers_v4.is_empty() {
        build_v4_and_layers(m, &portmap, &ipv6, default_obs)
    } else {
        (None, Obs::NotApplicable(NaReason::NoFamily), Vec::new())
    };
    if !m.portmap.done {
        pending.push(QuestionId::Portmap);
    }
    if !m.nat64.done {
        pending.push(QuestionId::Dns64);
    }
    if !m.v6.done {
        pending.push(QuestionId::UdpV6);
    }

    let servers: Vec<ServerRecord> = m
        .server_notes
        .values()
        .map(|s| ServerRecord { name: s.name.clone(), ip: s.ip, flags: s.flags.clone() })
        .collect();

    Report {
        schema: REPORT_SCHEMA,
        profile: m.settings.profile,
        started: m.started_at,
        finished: m.now,
        v4,
        v6: None,
        ipv6,
        nat64,
        portmap,
        layers,
        servers,
        observations: m.observations.clone(),
        pending,
    }
}

fn build_v4_and_layers(
    m: &ProbeMachine,
    portmap: &PortMappingSupport,
    ipv6: &Obs<Ipv6Status>,
    default_obs: ObsId,
) -> (Option<FamilyReport>, Obs<crate::report::NatLayers>, Vec<QuestionId>) {
    let mut pending = Vec::new();
    let udp_path = if m.map.m1.is_some() {
        let obs = m.map.m1_obs.unwrap_or(default_obs);
        Obs::observed(UdpPathStatus::Open, Evidence::exact(obs), Vec::new())
    } else if m.settings.profile != Profile::Fast
        && m.map.done
        && m.has_distinct_v4_server_ips() >= 2
        && m.online_ok == Some(true)
    {
        Obs::observed(UdpPathStatus::Blocked, Evidence::exact(default_obs), Vec::new())
    } else if m.settings.profile != Profile::Fast && m.map.done && m.has_distinct_v4_server_ips() >= 2 {
        Obs::unknown(UnknownReason::NoCorroboration)
    } else if m.cancelled {
        Obs::unknown(UnknownReason::Cancelled)
    } else {
        pending.push(QuestionId::UdpV4);
        Obs::unknown(UnknownReason::Pending)
    };

    if matches!(udp_path.value(), Some(UdpPathStatus::Blocked)) {
        let parts = FamilyParts {
            family: Family::V4,
            udp_path,
            connectivity: Obs::observed(Connectivity::UdpBlocked, Evidence::exact(default_obs), Vec::new()),
            mapping: Obs::NotApplicable(NaReason::UdpNotOpen),
            filtering: Obs::NotApplicable(NaReason::UdpNotOpen),
            allocation: Obs::NotApplicable(NaReason::UdpNotOpen),
            pooling: Obs::NotApplicable(NaReason::UdpNotOpen),
            hairpin: Obs::NotApplicable(NaReason::UdpNotOpen),
            lifetime: Obs::NotApplicable(NaReason::UdpNotOpen),
            alg: Obs::NotApplicable(NaReason::UdpNotOpen),
            public_ips: Vec::new(),
            mapping_socket: SOCK_M,
            filtering_socket: SOCK_F,
        };
        return (FamilyReport::new(parts).ok(), Obs::NotApplicable(NaReason::UdpNotOpen), pending);
    }

    let local_m = m.map.local_addr.and_then(|a| m.local_v4_endpoint(a.port()));
    let direct_public = m.map.m1.is_some() && local_m.is_some() && m.map.m1 == local_m;

    let ordered_alloc = m.alloc.ordered_samples();
    let ordered_alloc_obs = m.alloc.ordered_obs_ids();
    let per_dest_obs: Vec<DestObs> = m.map.per_dest.iter().map(|&(d, mapped, _)| DestObs { dest: d, mapped }).collect();
    let pool_analysis = classify_pooling(&ordered_alloc, &per_dest_obs);
    let pooling = if direct_public {
        Obs::NotApplicable(NaReason::DirectPublic)
    } else if m.map.m1.is_none() {
        Obs::unknown(UnknownReason::UdpNotOpen)
    } else {
        match pool_analysis.pooling {
            Ok(p) => Obs::observed(p, ev_from_slice(&ordered_alloc_obs, default_obs, Certainty::Exact), Vec::new()),
            Err(u) => Obs::Unknown(u),
        }
    };

    let allocation = if direct_public {
        Obs::NotApplicable(NaReason::DirectPublic)
    } else if m.map.m1.is_none() {
        Obs::unknown(UnknownReason::UdpNotOpen)
    } else {
        build_allocation(m, default_obs, &mut pending)
    };

    let mapping = build_mapping(m, local_m, pooling.value().copied(), default_obs, &mut pending);
    let filtering = build_filtering(m, default_obs, &mut pending);
    let hairpin = build_hairpin(m, direct_public, filtering.value().copied(), default_obs, &mut pending);
    let lifetime = build_lifetime(m, direct_public, default_obs, &mut pending);

    let valid_refs: Vec<_> = m.valid_responses.iter().map(|r| &r.validated).collect();
    let alg = match alg_from_responses(&valid_refs) {
        Some(a) => {
            let flags = if matches!(a, crate::report::AlgDetection::Suspected { .. }) {
                vec![Flag::AlgSuspected]
            } else {
                Vec::new()
            };
            let obs = m.valid_responses.first().map_or(default_obs, |r| r.obs_id);
            Obs::observed(a, Evidence::exact(obs), flags)
        }
        None => Obs::unknown(UnknownReason::NoT1),
    };

    let has_v6_gua = matches!(ipv6.value(), Some(Ipv6Status::Ok { local: crate::report::LocalV6Class::Gua, .. }));
    let pm_ext = portmap.external_v4.first().map(|&(_, ip)| ip);
    let lt_upper = lifetime.value().and_then(|l| match l.bound {
        crate::report::LifetimeBound::Exact { high, .. } | crate::report::LifetimeBound::Below(high) => Some(high),
        crate::report::LifetimeBound::AtLeast(_) => None,
    });
    let cgnat_in = CgnatInput {
        direct_public,
        route_source_v4: m.local_env.route_source_v4,
        route_iface_is_tunnel: m.local_env.route_iface_v4.as_deref().is_some_and(is_tunnel_iface),
        has_v6_gua,
        stun_mapped_ip: m.map.m1.map(|a| a.ip()),
        portmap_external_v4: pm_ext,
        pcp_address_mismatch: m.portmap.pcp_address_mismatch,
        pooling: pooling.value().copied(),
        allocation: allocation.value().copied(),
        lifetime_upper_bound: lt_upper,
        reachability_echo_failed: false,
        policy: m.settings.addr_policy,
    };
    let layer_res = cgnat_verdict(&cgnat_in);
    let connectivity = if m.map.m1.is_none() {
        Obs::unknown(UnknownReason::NoT1)
    } else if direct_public {
        Obs::observed(Connectivity::DirectPublic, Evidence::exact(m.map.m1_obs.unwrap_or(default_obs)), Vec::new())
    } else {
        let conn = match layer_res.kind {
            LayerKind::None => Connectivity::DirectPublic,
            LayerKind::Cgnat => Connectivity::BehindCgnat,
            LayerKind::DoubleNat => Connectivity::DoubleNat,
            LayerKind::SingleNat | LayerKind::UpstreamNatLikely | LayerKind::SingleNatAssumed => {
                Connectivity::BehindNat
            }
        };
        let mut flags = Vec::new();
        if matches!(layer_res.kind, LayerKind::Cgnat | LayerKind::DoubleNat) {
            flags.push(Flag::ComposedLayers);
        }
        Obs::observed(conn, Evidence::exact(m.map.m1_obs.unwrap_or(default_obs)), flags)
    };
    let layers = if m.map.m1.is_some() {
        Obs::observed(layer_res, Evidence::exact(m.map.m1_obs.unwrap_or(default_obs)), Vec::new())
    } else {
        Obs::unknown(UnknownReason::LayersUnknown)
    };

    let mut public_ips = Vec::new();
    for &(_, mapped, _) in &m.map.per_dest {
        if !public_ips.contains(&mapped.ip()) {
            public_ips.push(mapped.ip());
        }
    }
    for s in &ordered_alloc {
        if !public_ips.contains(&s.mapped.ip()) {
            public_ips.push(s.mapped.ip());
        }
    }

    let parts = FamilyParts {
        family: Family::V4,
        udp_path,
        connectivity,
        mapping,
        filtering,
        allocation,
        pooling,
        hairpin,
        lifetime,
        alg,
        public_ips,
        mapping_socket: SOCK_M,
        filtering_socket: SOCK_F,
    };
    (FamilyReport::new(parts).ok(), layers, pending)
}

fn build_mapping(
    m: &ProbeMachine,
    local_m: Option<std::net::SocketAddr>,
    pooling: Option<crate::report::IpPooling>,
    default_obs: ObsId,
    pending: &mut Vec<QuestionId>,
) -> Obs<MappingBehavior> {
    if !m.map.done && m.map.m1.is_none() {
        pending.push(QuestionId::Mapping);
        return Obs::unknown(UnknownReason::Pending);
    }
    let local_pair = local_m.map_or((IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 0), |a| (a.ip(), a.port()));
    let verdict = if m.map.topo.is_some() {
        let mut v = mapping_verdict(
            m.map.m1.map(|a| (a.ip(), a.port())),
            m.map.m2.map(|a| (a.ip(), a.port())),
            m.map.m3.map(|a| (a.ip(), a.port())),
            m.map.m1b.map(|a| (a.ip(), a.port())),
            local_pair,
        );
        if let (Verdict::Known { value, .. }, Some(m_pa), Some(m1)) = (&v, m.map.m_pa, m.map.m1)
            && matches!(value, MappingBehavior::AddressDependent | MappingBehavior::AddressAndPortDependent)
            && ((*value == MappingBehavior::AddressDependent) != (m_pa == m1))
        {
            v = Verdict::Unknown(crate::report::Unknown {
                reason: UnknownReason::InconsistentMapping,
                constraint: crate::report::Constraint::None,
            });
        }
        v
    } else {
        let pairs: Vec<_> = m.map.per_dest.iter().map(|&(d, mapped, _)| (d, mapped)).collect();
        let loc_sa = std::net::SocketAddr::new(local_pair.0, local_pair.1);
        mapping_verdict_multi(&pairs, m.map.m1, m.map.m1b, loc_sa, pooling)
    };
    match verdict {
        Verdict::Known { value, certainty, flags } => {
            let obs_ids: Vec<ObsId> = m.map.per_dest.iter().map(|&(_, _, id)| id).collect();
            Obs::observed(value, ev_from_slice(&obs_ids, default_obs, certainty), flags)
        }
        Verdict::Unknown(u) => {
            if !m.map.done {
                pending.push(QuestionId::Mapping);
            }
            Obs::Unknown(u)
        }
    }
}

fn build_filtering(m: &ProbeMachine, default_obs: ObsId, pending: &mut Vec<QuestionId>) -> Obs<FilteringBehavior> {
    if m.filter.t1_timeout {
        if m.settings.profile != Profile::Fast && m.map.m1.is_some() {
            return Obs::observed(
                FilteringBehavior::NoReturnTraffic,
                Evidence { refs: NonEmpty::new(default_obs, Vec::new()), certainty: Certainty::Suspected },
                Vec::new(),
            );
        }
        return Obs::unknown(UnknownReason::NoT1);
    }
    if let Some(reason) = m.filter.no_other_reason.clone() {
        return Obs::unknown(reason);
    }
    let Some(t2) = m.filter.t2 else {
        pending.push(QuestionId::Filtering);
        return Obs::unknown(UnknownReason::Pending);
    };
    let t3 = m.filter.t3.unwrap_or(crate::classify::Probe::Timeout { sends: 0, elapsed_ms: 0 });
    let mut verdict = filtering_verdict(t2, t3, m.filter.c06, m.filter.c02);
    if m.settings.profile == Profile::Fast
        && matches!(
            verdict,
            Verdict::Known {
                value: FilteringBehavior::AddressDependent | FilteringBehavior::AddressAndPortDependent,
                ..
            }
        )
    {
        verdict = Verdict::Unknown(crate::report::Unknown {
            reason: UnknownReason::Pending,
            constraint: crate::report::Constraint::None,
        });
    }
    match verdict {
        Verdict::Known { value, certainty, mut flags } => {
            if m.filter.late_upgraded {
                flags.push(Flag::LateResponse);
            }
            let obs = m.filter.t2_obs.or(m.filter.m1_obs).unwrap_or(default_obs);
            Obs::observed(value, Evidence { refs: NonEmpty::new(obs, Vec::new()), certainty }, flags)
        }
        Verdict::Unknown(u) => {
            if !m.filter.done {
                pending.push(QuestionId::Filtering);
            }
            Obs::Unknown(u)
        }
    }
}

//! Auxiliary report builders: allocation, hairpin, lifetime, portmapper, IPv6 and NAT64.

use std::net::IpAddr;

use super::machine::ProbeMachine;
use crate::classify::{AllocOutcome, classify_allocation, classify_lifetime, is_time_correlated_counter, ring_delta};
use crate::report::{
    AbsenceProof, Certainty, Evidence, FilteringBehavior, Flag, HairpinBehavior, Ipv6Status, NaReason, Nat64Status,
    NatLifetime, NonEmpty, Obs, ObsId, PortAllocation, PortMappingSupport, Profile, QuestionId, UnknownReason,
    UpnpState,
};
use crate::wire::addr::{AddrClass, classify_ip, local_v6_class};

pub(crate) fn build_allocation(
    m: &ProbeMachine,
    default_obs: ObsId,
    pending: &mut Vec<QuestionId>,
) -> Obs<PortAllocation> {
    let allow_random = m.settings.profile != Profile::Fast;
    let samples = m.alloc.ordered_samples();
    let obs_ids = m.alloc.ordered_obs_ids();
    match classify_allocation(&samples, allow_random) {
        AllocOutcome::Known { mut value, mut certainty, mut flags, .. } => {
            if let (PortAllocation::Sequential { delta, jitter, .. }, Some(gap_mapped), Some(last)) =
                (value, m.alloc.deep_gap_mapped, samples.last())
            {
                let adv = ring_delta(last.mapped.port(), gap_mapped.port());
                if is_time_correlated_counter(delta, jitter, adv) {
                    let pace_s = (f64::from(m.settings.alloc_pace_ms.max(1))) / 1000.0;
                    let drift =
                        (f64::from(delta) / pace_s).round().clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
                    value = PortAllocation::GlobalCounter { drift_per_s: drift, jitter };
                    certainty = Certainty::Exact;
                }
            }
            if !m.alloc.done {
                flags.push(Flag::LateResponse);
            }
            Obs::observed(value, ev_from_slice(&obs_ids, default_obs, certainty), flags)
        }
        AllocOutcome::Unknown(u) => {
            if !m.alloc.done {
                pending.push(QuestionId::Allocation);
            }
            Obs::Unknown(u)
        }
    }
}

pub(crate) fn build_hairpin(
    m: &ProbeMachine,
    direct_public: bool,
    filtering: Option<FilteringBehavior>,
    default_obs: ObsId,
    pending: &mut Vec<QuestionId>,
) -> Obs<HairpinBehavior> {
    if direct_public {
        return Obs::NotApplicable(NaReason::DirectPublic);
    }
    if m.settings.profile == Profile::Fast {
        pending.push(QuestionId::Hairpin);
        return Obs::unknown(UnknownReason::Pending);
    }
    if let Some((behavior, obs_id, flags)) = &m.hairpin.observed {
        return Obs::observed(*behavior, Evidence::exact(*obs_id), flags.clone());
    }
    if !m.hairpin.done {
        pending.push(QuestionId::Hairpin);
        return Obs::unknown(UnknownReason::Pending);
    }
    if !m.hairpin.started {
        return Obs::NotApplicable(NaReason::HairpinGuard);
    }
    if filtering == Some(FilteringBehavior::EndpointIndependent) {
        Obs::observed(HairpinBehavior::None, Evidence::exact(default_obs), Vec::new())
    } else if m.hairpin.punch_used && m.hairpin.rounds_sent >= 3 {
        Obs::observed(
            HairpinBehavior::None,
            Evidence { refs: NonEmpty::new(default_obs, Vec::new()), certainty: Certainty::Suspected },
            vec![Flag::AfterPunch],
        )
    } else {
        Obs::unknown(UnknownReason::FilterConfounds)
    }
}

pub(crate) fn build_lifetime(
    m: &ProbeMachine,
    direct_public: bool,
    default_obs: ObsId,
    pending: &mut Vec<QuestionId>,
) -> Obs<NatLifetime> {
    if !m.lifetime.enabled {
        return Obs::unknown(UnknownReason::Pending);
    }
    if direct_public {
        return Obs::NotApplicable(NaReason::DirectPublic);
    }
    let Some(ctrl) = m.lifetime.control_outcome else {
        if !m.lifetime.done {
            pending.push(QuestionId::Lifetime);
        }
        return Obs::unknown(UnknownReason::Pending);
    };
    let rungs: Vec<_> =
        m.lifetime.rungs.iter().filter(|r| !r.is_control).filter_map(|r| r.outcome.map(|o| (r.idle_dur, o))).collect();
    match classify_lifetime(ctrl, &rungs, 1) {
        Ok(lt) => Obs::observed(lt, ev_from_slice(&m.lifetime.obs_ids, default_obs, Certainty::Exact), Vec::new()),
        Err(u) => Obs::Unknown(u),
    }
}

pub(crate) fn build_portmap(m: &ProbeMachine, default_obs: ObsId) -> PortMappingSupport {
    let cls = m.local_env.route_source_v4.map(|ip| classify_ip(IpAddr::V4(ip), m.settings.addr_policy));
    let upnp = if !m.settings.run_portmap {
        Obs::unknown(UnknownReason::Pending)
    } else if m.local_env.gateway_v4.is_none() {
        Obs::unknown(UnknownReason::NoGateway)
    } else if m.local_env.gateway_v4.is_some_and(|g| g.is_p2p) {
        Obs::unknown(UnknownReason::GatewayRange)
    } else if matches!(cls, Some(AddrClass::Global)) {
        Obs::NotApplicable(NaReason::PublicLocalAddress)
    } else if !m.portmap.igd_locations.is_empty() {
        Obs::observed(
            UpnpState {
                igd_count: m.portmap.igd_locations.len().min(usize::from(u8::MAX)) as u8,
                control_url_seen: m.portmap.upnp_control_url_seen,
            },
            Evidence::exact(default_obs),
            Vec::new(),
        )
    } else if m.portmap.done {
        Obs::unknown(UnknownReason::NoReplyInWindow)
    } else {
        Obs::unknown(UnknownReason::Pending)
    };
    PortMappingSupport {
        nat_pmp: m.portmap.nat_pmp.clone(),
        pcp: m.portmap.pcp.clone(),
        upnp,
        external_v4: m.portmap.external_v4.clone(),
    }
}

pub(crate) fn build_ipv6(m: &ProbeMachine, default_obs: ObsId) -> Obs<Ipv6Status> {
    if !m.local_env.v6_stack_available {
        return Obs::observed(Ipv6Status::NoStack, Evidence::exact(default_obs), Vec::new());
    }
    if m.local_env.v6_no_route {
        return Obs::Absent { proof: AbsenceProof::OsError { errno: 101 } };
    }
    if let Some(src6) = m.local_env.route_source_v6 {
        let local_cls = local_v6_class(src6);
        if local_cls == crate::report::LocalV6Class::LinkLocal {
            return Obs::observed(Ipv6Status::LinkLocalOnly, Evidence::exact(default_obs), Vec::new());
        }
        if let Some((mapped, obs)) = m.v6.mapped {
            let nat66 = Some(mapped.ip() != IpAddr::V6(src6));
            return Obs::observed(Ipv6Status::Ok { nat66, local: local_cls }, Evidence::exact(obs), Vec::new());
        }
    } else if let Some((_, obs)) = m.v6.mapped {
        return Obs::observed(
            Ipv6Status::Ok { nat66: None, local: crate::report::LocalV6Class::Gua },
            Evidence::exact(obs),
            Vec::new(),
        );
    }
    if m.v6.done { Obs::unknown(UnknownReason::NoV6Answer) } else { Obs::unknown(UnknownReason::Pending) }
}

pub(crate) fn build_nat64(m: &ProbeMachine, default_obs: ObsId) -> Obs<Nat64Status> {
    if !m.nat64.queried || !m.nat64.done {
        return Obs::unknown(UnknownReason::Pending);
    }
    let Some(status) = m.nat64.status.clone() else {
        return Obs::unknown(UnknownReason::NoReplyInWindow);
    };
    let mut flags = Vec::new();
    if m.nat64.resolver_is_doh {
        flags.push(Flag::DnsResolverIsDoh);
    }
    Obs::observed(status, Evidence::exact(m.nat64.obs_id.unwrap_or(default_obs)), flags)
}

pub(crate) fn ev_from_slice(ids: &[ObsId], fallback: ObsId, certainty: Certainty) -> Evidence {
    let refs = NonEmpty::from_vec(ids.to_vec()).unwrap_or_else(|| NonEmpty::new(fallback, Vec::new()));
    Evidence { refs, certainty }
}

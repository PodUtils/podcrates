//! Portmapper (NAT-PMP / PCP / UPnP SSDP), Hairpinning and NAT64/DNS64 sub-engines.

use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use super::io::{Output, ProbeEvent, SOCK_ALLOC_BASE, SOCK_P};
use super::machine::ProbeMachine;
use crate::report::{
    AbsenceProof, Evidence, ExternalIpSource, Flag, HairpinBehavior, Obs, ObservationKind, PcpState, PmpState, Profile,
    QuestionId, SockId, UnknownReason,
};
use crate::wire::addr::{AddrClass, classify_ip};
use crate::wire::nat64::discover_pref64;
use crate::wire::pcp::{OP_ANNOUNCE, PcpResult, announce_request, parse_reply as parse_pcp};
use crate::wire::pmp::{PmpReply, PmpResult, external_addr_request, parse_reply as parse_pmp};
use crate::wire::ssdp::{
    MSEARCH_ALL, MSEARCH_IGD1, build_get_external_ip_soap, find_wan_service, parse_external_ip_soap, parse_ssdp_reply,
    url_matches_host,
};

impl ProbeMachine {
    pub(crate) fn send_portmap_burst(&mut self) {
        let Some(gw) = self.local_env.gateway_v4 else {
            self.portmap.done = true;
            return;
        };
        let me = self.local_env.route_source_v4.map(IpAddr::V4).unwrap_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
        let cls = classify_ip(me, self.settings.addr_policy);
        let pmp_allowed = matches!(cls, AddrClass::Private | AddrClass::SharedCgnat | AddrClass::DsLite);

        let pmp_dst = SocketAddr::new(gw.ip, gw.pmp_port);
        if pmp_allowed {
            self.outputs.push_back(Output::Send {
                sock: SOCK_P,
                dest: pmp_dst,
                bytes: external_addr_request().to_vec(),
            });
        }
        if !matches!(cls, AddrClass::LinkLocal | AddrClass::Ula) {
            self.outputs.push_back(Output::Send { sock: SOCK_P, dest: pmp_dst, bytes: announce_request(me).to_vec() });
        }
        if pmp_allowed {
            let gw_ssdp = SocketAddr::new(gw.ip, gw.ssdp_port);
            self.outputs.push_back(Output::Send { sock: SOCK_P, dest: gw_ssdp, bytes: MSEARCH_ALL.to_vec() });
            self.outputs.push_back(Output::Send { sock: SOCK_P, dest: gw.ssdp_multicast, bytes: MSEARCH_ALL.to_vec() });
            self.outputs.push_back(Output::Send {
                sock: SOCK_P,
                dest: gw.ssdp_multicast,
                bytes: MSEARCH_IGD1.to_vec(),
            });
        }
        if !self.portmap.started {
            self.portmap.started = true;
            self.portmap.retry_at = Some(self.now.saturating_add(100_000));
            self.portmap.deadline = Some(self.now.saturating_add(250_000));
        }
    }

    pub(crate) fn tick_portmap(&mut self) {
        if self.portmap.done || !self.portmap.started {
            return;
        }
        if !self.portmap.retry_sent
            && let Some(at) = self.portmap.retry_at
            && self.now >= at
        {
            self.portmap.retry_sent = true;
            self.portmap.retry_at = None;
            self.send_portmap_burst();
        }
        let upnp_done = self.portmap.upnp_end_at.is_some_and(|t| self.now >= t);
        let timed_out = self.portmap.deadline.is_some_and(|t| self.now >= t);
        if upnp_done {
            self.portmap.upnp_end_at = None;
            self.maybe_start_upnp_http();
        }
        if timed_out {
            self.portmap.deadline = None;
            self.portmap.retry_at = None;
            if matches!(self.portmap.nat_pmp, Obs::Unknown(_)) {
                self.portmap.nat_pmp = Obs::unknown(UnknownReason::NoReplyInWindow);
            }
            if matches!(self.portmap.pcp, Obs::Unknown(_)) {
                self.portmap.pcp = Obs::unknown(UnknownReason::NoReplyInWindow);
            }
            self.maybe_start_upnp_http();
            if self.portmap.http_desc_id.is_none() && self.portmap.http_soap_id.is_none() {
                self.portmap.done = true;
                self.events.push_back(ProbeEvent::Partial(QuestionId::Portmap));
            }
        }
    }

    pub(crate) fn on_portmap_datagram(&mut self, from: SocketAddr, bytes: &[u8]) {
        let Some(gw) = self.local_env.gateway_v4 else { return };
        if from.port() == gw.pmp_port {
            if from.ip() != gw.ip {
                self.portmap.not_gateway_drops += 1;
                let _ = self.record_obs(SOCK_P, ObservationKind::Dropped { reason: "not-gateway".to_string() });
                return;
            }
            if let Some(reply) = parse_pcp(bytes, from.ip(), gw.ip) {
                if reply.opcode == OP_ANNOUNCE {
                    let obs = self.record_obs(
                        SOCK_P,
                        ObservationKind::GatewayReply {
                            proto: "pcp".to_string(),
                            detail: format!("result={:?}", reply.result),
                        },
                    );
                    match reply.result {
                        PcpResult::Success => {
                            self.portmap.pcp = Obs::observed(PcpState::Present, Evidence::exact(obs), Vec::new());
                        }
                        PcpResult::NotAuthorized => {
                            self.portmap.pcp =
                                Obs::observed(PcpState::PresentDisabled, Evidence::exact(obs), Vec::new());
                        }
                        PcpResult::AddressMismatch => {
                            self.portmap.pcp_address_mismatch = true;
                            self.portmap.pcp = Obs::observed(PcpState::BehindNat, Evidence::exact(obs), Vec::new());
                        }
                        PcpResult::UnsuppVersion => {
                            self.portmap.pcp = Obs::Absent { proof: AbsenceProof::Refusal { code: 1 } };
                        }
                        _ => {}
                    }
                }
                return;
            }
            if let Some(reply) = parse_pmp(bytes, from.ip(), gw.ip) {
                let obs = self.record_obs(
                    SOCK_P,
                    ObservationKind::GatewayReply { proto: "nat-pmp".to_string(), detail: format!("{reply:?}") },
                );
                match reply {
                    PmpReply::ExternalAddr { result, external_ip, .. } => {
                        let state = if result == PmpResult::NotAuthorized {
                            PmpState::PresentDisabled
                        } else {
                            PmpState::Present
                        };
                        if !matches!(self.portmap.nat_pmp, Obs::NotApplicable(_)) {
                            self.portmap.nat_pmp = Obs::observed(state, Evidence::exact(obs), Vec::new());
                        }
                        if let Some(ip) = external_ip {
                            let entry = (ExternalIpSource::NatPmp, ip);
                            if !self.portmap.external_v4.contains(&entry) {
                                self.portmap.external_v4.push(entry);
                            }
                        }
                    }
                    PmpReply::UnsupportedVersion { .. } => {
                        if !matches!(self.portmap.nat_pmp, Obs::NotApplicable(_)) {
                            self.portmap.nat_pmp =
                                Obs::observed(PmpState::SpeaksNatPmpOnly, Evidence::exact(obs), Vec::new());
                        }
                        self.portmap.pcp = Obs::Absent { proof: AbsenceProof::Refusal { code: 1 } };
                    }
                    PmpReply::Mapping { .. } => {}
                }
                return;
            }
            let _ = self.record_obs(SOCK_P, ObservationKind::Dropped { reason: "bad-pmp-pcp".to_string() });
            return;
        }
        // Only accept SSDP replies from the gateway IP or non-global LAN space, and require
        // LOCATION to use http:// addressing the responder's own IP (SSRF defense).
        let src_cls = classify_ip(from.ip(), self.settings.addr_policy);
        let src_is_lan =
            from.ip() == gw.ip || matches!(src_cls, AddrClass::Private | AddrClass::LinkLocal | AddrClass::SharedCgnat);
        if src_is_lan
            && let Some(igd) = parse_ssdp_reply(bytes)
            && url_matches_host(&igd.location, from.ip())
            && !self.portmap.igd_locations.iter().any(|(loc, _)| loc == &igd.location)
            && self.portmap.igd_locations.len() < 10
        {
            let _ = self.record_obs(
                SOCK_P,
                ObservationKind::GatewayReply { proto: "ssdp".to_string(), detail: igd.location.clone() },
            );
            if self.portmap.igd_locations.is_empty() {
                self.portmap.upnp_end_at = Some(self.now.saturating_add(50_000));
            }
            self.portmap.igd_locations.push((igd.location, from.ip()));
        }
    }

    fn maybe_start_upnp_http(&mut self) {
        if !self.settings.run_upnp_http
            || self.settings.profile == Profile::Fast
            || self.portmap.upnp_http_tried
            || self.portmap.http_desc_id.is_some()
            || self.portmap.http_soap_id.is_some()
        {
            return;
        }
        if let Some((loc, host_ip)) = self.portmap.igd_locations.first().cloned() {
            self.portmap.upnp_http_tried = true;
            let id = self.next_http_id;
            self.next_http_id += 1;
            self.portmap.http_desc_id = Some(id);
            self.portmap.http_desc_location = Some((loc.clone(), host_ip));
            self.outputs.push_back(Output::HttpGet { id, url: loc });
        }
    }

    pub(crate) fn on_http_reply(&mut self, id: u32, body: Result<String, String>) {
        if self.portmap.http_desc_id == Some(id) {
            self.portmap.http_desc_id = None;
            let desc_loc = self.portmap.http_desc_location.clone();
            if let (Ok(xml), Some((loc, expected_ip))) = (body, desc_loc)
                && let Some(svc) = find_wan_service(&xml, &loc)
                && url_matches_host(&svc.control_url, expected_ip)
            {
                self.portmap.upnp_control_url_seen = true;
                let (action, soap_body) = build_get_external_ip_soap(&svc.service_type);
                let sid = self.next_http_id;
                self.next_http_id += 1;
                self.portmap.http_soap_id = Some(sid);
                self.portmap.http_service_type = Some(svc.service_type);
                self.outputs.push_back(Output::HttpSoap { id: sid, url: svc.control_url, action, body: soap_body });
                return;
            }
            if self.portmap.deadline.is_none() {
                self.portmap.done = true;
                self.events.push_back(ProbeEvent::Partial(QuestionId::Portmap));
            }
            return;
        }
        if self.portmap.http_soap_id == Some(id) {
            self.portmap.http_soap_id = None;
            if let Ok(xml) = body
                && let Some(ip) = parse_external_ip_soap(&xml)
            {
                let _ = self.record_obs(
                    SOCK_P,
                    ObservationKind::GatewayReply { proto: "upnp-soap".to_string(), detail: ip.to_string() },
                );
                let entry = (ExternalIpSource::Upnp, ip);
                if !self.portmap.external_v4.contains(&entry) {
                    self.portmap.external_v4.push(entry);
                }
            }
            if self.portmap.deadline.is_none() {
                self.portmap.done = true;
                self.events.push_back(ProbeEvent::Partial(QuestionId::Portmap));
            }
        }
    }

    pub(crate) fn maybe_start_hairpin(&mut self) {
        if self.hairpin.started || self.hairpin.done {
            return;
        }
        if self.settings.profile == Profile::Fast {
            self.hairpin.done = true;
            return;
        }
        if !self.map.done || !self.alloc.done || !self.filter.done {
            return;
        }
        if let (Some(m1), Some(local_m)) = (self.map.m1, self.map.local_addr)
            && self.local_v4_endpoint(local_m.port()) == Some(m1)
        {
            self.hairpin.done = true;
            self.events.push_back(ProbeEvent::Partial(QuestionId::Hairpin));
            return;
        }
        let s1 = SockId(SOCK_ALLOC_BASE + 1);
        let s2 = SockId(SOCK_ALLOC_BASE + 2);
        let (Some(&l1), Some(&m1), Some(&l2), Some(&m2)) = (
            self.alloc.sock_locals.get(&s1),
            self.alloc.sock_mapped.get(&s1),
            self.alloc.sock_locals.get(&s2),
            self.alloc.sock_mapped.get(&s2),
        ) else {
            self.hairpin.done = true;
            return;
        };
        if self.local_env.local_ips.contains(&m1.ip()) || self.local_env.local_ips.contains(&m2.ip()) {
            self.hairpin.done = true;
            return;
        }
        let loc1 = self.local_v4_endpoint(l1.port()).unwrap_or(l1);
        let loc2 = self.local_v4_endpoint(l2.port()).unwrap_or(l2);
        self.hairpin.started = true;
        self.hairpin.s1 = Some((s1, loc1, m1));
        self.hairpin.s2 = Some((s2, loc2, m2));
        self.hairpin.pool_split = m1.ip() != m2.ip();
        self.hairpin.punch_used = self.settings.hairpin_punch_mode;
        self.send_hairpin_round();
    }

    pub(crate) fn tick_hairpin(&mut self) {
        if self.hairpin.done || !self.hairpin.started {
            return;
        }
        if let Some(at) = self.hairpin.next_round_at
            && self.now >= at
        {
            if self.hairpin.rounds_sent < 3 {
                self.send_hairpin_round();
            } else {
                self.hairpin.next_round_at = None;
                self.hairpin.done = true;
                self.events.push_back(ProbeEvent::Partial(QuestionId::Hairpin));
            }
        }
    }

    pub(crate) fn on_hairpin_datagram(&mut self, sock: SockId, from: SocketAddr, txid: [u8; 16]) {
        if self.hairpin.done || !self.hairpin.started {
            return;
        }
        let (Some((s1, loc1, m1)), Some((s2, loc2, m2))) = (self.hairpin.s1, self.hairpin.s2) else {
            return;
        };
        let (expected_ext, expected_loc) = if sock == s2 && self.hairpin.sent_txids_12.contains(&txid) {
            (m1, loc1)
        } else if sock == s1 && self.hairpin.sent_txids_21.contains(&txid) {
            (m2, loc2)
        } else {
            return;
        };
        let obs = self.record_obs(sock, ObservationKind::LocalFact { note: format!("hairpin-from-{from}") });
        let mut flags = Vec::new();
        if self.hairpin.pool_split {
            flags.push(Flag::PoolSplit);
        }
        let behavior = if from == expected_ext {
            HairpinBehavior::ExternalSource
        } else if from == expected_loc {
            flags.push(Flag::HairpinRepliedFromInternal);
            HairpinBehavior::InternalSource
        } else if from.ip() == expected_ext.ip() {
            HairpinBehavior::ExternalIpOnly
        } else {
            HairpinBehavior::OtherSource
        };
        self.hairpin.observed = Some((behavior, obs, flags));
        self.hairpin.next_round_at = None;
        self.hairpin.done = true;
        self.events.push_back(ProbeEvent::Partial(QuestionId::Hairpin));
    }

    pub(crate) fn on_dns64_answer(&mut self, aaaa: Vec<Ipv6Addr>, has_a: bool, resolver_is_doh: bool, failed: bool) {
        self.nat64.done = true;
        self.nat64.resolver_is_doh = resolver_is_doh;
        self.nat64.failed = failed;
        let obs = self.record_obs(
            SOCK_P,
            ObservationKind::DnsResult { detail: format!("aaaa={} has_a={has_a} failed={failed}", aaaa.len()) },
        );
        self.nat64.obs_id = Some(obs);
        if !failed {
            self.nat64.status = Some(discover_pref64(&aaaa, self.local_env.v6_wkp_routable));
        }
        self.events.push_back(ProbeEvent::Partial(QuestionId::Dns64));
    }
}

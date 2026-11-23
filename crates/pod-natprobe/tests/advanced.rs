//! Integration tests for port allocation (ring wrap, counter vs random), CGNAT & double-NAT,
//! hairpinning, ALG, mapping lifetime, IPv6/NAT64, portmapper, 81-cell strategy predictor,
//! birthday math, legacy projections, compact binary wire profile and JSON roundtrip.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use pod_natprobe::classify::{AllocSample, classify_allocation, ring_delta};
use pod_natprobe::report::legacy::{Axes, EasyTier, Gaming, Rfc3489, easytier, nintendo, psn, rfc3489, xbox};
use pod_natprobe::report::wire::CompactProfile;
use pod_natprobe::report::{
    AbsenceProof, AlgDetection, Connectivity, FilteringBehavior, Flag, HairpinBehavior, Ipv6Status, LayerKind,
    LifetimeBound, LocalV6Class, MappingBehavior, NaReason, Nat64Status, Obs, PcpState, PmpState, PortAllocation,
    Pref64, Profile, UnknownReason,
};
use pod_natprobe::sim::{
    NatSpec, ServerMode, SimAlloc, SimDns64, SimGateway, SimPcpMode, SimPmpMode, SimStunServer, SimUpnpMode, SimWorld,
    drive_sim,
};
use pod_natprobe::strategy::{
    PeerClass, RelayReason, Side, TraversalStrategy, birthday_exact, plan_spray, predict, predict_same_nat,
};
use pod_natprobe::wire::AddrPolicy;
use pod_natprobe::wire::nat64::{MARKER_A, MARKER_B, discover_pref64, synthesize_v6};
use pod_natprobe::{Gateway, LifetimeSettings, ServerEndpoint, Settings};

const CLIENT_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 10);
const WAN_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10));
const SRV_PP: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)), 3478);
const SRV_AA: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 2)), 3479);

fn base_std() -> Settings {
    let mut s = Settings::for_profile(Profile::Standard, vec![ServerEndpoint::new("s1", SRV_PP)])
        .with_addr_policy(AddrPolicy::Lab);
    s.run_nat64 = false;
    s.run_portmap = false;
    s.run_ipv6 = false;
    s
}

#[test]
fn port_allocation_ring_wrap_collisions_random_and_counter() {
    // 1. Ring wrap: 65532 -> 1026 is +6 on the 64512-port ring (NOT +1030 as naive i16 thinks).
    assert_eq!(ring_delta(65532, 1026), 6);
    assert_eq!(ring_delta(1026, 65532), -6);

    let mut spec = NatSpec::home_eim_apdf(WAN_IP);
    spec.alloc = SimAlloc::Sequential { start: 65526, step: 6 };
    let mut w = SimWorld::new(CLIENT_IP, vec![spec], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], 10);
    let (r, _) = drive_sim(base_std(), &mut w, 10).unwrap();
    assert!(matches!(
        r.v4.unwrap().allocation.value(),
        Some(PortAllocation::Sequential { delta: 6, jitter: 0, tight: true })
    ));

    // 2. Preserving with collision on every 3rd sample -> Preserving + PreserveCollisions.
    let mut spec_col = NatSpec::home_eim_apdf(WAN_IP);
    spec_col.alloc = SimAlloc::PreserveWithCollision { collide_every: 3, fallback_start: 50_000 };
    let mut w2 = SimWorld::new(CLIENT_IP, vec![spec_col], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], 11);
    let (r2, _) = drive_sim(base_std(), &mut w2, 11).unwrap();
    let alloc_obs = r2.v4.unwrap().allocation;
    assert!(
        matches!(alloc_obs, Obs::Observed { value: PortAllocation::Preserving, ref flags, .. } if flags.contains(&Flag::PreserveCollisions))
    );

    // 3. Narrow random pool 49152..=65535 -> Random { pool: Some(_) } + NarrowPool.
    let mut spec_rand = NatSpec::home_eim_apdf(WAN_IP);
    spec_rand.alloc = SimAlloc::Random { lo: 49_152, hi: 65_535 };
    let mut w3 = SimWorld::new(CLIENT_IP, vec![spec_rand], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], 12);
    let (r3, _) = drive_sim(base_std(), &mut w3, 12).unwrap();
    assert!(matches!(r3.v4.unwrap().allocation.value(), Some(PortAllocation::Random { .. })));

    // 4. Deep idle-gap promotes a time-driven shared counter to GlobalCounter.
    let mut spec_ctr = NatSpec::home_eim_apdf(WAN_IP);
    spec_ctr.alloc = SimAlloc::Counter { start: 20_000, rate_per_s: 40 };
    let mut w4 = SimWorld::new(CLIENT_IP, vec![spec_ctr], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], 13);
    let mut deep =
        Settings::for_profile(Profile::Deep, vec![ServerEndpoint::new("s1", SRV_PP)]).with_addr_policy(AddrPolicy::Lab);
    deep.run_nat64 = false;
    deep.run_portmap = false;
    deep.run_ipv6 = false;
    deep.lifetime = None;
    let (r4, _) = drive_sim(deep, &mut w4, 13).unwrap();
    assert!(
        matches!(r4.v4.as_ref().unwrap().allocation.value(), Some(PortAllocation::GlobalCounter { .. })),
        "got {:?}",
        r4.v4
    );

    // 5. Uniform random ports must NOT trip GlobalCounter false positive.
    let mut false_pos = 0usize;
    let mut state = 0x1234_5678u64;
    for _ in 0..500 {
        let mut samples = Vec::new();
        for i in 0..8u64 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let port = (1024 + ((state >> 32) % 64512)) as u16;
            samples.push(AllocSample {
                local_port: 40000 + i as u16,
                mapped: SocketAddr::new(WAN_IP, port),
                at: i * 100_000,
            });
        }
        if let pod_natprobe::classify::AllocOutcome::Known { value: PortAllocation::GlobalCounter { .. }, .. } =
            classify_allocation(&samples, true)
        {
            false_pos += 1;
        }
    }
    assert_eq!(false_pos, 0, "uniform random ports tripped GlobalCounter {false_pos} times");
}

#[test]
fn cgnat_stacked_nat_double_nat_and_tunnel_guard() {
    // Stacked CGNAT: inner EIM/EIF 60s -> outer ADM/ADF/seq 6s; gateway NAT-PMP external = 100.64.10.2.
    let mut inner = NatSpec::home_eim_apdf(IpAddr::V4(Ipv4Addr::new(100, 64, 10, 2)));
    inner.filtering = FilteringBehavior::EndpointIndependent;
    inner.lifetime_us = Some(60_000_000);
    let mut outer = NatSpec::home_eim_apdf(WAN_IP);
    outer.mapping = MappingBehavior::AddressDependent;
    outer.filtering = FilteringBehavior::AddressDependent;
    outer.alloc = SimAlloc::Sequential { start: 31_000, step: 2 };
    outer.lifetime_us = Some(6_000_000);

    let gw_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
    let mut gw = SimGateway::silent(gw_ip);
    gw.pmp = SimPmpMode::ExternalIp(Ipv4Addr::new(100, 64, 10, 2));
    let mut w = SimWorld::new(CLIENT_IP, vec![inner, outer], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], 5);
    w.local_env.gateway_v4 = Some(Gateway::standard(gw_ip));
    w.gateway = Some(gw);

    let mut cfg = base_std();
    cfg.run_portmap = true;
    let (r, _) = drive_sim(cfg, &mut w, 5).unwrap();
    let v4 = r.v4.as_ref().unwrap();
    assert_eq!(v4.connectivity.value(), Some(&Connectivity::BehindCgnat));
    assert_eq!(v4.mapping.value(), Some(&MappingBehavior::AddressDependent));
    assert_eq!(r.layers.value().map(|l| l.kind), Some(LayerKind::Cgnat));

    // Tunnel interface `tailscale0` with 100.64/10 must NOT claim CGNAT.
    let mut w_ts = SimWorld::new(
        Ipv4Addr::new(100, 72, 1, 5),
        vec![NatSpec::home_eim_apdf(WAN_IP)],
        vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)],
        6,
    );
    w_ts.local_env.route_iface_v4 = Some("tailscale0".to_string());
    let (r_ts, _) = drive_sim(base_std(), &mut w_ts, 6).unwrap();
    assert_eq!(r_ts.v4.unwrap().connectivity.value(), Some(&Connectivity::BehindNat));

    // Double NAT via router WAN = 10.0.0.2 + PCP ADDRESS_MISMATCH.
    let mut gw2 = SimGateway::silent(gw_ip);
    gw2.pmp = SimPmpMode::ExternalIp(Ipv4Addr::new(10, 0, 0, 2));
    gw2.pcp = SimPcpMode::AddressMismatch;
    let mut w_dn = SimWorld::new(
        CLIENT_IP,
        vec![NatSpec::home_eim_apdf(WAN_IP)],
        vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)],
        7,
    );
    w_dn.local_env.gateway_v4 = Some(Gateway::standard(gw_ip));
    w_dn.gateway = Some(gw2);
    let mut cfg2 = base_std();
    cfg2.run_portmap = true;
    let (r_dn, _) = drive_sim(cfg2, &mut w_dn, 7).unwrap();
    assert_eq!(r_dn.v4.unwrap().connectivity.value(), Some(&Connectivity::DoubleNat));
    assert_eq!(r_dn.portmap.pcp.value(), Some(&PcpState::BehindNat));
}

#[test]
fn hairpin_modes_and_apdf_filter_confound_guard() {
    // ExternalSource hairpin under EIF.
    let mut nat_ext = NatSpec::home_eim_apdf(WAN_IP);
    nat_ext.filtering = FilteringBehavior::EndpointIndependent;
    nat_ext.hairpin = HairpinBehavior::ExternalSource;
    let mut w1 = SimWorld::new(CLIENT_IP, vec![nat_ext], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], 1);
    let (r1, _) = drive_sim(base_std(), &mut w1, 1).unwrap();
    assert_eq!(r1.v4.unwrap().hairpin.value(), Some(&HairpinBehavior::ExternalSource));

    // InternalSource hairpin -> flagged HairpinRepliedFromInternal.
    let mut nat_int = NatSpec::home_eim_apdf(WAN_IP);
    nat_int.filtering = FilteringBehavior::EndpointIndependent;
    nat_int.hairpin = HairpinBehavior::InternalSource;
    let mut w2 = SimWorld::new(CLIENT_IP, vec![nat_int], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], 2);
    let (r2, _) = drive_sim(base_std(), &mut w2, 2).unwrap();
    assert!(matches!(
        r2.v4.unwrap().hairpin,
        Obs::Observed { value: HairpinBehavior::InternalSource, ref flags, .. }
            if flags.contains(&Flag::HairpinRepliedFromInternal)
    ));

    // No hairpin under APDF without punch mode -> Unknown(FilterConfounds), NEVER None!
    let mut w3 = SimWorld::new(
        CLIENT_IP,
        vec![NatSpec::home_eim_apdf(WAN_IP)],
        vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)],
        3,
    );
    let mut cfg_nopunch = base_std();
    cfg_nopunch.hairpin_punch_mode = false;
    let (r3, _) = drive_sim(cfg_nopunch, &mut w3, 3).unwrap();
    assert!(matches!(r3.v4.unwrap().hairpin, Obs::Unknown(ref u) if u.reason == UnknownReason::FilterConfounds));

    // No hairpin under APDF WITH punch mode -> None + AfterPunch.
    let mut w4 = SimWorld::new(
        CLIENT_IP,
        vec![NatSpec::home_eim_apdf(WAN_IP)],
        vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)],
        4,
    );
    let (r4, _) = drive_sim(base_std(), &mut w4, 4).unwrap();
    assert!(matches!(
        r4.v4.unwrap().hairpin,
        Obs::Observed { value: HairpinBehavior::None, ref flags, .. } if flags.contains(&Flag::AfterPunch)
    ));
}

#[test]
fn alg_detection_keeps_xor_address_and_flags_mangle() {
    let mut nat = NatSpec::home_eim_apdf(WAN_IP);
    nat.alg_mangle_plain = true;
    let mut w = SimWorld::new(CLIENT_IP, vec![nat], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], 8);
    let (r, _) = drive_sim(base_std(), &mut w, 8).unwrap();
    let v4 = r.v4.unwrap();
    assert!(matches!(v4.alg.value(), Some(AlgDetection::Suspected { .. })));
    assert!(v4.public_ips.contains(&WAN_IP), "XOR address must win over ALG-mangled MAPPED-ADDRESS");
}

#[test]
fn deep_mapping_lifetime_beats_port_preserving_recreation_and_detects_ignored_response_port() {
    // 8s port-preserving NAT -> Exact bracket covering 8s.
    let mut nat = NatSpec::home_eim_apdf(WAN_IP);
    nat.alloc = SimAlloc::Preserve;
    nat.lifetime_us = Some(8_000_000);
    let mut w = SimWorld::new(CLIENT_IP, vec![nat], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], 14);
    let mut cfg =
        Settings::for_profile(Profile::Deep, vec![ServerEndpoint::new("s1", SRV_PP)]).with_addr_policy(AddrPolicy::Lab);
    cfg.run_nat64 = false;
    cfg.run_portmap = false;
    cfg.run_ipv6 = false;
    cfg.lifetime = Some(LifetimeSettings { coarse_rungs_secs: vec![5, 10, 20], run_fine_rungs: true, repetitions: 1 });
    let (r, _) = drive_sim(cfg.clone(), &mut w, 14).unwrap();
    let lt = r.v4.unwrap().lifetime.value().copied().expect("lifetime observed");
    assert!(
        matches!(lt.bound, LifetimeBound::Exact { low, high } if low.as_secs_f64() <= 8.0 && high.as_secs_f64() >= 8.0),
        "unexpected bound: {lt:?}"
    );
    assert_eq!(lt.meets_req5, Some(false));

    // Server ignoring RESPONSE-PORT -> Unknown(ServerIgnoresResponsePort), NEVER Below!
    let mut srv = SimStunServer::rfc5780("ign", SRV_PP, SRV_AA);
    srv.mode = ServerMode::IgnoresResponsePort;
    let mut w2 = SimWorld::new(CLIENT_IP, vec![NatSpec::home_eim_apdf(WAN_IP)], vec![srv], 15);
    let (r2, _) = drive_sim(cfg, &mut w2, 15).unwrap();
    assert!(matches!(
        r2.v4.unwrap().lifetime,
        Obs::Unknown(ref u) if u.reason == UnknownReason::ServerIgnoresResponsePort
    ));
}

#[test]
fn portmapper_natpmp_only_8byte_spoof_drop_and_upnp_soap() {
    let gw_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
    let mut gw = SimGateway::silent(gw_ip);
    gw.pcp = SimPcpMode::NatPmpOnly8Byte;
    gw.upnp = SimUpnpMode {
        enabled: true,
        reply_from_ephemeral_port: true,
        location: "http://192.168.1.1:5000/rootDesc.xml".to_string(),
        external_ip: Some(Ipv4Addr::new(198, 51, 100, 10)),
    };
    let mut w = SimWorld::new(
        CLIENT_IP,
        vec![NatSpec::home_eim_apdf(WAN_IP)],
        vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)],
        16,
    );
    w.local_env.gateway_v4 = Some(Gateway::standard(gw_ip));
    w.gateway = Some(gw);
    let mut cfg = base_std();
    cfg.run_portmap = true;
    let (r, _) = drive_sim(cfg.clone(), &mut w, 16).unwrap();
    assert_eq!(r.portmap.nat_pmp.value(), Some(&PmpState::SpeaksNatPmpOnly));
    assert_eq!(r.portmap.pcp, Obs::Absent { proof: AbsenceProof::Refusal { code: 1 } });
    assert_eq!(r.portmap.upnp.value().map(|u| (u.igd_count, u.control_url_seen)), Some((1, true)));
    assert_eq!(r.layers.value().map(|l| l.kind), Some(LayerKind::SingleNat));

    // Spoofed 5351 reply from non-gateway IP is dropped.
    let mut gw_spoof = SimGateway::silent(gw_ip);
    gw_spoof.pmp = SimPmpMode::ExternalIp(Ipv4Addr::new(10, 0, 0, 1));
    gw_spoof.spoof_src_ip = Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 99)));
    let mut w2 = SimWorld::new(
        CLIENT_IP,
        vec![NatSpec::home_eim_apdf(WAN_IP)],
        vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)],
        17,
    );
    w2.local_env.gateway_v4 = Some(Gateway::standard(gw_ip));
    w2.gateway = Some(gw_spoof);
    let (r2, _) = drive_sim(cfg, &mut w2, 17).unwrap();
    assert!(matches!(r2.portmap.nat_pmp, Obs::Unknown(ref u) if u.reason == UnknownReason::NoReplyInWindow));
}

#[test]
fn nat64_all_six_rfc6052_lengths_and_hijack_and_ipv6_nat66() {
    for len in [96u8, 64, 56, 48, 40, 32] {
        let base_pref = Pref64 { prefix: "2001:db8:1234:5678::".parse().unwrap(), len };
        let a = synthesize_v6(base_pref, MARKER_A).unwrap();
        let b = synthesize_v6(base_pref, MARKER_B).unwrap();
        let status = discover_pref64(&[a, b], Some(true));
        match status {
            Nat64Status::Pref64 { prefixes, route_to_wkp } => {
                assert_eq!(route_to_wkp, Some(true));
                assert_eq!(prefixes.len(), 1, "len {len}");
                assert_eq!(prefixes[0].len, len);
            }
            other => panic!("expected Pref64 for len {len}, got {other:?}"),
        }
    }
    let hijack = discover_pref64(&["2001:db8::1".parse().unwrap()], None);
    assert_eq!(hijack, Nat64Status::DnsHijackSuspect);

    // End-to-end IPv6 + DNS64 in simulator.
    let srv6 = SocketAddr::new(IpAddr::V6("2001:db8::10".parse().unwrap()), 3478);
    let mut w = SimWorld::new(
        CLIENT_IP,
        vec![NatSpec::home_eim_apdf(WAN_IP)],
        vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA), SimStunServer::plain("s6", srv6)],
        19,
    );
    let local_v6: Ipv6Addr = "2001:db8:1::2".parse().unwrap();
    w.local_env.v6_stack_available = true;
    w.local_env.v6_no_route = false;
    w.local_env.route_source_v6 = Some(local_v6);
    w.dns64 = SimDns64 {
        aaaa: vec!["64:ff9b::192.0.0.170".parse().unwrap(), "64:ff9b::192.0.0.171".parse().unwrap()],
        has_a: true,
        resolver_is_doh: false,
        failed: false,
    };
    let mut cfg = base_std().with_servers_v6(vec![ServerEndpoint::new("s6", srv6)]);
    cfg.run_ipv6 = true;
    cfg.run_nat64 = true;
    let (r, _) = drive_sim(cfg, &mut w, 19).unwrap();
    assert_eq!(r.ipv6.value(), Some(&Ipv6Status::Ok { nat66: Some(false), local: LocalV6Class::Gua }));
    assert!(matches!(r.nat64.value(), Some(Nat64Status::Pref64 { prefixes, .. }) if prefixes[0].len == 96));
}

#[test]
fn strategy_81_cells_birthday_spray_legacy_wire_and_json() {
    use FilteringBehavior as F;
    use MappingBehavior as M;
    let ms = [M::EndpointIndependent, M::AddressDependent, M::AddressAndPortDependent];
    let fs = [F::EndpointIndependent, F::AddressDependent, F::AddressAndPortDependent];
    let (mut direct, mut sim_open, mut bday, mut relay) = (0, 0, 0, 0);
    for ma in ms {
        for fa in fs {
            for mb in ms {
                for fb in fs {
                    let pa = PeerClass { udp_ok: true, mapping: Some(ma), filtering: Some(fa), predictable: false };
                    let pb = PeerClass { udp_ok: true, mapping: Some(mb), filtering: Some(fb), predictable: false };
                    match predict(&pa, &pb) {
                        TraversalStrategy::Direct { .. } => direct += 1,
                        TraversalStrategy::SimultaneousOpen => sim_open += 1,
                        TraversalStrategy::BirthdaySpray { .. } => bday += 1,
                        TraversalStrategy::RelayRequired(RelayReason::BothHard) => relay += 1,
                        other => panic!("unexpected strategy {other:?}"),
                    }
                }
            }
        }
    }
    assert_eq!((direct, sim_open, bday, relay), (33, 12, 12, 24));
    assert_eq!(
        predict_same_nat(false, Some(HairpinBehavior::InternalSource)),
        (TraversalStrategy::Direct { dialer: Side::Both }, pod_natprobe::Certainty::Suspected)
    );
    assert!((birthday_exact(256, 256, 64512) - 0.6394).abs() < 0.001);
    assert!((birthday_exact(256, 1024, 64512) - 0.9835).abs() < 0.001);
    let spray = plan_spray(PortAllocation::Sequential { delta: 2, jitter: 0, tight: true }, 65534, 0.5, 1, 16, 1);
    assert!(spray.contains(&1024) && spray.len() == 61);

    // Legacy projections.
    let axes = Axes {
        connectivity: Connectivity::BehindNat,
        mapping: Some(M::AddressAndPortDependent),
        mapping_not_ei: false,
        filtering: Some(F::AddressAndPortDependent),
        allocation: Some(PortAllocation::Sequential { delta: 2, jitter: 0, tight: true }),
        portmap_active: false,
    };
    assert_eq!(rfc3489(&axes), Rfc3489::Symmetric);
    assert_eq!(easytier(&axes), EasyTier::SymmetricEasyInc);
    assert_eq!(nintendo(&axes), Gaming::NintendoD);
    assert_eq!(xbox(&axes), Gaming::XboxStrict);
    assert_eq!(psn(&axes), Gaming::Psn3);

    // Compact 20-byte binary profile + JSON roundtrip.
    let mut w = SimWorld::new(
        CLIENT_IP,
        vec![NatSpec::home_eim_apdf(WAN_IP)],
        vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)],
        99,
    );
    let (rep, _) = drive_sim(base_std(), &mut w, 99).unwrap();
    let cp = rep.compact_profile();
    let enc = cp.encode();
    assert_eq!(CompactProfile::decode(&enc).unwrap(), cp);
    let json = rep.to_json().unwrap();
    let parsed: pod_natprobe::NatReport = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.schema, rep.schema);
    assert_eq!(parsed.v4.unwrap().mapping, rep.v4.unwrap().mapping);

    // Public local address -> NAT-PMP/UPnP NotApplicable.
    let mut w_pub =
        SimWorld::new(Ipv4Addr::new(198, 51, 100, 5), vec![], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], 100);
    w_pub.local_env.gateway_v4 = Some(Gateway::standard(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1))));
    let mut cfg_pub = base_std();
    cfg_pub.run_portmap = true;
    let (r_pub, _) = drive_sim(cfg_pub, &mut w_pub, 100).unwrap();
    assert_eq!(r_pub.portmap.nat_pmp, Obs::NotApplicable(NaReason::PublicLocalAddress));
    assert_eq!(r_pub.portmap.upnp, Obs::NotApplicable(NaReason::PublicLocalAddress));
}

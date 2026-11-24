//! Integration tests for RFC 5780 NAT classes, broken servers, Fast profile invariants,
//! packet loss soundness, UDP blocking and report constructor invariants.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use pod_natprobe::report::{
    Connectivity, Constraint, Evidence, Family, FamilyParts, FamilyReport, FilteringBehavior, MappingBehavior,
    NaReason, Obs, ObsId, Profile, ReportError, SockId, UdpPathStatus, UnknownReason,
};
use pod_natprobe::sim::{NatSpec, ServerMode, SimAlloc, SimPool, SimStunServer, SimWorld, drive_sim};
use pod_natprobe::wire::AddrPolicy;
use pod_natprobe::{ServerEndpoint, Settings};

const CLIENT_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 10);
const WAN_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10));
const SRV_PP: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)), 3478);
const SRV_AA: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 2)), 3479);
const PLAIN_B: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 3)), 19302);

fn std_settings(servers: Vec<ServerEndpoint>) -> Settings {
    let mut s = Settings::for_profile(Profile::Standard, servers).with_addr_policy(AddrPolicy::Lab);
    s.run_nat64 = false;
    s.run_portmap = false;
    s.run_ipv6 = false;
    s
}

#[test]
fn all_nine_rfc5780_classes_verified() {
    use FilteringBehavior as F;
    use MappingBehavior as M;
    let mappings = [M::EndpointIndependent, M::AddressDependent, M::AddressAndPortDependent];
    let filterings = [F::EndpointIndependent, F::AddressDependent, F::AddressAndPortDependent];

    for map in mappings {
        for filt in filterings {
            let mut spec = NatSpec::home_eim_apdf(WAN_IP);
            spec.mapping = map;
            spec.filtering = filt;
            spec.alloc = SimAlloc::Sequential { start: 30_000, step: 1 };
            let srv = SimStunServer::rfc5780("s1", SRV_PP, SRV_AA);
            let mut world = SimWorld::new(CLIENT_IP, vec![spec], vec![srv], 42);
            let settings = std_settings(vec![ServerEndpoint::new("s1", SRV_PP)]);
            let (report, _cap) = drive_sim(settings, &mut world, 42).expect("sim ok");
            let v4 = report.v4.as_ref().expect("v4 report");

            assert_eq!(v4.mapping.value(), Some(&map), "mapping mismatch for ({map:?}, {filt:?})");
            // An address-dependent mapping keeps one mapping for PP and PA: the control of
            // CHANGE-REQUEST 0x02 stays valid and proves the port filter.
            let exact = filt == F::EndpointIndependent
                || map == M::EndpointIndependent
                || (map == M::AddressDependent && filt == F::AddressAndPortDependent);
            if exact {
                assert_eq!(v4.filtering.value(), Some(&filt), "filtering mismatch for ({map:?}, {filt:?})");
            } else {
                // A dependent mapping puts the pinhole of a control on a different external
                // port (Control::Invalid): the restrictive filtering stays Unknown.
                assert!(
                    matches!(v4.filtering, Obs::Unknown(_)),
                    "dependent mapping + restrictive filtering ({map:?}, {filt:?}) must not forge control: {:?}",
                    v4.filtering
                );
            }
        }
    }
}

#[test]
fn plain_servers_never_send_change_request_and_degrade_honestly() {
    // EIM behind two plain XOR-only servers (Cloudflare + Google model).
    let spec = NatSpec::home_eim_apdf(WAN_IP);
    let s1 = SimStunServer::plain("cf", SRV_PP);
    let s2 = SimStunServer::plain("gg", PLAIN_B);
    let mut world = SimWorld::new(CLIENT_IP, vec![spec], vec![s1, s2], 7);
    let settings = std_settings(vec![ServerEndpoint::new("cf", SRV_PP), ServerEndpoint::new("gg", PLAIN_B)]);
    let (report, cap) = drive_sim(settings, &mut world, 7).expect("sim");
    let v4 = report.v4.as_ref().unwrap();
    assert!(!cap.has_change_request(), "must never send CHANGE-REQUEST without OTHER-ADDRESS");
    assert_eq!(v4.mapping.value(), Some(&MappingBehavior::EndpointIndependent));
    assert!(matches!(v4.filtering, Obs::Unknown(ref u) if u.reason == UnknownReason::NoOtherAddress));

    // APDM behind two plain servers -> Unknown(NoT3) + MappingNotEndpointIndependent.
    let mut spec2 = NatSpec::home_eim_apdf(WAN_IP);
    spec2.mapping = MappingBehavior::AddressAndPortDependent;
    spec2.alloc = SimAlloc::Sequential { start: 25_000, step: 2 };
    let s1 = SimStunServer::plain("cf", SRV_PP);
    let s2 = SimStunServer::plain("gg", PLAIN_B);
    let mut world2 = SimWorld::new(CLIENT_IP, vec![spec2], vec![s1, s2], 9);
    let settings2 = std_settings(vec![ServerEndpoint::new("cf", SRV_PP), ServerEndpoint::new("gg", PLAIN_B)]);
    let (report2, cap2) = drive_sim(settings2, &mut world2, 9).expect("sim2");
    let v4_2 = report2.v4.as_ref().unwrap();
    assert!(!cap2.has_change_request());
    assert_eq!(v4_2.mapping.constraint(), Constraint::MappingNotEndpointIndependent);
    assert!(matches!(v4_2.filtering, Obs::Unknown(ref u) if u.reason == UnknownReason::NoOtherAddress));
}

#[test]
fn direct_public_host_sends_zero_hairpin_datagrams() {
    let public_ip = Ipv4Addr::new(198, 51, 100, 44);
    let srv = SimStunServer::rfc5780("s1", SRV_PP, SRV_AA);
    let mut world = SimWorld::new(public_ip, vec![], vec![srv], 11);
    let settings = std_settings(vec![ServerEndpoint::new("s1", SRV_PP)]);
    let (report, cap) = drive_sim(settings, &mut world, 11).expect("sim");
    let v4 = report.v4.as_ref().unwrap();
    assert_eq!(v4.connectivity.value(), Some(&Connectivity::DirectPublic));
    assert_eq!(v4.mapping.value(), Some(&MappingBehavior::EndpointIndependent));
    assert_eq!(v4.filtering.value(), Some(&FilteringBehavior::EndpointIndependent));
    assert_eq!(v4.hairpin, Obs::NotApplicable(NaReason::DirectPublic));
    assert_eq!(v4.allocation, Obs::NotApplicable(NaReason::DirectPublic));
    assert_eq!(cap.hairpin_count(), 0, "public host must emit zero hairpin datagrams");
}

#[test]
fn broken_server_ignoring_change_request_never_reports_eif() {
    let spec = NatSpec::home_eim_apdf(WAN_IP);
    let mut srv = SimStunServer::rfc5780("broken", SRV_PP, SRV_AA);
    srv.mode = ServerMode::IgnoresChangeRequest;
    let mut world = SimWorld::new(CLIENT_IP, vec![spec], vec![srv], 13);
    let settings = std_settings(vec![ServerEndpoint::new("broken", SRV_PP)]);
    let (report, _) = drive_sim(settings, &mut world, 13).expect("sim");
    let v4 = report.v4.as_ref().unwrap();
    assert!(
        matches!(v4.filtering, Obs::Unknown(ref u) if u.reason == UnknownReason::ServerIgnoresChangeRequest),
        "got {:?}",
        v4.filtering
    );
}

#[test]
fn server_refusing_change_request_420_and_mixed_n10_and_firewalled_alt() {
    // 420 refusal
    let mut srv420 = SimStunServer::rfc5780("s420", SRV_PP, SRV_AA);
    srv420.mode = ServerMode::RefusesChangeRequest420;
    let mut w1 = SimWorld::new(CLIENT_IP, vec![NatSpec::home_eim_apdf(WAN_IP)], vec![srv420], 1);
    let (r1, _) = drive_sim(std_settings(vec![ServerEndpoint::new("s420", SRV_PP)]), &mut w1, 1).unwrap();
    assert!(
        matches!(r1.v4.unwrap().filtering, Obs::Unknown(ref u) if u.reason == UnknownReason::ServerLacksChangeRequest)
    );

    // N-10: wrong source on 0x06, honours 0x02 -> ChangeIpUnverified + FilteringNotAddressAndPortDependent
    let mut srv_mix = SimStunServer::rfc5780("mix", SRV_PP, SRV_AA);
    srv_mix.mode = ServerMode::WrongSource06Honours02;
    let mut nat_eif = NatSpec::home_eim_apdf(WAN_IP);
    nat_eif.filtering = FilteringBehavior::AddressDependent;
    let mut w2 = SimWorld::new(CLIENT_IP, vec![nat_eif], vec![srv_mix], 2);
    let (r2, _) = drive_sim(std_settings(vec![ServerEndpoint::new("mix", SRV_PP)]), &mut w2, 2).unwrap();
    let f2 = r2.v4.unwrap().filtering;
    assert!(
        matches!(f2, Obs::Unknown(ref u) if u.reason == UnknownReason::ChangeIpUnverified
            && u.constraint == Constraint::FilteringNotAddressAndPortDependent),
        "got {f2:?}"
    );

    // Firewalled alternate server socket -> ServerSilentOrControlFailed (NEVER false APDF!)
    let mut srv_fw = SimStunServer::rfc5780("fw", SRV_PP, SRV_AA);
    srv_fw.mode = ServerMode::FirewalledAlt;
    let mut w3 = SimWorld::new(CLIENT_IP, vec![NatSpec::home_eim_apdf(WAN_IP)], vec![srv_fw], 3);
    let (r3, _) = drive_sim(std_settings(vec![ServerEndpoint::new("fw", SRV_PP)]), &mut w3, 3).unwrap();
    assert!(
        matches!(r3.v4.unwrap().filtering, Obs::Unknown(ref u) if u.reason == UnknownReason::ServerSilentOrControlFailed)
    );
}

#[test]
fn fast_profile_stays_within_600ms_and_never_emits_restrictive_filtering() {
    // EIF NAT in Fast -> reports EIF quickly.
    let mut nat_eif = NatSpec::home_eim_apdf(WAN_IP);
    nat_eif.filtering = FilteringBehavior::EndpointIndependent;
    let mut w_eif = SimWorld::new(CLIENT_IP, vec![nat_eif], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], 5);
    let mut fast =
        Settings::for_profile(Profile::Fast, vec![ServerEndpoint::new("s1", SRV_PP)]).with_addr_policy(AddrPolicy::Lab);
    fast.run_nat64 = false;
    fast.run_portmap = false;
    fast.run_ipv6 = false;
    let (r_eif, _) = drive_sim(fast.clone(), &mut w_eif, 5).unwrap();
    assert!(r_eif.elapsed_us() <= 600_000, "elapsed {}us", r_eif.elapsed_us());
    assert_eq!(r_eif.v4.unwrap().filtering.value(), Some(&FilteringBehavior::EndpointIndependent));

    // APDF NAT in Fast -> must NOT emit APDF (positive-only!).
    let mut w_apdf = SimWorld::new(
        CLIENT_IP,
        vec![NatSpec::home_eim_apdf(WAN_IP)],
        vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)],
        6,
    );
    let (r_apdf, _) = drive_sim(fast, &mut w_apdf, 6).unwrap();
    assert!(r_apdf.elapsed_us() <= 600_000);
    assert!(matches!(r_apdf.v4.unwrap().filtering, Obs::Unknown(_)), "Fast mode must never emit restrictive filtering");
}

#[test]
fn udp_blocked_requires_online_corroboration() {
    let mut spec = NatSpec::home_eim_apdf(WAN_IP);
    spec.udp_blocked = true;
    let servers = vec![SimStunServer::plain("s1", SRV_PP), SimStunServer::plain("s2", PLAIN_B)];
    let endpoints = vec![ServerEndpoint::new("s1", SRV_PP), ServerEndpoint::new("s2", PLAIN_B)];

    // With online corroboration -> Blocked.
    let mut w1 = SimWorld::new(CLIENT_IP, vec![spec.clone()], servers.clone(), 20);
    w1.online_ok = true;
    let (r1, _) = drive_sim(std_settings(endpoints.clone()), &mut w1, 20).unwrap();
    let v4_1 = r1.v4.unwrap();
    assert_eq!(v4_1.udp_path.value(), Some(&UdpPathStatus::Blocked));
    assert_eq!(v4_1.connectivity.value(), Some(&Connectivity::UdpBlocked));
    assert_eq!(v4_1.mapping, Obs::NotApplicable(NaReason::UdpNotOpen));

    // Without online corroboration -> Unknown(NoCorroboration), NOT Blocked.
    let mut w2 = SimWorld::new(CLIENT_IP, vec![spec], servers, 21);
    w2.online_ok = false;
    let (r2, _) = drive_sim(std_settings(endpoints), &mut w2, 21).unwrap();
    assert!(matches!(r2.v4.unwrap().udp_path, Obs::Unknown(ref u) if u.reason == UnknownReason::NoCorroboration));
}

#[test]
fn mid_run_remap_yields_unstable_mapping() {
    let mut spec = NatSpec::home_eim_apdf(WAN_IP);
    // RTT is 40 ms: T1 runs at t=0, T2 at t=40 ms, control repeat at t=80 ms.
    // Triggering remap at t=60 ms shifts the mapping before the control repeat.
    spec.remap_after_tick = Some(60_000);
    let mut w = SimWorld::new(CLIENT_IP, vec![spec], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], 31);
    let (r, _) = drive_sim(std_settings(vec![ServerEndpoint::new("s1", SRV_PP)]), &mut w, 31).unwrap();
    assert!(
        matches!(r.v4.unwrap().mapping, Obs::Unknown(ref u) if u.reason == UnknownReason::UnstableMapping),
        "mid-run remap must be caught by control repeat"
    );
}

#[test]
fn packet_loss_never_produces_false_wrong_class() {
    for seed in 1u64..=12 {
        let mut nat = NatSpec::home_eim_apdf(WAN_IP);
        nat.filtering = FilteringBehavior::EndpointIndependent;
        let mut w = SimWorld::new(CLIENT_IP, vec![nat], vec![SimStunServer::rfc5780("s1", SRV_PP, SRV_AA)], seed);
        w.loss_percent = 20;
        let (r, _) = drive_sim(std_settings(vec![ServerEndpoint::new("s1", SRV_PP)]), &mut w, seed).unwrap();
        let v4 = r.v4.unwrap();
        if let Some(&m) = v4.mapping.value() {
            assert_eq!(m, MappingBehavior::EndpointIndependent, "seed {seed}");
        }
        if let Some(&f) = v4.filtering.value() {
            assert_eq!(f, FilteringBehavior::EndpointIndependent, "seed {seed} must never turn loss into ADF/APDF");
        }
    }
}

#[test]
fn family_report_constructor_refuses_planted_defects() {
    let ev = Evidence::exact(ObsId(0));
    let base = || FamilyParts {
        family: Family::V4,
        udp_path: Obs::observed(UdpPathStatus::Open, ev.clone(), vec![]),
        connectivity: Obs::observed(Connectivity::BehindNat, ev.clone(), vec![]),
        mapping: Obs::unknown(UnknownReason::Pending),
        filtering: Obs::unknown(UnknownReason::Pending),
        allocation: Obs::unknown(UnknownReason::Pending),
        pooling: Obs::unknown(UnknownReason::Pending),
        hairpin: Obs::unknown(UnknownReason::Pending),
        lifetime: Obs::unknown(UnknownReason::Pending),
        alg: Obs::unknown(UnknownReason::Pending),
        public_ips: vec![],
        mapping_socket: SockId(0),
        filtering_socket: SockId(1),
    };

    let mut p = base();
    p.filtering_socket = SockId(0);
    assert_eq!(FamilyReport::new(p), Err(ReportError::SharedSocket));

    let mut p = base();
    p.udp_path = Obs::observed(UdpPathStatus::Blocked, ev.clone(), vec![]);
    assert_eq!(FamilyReport::new(p), Err(ReportError::BlockedMismatch));

    let mut p = base();
    p.udp_path = Obs::observed(UdpPathStatus::Blocked, ev.clone(), vec![]);
    p.connectivity = Obs::observed(Connectivity::UdpBlocked, ev.clone(), vec![]);
    p.mapping = Obs::observed(MappingBehavior::EndpointIndependent, ev.clone(), vec![]);
    assert_eq!(FamilyReport::new(p), Err(ReportError::NatFieldOnBlockedPath("mapping")));

    let mut p = base();
    p.connectivity = Obs::observed(Connectivity::DirectPublic, ev.clone(), vec![]);
    p.hairpin = Obs::observed(pod_natprobe::HairpinBehavior::ExternalSource, ev, vec![]);
    assert_eq!(FamilyReport::new(p), Err(ReportError::FieldOnDirectPublic("hairpin")));
}

#[test]
fn alt_xor_8020_server_still_succeeds_and_flags() {
    let mut srv = SimStunServer::rfc5780("old", SRV_PP, SRV_AA);
    srv.mode = ServerMode::AltXor8020;
    let mut w = SimWorld::new(CLIENT_IP, vec![NatSpec::home_eim_apdf(WAN_IP)], vec![srv], 99);
    let (r, _) = drive_sim(std_settings(vec![ServerEndpoint::new("old", SRV_PP)]), &mut w, 99).unwrap();
    let v4 = r.v4.unwrap();
    assert_eq!(v4.mapping.value(), Some(&MappingBehavior::EndpointIndependent));
    assert_eq!(v4.filtering.value(), Some(&FilteringBehavior::AddressAndPortDependent));
}

#[test]
fn arbitrary_pool_with_eim_ports_is_not_symmetric() {
    let mut spec = NatSpec::home_eim_apdf(WAN_IP);
    spec.pool = SimPool::Arbitrary(vec![
        IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10)),
        IpAddr::V4(Ipv4Addr::new(198, 51, 100, 11)),
    ]);
    spec.alloc = SimAlloc::Preserve;
    let servers = vec![SimStunServer::plain("s1", SRV_PP), SimStunServer::plain("s2", PLAIN_B)];
    let endpoints = vec![ServerEndpoint::new("s1", SRV_PP), ServerEndpoint::new("s2", PLAIN_B)];
    let mut w = SimWorld::new(CLIENT_IP, vec![spec], servers, 77);
    let (r, _) = drive_sim(std_settings(endpoints), &mut w, 77).unwrap();
    let v4 = r.v4.unwrap();
    assert_eq!(v4.pooling.value(), Some(&pod_natprobe::IpPooling::Arbitrary));
    assert_eq!(v4.mapping.value(), Some(&MappingBehavior::EndpointIndependent));
    let (ts_varies, diverges) = pod_natprobe::report::legacy::tailscale_varies_by_dest_ip(&v4.mapping, &v4.pooling);
    assert_eq!(ts_varies, Some(false));
    assert!(diverges);
}

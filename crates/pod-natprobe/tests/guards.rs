//! The guards against a hostile reply (T-1152) and a flood of stray datagrams (T-1153).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use pod_natprobe::engine::{Input, ProbeEvent, ProbeMachine, SOCK_M};
use pod_natprobe::report::{MAX_DROPPED_OBS, ObservationKind, Profile};
use pod_natprobe::sim::{NatSpec, SeededEntropy, SimGateway, SimPool, SimStunServer, SimUpnpMode, SimWorld, drive_sim};
use pod_natprobe::wire::ssdp::{gateway_url, url_matches_host};
use pod_natprobe::wire::{AddrPolicy, usable_other};
use pod_natprobe::{Gateway, ServerEndpoint, Settings};

const CLIENT_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 10);
const GW: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

fn settings(servers: Vec<ServerEndpoint>, policy: AddrPolicy) -> Settings {
    let mut s = Settings::for_profile(Profile::Standard, servers).with_addr_policy(policy);
    s.run_nat64 = false;
    s.run_portmap = false;
    s.run_ipv6 = false;
    s
}

#[test]
fn ssrf_gateway_url_takes_the_gateway_alone() {
    // A URL parser reads `192.168.1.1:80` as user information and the host as attacker.example.
    assert!(!url_matches_host("http://192.168.1.1:80@attacker.example/", GW));
    assert!(!url_matches_host("http://192.168.1.1@attacker.example/x", GW));
    assert!(!url_matches_host("http://192.168.1.1:5000/a\r\nHost: x/", GW));
    assert!(!url_matches_host("http://192.168.1.1:5000/a b", GW));
    assert!(!url_matches_host("http://192.168.1.1:5000\\@x/", GW));
    assert!(!url_matches_host("http://192.168.1.1:5000/#frag", GW));
    assert!(!url_matches_host("http://192.168.1.1:0/", GW));
    assert!(!url_matches_host("http://192.168.1.1:65536/", GW));
    assert!(!url_matches_host("http://192.168.1.1:+80/", GW));
    assert!(!url_matches_host("http://gateway.lan:5000/", GW));
    assert_eq!(gateway_url("http://192.168.1.1/d.xml", GW).as_deref(), Some("http://192.168.1.1:80/d.xml"));
    assert_eq!(gateway_url("http://192.168.1.1:5000", GW).as_deref(), Some("http://192.168.1.1:5000/"));
    let v6: IpAddr = "fe80::1".parse().unwrap();
    assert_eq!(gateway_url("http://[fe80::1]:49000/igd.xml", v6).as_deref(), Some("http://[fe80::1]:49000/igd.xml"));
    assert!(!url_matches_host("http://[fe80::1]x/igd.xml", v6));
}

#[test]
fn ssrf_engine_follows_no_location_with_user_information() {
    let mut gw = SimGateway::silent(GW);
    gw.upnp = SimUpnpMode {
        enabled: true,
        reply_from_ephemeral_port: false,
        location: "http://192.168.1.1:80@attacker.example/rootDesc.xml".to_string(),
        external_ip: Some(Ipv4Addr::new(198, 51, 100, 10)),
    };
    let srv_pp: SocketAddr = "203.0.113.1:3478".parse().unwrap();
    let srv_aa: SocketAddr = "203.0.113.2:3479".parse().unwrap();
    let wan = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10));
    let mut w = SimWorld::new(
        CLIENT_IP,
        vec![NatSpec::home_eim_apdf(wan)],
        vec![SimStunServer::rfc5780("s1", srv_pp, srv_aa)],
        5,
    );
    w.local_env.gateway_v4 = Some(Gateway::standard(GW));
    w.gateway = Some(gw);
    let mut cfg = settings(vec![ServerEndpoint::new("s1", srv_pp)], AddrPolicy::Lab);
    cfg.run_portmap = true;
    let (r, _) = drive_sim(cfg, &mut w, 5).expect("sim");
    let upnp = r.portmap.upnp.value().map(|u| (u.igd_count, u.control_url_seen));
    assert!(!matches!(upnp, Some((n, _)) if n > 0), "a LOCATION with user information is no IGD: {upnp:?}");
    assert!(
        !r.observations
            .iter()
            .any(|o| matches!(&o.kind, ObservationKind::GatewayReply { proto, .. } if proto == "ssdp"))
    );
}

#[test]
fn ssrf_public_server_sends_the_probe_to_no_lan_other_address() {
    // A public server names an OTHER-ADDRESS in the LAN of the client.
    let pp: SocketAddr = "1.2.3.4:3478".parse().unwrap();
    let lan: SocketAddr = "192.168.1.1:3479".parse().unwrap();
    assert!(!usable_other(pp, lan, AddrPolicy::Internet));
    assert!(usable_other(pp, "1.2.3.5:3479".parse().unwrap(), AddrPolicy::Internet));
    let wan = IpAddr::V4(Ipv4Addr::new(5, 6, 7, 8));
    let mut w =
        SimWorld::new(CLIENT_IP, vec![NatSpec::home_eim_apdf(wan)], vec![SimStunServer::rfc5780("s1", pp, lan)], 6);
    let (_, cap) =
        drive_sim(settings(vec![ServerEndpoint::new("s1", pp)], AddrPolicy::Internet), &mut w, 6).expect("sim");
    let to_lan: Vec<_> = cap.sent.iter().filter(|p| p.dst.ip() == lan.ip()).map(|p| p.dst).collect();
    assert!(to_lan.is_empty(), "datagrams to the LAN: {to_lan:?}");
    assert!(!cap.has_change_request(), "no CHANGE-REQUEST without a usable OTHER-ADDRESS");
}

#[test]
fn ssrf_hairpin_goes_to_no_private_mapped_address() {
    // The outer address of the NAT is private: a hairpin datagram would go into a LAN.
    let pp: SocketAddr = "1.2.3.4:3478".parse().unwrap();
    let aa: SocketAddr = "1.2.3.5:3479".parse().unwrap();
    let mut nat = NatSpec::home_eim_apdf(IpAddr::V4(Ipv4Addr::new(10, 9, 8, 7)));
    nat.pool = SimPool::Single(IpAddr::V4(Ipv4Addr::new(10, 9, 8, 7)));
    let mut w = SimWorld::new(CLIENT_IP, vec![nat], vec![SimStunServer::rfc5780("s1", pp, aa)], 7);
    let (_, cap) =
        drive_sim(settings(vec![ServerEndpoint::new("s1", pp)], AddrPolicy::Internet), &mut w, 7).expect("sim");
    assert_eq!(cap.hairpin_count(), 0, "no hairpin datagram to a private mapped address");
}

#[test]
fn stray_datagrams_keep_the_report_bounded() {
    let pp: SocketAddr = "203.0.113.1:3478".parse().unwrap();
    let env = pod_natprobe::LocalEnv { route_source_v4: Some(CLIENT_IP), ..Default::default() };
    let mut m = ProbeMachine::new(
        settings(vec![ServerEndpoint::new("s1", pp)], AddrPolicy::Lab),
        env,
        Box::new(SeededEntropy::new(1)),
    )
    .expect("machine");
    m.start(0);
    let from: SocketAddr = "198.51.100.66:4444".parse().unwrap();
    let floods = 100_000u64;
    for i in 0..floods {
        let bytes = if i % 2 == 0 { vec![0u8; 8] } else { vec![0u8; 20] };
        m.handle_input(i, Input::Datagram { sock: SOCK_M, from, bytes });
    }
    m.handle_input(floods, Input::Cancel);
    let mut report = None;
    while let Some(ev) = m.poll_event() {
        if let ProbeEvent::Final(r) = ev {
            report = Some(r);
        }
    }
    let r = report.expect("final report");
    let dropped = r.observations.iter().filter(|o| matches!(o.kind, ObservationKind::Dropped { .. })).count();
    assert_eq!(dropped, MAX_DROPPED_OBS as usize);
    assert!(r.observations.len() < 2 * MAX_DROPPED_OBS as usize, "{} observations", r.observations.len());
    let counted: u64 = r.dropped_unrecorded.values().sum();
    assert_eq!(counted + u64::from(MAX_DROPPED_OBS), floods);
}

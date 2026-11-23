//! Regression tests for wire, classifier, security and state-machine edge cases.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use pod_natprobe::classify::{
    AllocSample, Control, DestObs, Probe, RungOutcome, Verdict, classify_lifetime, classify_pooling, filtering_verdict,
    mapping_verdict_multi,
};
use pod_natprobe::report::{FilteringBehavior, MappingBehavior, Nat64Status, Pref64, Profile, UnknownReason};
use pod_natprobe::sim::{NatSpec, SimGateway, SimStunServer, SimUpnpMode, SimWorld, drive_sim};
use pod_natprobe::wire::nat64::{MARKER_A, MARKER_B, discover_pref64, synthesize_v6};
use pod_natprobe::wire::ssdp::{find_wan_service, parse_ssdp_reply, url_matches_host};
use pod_natprobe::wire::{AddrPolicy, ValidateOutcome, usable_other, validate_response};
use pod_natprobe::{Gateway, ServerEndpoint, Settings};
use pod_turn::stun::{BINDING_SUCCESS, MESSAGE_INTEGRITY, XOR_MAPPED_ADDRESS, message, write_address};

#[test]
fn nat64_slash96_with_nonzero_byte8_roundtrips_and_discovers() {
    let pref = Pref64 { prefix: Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0x0100, 0, 0, 0), len: 96 };
    let a = synthesize_v6(pref, MARKER_A).expect("synth a");
    let b = synthesize_v6(pref, MARKER_B).expect("synth b");
    assert_eq!(a.octets()[8], 0x01, "/96 must preserve byte 8");
    let status = discover_pref64(&[a, b], Some(true));
    assert_eq!(status, Nat64Status::Pref64 { prefixes: vec![pref], route_to_wkp: Some(true) });
}

#[test]
fn ssdp_rejects_ssrf_and_handles_xml_attrs_and_relative_urls() {
    // Status line must check token 200, not substring " 200".
    assert!(
        parse_ssdp_reply(
            b"HTTP/1.1 404 Not Found 200\r\nLOCATION: http://192.168.1.1/x\r\nST: :InternetGatewayDevice:1\r\n\r\n"
        )
        .is_none()
    );

    // SSRF guard: LOCATION or controlURL to non-responder host or non-http scheme is rejected.
    let gw = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
    assert!(url_matches_host("http://192.168.1.1:5000/rootDesc.xml", gw));
    assert!(!url_matches_host("http://169.254.169.254/latest/meta-data/", gw));
    assert!(!url_matches_host("https://192.168.1.1:5000/rootDesc.xml", gw));
    assert!(!url_matches_host("file:///etc/passwd", gw));

    // Relative controlURL without leading slash resolves against directory, and XML attributes parse.
    let xml = r#"<root><URLBase></URLBase><device><serviceList>
        <service xmlns="urn:x"><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
        <controlURL>ctl/IPConn</controlURL></service>
    </serviceList></device></root>"#;
    let svc = find_wan_service(xml, "http://192.168.1.1:5000/igd/rootDesc.xml").expect("found");
    assert_eq!(svc.control_url, "http://192.168.1.1:5000/igd/ctl/IPConn");

    // End-to-end: gateway advertising LOCATION on 169.254.169.254 is ignored (0 IGDs).
    let client_ip = Ipv4Addr::new(192, 168, 1, 10);
    let wan_ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10));
    let pp: SocketAddr = "203.0.113.1:3478".parse().unwrap();
    let aa: SocketAddr = "203.0.113.2:3479".parse().unwrap();
    let mut g = SimGateway::silent(gw);
    g.upnp = SimUpnpMode {
        enabled: true,
        reply_from_ephemeral_port: false,
        location: "http://169.254.169.254/latest/meta-data/".to_string(),
        external_ip: Some(Ipv4Addr::new(198, 51, 100, 10)),
    };
    let mut w =
        SimWorld::new(client_ip, vec![NatSpec::home_eim_apdf(wan_ip)], vec![SimStunServer::rfc5780("s1", pp, aa)], 1);
    w.local_env.gateway_v4 = Some(Gateway::standard(gw));
    w.gateway = Some(g);
    let mut cfg =
        Settings::for_profile(Profile::Standard, vec![ServerEndpoint::new("s1", pp)]).with_addr_policy(AddrPolicy::Lab);
    cfg.run_nat64 = false;
    cfg.run_ipv6 = false;
    cfg.run_portmap = true;
    let (rep, _) = drive_sim(cfg, &mut w, 1).unwrap();
    assert!(rep.portmap.upnp.value().is_none(), "SSRF LOCATION must be rejected");
}

#[test]
fn usable_other_rejects_multicast_broadcast_and_remote_loopback() {
    let pp: SocketAddr = "203.0.113.1:3478".parse().unwrap();
    assert!(!usable_other(pp, "127.0.0.1:3479".parse().unwrap()));
    assert!(!usable_other(pp, "224.0.0.1:3479".parse().unwrap()));
    assert!(!usable_other(pp, "255.255.255.255:3479".parse().unwrap()));
    assert!(usable_other("127.0.0.1:3478".parse().unwrap(), "127.0.0.2:3479".parse().unwrap()));
}

#[test]
fn validate_accepts_message_integrity_in_binding_success() {
    let tid = [7u8; 12];
    let src: SocketAddr = "203.0.113.1:3478".parse().unwrap();
    let mapped: SocketAddr = "198.51.100.10:40000".parse().unwrap();
    let xval = write_address(mapped, Some(&tid));
    let mi = [0xaa; 20];
    let bytes = message(BINDING_SUCCESS, &tid, &[(XOR_MAPPED_ADDRESS, &xval), (MESSAGE_INTEGRITY, &mi)]);
    assert!(matches!(validate_response(&bytes, src, src, &tid), ValidateOutcome::Ok(v) if v.mapped() == mapped));
}

#[test]
fn classifiers_handle_k_lt_5_pooling_non_monotone_lifetime_and_out_of_order_multi() {
    // k < 5 fresh samples must NOT claim PerDestinationEgress.
    let wan1: IpAddr = "198.51.100.10".parse().unwrap();
    let wan2: IpAddr = "198.51.100.11".parse().unwrap();
    let fresh = vec![
        AllocSample { local_port: 40001, mapped: SocketAddr::new(wan1, 40001), at: 0 },
        AllocSample { local_port: 40002, mapped: SocketAddr::new(wan1, 40002), at: 25_000 },
        AllocSample { local_port: 40003, mapped: SocketAddr::new(wan1, 40003), at: 50_000 },
    ];
    let per_dest = vec![
        DestObs { dest: "203.0.113.1:3478".parse().unwrap(), mapped: SocketAddr::new(wan1, 40001) },
        DestObs { dest: "203.0.113.2:3478".parse().unwrap(), mapped: SocketAddr::new(wan2, 40001) },
    ];
    assert!(classify_pooling(&fresh, &per_dest).pooling.is_err());

    // Non-monotone lifetime (10s Expired, 20s Alive) -> NonMonotone.
    let lt_err = classify_lifetime(
        RungOutcome::Alive,
        &[(Duration::from_secs(10), RungOutcome::ExpiredSilent), (Duration::from_secs(20), RungOutcome::Alive)],
        1,
    )
    .unwrap_err();
    assert_eq!(lt_err.reason, UnknownReason::NonMonotone);

    // Filtering: proven t2 Ok(()) wins even if t3 got 420 RefusedChangeRequest.
    let fv = filtering_verdict(Probe::Ok(()), Probe::RefusedChangeRequest, Control::NotRun, Control::NotRun);
    assert!(matches!(fv, Verdict::Known { value: FilteringBehavior::EndpointIndependent, .. }));

    // Multi-server mapping: primary_mapped != obs[0].1 (arrival order) must not cause false UnstableMapping.
    let obs = vec![
        ("203.0.113.2:3478".parse().unwrap(), "198.51.100.10:50002".parse().unwrap()),
        ("203.0.113.1:3478".parse().unwrap(), "198.51.100.10:50001".parse().unwrap()),
    ];
    let mv = mapping_verdict_multi(
        &obs,
        Some("198.51.100.10:50001".parse().unwrap()),
        Some("198.51.100.10:50001".parse().unwrap()),
        "192.168.1.10:40000".parse().unwrap(),
        None,
    );
    assert!(matches!(mv, Verdict::Unknown(u) if u.reason == UnknownReason::NoT3));
}

#[test]
fn filter_socket_falls_back_from_plain_to_rfc5780_server() {
    let client_ip = Ipv4Addr::new(192, 168, 1, 10);
    let wan_ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10));
    let plain_s: SocketAddr = "203.0.113.1:3478".parse().unwrap();
    let rfc_pp: SocketAddr = "203.0.113.10:3478".parse().unwrap();
    let rfc_aa: SocketAddr = "203.0.113.11:3479".parse().unwrap();

    let mut w = SimWorld::new(
        client_ip,
        vec![NatSpec::home_eim_apdf(wan_ip)],
        vec![SimStunServer::plain("plain", plain_s), SimStunServer::rfc5780("rfc", rfc_pp, rfc_aa)],
        55,
    );
    // Caller did NOT set rfc5780 hint on server 1: SOCK_F must still fall back and classify APDF!
    let mut cfg = Settings::for_profile(
        Profile::Standard,
        vec![ServerEndpoint::new("plain", plain_s), ServerEndpoint::new("rfc", rfc_pp)],
    )
    .with_addr_policy(AddrPolicy::Lab);
    cfg.run_nat64 = false;
    cfg.run_portmap = false;
    cfg.run_ipv6 = false;
    let (rep, _) = drive_sim(cfg, &mut w, 55).unwrap();
    let v4 = rep.v4.unwrap();
    assert_eq!(v4.mapping.value(), Some(&MappingBehavior::EndpointIndependent));
    assert_eq!(v4.filtering.value(), Some(&FilteringBehavior::AddressAndPortDependent));
}

//! Real-socket loopback tests for the async `Prober`, `SystemUdpFactory`, gateway stand-in
//! and Linux `/proc/net/route` parser. All binds use port 0 and start with `need_loopback!()`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use pod_natprobe::report::{Connectivity, MappingBehavior, PmpState, Profile, UnknownReason};
use pod_natprobe::run::system::{parse_proc_net_route, udp_route_source_v4};
use pod_natprobe::seams::async_seams::Netenv;
use pod_natprobe::wire::AddrPolicy;
use pod_natprobe::wire::pmp::{OP_EXTERNAL_ADDR, PMP_VERSION, RESPONSE_BIT};
use pod_natprobe::{Gateway, LocalEnv, NatError, Obs, Prober, Seams, ServerEndpoint, Settings};
use pod_turn::stun::{BINDING_REQUEST, binding_success_nat, decode};
use tokio::net::UdpSocket;

struct FixedNetenv(LocalEnv);
impl Netenv for FixedNetenv {
    fn snapshot(&self) -> LocalEnv {
        self.0.clone()
    }
}

async fn spawn_plain_stun() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let sock = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await.unwrap();
    let addr = sock.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        loop {
            let Ok((n, from)) = sock.recv_from(&mut buf).await else { break };
            if let Ok(msg) = decode(&buf[..n])
                && msg.kind == BINDING_REQUEST
            {
                let resp = binding_success_nat(&msg.id, Some(from), Some(from), None, Some(addr), None);
                let _ = sock.send_to(&resp, from).await;
            }
        }
    });
    (addr, handle)
}

async fn spawn_pmp_gateway(wan_ip: Ipv4Addr) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let sock = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await.unwrap();
    let addr = sock.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let mut buf = [0u8; 512];
        loop {
            let Ok((n, from)) = sock.recv_from(&mut buf).await else { break };
            if n == 2 && buf[0] == PMP_VERSION && buf[1] == OP_EXTERNAL_ADDR {
                let mut r = [0u8; 12];
                r[0] = PMP_VERSION;
                r[1] = RESPONSE_BIT | OP_EXTERNAL_ADDR;
                r[4..8].copy_from_slice(&42u32.to_be_bytes());
                r[8..12].copy_from_slice(&wan_ip.octets());
                let _ = sock.send_to(&r, from).await;
            }
        }
    });
    (addr, handle)
}

#[tokio::test]
async fn async_prober_loopback_direct_and_pmp_gateway() {
    pod_testkit::need_loopback!();
    let (s1, h1) = spawn_plain_stun().await;
    let (s2, h2) = spawn_plain_stun().await;
    let (gw_pmp, h_gw) = spawn_pmp_gateway(Ipv4Addr::new(198, 51, 100, 77)).await;

    // Also test zero-packet route_source against loopback.
    let src = udp_route_source_v4(s1).expect("route source to loopback");
    assert_eq!(src, Ipv4Addr::LOCALHOST);

    let env = LocalEnv {
        route_source_v4: Some(Ipv4Addr::LOCALHOST),
        route_iface_v4: Some("lo".to_string()),
        route_source_v6: None,
        v6_stack_available: false,
        v6_no_route: true,
        v6_wkp_routable: None,
        local_ips: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
        gateway_v4: None,
    };
    let mut settings =
        Settings::for_profile(Profile::Fast, vec![ServerEndpoint::new("s1", s1), ServerEndpoint::new("s2", s2)])
            .with_addr_policy(AddrPolicy::Lab);
    settings.run_nat64 = false;
    settings.run_ipv6 = false;
    settings.run_portmap = false;

    let mut seams = Seams::system();
    seams.netenv = Arc::new(FixedNetenv(env));

    let prober = Prober::new();
    let report = prober.run(settings, seams).await.expect("loopback probe ok");
    let v4 = report.v4.as_ref().expect("v4 report");
    assert_eq!(v4.connectivity.value(), Some(&Connectivity::DirectPublic));
    assert_eq!(v4.mapping.value(), Some(&MappingBehavior::EndpointIndependent));
    assert!(matches!(v4.filtering, Obs::Unknown(ref u) if u.reason == UnknownReason::NoOtherAddress));

    // Verify NAT-PMP probe over real UDP socket using injected non-5351 gateway port.
    let env_pmp = LocalEnv {
        route_source_v4: Some(Ipv4Addr::new(10, 0, 0, 2)),
        route_iface_v4: Some("eth0".to_string()),
        route_source_v6: None,
        v6_stack_available: false,
        v6_no_route: true,
        v6_wkp_routable: None,
        local_ips: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
        gateway_v4: Some(Gateway {
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            pmp_port: gw_pmp.port(),
            ssdp_port: gw_pmp.port(),
            ssdp_multicast: gw_pmp,
            is_p2p: false,
        }),
    };
    let mut settings2 =
        Settings::for_profile(Profile::Fast, vec![ServerEndpoint::new("s1", s1)]).with_addr_policy(AddrPolicy::Lab);
    settings2.run_nat64 = false;
    settings2.run_ipv6 = false;
    settings2.run_portmap = true;

    // Bind SOCK_P to 127.0.0.1 instead of 10.0.0.2 by wrapping the factory.
    struct LoopbackUdp;
    impl pod_natprobe::seams::async_seams::UdpFactory for LoopbackUdp {
        fn bind<'a>(
            &'a self,
            spec: pod_natprobe::BindSpec,
        ) -> pod_core::BoxFuture<'a, Result<pod_natprobe::seams::async_seams::Bound, pod_core::Failure>> {
            Box::pin(async move {
                let _ = spec;
                let dg = pod_core::SystemDatagrams::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                    .await
                    .map_err(|e| pod_core::Failure::io(pod_core::Stage::Connect, "the connection", e))?;
                let local = dg.socket().local_addr().unwrap();
                Ok(pod_natprobe::seams::async_seams::Bound { socket: Arc::new(dg), local })
            })
        }
    }

    let mut seams2 = Seams::system();
    seams2.udp = Arc::new(LoopbackUdp);
    seams2.netenv = Arc::new(FixedNetenv(env_pmp));
    let report2 = prober.run(settings2, seams2).await.expect("pmp loopback probe ok");
    assert_eq!(report2.portmap.nat_pmp.value(), Some(&PmpState::Present));
    assert_eq!(
        report2.portmap.external_v4,
        vec![(pod_natprobe::ExternalIpSource::NatPmp, Ipv4Addr::new(198, 51, 100, 77))]
    );

    h1.abort();
    h2.abort();
    h_gw.abort();
}

#[test]
fn proc_net_route_parser_extracts_default_gateway() {
    // 0101A8C0 in little-endian hex = 192.168.1.1.
    let sample = "\
Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT
eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0
eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
";
    let (gw, iface) = parse_proc_net_route(sample);
    assert_eq!(iface.as_deref(), Some("eth0"));
    assert_eq!(gw.map(|g| g.ip), Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    let _ = NatError::Busy;
}

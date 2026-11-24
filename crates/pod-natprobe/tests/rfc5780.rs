//! The probe on real sockets against a stand-in RFC 5780 server with two addresses on the
//! loopback (127.0.0.1 and 127.0.0.2). Each bind is on port 0, or on the port that the
//! system gave to the first address. A host with no 127.0.0.2 skips the test.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use pod_natprobe::report::{Connectivity, FilteringBehavior, MappingBehavior, Profile};
use pod_natprobe::seams::async_seams::Netenv;
use pod_natprobe::sim::SimStunServer;
use pod_natprobe::wire::AddrPolicy;
use pod_natprobe::{LocalEnv, Prober, Seams, ServerEndpoint, Settings};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

struct FixedNetenv(LocalEnv);
impl Netenv for FixedNetenv {
    fn snapshot(&self) -> LocalEnv {
        self.0.clone()
    }
}

const PRIMARY: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const ALTERNATE: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));

/// Four sockets PP, PA, AP and AA: the ports of the primary address, again on the alternate
/// address. `None` when the host has no 127.0.0.2.
async fn bind_square() -> Option<Vec<Arc<UdpSocket>>> {
    for _ in 0..16 {
        let pp = UdpSocket::bind(SocketAddr::new(PRIMARY, 0)).await.ok()?;
        let pa = UdpSocket::bind(SocketAddr::new(PRIMARY, 0)).await.ok()?;
        let (p1, p2) = (pp.local_addr().ok()?.port(), pa.local_addr().ok()?.port());
        let ap = UdpSocket::bind(SocketAddr::new(ALTERNATE, p1)).await;
        let aa = UdpSocket::bind(SocketAddr::new(ALTERNATE, p2)).await;
        match (ap, aa) {
            (Ok(ap), Ok(aa)) => return Some([pp, pa, ap, aa].into_iter().map(Arc::new).collect()),
            // Another program holds one of the two ports there: try two new ports.
            (Err(e), _) | (_, Err(e)) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
            _ => return None,
        }
    }
    None
}

/// Serve the stand-in: each socket answers with the logic of the simulator's RFC 5780 server,
/// and the reply leaves from the socket that the server chose (CHANGE-REQUEST).
fn serve(square: Vec<Arc<UdpSocket>>, server: SimStunServer) -> Vec<JoinHandle<()>> {
    let mut tasks = Vec::new();
    for sock in &square {
        let (sock, square, server) = (Arc::clone(sock), square.clone(), server.clone());
        tasks.push(tokio::spawn(async move {
            let Ok(here) = sock.local_addr() else { return };
            let mut buf = [0u8; 1500];
            while let Ok((n, from)) = sock.recv_from(&mut buf).await {
                let Some((src, dst, body)) = server.handle(here, from, &buf[..n]) else { continue };
                for out in &square {
                    if out.local_addr().ok() == Some(src) {
                        let _ = out.send_to(&body, dst).await;
                    }
                }
            }
        }));
    }
    tasks
}

#[tokio::test]
async fn rfc5780_standin_on_two_loopback_addresses() {
    pod_testkit::need_loopback!();
    let Some(square) = bind_square().await else {
        eprintln!("skip: this host has no 127.0.0.2");
        return;
    };
    let pp = square[0].local_addr().expect("pp");
    let aa = square[3].local_addr().expect("aa");
    let tasks = serve(square, SimStunServer::rfc5780("standin", pp, aa));

    let env = LocalEnv {
        route_source_v4: Some(Ipv4Addr::LOCALHOST),
        route_iface_v4: Some("lo".to_string()),
        local_ips: vec![PRIMARY],
        v6_no_route: true,
        ..LocalEnv::default()
    };
    let mut settings = Settings::for_profile(Profile::Standard, vec![ServerEndpoint::rfc5780("standin", pp)])
        .with_addr_policy(AddrPolicy::Lab);
    settings.run_nat64 = false;
    settings.run_ipv6 = false;
    settings.run_portmap = false;
    settings.run_upnp_http = false;
    let mut seams = Seams::system();
    seams.netenv = Arc::new(FixedNetenv(env));

    let report = Prober::new().run(settings, seams).await.expect("loopback probe runs");
    for task in tasks {
        task.abort();
    }
    let v4 = report.v4.as_ref().expect("v4 report");
    assert_eq!(v4.connectivity.value(), Some(&Connectivity::DirectPublic), "{report}");
    assert_eq!(v4.mapping.value(), Some(&MappingBehavior::EndpointIndependent), "{report}");
    // The reply of CHANGE-REQUEST 0x06 came from 127.0.0.2 on the other port: no filter.
    assert_eq!(v4.filtering.value(), Some(&FilteringBehavior::EndpointIndependent), "{report}");
    assert!(report.pending.is_empty(), "each question has an answer: {:?}", report.pending);
}

//! System UDP socket factory, zero-packet local environment probe and `pod-services` server loader.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::Arc;

use pod_core::{BoxFuture, Failure, Stage, SystemDatagrams};

use crate::report::Family;
use crate::seams::async_seams::{Bound, Netenv, UdpFactory};
use crate::seams::{BindSpec, Gateway, LocalEnv};
use crate::settings::ServerEndpoint;
use crate::wire::nat64::WKP_PROBE_TARGET;

/// Public IPv4 target used ONLY for zero-packet UDP `connect()` route-source discovery.
const ROUTE_TARGET_V4: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 53);
/// Public IPv6 target used ONLY for zero-packet UDP `connect()` route-source discovery.
const ROUTE_TARGET_V6: SocketAddr =
    SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111)), 53);

/// System UDP factory creating ephemeral OS UDP sockets wrapped in [`SystemDatagrams`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemUdpFactory;

impl UdpFactory for SystemUdpFactory {
    fn bind<'a>(&'a self, spec: BindSpec) -> BoxFuture<'a, Result<Bound, Failure>> {
        Box::pin(async move {
            let ip = spec.local_ip.unwrap_or(match spec.family {
                Family::V4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                Family::V6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            });
            let addr = SocketAddr::new(ip, spec.local_port.unwrap_or(0));
            let dg = SystemDatagrams::bind(addr).await.map_err(|e| Failure::io(Stage::Connect, e))?;
            let local = dg.socket().local_addr().map_err(|e| Failure::io(Stage::Connect, e))?;
            Ok(Bound { socket: Arc::new(dg), local })
        })
    }
}

/// Zero-packet local network environment reader using UDP `connect()` and (on Linux)
/// `/proc/net/route`. Sends zero network packets.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemNetenv;

impl Netenv for SystemNetenv {
    fn snapshot(&self) -> LocalEnv {
        let route_source_v4 = udp_route_source_v4(ROUTE_TARGET_V4).ok();
        let v6_stack_available = UdpSocket::bind(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 0)).is_ok();
        let (route_source_v6, v6_no_route) = if v6_stack_available {
            match udp_route_source_v6(ROUTE_TARGET_V6) {
                Ok(ip) => (Some(ip), false),
                Err(e) if is_no_route(&e) => (None, true),
                Err(_) => (None, false),
            }
        } else {
            (None, false)
        };
        let v6_wkp_routable = if v6_stack_available {
            Some(udp_route_source_v6(SocketAddr::new(IpAddr::V6(WKP_PROBE_TARGET), 53)).is_ok())
        } else {
            None
        };
        let mut local_ips = Vec::new();
        if let Some(v4) = route_source_v4 {
            local_ips.push(IpAddr::V4(v4));
        }
        if let Some(v6) = route_source_v6 {
            local_ips.push(IpAddr::V6(v6));
        }
        let (gateway_v4, route_iface_v4) = read_default_gateway_v4();
        LocalEnv {
            route_source_v4,
            route_iface_v4,
            route_source_v6,
            v6_stack_available,
            v6_no_route,
            v6_wkp_routable,
            local_ips,
            gateway_v4,
        }
    }
}

/// Discover the kernel-selected local IPv4 source address for `dest` without sending any packet.
pub fn udp_route_source_v4(dest: SocketAddr) -> std::io::Result<Ipv4Addr> {
    let sock = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))?;
    sock.connect(dest)?;
    match sock.local_addr()?.ip() {
        IpAddr::V4(v4) => Ok(v4),
        IpAddr::V6(_) => Err(std::io::Error::new(std::io::ErrorKind::AddrNotAvailable, "non-v4 address")),
    }
}

/// Discover the kernel-selected local IPv6 source address for `dest` without sending any packet.
pub fn udp_route_source_v6(dest: SocketAddr) -> std::io::Result<Ipv6Addr> {
    let sock = UdpSocket::bind(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0))?;
    sock.connect(dest)?;
    match sock.local_addr()?.ip() {
        IpAddr::V6(v6) => Ok(v6),
        IpAddr::V4(_) => Err(std::io::Error::new(std::io::ErrorKind::AddrNotAvailable, "non-v6 address")),
    }
}

fn is_no_route(e: &std::io::Error) -> bool {
    matches!(e.kind(), std::io::ErrorKind::NetworkUnreachable | std::io::ErrorKind::HostUnreachable)
        || matches!(e.raw_os_error(), Some(101 | 51 | 65 | 10051 | 10065))
}

/// Parse a Linux `/proc/net/route` table to locate the active IPv4 default gateway with lowest
/// metric and its egress interface.
pub fn parse_proc_net_route(contents: &str) -> (Option<Gateway>, Option<String>) {
    const RTF_UP: u32 = 0x0001;
    const RTF_GATEWAY: u32 = 0x0002;
    let mut best: Option<(u32, Option<Gateway>, String)> = None;
    for line in contents.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 8 {
            continue;
        }
        let iface = cols[0];
        let dest_hex = cols[1];
        let gw_hex = cols[2];
        let flags = u32::from_str_radix(cols[3], 16).unwrap_or(0);
        let metric = cols[6].parse::<u32>().unwrap_or(u32::MAX);
        let mask_hex = cols[7];
        if (flags & RTF_UP) == 0 || dest_hex != "00000000" || mask_hex != "00000000" {
            continue;
        }
        if let Ok(raw) = u32::from_str_radix(gw_hex, 16) {
            let ip = Ipv4Addr::from(raw.to_ne_bytes());
            let gw = if !ip.is_unspecified() {
                let mut g = Gateway::standard(IpAddr::V4(ip));
                g.is_p2p = (flags & RTF_GATEWAY) == 0;
                Some(g)
            } else {
                None
            };
            if best.as_ref().is_none_or(|(m, _, _)| metric < *m) {
                best = Some((metric, gw, iface.to_string()));
            }
        }
    }
    match best {
        Some((_, gw, iface)) => (gw, Some(iface)),
        None => (None, None),
    }
}

#[cfg(target_os = "linux")]
fn read_default_gateway_v4() -> (Option<Gateway>, Option<String>) {
    match std::fs::read_to_string("/proc/net/route") {
        Ok(text) => parse_proc_net_route(&text),
        Err(_) => (None, None),
    }
}

#[cfg(not(target_os = "linux"))]
fn read_default_gateway_v4() -> (Option<Gateway>, Option<String>) {
    (None, None)
}

/// Resolve the built-in STUN servers listed in `pod-services` (`kind == "stun"`) concurrently
/// (with a 3 s timeout per endpoint), preserving catalog rank order.
pub async fn resolve_pod_services_stun() -> (Vec<ServerEndpoint>, Vec<ServerEndpoint>) {
    let mut jobs = Vec::new();
    for svc in pod_services::of_kind("stun") {
        let is_rfc5780 = svc.fact("rfc5780").and_then(|f| f.value().as_bool()) == Some(true);
        for ep in svc.endpoints() {
            let port = ep.port();
            if port == 0 {
                continue;
            }
            let idx = jobs.len();
            let id = svc.id().to_string();
            let target = format!("{}:{port}", ep.host());
            jobs.push((idx, id, target, is_rfc5780));
        }
    }
    let mut set = tokio::task::JoinSet::new();
    for (idx, id, target, is_rfc5780) in jobs {
        set.spawn(async move {
            let found = tokio::time::timeout(std::time::Duration::from_secs(3), tokio::net::lookup_host(target)).await;
            let mut ep_v4 = None;
            let mut ep_v6 = None;
            if let Ok(Ok(addrs)) = found {
                for addr in addrs {
                    let make = |a| {
                        if is_rfc5780 {
                            ServerEndpoint::rfc5780(id.clone(), a)
                        } else {
                            ServerEndpoint::new(id.clone(), a)
                        }
                    };
                    match addr {
                        SocketAddr::V4(_) if ep_v4.is_none() => ep_v4 = Some(make(addr)),
                        SocketAddr::V6(_) if ep_v6.is_none() => ep_v6 = Some(make(addr)),
                        _ => {}
                    }
                }
            }
            (idx, ep_v4, ep_v6)
        });
    }
    let mut results = Vec::new();
    while let Some(Ok(item)) = set.join_next().await {
        results.push(item);
    }
    results.sort_by_key(|(idx, _, _)| *idx);
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for (_, e4, e6) in results {
        if let Some(e) = e4 {
            v4.push(e);
        }
        if let Some(e) = e6 {
            v6.push(e);
        }
    }
    (v4, v6)
}

//! Caller-supplied I/O and local environment seams (design rules 5, 6, 7).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::report::Family;
use crate::wire::pmp::PMP_PORT;
use crate::wire::ssdp::{SSDP_MULTICAST, SSDP_PORT};

/// Gateway details for the local LAN portmapper probe. Ports are injectable so loopback tests
/// never bind fixed privileged or colliding ports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gateway {
    pub ip: IpAddr,
    pub pmp_port: u16,
    pub ssdp_port: u16,
    pub ssdp_multicast: SocketAddr,
    /// True for a `/32` point-to-point gateway where LAN port mapping does not apply.
    pub is_p2p: bool,
}

impl Gateway {
    /// Construct a gateway with standard ports (`5351` for NAT-PMP/PCP, `1900` for SSDP).
    pub fn standard(ip: IpAddr) -> Self {
        Self { ip, pmp_port: PMP_PORT, ssdp_port: SSDP_PORT, ssdp_multicast: SSDP_MULTICAST, is_p2p: false }
    }
}

/// Zero-packet local host network snapshot collected before the first probe datagram is sent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalEnv {
    /// Egress source IPv4 selected by the kernel for global destinations (via UDP `connect()`).
    pub route_source_v4: Option<Ipv4Addr>,
    /// Egress interface name for `route_source_v4`, if known (for tunnel interface filtering).
    pub route_iface_v4: Option<String>,
    /// Egress source IPv6 selected by the kernel for global destinations.
    pub route_source_v6: Option<Ipv6Addr>,
    /// True when the OS allows binding an IPv6 UDP socket (`[::1]:0`).
    pub v6_stack_available: bool,
    /// True when UDP `connect()` to a global IPv6 address returned `ENETUNREACH`.
    pub v6_no_route: bool,
    /// Result of zero-packet UDP `connect()` to `64:ff9b::808:808` (Well-Known NAT64 prefix).
    pub v6_wkp_routable: Option<bool>,
    /// All known local interface IP addresses (used by hairpin guard G2).
    pub local_ips: Vec<IpAddr>,
    /// Default IPv4 gateway, if discovered.
    pub gateway_v4: Option<Gateway>,
}

/// Specification for binding one isolated probe UDP socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindSpec {
    pub family: Family,
    pub local_ip: Option<IpAddr>,
    pub local_port: Option<u16>,
}

/// Async UDP socket factory seam (feature `run`).
#[cfg(feature = "run")]
pub mod async_seams {
    use std::net::{Ipv6Addr, SocketAddr};
    use std::sync::Arc;

    use pod_core::{BoxFuture, Datagrams, Failure};

    use super::{BindSpec, LocalEnv};

    /// Bound UDP socket returned by [`UdpFactory::bind`].
    pub struct Bound {
        pub socket: Arc<dyn Datagrams>,
        pub local: SocketAddr,
    }

    /// Factory creating isolated UDP sockets for the probe runner.
    pub trait UdpFactory: Send + Sync {
        fn bind<'a>(&'a self, spec: BindSpec) -> BoxFuture<'a, Result<Bound, Failure>>;
    }

    /// Synchronous local network environment reader.
    pub trait Netenv: Send + Sync {
        fn snapshot(&self) -> LocalEnv;
    }

    /// Record-typed DNS64 resolver seam for RFC 7050 (`ipv4only.arpa`, CD=0).
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Dns64Lookup {
        pub aaaa: Vec<Ipv6Addr>,
        pub has_a: bool,
        pub resolver_is_doh: bool,
        pub failed: bool,
    }

    pub trait Nat64Resolver: Send + Sync {
        fn query_ipv4only<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, Dns64Lookup>;
    }

    /// Optional read-only HTTP client for UPnP IGD device description and `GetExternalIPAddress`.
    pub trait UpnpHttp: Send + Sync {
        fn get<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<String, String>>;
        fn soap_post<'a>(
            &'a self,
            url: &'a str,
            action: &'a str,
            body: &'a str,
        ) -> BoxFuture<'a, Result<String, String>>;
    }

    /// Optional non-UDP reachability corroboration (for example a TCP/443 dial via `pod_core::Dial`).
    pub trait OnlineCheck: Send + Sync {
        fn check<'a>(&'a self) -> BoxFuture<'a, bool>;
    }
}

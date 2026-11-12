//! The probes of `sand net check` (issue 178): what the guest (or the
//! world, for an inbound flow) measures, and where each probe goes.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr};

use crate::knobs::{Family, Knobs};
use crate::layout::{CGN_INSIDE_V4, DNAT_PORT, GUEST_PORT, Layout, WORLD_V4, nat64_address};

/// The world's TCP ports (`world.py`): 22, 25, 443, 7844, 8443 reply
/// `you=ADDR port=PORT`; 80 is HTTP; 3128 is the CONNECT proxy.
pub const WORLD_TCP: [u16; 7] = [22, 25, 80, 443, 3128, 7844, 8443];
pub const HTTP_PORT: u16 = 80;
pub const PROXY_PORT: u16 = 3128;
pub const TLS_PORT: u16 = 443;
pub const DNS_PORT: u16 = 53;
pub const ECHO_PORT: u16 = 9999;
/// The source port of the probe `srcport`.
pub const SRCPORT: u16 = 40001;
/// `GET /bulk` is 256 KiB.
pub const BULK_BYTES: u64 = 256 * 1024;
/// The datagrams of `udp-burst`.
pub const BURST: u32 = 100;
/// The tries of a probe of one datagram (udp, dns-udp, icmp) before it
/// says DROPPED.
pub const TRIES: u32 = 5;
/// The names that the world's DNS serves.
pub const NAME_BOTH: &str = "a.world.test";
pub const NAME_V4_ONLY: &str = "v4only.world.test";
pub const NAME_V6_ONLY: &str = "v6only.world.test";

/// One probe of a lab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Probe {
    /// A TCP connect to a port of the world.
    Tcp(Family, u16),
    /// A datagram to the world's UDP echo, on a connected socket (L2).
    Udp(Family),
    /// A DNS query over UDP to the world's address.
    DnsUdp(Family),
    /// `GET /` of the world's HTTP: the source address that it saw.
    Whoami(Family),
    /// A connect from source port 40001: the source port that it saw.
    Srcport(Family),
    /// A CONNECT through the world's proxy to the world's port 443.
    Proxy(Family),
    /// The best of three TCP connects to port 443, in milliseconds.
    Rtt(Family),
    /// `GET /bulk` (256 KiB), in milliseconds to the last byte.
    Bulk(Family),
    /// 100 datagrams to the UDP echo: how many come back.
    UdpBurst(Family),
    /// An echo request over a ping socket.
    Icmp(Family),
    /// A connect to the router's outside address, port 2222.
    Hairpin(Family),
    /// `a.world.test` through the guest's resolver.
    Resolve,
    /// The AAAA of `v4only.world.test`: an address in the NAT64 prefix.
    Dns64,
    /// A TCP connect to the world's IPv4 port through the NAT64 prefix.
    Nat64Tcp(u16),
    /// `GET /` through the NAT64 prefix.
    Nat64Whoami,
    /// cgnat: `GET /` of the carrier's own address: the router's outside
    /// address as the carrier sees it.
    IspWhoami,
    /// From the world: a connect to the guest's port 2200.
    Inbound(Family),
    /// From the world: a connect to the router's port 2222.
    InboundDnat(Family),
}

/// Where a probe runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Guest,
    World,
}

/// The far end of a probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub side: Side,
    pub addr: IpAddr,
    pub port: Option<u16>,
}

impl Probe {
    /// The probe's name in a report: `tcp4-443`, `dns-udp6`, `nat64-80`.
    pub fn name(self) -> String {
        let f = |family: Family| family.digit();
        match self {
            Probe::Tcp(family, port) => format!("tcp{}-{port}", f(family)),
            Probe::Udp(family) => format!("udp{}", f(family)),
            Probe::DnsUdp(family) => format!("dns-udp{}", f(family)),
            Probe::Whoami(family) => format!("whoami{}", f(family)),
            Probe::Srcport(family) => format!("srcport{}", f(family)),
            Probe::Proxy(family) => format!("proxy{}", f(family)),
            Probe::Rtt(family) => format!("rtt{}", f(family)),
            Probe::Bulk(family) => format!("bulk{}", f(family)),
            Probe::UdpBurst(family) => format!("udp-burst{}", f(family)),
            Probe::Icmp(family) => format!("icmp{}", f(family)),
            Probe::Hairpin(family) => format!("hairpin{}", f(family)),
            Probe::Resolve => "resolve".into(),
            Probe::Dns64 => "dns64".into(),
            Probe::Nat64Tcp(port) => format!("nat64-{port}"),
            Probe::Nat64Whoami => "nat64-whoami".into(),
            Probe::IspWhoami => "isp-whoami".into(),
            Probe::Inbound(family) => format!("inbound{}", f(family)),
            Probe::InboundDnat(family) => format!("inbound-dnat{}", f(family)),
        }
    }

    /// The probe of a name that `Probe::name` gives.
    pub fn from_name(name: &str) -> Option<Probe> {
        match name {
            "resolve" => return Some(Probe::Resolve),
            "dns64" => return Some(Probe::Dns64),
            "nat64-whoami" => return Some(Probe::Nat64Whoami),
            "isp-whoami" => return Some(Probe::IspWhoami),
            _ => {}
        }
        let port = |text: &str| text.parse::<u16>().ok().filter(|p| *p > 0 && !text.starts_with('0'));
        if let Some(p) = name.strip_prefix("nat64-") {
            return port(p).map(Probe::Nat64Tcp);
        }
        if let Some(p) = name.strip_prefix("tcp4-") {
            return port(p).map(|p| Probe::Tcp(Family::V4, p));
        }
        if let Some(p) = name.strip_prefix("tcp6-") {
            return port(p).map(|p| Probe::Tcp(Family::V6, p));
        }
        let (stem, family) = match name.as_bytes().last()? {
            b'4' => (&name[..name.len() - 1], Family::V4),
            b'6' => (&name[..name.len() - 1], Family::V6),
            _ => return None,
        };
        let kind: fn(Family) -> Probe = match stem {
            "udp" => Probe::Udp,
            "dns-udp" => Probe::DnsUdp,
            "whoami" => Probe::Whoami,
            "srcport" => Probe::Srcport,
            "proxy" => Probe::Proxy,
            "rtt" => Probe::Rtt,
            "bulk" => Probe::Bulk,
            "udp-burst" => Probe::UdpBurst,
            "icmp" => Probe::Icmp,
            "hairpin" => Probe::Hairpin,
            "inbound" => Probe::Inbound,
            "inbound-dnat" => Probe::InboundDnat,
            _ => return None,
        };
        Some(kind(family))
    }

    /// The far end of the probe in `layout`. A probe of the resolver goes
    /// to the first nameserver of the guest.
    pub fn target(self, knobs: &Knobs, layout: &Layout) -> Target {
        let guest = |addr, port| Target { side: Side::Guest, addr, port: Some(port) };
        let world = Layout::world;
        let nat64 = |port| guest(IpAddr::V6(nat64_address(WORLD_V4)), port);
        match self {
            Probe::Tcp(family, port) => guest(world(family), port),
            Probe::Udp(family) | Probe::UdpBurst(family) => guest(world(family), ECHO_PORT),
            Probe::DnsUdp(family) => guest(world(family), DNS_PORT),
            Probe::Whoami(family) | Probe::Srcport(family) | Probe::Bulk(family) => guest(world(family), HTTP_PORT),
            Probe::Proxy(family) => guest(world(family), PROXY_PORT),
            Probe::Rtt(family) => guest(world(family), TLS_PORT),
            Probe::Icmp(family) => Target { side: Side::Guest, addr: world(family), port: None },
            Probe::Hairpin(family) => guest(layout.router_out(family), DNAT_PORT),
            Probe::Resolve | Probe::Dns64 => {
                let family = if knobs.stack.has(Family::V4) { Family::V4 } else { Family::V6 };
                guest(world(family), DNS_PORT)
            }
            Probe::Nat64Tcp(port) => nat64(port),
            Probe::Nat64Whoami => nat64(HTTP_PORT),
            Probe::IspWhoami => guest(IpAddr::V4(CGN_INSIDE_V4), HTTP_PORT),
            Probe::Inbound(family) => Target { side: Side::World, addr: Layout::guest(family), port: Some(GUEST_PORT) },
            Probe::InboundDnat(family) => {
                Target { side: Side::World, addr: layout.public(family), port: Some(DNAT_PORT) }
            }
        }
    }
}

impl fmt::Display for Probe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name())
    }
}

/// The probes of a lab: each probe of the guest on both families (a family
/// that the guest lacks gives DENY), the resolver, NAT64 with `--nat64`,
/// `isp-whoami` with `--cgnat`, and the inbound probes of the world on the
/// families of the guest.
pub fn probes(knobs: &Knobs) -> Vec<Probe> {
    let mut all = Vec::new();
    for family in Family::BOTH {
        all.extend(WORLD_TCP.map(|port| Probe::Tcp(family, port)));
        all.extend([
            Probe::Udp(family),
            Probe::DnsUdp(family),
            Probe::Whoami(family),
            Probe::Srcport(family),
            Probe::Proxy(family),
            Probe::Rtt(family),
            Probe::Bulk(family),
            Probe::UdpBurst(family),
            Probe::Icmp(family),
            Probe::Hairpin(family),
        ]);
    }
    all.extend([Probe::Resolve, Probe::Dns64]);
    if knobs.nat64 {
        all.extend([Probe::Nat64Tcp(HTTP_PORT), Probe::Nat64Tcp(TLS_PORT), Probe::Nat64Whoami]);
    }
    if knobs.cgnat {
        all.push(Probe::IspWhoami);
    }
    for family in Family::BOTH.into_iter().filter(|f| knobs.stack.has(*f)) {
        all.extend([Probe::Inbound(family), Probe::InboundDnat(family)]);
    }
    all
}

/// The address that the world's DNS64 gives for `v4only.world.test`.
pub fn dns64_answer() -> IpAddr {
    IpAddr::V6(nat64_address(WORLD_V4))
}

/// The carrier's address that `isp-whoami` asks.
pub fn isp_address() -> Ipv4Addr {
    CGN_INSIDE_V4
}

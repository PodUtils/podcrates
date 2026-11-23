//! IP address classification, canonicalisation and tunnel interface filtering.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::report::LocalV6Class;

/// Policy governing documentation and benchmark ranges (`192.0.2/24`, `198.18/15`,
/// `198.51.100/24`, `203.0.113/24`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum AddrPolicy {
    /// Production default: documentation and benchmark ranges are non-global.
    #[default]
    Internet,
    /// Isolated network lab: documentation and benchmark ranges count as global WAN space.
    Lab,
}

/// High-level RFC address classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddrClass {
    Global,
    Private,
    SharedCgnat,
    DsLite,
    LinkLocal,
    Ula,
    Special,
    Documentation,
    Nat64Synth,
    Teredo,
    SixToFour,
}

/// Canonicalise IPv4-mapped IPv6 (`::ffff:a.b.c.d`) to plain IPv4.
pub fn canonical_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        IpAddr::V4(v4) => IpAddr::V4(v4),
    }
}

/// Canonicalise a socket address (IPv4-mapped IPv6 becomes IPv4).
pub fn canonical_addr(addr: SocketAddr) -> SocketAddr {
    SocketAddr::new(canonical_ip(addr.ip()), addr.port())
}

/// True when `name` matches a known VPN or tunnel interface prefix (`tailscale`, `tun`, `tap`,
/// `wg`, `ipsec`, `utun`). Shared CGNAT space (`100.64/10`) on these interfaces does not prove CGNAT.
pub fn is_tunnel_iface(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["tailscale", "tun", "tap", "wg", "ipsec", "utun", "zt", "nebula", "nordlynx", "mullvad"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// Classify `ip` under `policy`.
pub fn classify_ip(ip: IpAddr, policy: AddrPolicy) -> AddrClass {
    match canonical_ip(ip) {
        IpAddr::V4(v4) => classify_v4(v4, policy),
        IpAddr::V6(v6) => classify_v6(v6, policy),
    }
}

/// True when `ip` is routable global unicast under `policy`.
pub fn is_global(ip: IpAddr, policy: AddrPolicy) -> bool {
    matches!(classify_ip(ip, policy), AddrClass::Global)
}

/// True when `ip` is an RFC 1918 private IPv4 address.
pub fn is_rfc1918(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168)
}

/// True when `ip` lies in RFC 6598 shared CGNAT space (`100.64.0.0/10`).
pub fn is_shared_cgnat(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 100 && (b & 0xc0) == 64
}

/// True when `ip` lies in RFC 6333 DS-Lite B4/AFTR range (`192.0.0.0/29`), excluding the RFC 7050
/// markers `.170` and `.171` (already outside `/29`, checked explicitly for clarity).
pub fn is_dslite_range(ip: Ipv4Addr) -> bool {
    let [a, b, c, d] = ip.octets();
    a == 192 && b == 0 && c == 0 && d <= 7 && d != 170 && d != 171
}

/// True when `ip` is in RFC 5737 documentation or RFC 2544 benchmark space.
pub fn is_doc_or_bench_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    (a == 192 && b == 0 && c == 2)
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || (a == 198 && (b == 18 || b == 19))
}

fn classify_v4(ip: Ipv4Addr, policy: AddrPolicy) -> AddrClass {
    let [a, b, _, _] = ip.octets();
    if is_rfc1918(ip) {
        return AddrClass::Private;
    }
    if is_shared_cgnat(ip) {
        return AddrClass::SharedCgnat;
    }
    if is_dslite_range(ip) {
        return AddrClass::DsLite;
    }
    if a == 169 && b == 254 {
        return AddrClass::LinkLocal;
    }
    if a == 0 || a == 127 || a >= 224 {
        return AddrClass::Special;
    }
    if is_doc_or_bench_v4(ip) {
        return match policy {
            AddrPolicy::Internet => AddrClass::Documentation,
            AddrPolicy::Lab => AddrClass::Global,
        };
    }
    AddrClass::Global
}

fn classify_v6(ip: Ipv6Addr, policy: AddrPolicy) -> AddrClass {
    let segs = ip.segments();
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return AddrClass::Special;
    }
    if (segs[0] & 0xffc0) == 0xfe80 {
        return AddrClass::LinkLocal;
    }
    if (segs[0] & 0xfe00) == 0xfc00 {
        return AddrClass::Ula;
    }
    if segs[0] == 0x0064 && segs[1] == 0xff9b && segs[2..6] == [0, 0, 0, 0] {
        return AddrClass::Nat64Synth;
    }
    if segs[0] == 0x2001 && segs[1] == 0x0000 {
        return AddrClass::Teredo;
    }
    if segs[0] == 0x2002 {
        return AddrClass::SixToFour;
    }
    if segs[0] == 0x2001 && segs[1] == 0x0db8 {
        return match policy {
            AddrPolicy::Internet => AddrClass::Documentation,
            AddrPolicy::Lab => AddrClass::Global,
        };
    }
    AddrClass::Global
}

/// Classify a local IPv6 source address for [`crate::report::Ipv6Status`].
pub fn local_v6_class(ip: Ipv6Addr) -> LocalV6Class {
    match classify_v6(ip, AddrPolicy::Lab) {
        AddrClass::Ula => LocalV6Class::Ula,
        AddrClass::LinkLocal => LocalV6Class::LinkLocal,
        AddrClass::Teredo => LocalV6Class::Teredo,
        AddrClass::SixToFour => LocalV6Class::SixToFour,
        AddrClass::Nat64Synth => LocalV6Class::Nat64Synth,
        _ => LocalV6Class::Gua,
    }
}

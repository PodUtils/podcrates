//! The routes and the resolver of a TUN that takes the whole host
//! (podnet's `warp tun --default-route`, PodUtils/podnet issue 77): with
//! `CAP_NET_ADMIN`, the whole host goes through WARP.
//!
//! * Host routes through the gateway of the default route for each address
//!   that the MASQUE connection uses (the proxy, the front door, the
//!   relay), so that the connection does not go into the tunnel.
//! * Default routes on the TUN: `0.0.0.0/1` and `128.0.0.0/1`, and `::/1`
//!   and `8000::/1` (more specific than the default route, which stays).
//!   They go away while the session is down, so that traffic fails fast.
//! * DNS: `resolvectl` on the TUN when systemd-resolved runs, else the
//!   resolver file with the caller's nameservers (a record of the original
//!   is kept, `resolv`).
//!
//! The parse of the route tables and the plan of each address are pure and
//! run on each target. `Routes` (Linux) puts each change back when it is
//! dropped. The calls are ioctls (`SIOCADDRT`, `SIOCDELRT`): no hook script
//! and no shell runs as root.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::resolver::Resolver;

#[cfg(target_os = "linux")]
mod host;
#[cfg(unix)]
pub mod resolv;

#[cfg(target_os = "linux")]
pub use host::{Routes, Tables, set_dns};

/// The gateway and the device of the IPv4 default route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gateway {
    pub dev: String,
    pub gateway: Ipv4Addr,
}

/// The default route of `/proc/net/route` (the lowest metric).
pub fn parse_route_table(text: &str) -> Option<Gateway> {
    let mut best: Option<(u32, Gateway)> = None;
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 8 || f[1] != "00000000" || f[7] != "00000000" {
            continue;
        }
        let Ok(gw) = u32::from_str_radix(f[2], 16) else {
            continue;
        };
        let metric = f[6].parse().unwrap_or(u32::MAX);
        let gateway = Ipv4Addr::from(gw.to_le_bytes());
        if gateway.is_unspecified() {
            continue;
        }
        if best.as_ref().is_none_or(|(m, _)| metric < *m) {
            best = Some((metric, Gateway { dev: f[0].to_string(), gateway }));
        }
    }
    best.map(|(_, g)| g)
}

/// A route of `/proc/net/route` (an UP route; REJECT routes are skipped).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub dev: String,
    pub dest: Ipv4Addr,
    pub mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub metric: u32,
}

/// The UP routes of `/proc/net/route`.
pub fn parse_entries(text: &str) -> Vec<Entry> {
    let hex = |f: &str| u32::from_str_radix(f, 16).ok().map(|v| Ipv4Addr::from(v.to_le_bytes()));
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 8 {
                return None;
            }
            let flags = u32::from_str_radix(f[3], 16).ok()?;
            if flags & 0x1 == 0 || flags & 0x200 != 0 {
                return None;
            }
            Some(Entry {
                dev: f[0].to_string(),
                dest: hex(f[1])?,
                gateway: hex(f[2])?,
                metric: f[6].parse().unwrap_or(u32::MAX),
                mask: hex(f[7])?,
            })
        })
        .collect()
}

/// What an address of the MASQUE connection needs so that the default
/// routes of the TUN (`0/1` and `128/1`) do not capture it
/// (PodUtils/podnet issue 105).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exempt {
    /// Nothing: a loopback address, or a route more specific than /1 (an
    /// on-link subnet, for example) wins over the TUN.
    No,
    /// A host route through this device (and gateway).
    Via { dev: String, gateway: Option<Ipv4Addr> },
    /// No route at all: the address cannot be kept outside the tunnel.
    Unreachable,
}

/// The plan for `ip` with the routes `entries`, not counting the routes of
/// the TUN `tun`.
pub fn plan_exempt(ip: Ipv4Addr, entries: &[Entry], tun: &str) -> Exempt {
    if ip.is_loopback() {
        return Exempt::No;
    }
    let best = entries
        .iter()
        .filter(|e| e.dev != tun && u32::from(ip) & u32::from(e.mask) == u32::from(e.dest))
        .max_by_key(|e| (u32::from(e.mask).count_ones(), std::cmp::Reverse(e.metric)));
    match best {
        None => Exempt::Unreachable,
        Some(e) if u32::from(e.mask).count_ones() >= 2 => Exempt::No,
        Some(e) => Exempt::Via { dev: e.dev.clone(), gateway: (!e.gateway.is_unspecified()).then_some(e.gateway) },
    }
}

/// A route of `/proc/net/ipv6_route` (an UP route; REJECT routes are
/// skipped).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry6 {
    pub dev: String,
    pub dest: Ipv6Addr,
    pub prefix: u8,
    pub gateway: Ipv6Addr,
    pub metric: u32,
}

/// The UP routes of `/proc/net/ipv6_route`: dest, dest length, source,
/// source length, next hop, metric, refcount, use, flags, device.
pub fn parse_entries6(text: &str) -> Vec<Entry6> {
    let addr = |f: &str| u128::from_str_radix(f, 16).ok().filter(|_| f.len() == 32).map(Ipv6Addr::from);
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 10 {
                return None;
            }
            let flags = u32::from_str_radix(f[8], 16).ok()?;
            if flags & 0x1 == 0 || flags & 0x200 != 0 {
                return None;
            }
            Some(Entry6 {
                dest: addr(f[0])?,
                prefix: u8::from_str_radix(f[1], 16).ok()?,
                gateway: addr(f[4])?,
                metric: u32::from_str_radix(f[5], 16).unwrap_or(u32::MAX),
                dev: f[9].to_string(),
            })
        })
        .collect()
}

/// [`Exempt`] for an IPv6 address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exempt6 {
    No,
    Via { dev: String, gateway: Option<Ipv6Addr> },
    Unreachable,
}

/// What an IPv6 address of the MASQUE connection needs, as
/// [`plan_exempt`] (podnet's IPv6-only kit, 0001).
pub fn plan_exempt6(ip: Ipv6Addr, entries: &[Entry6], tun: &str) -> Exempt6 {
    if ip.is_loopback() {
        return Exempt6::No;
    }
    let bits = u128::from(ip);
    let best = entries
        .iter()
        .filter(|e| e.dev != tun && e.dev != "lo")
        .filter(|e| {
            let mask = if e.prefix == 0 { 0 } else { u128::MAX << (128 - u32::from(e.prefix.min(128))) };
            bits & mask == u128::from(e.dest) & mask
        })
        .max_by_key(|e| (e.prefix, std::cmp::Reverse(e.metric)));
    match best {
        None => Exempt6::Unreachable,
        Some(e) if e.prefix >= 2 => Exempt6::No,
        Some(e) => Exempt6::Via { dev: e.dev.clone(), gateway: (!e.gateway.is_unspecified()).then_some(e.gateway) },
    }
}

/// The nameservers of a TUN that takes the host: the UDP resolvers of
/// `list` on port 53 (at most 3; PodUtils/podnet issue 145), else
/// [`nameservers_of_service`]. A program of the host speaks plain DNS, so
/// a resolver over TLS is not one of them.
pub fn nameservers(list: &[Resolver]) -> Vec<IpAddr> {
    let set: Vec<IpAddr> = list
        .iter()
        .filter_map(|r| match r {
            Resolver::Udp(a) if a.port() == 53 => Some(a.ip()),
            _ => None,
        })
        .take(3)
        .collect();
    if set.is_empty() { nameservers_of_service() } else { set }
}

/// The plain DNS of 1.1.1.1 and 1.0.0.1: the endpoints of the role `udp` of
/// `cloudflare-dns` (`pod-services`).
pub fn nameservers_of_service() -> Vec<IpAddr> {
    let dns = pod_services::get("cloudflare-dns").expect("pod-services holds the service cloudflare-dns");
    dns.endpoints_of("udp").flat_map(|e| e.socket_addrs()).map(|a| a.ip()).collect()
}

/// The text of a resolver file with `marker` as its first line and
/// `servers`.
pub fn resolv_text(marker: &str, servers: &[IpAddr]) -> String {
    let mut text = format!("{marker}\n");
    for s in servers {
        text.push_str(&format!("nameserver {s}\n"));
    }
    text
}

/// The IPv4 netmask of `prefix`.
pub fn mask(prefix: u8) -> Ipv4Addr {
    if prefix == 0 { Ipv4Addr::UNSPECIFIED } else { Ipv4Addr::from(u32::MAX << (32 - u32::from(prefix.min(32)))) }
}

/// Add each item with `apply(item, true)`. When one fails, remove the
/// items that are in (`apply(item, false)`, in the reverse order) and give
/// the failure.
pub fn install_all<T, E>(items: &[T], mut apply: impl FnMut(&T, bool) -> Result<(), E>) -> Result<(), E> {
    for (i, item) in items.iter().enumerate() {
        if let Err(err) = apply(item, true) {
            for done in items[..i].iter().rev() {
                let _ = apply(done, false);
            }
            return Err(err);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

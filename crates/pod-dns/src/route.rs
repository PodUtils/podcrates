//! The family of this host's default route, and the order of the resolvers
//! by it (podnet): on a host with no IPv4 default route, each IPv4 resolver
//! fails before a resolver that works, so the IPv6 resolvers go first. The
//! routes are read only when the caller asks ([`has_default_route`]).

use crate::doh::Doh;

/// The file of the IPv4 routes of Linux.
pub const ROUTES_V4: &str = "/proc/net/route";

/// The file of the IPv6 routes of Linux.
pub const ROUTES_V6: &str = "/proc/net/ipv6_route";

/// True when this host has a default route of the family ([`ROUTES_V4`] for
/// IPv4, [`ROUTES_V6`] for IPv6). With no file, true for IPv4 and false for
/// IPv6.
pub fn has_default_route(ipv6: bool) -> bool {
    let path = if ipv6 { ROUTES_V6 } else { ROUTES_V4 };
    match std::fs::read_to_string(path) {
        Ok(text) => default_route_in(&text, ipv6),
        Err(_) => !ipv6,
    }
}

/// Whether the text of a route file of Linux holds a default route: in
/// `/proc/net/route` a destination and a mask of `00000000`; in
/// `/proc/net/ipv6_route` the destination `::/0` on an interface that is not
/// the loopback.
pub fn default_route_in(text: &str, ipv6: bool) -> bool {
    if ipv6 {
        return text.lines().any(|l| l.starts_with("00000000000000000000000000000000 00 ") && !l.ends_with(" lo"));
    }
    text.lines().skip(1).any(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.len() > 7 && f[1] == "00000000" && f[7] == "00000000"
    })
}

/// The resolvers of the family of this host first (`ipv4_route`: the host
/// has an IPv4 default route); the order in each family stays.
pub fn by_family(resolvers: &[Doh], ipv4_route: bool) -> Vec<Doh> {
    let (v6, v4): (Vec<Doh>, Vec<Doh>) = resolvers.iter().cloned().partition(|r| r.ip.is_ipv6());
    if ipv4_route { v4.into_iter().chain(v6).collect() } else { v6.into_iter().chain(v4).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// podnet's servers: Cloudflare's and Google's, IPv4 and IPv6.
    fn servers() -> Vec<Doh> {
        let services = ["cloudflare-doh", "google-doh"].map(|id| pod_services::get(id).expect(id));
        crate::doh::of_services(services, &["v4", "v6"])
    }

    // A host with no IPv4 route asks the IPv6 servers first; the order of
    // the operators stays in each family.
    #[test]
    fn the_family_of_the_host_goes_first() {
        let servers = servers();
        let v6_first = by_family(&servers, false);
        assert!(v6_first[0].ip.is_ipv6(), "{}", v6_first[0].ip);
        assert_eq!(v6_first[0].operator, "Cloudflare");
        assert!(v6_first.iter().take(4).all(|s| s.ip.is_ipv6()));
        let v4_first = by_family(&servers, true);
        assert_eq!(v4_first[0].ip.to_string(), "1.1.1.1");
        assert!(v4_first.iter().take(4).all(|s| s.ip.is_ipv4()));
        assert_eq!(v4_first.len(), servers.len());
    }

    /// The default routes of the files of Linux, and a file with none.
    #[test]
    fn a_default_route_is_read_from_the_text_of_the_routes() {
        let v4 = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
                  eth0\t00000000\t0102A8C0\t0003\t0\t0\t100\t00000000\n";
        assert!(default_route_in(v4, false));
        let none = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
                    eth0\t0002A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\n";
        assert!(!default_route_in(none, false));
        let v6 = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 \
                  fe800000000000000000000000000001 00000400 00000001 00000000 00000003 eth0\n";
        assert!(default_route_in(v6, true));
        let lo = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 \
                  00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo\n";
        assert!(!default_route_in(lo, true));
    }
}

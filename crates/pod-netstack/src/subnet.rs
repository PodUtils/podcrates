//! The private address space of a stack.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// The private address space that a stack owns: one IPv4 and one IPv6
/// prefix. The gateway of each is the host `1` of the prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subnet {
    pub v4: Ipv4Addr,
    pub v4_prefix: u8,
    pub v6: Ipv6Addr,
    pub v6_prefix: u8,
}

impl Default for Subnet {
    /// `10.66.0.0/24` and `fd00:66::/64`.
    fn default() -> Self {
        Self {
            v4: Ipv4Addr::new(10, 66, 0, 0),
            v4_prefix: 24,
            v6: Ipv6Addr::new(0xfd00, 0x66, 0, 0, 0, 0, 0, 0),
            v6_prefix: 64,
        }
    }
}

impl Subnet {
    /// The IPv4 gateway (`.1`). The services start at `.2`.
    pub fn gateway_v4(&self) -> Ipv4Addr {
        let mut octets = self.v4.octets();
        octets[3] = 1;
        Ipv4Addr::from(octets)
    }

    /// The IPv6 gateway (`::1` in the prefix).
    pub fn gateway_v6(&self) -> Ipv6Addr {
        let mut segments = self.v6.segments();
        segments[7] = 1;
        Ipv6Addr::from(segments)
    }

    /// The gateway of the family of `ip`.
    pub fn gateway_for(&self, ip: IpAddr) -> IpAddr {
        match ip {
            IpAddr::V4(_) => IpAddr::V4(self.gateway_v4()),
            IpAddr::V6(_) => IpAddr::V6(self.gateway_v6()),
        }
    }

    /// True when `ip` is in the subnet.
    pub fn contains(&self, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(ip) => prefix_contains_v4(self.v4, self.v4_prefix, ip),
            IpAddr::V6(ip) => prefix_contains_v6(self.v6, self.v6_prefix, ip),
        }
    }

    /// The prefix length of the family of `ip`.
    pub fn prefix_for(&self, ip: IpAddr) -> u8 {
        match ip {
            IpAddr::V4(_) => self.v4_prefix,
            IpAddr::V6(_) => self.v6_prefix,
        }
    }
}

fn prefix_contains_v4(base: Ipv4Addr, prefix: u8, ip: Ipv4Addr) -> bool {
    if prefix == 0 {
        return true;
    }
    if prefix >= 32 {
        return base == ip;
    }
    let mask = u32::MAX << (32 - prefix);
    (u32::from(base) & mask) == (u32::from(ip) & mask)
}

fn prefix_contains_v6(base: Ipv6Addr, prefix: u8, ip: Ipv6Addr) -> bool {
    if prefix == 0 {
        return true;
    }
    if prefix >= 128 {
        return base == ip;
    }
    let mask = u128::MAX << (128 - prefix);
    (u128::from(base) & mask) == (u128::from(ip) & mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subnet_holds_its_prefixes_and_its_gateways() {
        let subnet = Subnet::default();
        assert!(subnet.contains("10.66.0.2".parse().unwrap()));
        assert!(!subnet.contains("10.66.1.2".parse().unwrap()));
        assert!(subnet.contains("fd00:66::2".parse().unwrap()));
        assert!(!subnet.contains("fd00:67::2".parse().unwrap()));
        assert_eq!(subnet.gateway_v4(), Ipv4Addr::new(10, 66, 0, 1));
        assert_eq!(subnet.gateway_v6(), "fd00:66::1".parse::<Ipv6Addr>().unwrap());
        assert_eq!(subnet.gateway_for("fd00:66::9".parse().unwrap()), "fd00:66::1".parse::<IpAddr>().unwrap());
        assert_eq!(subnet.prefix_for("10.66.0.9".parse().unwrap()), 24);
        // A prefix of 0 holds each address; a full prefix holds one.
        let all = Subnet { v4_prefix: 0, v6_prefix: 128, ..subnet };
        assert!(all.contains("192.0.2.1".parse().unwrap()));
        assert!(all.contains("fd00:66::".parse().unwrap()));
        assert!(!all.contains("fd00:66::1".parse().unwrap()));
    }
}

//! The layout of a lab (issue 178): the namespaces `PREFIX-NAME-g|r|w`
//! (and `-c` for cgnat), their veth pairs and their addresses. The guest
//! has private addresses; each address past the router is in a range for
//! documentation (RFC 5737, RFC 3849) or the shared range of RFC 6598.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::knobs::{Family, Knobs};

pub const GUEST_V4: Ipv4Addr = Ipv4Addr::new(10, 77, 0, 2);
pub const GUEST_V6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0x77, 0, 0, 0, 0, 0, 2);
/// The router on the guest's link (`rg`).
pub const ROUTER_IN_V4: Ipv4Addr = Ipv4Addr::new(10, 77, 0, 1);
pub const ROUTER_IN_V6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0x77, 0, 0, 0, 0, 0, 1);
/// The router on the world's link (`rw`); with cgnat, the carrier's
/// namespace has these addresses on `cw`.
pub const ROUTER_OUT_V4: Ipv4Addr = Ipv4Addr::new(198, 51, 100, 1);
pub const ROUTER_OUT_V6: Ipv6Addr = Ipv6Addr::new(0x2001, 0xdb8, 0x100, 0, 0, 0, 0, 1);
pub const WORLD_V4: Ipv4Addr = Ipv4Addr::new(198, 51, 100, 10);
pub const WORLD_V6: Ipv6Addr = Ipv6Addr::new(0x2001, 0xdb8, 0x100, 0, 0, 0, 0, 0x10);
/// cgnat: the router's outside address on `rc` (issue 178) and the
/// carrier's inside address on `cr`.
pub const CGN_ROUTER_V4: Ipv4Addr = Ipv4Addr::new(100, 64, 0, 2);
pub const CGN_INSIDE_V4: Ipv4Addr = Ipv4Addr::new(100, 64, 0, 1);
/// cgnat: the IPv6 transit between the router and the carrier (call C147).
pub const CGN_ROUTER_V6: Ipv6Addr = Ipv6Addr::new(0x2001, 0xdb8, 0x101, 0, 0, 0, 0, 2);
pub const CGN_INSIDE_V6: Ipv6Addr = Ipv6Addr::new(0x2001, 0xdb8, 0x101, 0, 0, 0, 0, 1);
/// nat64: tayga's prefix. The well-known 64:ff9b::/96 cannot carry the
/// world's documentation range (RFC 6052 forbids a non-global IPv4; F18).
pub const NAT64_PREFIX: Ipv6Addr = Ipv6Addr::new(0x2001, 0xdb8, 0x64, 0, 0, 0, 0, 0);
pub const NAT64_PREFIX_LEN: u8 = 96;
/// nat64: tayga's dynamic pool and its own address in it.
pub const NAT64_POOL: Ipv4Addr = Ipv4Addr::new(192, 168, 255, 0);
pub const NAT64_POOL_LEN: u8 = 24;
pub const NAT64_TAYGA_V4: Ipv4Addr = Ipv4Addr::new(192, 168, 255, 1);
/// nat64: the name of tayga's tun device in the router.
pub const NAT64_DEVICE: &str = "nat64";
/// The guest's port that the router's port `DNAT_PORT` reaches.
pub const GUEST_PORT: u16 = 2200;
pub const DNAT_PORT: u16 = 2222;

/// The address in the NAT64 prefix of an IPv4 address (RFC 6052, /96).
pub fn nat64_address(v4: Ipv4Addr) -> Ipv6Addr {
    let [a, b, c, d] = v4.octets();
    let s = NAT64_PREFIX.segments();
    Ipv6Addr::new(s[0], s[1], s[2], s[3], s[4], s[5], u16::from_be_bytes([a, b]), u16::from_be_bytes([c, d]))
}

/// The role of a namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Guest,
    Router,
    World,
    /// cgnat: the carrier between the router and the world.
    Carrier,
}

impl Role {
    /// The last letter of the namespace's name.
    pub fn letter(self) -> char {
        match self {
            Role::Guest => 'g',
            Role::Router => 'r',
            Role::World => 'w',
            Role::Carrier => 'c',
        }
    }
}

/// One end of a veth pair: its namespace and its device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct End {
    pub role: Role,
    pub device: &'static str,
}

/// An address of a device, with its prefix length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address {
    pub role: Role,
    pub device: &'static str,
    pub addr: IpAddr,
    pub len: u8,
}

/// The layout of one lab: the roles in the order of their creation, the
/// veth pairs, and the addresses of the guest's stack and of the rest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub roles: Vec<Role>,
    pub veths: Vec<(End, End)>,
    pub addresses: Vec<Address>,
    /// The router's device toward the world: `rw`, or `rc` with cgnat.
    pub outside: &'static str,
    cgnat: bool,
}

const fn end(role: Role, device: &'static str) -> End {
    End { role, device }
}

fn v4(role: Role, device: &'static str, addr: Ipv4Addr, len: u8) -> Address {
    Address { role, device, addr: IpAddr::V4(addr), len }
}

fn v6(role: Role, device: &'static str, addr: Ipv6Addr, len: u8) -> Address {
    Address { role, device, addr: IpAddr::V6(addr), len }
}

impl Layout {
    /// The layout of `knobs`. The guest's link carries only the families of
    /// `--stack`; the other links carry both.
    pub fn of(knobs: &Knobs) -> Layout {
        use Role::*;
        let mut roles = vec![Guest, Router, World];
        let mut veths = vec![(end(Guest, "gr"), end(Router, "rg"))];
        let mut addresses = Vec::new();
        if knobs.stack.has(Family::V4) {
            addresses.push(v4(Guest, "gr", GUEST_V4, 24));
            addresses.push(v4(Router, "rg", ROUTER_IN_V4, 24));
        }
        if knobs.stack.has(Family::V6) {
            addresses.push(v6(Guest, "gr", GUEST_V6, 64));
            addresses.push(v6(Router, "rg", ROUTER_IN_V6, 64));
        }
        let outside = if knobs.cgnat {
            roles.push(Carrier);
            veths.push((end(Router, "rc"), end(Carrier, "cr")));
            veths.push((end(Carrier, "cw"), end(World, "wr")));
            addresses.extend([
                v4(Router, "rc", CGN_ROUTER_V4, 24),
                v6(Router, "rc", CGN_ROUTER_V6, 64),
                v4(Carrier, "cr", CGN_INSIDE_V4, 24),
                v6(Carrier, "cr", CGN_INSIDE_V6, 64),
                v4(Carrier, "cw", ROUTER_OUT_V4, 24),
                v6(Carrier, "cw", ROUTER_OUT_V6, 64),
            ]);
            "rc"
        } else {
            veths.push((end(Router, "rw"), end(World, "wr")));
            addresses.extend([v4(Router, "rw", ROUTER_OUT_V4, 24), v6(Router, "rw", ROUTER_OUT_V6, 64)]);
            "rw"
        };
        addresses.extend([v4(World, "wr", WORLD_V4, 24), v6(World, "wr", WORLD_V6, 64)]);
        Layout { roles, veths, addresses, outside, cgnat: knobs.cgnat }
    }

    /// The guest's address of `family`.
    pub fn guest(family: Family) -> IpAddr {
        match family {
            Family::V4 => IpAddr::V4(GUEST_V4),
            Family::V6 => IpAddr::V6(GUEST_V6),
        }
    }

    /// The router's address on the guest's link.
    pub fn router_in(family: Family) -> IpAddr {
        match family {
            Family::V4 => IpAddr::V4(ROUTER_IN_V4),
            Family::V6 => IpAddr::V6(ROUTER_IN_V6),
        }
    }

    pub fn world(family: Family) -> IpAddr {
        match family {
            Family::V4 => IpAddr::V4(WORLD_V4),
            Family::V6 => IpAddr::V6(WORLD_V6),
        }
    }

    /// The router's own outside address: the source of its masquerade and
    /// the target of a hairpin.
    pub fn router_out(&self, family: Family) -> IpAddr {
        match (family, self.cgnat) {
            (Family::V4, false) => IpAddr::V4(ROUTER_OUT_V4),
            (Family::V6, false) => IpAddr::V6(ROUTER_OUT_V6),
            (Family::V4, true) => IpAddr::V4(CGN_ROUTER_V4),
            (Family::V6, true) => IpAddr::V6(CGN_ROUTER_V6),
        }
    }

    /// The router's next hop toward the world.
    pub fn router_gateway(&self, family: Family) -> IpAddr {
        match (family, self.cgnat) {
            (Family::V4, false) => IpAddr::V4(WORLD_V4),
            (Family::V6, false) => IpAddr::V6(WORLD_V6),
            (Family::V4, true) => IpAddr::V4(CGN_INSIDE_V4),
            (Family::V6, true) => IpAddr::V6(CGN_INSIDE_V6),
        }
    }

    /// The address on the world's link that faces the world: the router's,
    /// or the carrier's with cgnat. The world's default route goes there.
    pub fn world_gateway(family: Family) -> IpAddr {
        match family {
            Family::V4 => IpAddr::V4(ROUTER_OUT_V4),
            Family::V6 => IpAddr::V6(ROUTER_OUT_V6),
        }
    }

    /// The address of the guest's side as the world sees it: the
    /// carrier's IPv4 with cgnat, else the router's outside address.
    pub fn public(&self, family: Family) -> IpAddr {
        if self.cgnat && family == Family::V4 { Layout::world_gateway(family) } else { self.router_out(family) }
    }

    pub fn has_carrier(&self) -> bool {
        self.cgnat
    }
}

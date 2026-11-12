//! The namespaces, the sysctls, the veth pairs, the addresses, the routes
//! and the first packet along each hop.

use std::net::IpAddr;

use super::{Step, exec, ip};
use crate::knobs::{Family, Stack};
use crate::lab::{Lab, Settings};
use crate::layout::{CGN_ROUTER_V4, CGN_ROUTER_V6, Layout, ROUTER_IN_V4, ROUTER_OUT_V4, Role, WORLD_V4, WORLD_V6};

fn flag(family: Family) -> &'static str {
    match family {
        Family::V4 => "-4",
        Family::V6 => "-6",
    }
}

fn sysctl(lab: &Lab, role: Role, setting: &str) -> Step {
    exec(lab, role, &["sysctl", "-w", setting])
}

/// The namespaces with their loopback, the sysctls, the veth pairs and the
/// addresses.
pub(super) fn namespaces(lab: &Lab, settings: &Settings, steps: &mut Vec<Step>) {
    let layout = lab.layout();
    let knobs = lab.knobs();
    for role in &layout.roles {
        steps.push(Step::run(&["ip", "netns", "add", &lab.namespace(*role)]));
        steps.push(ip(lab, *role, &["link", "set", "lo", "up"]));
    }
    if knobs.stack == Stack::V4 {
        // Before the veth: a new device takes `default`.
        steps.push(sysctl(lab, Role::Guest, "net.ipv6.conf.default.disable_ipv6=1"));
        steps.push(sysctl(lab, Role::Guest, "net.ipv6.conf.all.disable_ipv6=1"));
    }
    // The probe program opens ping sockets with no privilege (F20).
    let range = format!("net.ipv4.ping_group_range={} {}", settings.ping_gid, settings.ping_gid);
    steps.push(sysctl(lab, Role::Guest, &range));
    // A black hole of PMTU stays a black hole: no MSS probing at the ends.
    for role in [Role::Guest, Role::World] {
        steps.push(sysctl(lab, role, "net.ipv4.tcp_mtu_probing=0"));
    }
    for role in layout.roles.iter().filter(|r| matches!(r, Role::Router | Role::Carrier)) {
        steps.push(sysctl(lab, *role, "net.ipv4.ip_forward=1"));
        steps.push(sysctl(lab, *role, "net.ipv6.conf.all.forwarding=1"));
        // Else the kernel limits its ICMP errors, and most refusals of a
        // `reject` arrive as silent drops (L1).
        steps.push(sysctl(lab, *role, "net.ipv4.icmp_ratelimit=0"));
        steps.push(sysctl(lab, *role, "net.ipv6.icmp.ratelimit=0"));
    }
    for (a, b) in &layout.veths {
        let (na, nb) = (lab.namespace(a.role), lab.namespace(b.role));
        steps.push(Step::run(&[
            "ip", "link", "add", a.device, "netns", &na, "type", "veth", "peer", "name", b.device, "netns", &nb,
        ]));
    }
    for addr in &layout.addresses {
        let cidr = format!("{}/{}", addr.addr, addr.len);
        let mut args = vec!["addr", "add", cidr.as_str(), "dev", addr.device];
        // No duplicate address detection: an address that is tentative for
        // a second fails the first probe.
        if addr.addr.is_ipv6() {
            args.push("nodad");
        }
        steps.push(ip(lab, addr.role, &args));
    }
    for (a, b) in &layout.veths {
        steps.push(ip(lab, a.role, &["link", "set", a.device, "up"]));
        steps.push(ip(lab, b.role, &["link", "set", b.device, "up"]));
    }
    if knobs.stack == Stack::V6 {
        steps.push(ip(lab, Role::Guest, &["addr", "del", "127.0.0.1/8", "dev", "lo"]));
    }
}

fn route(lab: &Lab, role: Role, family: Family, args: &[&str]) -> Step {
    let mut all = vec![flag(family), "route"];
    all.extend_from_slice(args);
    ip(lab, role, &all)
}

/// The default routes, the routes of the carrier to the guest, and with
/// `--mtu` the MTU of each route of the router: the router sends
/// frag-needed and packet-too-big, as the links keep 1500 at both ends.
pub(super) fn routes(lab: &Lab, steps: &mut Vec<Step>) {
    let layout = lab.layout();
    let knobs = lab.knobs();
    let mtu = knobs.mtu.map(|m| m.to_string());
    let with_mtu = |mut args: Vec<String>| {
        if let Some(m) = &mtu {
            args.extend(["mtu".to_string(), m.clone()]);
        }
        args
    };
    for family in Family::BOTH.into_iter().filter(|f| knobs.stack.has(*f)) {
        let via = Layout::router_in(family).to_string();
        steps.push(route(lab, Role::Guest, family, &["add", "default", "via", &via]));
    }
    for family in Family::BOTH {
        let gw = layout.router_gateway(family).to_string();
        let args = with_mtu(vec!["add".into(), "default".into(), "via".into(), gw]);
        steps.push(route(lab, Role::Router, family, &args.iter().map(String::as_str).collect::<Vec<_>>()));
        let world_gw = Layout::world_gateway(family).to_string();
        steps.push(route(lab, Role::World, family, &["add", "default", "via", &world_gw]));
    }
    if knobs.mtu.is_some() {
        let mut connected: Vec<(Family, String, &str, Option<IpAddr>)> = Vec::new();
        if knobs.stack.has(Family::V4) {
            connected.push((Family::V4, "10.77.0.0/24".into(), "rg", Some(IpAddr::V4(ROUTER_IN_V4))));
        }
        if knobs.stack.has(Family::V6) {
            connected.push((Family::V6, "fd00:77::/64".into(), "rg", None));
        }
        if layout.has_carrier() {
            connected.push((Family::V4, "100.64.0.0/24".into(), "rc", Some(IpAddr::V4(CGN_ROUTER_V4))));
            connected.push((Family::V6, "2001:db8:101::/64".into(), "rc", None));
        } else {
            connected.push((Family::V4, "198.51.100.0/24".into(), "rw", Some(IpAddr::V4(ROUTER_OUT_V4))));
            connected.push((Family::V6, "2001:db8:100::/64".into(), "rw", None));
        }
        for (family, prefix, dev, src) in connected {
            // The same key as the kernel's route of the link, so that
            // `replace` changes it and adds no second route.
            let mut args: Vec<String> = vec!["replace".into(), prefix, "dev".into(), dev.into()];
            let tail = match src {
                Some(src) => format!("proto kernel scope link src {src}"),
                None => "proto kernel metric 256".to_string(),
            };
            args.extend(tail.split(' ').map(String::from));
            let args = with_mtu(args);
            steps.push(route(lab, Role::Router, family, &args.iter().map(String::as_str).collect::<Vec<_>>()));
        }
    }
    if layout.has_carrier() {
        steps.push(route(lab, Role::Carrier, Family::V4, &["add", "default", "via", &WORLD_V4.to_string()]));
        steps.push(route(lab, Role::Carrier, Family::V6, &["add", "default", "via", &WORLD_V6.to_string()]));
        // The carrier drops the world's new flows to the guest itself; a
        // route makes them reach its rule and not an ICMP error.
        let v4 = CGN_ROUTER_V4.to_string();
        let v6 = CGN_ROUTER_V6.to_string();
        steps.push(route(lab, Role::Carrier, Family::V4, &["add", "10.77.0.0/24", "via", &v4]));
        steps.push(route(lab, Role::Carrier, Family::V6, &["add", "fd00:77::/64", "via", &v6]));
    }
}

/// One echo along each hop, before the netem queues: a one-shot resolver
/// failed right after `up` until the neighbours were known (L3).
pub(super) fn hops(lab: &Lab, steps: &mut Vec<Step>) {
    let layout = lab.layout();
    let knobs = lab.knobs();
    let mut hop = |role: Role, to: IpAddr| {
        steps.push(exec(lab, role, &["ping", "-c", "1", "-W", "2", &to.to_string()]));
    };
    for family in Family::BOTH {
        if knobs.stack.has(family) {
            hop(Role::Guest, Layout::router_in(family));
        }
        hop(Role::Router, layout.router_gateway(family));
        if layout.has_carrier() {
            hop(Role::Carrier, Layout::world(family));
        }
        hop(Role::World, Layout::world_gateway(family));
    }
}

//! The router's nftables: one `inet` table with the named counters of
//! sand-net, and the carrier's table with cgnat.
//!
//! The counters go in through nft's JSON: `return` is a keyword of nft's
//! text, and nft 1.0.9 refuses `counter return {}` (call C148). The chains
//! go in as text, which reads `counter name "return"`.

use super::{Step, exec};
use crate::knobs::{Broken, Family, Icmp};
use crate::lab::Lab;
use crate::layout::{DNAT_PORT, GUEST_PORT, Layout, NAT64_DEVICE, NAT64_POOL, NAT64_POOL_LEN, Role};

/// The router's table.
pub const TABLE: &str = "lab";
/// The carrier's table (cgnat).
pub const CARRIER_TABLE: &str = "cgn";
/// The router's filter chain of forwarded flows. Not `fwd`: that is a
/// keyword of nft and cannot name a chain (F9).
pub const FORWARD_CHAIN: &str = "forward_policy";

/// A named counter of the router's table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Counter {
    /// A packet of a flow that the router knows (established or related).
    Return,
    /// A new flow from the guest, before each verdict.
    Forwarded,
    /// A new flow to a forwarded port, or a new flow from the outside.
    Inbound,
    /// A flow of the family of `--broken`.
    Broken,
    /// An echo request of the guest.
    Ping,
    /// A new flow of the guest under `--policy open`.
    Open,
    /// A new TCP flow of the guest to a port of the policy's list.
    Tcp,
    /// A new UDP flow of the guest to a port of the policy's list.
    Udp,
    /// A flow that no rule passes, port 53 under `--no-dns`, and an ICMP
    /// error that `--icmp drop` withholds.
    Dropped,
}

impl Counter {
    pub const ALL: [Counter; 9] = [
        Counter::Return,
        Counter::Forwarded,
        Counter::Inbound,
        Counter::Broken,
        Counter::Ping,
        Counter::Open,
        Counter::Tcp,
        Counter::Udp,
        Counter::Dropped,
    ];

    /// The counter's name in the table.
    pub fn name(self) -> &'static str {
        match self {
            Counter::Return => "return",
            Counter::Forwarded => "forwarded",
            Counter::Inbound => "inbound",
            Counter::Broken => "broken",
            Counter::Ping => "ping",
            Counter::Open => "open",
            Counter::Tcp => "tcp",
            Counter::Udp => "udp",
            Counter::Dropped => "dropped",
        }
    }

    pub fn from_name(name: &str) -> Option<Counter> {
        Counter::ALL.into_iter().find(|c| c.name() == name)
    }
}

/// The table and its counters as nft's JSON (`nft -j -f -`).
pub fn counters_json() -> String {
    let mut items = vec![format!(r#"{{"add":{{"table":{{"family":"inet","name":"{TABLE}"}}}}}}"#)];
    for c in Counter::ALL {
        items.push(format!(r#"{{"add":{{"counter":{{"family":"inet","table":"{TABLE}","name":"{}"}}}}}}"#, c.name()));
    }
    format!(r#"{{"nftables":[{}]}}"#, items.join(","))
}

fn count(c: Counter) -> String {
    format!("counter name \"{}\"", c.name())
}

fn prefix(family: Family) -> &'static str {
    match family {
        Family::V4 => "ip saddr 10.77.0.0/24",
        Family::V6 => "ip6 saddr fd00:77::/64",
    }
}

fn daddr(family: Family) -> &'static str {
    match family {
        Family::V4 => "ip daddr",
        Family::V6 => "ip6 daddr",
    }
}

/// `dnat ip to 10.77.0.2:2200` or `dnat ip6 to [fd00:77::2]:2200`.
fn dnat_to(family: Family) -> String {
    match family {
        Family::V4 => format!("dnat ip to {}:{GUEST_PORT}", Layout::guest(Family::V4)),
        Family::V6 => format!("dnat ip6 to [{}]:{GUEST_PORT}", Layout::guest(Family::V6)),
    }
}

/// The rules of the forward chain, in order.
pub fn forward_rules(lab: &Lab) -> Vec<String> {
    let knobs = lab.knobs();
    let out = lab.layout().outside;
    let mut rules = vec![
        format!("ct state established,related {} accept", count(Counter::Return)),
        format!("iifname \"rg\" ct state new {}", count(Counter::Forwarded)),
    ];
    if let Some(broken) = knobs.broken {
        let verdict = if broken.rejects() { "reject with icmpx type port-unreachable" } else { "drop" };
        rules.push(format!("meta nfproto {} {} {verdict}", broken.family().nfproto(), count(Counter::Broken)));
    }
    // Before any drop of a new flow: a forwarded DNAT flow is new too (L5).
    rules.push(format!("ct status dnat {} accept", count(Counter::Inbound)));
    if knobs.nat64 {
        // tayga's IPv4 side: its IPv6 side passed the rules below.
        rules.push(format!("iifname \"{NAT64_DEVICE}\" accept"));
    }
    for family in Family::BOTH {
        // With NAT on a family, NAT hides nothing inbound unless new flows
        // are dropped (L6).
        let verdict = if knobs.nat_on(family) { "drop" } else { "accept" };
        rules.push(format!(
            "iifname \"{out}\" meta nfproto {} ct state new {} {verdict}",
            family.nfproto(),
            count(Counter::Inbound)
        ));
    }
    let ping = if knobs.ping_passes() { "accept" } else { "drop" };
    rules.push(format!("iifname \"rg\" icmp type echo-request {} {ping}", count(Counter::Ping)));
    rules.push(format!("iifname \"rg\" icmpv6 type echo-request {} {ping}", count(Counter::Ping)));
    if !knobs.dns {
        rules.push(format!("iifname \"rg\" meta l4proto {{ tcp, udp }} th dport 53 {} drop", count(Counter::Dropped)));
    }
    match knobs.policy.lists() {
        None => rules.push(format!("iifname \"rg\" {} accept", count(Counter::Open))),
        Some((tcp, udp)) => {
            let set = |l: &[u16]| l.iter().map(u16::to_string).collect::<Vec<_>>().join(", ");
            if !tcp.is_empty() {
                rules.push(format!("iifname \"rg\" tcp dport {{ {} }} {} accept", set(&tcp), count(Counter::Tcp)));
            }
            if !udp.is_empty() {
                rules.push(format!("iifname \"rg\" udp dport {{ {} }} {} accept", set(&udp), count(Counter::Udp)));
            }
        }
    }
    rules.push(format!("{} drop", count(Counter::Dropped)));
    rules
}

fn chain(name: &str, head: &str, rules: &[String]) -> String {
    let mut text = format!("\tchain {name} {{\n\t\t{head}\n");
    for rule in rules {
        text.push_str(&format!("\t\t{rule}\n"));
    }
    text.push_str("\t}\n");
    text
}

/// The chains of the router's table as nft's text (`nft -f -`). The table
/// and its counters exist first (`counters_json`).
pub fn router_rules(lab: &Lab) -> String {
    let knobs = lab.knobs();
    let layout = lab.layout();
    let out = layout.outside;
    let families: Vec<Family> = Family::BOTH.into_iter().filter(|f| knobs.stack.has(*f)).collect();
    let mut text = format!("table inet {TABLE} {{\n");
    let head = "type filter hook forward priority filter; policy drop;";
    text.push_str(&chain(FORWARD_CHAIN, head, &forward_rules(lab)));
    if knobs.icmp == Icmp::Drop {
        let rules = [
            format!("icmp type destination-unreachable icmp code frag-needed {} drop", count(Counter::Dropped)),
            format!("icmpv6 type packet-too-big {} drop", count(Counter::Dropped)),
        ];
        text.push_str(&chain("icmp_out", "type filter hook output priority filter; policy accept;", &rules));
    }
    let mut pre = Vec::new();
    for &family in &families {
        let to = layout.router_out(family);
        if knobs.dnat {
            pre.push(format!("iifname \"{out}\" {} {to} tcp dport {DNAT_PORT} {}", daddr(family), dnat_to(family)));
        }
        if knobs.hairpin {
            pre.push(format!("iifname \"rg\" {} {to} tcp dport {DNAT_PORT} {}", daddr(family), dnat_to(family)));
        }
    }
    if !pre.is_empty() {
        text.push_str(&chain("nat_pre", "type nat hook prerouting priority dstnat; policy accept;", &pre));
    }
    let flags = knobs.nat_flags.map(|f| format!(" {}", f.word())).unwrap_or_default();
    let mut post = Vec::new();
    if knobs.nat.has(Family::V4) {
        post.push(format!("oifname \"{out}\" meta nfproto ipv4 masquerade{flags}"));
    } else if knobs.nat64 {
        post.push(format!("oifname \"{out}\" ip saddr {NAT64_POOL}/{NAT64_POOL_LEN} masquerade{flags}"));
    }
    if knobs.nat.has(Family::V6) {
        post.push(format!("oifname \"{out}\" meta nfproto ipv6 masquerade{flags}"));
    }
    if knobs.hairpin {
        // The reply of the guest to itself goes back through the router.
        for &family in &families {
            post.push(format!("oifname \"rg\" {} ct status dnat masquerade", prefix(family)));
        }
    }
    if !post.is_empty() {
        text.push_str(&chain("nat_post", "type nat hook postrouting priority srcnat; policy accept;", &post));
    }
    text.push_str("}\n");
    text
}

/// The carrier's table: NAT44 to its outside address, and the world's new
/// IPv4 flows dropped, to the carrier itself and through it. IPv6 is
/// routed.
pub fn carrier_rules() -> String {
    let forward = [
        "ct state established,related accept".to_string(),
        "iifname \"cw\" meta nfproto ipv4 ct state new drop".to_string(),
    ];
    // An echo of the world still reaches the carrier (the hop of L3).
    let input = ["iifname \"cw\" meta nfproto ipv4 meta l4proto { tcp, udp } ct state new drop".to_string()];
    let post = ["oifname \"cw\" meta nfproto ipv4 masquerade".to_string()];
    let mut text = format!("table inet {CARRIER_TABLE} {{\n");
    text.push_str(&chain("forward_policy", "type filter hook forward priority filter; policy accept;", &forward));
    text.push_str(&chain("input_policy", "type filter hook input priority filter; policy accept;", &input));
    text.push_str(&chain("nat_post", "type nat hook postrouting priority srcnat; policy accept;", &post));
    text.push_str("}\n");
    text
}

pub(super) fn steps(lab: &Lab, steps: &mut Vec<Step>) {
    steps.push(exec(lab, Role::Router, &["nft", "-j", "-f", "-"]).with_stdin(counters_json()));
    steps.push(exec(lab, Role::Router, &["nft", "-f", "-"]).with_stdin(router_rules(lab)));
    if lab.layout().has_carrier() {
        steps.push(exec(lab, Role::Carrier, &["nft", "-f", "-"]).with_stdin(carrier_rules()));
    }
}

/// The command that reads the router's counters for the judge.
pub fn list_counters(lab: &Lab) -> Step {
    exec(lab, Role::Router, &["nft", "-j", "list", "counters", "table", "inet", TABLE])
}

/// Whether a broken family rejects (for the derivation).
pub(crate) fn rejects(broken: Option<Broken>, family: Family) -> Option<bool> {
    broken.filter(|b| b.family() == family).map(Broken::rejects)
}

//! The knobs of a lab (issue 178, "Knobs"), with their ranges. Each knob
//! keeps the word of sand-net's flag; `flags` reads and writes the flags,
//! and `check` refuses each contradiction.

use std::fmt;

mod check;
mod flags;

pub use check::{Contradiction, KnobError};

/// An address family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    V4,
    V6,
}

impl Family {
    pub const BOTH: [Family; 2] = [Family::V4, Family::V6];

    /// `4` or `6`, as a probe name writes it.
    pub fn digit(self) -> char {
        match self {
            Family::V4 => '4',
            Family::V6 => '6',
        }
    }

    /// The word of nft's `meta nfproto`.
    pub fn nfproto(self) -> &'static str {
        match self {
            Family::V4 => "ipv4",
            Family::V6 => "ipv6",
        }
    }
}

/// `--stack`: the families of the guest. `v6` also removes 127.0.0.1 from
/// the guest's loopback. The router and the world keep both families.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Stack {
    #[default]
    Dual,
    V4,
    V6,
}

impl Stack {
    pub fn has(self, family: Family) -> bool {
        match self {
            Stack::Dual => true,
            Stack::V4 => family == Family::V4,
            Stack::V6 => family == Family::V6,
        }
    }
}

/// `--nat`: the families that the router translates to its outside
/// address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Nat {
    #[default]
    None,
    Nat44,
    Nat66,
    Both,
}

impl Nat {
    /// Whether the router masquerades `family` (NAT44 or NAT66).
    pub fn has(self, family: Family) -> bool {
        matches!((self, family), (Nat::Nat44 | Nat::Both, Family::V4) | (Nat::Nat66 | Nat::Both, Family::V6))
    }
}

/// `--nat-flags`: the flag of each masquerade rule of the router.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NatFlags {
    Persistent,
    Random,
    FullyRandom,
}

impl NatFlags {
    /// Whether the source port of a flow changes (`random`, `fully-random`).
    pub fn moves_ports(self) -> bool {
        matches!(self, NatFlags::Random | NatFlags::FullyRandom)
    }
}

/// `--policy`, or the lists of `--tcp` and `--udp`: the new flows that the
/// guest can open through the router.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Policy {
    /// Each flow.
    #[default]
    Open,
    /// TCP 80 and 443, and DNS on UDP 53.
    Web,
    /// TCP 80 and 443.
    WebNoDns,
    /// TCP 443.
    Only443,
    /// No flow.
    Blackout,
    /// `--tcp` and `--udp`: the ports of each protocol, each port one time.
    Ports { tcp: Vec<u16>, udp: Vec<u16> },
}

/// A transport protocol of a probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proto {
    Tcp,
    Udp,
}

impl Policy {
    /// The ports of each list, or `None` for `open`.
    pub fn lists(&self) -> Option<(Vec<u16>, Vec<u16>)> {
        match self {
            Policy::Open => None,
            Policy::Web => Some((vec![80, 443], vec![53])),
            Policy::WebNoDns => Some((vec![80, 443], vec![])),
            Policy::Only443 => Some((vec![443], vec![])),
            Policy::Blackout => Some((vec![], vec![])),
            Policy::Ports { tcp, udp } => Some((tcp.clone(), udp.clone())),
        }
    }

    /// Whether a new flow of `proto` to `port` passes.
    pub fn allows(&self, proto: Proto, port: u16) -> bool {
        match self.lists() {
            None => true,
            Some((tcp, udp)) => match proto {
                Proto::Tcp => tcp.contains(&port),
                Proto::Udp => udp.contains(&port),
            },
        }
    }
}

/// `--broken`: one family that the router breaks, with a silent drop or a
/// refusal (an ICMP port-unreachable).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Broken {
    V4Drop,
    V4Reject,
    V6Drop,
    V6Reject,
}

impl Broken {
    pub fn family(self) -> Family {
        match self {
            Broken::V4Drop | Broken::V4Reject => Family::V4,
            Broken::V6Drop | Broken::V6Reject => Family::V6,
        }
    }

    pub fn rejects(self) -> bool {
        matches!(self, Broken::V4Reject | Broken::V6Reject)
    }
}

/// `--icmp`: `drop` withholds the router's frag-needed and packet-too-big.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Icmp {
    #[default]
    Pass,
    Drop,
}

/// `--ping`: the rule of an echo request of the guest. With no flag, an
/// echo request follows the policy: it passes under `open`, else it is
/// dropped (call C146).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Ping {
    #[default]
    Policy,
    Allow,
    Drop,
}

/// Each knob of a lab. `Knobs::default()` is a lab with no flag: a dual
/// stack, no NAT, the policy `open`, DNS, no impairment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Knobs {
    pub stack: Stack,
    pub nat: Nat,
    pub nat_flags: Option<NatFlags>,
    pub policy: Policy,
    /// `false` is `--no-dns`: the router drops each flow to port 53.
    pub dns: bool,
    pub broken: Option<Broken>,
    /// `--mtu`: the MTU of each route of the router, 1280 to 1499.
    pub mtu: Option<u16>,
    pub icmp: Icmp,
    /// `--delay`: milliseconds added in each direction, 1 to 10,000.
    pub delay_ms: Option<u32>,
    /// `--loss`: the loss of each direction in hundredths of a percent
    /// (500 is 5 %), 1 to 2,000.
    pub loss_bp: Option<u16>,
    /// `--rate`: kbit/s in each direction, 64 to 10,000,000.
    pub rate_kbit: Option<u32>,
    /// `--dnat`: the router's port 2222 to the guest's port 2200.
    pub dnat: bool,
    /// `--hairpin`: the guest reaches its own forward through the router's
    /// outside address.
    pub hairpin: bool,
    pub ping: Ping,
    /// `--nat64`: tayga on the router, prefix 2001:db8:64::/96.
    pub nat64: bool,
    /// `--dns64`: the world's DNS synthesizes AAAA records in the prefix.
    pub dns64: bool,
    /// `--cgnat`: a fourth namespace between the router and the world.
    pub cgnat: bool,
}

impl Default for Knobs {
    fn default() -> Knobs {
        Knobs {
            stack: Stack::Dual,
            nat: Nat::None,
            nat_flags: None,
            policy: Policy::Open,
            dns: true,
            broken: None,
            mtu: None,
            icmp: Icmp::Pass,
            delay_ms: None,
            loss_bp: None,
            rate_kbit: None,
            dnat: false,
            hairpin: false,
            ping: Ping::Policy,
            nat64: false,
            dns64: false,
            cgnat: false,
        }
    }
}

impl Knobs {
    /// Whether the router translates the source of `family` toward the
    /// outside: NAT44 or NAT66, and for IPv4 also the pool of NAT64.
    pub fn nat_on(&self, family: Family) -> bool {
        self.nat.has(family) || (family == Family::V4 && self.nat64)
    }

    /// Whether an echo request of the guest passes the router.
    pub fn ping_passes(&self) -> bool {
        match self.ping {
            Ping::Allow => true,
            Ping::Drop => false,
            Ping::Policy => self.policy == Policy::Open,
        }
    }

    /// Whether the router has a netem queue on its two links.
    pub fn impaired(&self) -> bool {
        self.delay_ms.is_some() || self.loss_bp.is_some() || self.rate_kbit.is_some()
    }
}

/// A loss in hundredths of a percent, as netem reads it: `5%`, `0.25%`.
pub fn loss_text(bp: u16) -> String {
    if bp.is_multiple_of(100) { format!("{}%", bp / 100) } else { format!("{}.{:02}%", bp / 100, bp % 100) }
}

macro_rules! words {
    ($ty:ty { $($variant:path => $word:literal),+ $(,)? }) => {
        impl $ty {
            /// The words of sand-net's flag, in the order of the type.
            pub const WORDS: &'static [&'static str] = &[$($word),+];

            /// The word of sand-net's flag.
            pub fn word(&self) -> &'static str {
                match self { $($variant => $word),+ }
            }

            /// The value of a word of sand-net's flag.
            pub fn from_word(word: &str) -> Option<Self> {
                match word { $($word => Some($variant),)+ _ => None }
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.word())
            }
        }
    };
}

words!(Stack { Stack::Dual => "dual", Stack::V4 => "v4", Stack::V6 => "v6" });
words!(Nat { Nat::None => "none", Nat::Nat44 => "nat44", Nat::Nat66 => "nat66", Nat::Both => "both" });
words!(NatFlags {
    NatFlags::Persistent => "persistent",
    NatFlags::Random => "random",
    NatFlags::FullyRandom => "fully-random",
});
words!(Broken {
    Broken::V4Drop => "v4-drop",
    Broken::V4Reject => "v4-reject",
    Broken::V6Drop => "v6-drop",
    Broken::V6Reject => "v6-reject",
});
words!(Icmp { Icmp::Pass => "pass", Icmp::Drop => "drop" });
words!(Ping { Ping::Policy => "policy", Ping::Allow => "allow", Ping::Drop => "drop" });

impl Policy {
    /// The word of `--policy`, or `None` for the lists of `--tcp` and
    /// `--udp`.
    pub fn word(&self) -> Option<&'static str> {
        match self {
            Policy::Open => Some("open"),
            Policy::Web => Some("web"),
            Policy::WebNoDns => Some("web-nodns"),
            Policy::Only443 => Some("only443"),
            Policy::Blackout => Some("blackout"),
            Policy::Ports { .. } => None,
        }
    }

    /// The words of `--policy`.
    pub const WORDS: &'static [&'static str] = &["open", "web", "web-nodns", "only443", "blackout"];

    /// The policy of a word of `--policy`.
    pub fn from_word(word: &str) -> Option<Policy> {
        Some(match word {
            "open" => Policy::Open,
            "web" => Policy::Web,
            "web-nodns" => Policy::WebNoDns,
            "only443" => Policy::Only443,
            "blackout" => Policy::Blackout,
            _ => return None,
        })
    }
}

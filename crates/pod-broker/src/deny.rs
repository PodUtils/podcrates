//! The denies: the fixed denies that no policy lifts, the addresses of the
//! host, the deny file of the host and the extra denies of a policy. An
//! address is checked in its plain form: an IPv4 address inside an IPv6
//! address (mapped `::ffff:a.b.c.d`, compatible `::a.b.c.d`, NAT64
//! `64:ff9b::/96`, 6to4 `2002::/16`) is checked as IPv4 too.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

use pod_core::{Class, Failure, Stage};

use crate::target::{Host, parse_host};

/// The fixed denies of an address: each net and the name of its rule
/// (`fixed:NAME`). The first net that holds an address names the rule, so a
/// metadata address comes before its wider range.
pub const FIXED_NETS: &[(&str, &str)] = &[
    ("169.254.169.254/32", "metadata"),
    ("169.254.170.2/32", "metadata"),
    ("168.63.129.16/32", "metadata"),
    ("100.100.100.200/32", "metadata"),
    ("fd00:ec2::254/128", "metadata"),
    ("0.0.0.0/8", "unspecified"),
    ("10.0.0.0/8", "private"),
    ("100.64.0.0/10", "cgnat"),
    ("127.0.0.0/8", "loopback"),
    ("169.254.0.0/16", "link-local"),
    ("172.16.0.0/12", "private"),
    ("192.0.0.0/24", "special"),
    ("192.168.0.0/16", "private"),
    ("224.0.0.0/4", "multicast"),
    ("240.0.0.0/4", "reserved"),
    ("::/128", "unspecified"),
    ("::1/128", "loopback"),
    ("fc00::/7", "private"),
    ("fe80::/10", "link-local"),
    ("fec0::/10", "site-local"),
    ("ff00::/8", "multicast"),
];

/// The fixed denies of a name: the names of the metadata services and of
/// loopback. A `*.` rule holds each name below it.
pub const FIXED_NAMES: &[(&str, &str)] = &[
    ("metadata.google.internal", "metadata"),
    ("metadata.goog", "metadata"),
    ("metadata", "metadata"),
    ("instance-data", "metadata"),
    ("instance-data.ec2.internal", "metadata"),
    ("localhost", "loopback"),
    ("*.localhost", "loopback"),
];

/// The fixed denies of a port: SMTP.
pub const FIXED_PORTS: &[(u16, &str)] = &[(25, "smtp")];

/// A net of addresses: an address and the length of its prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Net {
    addr: IpAddr,
    prefix: u8,
}

impl Net {
    /// `ADDR/PREFIX`, or one address. The bits after the prefix are cleared.
    pub fn parse(text: &str) -> Result<Net, String> {
        let (addr, prefix) = match text.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (text, None),
        };
        let addr: IpAddr = addr.parse().map_err(|_| format!("{text:?} is not an address or a net"))?;
        let most = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(p) => p.parse::<u8>().ok().filter(|p| *p <= most).ok_or_else(|| format!("{text:?}: a bad prefix"))?,
            None => most,
        };
        let addr = match addr {
            IpAddr::V4(ip) => IpAddr::V4(Ipv4Addr::from(u32::from(ip) & mask32(prefix))),
            IpAddr::V6(ip) => IpAddr::V6(Ipv6Addr::from(u128::from(ip) & mask128(prefix))),
        };
        Ok(Net { addr, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => u32::from(ip) & mask32(self.prefix) == u32::from(net),
            (IpAddr::V6(net), IpAddr::V6(ip)) => u128::from(ip) & mask128(self.prefix) == u128::from(net),
            _ => false,
        }
    }
}

fn mask32(prefix: u8) -> u32 {
    if prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(prefix)) }
}

fn mask128(prefix: u8) -> u128 {
    if prefix == 0 { 0 } else { u128::MAX << (128 - u32::from(prefix)) }
}

/// A name rule: one name, or each name below a suffix (`*.example.com`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameRule {
    Exact(String),
    /// The suffix with its leading dot: `.example.com`.
    Below(String),
}

impl NameRule {
    /// `NAME` or `*.NAME`, in the form of a target's host.
    pub fn parse(text: &str) -> Result<NameRule, String> {
        let (below, name) = match text.strip_prefix("*.") {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        match parse_host(name) {
            Ok(Host::Name(name)) if below => Ok(NameRule::Below(format!(".{name}"))),
            Ok(Host::Name(name)) => Ok(NameRule::Exact(name)),
            Ok(Host::Ip(_)) => Err(format!("{text:?}: an address is not a name rule")),
            Err(e) => Err(format!("{text:?}: {e}")),
        }
    }

    /// Whether the rule holds `name` (a name of [`parse_host`]), on whole
    /// labels.
    pub fn holds(&self, name: &str) -> bool {
        match self {
            NameRule::Exact(rule) => name == rule,
            NameRule::Below(suffix) => name.len() > suffix.len() && name.ends_with(suffix.as_str()),
        }
    }
}

/// A list of denies: nets and names, each with the name of its rule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DenyList {
    nets: Vec<(Net, String)>,
    names: Vec<(NameRule, String)>,
}

impl DenyList {
    pub fn new() -> DenyList {
        DenyList::default()
    }

    /// The fixed denies.
    pub fn fixed() -> DenyList {
        let mut list = DenyList::new();
        for (net, why) in FIXED_NETS {
            list.nets.push((Net::parse(net).expect("a fixed net"), format!("fixed:{why}")));
        }
        for (name, why) in FIXED_NAMES {
            list.names.push((NameRule::parse(name).expect("a fixed name"), format!("fixed:{why}")));
        }
        list
    }

    /// The addresses of the host, under the rule `host:address`.
    pub fn host_addresses(addrs: &[IpAddr]) -> DenyList {
        let mut list = DenyList::new();
        for ip in addrs {
            let ip = plain(*ip);
            let prefix = if ip.is_ipv4() { 32 } else { 128 };
            list.nets.push((Net { addr: ip, prefix }, "host:address".into()));
        }
        list
    }

    /// One entry: a net, an address, a name or `*.NAME`, under `rule`.
    pub fn add(&mut self, entry: &str, rule: impl Into<String>) -> Result<(), String> {
        let rule = rule.into();
        let entry = entry.trim();
        if entry.contains('/') || entry.parse::<IpAddr>().is_ok() {
            self.nets.push((Net::parse(entry)?, rule));
        } else {
            self.names.push((NameRule::parse(entry)?, rule));
        }
        Ok(())
    }

    /// A deny file: one net, address or name on each line; `#` starts a
    /// comment. Each entry is the rule `PREFIX:LINE` (`file:12`).
    pub fn parse(text: &str, prefix: &str) -> Result<DenyList, Failure> {
        let mut list = DenyList::new();
        for (n, line) in text.lines().enumerate() {
            let entry = line.split('#').next().unwrap_or("").trim();
            if entry.is_empty() {
                continue;
            }
            list.add(entry, format!("{prefix}:{}", n + 1))
                .map_err(|e| Failure::new(Stage::Proxy, Class::Usage, format!("the deny list, line {}: {e}", n + 1)))?;
        }
        Ok(list)
    }

    /// Read the deny file at `path` (at most 4 MiB), with the rules
    /// `file:LINE`.
    pub fn read(path: &Path) -> Result<DenyList, Failure> {
        const MOST: u64 = 4 << 20;
        let size = std::fs::metadata(path).map_err(|e| Failure::io(Stage::Host, e))?.len();
        if size > MOST {
            return Err(Failure::new(Stage::Host, Class::Usage, format!("{}: larger than 4 MiB", path.display())));
        }
        let text = std::fs::read_to_string(path).map_err(|e| Failure::io(Stage::Host, e))?;
        DenyList::parse(&text, "file")
    }

    /// Each entry of `other` after the entries of this list.
    pub fn extend(&mut self, other: &DenyList) {
        self.nets.extend(other.nets.iter().cloned());
        self.names.extend(other.names.iter().cloned());
    }

    pub fn len(&self) -> usize {
        self.nets.len() + self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The rule that denies `ip`, in its plain form and as each IPv4
    /// address that it holds.
    pub fn check_ip(&self, ip: IpAddr) -> Option<&str> {
        let forms = forms(ip);
        self.nets
            .iter()
            .find(|(net, _)| forms.iter().flatten().any(|ip| net.contains(*ip)))
            .map(|(_, rule)| rule.as_str())
    }

    /// The rule that denies the name `name` (a name of [`parse_host`]).
    pub fn check_name(&self, name: &str) -> Option<&str> {
        self.names.iter().find(|(rule, _)| rule.holds(name)).map(|(_, rule)| rule.as_str())
    }

    /// The rule that denies `host`.
    pub fn check_host(&self, host: &Host) -> Option<&str> {
        match host {
            Host::Name(name) => self.check_name(name),
            Host::Ip(ip) => self.check_ip(*ip),
        }
    }
}

/// The rule that denies `port`: `fixed:smtp` for 25.
pub fn check_port(port: u16) -> Option<String> {
    FIXED_PORTS.iter().find(|(p, _)| *p == port).map(|(_, why)| format!("fixed:{why}"))
}

/// `ip` with an IPv4-mapped address made IPv4.
pub fn plain(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

/// `ip` in its plain form, and the IPv4 address that an IPv6 address holds
/// (compatible, NAT64, 6to4), when it holds one.
fn forms(ip: IpAddr) -> [Option<IpAddr>; 2] {
    let ip = plain(ip);
    let IpAddr::V6(v6) = ip else { return [Some(ip), None] };
    let bits = u128::from(v6);
    let low = Ipv4Addr::from(bits as u32);
    let compatible = bits >> 32 == 0 && bits > 1;
    let nat64 = bits >> 32 == 0x0064_ff9b_0000_0000_0000_0000;
    let inner = if compatible || nat64 {
        Some(low)
    } else if bits >> 112 == 0x2002 {
        Some(Ipv4Addr::from((bits >> 80) as u32))
    } else {
        None
    };
    [Some(ip), inner.map(IpAddr::V4)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_net_clears_its_host_bits_and_holds_its_range() {
        let net = Net::parse("10.1.2.3/8").unwrap();
        assert!(net.contains("10.200.0.1".parse().unwrap()) && !net.contains("11.0.0.1".parse().unwrap()));
        assert!(Net::parse("0.0.0.0/0").unwrap().contains("8.8.8.8".parse().unwrap()));
        assert!(!Net::parse("::/0").unwrap().contains("8.8.8.8".parse().unwrap()));
        for bad in ["10.0.0.0/33", "::/129", "x/8", "10.0.0.0/"] {
            assert!(Net::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_ipv4_address_inside_an_ipv6_address_is_checked_as_ipv4() {
        let fixed = DenyList::fixed();
        for ip in ["::ffff:169.254.169.254", "::169.254.169.254", "64:ff9b::a9fe:a9fe", "2002:a9fe:a9fe::1"] {
            assert_eq!(fixed.check_ip(ip.parse().unwrap()), Some("fixed:metadata"), "{ip}");
        }
        assert_eq!(fixed.check_ip("64:ff9b::808:808".parse().unwrap()), None);
    }
}

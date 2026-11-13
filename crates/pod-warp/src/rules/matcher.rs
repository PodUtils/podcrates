//! The matcher of a rule: a name (`domain:NAME`, `domain:.NAME`),
//! prefixes (`cidr:ADDR/PREFIX`, `cidr:@FILE`), ports of any host
//! (`port:N`, `port:N-M`) or one host and port (`tcp:HOST:PORT`), as
//! podnet's netns rules read them, so that one text of a rule means the
//! same in both (PodUtils/podnet#518).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

use pod_core::{Class, Failure, Stage};

fn usage(text: String) -> Failure {
    Failure::new(Stage::Warp, Class::Usage, text)
}

/// What a rule matches.
#[derive(Clone, Debug)]
pub enum RuleMatch {
    /// `NAME`, or `.NAME` for the name and each name under it.
    Domain(String),
    /// One or more IPv4 or IPv6 prefixes (`cidr:NET`, or `cidr:@FILE`).
    Cidr(Arc<[Net]>),
    /// The ports `lo` to `hi` of any host, on a TCP connection (`port:N`,
    /// `port:N-M`).
    Port(u16, u16),
    /// One host and one port, on a TCP connection (`tcp:HOST:PORT`).
    Tcp(TcpHost, u16),
}

/// The host of a `tcp:HOST:PORT` rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TcpHost {
    /// A name, in lower case, with no dot at the end. It matches the same
    /// name only: no lookup.
    Name(String),
    /// An IPv4 address, or an IPv6 address in brackets in the rule.
    Ip(IpAddr),
}

impl RuleMatch {
    /// `domain:NAME`, `domain:.NAME`, `cidr:ADDR/PREFIX` (IPv4 or IPv6),
    /// `cidr:@FILE` (one prefix on each line; `#` starts a comment),
    /// `port:N` or `port:N-M` (1 to 65535), or `tcp:HOST:PORT` (a name, an
    /// IPv4 address or `[IPv6]`).
    pub fn parse(spec: &str) -> Result<RuleMatch, Failure> {
        let (kind, value) = spec
            .split_once(':')
            .ok_or_else(|| usage(format!("{spec:?}: give KIND:VALUE (domain, cidr, port, tcp)")))?;
        match kind {
            "domain" if !value.is_empty() && !value.contains(['/', ' ', ':']) => {
                Ok(RuleMatch::Domain(value.to_ascii_lowercase()))
            }
            "domain" => Err(usage(format!("{spec:?}: give domain:NAME or domain:.NAME"))),
            "cidr" => match value.strip_prefix('@') {
                Some(path) => Ok(RuleMatch::Cidr(load_nets(std::path::Path::new(path))?.into())),
                None => Ok(RuleMatch::Cidr(vec![Net::parse(value)?].into())),
            },
            "port" => parse_ports(value).map(|(lo, hi)| RuleMatch::Port(lo, hi)),
            "tcp" => parse_tcp(value).map(|(host, port)| RuleMatch::Tcp(host, port)),
            other => Err(usage(format!("unknown rule kind {other:?} (domain, cidr, port, tcp)"))),
        }
    }

    /// True when `host` (a name, or an address in text) or `ip`, with
    /// `port`, matches. `port` is `None` for a destination that is not a
    /// TCP connection (a UDP association): no port rule and no tcp rule
    /// match it.
    pub fn matches(&self, host: &str, ip: Option<IpAddr>, port: Option<u16>) -> bool {
        match self {
            RuleMatch::Domain(d) => ip.is_none() && domain_matches(d, host),
            RuleMatch::Cidr(nets) => ip.is_some_and(|ip| nets.iter().any(|n| n.contains(ip))),
            RuleMatch::Port(lo, hi) => port.is_some_and(|p| (*lo..=*hi).contains(&p)),
            RuleMatch::Tcp(want, p) => {
                port == Some(*p)
                    && match (want, ip) {
                        (TcpHost::Ip(want), Some(ip)) => want.to_canonical() == ip.to_canonical(),
                        (TcpHost::Name(want), None) => *want == host.trim_end_matches('.').to_ascii_lowercase(),
                        _ => false,
                    }
            }
        }
    }

    /// True when the rule matches addresses, so that a name is resolved
    /// for it: a cidr rule, or a tcp rule of an address.
    pub fn takes_addresses(&self) -> bool {
        matches!(self, RuleMatch::Cidr(_) | RuleMatch::Tcp(TcpHost::Ip(_), _))
    }
}

/// One port, 1 to 65535.
fn parse_port(text: &str) -> Option<u16> {
    text.parse::<u16>().ok().filter(|p| *p != 0)
}

/// `N` or `N-M`, each 1 to 65535, `N` not more than `M`.
fn parse_ports(value: &str) -> Result<(u16, u16), Failure> {
    let bad = || usage(format!("port:{value}: give port:N or port:N-M (1 to 65535, N not more than M)"));
    let (lo, hi) = value.split_once('-').unwrap_or((value, value));
    match (parse_port(lo), parse_port(hi)) {
        (Some(lo), Some(hi)) if lo <= hi => Ok((lo, hi)),
        _ => Err(bad()),
    }
}

/// `HOST:PORT`: a name, an IPv4 address or `[IPv6]`, and one port.
fn parse_tcp(value: &str) -> Result<(TcpHost, u16), Failure> {
    let bad = || usage(format!("tcp:{value}: give tcp:HOST:PORT (a name, an IPv4 address or [IPv6]; 1 to 65535)"));
    let (host, port) = match value.strip_prefix('[') {
        Some(rest) => {
            let (v6, port) = rest.split_once("]:").ok_or_else(bad)?;
            (TcpHost::Ip(IpAddr::V6(v6.parse::<Ipv6Addr>().map_err(|_| bad())?)), port)
        }
        None => {
            let (host, port) = value.rsplit_once(':').ok_or_else(bad)?;
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            let host = match host.parse::<Ipv4Addr>() {
                Ok(v4) => TcpHost::Ip(IpAddr::V4(v4)),
                // An IPv6 address with no brackets has more than one ':'.
                Err(_) if host.is_empty() || host.starts_with('.') || host.contains(['/', ' ', ':', '[', ']']) => {
                    return Err(bad());
                }
                Err(_) => TcpHost::Name(host),
            };
            (host, port)
        }
    };
    Ok((host, parse_port(port).ok_or_else(bad)?))
}

/// An IPv4 or an IPv6 prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Net {
    pub addr: IpAddr,
    pub prefix: u8,
}

impl Net {
    /// `ADDR/PREFIX`, or `ADDR` (one address).
    pub fn parse(text: &str) -> Result<Net, Failure> {
        let text = text.trim();
        let (addr, prefix) = match text.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (text, None),
        };
        let addr: IpAddr = addr
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse()
            .map_err(|_| usage(format!("{text:?} is not ADDR/PREFIX")))?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(p) => p
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= max)
                .ok_or_else(|| usage(format!("{text:?}: the prefix is 0 to {max}")))?,
            None => max,
        };
        Ok(Net { addr, prefix })
    }

    /// True when `ip` is in the prefix. An IPv4-mapped IPv6 address is taken
    /// as its IPv4 address.
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip.to_canonical()) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => in_cidr(ip, net, self.prefix),
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                if self.prefix == 0 {
                    return true;
                }
                let mask = u128::MAX << (128 - u32::from(self.prefix));
                (u128::from(ip) & mask) == (u128::from(net) & mask)
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

/// The most prefixes of one file.
pub const MAX_NETS: usize = 1_000_000;

/// The prefixes of a file: one on each line; a blank line, and the text
/// after `#`, are ignored. A bad line is a failure with its number.
pub fn load_nets(path: &std::path::Path) -> Result<Vec<Net>, Failure> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        let text = format!("reading the prefix file {}", path.display());
        Failure::caused(Stage::Warp, Class::Usage, text, e)
    })?;
    let mut nets = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if nets.len() == MAX_NETS {
            return Err(usage(format!("{}: more than {MAX_NETS} prefixes", path.display())));
        }
        let net = Net::parse(line).map_err(|e| usage(format!("{}:{}: {e}", path.display(), n + 1)))?;
        nets.push(net);
    }
    if nets.is_empty() {
        return Err(usage(format!("{}: no prefix", path.display())));
    }
    Ok(nets)
}

/// True when `ip` is in `net/prefix`.
pub fn in_cidr(ip: Ipv4Addr, net: Ipv4Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let mask = u32::MAX << (32 - u32::from(prefix));
    (u32::from(ip) & mask) == (u32::from(net) & mask)
}

/// A domain rule: `NAME` matches the name only; `.NAME` matches the name
/// and each name under it (`a.NAME`), not `NAMEx` or `NAME.evil`.
pub fn domain_matches(rule: &str, host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let rule = rule.to_ascii_lowercase();
    match rule.strip_prefix('.') {
        Some(base) => host == base || host.ends_with(&format!(".{base}")),
        None => host == rule.trim_end_matches('.'),
    }
}

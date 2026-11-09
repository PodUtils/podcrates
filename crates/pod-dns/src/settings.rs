//! The settings of a [`crate::Resolver`], which the consumer fills once: in
//! place of the variables, the constants and the process state of the
//! sources. Each field that differs between podssh and podnet has podssh's
//! value by default; the README gives podnet's.

use std::net::IpAddr;
use std::time::Duration;

use pod_tls::Trust;

use crate::doh::{self, Doh, Names};

/// The bound of one resolver's answers (podssh `DOH_EACH`; podnet's bound
/// of one step, `STEP_TIMEOUT`).
pub const EACH: Duration = Duration::from_secs(5);

/// The bound of the connection to one resolver (podssh `DOH_CONNECT`): an
/// address that a firewall drops costs this, not [`EACH`], before the next
/// resolver is asked.
pub const CONNECT: Duration = Duration::from_secs(3);

/// What an answer that a name does not exist (NXDOMAIN) does to a lookup of
/// addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Nxdomain {
    /// It ends the lookup (podssh): it holds for every record type and every
    /// resolver.
    #[default]
    End,
    /// The other addresses of its operator are passed over, and the next
    /// operator is asked (podnet): a new name is NXDOMAIN at one operator for
    /// minutes, and has an address at the other.
    NextOperator,
}

/// The settings of a resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The resolvers of DNS over HTTPS, in the order asked ([`doh::defaults`]).
    pub resolvers: Vec<Doh>,
    /// Where the names of the resolvers go in the order through a way that
    /// resolves names.
    pub names: Names,
    /// The pins: a name and one of its addresses each, in the order given; a
    /// name may come more than once ([`parse_pins`]). A pinned name is not
    /// asked of the system. The first table of a [`crate::Resolver`], which
    /// the caller replaces at run time ([`crate::Resolver::set_pins`]).
    pub pins: Vec<(String, IpAddr)>,
    /// The bound of one resolver's answers.
    pub each: Duration,
    /// The bound of the connection to one resolver asked for addresses.
    pub connect: Duration,
    /// The bound of the system resolver; `None`: the deadline of the lookup.
    pub system: Option<Duration>,
    /// Ask for AAAA records also when the A records gave addresses, and give
    /// both; `false`: AAAA only when A gave none.
    pub both: bool,
    /// What NXDOMAIN does to a lookup of addresses.
    pub nxdomain: Nxdomain,
    /// Use the cache of the addresses that carried a session ([`crate::Memory`]);
    /// `false`: the cache keeps nothing and gives nothing.
    pub memory: bool,
    /// The trust of the certificates of the resolvers.
    pub trust: Trust,
    /// The `User-Agent` of a question over HTTP/1.1; none when empty.
    pub user_agent: String,
}

impl Default for Settings {
    /// podssh's settings: the [`doh::defaults`] resolvers, the names first,
    /// no pin, [`EACH`] and [`CONNECT`], the deadline for the system
    /// resolver, AAAA only when A gave none, NXDOMAIN ends the lookup, the
    /// cache on, the default trust and no user agent.
    fn default() -> Settings {
        Settings {
            resolvers: doh::defaults(),
            names: Names::First,
            pins: Vec::new(),
            each: EACH,
            connect: CONNECT,
            system: None,
            both: false,
            nxdomain: Nxdomain::End,
            memory: true,
            trust: Trust::Default,
            user_agent: String::new(),
        }
    }
}

impl Settings {
    /// The addresses pinned for `host`, in the order given.
    pub fn pinned(&self, host: &str) -> Vec<IpAddr> {
        let host = key(host);
        self.pins.iter().filter(|(h, _)| *h == host).map(|(_, ip)| *ip).collect()
    }
}

/// A name as the pins and the cache keep it: lower case, no final dot.
pub(crate) fn key(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// `HOST=IP[,HOST=IP...]`; a host may appear more than once. IPv6 addresses
/// may be bracketed.
pub fn parse_pins(text: &str) -> Result<Vec<(String, IpAddr)>, String> {
    let mut pins = Vec::new();
    for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (host, ip) = item.split_once('=').ok_or_else(|| format!("{item:?} is not HOST=IP"))?;
        let host = key(host.trim());
        if host.is_empty() || !crate::dns_safe(&host) {
            return Err(format!("{item:?}: not a host name"));
        }
        let ip: IpAddr = ip
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse()
            .map_err(|_| format!("{item:?}: not an IP address"))?;
        pins.push((host, ip));
    }
    Ok(pins)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// podnet's pins: one host may have several entries, in any case and
    /// with a final dot; what is not `HOST=IP` is refused.
    #[test]
    fn pins() {
        let pins = parse_pins("api.trycloudflare.com=104.16.1.1, Api.TryCloudflare.com.=[2606::1],x=1.2.3.4").unwrap();
        let settings = Settings { pins, ..Settings::default() };
        assert_eq!(settings.pinned("api.trycloudflare.com").len(), 2);
        assert_eq!(settings.pinned("x"), vec!["1.2.3.4".parse::<IpAddr>().unwrap()]);
        for bad in ["noequals", "=1.2.3.4", "h=notanip"] {
            assert!(parse_pins(bad).is_err(), "{bad}");
        }
    }
}

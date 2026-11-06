//! The settings of a [`crate::Resolver`], which the consumer fills once: in
//! place of the variables, the constants and the process state of podssh.

use std::net::IpAddr;
use std::time::Duration;

use pod_tls::Trust;

use crate::doh::{self, Doh};

/// The bound of one resolver's answers (podssh `DOH_EACH`; podnet's bound
/// of one step, `STEP_TIMEOUT`).
pub const EACH: Duration = Duration::from_secs(5);

/// The bound of the connection to one resolver (podssh `DOH_CONNECT`): an
/// address that a firewall drops costs this, not [`EACH`], before the next
/// resolver is asked.
pub const CONNECT: Duration = Duration::from_secs(3);

/// The settings of a resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The resolvers of DNS over HTTPS, in the order asked ([`doh::defaults`]).
    pub resolvers: Vec<Doh>,
    /// The pins: a name and one of its addresses each, in the order given; a
    /// name may come more than once ([`parse_pins`]). A pinned name is not
    /// asked of the system.
    pub pins: Vec<(String, IpAddr)>,
    /// The bound of one resolver's answers.
    pub each: Duration,
    /// The bound of the connection to one resolver asked for addresses.
    pub connect: Duration,
    /// Use the cache of the addresses that carried a session ([`crate::Memory`]);
    /// `false`: the cache keeps nothing and gives nothing.
    pub memory: bool,
    /// The trust of the certificates of the resolvers.
    pub trust: Trust,
    /// The `User-Agent` of a question over HTTP/1.1; none when empty.
    pub user_agent: String,
}

impl Default for Settings {
    /// podssh's settings: the [`doh::defaults`] resolvers, no pin, [`EACH`]
    /// and [`CONNECT`], the cache on, the default trust and no user agent.
    fn default() -> Settings {
        Settings {
            resolvers: doh::defaults(),
            pins: Vec::new(),
            each: EACH,
            connect: CONNECT,
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

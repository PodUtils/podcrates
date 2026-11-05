//! The proxy that the variables of an environment name: `https_proxy`,
//! `HTTPS_PROXY`, `all_proxy`, `ALL_PROXY`, minus what `no_proxy` or
//! `NO_PROXY` excludes, and minus the loopback.
//!
//! The crate reads no variable of its own: [`Environment::read`] reads
//! them through the reader that the caller gives, only when the caller
//! asks, and keeps the name of each variable that gave a value.
//!
//! On the hosts that the consumers are made for, the proxy is often the
//! only way out: a direct `connect()` is refused or dropped and DNS does not
//! resolve, while the proxy accepts `CONNECT name:443` and resolves the name
//! itself. So the proxy path sends the target's *name*, never an address.

use std::ffi::OsString;
use std::fmt;
use std::net::IpAddr;

use crate::host::split_host_port;
use crate::proxy::Proxy;

/// The variables that name a proxy, in the order that they are read: the
/// first that is set, and not empty, is the proxy.
pub const PROXY_VARS: [&str; 4] = ["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"];

/// The variables that list the hosts that bypass the proxy, in the order
/// that they are read.
pub const NO_PROXY_VARS: [&str; 2] = ["no_proxy", "NO_PROXY"];

/// Where a value of the proxy settings came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A variable of the environment that other programs read too
    /// (`https_proxy`, `NO_PROXY`), by its name.
    Variable(String),
    /// A value that the user gave to the consumer on purpose: an option or
    /// a variable of its own (`--proxy`), by the name that the consumer
    /// gives it.
    User(String),
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Origin::Variable(name) | Origin::User(name) => f.write_str(name),
        }
    }
}

/// A value of the proxy settings, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Given<T> {
    pub value: T,
    pub origin: Origin,
}

/// The proxy settings of an environment, as the caller's reader gave them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    /// The proxy of the first variable of [`PROXY_VARS`] that is set and
    /// not empty, or why its URL cannot be used.
    pub proxy: Option<Given<Result<Proxy, String>>>,
    /// The list of the first variable of [`NO_PROXY_VARS`] that is set and
    /// not empty.
    pub no_proxy: Option<Given<String>>,
}

impl Environment {
    /// Read [`PROXY_VARS`] and [`NO_PROXY_VARS`] through `var`: a consumer
    /// passes `&|name| std::env::var_os(name)`. Lower-case names win over
    /// upper-case ones, as in curl; an empty value, and one that is not
    /// UTF-8, counts as unset.
    pub fn read(var: &dyn Fn(&str) -> Option<OsString>) -> Environment {
        let first = |names: &[&str]| {
            names.iter().find_map(|name| {
                let value = var(name)?.into_string().ok().filter(|v| !v.trim().is_empty())?;
                Some(Given { value, origin: Origin::Variable(name.to_string()) })
            })
        };
        let proxy = first(&PROXY_VARS).map(|g| Given { value: Proxy::parse(&g.value), origin: g.origin });
        Environment { proxy, no_proxy: first(&NO_PROXY_VARS) }
    }

    /// The proxy for `target_host`, or none: a loopback target never goes
    /// through a proxy (as in Go's net/http), as the proxy's loopback is not
    /// this host's, and podvm measured a proxy that answered 405 to
    /// `CONNECT 127.0.0.1`. A name that the caller pinned to loopback
    /// addresses alone (`pinned`, the pins of `target_host`) is such a
    /// target too; a pin to any other address keeps the proxy, which is
    /// given the name. Then what the `NO_PROXY` list excludes. `Err` when
    /// the URL of the proxy cannot be used, with why.
    pub fn proxy_for(&self, target_host: &str, pinned: &[IpAddr]) -> Result<Option<Proxy>, String> {
        if is_loopback(target_host) || (!pinned.is_empty() && pinned.iter().all(IpAddr::is_loopback)) {
            return Ok(None);
        }
        let Some(proxy) = &self.proxy else { return Ok(None) };
        if self.no_proxy_entry(target_host).is_some() {
            return Ok(None);
        }
        proxy.value.clone().map(Some)
    }

    /// The first entry of the `NO_PROXY` list that excludes `host`
    /// ([`no_proxy_entry`]).
    pub fn no_proxy_entry(&self, host: &str) -> Option<&str> {
        self.no_proxy.as_ref().and_then(|list| no_proxy_entry(&list.value, host))
    }
}

/// The proxy that the variables of `var` select for `target_host`:
/// [`Environment::read`], then [`Environment::proxy_for`] with no pin.
pub fn proxy_from_vars(target_host: &str, var: &dyn Fn(&str) -> Option<OsString>) -> Result<Option<Proxy>, String> {
    Environment::read(var).proxy_for(target_host, &[])
}

/// `localhost`, `*.localhost`, `127.0.0.0/8` and `::1`, with or without
/// brackets.
pub fn is_loopback(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']').trim_end_matches('.').to_ascii_lowercase();
    if h == "localhost" || h.ends_with(".localhost") {
        return true;
    }
    h.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Whether a `no_proxy` list excludes `host`. Entries are separated by commas
/// or whitespace; `*` matches everything; `example.com`, `.example.com` and
/// `*.example.com` all match `example.com` and its subdomains; a `:port`
/// suffix on an entry is ignored. Matching is case-insensitive. A block
/// `ADDR/BITS`, IPv4 or IPv6, matches each address inside it, as in curl
/// (7.86 on), Go and Python's requests; a name is never resolved for it.
pub fn no_proxy_matches(list: &str, host: &str) -> bool {
    no_proxy_entry(list, host).is_some()
}

/// The first entry of a `no_proxy` list that excludes `host`, by the rules of
/// [`no_proxy_matches`]: a doctor names it.
pub fn no_proxy_entry<'a>(list: &'a str, host: &str) -> Option<&'a str> {
    let host = host.trim_start_matches('[').trim_end_matches(']').trim_end_matches('.').to_ascii_lowercase();
    list.split(|c: char| c == ',' || c.is_whitespace()).map(str::trim).filter(|e| !e.is_empty()).find(|raw| {
        if *raw == "*" || in_block(raw, &host) {
            return true;
        }
        let entry = match split_host_port(raw) {
            Ok((h, _)) => h,
            Err(_) => raw.to_string(),
        };
        let entry = entry.trim_start_matches("*.").trim_start_matches('.').trim_end_matches('.').to_ascii_lowercase();
        !entry.is_empty() && (host == entry || host.ends_with(&format!(".{entry}")))
    })
}

/// Whether `host`, an address, lies inside the block `entry` (`ADDR/BITS`). An
/// IPv4 address written as IPv6 (`::ffff:10.1.2.3`) is the IPv4 address.
fn in_block(entry: &str, host: &str) -> bool {
    let Some((net, bits)) = entry.split_once('/') else { return false };
    let (Ok(net), Ok(bits)) =
        (net.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>(), bits.parse::<u32>())
    else {
        return false;
    };
    let Ok(addr) = host.parse::<IpAddr>() else { return false };
    let addr = match addr {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(addr, IpAddr::V4),
        v4 => v4,
    };
    match (net, addr) {
        (IpAddr::V4(net), IpAddr::V4(addr)) if bits <= 32 => {
            let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
            u32::from(net) & mask == u32::from(addr) & mask
        }
        (IpAddr::V6(net), IpAddr::V6(addr)) if bits <= 128 => {
            let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
            u128::from(net) & mask == u128::from(addr) & mask
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |name| pairs.iter().find(|(n, _)| *n == name).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn the_environment_keeps_the_variable_of_each_value() {
        let env = Environment::read(&vars(&[("HTTPS_PROXY", "http://proxy.example:3128"), ("no_proxy", "10.0.0.0/8")]));
        let proxy = env.proxy.as_ref().expect("a proxy");
        assert_eq!(proxy.origin, Origin::Variable("HTTPS_PROXY".into()));
        assert_eq!(proxy.value.as_ref().map(|p| p.port), Ok(3128));
        assert_eq!(env.no_proxy.as_ref().map(|l| l.origin.to_string()).as_deref(), Some("no_proxy"));
        assert_eq!(env.no_proxy_entry("10.1.2.3"), Some("10.0.0.0/8"));
        assert_eq!(env.proxy_for("10.1.2.3", &[]), Ok(None));
        assert!(env.proxy_for("relay.example", &[]).unwrap().is_some());
        assert_eq!(Environment::read(&vars(&[])), Environment::default());
    }

    /// A URL that cannot be used is kept with why, and a target that goes
    /// directly never meets it.
    #[test]
    fn a_bad_url_is_an_error_only_for_a_target_that_takes_the_proxy() {
        let env = Environment::read(&vars(&[("ALL_PROXY", "ftp://proxy.example"), ("NO_PROXY", "example.com")]));
        assert!(env.proxy_for("relay.test", &[]).unwrap_err().contains("not supported"));
        assert_eq!(env.proxy_for("relay.example.com", &[]), Ok(None));
        assert_eq!(env.proxy_for("localhost", &[]), Ok(None));
    }

    /// A value that is not UTF-8 counts as unset, as `std::env::var` reads
    /// it.
    #[cfg(unix)]
    #[test]
    fn a_value_that_is_not_utf8_is_unset() {
        use std::os::unix::ffi::OsStringExt;
        let var = |name: &str| match name {
            "https_proxy" => Some(OsString::from_vec(vec![0xff, 0xfe])),
            "HTTPS_PROXY" => Some(OsString::from("http://second.example:8080")),
            _ => None,
        };
        let env = Environment::read(&var);
        assert_eq!(env.proxy.map(|p| p.origin), Some(Origin::Variable("HTTPS_PROXY".into())));
    }
}

//! The rules of a `NO_PROXY` list, and the loopback.

use std::net::IpAddr;

use crate::host::split_host_port;

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
/// suffix on an entry is ignored. Matching is case-insensitive, and an
/// address matches the same address in another form (`::1` and
/// `0:0:0:0:0:0:0:1`). A block `ADDR/BITS`, IPv4 or IPv6, matches each
/// address inside it, as in curl (7.86 on), Go and Python's requests; a name
/// is never resolved for it.
pub fn no_proxy_matches(list: &str, host: &str) -> bool {
    no_proxy_entry(list, host).is_some()
}

/// The first entry of a `no_proxy` list that excludes `host`, by the rules of
/// [`no_proxy_matches`]: a doctor names it.
pub fn no_proxy_entry<'a>(list: &'a str, host: &str) -> Option<&'a str> {
    no_proxy_entry_at(list, host, None)
}

/// [`no_proxy_entry`] for `host:port`: with a port, an entry with a port
/// (`internal:8443`, `[::1]:22`) excludes that port only; with none, the
/// port of an entry is passed over.
pub fn no_proxy_entry_at<'a>(list: &'a str, host: &str, port: Option<u16>) -> Option<&'a str> {
    let host = host.trim_start_matches('[').trim_end_matches(']').trim_end_matches('.').to_ascii_lowercase();
    let host_ip = host.parse::<IpAddr>().ok().map(unmapped);
    list.split(|c: char| c == ',' || c.is_whitespace()).map(str::trim).filter(|e| !e.is_empty()).find(|raw| {
        if *raw == "*" || in_block(raw, &host) {
            return true;
        }
        let (entry, entry_port) = match split_host_port(raw) {
            Ok((h, p)) => (h, p),
            Err(_) => (raw.to_string(), None),
        };
        if let (Some(want), Some(got)) = (port, entry_port)
            && want != got
        {
            return false;
        }
        let entry = entry.trim_start_matches("*.").trim_start_matches('.').trim_end_matches('.').to_ascii_lowercase();
        if let (Some(host_ip), Ok(entry_ip)) = (host_ip, entry.parse::<IpAddr>()) {
            return host_ip == unmapped(entry_ip);
        }
        !entry.is_empty() && (host == entry || host.ends_with(&format!(".{entry}")))
    })
}

/// An IPv4 address written as IPv6 (`::ffff:10.1.2.3`) is the IPv4 address.
fn unmapped(addr: IpAddr) -> IpAddr {
    match addr {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(addr, IpAddr::V4),
        v4 => v4,
    }
}

/// Whether `host`, an address, lies inside the block `entry` (`ADDR/BITS`).
fn in_block(entry: &str, host: &str) -> bool {
    let Some((net, bits)) = entry.split_once('/') else { return false };
    let (Ok(net), Ok(bits)) =
        (net.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>(), bits.parse::<u32>())
    else {
        return false;
    };
    let Ok(addr) = host.parse::<IpAddr>() else { return false };
    match (net, unmapped(addr)) {
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

/// A `NO_PROXY` list whose entries with a port exclude that port only, as
/// podnet reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NoProxy {
    list: String,
}

impl NoProxy {
    /// The list, entries separated by commas or whitespace.
    pub fn parse(list: &str) -> NoProxy {
        NoProxy { list: list.to_string() }
    }

    /// True when `host:port` does not use the proxy.
    pub fn matches(&self, host: &str, port: u16) -> bool {
        no_proxy_entry_at(&self.list, host, Some(port)).is_some()
    }
}

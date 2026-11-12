//! The names of the WARP commands (the proxy, the forward, nc): a literal
//! address or a pin of the caller first, then DNS questions through the
//! tunnel (PodUtils/podnet issue 81); and the connect to the addresses of
//! a name, IPv4 first.
//!
//! The pins of the caller (podnet's `--resolve` and `PODNET_RESOLVE`,
//! T-073) come before any question.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pod_core::{BoxFuture, Class, Connected, Dial, Failure, Stage, Target};
use pod_dns::wire;

use crate::resolver::Resolvers;
use crate::rules::Lookup;

/// The pins of the caller: the addresses of a name with no question, or
/// none.
pub type Pins = Arc<dyn Fn(&str) -> Vec<IpAddr> + Send + Sync>;

/// The resolvers inside the tunnel and the caller's pins.
#[derive(Clone, Default)]
pub struct Names {
    pub resolvers: Resolvers,
    pub pins: Option<Pins>,
}

impl std::fmt::Debug for Names {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Names").field("resolvers", &self.resolvers).field("pins", &self.pins.is_some()).finish()
    }
}

/// The order of the dial: IPv4 addresses, then IPv6, each in the order of
/// the answer, with no copy.
pub fn order(addresses: Vec<IpAddr>) -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = Vec::new();
    for ip in addresses.iter().filter(|a| a.is_ipv4()).chain(addresses.iter().filter(|a| a.is_ipv6())) {
        if !out.contains(ip) {
            out.push(*ip);
        }
    }
    out
}

/// The addresses of `name` that need no question, in the order of the
/// dial: a literal address, else the pins `pins` of `name`.
fn known(name: &str, pins: Vec<IpAddr>) -> Option<Vec<IpAddr>> {
    if let Ok(ip) = name.trim_matches(|c| c == '[' || c == ']').parse::<IpAddr>() {
        return Some(vec![ip]);
    }
    (!pins.is_empty()).then(|| order(pins))
}

fn no_address(name: &str, causes: &[&Failure]) -> Failure {
    let class = match causes.iter().all(|f| f.class == Class::NotFound) && !causes.is_empty() {
        true => Class::NotFound,
        false => Class::Unavailable,
    };
    let why: Vec<String> = causes.iter().map(|f| f.chain()).collect();
    let text = match why.is_empty() {
        true => format!("no address for {name} through WARP"),
        false => format!("no address for {name} through WARP ({})", why.join("; ")),
    };
    Failure::new(Stage::Dns, class, text)
}

impl Names {
    fn pinned(&self, name: &str) -> Vec<IpAddr> {
        self.pins.as_ref().map(|pins| pins(name)).unwrap_or_default()
    }

    /// The A and the AAAA addresses of `name`, in the order of the dial:
    /// its pins, else two questions at once through `net`.
    pub async fn resolve_all(&self, net: &dyn Dial, name: &str) -> Result<Vec<IpAddr>, Failure> {
        if let Some(known) = known(name, self.pinned(name)) {
            return Ok(known);
        }
        let (a, aaaa) =
            tokio::join!(self.resolvers.query(net, name, wire::A), self.resolvers.query(net, name, wire::AAAA));
        let mut all: Vec<IpAddr> = a.as_ref().map(Vec::clone).unwrap_or_default();
        all.extend(aaaa.as_ref().map(Vec::clone).unwrap_or_default());
        if all.is_empty() {
            let causes: Vec<&Failure> = [a.as_ref().err(), aaaa.as_ref().err()].into_iter().flatten().collect();
            return Err(no_address(name, &causes));
        }
        Ok(order(all))
    }

    /// The first address of `name` (IPv4 first): its first pin, else DNS
    /// through `net`. The AAAA question goes only when there is no A record
    /// (PodUtils/podnet issue 114: a lost AAAA answer delayed each name by
    /// the time limit).
    pub async fn resolve(&self, net: &dyn Dial, name: &str) -> Result<IpAddr, Failure> {
        if let Some(known) = known(name, self.pinned(name)) {
            return Ok(known[0]);
        }
        let a = self.resolvers.query(net, name, wire::A).await;
        if let Some(ip) = a.as_ref().ok().and_then(|a| a.first()) {
            return Ok(*ip);
        }
        let aaaa = self.resolvers.query(net, name, wire::AAAA).await;
        match (&a, &aaaa) {
            (_, Ok(aaaa)) if !aaaa.is_empty() => Ok(aaaa[0]),
            _ => {
                let causes: Vec<&Failure> = [a.as_ref().err(), aaaa.as_ref().err()].into_iter().flatten().collect();
                Err(no_address(name, &causes))
            }
        }
    }

    /// A TCP stream to `target` through `net`: the addresses of its name
    /// ([`Names::resolve_all`]), IPv4 first (issue 81: a host with no A
    /// record works too), each with `limit` to connect.
    pub async fn connect(&self, net: &dyn Dial, target: &Target, limit: Duration) -> Result<Connected, Failure> {
        let addresses = match target {
            Target::Addr(addr) => vec![addr.ip()],
            Target::Name { host, .. } => self.resolve_all(net, host).await?,
        };
        connect_ips(net, addresses, target.port(), limit).await
    }
}

/// Connect to the first of `addresses` that answers on `port` through
/// `net`, IPv4 first.
pub(crate) async fn connect_ips(
    net: &dyn Dial,
    addresses: Vec<IpAddr>,
    port: u16,
    limit: Duration,
) -> Result<Connected, Failure> {
    let mut last = None;
    for ip in order(addresses) {
        let deadline = tokio::time::Instant::now() + limit;
        match net.dial(&Target::Addr(SocketAddr::new(ip, port)), deadline).await {
            Ok(connected) => return Ok(connected),
            Err(failure) => last = Some(failure),
        }
    }
    Err(last.unwrap_or_else(|| Failure::new(Stage::Dns, Class::Unavailable, "no address to connect to")))
}

/// The names of the proxy over one session, for the address rules.
pub(crate) struct Inside<'a> {
    pub(crate) names: &'a Names,
    pub(crate) net: &'a dyn Dial,
}

impl Lookup for Inside<'_> {
    fn lookup<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<IpAddr>, Failure>> {
        Box::pin(self.names.resolve_all(self.net, name))
    }
}

/// The addresses of the names of one UDP association, so that only the
/// first datagram to a name waits for DNS (PodUtils/podnet issue 114).
#[derive(Default)]
pub(crate) struct Cache(HashMap<String, IpAddr>);

impl Cache {
    /// The address of `target`; `None` when its name has no address.
    pub(crate) async fn address(&mut self, names: &Names, net: &dyn Dial, target: &Target) -> Option<SocketAddr> {
        let name = match target {
            Target::Addr(addr) => return Some(*addr),
            Target::Name { host, .. } => host,
        };
        if let Some(ip) = self.0.get(name) {
            return Some(SocketAddr::new(*ip, target.port()));
        }
        match names.resolve(net, name).await {
            Ok(ip) => {
                if self.0.len() >= 256 {
                    self.0.clear();
                }
                self.0.insert(name.clone(), ip);
                Some(SocketAddr::new(ip, target.port()))
            }
            Err(failure) => {
                tracing::debug!(%name, error = %failure.chain(), "a UDP name is not resolved");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// PodUtils/podnet issue 81: IPv4 first, then IPv6, no copy; an
    /// IPv6-only answer stays.
    #[test]
    fn the_dial_order() {
        let a: IpAddr = "1.2.3.4".parse().unwrap();
        let b: IpAddr = "2606:4700::1".parse().unwrap();
        let c: IpAddr = "5.6.7.8".parse().unwrap();
        assert_eq!(order(vec![b, a, c, a]), vec![a, c, b]);
        assert_eq!(order(vec![b]), vec![b]);
    }

    /// A dial that counts each use: a question or a connect through it.
    #[derive(Default)]
    struct Counted(AtomicUsize);

    impl Dial for Counted {
        fn label(&self) -> String {
            "counted".into()
        }

        fn dial<'a>(&'a self, _t: &'a Target, _d: tokio::time::Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(Failure::new(Stage::Connect, Class::Unavailable, "no way")) })
        }

        fn datagrams<'a>(&'a self) -> BoxFuture<'a, Result<Arc<dyn pod_core::Datagrams>, Failure>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(Failure::new(Stage::Connect, Class::Unavailable, "no way")) })
        }
    }

    /// T-073: a pinned name gives its pins (IPv4 first), and nothing goes
    /// into the tunnel: no DNS question.
    #[tokio::test]
    async fn warp_resolve_takes_the_pin_first() {
        let net = Counted::default();
        let pins: Pins = Arc::new(|name: &str| match name.trim_end_matches('.').to_ascii_lowercase().as_str() {
            "pinned.t073.test" => vec!["2001:db8::9".parse().unwrap(), "192.0.2.9".parse().unwrap()],
            _ => Vec::new(),
        });
        let names = Names { pins: Some(pins), ..Names::default() };
        let v4: IpAddr = "192.0.2.9".parse().unwrap();
        let v6: IpAddr = "2001:db8::9".parse().unwrap();
        assert_eq!(names.resolve_all(&net, "Pinned.T073.test.").await.unwrap(), vec![v4, v6]);
        assert_eq!(names.resolve(&net, "pinned.t073.test").await.unwrap(), v4);
        assert_eq!(names.resolve(&net, "[2001:db8::1]").await.unwrap(), "2001:db8::1".parse::<IpAddr>().unwrap());
        assert_eq!(net.0.load(Ordering::SeqCst), 0, "no DNS question goes into the tunnel");
        let failure = names.resolve_all(&net, "other.t073.test").await.unwrap_err();
        assert_eq!(failure.stage, Stage::Dns);
        assert!(failure.chain().contains("no address for other.t073.test through WARP"), "{}", failure.chain());
        assert_eq!(net.0.load(Ordering::SeqCst), 2, "an A and an AAAA question");
    }
}

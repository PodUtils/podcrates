//! A resolver of the system, with the pins that the caller gives: an IP
//! literal is its own address, a pinned name has its pins, and the system
//! resolver answers for the rest. For a consumer that needs no DNS over
//! HTTPS, and for the tests; `pod-dns` gives the whole order (the cache of
//! addresses, DNS over HTTPS).

use std::net::{IpAddr, SocketAddr};

use pod_core::{BoxFuture, Class, Failure, Found, Resolve, Source, Stage};
use tokio::time::Instant;

/// The name is an IP address.
pub const LITERAL: Source = Source { rank: 0, name: "literal" };
/// An address that the caller pinned for the name.
pub const PIN: Source = Source { rank: 1, name: "pin" };
/// The system resolver.
pub const SYSTEM: Source = Source { rank: 2, name: "system" };

/// The system resolver, after the IP literals and the pins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct System {
    pins: Vec<(String, IpAddr)>,
}

impl System {
    /// The system resolver with no pin.
    pub fn new() -> System {
        System::default()
    }

    /// The system resolver with `pins`: each a name and one of its
    /// addresses, in the order given (podssh's `--relay-addr`). A name may
    /// come more than once. A pinned name is not asked of the system.
    pub fn with_pins(pins: Vec<(String, IpAddr)>) -> System {
        System { pins: pins.into_iter().map(|(host, ip)| (key(&host), ip)).collect() }
    }
}

/// A name as the pins keep it: lower case, with no final dot.
fn key(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

impl Resolve for System {
    fn label(&self) -> String {
        "the system resolver".into()
    }

    fn resolve_from<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        deadline: Instant,
        after: Option<Source>,
    ) -> BoxFuture<'a, Result<Found, Failure>> {
        Box::pin(async move {
            // Each source is the last: nothing comes after one that answered.
            if after.is_some() {
                return Err(Failure::new(Stage::Dns, Class::Unavailable, "no source after the system resolver"));
            }
            let bare = host.trim_start_matches('[').trim_end_matches(']');
            if let Ok(ip) = bare.parse::<IpAddr>() {
                return Ok(Found { addrs: vec![SocketAddr::new(ip, port)], source: LITERAL, more: false });
            }
            let pins = self.pinned(host);
            if !pins.is_empty() {
                let addrs = pins.into_iter().map(|ip| SocketAddr::new(ip, port)).collect();
                return Ok(Found { addrs, source: PIN, more: false });
            }
            match tokio::time::timeout_at(deadline, tokio::net::lookup_host((host, port))).await {
                Ok(Ok(addrs)) => {
                    let addrs: Vec<SocketAddr> = addrs.collect();
                    if addrs.is_empty() {
                        Err(Failure::new(Stage::Dns, Class::Unavailable, "no addresses"))
                    } else {
                        Ok(Found { addrs, source: SYSTEM, more: false })
                    }
                }
                Ok(Err(e)) => Err(Failure::io(Stage::Dns, e)),
                Err(_) => Err(Failure::new(Stage::Dns, Class::Timeout, "timed out")),
            }
        })
    }

    fn pinned(&self, host: &str) -> Vec<IpAddr> {
        let host = key(host);
        self.pins.iter().filter(|(h, _)| *h == host).map(|(_, ip)| *ip).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn a_literal_and_a_pin_end_the_order_and_a_pin_ignores_case_and_the_final_dot() {
        let r = System::with_pins(vec![("Relay.Test.".into(), "192.0.2.7".parse().unwrap())]);
        let deadline = Instant::now() + Duration::from_secs(5);
        let literal = r.resolve_from("[2001:db8::1]", 443, deadline, None).await.unwrap();
        assert_eq!(
            (literal.addrs, literal.source, literal.more),
            (vec!["[2001:db8::1]:443".parse().unwrap()], LITERAL, false)
        );
        let pinned = r.resolve_from("relay.test", 22, deadline, None).await.unwrap();
        assert_eq!((pinned.addrs, pinned.source), (vec!["192.0.2.7:22".parse().unwrap()], PIN));
        assert_eq!(r.pinned("RELAY.test."), ["192.0.2.7".parse::<IpAddr>().unwrap()]);
        assert!(r.pinned("other.test").is_empty());
        let after = r.resolve_from("relay.test", 22, deadline, Some(PIN)).await.unwrap_err();
        assert_eq!((after.stage, after.class), (Stage::Dns, Class::Unavailable));
    }

    #[tokio::test]
    async fn the_system_answers_for_localhost() {
        pod_testkit::need_localhost!();
        let system = System::new();
        let found = system.resolve_from("localhost", 80, Instant::now() + Duration::from_secs(10), None);
        let found = found.await.expect("localhost resolves");
        assert_eq!(found.source, SYSTEM);
        assert!(found.addrs.iter().all(|a| a.ip().is_loopback() && a.port() == 80), "{found:?}");
    }
}

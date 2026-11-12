//! The check that a packet crosses a new session (PodUtils/podnet issue
//! 120): a DNS question to a resolver inside the tunnel gets an answer. A
//! session is not up at the answer 200 of the CONNECT: a front door can
//! take the handshake and carry no packet. [`crate::Warp::connect`] runs
//! it, and marks such a door bad (issue 133). The servers are resolvers of
//! [`crate::resolver`]: DNS on UDP, or DNS over TLS (issue 145).

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use pod_dns::wire;

use crate::net::WarpNet;
use crate::resolver::{self, Resolver};

/// The time limit of the question to one server.
pub const CHECK_LIMIT: Duration = resolver::DNS_TIMEOUT;
/// The interval of the resends of the question on UDP (PodUtils/podnet
/// issue 114: one lost datagram cost the full time limit).
pub const CHECK_RESEND: Duration = resolver::DNS_RESEND;

/// The settings of the check of the data path.
#[derive(Clone, Debug)]
pub struct DataPathCheck {
    /// The resolvers that the question goes to, one after the other, those
    /// of a family of the device; the first answer is the end (podnet's
    /// `--resolver`). Empty: the resolver of the tunnel of the first family
    /// of the device, [`crate::service::tunnel_resolver`] (1.1.1.1:53) with
    /// an IPv4 address, else `[2606:4700:4700::1111]:53`.
    pub servers: Vec<Resolver>,
    /// The TLS client of the caller, for a server over TLS.
    pub tls: Option<Arc<rustls::ClientConfig>>,
    /// The name of the question of type A: [`crate::service::check_name`].
    pub name: String,
    /// The time limit of the question to each server.
    pub limit: Duration,
    /// The time after which the question goes again on UDP.
    pub resend: Duration,
}

impl Default for DataPathCheck {
    fn default() -> DataPathCheck {
        DataPathCheck {
            servers: Vec::new(),
            tls: None,
            name: crate::service::check_name().to_string(),
            limit: CHECK_LIMIT,
            resend: CHECK_RESEND,
        }
    }
}

impl DataPathCheck {
    /// The check of the resolvers of a proxy: their list and their TLS
    /// client (podnet's `check_data_path` with `--resolver`).
    pub fn of(resolvers: &resolver::Resolvers) -> DataPathCheck {
        DataPathCheck { servers: resolvers.list.clone(), tls: resolvers.tls.clone(), ..DataPathCheck::default() }
    }

    /// The servers that a device with the addresses `ours` asks, in order.
    pub fn servers_for(&self, ours: &[IpAddr]) -> Vec<Resolver> {
        if self.servers.is_empty() {
            let server = match ours.first() {
                Some(IpAddr::V4(_)) => crate::service::tunnel_resolver(),
                _ => std::net::SocketAddr::new(crate::dns::RESOLVERS[2], crate::dns::PORT),
            };
            return vec![Resolver::Udp(server)];
        }
        let families: Vec<bool> = ours.iter().map(IpAddr::is_ipv4).collect();
        self.servers.iter().filter(|s| families.contains(&s.addr().is_ipv4())).cloned().collect()
    }
}

impl WarpNet {
    /// Ask the servers of `check` through the session, one after the other,
    /// until one answers. The time of the answer, or an `Unavailable`
    /// failure of the stage `Warp` when no server answered.
    pub async fn check_data_path(&self, check: &DataPathCheck) -> Result<Duration, Failure> {
        let start = std::time::Instant::now();
        let servers = check.servers_for(&self.addresses());
        if servers.is_empty() {
            let text = "the check of the data path has no server of a family of the WARP device";
            return Err(Failure::new(Stage::Warp, Class::Unavailable, text));
        }
        let mut reasons = Vec::new();
        for server in &servers {
            match self.ask_once(server, check).await {
                Ok(()) => {
                    let took = start.elapsed();
                    self.note_checked(took);
                    return Ok(took);
                }
                Err(why) => reasons.push(format!("{server}: {why}")),
            }
        }
        let shown: Vec<String> = servers.iter().map(Resolver::to_string).collect();
        let text = format!(
            "the MASQUE session is open, and a DNS query to {} through it got no answer ({})",
            shown.join(", "),
            reasons.join("; ")
        );
        Err(Failure::new(Stage::Warp, Class::Unavailable, text))
    }

    /// One question of type A to `server`, in at most `check.limit`; on UDP
    /// it goes again each `check.resend`. Each answer with the id of the
    /// question counts: the check is of the path, not of the name.
    async fn ask_once(&self, server: &Resolver, check: &DataPathCheck) -> Result<(), String> {
        // A server of a family that the device lacks gets no question.
        self.families().check(server.addr()).map_err(|e| e.to_string())?;
        let query = resolver::question(&check.name, wire::A)?;
        let resolvers = resolver::Resolvers {
            list: vec![server.clone()],
            tls: check.tls.clone(),
            limit: check.limit,
            resend: check.resend,
            connect_timeout: check.limit,
        };
        resolvers.exchange_with(self, server, &query).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// podnet: an IPv4 device asks 1.1.1.1, an IPv6 device the IPv6
    /// resolver; the servers of the caller go in order, those of a family of
    /// the device, on UDP or over TLS.
    #[test]
    fn the_servers_of_the_check_follow_the_families_of_the_device() {
        let check = DataPathCheck::default();
        let v4: IpAddr = "172.16.0.2".parse().unwrap();
        let v6: IpAddr = "2606:4700:110:8a36::2".parse().unwrap();
        let udp = |s: &str| Resolver::parse(s).unwrap();
        assert_eq!(check.servers_for(&[v4, v6]), vec![udp("1.1.1.1:53")]);
        assert_eq!(check.servers_for(&[v6]), vec![udp("[2606:4700:4700::1111]:53")]);
        let set = vec![udp("[2620:fe::fe]:53"), udp("9.9.9.9:53"), udp("tls://1.1.1.1")];
        let check = DataPathCheck { servers: set.clone(), ..DataPathCheck::default() };
        assert_eq!(check.servers_for(&[v4]), vec![set[1].clone(), set[2].clone()]);
        assert_eq!(check.servers_for(&[v4, v6]), set);
        assert_eq!(DataPathCheck::default().name, "cloudflare.com");
        let resolvers = resolver::Resolvers::new(vec![udp("tls://1.1.1.1")]);
        assert_eq!(DataPathCheck::of(&resolvers).servers, resolvers.list);
    }
}

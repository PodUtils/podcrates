//! The check that a packet crosses a new session (PodUtils/podnet issue
//! 120): a DNS question on UDP to a resolver inside the tunnel gets an
//! answer. A session is not up at the answer 200 of the CONNECT: a front
//! door can take the handshake and carry no packet. [`crate::Warp::connect`]
//! runs it, and marks such a door bad (issue 133).

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use pod_core::entropy::{Entropy as _, OsEntropy};
use pod_core::{Class, Failure, Stage};
use pod_dns::wire;

use crate::net::WarpNet;

/// The time limit of the question to one server.
pub const CHECK_LIMIT: Duration = Duration::from_secs(5);
/// The interval of the resends of the question (PodUtils/podnet issue 114:
/// one lost datagram cost the full time limit).
pub const CHECK_RESEND: Duration = Duration::from_millis(1200);

/// The settings of the check of the data path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataPathCheck {
    /// The resolvers that the question goes to, one after the other, those
    /// of a family of the device; the first answer is the end (podnet's
    /// `--resolver`). Empty: the resolver of the tunnel of the first family
    /// of the device, [`crate::service::tunnel_resolver`] (1.1.1.1:53) with
    /// an IPv4 address, else `[2606:4700:4700::1111]:53`.
    pub servers: Vec<SocketAddr>,
    /// The name of the question of type A: [`crate::service::check_name`].
    pub name: String,
    /// The time limit of the question to each server.
    pub limit: Duration,
    /// The time after which the question goes again.
    pub resend: Duration,
}

impl Default for DataPathCheck {
    fn default() -> DataPathCheck {
        DataPathCheck {
            servers: Vec::new(),
            name: crate::service::check_name().to_string(),
            limit: CHECK_LIMIT,
            resend: CHECK_RESEND,
        }
    }
}

impl DataPathCheck {
    /// The servers that a device with the addresses `ours` asks, in order.
    pub fn servers_for(&self, ours: &[IpAddr]) -> Vec<SocketAddr> {
        if self.servers.is_empty() {
            let server = match ours.first() {
                Some(IpAddr::V4(_)) => crate::service::tunnel_resolver(),
                _ => SocketAddr::new(crate::dns::RESOLVERS[2], crate::dns::PORT),
            };
            return vec![server];
        }
        let families: Vec<bool> = ours.iter().map(IpAddr::is_ipv4).collect();
        self.servers.iter().copied().filter(|s| families.contains(&s.is_ipv4())).collect()
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
            match self.ask_once(*server, check).await {
                Ok(()) => {
                    let took = start.elapsed();
                    self.note_checked(took);
                    return Ok(took);
                }
                Err(why) => reasons.push(format!("{server}: {why}")),
            }
        }
        let shown: Vec<String> = servers.iter().map(SocketAddr::to_string).collect();
        let text = format!(
            "the MASQUE session is open, and a DNS query to {} through it got no answer ({})",
            shown.join(", "),
            reasons.join("; ")
        );
        Err(Failure::new(Stage::Warp, Class::Unavailable, text))
    }

    /// One question of type A to `server`, sent again each `check.resend`
    /// until an answer with its id comes from `server`, in at most
    /// `check.limit`. Each answer counts: the check is of the path, not of
    /// the name.
    async fn ask_once(&self, server: SocketAddr, check: &DataPathCheck) -> Result<(), String> {
        let mut socket = self.bind_udp().await.map_err(|e| e.chain())?;
        let mut id = [0x5a, 0x5a];
        let _ = OsEntropy.fill(&mut id);
        let mut query = wire::query(&check.name, wire::A)?;
        query[..2].copy_from_slice(&id);
        let deadline = tokio::time::Instant::now() + check.limit;
        let mut buf = vec![0u8; 2048];
        loop {
            socket.send_to(&query, server).await.map_err(|e| e.to_string())?;
            let resend = (tokio::time::Instant::now() + check.resend).min(deadline);
            loop {
                match tokio::time::timeout_at(resend, socket.recv_from(&mut buf)).await {
                    Ok(Ok((n, from))) if from == server && n >= 2 && buf[..2] == id => return Ok(()),
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => return Err(e.to_string()),
                    Err(_) => break,
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(format!("no answer in {} ms", check.limit.as_millis()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// podnet: an IPv4 device asks 1.1.1.1, an IPv6 device the IPv6
    /// resolver; the servers of the caller go in order, those of a family of
    /// the device.
    #[test]
    fn the_servers_of_the_check_follow_the_families_of_the_device() {
        let check = DataPathCheck::default();
        let v4: IpAddr = "172.16.0.2".parse().unwrap();
        let v6: IpAddr = "2606:4700:110:8a36::2".parse().unwrap();
        assert_eq!(check.servers_for(&[v4, v6]), vec!["1.1.1.1:53".parse().unwrap()]);
        assert_eq!(check.servers_for(&[v6]), vec!["[2606:4700:4700::1111]:53".parse().unwrap()]);
        let set: Vec<SocketAddr> = vec!["[2620:fe::fe]:53".parse().unwrap(), "9.9.9.9:53".parse().unwrap()];
        let check = DataPathCheck { servers: set.clone(), ..DataPathCheck::default() };
        assert_eq!(check.servers_for(&[v4]), vec![set[1]]);
        assert_eq!(check.servers_for(&[v4, v6]), set);
        assert_eq!(DataPathCheck::default().name, "cloudflare.com");
    }
}

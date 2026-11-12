//! The DNS forwarder of a proxy (podnet's `warp proxy --dns`, issue 117)
//! with a bound (PodUtils/podnet@T-215): at most [`MAX_DNS_TASKS`]
//! questions are in flight, and a datagram that is too short for a DNS
//! header starts no task.

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;

use super::{Proxy, Stats};
use crate::resolver::DNS_HEADER;

/// The most DNS questions in flight at the same time. A question can wait
/// for a session, so a burst with no bound would hold a task each.
pub const MAX_DNS_TASKS: usize = 256;

/// The places of the DNS questions in flight.
#[derive(Debug)]
pub struct DnsGate {
    places: Arc<Semaphore>,
    stats: Arc<Stats>,
}

impl DnsGate {
    /// A gate of `limit` places, which counts each drop in `stats`.
    pub fn new(limit: usize, stats: Arc<Stats>) -> DnsGate {
        DnsGate { places: Arc::new(Semaphore::new(limit)), stats }
    }

    /// A place for a datagram of `len` bytes. `None` for a datagram under
    /// the header (it is no DNS message), and for one over the bound
    /// (counted in [`Stats::dns_dropped`]).
    pub fn admit(&self, len: usize) -> Option<OwnedSemaphorePermit> {
        if len < DNS_HEADER {
            tracing::debug!(len, "a DNS datagram is too short; dropped");
            return None;
        }
        match self.places.clone().try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => {
                self.stats.dns_dropped.fetch_add(1, Relaxed);
                None
            }
        }
    }
}

impl Proxy {
    /// A DNS forwarder on `socket` (PodUtils/podnet issue 117): each
    /// question goes through the session of now to the resolvers of
    /// [`Proxy::names`], and the answer goes back to its sender. For a
    /// program that sends its own DNS on UDP (a host with a broken
    /// resolver). Each question is one task; the first answer wins.
    pub fn serve_dns(self: &Arc<Self>, socket: tokio::net::UdpSocket) -> JoinHandle<()> {
        let socket = Arc::new(socket);
        if let Ok(addr) = socket.local_addr() {
            tracing::info!(%addr, "the DNS forwarder through WARP is listening");
        }
        let gate = DnsGate::new(MAX_DNS_TASKS, self.stats.clone());
        let proxy = self.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            while let Ok((n, from)) = socket.recv_from(&mut buf).await {
                let Some(permit) = gate.admit(n) else { continue };
                let query = buf[..n].to_vec();
                let (socket, proxy) = (socket.clone(), proxy.clone());
                tokio::spawn(async move {
                    let _permit = permit;
                    let Ok(net) = proxy.tunnel.session().await else { return };
                    match proxy.names.resolvers.exchange(net.as_ref(), &query).await {
                        Ok(answer) => {
                            let _ = socket.send_to(&answer, from).await;
                        }
                        Err(failure) => tracing::debug!(error = %failure.chain(), "a DNS question through WARP failed"),
                    }
                });
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_gives_at_most_the_bound_in_flight() {
        let stats = Arc::new(Stats::default());
        let gate = DnsGate::new(4, stats.clone());
        let held: Vec<_> = (0..10).filter_map(|_| gate.admit(40)).collect();
        assert_eq!(held.len(), 4);
        assert_eq!(stats.dns_dropped.load(Relaxed), 6);
        // A finished question frees its place.
        drop(held);
        assert!(gate.admit(40).is_some());
    }

    #[test]
    fn a_short_datagram_starts_nothing() {
        let gate = DnsGate::new(1, Arc::new(Stats::default()));
        assert!(gate.admit(1).is_none());
        assert!(gate.admit(11).is_none());
        // It took no place.
        assert!(gate.admit(12).is_some());
    }
}

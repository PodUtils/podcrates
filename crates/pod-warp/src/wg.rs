//! WireGuard for the WARP tunnel (PodUtils/podnet issue 152). The Noise
//! IKpsk2 handshake, the cookie, the transport messages and the timers are
//! `pod-wg`, sans I/O. This module keeps the peer of a WARP device
//! ([`Peer`]), the datagrams and the tasks of a tunnel.
//!
//! The device is a WireGuard device of the WARP service (podnet's `warp
//! export` gives it): its X25519 key, the key of the peer, and the three
//! `reserved` bytes of `client_id`, which WARP reads in bytes 1 to 3 of each
//! message. The datagrams are the caller's ([`crate::UdpWay`]): a socket of
//! the system, or the datagrams of another session for a tunnel in a tunnel
//! (PodUtils/podnet issues 154 and 155).

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use bytes::Bytes;
use pod_core::{Class, Datagrams, Failure, Stage};
pub use pod_wg::{Cookie, Keys, MSG_COOKIE, MSG_RESPONSE, REKEY_TIMEOUT, Received, Stamps, Tunnel};

use crate::net::WarpNet;

/// The handshakes of a connect, [`REKEY_TIMEOUT`] apart.
pub const HANDSHAKE_TRIES: u32 = 3;
/// The largest message of WireGuard that the tunnel reads.
const MESSAGE: usize = 2048;

/// A WireGuard peer of the WARP service: the keys, the front door and the
/// addresses of the device.
#[derive(Clone)]
pub struct Peer {
    pub keys: Keys,
    pub endpoint: SocketAddr,
    pub v4: Option<Ipv4Addr>,
    pub v6: Option<Ipv6Addr>,
}

impl std::fmt::Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The keys are not shown.
        f.debug_struct("Peer").field("endpoint", &self.endpoint).field("v4", &self.v4).field("v6", &self.v6).finish()
    }
}

/// The datagrams of a tunnel and its front door. Only a datagram from the
/// front door is read. `keep` holds the session that carries a tunnel in a
/// tunnel for as long as the tunnel lives.
#[derive(Clone)]
struct Sock {
    datagrams: Arc<dyn Datagrams>,
    door: SocketAddr,
    _keep: Option<WarpNet>,
}

impl Sock {
    async fn send(&self, message: &[u8]) -> Result<(), Failure> {
        pod_core::datagrams::send_to(&*self.datagrams, message, self.door).await.map_err(|e| {
            let text = format!("UDP to the WireGuard front door {}: {e}", self.door);
            Failure::caused(Stage::Warp, Class::of_io(e.kind()), text, e)
        })
    }

    async fn recv(&self, buf: &mut [u8]) -> Result<usize, Failure> {
        loop {
            let (n, from) = pod_core::datagrams::recv_from(&*self.datagrams, buf).await.map_err(|e| {
                let text = format!("UDP from the WireGuard front door {}: {e}", self.door);
                Failure::caused(Stage::Warp, Class::of_io(e.kind()), text, e)
            })?;
            if from == self.door {
                return Ok(n);
            }
        }
    }
}

/// The write half of a WireGuard transport.
pub struct WgTx {
    sock: Sock,
    state: Arc<Mutex<Tunnel>>,
}

/// The read half of a WireGuard transport.
pub struct WgRx {
    sock: Sock,
    state: Arc<Mutex<Tunnel>>,
    buf: Vec<u8>,
}

/// The liveness check: the keepalives, the rekey, and a dead path.
pub struct WgKeepalive {
    sock: Sock,
    state: Arc<Mutex<Tunnel>>,
}

fn lock(state: &Mutex<Tunnel>) -> std::sync::MutexGuard<'_, Tunnel> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// The handshake with `peer` over `datagrams`.
pub async fn connect(peer: &Peer, datagrams: Arc<dyn Datagrams>) -> Result<(WgTx, WgRx, WgKeepalive), Failure> {
    connect_on(peer, Sock { datagrams, door: peer.endpoint, _keep: None }).await
}

/// The handshake with `peer` over a UDP socket of `outer`: a tunnel in a
/// tunnel. The tunnel holds `outer` for as long as it lives.
pub async fn connect_inside(peer: &Peer, outer: &WarpNet) -> Result<(WgTx, WgRx, WgKeepalive), Failure> {
    let datagrams = pod_core::Dial::datagrams(outer)
        .await
        .map_err(|e| Failure::caused(e.stage, e.class, "a UDP socket in the outer session", e))?;
    connect_on(peer, Sock { datagrams, door: peer.endpoint, _keep: Some(outer.clone()) }).await
}

/// The handshake with `peer` on `sock`: [`HANDSHAKE_TRIES`] tries,
/// [`REKEY_TIMEOUT`] apart. A cookie reply of a front door under load gives
/// its mac2 to the next try.
async fn connect_on(peer: &Peer, sock: Sock) -> Result<(WgTx, WgRx, WgKeepalive), Failure> {
    let mut buf = vec![0u8; MESSAGE];
    let mut last: Option<Failure> = None;
    let mut stamps = Stamps::new(SystemTime::now(), Instant::now());
    let mut cookie: Option<Cookie> = None;
    for _ in 0..HANDSHAKE_TRIES {
        let mut init =
            pod_wg::initiate_from(&peer.keys, &mut pod_core::entropy::OsEntropy, stamps.next(Instant::now()))?;
        if let Some(cookie) = &cookie {
            init.add_mac2(cookie, Instant::now());
        }
        sock.send(&init.message).await?;
        let deadline = tokio::time::Instant::now() + REKEY_TIMEOUT;
        loop {
            let n = match tokio::time::timeout_at(deadline, sock.recv(&mut buf)).await {
                Err(_) => break,
                Ok(Err(err)) => {
                    last = Some(err);
                    break;
                }
                Ok(Ok(n)) => n,
            };
            // An empty datagram has no type byte.
            match buf[..n].first() {
                Some(&MSG_RESPONSE) => match init.finish(&peer.keys, &buf[..n], Instant::now()) {
                    Ok(session) => {
                        let state =
                            Arc::new(Mutex::new(Tunnel::new(peer.keys.clone(), session, stamps, Instant::now())));
                        tracing::debug!(endpoint = %peer.endpoint, "the WireGuard handshake is done");
                        let tx = WgTx { sock: sock.clone(), state: state.clone() };
                        let rx = WgRx { sock: sock.clone(), state: state.clone(), buf: vec![0u8; MESSAGE] };
                        return Ok((tx, rx, WgKeepalive { sock, state }));
                    }
                    Err(err) => last = Some(err.into()),
                },
                Some(&MSG_COOKIE) => match Cookie::open(&peer.keys, &init, &buf[..n], Instant::now()) {
                    Ok(fresh) => {
                        cookie = Some(fresh);
                        let text = "the WireGuard front door is under load (a cookie reply)";
                        last = Some(Failure::new(Stage::Warp, Class::TempFail, text));
                    }
                    Err(err) => last = Some(err.into()),
                },
                _ => {}
            }
        }
    }
    Err(last.unwrap_or_else(|| {
        let secs = u64::from(HANDSHAKE_TRIES) * REKEY_TIMEOUT.as_secs();
        let text = format!("no WireGuard handshake answer from {} (UDP) in {secs} s", peer.endpoint);
        Failure::new(Stage::Warp, Class::Timeout, text)
    }))
}

impl WgTx {
    /// Seal and send each packet.
    pub async fn send_packets<P: AsRef<[u8]>>(&mut self, packets: &[P]) -> Result<(), Failure> {
        let out = lock(&self.state).send(packets, Instant::now())?;
        if let Some(m) = out.handshake {
            self.sock.send(&m).await?;
        }
        for m in out.messages {
            self.sock.send(&m).await?;
        }
        Ok(())
    }
}

impl WgRx {
    /// The next IP packet. A handshake response, a cookie reply, a
    /// keepalive and a message that does not authenticate are taken and not
    /// given.
    pub async fn recv_packet(&mut self) -> Result<Option<Bytes>, Failure> {
        loop {
            let n = self.sock.recv(&mut self.buf).await?;
            let received = lock(&self.state).receive(&self.buf[..n], Instant::now());
            match received {
                Received::Packet(packet) => return Ok(Some(packet.into())),
                Received::Rekeyed => tracing::debug!("a new WireGuard session (rekey)"),
                Received::Cookie => tracing::debug!("a WireGuard cookie reply: the next try of the rekey has its mac2"),
                Received::Dropped(err) => tracing::debug!(error = %err, "a WireGuard message is dropped"),
                Received::Keepalive | Received::Ignored => {}
            }
        }
    }
}

impl WgKeepalive {
    /// Ready with a failure when the path is dead: a handshake had no
    /// answer in [`pod_wg::HANDSHAKE_DEAD`]. It sends a keepalive when
    /// nothing went out for [`pod_wg::KEEPALIVE`], and the handshake of the
    /// rekey.
    pub async fn run(self) -> Result<(), Failure> {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let messages = lock(&self.state).tick(Instant::now())?;
            for m in &messages {
                let _ = self.sock.send(m).await;
            }
        }
    }
}

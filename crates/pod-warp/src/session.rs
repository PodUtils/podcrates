//! A MASQUE CONNECT-IP session over HTTP/2 on TCP 443, over HTTP/3 on UDP
//! 443 ([`crate::h3`], [`Way::h3`]), or a WireGuard tunnel on UDP
//! ([`crate::wg`], [`MasqueSession::connect_wg`]); the halves of each are
//! [`MasqueTx`] and [`MasqueRx`].
//!
//! This transport works where only 443 and 80 are open: the MASQUE front
//! door speaks HTTP/2 as well as HTTP/3, and the CONNECT-IP request is a
//! `CONNECT` with a `cf-connect-proto` header in place of an extended
//! connect with a `:protocol` pseudo-header. IP packets go as capsules in
//! the request body, in both directions.
//!
//! The credential is mutual TLS ([`crate::tls`]). The TCP connection is the
//! caller's: its [`Dial`] (a direct socket, a proxy, a relay) carries the
//! bytes of TLS and sees nothing more.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::{Method, Request, StatusCode};
use pod_core::{BoxedIo, Class, Dial, Failure, Stage, Target};
use rustls::crypto::CryptoProvider;
use rustls_pki_types::ServerName;
use tokio_rustls::TlsConnector;

use crate::identity::Identity;
use crate::net::{Renew, WarpNet};
use crate::service;
use crate::udp::{UdpWay, no_udp};
use crate::wg::{self, Peer};

mod keepalive;
mod rx;
mod tx;

pub use keepalive::{Keepalive, MAX_MISSED_PONGS};
pub use rx::MasqueRx;
use rx::RxInner;
use tx::TxInner;
pub use tx::{MasqueTx, SEND_RESERVE, SEND_STALL};

/// The HTTP/2 receive window of the MASQUE stream, and of the connection.
pub const STREAM_WINDOW: u32 = 4 << 20;
pub const CONNECTION_WINDOW: u32 = 8 << 20;

/// The smallest DATA frame that the front door sends: a capsule header (2
/// bytes) and an IPv4 header (20 bytes).
pub const MIN_DATA_FRAME: usize = 22;

/// The budget of h2 for the framing overhead of small DATA frames. Each
/// frame under 256 bytes takes 256 bytes less its size until it is read.
/// The front door sends one DATA frame for each IP packet, and most are
/// small (QUIC and TCP ACKs). The receive task can fall behind during a
/// large transfer; then a full stream window of small frames waits in h2.
/// The budget holds that worst case. A smaller budget closes the connection
/// with ENHANCE_YOUR_CALM (too_many_data_frames, PodUtils/podnet issue 32).
/// There is one stream, so the stream window is the bound.
pub const DATA_FRAME_BUDGET: usize = STREAM_WINDOW as usize / MIN_DATA_FRAME * (256 - MIN_DATA_FRAME);

/// The time limit of the TCP connect, the TLS handshake and the CONNECT
/// response.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// The HTTP/2 client settings of the MASQUE session.
pub fn h2_builder() -> h2::client::Builder {
    let mut builder = h2::client::Builder::new();
    builder
        .initial_window_size(STREAM_WINDOW)
        .initial_connection_window_size(CONNECTION_WINDOW)
        .data_frame_budget(DATA_FRAME_BUDGET);
    builder
}

/// The meter of a stream of a relay (podnet: `ws_relay::meter`, the
/// `RelayBudget::meter` of `pod-relay`): it takes the stream of the dial and
/// gives the stream that counts its bytes and the function that says when
/// the budget of the relay is spent.
pub type Meter = Arc<dyn Fn(BoxedIo) -> (BoxedIo, Renew) + Send + Sync>;

/// How a session reaches the front door: the caller's dial and TLS
/// provider, the meter of a relay, the TLS name, the `:authority`, the time
/// limit, and HTTP/3 on the caller's UDP.
#[derive(Clone)]
pub struct Way {
    pub dial: Arc<dyn Dial>,
    /// The UDP of the caller, for HTTP/3 and WireGuard. None: the way
    /// carries TCP only (a proxy, a relay), and HTTP/3 and WireGuard fail.
    pub udp: Option<Arc<dyn UdpWay>>,
    /// HTTP/3 on UDP 443 over [`Way::udp`] in place of HTTP/2 on TCP 443
    /// (PodUtils/podnet issue 157). It takes no relay.
    pub h3: bool,
    /// The meter of a dial through a relay; None: no budget.
    pub meter: Option<Meter>,
    pub provider: Arc<CryptoProvider>,
    /// The TLS name in place of [`service::masque_sni`].
    pub sni: Option<String>,
    /// The `:authority` of the CONNECT-IP request: [`service::connect_uri`].
    pub authority: String,
    pub timeout: Duration,
}

impl Way {
    /// The way through `dial` on `provider`, with the facts of
    /// `pod-services`.
    pub fn new(dial: Arc<dyn Dial>, provider: Arc<CryptoProvider>) -> Way {
        Way {
            dial,
            udp: None,
            h3: false,
            meter: None,
            provider,
            sni: None,
            authority: service::connect_uri().into(),
            timeout: CONNECT_TIMEOUT,
        }
    }
}

impl std::fmt::Debug for Way {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Way")
            .field("dial", &self.dial.label())
            .field("udp", &self.udp.as_ref().map(|u| u.label()))
            .field("h3", &self.h3)
            .field("meter", &self.meter.is_some())
            .field("sni", &self.sni)
            .finish_non_exhaustive()
    }
}

/// An established CONNECT-IP session.
pub struct MasqueSession {
    tx: MasqueTx,
    rx: MasqueRx,
    /// The budget of the relay that carries the session ([`Way::meter`]).
    budget: Option<Renew>,
}

fn failure(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Warp, class, text)
}

impl MasqueSession {
    /// Dial the front door `door` through the way `way`, and open the
    /// CONNECT-IP tunnel with the certificate of `identity` and the pinned
    /// key `pinned_spki`: over HTTP/3 on [`Way::udp`] with [`Way::h3`], else
    /// over HTTP/2 on [`Way::dial`].
    pub async fn connect(
        door: &Target,
        identity: &Identity,
        pinned_spki: Vec<u8>,
        way: &Way,
    ) -> Result<MasqueSession, Failure> {
        let limit = way.timeout;
        if way.h3 {
            return tokio::time::timeout(limit, Self::connect_h3(door, identity, pinned_spki, way)).await.map_err(
                |_| {
                    let text = format!("MASQUE connect over HTTP/3 to {door} timed out after {} s", limit.as_secs());
                    failure(Class::Timeout, format!("{text} (is UDP 443 open?)"))
                },
            )?;
        }
        tokio::time::timeout(limit, Self::connect_inner(door, identity, pinned_spki, way)).await.map_err(|_| {
            failure(Class::Timeout, format!("MASQUE connect to {door} timed out after {} s", limit.as_secs()))
        })?
    }

    /// Open the session over HTTP/3 (PodUtils/podnet issue 157): QUIC to
    /// the front door on the UDP of the way, with the client certificate and
    /// the pinned key. A door must be an address: the crate resolves no
    /// name.
    async fn connect_h3(
        door: &Target,
        identity: &Identity,
        pinned_spki: Vec<u8>,
        way: &Way,
    ) -> Result<MasqueSession, Failure> {
        let Some(udp) = &way.udp else {
            return Err(no_udp("MASQUE over HTTP/3", &way.dial.label()));
        };
        let Target::Addr(addr) = door else {
            return Err(failure(Class::Usage, format!("MASQUE over HTTP/3 takes the address of a door, not {door}")));
        };
        let tls = crate::tls::client_config(way.provider.clone(), identity, pinned_spki, crate::h3::ALPN)?;
        let datagrams = udp.datagrams(*addr).await?;
        let sni = way.sni.as_deref().unwrap_or(service::masque_sni());
        let authority = way.authority.trim_start_matches("https://");
        let (tx, rx) = crate::h3::connect(*addr, datagrams, tls, sni, authority, way.timeout).await?;
        Ok(Self::of(TxInner::H3 { tx: Box::new(tx), keepalive_taken: false }, RxInner::H3(Box::new(rx))))
    }

    /// A session over WireGuard (PodUtils/podnet issue 152): the handshake
    /// with `peer` on the UDP of `way`.
    pub async fn connect_wg(peer: &Peer, way: &Way) -> Result<MasqueSession, Failure> {
        let Some(udp) = &way.udp else {
            return Err(no_udp("WireGuard", &way.dial.label()));
        };
        let datagrams = udp.datagrams(peer.endpoint).await?;
        let (tx, rx, keepalive) = wg::connect(peer, datagrams).await?;
        Ok(Self::of_wg(tx, rx, keepalive))
    }

    /// A session over WireGuard on a UDP socket of `outer`: a tunnel in a
    /// tunnel (PodUtils/podnet issues 154 and 155). The session holds
    /// `outer`.
    pub async fn connect_wg_inside(peer: &Peer, outer: &WarpNet) -> Result<MasqueSession, Failure> {
        let (tx, rx, keepalive) = wg::connect_inside(peer, outer).await?;
        Ok(Self::of_wg(tx, rx, keepalive))
    }

    fn of_wg(tx: wg::WgTx, rx: wg::WgRx, keepalive: wg::WgKeepalive) -> MasqueSession {
        Self::of(TxInner::Wg { tx: Box::new(tx), keepalive: Some(keepalive) }, RxInner::Wg(Box::new(rx)))
    }

    fn of(tx: TxInner, rx: RxInner) -> MasqueSession {
        MasqueSession { tx: MasqueTx::new(tx), rx: MasqueRx::new(rx), budget: None }
    }

    /// True for a session over HTTP/3.
    pub fn is_h3(&self) -> bool {
        self.tx.is_h3()
    }

    /// True for a session over WireGuard.
    pub fn is_wg(&self) -> bool {
        self.tx.is_wg()
    }

    async fn connect_inner(
        door: &Target,
        identity: &Identity,
        pinned_spki: Vec<u8>,
        way: &Way,
    ) -> Result<MasqueSession, Failure> {
        let deadline = tokio::time::Instant::now() + way.timeout;
        let tcp = way.dial.dial(door, deadline).await.map_err(|e| {
            let text = format!("TCP to the MASQUE front door {door} through {}", way.dial.label());
            Failure::caused(e.stage, e.class, text, e)
        })?;
        // Through a relay, the session has a budget of bytes and time: the
        // caller replaces it before the relay ends it.
        let (io, budget) = match &way.meter {
            Some(meter) => {
                let (io, budget) = meter(tcp.io);
                (io, Some(budget))
            }
            None => (tcp.io, None),
        };
        let config = crate::tls::client_config(way.provider.clone(), identity, pinned_spki, crate::tls::ALPN_H2)?;
        let sni = way.sni.as_deref().unwrap_or(service::masque_sni()).to_string();
        let server_name = ServerName::try_from(sni.clone())
            .map_err(|_| failure(Class::Usage, format!("{sni:?} is not a TLS name of the MASQUE front door")))?;
        let tls = TlsConnector::from(config)
            .connect(server_name, io)
            .await
            .map_err(|e| crate::tls::handshake_failure(&door.to_string(), e))?;
        let (client, mut connection) = h2_builder().handshake(tls).await.map_err(|e| {
            Failure::caused(Stage::Warp, Class::Protocol, "HTTP/2 handshake with the MASQUE front door", e)
        })?;
        // The PING handle is taken before the connection moves to its task:
        // the edge ends a MASQUE tunnel that stays idle for too long.
        let ping = connection.ping_pong();
        tokio::spawn(async move {
            if let Err(err) = connection.await {
                tracing::warn!(%err, "the MASQUE HTTP/2 connection closed");
            }
        });
        let mut client = client
            .ready()
            .await
            .map_err(|e| Failure::caused(Stage::Warp, Class::Protocol, "the HTTP/2 client is not ready", e))?;
        let request = Request::builder()
            .method(Method::CONNECT)
            .uri(way.authority.as_str())
            .header("cf-connect-proto", "cf-connect-ip")
            .header("capsule-protocol", "?1")
            .header("pq-enabled", "false")
            .header("user-agent", "")
            .body(())
            .map_err(|e| Failure::caused(Stage::Warp, Class::Usage, "the CONNECT-IP request", e))?;
        let (response, send) = client
            .send_request(request, false)
            .map_err(|e| Failure::caused(Stage::Warp, Class::Protocol, "sending the CONNECT-IP request", e))?;
        let response = response
            .await
            .map_err(|e| Failure::caused(Stage::Warp, Class::Protocol, "awaiting the CONNECT-IP response", e))?;
        let status = response.status();
        if status != StatusCode::OK {
            let (class, why) = match status.as_u16() {
                403 => (Class::Refused, " (the enrolled certificate was not accepted)"),
                _ => (Class::Protocol, ""),
            };
            return Err(failure(class, format!("CONNECT-IP refused with {status}{why}")));
        }
        let mut session = Self::from_h2(ping, send, response.into_body());
        session.budget = budget;
        Ok(session)
    }

    /// A session over the streams of an HTTP/2 request.
    pub(crate) fn from_h2(ping: Option<h2::PingPong>, send: h2::SendStream<Bytes>, recv: h2::RecvStream) -> Self {
        Self::of(TxInner::H2 { ping, send }, RxInner::H2(recv))
    }

    /// The budget of the relay that carries the session: true when it is
    /// spent. None for a way with no meter.
    pub fn relay_budget(&self) -> Option<Renew> {
        self.budget.clone()
    }

    /// Split into a write half and a read half, so that two tasks pump the
    /// two directions.
    pub fn split(self) -> (MasqueTx, MasqueRx) {
        (self.tx, self.rx)
    }
}

#[cfg(test)]
mod tests;

//! The QUIC arm of the race (podssh T-667): whether a session takes it, and
//! its session. Its UDP socket is the connection's own, on a port that the
//! system picks, and nothing listens on it; before it, the probe of
//! `pod-quic` sends one packet of a version that no server speaks to the
//! first address of each family, which a server of QUIC answers with a
//! Version Negotiation within 300 ms, so that QUIC is tried only where UDP
//! passes. What a host's UDP did is kept for the run of the `Context`: a
//! host that failed is not tried again over QUIC, and one that answered is
//! not probed again, for [`super::REMEMBERED`].

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use noq::crypto::rustls::QuicClientConfig;
use noq::{ClientConfig, Connection, ConnectionError, Endpoint, IdleTimeout, TransportConfig, VarInt};
use pod_core::Resolve;
use pod_proxy::DialError;
use pod_quic::probe;
use pod_ws::url::Stream;
use pod_ws::{ConnectError, RelaySession, Trust, WsClientConfig};

use super::stream::QuicStream;
use super::{ALPN, Choice, REMEMBERED, WORDS};
use crate::context::Context;
use crate::relay::Relay;

/// How long QUIC's handshake may take, as the quick tunnel's.
pub const HANDSHAKE_WAIT: Duration = Duration::from_secs(5);
/// A silence that ends a connection, and the keepalive that keeps a live
/// one, and its mapping in a NAT, within it: a session's own frames may be
/// 25 s apart.
pub const SILENCE: Duration = Duration::from_secs(30);
pub const KEEPALIVE: Duration = Duration::from_secs(10);
/// The first packets: 1200 bytes fit a path whose MTU is 1280, IPv6's least.
pub const FIRST_PACKET: u16 = 1200;

/// What a run learned of a host's UDP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Learned {
    /// QUIC answered at this address.
    Answered(SocketAddr),
    /// The probe, or the handshake, failed.
    Failed,
}

/// The key of a host's UDP port in the memory.
fn key(host: &str, port: u16) -> String {
    format!("{}:{port}", host.trim_end_matches('.').to_ascii_lowercase())
}

/// What the run of `ctx` learned of `host`'s UDP `port` within
/// [`REMEMBERED`].
pub fn learned(ctx: &Context, host: &str, port: u16) -> Option<Learned> {
    let memory = ctx.quic_memory().lock().unwrap_or_else(|e| e.into_inner());
    memory.get(&key(host, port)).filter(|(at, _)| at.elapsed() < REMEMBERED).map(|(_, learned)| *learned)
}

/// Keep `learned` of `host`'s UDP `port`.
pub fn learn(ctx: &Context, host: &str, port: u16, learned: Learned) {
    let mut memory = ctx.quic_memory().lock().unwrap_or_else(|e| e.into_inner());
    memory.insert(key(host, port), (Instant::now(), learned));
}

/// The UDP port of QUIC for a session of `config` to a host of a list that
/// the user named when `explicit`, where [`Choice`] and the host's kept offer
/// give one; none where a proxy is set for the host, where the caller gave
/// its own TLS configuration, or where the run saw the host's UDP fail.
pub fn offered(ctx: &Context, config: &WsClientConfig, explicit: bool) -> Option<u16> {
    let choice = Choice::of(ctx).ok()?;
    if choice == Choice::No || (choice == Choice::Auto && !explicit) {
        return None;
    }
    let host = &config.endpoint.host;
    // The offer is kept with the resumable layer's, from /relays.json.
    let relay = Relay { host: host.clone(), port: config.endpoint.port };
    let kept = crate::forward_layer::kept::kept_now(&ctx.cache, &relay).and_then(|offer| offer.quic);
    let port = choice.port(explicit, kept, config.endpoint.port)?;
    let own_tls = matches!(config.trust, Trust::Caller(_));
    (ctx.direct_to(host) && !own_tls && learned(ctx, host, port) != Some(Learned::Failed)).then_some(port)
}

/// TLS 1.3 alone, as QUIC needs, on `provider`, with the roots of the
/// session's trust and the role's ALPN.
pub fn tls(
    provider: Arc<rustls::crypto::CryptoProvider>,
    roots: &pod_tls::TlsRoots,
) -> Result<Arc<rustls::ClientConfig>, String> {
    pod_quic::connect::tls(provider, Arc::new(roots.roots.clone()), &[ALPN]).map_err(|f| f.to_string())
}

/// The settings of a client's connection: the role opens no stream to it.
fn transport() -> TransportConfig {
    let mut transport = TransportConfig::default();
    transport
        .max_idle_timeout(IdleTimeout::try_from(SILENCE).ok())
        .keep_alive_interval(Some(KEEPALIVE))
        .initial_mtu(FIRST_PACKET)
        .max_concurrent_bidi_streams(VarInt::from_u32(0))
        .max_concurrent_uni_streams(VarInt::from_u32(0));
    transport
}

/// A QUIC connection to `addr` as `server_name`, within the handshake's wait.
async fn connect(
    endpoint: &Endpoint,
    addr: SocketAddr,
    server_name: &str,
    tls: Arc<rustls::ClientConfig>,
) -> Result<Connection, String> {
    let crypto = QuicClientConfig::try_from(tls).map_err(|e| format!("QUIC's TLS: {e}"))?;
    let mut config = ClientConfig::new(Arc::new(crypto));
    config.transport_config(Arc::new(transport()));
    let connecting = endpoint.connect_with(config, addr, server_name).map_err(|e| format!("{addr}: {e}"))?;
    match tokio::time::timeout(HANDSHAKE_WAIT, connecting).await {
        Ok(Ok(connection)) => Ok(connection),
        Ok(Err(ConnectionError::TimedOut)) | Err(_) => {
            Err(format!("{addr}: no answer over QUIC within {} s", HANDSHAKE_WAIT.as_secs()))
        }
        Ok(Err(e)) => Err(format!("{addr}: QUIC: {e}")),
    }
}

/// The session of `config` with `token` over QUIC to the host's UDP `port`:
/// the probe, unless QUIC answered the run there lately; the handshake;
/// one stream, and the contract's upgrade on it. QUIC's own failure is a
/// dial's, which ends no race; the relay's refusal on the stream is the
/// upgrade's, as on TCP.
pub async fn session(
    ctx: &Context,
    config: &WsClientConfig,
    token: &str,
    port: u16,
) -> Result<RelaySession, ConnectError> {
    config.validate().map_err(ConnectError::Config)?;
    if token.contains(['\r', '\n']) {
        return Err(ConnectError::Config("the relay token contains a line break".into()));
    }
    let host = config.endpoint.host.as_str();
    let failed = |detail: String| {
        let target = format!("UDP {}", pod_proxy::host::authority(host, port));
        ConnectError::Dial(DialError::Connect { target, detail, cut: None, kind: None })
    };
    let roots = config.way.tls.roots_for(&config.trust).map_err(|e| failed(e.to_string()))?;
    let tls = tls(config.way.tls.provider().clone(), &roots).map_err(failed)?;
    let deadline = tokio::time::Instant::now() + config.timeout;
    let addrs = resolve(ctx, host, port, deadline).await.map_err(failed)?;
    let addr = match learned(ctx, host, port) {
        Some(Learned::Answered(at)) if addrs.contains(&at) => at,
        _ => {
            let firsts = [addrs.iter().find(|a| a.is_ipv4()), addrs.iter().find(|a| a.is_ipv6())];
            let firsts: Vec<SocketAddr> = firsts.into_iter().flatten().copied().collect();
            match probe::probe(&firsts, probe::WAIT).await {
                Ok((at, _)) => at,
                Err(why) => {
                    learn(ctx, host, port, Learned::Failed);
                    return Err(failed(why.to_string()));
                }
            }
        }
    };
    let endpoint = pod_quic::endpoint::system(addr.is_ipv6()).map_err(|f| failed(f.to_string()))?;
    let connection = match connect(&endpoint, addr, &config.server_name, tls).await {
        Ok(connection) => connection,
        Err(why) => {
            learn(ctx, host, port, Learned::Failed);
            return Err(failed(why));
        }
    };
    learn(ctx, host, port, Learned::Answered(addr));
    let (send, recv) = connection.open_bi().await.map_err(|e| failed(format!("no stream: {e}")))?;
    let stream = QuicStream::client(send, recv, connection, endpoint);
    let host_header = pod_proxy::host::authority(host, config.endpoint.port);
    let path = &config.endpoint.path;
    let upgraded = tokio::time::timeout(config.timeout, pod_ws::client::upgrade(stream, &host_header, path, token));
    let late = |_| ConnectError::Timeout { step: "the WebSocket upgrade over QUIC", after: config.timeout };
    let (stream, pending) = upgraded.await.map_err(late)??;
    let carried = Stream::Carried(Box::new(stream));
    let session = RelaySession::new(carried, pending, config.idle_timeout, pod_ws::client::WRITE_TIMEOUT);
    Ok(session.with_road(WORDS.to_string()))
}

/// The addresses of `host:port` through the resolver of the way of `ctx`,
/// where the caller gave one, else the system's, each within `deadline`.
async fn resolve(
    ctx: &Context,
    host: &str,
    port: u16,
    deadline: tokio::time::Instant,
) -> Result<Vec<SocketAddr>, String> {
    let found = match &ctx.resolver {
        Some(resolver) => resolver.resolve(host, port, deadline).await,
        None => pod_proxy::resolve::System::new().resolve(host, port, deadline).await,
    };
    found.map_err(|f| f.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host's UDP is kept by its name and port, in any case, and a later
    /// word replaces an earlier one.
    #[test]
    fn what_a_host_s_udp_did_is_kept_by_its_name_and_port() {
        let ctx = crate::testing::context();
        let at: SocketAddr = "192.0.2.7:443".parse().unwrap();
        assert_eq!(learned(&ctx, "udp-kept.example", 443), None);
        learn(&ctx, "UDP-Kept.example.", 443, Learned::Failed);
        assert_eq!(learned(&ctx, "udp-kept.example", 443), Some(Learned::Failed));
        assert_eq!(learned(&ctx, "udp-kept.example", 8443), None, "another port is another host");
        learn(&ctx, "udp-kept.example", 443, Learned::Answered(at));
        assert_eq!(learned(&ctx, "udp-kept.example", 443), Some(Learned::Answered(at)));
    }

    /// QUIC's TLS offers the role's ALPN alone, over TLS 1.3, which QUIC's
    /// configuration takes.
    #[test]
    fn the_tls_of_quic_offers_the_role_s_alpn_alone() {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = tls(provider, &pod_tls::roots_from_compiled_set()).expect("a configuration");
        assert_eq!(tls.alpn_protocols, vec![ALPN.to_vec()]);
        assert!(QuicClientConfig::try_from(tls).is_ok(), "QUIC takes it");
    }
}

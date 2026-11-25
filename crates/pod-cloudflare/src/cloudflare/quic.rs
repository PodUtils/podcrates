//! The QUIC edge transport.
//!
//! # The handshake
//!
//! QUIC is the default and the faster transport. But it needs UDP, and many
//! networks and most sandboxes block UDP. This transport is for the normal
//! case. [`super::http2`] works where UDP is blocked.
//!
//! Two details decide if this transport works:
//!
//! * **ALPN is `argotunnel`** and **SNI is `quic.cftunnel.com`**. These are
//!   *different strings*. With the ALPN value as SNI, the key derivation
//!   completes. Then the connection stops with no message after the handshake
//!   timeout. This is the most frequent error of this transport, and the
//!   most difficult to find.
//! * **The edge certificate chains to internal roots of Cloudflare.** No
//!   public trust store has them. Thus [`super::edge_roots_pem`] goes next to
//!   the platform roots and does not replace them.
//!
//! # Streams
//!
//! The **first** stream that the client opens is the control plane. The edge
//! expects this and opens each other stream. Each visitor request then
//! arrives on its own bidirectional stream. A six-byte signature and a
//! version come before the Cap'n Proto payload.

use std::sync::Arc;
use std::time::Duration;

use noq::{ClientConfig, Endpoint, TransportConfig};

use pod_core::{Class, Failure, Stage};

use super::EdgeAddr;
use crate::fail::{Result, bail};

/// The handshake timeout of the QUIC edge connection.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The idle timeout. The edge sends keepalives. A short idle timeout can drop
/// a good tunnel between two keepalives.
pub const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// How often to send a keepalive.
pub const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(1);

pub use super::MAX_EDGE_STREAMS;

/// The QUIC transport settings of an edge connection, with `initial_mtu` as
/// the first UDP payload size.
pub fn transport_config(initial_mtu: u16) -> TransportConfig {
    let mut transport = TransportConfig::default();
    transport
        .max_idle_timeout(Some(noq::IdleTimeout::try_from(MAX_IDLE_TIMEOUT).expect("30 s is a valid idle timeout")));
    transport.keep_alive_interval(Some(KEEP_ALIVE_INTERVAL));
    transport.initial_mtu(initial_mtu);
    transport.max_concurrent_bidi_streams(MAX_EDGE_STREAMS.into());
    transport
}

/// The rustls config for the QUIC edge.
///
/// This function is separate from [`client_config`], so that a test can
/// check the ALPN and the trust roots directly. The config of noq has no
/// getters, so a test that reads back a noq config proves nothing.
pub fn tls_config(tls: &pod_tls::Context) -> Result<Arc<rustls::ClientConfig>> {
    super::edge_tls_config(tls, super::quic_alpn())
}

/// Build a noq client config for the edge.
pub fn client_config(tls: &pod_tls::Context) -> Result<ClientConfig> {
    let tls = tls_config(tls)?;

    let quic_tls = noq::crypto::rustls::QuicClientConfig::try_from(tls)
        .map_err(|e| crate::fail::caused(Stage::Tls, Class::Config, "quic tls config", e))?;

    let mut config = ClientConfig::new(Arc::new(quic_tls));

    // 1252 is a safe IPv4 payload size. It prevents the loss of fragmented
    // packets on home routers. cloudflared uses the same value.
    let transport = transport_config(1252);
    config.transport_config(Arc::new(transport));

    Ok(config)
}

/// Make a client endpoint on an ephemeral UDP port, with the edge client
/// configuration over the roots and the provider of `tls`. When no UDP
/// socket opens, the error names `protocol`, the setting of the protocol in
/// the words of the consumer.
pub fn build_endpoint(tls: &pod_tls::Context, protocol: &str) -> Result<Endpoint> {
    let bind: std::net::SocketAddr = "[::]:0".parse().expect("a valid address");
    let endpoint = match pod_quic::endpoint::bind(bind) {
        Ok(endpoint) => endpoint,
        // A host with no IPv6 refuses `[::]`. Use IPv4 only.
        Err(_) => pod_quic::endpoint::bind("0.0.0.0:0".parse().expect("a valid address")).map_err(|e| {
            let text = format!(
                "could not open a local UDP socket: {}. QUIC needs UDP; \
                 where it is blocked, use {protocol} http2",
                e.text()
            );
            Failure::caused(Stage::Quic, e.class, text, e)
        })?,
    };
    endpoint.set_default_client_config(client_config(tls)?);
    Ok(endpoint)
}

/// Complete a QUIC handshake with `addr`. The SNI is `quic.cftunnel.com`
/// for every address.
pub async fn dial(endpoint: &Endpoint, addr: &EdgeAddr) -> Result<noq::Connection> {
    let socket = addr.socket();
    let connecting = endpoint.connect(socket, super::quic_sni()).map_err(|e| {
        crate::fail::caused(Stage::Quic, Class::Unavailable, format!("could not start connecting to {socket}"), e)
    })?;
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, connecting).await {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(e)) => {
            Err(crate::fail::caused(Stage::Quic, Class::Unavailable, format!("QUIC handshake with {socket}"), e))
        }
        Err(_) => Err(crate::fail::new(Stage::Quic, Class::Timeout, format!("QUIC handshake with {socket} timed out"))),
    }
}

/// Try the addresses in sequence. Return the first connection.
pub async fn dial_any(endpoint: &Endpoint, edges: &[EdgeAddr]) -> Result<noq::Connection> {
    let mut failures = Vec::new();
    for addr in edges {
        match dial(endpoint, addr).await {
            Ok(conn) => return Ok(conn),
            Err(err) => {
                tracing::debug!(error = %err.chain(), "QUIC edge attempt failed");
                failures.push(err.chain());
            }
        }
    }
    if failures.is_empty() {
        bail!(Stage::Quic, Class::Config, "no edge addresses to dial");
    }
    bail!(Stage::Quic, Class::Unavailable, "no edge answered: {}", failures.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A local QUIC server opens 200 streams to the client and keeps them
    /// open, as the edge does with 200 requests at the same time. With the
    /// default of noq (100), the stream after the 100th waits.
    #[tokio::test]
    async fn the_edge_can_open_200_streams_at_the_same_time() {
        pod_testkit::need_loopback!();
        let pair = crate::standin::quic::local_pair().await;
        let edge = pair.edge.clone();
        let opener = tokio::spawn(async move {
            let mut streams = Vec::new();
            for i in 0..200 {
                let (mut send, recv) = tokio::time::timeout(Duration::from_secs(5), edge.open_bi())
                    .await
                    .unwrap_or_else(|_| panic!("stream {i} waits for the limit of the client"))
                    .unwrap();
                send.write_all(b"x").await.unwrap();
                streams.push((send, recv));
            }
            streams
        });
        let accept = async {
            let mut accepted = Vec::new();
            for _ in 0..200 {
                let (send, mut recv) = pair.client.accept_bi().await.unwrap();
                let mut byte = [0u8; 1];
                recv.read_exact(&mut byte).await.unwrap();
                accepted.push((send, recv));
            }
            accepted
        };
        let (accepted, opened) = tokio::join!(accept, opener);
        assert_eq!((accepted.len(), opened.unwrap().len()), (200, 200));
    }

    #[test]
    fn alpn_and_sni_are_distinct() {
        assert_eq!(super::super::quic_alpn(), b"argotunnel");
        assert_eq!(super::super::quic_sni(), "quic.cftunnel.com");
        assert_ne!(super::super::quic_sni().as_bytes(), super::super::quic_alpn());
    }

    #[test]
    fn the_quic_tls_config_offers_exactly_the_argotunnel_alpn() {
        let tls = tls_config(&crate::testing::tls()).expect("quic tls config");
        assert_eq!(
            tls.alpn_protocols,
            vec![b"argotunnel".to_vec()],
            "the edge accepts one protocol; offering anything else fails selection"
        );
    }

    #[test]
    fn noq_accepts_the_tls_config_it_is_given() {
        // This failure can occur: rustls 0.23 and noq can have different
        // versions of the crypto provider. The try_from of noq shows the
        // error, and it looks like a TLS problem of the handshake.
        client_config(&crate::testing::tls()).expect("noq accepts the edge tls config");
    }
}

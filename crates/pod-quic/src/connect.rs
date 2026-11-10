//! A client's connection: TLS 1.3 for QUIC on the provider of the caller,
//! the client configuration of the [`Settings`], a connection within the
//! handshake's wait, and a race of handshakes to several addresses (podssh's
//! `doctor`).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use noq::crypto::rustls::QuicClientConfig;
use noq::{ClientConfig, Connection, ConnectionError, Endpoint, VarInt};
use pod_core::{Class, Failure, Stage};
use rustls::RootCertStore;
use rustls::crypto::CryptoProvider;

use crate::endpoint;
use crate::settings::Settings;

/// How long a handshake's close may take to leave, before its endpoint goes.
const CLOSE_WAIT: Duration = Duration::from_millis(500);

/// TLS 1.3 alone, as QUIC needs, on `provider`, with `roots` and `alpn`.
pub fn tls(
    provider: Arc<CryptoProvider>,
    roots: Arc<RootCertStore>,
    alpn: &[&[u8]],
) -> Result<Arc<rustls::ClientConfig>, Failure> {
    let config = |e: &dyn std::fmt::Display| Failure::new(Stage::Tls, Class::Config, format!("QUIC's TLS: {e}"));
    let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(roots, provider.clone())
        .build()
        .map_err(|e| config(&e))?;
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| config(&e))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    tls.alpn_protocols = alpn.iter().map(|a| a.to_vec()).collect();
    Ok(Arc::new(tls))
}

/// The client configuration of `tls` with the transport of `settings`
/// toward an address of the family `v6`.
pub fn client_config(tls: Arc<rustls::ClientConfig>, settings: &Settings, v6: bool) -> Result<ClientConfig, Failure> {
    let crypto = QuicClientConfig::try_from(tls)
        .map_err(|e| Failure::new(Stage::Tls, Class::Config, format!("QUIC's TLS: {e}")))?;
    let mut config = ClientConfig::new(Arc::new(crypto));
    config.transport_config(Arc::new(settings.transport(v6)));
    Ok(config)
}

/// The class of a failed connection.
fn class(e: &ConnectionError) -> Class {
    match e {
        ConnectionError::TimedOut => Class::Timeout,
        ConnectionError::VersionMismatch => Class::Unsupported,
        ConnectionError::TransportError(_) => Class::Protocol,
        ConnectionError::ConnectionClosed(_) | ConnectionError::ApplicationClosed(_) => Class::Refused,
        _ => Class::Unavailable,
    }
}

/// A QUIC connection to `addr` as `server_name`, within the handshake's
/// wait of `settings`.
pub async fn connect(
    endpoint: &Endpoint,
    addr: SocketAddr,
    server_name: &str,
    tls: Arc<rustls::ClientConfig>,
    settings: &Settings,
) -> Result<Connection, Failure> {
    let config = client_config(tls, settings, addr.is_ipv6())?;
    let connecting = endpoint
        .connect_with(config, addr, server_name)
        .map_err(|e| Failure::caused(Stage::Quic, Class::Usage, format!("{addr}: {e}"), e))?;
    let wait = settings.handshake_wait;
    match tokio::time::timeout(wait, connecting).await {
        Ok(Ok(connection)) => Ok(connection),
        // QUIC's own limit of a silence is the handshake's wait too, and
        // either can end it first: one fact, in one set of words.
        Ok(Err(ConnectionError::TimedOut)) | Err(_) => Err(Failure::new(
            Stage::Quic,
            Class::Timeout,
            format!("{addr}: no answer over QUIC within {} s", wait.as_secs()),
        )),
        Ok(Err(e)) => Err(Failure::caused(Stage::Quic, class(&e), format!("{addr}: QUIC: {e}"), e)),
    }
}

/// A QUIC handshake with each of `addrs` at once, each from an endpoint of
/// its own on a socket of the system, each within the handshake's wait: the
/// address that answered first and the time that its handshake took, the
/// connection closed at once with no stream; else why none answered.
pub async fn handshake(
    addrs: &[SocketAddr],
    server_name: &str,
    tls: Arc<rustls::ClientConfig>,
    settings: &Settings,
) -> Result<(SocketAddr, Duration), Failure> {
    let mut tries = tokio::task::JoinSet::new();
    for &addr in addrs {
        let (name, tls, settings) = (server_name.to_string(), tls.clone(), settings.clone());
        tries.spawn(async move {
            let socket = endpoint::system(addr.is_ipv6())?;
            let began = tokio::time::Instant::now();
            let connection = connect(&socket, addr, &name, tls, &settings).await?;
            let took = began.elapsed();
            connection.close(VarInt::from_u32(0), b"");
            let _ = tokio::time::timeout(CLOSE_WAIT, socket.wait_idle()).await;
            Ok::<_, Failure>((addr, took))
        });
    }
    let mut failed: Vec<Failure> = Vec::new();
    while let Some(joined) = tries.join_next().await {
        match joined {
            Ok(Ok(answered)) => {
                tries.abort_all();
                return Ok(answered);
            }
            Ok(Err(why)) => failed.push(why),
            Err(e) => failed.push(Failure::new(Stage::Quic, Class::Unavailable, e.to_string())),
        }
    }
    // Each address timed out: the class is a timeout; else the class of the
    // first failure.
    let class = match failed.iter().all(|f| f.class == Class::Timeout) {
        true => Class::Timeout,
        false => failed.iter().find(|f| f.class != Class::Timeout).map_or(Class::Unavailable, |f| f.class),
    };
    let said: Vec<String> = failed.iter().map(|f| f.to_string()).collect();
    let text = match said.is_empty() {
        true => "no address to try over QUIC".to_string(),
        false => format!("no address answered over QUIC: {}", said.join("; ")),
    };
    Err(Failure::new(Stage::Quic, if said.is_empty() { Class::Usage } else { class }, text))
}

//! The way out of each connection that the crate opens, as its caller gives
//! it: the TLS context of the binary (`pod-tls`: the provider, the roots,
//! the kept configurations, the state of ECH), the dialer of each TCP
//! connection (`pod_core::Dial`: direct, a proxy, a relay), the reader of a
//! name's HTTPS record for Encrypted Client Hello (feature `ech`), and the
//! user agent of the binary. The crate reads no variable of the
//! environment and keeps no process state: the binary makes one `Way` and
//! shares it.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Connected, Dial, Failure, Stage, Target};
use pod_proxy::{Cut, DialError, TlsFailure};
use pod_tls::{Context, Trust};
use tokio::time::Instant;

use crate::client::ConnectError;
use crate::session::RelayStream;

/// The way out of each connection, and the TLS of each.
#[derive(Clone)]
pub struct Way {
    /// The TLS context of the binary.
    pub tls: Arc<Context>,
    /// The dialer of each TCP connection.
    pub dial: Arc<dyn Dial>,
    /// The reader of a name's HTTPS record, through the same way out, for a
    /// hello sealed with ECH where the context asks for it.
    #[cfg(feature = "ech")]
    pub records: Arc<dyn pod_tls::ech::Records>,
    /// The `User-Agent` of each request of the crate: `podssh/VERSION`,
    /// `podnet/VERSION`.
    pub user_agent: String,
}

impl std::fmt::Debug for Way {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Way").field("dial", &self.dial.label()).field("user_agent", &self.user_agent).finish()
    }
}

impl Way {
    /// A way through `dial`, with the TLS of `tls`, that names itself
    /// `user_agent`. With the feature `ech`, a name's record is not read
    /// until [`Way::with_records`] gives a reader.
    pub fn new(tls: Arc<Context>, dial: Arc<dyn Dial>, user_agent: impl Into<String>) -> Way {
        Way {
            tls,
            dial,
            #[cfg(feature = "ech")]
            records: Arc::new(pod_tls::ech::NoRecords),
            user_agent: user_agent.into(),
        }
    }

    /// The reader of a name's HTTPS record, for ECH.
    #[cfg(feature = "ech")]
    pub fn with_records(mut self, records: Arc<dyn pod_tls::ech::Records>) -> Way {
        self.records = records;
        self
    }

    /// The name of the way out, as its dialer gives it. Two ways with the
    /// same label are one way to the pool of HTTP/2 connections.
    pub fn label(&self) -> String {
        self.dial.label()
    }

    /// A TCP connection to `host:port` through the dialer, within `limit`.
    pub async fn tcp(&self, host: &str, port: u16, limit: Duration) -> Result<Connected, DialError> {
        let target = Target::new(host, port);
        self.dial.dial(&target, Instant::now() + limit).await.map_err(|f| dial_error(f, &target))
    }

    /// TCP to `host:port` through the dialer, then TLS for `server_name`
    /// with the roots of `trust`, offering `alpn`; and the address that the
    /// TCP reached. With the feature `ech`, the hello is sealed where the
    /// context asks for it and the name publishes a list.
    pub async fn open_tls(
        &self,
        host: &str,
        port: u16,
        server_name: &str,
        trust: &Trust,
        limit: Duration,
        alpn: &[&[u8]],
    ) -> Result<(RelayStream, Option<SocketAddr>), ConnectError> {
        // The configuration and the name are checked before anything connects.
        self.tls.config_for(trust).map_err(|e| ConnectError::Config(WsErrorText::of(e)))?;
        rustls_pki_types::ServerName::try_from(server_name.to_string())
            .map_err(|e| ConnectError::Config(format!("{server_name:?} is not a valid server name: {e}")))?;
        self.open_checked(host, port, server_name, trust, limit, alpn).await
    }

    #[cfg(feature = "ech")]
    async fn open_checked(
        &self,
        host: &str,
        port: u16,
        server_name: &str,
        trust: &Trust,
        limit: Duration,
        alpn: &[&[u8]],
    ) -> Result<(RelayStream, Option<SocketAddr>), ConnectError> {
        let target = Target::new(host, port);
        let dial: &dyn Dial = &*self.dial;
        let records: &dyn pod_tls::ech::Records = &*self.records;
        let route = pod_tls::ech::Route { target: &target, server_name, trust, dial, records, limit, alpn };
        pod_tls::ech::open(&self.tls, &route).await.map_err(|e| connect_error(e, limit, &target))
    }

    #[cfg(not(feature = "ech"))]
    async fn open_checked(
        &self,
        host: &str,
        port: u16,
        server_name: &str,
        trust: &Trust,
        limit: Duration,
        alpn: &[&[u8]],
    ) -> Result<(RelayStream, Option<SocketAddr>), ConnectError> {
        let tcp = self.tcp(host, port, limit).await.map_err(ConnectError::Dial)?;
        let peer = tcp.peer;
        let target = Target::new(host, port);
        let failed = |e| connect_error(e, limit, &target);
        if alpn.is_empty() {
            let tls = self.tls.tls_over(tcp.io, server_name, trust, limit).await.map_err(failed)?;
            return Ok((tls, peer));
        }
        // A copy of the kept configuration with this road's protocols: the
        // same verifier and the same cache of sessions.
        let kept = self.tls.config_for(trust).map_err(|e| ConnectError::Config(WsErrorText::of(e)))?;
        let mut config: rustls::ClientConfig = (*kept).clone();
        config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
        let words = self.tls.words();
        let tls = pod_tls::tls_over(tcp.io, server_name, Arc::new(config), limit, words).await.map_err(failed)?;
        Ok((tls, peer))
    }

    /// A verified TLS handshake for `server_name` over `stream`, a connection
    /// that is open already, within `limit`. rustls's text distinguishes an
    /// unknown issuer from a name mismatch, and an unknown issuer gets what
    /// to do; the kind of a cut is kept beside it.
    pub async fn tls_over<S>(
        &self,
        stream: S,
        server_name: &str,
        trust: &Trust,
        limit: Duration,
    ) -> Result<tokio_rustls::client::TlsStream<S>, ConnectError>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let target = Target::new(server_name, 0);
        self.tls.tls_over(stream, server_name, trust, limit).await.map_err(|e| connect_error(e, limit, &target))
    }
}

/// The text of an error of `pod-tls` as a configuration error says it.
struct WsErrorText;

impl WsErrorText {
    fn of(e: pod_tls::Error) -> String {
        crate::error::WsError::from(e).to_string()
    }
}

impl From<pod_tls::Error> for crate::error::WsError {
    fn from(e: pod_tls::Error) -> crate::error::WsError {
        match e {
            pod_tls::Error::Bundle { path, why } => crate::error::WsError::Bundle { path, why },
            pod_tls::Error::Config(why) => crate::error::WsError::Config(why),
            other => crate::error::WsError::Config(other.to_string()),
        }
    }
}

/// The failure of a dial as a [`DialError`]: the dialer's own, as
/// `pod-proxy` keeps it in the cause, else one that keeps the words, the
/// stage of a bound and the cut of an error of I/O.
pub fn dial_error(failure: Failure, target: &Target) -> DialError {
    let cause = std::error::Error::source(&failure);
    if let Some(dial) = cause.and_then(|e| e.downcast_ref::<DialError>()) {
        return dial.clone();
    }
    let kind = failure.io_cause().map(|e| e.kind());
    match failure.class {
        Class::Timeout => DialError::Timeout {
            step: format!("the connection to {target}"),
            after: Duration::ZERO,
            stage: failure.stage,
        },
        Class::Usage => DialError::InvalidTarget(failure.text().to_string()),
        Class::Config => DialError::BadProxy(failure.text().to_string()),
        _ if failure.stage == Stage::Dns => {
            DialError::Resolve { host: target.host(), detail: failure.text().to_string() }
        }
        _ => DialError::Connect {
            target: target.to_string(),
            detail: failure.text().to_string(),
            cut: kind.map(Cut::of),
            kind,
        },
    }
}

/// The failure of a TLS road of `pod-tls` as a [`ConnectError`]: the dial's
/// error as it was, a configuration or a name that cannot be used, the
/// bound of the handshake, or the handshake's words with the cut of its
/// error of I/O.
pub fn connect_error(failure: Failure, limit: Duration, target: &Target) -> ConnectError {
    let cause = std::error::Error::source(&failure);
    let of_tls = matches!(failure.stage, Stage::Tls | Stage::Network);
    let usage = matches!(failure.class, Class::Config | Class::Usage);
    // A failure of the way out, before TLS: the dialer's, or another
    // dialer's at its own stage.
    if cause.is_some_and(|e| e.is::<DialError>()) || (!of_tls && !usage) {
        return ConnectError::Dial(dial_error(failure, target));
    }
    let kind = cause.and_then(|e| e.downcast_ref::<std::io::Error>()).map(|e| e.kind());
    match (failure.class, kind) {
        (Class::Config | Class::Usage, _) => ConnectError::Config(failure.text().to_string()),
        (Class::Timeout, None) => ConnectError::Timeout { step: "the TLS handshake", after: limit },
        // rustls's own refusal (a certificate, a name, a protocol) says
        // nothing of the path.
        (_, kind) => ConnectError::Tls(TlsFailure {
            why: failure.text().to_string(),
            cut: kind.filter(|k| *k != std::io::ErrorKind::InvalidData).map(Cut::of),
        }),
    }
}

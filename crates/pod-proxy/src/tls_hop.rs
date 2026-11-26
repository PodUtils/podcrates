//! TLS to a proxy itself, over a stream that is open already: the
//! certificate checked for the proxy's host with the caller's trust, the
//! protocols that the caller offers by ALPN, one bound. The `https://` proxy
//! of a [`crate::Dialer`] and the second proxy of `pod_egress::via` take
//! this one hop, so that a refusal has the same words on each road. A
//! failure is at the stage `Proxy`, never a fall back to cleartext.

use std::sync::Arc;
use std::time::Duration;

use pod_core::Stage;
use pod_tls::{Context, Trust};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls;

use crate::error::DialError;

/// The TLS of a proxy: the caller's context (its provider, its roots, its
/// words) and the trust of the proxy's certificate.
#[derive(Clone)]
pub struct ProxyTls {
    pub context: Arc<Context>,
    pub trust: Trust,
}

impl std::fmt::Debug for ProxyTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyTls").field("trust", &self.trust).finish()
    }
}

/// TLS over `stream` to the proxy that messages name `shown`, checked for
/// `host` with the trust of `tls`, offering `alpn` (none: the configuration
/// as it is), within `limit`.
///
/// * A configuration that the trust cannot give: [`DialError::BadProxy`].
/// * A host that is no server name: [`DialError::InvalidTarget`].
/// * No end within `limit`: [`DialError::Timeout`] at the stage `Proxy`.
/// * The proxy chose no protocol of `alpn` (the alert
///   `no_application_protocol`): [`DialError::ProxyProtocol`].
/// * Each other refusal: [`DialError::ProxyUnreachable`], `TLS: ` and the
///   words of [`pod_tls::handshake_failure`].
pub async fn handshake<S>(
    stream: S,
    shown: &str,
    host: &str,
    tls: &ProxyTls,
    alpn: &[&[u8]],
    limit: Duration,
) -> Result<TlsStream<S>, DialError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let config = tls.context.config_for(&tls.trust).map_err(|e| DialError::BadProxy(e.to_string()))?;
    let config = match alpn {
        [] => config,
        _ => {
            let mut own = (*config).clone();
            own.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
            Arc::new(own)
        }
    };
    // An IPv6 address of a proxy URL is in brackets, which no server name
    // holds.
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let name = rustls_pki_types::ServerName::try_from(bare.to_string())
        .map_err(|e| DialError::InvalidTarget(format!("{host:?} is not a server name: {e}")))?;
    let connector = tokio_rustls::TlsConnector::from(config);
    tokio::time::timeout(limit, connector.connect(name, stream))
        .await
        .map_err(|_| DialError::Timeout {
            step: format!("the TLS handshake with {shown}"),
            after: limit,
            stage: Stage::Proxy,
        })?
        .map_err(|e| {
            // A server with no protocol in common ends the handshake with
            // this alert (rustls's and Go's do), where others choose none.
            let none = e.get_ref().and_then(|inner| inner.downcast_ref::<rustls::Error>()).is_some_and(|r| {
                matches!(r, rustls::Error::AlertReceived(rustls::AlertDescription::NoApplicationProtocol))
            });
            if none && !alpn.is_empty() {
                let offered: Vec<String> = alpn.iter().map(|p| protocol(p)).collect();
                let detail = format!(
                    "it did not choose {} by ALPN, which was asked for: it ended TLS with no_application_protocol",
                    offered.join(" or ")
                );
                return DialError::ProxyProtocol { proxy: shown.to_string(), detail };
            }
            DialError::ProxyUnreachable {
                proxy: shown.to_string(),
                detail: format!("TLS: {}", pod_tls::handshake_failure(&e, tls.context.words())),
            }
        })
}

/// A protocol of ALPN as a message names it: `HTTP/2 (h2)`.
fn protocol(id: &[u8]) -> String {
    match id {
        b"h2" => "HTTP/2 (h2)".into(),
        b"http/1.1" => "HTTP/1.1 (http/1.1)".into(),
        other => String::from_utf8_lossy(other).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::protocol;

    #[test]
    fn a_protocol_of_alpn_is_named_with_its_version() {
        assert_eq!(protocol(b"h2"), "HTTP/2 (h2)");
        assert_eq!(protocol(b"http/1.1"), "HTTP/1.1 (http/1.1)");
        assert_eq!(protocol(b"x-other"), "x-other");
    }
}

//! RFC 8484 over HTTP/2, for a resolver that serves no HTTP/1.1: Quad9
//! answered a GET of HTTP/1.1 with `505`, and the same GET over HTTP/2 with
//! its answer (2026-10-31, from GitHub's runners and through a proxy). RFC
//! 8484 asks for HTTP/2 at least (section 5.2); the other resolvers serve
//! HTTP/1.1 too, which `pod-proxy`'s exchange speaks.

use std::sync::Arc;
use std::time::Duration;

use pod_proxy::Missed;
use pod_proxy::http::Response;
use pod_tls::Words;
use rustls::ClientConfig;
use tokio::io::{AsyncRead, AsyncWrite};

/// `config` offering `h2` alone, so that a server that chose another
/// protocol is refused before a byte of HTTP is sent.
pub fn config_h2(config: &Arc<ClientConfig>) -> Arc<ClientConfig> {
    let mut config = (**config).clone();
    config.alpn_protocols = vec![b"h2".to_vec()];
    Arc::new(config)
}

/// TLS for `name` (a resolver's address, or its name through a proxy) with
/// ALPN `h2` over `stream`, then one GET of `target`. A failed handshake has
/// the `words` of the consumer.
pub async fn get_over<S>(
    config: &Arc<ClientConfig>,
    stream: S,
    name: &str,
    target: &str,
    accept: &str,
    max: usize,
    words: &Words,
) -> Result<Response, String>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    get_over_kept(config, stream, name, target, accept, max, words).await.map_err(|missed| missed.why)
}

/// [`get_over`], with the cut of a failed handshake beside its words; a
/// failure after the handshake has none.
pub(crate) async fn get_over_kept<S>(
    config: &Arc<ClientConfig>,
    stream: S,
    name: &str,
    target: &str,
    accept: &str,
    max: usize,
    words: &Words,
) -> Result<Response, Missed>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut conn = open(config, stream, name, words).await?;
    request(&mut conn, name, target, accept, max).await.map_err(Missed::words)
}

/// An HTTP/2 connection to a resolver, after its handshake. The connection
/// is driven in a task. With the last handle gone h2 ends the connection
/// itself: GOAWAY, then TLS's close_notify, which a resolver reads as an end
/// and not as a torn socket. A drop does that in the background, so that no
/// caller waits for it; it is bounded, for a server that takes no more bytes.
pub(crate) struct H2 {
    client: h2::client::SendRequest<bytes::Bytes>,
    driver: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for H2 {
    fn drop(&mut self) {
        let Some(mut driver) = self.driver.take() else { return };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    if tokio::time::timeout(CLOSE, &mut driver).await.is_err() {
                        driver.abort();
                    }
                });
            }
            Err(_) => driver.abort(),
        }
    }
}

/// TLS with ALPN `h2` and the HTTP/2 handshake over `stream`.
pub(crate) async fn open<S>(config: &Arc<ClientConfig>, stream: S, name: &str, words: &Words) -> Result<H2, Missed>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let server = rustls_pki_types::ServerName::try_from(name.to_string()).map_err(|e| Missed::words(e.to_string()))?;
    let tls = tokio_rustls::TlsConnector::from(config_h2(config))
        .connect(server, stream)
        .await
        .map_err(|e| Missed::tls(&e, words))?;
    if tls.get_ref().1.alpn_protocol() != Some(b"h2") {
        return Err(Missed::words("the resolver did not take HTTP/2"));
    }
    let (client, connection) = h2::client::handshake(tls).await.map_err(|e| Missed::words(format!("HTTP/2: {e}")))?;
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(H2 { client, driver: Some(driver) })
}

/// The bound of the connection's own end, after the answer.
const CLOSE: Duration = Duration::from_secs(2);

/// One GET of `target` on the connection.
pub(crate) async fn request(
    conn: &mut H2,
    name: &str,
    target: &str,
    accept: &str,
    max: usize,
) -> Result<Response, String> {
    let client = conn.client.clone();
    let mut client = client.ready().await.map_err(|e| format!("HTTP/2: {e}"))?;
    let authority = if name.contains(':') { format!("[{name}]") } else { name.to_string() };
    let request = ::http::Request::get(format!("https://{authority}{target}"))
        .header("accept", accept)
        .body(())
        .map_err(|e| format!("HTTP/2: {e}"))?;
    let (response, _) = client.send_request(request, true).map_err(|e| format!("HTTP/2: {e}"))?;
    let response = response.await.map_err(|e| format!("HTTP/2: {e}"))?;
    let status = response.status().as_u16();
    let mut body = response.into_body();
    let mut out = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.map_err(|e| format!("HTTP/2: {e}"))?;
        let _ = body.flow_control().release_capacity(chunk.len());
        if out.len() + chunk.len() > max {
            return Err(format!("an answer over {max} bytes"));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(Response { status, reason: String::new(), headers: Vec::new(), body: out, url: String::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The configuration offers `h2` alone, and the caller's is unchanged.
    #[test]
    fn only_h2_is_offered() {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let context = pod_tls::Context::new(provider, pod_tls::Settings::new(Words::new("podssh")));
        let config = context.config_for(&pod_tls::Trust::Default).unwrap();
        let h2 = config_h2(&config);
        assert_eq!(h2.alpn_protocols, vec![b"h2".to_vec()]);
        assert!(config.alpn_protocols.is_empty());
    }
}

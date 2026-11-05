//! The HTTP client of a consumer, over the same path as each of its
//! connections: its dial (a proxy, the race of addresses, a resolver that
//! the caller gives, as DNS over HTTPS), TLS verified with its context, and
//! bounded waits.
//!
//! * [`Client::https_get`] and [`Client::https_get_accepting`]: a GET over
//!   HTTPS with the trust of the call. Each step (the dial, the handshake,
//!   the exchange) has the bound of the call, and each failure says which
//!   step failed and why.
//! * [`Client::send`]: one [`Request`] of any method to an `http://` or
//!   `https://` URL, with its headers and its body, under one time limit;
//!   `https://` offers ALPN `http/1.1` and takes the roots of the context
//!   ([`pod_tls::Context::roots`]) unless the request gives a trust. The
//!   answer's head is bounded by [`request::MAX_HEAD`], its body by the
//!   request.

mod request;
mod url;

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use pod_core::{BoxedIo, Class, Dial, Failure, Stage, Target};
use pod_tls::{Context, Trust};
use rustls_pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_rustls::client::TlsStream;

pub use request::{MAX_BODY, MAX_HEAD, Request, TIMEOUT};

use crate::host::authority;
use crate::http::{self, Reading, Response};

/// The ALPN of [`Client::send`] over TLS.
const HTTP1: &[&[u8]] = &[b"http/1.1"];

/// The client of a consumer: its TLS context, its dial and its user agent;
/// with the feature `ech`, the reader of the HTTPS records that a sealed
/// hello needs.
#[derive(Clone)]
pub struct Client {
    context: Arc<Context>,
    dial: Arc<dyn Dial>,
    user_agent: String,
    /// The trust of the roots of the context, made at the first request
    /// that needs it, a failure too.
    roots: Arc<OnceLock<Result<Trust, pod_tls::Error>>>,
    #[cfg(feature = "ech")]
    records: Arc<dyn pod_tls::ech::Records>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("dial", &self.dial.label()).field("user_agent", &self.user_agent).finish()
    }
}

impl Client {
    /// A client over `dial`, with the TLS of `context` and no user agent.
    pub fn new(context: Arc<Context>, dial: Arc<dyn Dial>) -> Client {
        Client {
            context,
            dial,
            user_agent: String::new(),
            roots: Arc::new(OnceLock::new()),
            #[cfg(feature = "ech")]
            records: Arc::new(pod_tls::ech::NoRecords),
        }
    }

    /// The `User-Agent` of each request (`podssh/VERSION`); a request that
    /// gives its own keeps it.
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Client {
        self.user_agent = user_agent.into();
        self
    }

    /// The reader of the HTTPS records of a name: where the context asks
    /// for ECH and the name publishes a list, the hello goes sealed. With
    /// none, a hello that ECH asks for goes plain (and strict ECH refuses
    /// it).
    #[cfg(feature = "ech")]
    pub fn with_records(mut self, records: Arc<dyn pod_tls::ech::Records>) -> Client {
        self.records = records;
        self
    }

    /// GET over HTTPS, `Accept: application/json`. The body is capped at
    /// `max_body` bytes.
    pub async fn https_get(
        &self,
        host: &str,
        port: u16,
        path: &str,
        max_body: usize,
        trust: &Trust,
        timeout: Duration,
    ) -> Result<Response, Failure> {
        self.request(host, port, path, &[("Accept", "application/json")], max_body, trust, timeout).await
    }

    /// GET over HTTPS as [`Client::https_get`], with `accept` as the
    /// answer's type: a resolver's JSON API answers `application/dns-json`.
    #[allow(clippy::too_many_arguments)]
    pub async fn https_get_accepting(
        &self,
        host: &str,
        port: u16,
        path: &str,
        accept: &str,
        max_body: usize,
        trust: &Trust,
        timeout: Duration,
    ) -> Result<Response, Failure> {
        self.request(host, port, path, &[("Accept", accept)], max_body, trust, timeout).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn request(
        &self,
        host: &str,
        port: u16,
        path: &str,
        headers: &[(&str, &str)],
        max_body: usize,
        trust: &Trust,
        timeout: Duration,
    ) -> Result<Response, Failure> {
        let mut tls = self.open_tls(&*self.dial, host, port, trust, timeout, &[]).await?;
        let host_header = if port == 443 { host.to_string() } else { authority(host, port) };
        let exchange = http::exchange(&mut tls, &self.user_agent, "GET", &host_header, path, headers, b"", max_body);
        tokio::time::timeout(timeout, exchange)
            .await
            .map_err(|_| {
                let text = format!("the HTTPS request timed out after {}s", timeout.as_secs());
                Failure::new(Stage::Connect, Class::Timeout, text)
            })?
            .map_err(|e| {
                let text = format!("the HTTPS request failed: {e}");
                Failure::caused(Stage::Connect, Class::of_io(e.kind()), text, e)
            })
    }

    /// Send `request` and read its answer, the whole within its time limit.
    /// A failure says the method and the URL with no query, then why, at
    /// the stage of the step that failed.
    pub async fn send(&self, request: Request) -> Result<Response, Failure> {
        let what = format!("{} {}", request.method, url::without_query(&request.url));
        let timeout = request.timeout;
        match tokio::time::timeout(timeout, self.send_now(&request)).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(e)) => Err(Failure::caused(e.stage, e.class, format!("{what}: {}", e.text()), e)),
            Err(_) => Err(Failure::new(
                Stage::Connect,
                Class::Timeout,
                format!("{what}: no answer in {} s", timeout.as_secs()),
            )),
        }
    }

    async fn send_now(&self, request: &Request) -> Result<Response, Failure> {
        let target = url::target(&request.url).map_err(|e| Failure::new(Stage::Connect, Class::Usage, e))?;
        let head =
            request.head(&target, &self.user_agent).map_err(|e| Failure::new(Stage::Connect, Class::Usage, e))?;
        let dial = request.via.as_deref().unwrap_or(&*self.dial);
        let reading = Reading { max_head: MAX_HEAD, max_body: request.max_body, head_only: request.method == "HEAD" };
        let body = request.body.as_deref().unwrap_or_default();
        if target.tls {
            let trust = match &request.trust {
                Some(trust) => trust.clone(),
                None => self.roots()?,
            };
            let tls = self.open_tls(dial, &target.host, target.port, &trust, request.timeout, HTTP1).await;
            exchange(tls.map_err(network_or_tls)?, &head, body, reading).await
        } else {
            let deadline = tokio::time::Instant::now() + request.timeout;
            let tcp = dial.dial(&Target::new(&target.host, target.port), deadline).await?;
            exchange(tcp.io, &head, body, reading).await
        }
    }

    /// The trust of the roots of the context, with no ALPN, so that a hello
    /// offers what the call asks for.
    fn roots(&self) -> Result<Trust, Failure> {
        let made = self.roots.get_or_init(|| {
            let config = self.context.client_config_with(&[], &[])?;
            Trust::caller(Arc::new(config))
        });
        made.clone().map_err(Failure::from)
    }

    /// The dial and a verified TLS handshake for `host` that offers `alpn`.
    /// The configuration and the name are checked before anything connects.
    async fn open_tls(
        &self,
        dial: &dyn Dial,
        host: &str,
        port: u16,
        trust: &Trust,
        timeout: Duration,
        alpn: &[&[u8]],
    ) -> Result<TlsStream<BoxedIo>, Failure> {
        let config = self.context.config_for(trust)?;
        ServerName::try_from(host.to_string())
            .map_err(|e| Failure::new(Stage::Tls, Class::Usage, format!("{host:?} is not a valid server name: {e}")))?;
        let target = Target::new(host, port);
        #[cfg(feature = "ech")]
        {
            let _ = config;
            let route = pod_tls::ech::Route {
                target: &target,
                server_name: host,
                trust,
                dial,
                records: &*self.records,
                limit: timeout,
                alpn,
            };
            pod_tls::ech::open(&self.context, &route).await.map(|(tls, _)| tls)
        }
        #[cfg(not(feature = "ech"))]
        {
            let tcp = dial.dial(&target, tokio::time::Instant::now() + timeout).await?;
            if alpn.is_empty() {
                return self.context.tls_over(tcp.io, host, trust, timeout).await;
            }
            let mut config = (*config).clone();
            config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
            pod_tls::tls_over(tcp.io, host, Arc::new(config), timeout, self.context.words()).await
        }
    }
}

/// A handshake that the network ended (a reset, an abort, a close, a
/// refusal, a time limit of I/O) is at [`Stage::Network`]: a policy of the
/// host or of its network ended it, not trust or the clock. A refusal of
/// rustls (a certificate, a name) stays at [`Stage::Tls`].
fn network_or_tls(failure: Failure) -> Failure {
    use std::io::ErrorKind as K;
    if failure.stage != Stage::Tls {
        return failure;
    }
    let mut next = std::error::Error::source(&failure);
    while let Some(e) = next {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            return match io.kind() {
                K::ConnectionReset | K::ConnectionAborted | K::BrokenPipe | K::UnexpectedEof | K::TimedOut => {
                    failure.at(Stage::Network)
                }
                K::ConnectionRefused => failure.at(Stage::Network),
                _ => failure,
            };
        }
        next = e.source();
    }
    failure
}

/// Write the head and the body of a request on `io`, and read the answer.
async fn exchange<S>(mut io: S, head: &str, body: &[u8], reading: Reading) -> Result<Response, Failure>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let sent = async {
        io.write_all(head.as_bytes()).await?;
        io.write_all(body).await?;
        io.flush().await
    };
    sent.await
        .map_err(|e| Failure::caused(Stage::Connect, Class::of_io(e.kind()), format!("sending the request: {e}"), e))?;
    http::read_answer(&mut io, Vec::new(), reading)
        .await
        .map_err(|e| Failure::caused(Stage::Connect, Class::of_io(e.kind()), format!("reading the answer: {e}"), e))
}

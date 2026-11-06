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
//! * [`Client::send_streamed`]: the same request; the head of the answer
//!   within the time limit, and its body as a reader ([`Streamed`]) with no
//!   bound of its size and a limit for each wait. [`Client::send`] reads
//!   through it.
//! * Both follow redirects when the request asks
//!   ([`Request::redirects`]); the answer gives the URL that it came from.
//! * Both keep a connection open after its answer and send the next request
//!   of the same key on it ([`Client::with_keep_alive`], on by default).
//!   [`Client::https_get`] does not: it sends `Connection: close`.

pub mod pool;
mod redirect;
mod request;
mod streamed;
mod url;

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use pod_core::{BoxedIo, Class, Dial, Failure, Stage};
use pod_tls::{Context, Trust};
use rustls_pki_types::ServerName;
use tokio_rustls::client::TlsStream;

pub use request::{BODY_IDLE, MAX_BODY, MAX_HEAD, Request, TIMEOUT};
pub use streamed::Streamed;

use crate::host::authority;
use crate::http::{self, Response};
use pool::{Key, Pool};
use streamed::Bounds;

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
    /// Send each request of [`Client::send`] and [`Client::send_streamed`]
    /// on an idle connection of the same key when the pool has one.
    keep_alive: bool,
    /// The idle connections, shared by the clones.
    pool: Arc<Pool>,
    #[cfg(feature = "ech")]
    records: Arc<dyn pod_tls::ech::Records>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("dial", &self.dial.label())
            .field("user_agent", &self.user_agent)
            .field("keep_alive", &self.keep_alive)
            .finish()
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
            keep_alive: true,
            pool: Arc::new(Pool::default()),
            #[cfg(feature = "ech")]
            records: Arc::new(pod_tls::ech::NoRecords),
        }
    }

    /// Keep connections open for the next requests (`true`, the default),
    /// or send `Connection: close` and open a new connection for each
    /// request (`false`). The clones share the idle connections
    /// ([`pool`]). A connection belongs to the tokio runtime that opened
    /// it: give each runtime its own client.
    pub fn with_keep_alive(mut self, on: bool) -> Client {
        self.keep_alive = on;
        self
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

    /// Send `request` and read its answer, the whole within its time limit,
    /// each redirect that it asks for too. A failure says the method and the
    /// URL with no query, then why, at the stage of the step that failed.
    pub async fn send(&self, request: Request) -> Result<Response, Failure> {
        let limit = u64::try_from(request.max_body).unwrap_or(u64::MAX);
        let bounds = Bounds { head_only: request.method == "HEAD", limit: Some(limit), idle: None };
        within(&request, async { self.follow(&request, bounds).await?.into_response().await }).await
    }

    /// Send `request` and read the head of its answer within its time
    /// limit, the heads of each redirect that it asks for too; the caller reads the body from the [`Streamed`] as it comes,
    /// with no bound of its size, each read within
    /// [`Request::body_idle`]. A failure has the words of
    /// [`Client::send`].
    pub async fn send_streamed(&self, request: Request) -> Result<Streamed, Failure> {
        let bounds = Bounds { head_only: request.method == "HEAD", limit: None, idle: Some(request.body_idle) };
        within(&request, self.follow(&request, bounds)).await
    }

    /// The connection of `request`, the request sent on it, and the head of
    /// the answer. An idle connection of the same key goes first; when it
    /// fails before a byte of the answer, a `GET` or a `HEAD` goes again,
    /// once, on a new connection.
    async fn open(&self, request: &Request, bounds: Bounds) -> Result<Streamed, Failure> {
        let target = url::target(&request.url).map_err(|e| Failure::new(Stage::Connect, Class::Usage, e))?;
        let keep = self.keep_alive && !request.says_close();
        let head =
            request.head(&target, &self.user_agent, keep).map_err(|e| Failure::new(Stage::Connect, Class::Usage, e))?;
        let dial = request.via.clone().unwrap_or_else(|| self.dial.clone());
        let body = request.body.as_deref().unwrap_or_default();
        let trust = match (&request.trust, target.tls) {
            (_, false) => None,
            (Some(trust), true) => Some(trust.clone()),
            (None, true) => Some(self.roots()?),
        };
        let key = keep.then(|| Key::new(&target, &dial, trust.as_ref()));
        if let Some(key) = &key
            && let Some(io) = self.pool.take(key)
        {
            match streamed::start(io, &request.url, &head, body, bounds).await {
                Ok(answer) => return Ok(answer.homed(&self.pool, key.clone())),
                Err(failed) if failed.silent && matches!(request.method, "GET" | "HEAD") => {
                    tracing::debug!(why = %failed.failure, "an idle connection failed; the request goes on a new one");
                }
                Err(failed) => return Err(failed.failure),
            }
        }
        let io = self.connect(&*dial, &target, trust.as_ref(), request.timeout).await?;
        let answer = streamed::start(io, &request.url, &head, body, bounds).await.map_err(|failed| failed.failure)?;
        Ok(match key {
            Some(key) => answer.homed(&self.pool, key),
            None => answer,
        })
    }

    /// A new connection to `target`: TLS with `trust`, or plain TCP through
    /// the proxy of `http_proxy`, not of `https_proxy`.
    async fn connect(
        &self,
        dial: &dyn Dial,
        target: &url::Target,
        trust: Option<&Trust>,
        timeout: Duration,
    ) -> Result<BoxedIo, Failure> {
        match trust {
            Some(trust) => {
                let tls = self.open_tls(dial, &target.host, target.port, trust, timeout, HTTP1).await;
                Ok(Box::new(tls.map_err(network_or_tls)?))
            }
            None => {
                let deadline = tokio::time::Instant::now() + timeout;
                Ok(dial.dial_http(&pod_core::Target::new(&target.host, target.port), deadline).await?.io)
            }
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
        let target = pod_core::Target::new(host, port);
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

/// `work` within the time limit of `request`. A failure says the method
/// and the URL with no query, then why.
async fn within<T>(request: &Request, work: impl Future<Output = Result<T, Failure>>) -> Result<T, Failure> {
    let what = format!("{} {}", request.method, url::without_query(&request.url));
    match tokio::time::timeout(request.timeout, work).await {
        Ok(Ok(done)) => Ok(done),
        Ok(Err(e)) => Err(Failure::caused(e.stage, e.class, format!("{what}: {}", e.text()), e)),
        Err(_) => Err(Failure::new(
            Stage::Connect,
            Class::Timeout,
            format!("{what}: no answer in {} s", request.timeout.as_secs()),
        )),
    }
}

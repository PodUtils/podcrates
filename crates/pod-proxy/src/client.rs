//! A GET over HTTPS through the same path as each connection of the
//! consumer: its dial (a proxy, the race of addresses), TLS verified with
//! its context, and bounded waits. Each step (the dial, the handshake, the
//! exchange) has the bound of the call, and each failure says which step
//! failed and why.

use std::sync::Arc;
use std::time::Duration;

use pod_core::{BoxedIo, Class, Dial, Failure, Stage, Target};
use pod_tls::{Context, Trust};
use rustls_pki_types::ServerName;
use tokio_rustls::client::TlsStream;

use crate::host::authority;
use crate::http::{self, Response};

/// The GET client of a consumer: its TLS context, its dial and its user
/// agent; with the feature `ech`, the reader of the HTTPS records that a
/// sealed hello needs.
#[derive(Clone)]
pub struct Client {
    context: Arc<Context>,
    dial: Arc<dyn Dial>,
    user_agent: String,
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
            #[cfg(feature = "ech")]
            records: Arc::new(pod_tls::ech::NoRecords),
        }
    }

    /// The `User-Agent` of each request (`podssh/VERSION`).
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
        let mut tls = self.open_tls(host, port, trust, timeout).await?;
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

    /// The dial and a verified TLS handshake for `host`. The configuration
    /// and the name are checked before anything connects.
    async fn open_tls(
        &self,
        host: &str,
        port: u16,
        trust: &Trust,
        timeout: Duration,
    ) -> Result<TlsStream<BoxedIo>, Failure> {
        self.context.config_for(trust)?;
        ServerName::try_from(host.to_string())
            .map_err(|e| Failure::new(Stage::Tls, Class::Usage, format!("{host:?} is not a valid server name: {e}")))?;
        let target = Target::new(host, port);
        #[cfg(feature = "ech")]
        {
            let route = pod_tls::ech::Route {
                target: &target,
                server_name: host,
                trust,
                dial: &*self.dial,
                records: &*self.records,
                limit: timeout,
                alpn: &[],
            };
            pod_tls::ech::open(&self.context, &route).await.map(|(tls, _)| tls)
        }
        #[cfg(not(feature = "ech"))]
        {
            let tcp = self.dial.dial(&target, tokio::time::Instant::now() + timeout).await?;
            self.context.tls_over(tcp.io, host, trust, timeout).await
        }
    }
}

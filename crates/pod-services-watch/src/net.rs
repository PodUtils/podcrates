//! The way out of each check: the TLS context and the trust, the dial, the
//! HTTP client over them, the resolvers of DNS over HTTPS, the services
//! (for a watch that names another service), the bases of the public
//! APIs, the day of the run and the bound of each step. The runner fills it
//! once; a test gives its stand-ins.

use std::sync::Arc;
use std::time::Duration;

use pod_core::{BoxedIo, Dial, Target};
use pod_proxy::Client;
use pod_proxy::client::{Request, Streamed};
use pod_proxy::http::Response;
use pod_services::{Catalog, Date};
use pod_tls::{Context, Trust};
use tokio::time::Instant;
use tokio_rustls::client::TlsStream;

use crate::Finding;

/// The bases of the public APIs that some watch kinds read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bases {
    /// GitHub's REST API (`https://api.github.com`).
    pub github_api: String,
    /// The raw files of GitHub (`https://raw.githubusercontent.com`).
    pub github_raw: String,
    /// The API of crates.io (`https://crates.io`).
    pub crates_api: String,
    /// The archives of crates.io (`https://static.crates.io`).
    pub crates_static: String,
    /// IANA's bootstrap of the RDAP servers of each top-level domain.
    pub rdap_bootstrap: String,
    /// The RDAP server for a top-level domain that the bootstrap does not
    /// name (`https://rdap.org/`).
    pub rdap_fallback: String,
}

impl Bases {
    /// The public bases.
    pub fn public() -> Bases {
        Bases {
            github_api: "https://api.github.com".into(),
            github_raw: "https://raw.githubusercontent.com".into(),
            crates_api: "https://crates.io".into(),
            crates_static: "https://static.crates.io".into(),
            rdap_bootstrap: "https://data.iana.org/rdap/dns.json".into(),
            rdap_fallback: "https://rdap.org/".into(),
        }
    }

    /// Each base at `origin` (`https://stand.test:PORT`), for a test.
    pub fn at(origin: &str) -> Bases {
        Bases {
            github_api: format!("{origin}/api"),
            github_raw: format!("{origin}/raw"),
            crates_api: origin.to_string(),
            crates_static: format!("{origin}/static"),
            rdap_bootstrap: format!("{origin}/rdap/dns.json"),
            rdap_fallback: format!("{origin}/rdap-any/"),
        }
    }
}

/// The way out of each check.
#[derive(Clone)]
pub struct Net {
    pub context: Arc<Context>,
    pub dial: Arc<dyn Dial>,
    /// The trust of each TLS connection and each HTTPS request.
    pub trust: Trust,
    pub client: Client,
    /// The resolvers of a DNS watch, with their trust.
    pub dns: pod_dns::Settings,
    /// The services that a watch names (`target`), with the overlay.
    pub services: Catalog,
    pub bases: Bases,
    /// A token of GitHub's API, sent to `bases.github_api` alone.
    pub github_token: Option<String>,
    /// The day of the run: the age of a pin, the end of a registration.
    pub today: Date,
    /// The bound of each step: a dial, a handshake, the head of an answer.
    pub limit: Duration,
    /// The host has a route for IPv6. With none, a watch does not check an
    /// IPv6 address, and says so: a runner of GitHub has no IPv6.
    pub ipv6: bool,
    pub user_agent: String,
}

impl std::fmt::Debug for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Net")
            .field("dial", &self.dial.label())
            .field("bases", &self.bases)
            .field("today", &self.today)
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

/// The longest body that a check reads whole: a document, a source file,
/// the archive of a crate.
pub const MAX_BODY: usize = 32 << 20;

impl Net {
    /// A way out over `dial` with `trust`, on `today`: the public bases, the
    /// built-in services, the default resolvers with the same trust, and a
    /// bound of 30 s for each step.
    pub fn new(context: Arc<Context>, dial: Arc<dyn Dial>, trust: Trust, today: Date) -> Net {
        let user_agent = format!("pod-services-watch/{}", env!("CARGO_PKG_VERSION"));
        let client = Client::new(context.clone(), dial.clone()).with_user_agent(user_agent.clone());
        let dns = pod_dns::Settings { trust: trust.clone(), memory: false, ..pod_dns::Settings::default() };
        Net {
            context,
            dial,
            trust,
            client,
            dns,
            services: Catalog::new(),
            bases: Bases::public(),
            github_token: None,
            today,
            limit: Duration::from_secs(30),
            ipv6: true,
            user_agent,
        }
    }

    /// The request of `method` to `url` with the trust, the bound and the
    /// token of GitHub's API where the URL is under its base.
    pub fn request(&self, method: &'static str, url: &str) -> Request {
        let mut request = Request::new(method, url).trust(self.trust.clone()).timeout(self.limit).max_body(MAX_BODY);
        if let Some(token) = self.github_token.as_deref().filter(|_| url.starts_with(&self.bases.github_api)) {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        request
    }

    /// The answer to `request`, any status.
    pub async fn send(&self, request: Request) -> Result<Response, Finding> {
        let url = request.url.clone();
        self.client.send(request).await.map_err(|e| Finding::failed(format!("{}: {e}", shown(&url))))
    }

    /// The answer to a GET of `url` that follows up to five redirects; a
    /// status that is not 2xx is a failure.
    pub async fn get_ok(&self, url: &str, accept: &str) -> Result<Response, Finding> {
        let answer = self.send(self.request("GET", url).header("Accept", accept).redirects(5)).await?;
        if !answer.is_success() {
            return Err(Finding::failed(format!("{} answered HTTP {}", shown(url), answer.status)));
        }
        Ok(answer)
    }

    /// The JSON document at `url`.
    pub async fn json(&self, url: &str) -> Result<serde_json::Value, Finding> {
        let answer = self.get_ok(url, "application/json").await?;
        serde_json::from_slice(&answer.body).map_err(|e| Finding::failed(format!("{} is not JSON: {e}", shown(url))))
    }

    /// The head of the answer to `request` and its body as a reader.
    pub async fn streamed(&self, request: Request) -> Result<Streamed, Finding> {
        let url = request.url.clone();
        self.client.send_streamed(request).await.map_err(|e| Finding::failed(format!("{}: {e}", shown(&url))))
    }

    /// A connection to `host:port` (a name or an address) through the dial.
    pub async fn tcp(&self, host: &str, port: u16) -> Result<BoxedIo, Finding> {
        let target = Target::new(host, port);
        let deadline = Instant::now() + self.limit;
        let connected =
            self.dial.dial(&target, deadline).await.map_err(|e| Finding::failed(format!("{target}: {e}")))?;
        Ok(connected.io)
    }

    /// A TLS connection to `host:port` that verifies `server_name`.
    pub async fn tls(&self, host: &str, port: u16, server_name: &str) -> Result<TlsStream<BoxedIo>, Finding> {
        let io = self.tcp(host, port).await?;
        self.context
            .tls_over(io, server_name, &self.trust, self.limit)
            .await
            .map_err(|e| Finding::failed(format!("TLS for {server_name} at {}: {e}", Target::new(host, port))))
    }

    /// [`Net::tls`] that offers `alpn`; the stream tells the protocol that
    /// the server chose.
    pub async fn tls_alpn(
        &self,
        host: &str,
        port: u16,
        server_name: &str,
        alpn: &[&[u8]],
    ) -> Result<TlsStream<BoxedIo>, Finding> {
        let at = || format!("TLS for {server_name} at {}", Target::new(host, port));
        let mut config =
            (*self.context.config_for(&self.trust).map_err(|e| Finding::failed(format!("{}: {e}", at())))?).clone();
        config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
        let io = self.tcp(host, port).await?;
        pod_tls::tls_over(io, server_name, Arc::new(config), self.limit, self.context.words())
            .await
            .map_err(|e| Finding::failed(format!("{}: {e}", at())))
    }

    /// The way of the DNS watches: the resolvers of [`Net::dns`] through the
    /// dial.
    pub fn way(&self) -> pod_dns::Way<'_> {
        pod_dns::Way { settings: &self.dns, context: &self.context, dial: &*self.dial, kept: None }
    }

    /// The deadline of one step that starts now.
    pub fn deadline(&self) -> Instant {
        Instant::now() + self.limit
    }
}

/// A URL with no query, for a line: a query may hold a token.
pub fn shown(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

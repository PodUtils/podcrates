//! What the consumer gives the tunnel: the TLS of the binary, the way out,
//! the resolver, its own services, the relay and WARP, and the settings
//! (`docs/design.md`, section 1, rules 4 to 7 and 10). The crate keeps no
//! process state.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use pod_core::{BoxedIo, Class, Dial, Event, Failure, Report, Stage};

use crate::cloudflare::stream::ConnectRequest;
use crate::ingress::Service;
use crate::proxy::{EdgeReader, EdgeWriter};

/// The services of the consumer that a rule of the ingress can name
/// ([`Service::Static`] and [`Service::Custom`]): the consumer builds and
/// serves them.
pub trait ServiceDispatch: Send + Sync {
    /// Check and build the service of rule `index` before the tunnel
    /// starts. An error stops the start (a missing directory, for example).
    fn prepare(&self, index: usize, service: &Service) -> Result<(), Failure>;

    /// Answer one request of the edge for rule `index`. `host` is the host
    /// of the request.
    fn serve<'a>(
        &'a self,
        index: usize,
        request: &'a ConnectRequest,
        host: &'a str,
        reader: EdgeReader,
        writer: EdgeWriter,
    ) -> BoxFuture<'a, Result<(), Failure>>;
}

/// A consumer with no services of its own: a rule that names one is
/// refused when the tunnel starts.
pub struct NoServices;

impl ServiceDispatch for NoServices {
    fn prepare(&self, _index: usize, service: &Service) -> Result<(), Failure> {
        let text = format!("the service {} needs the consumer, which gives none", service.describe());
        Err(Failure::new(Stage::Host, Class::Config, text))
    }

    fn serve<'a>(
        &'a self,
        _index: usize,
        _request: &'a ConnectRequest,
        _host: &'a str,
        _reader: EdgeReader,
        _writer: EdgeWriter,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async { Err(Failure::new(Stage::Origin, Class::Config, "the consumer serves no service")) })
    }
}

/// One WARP session and the QUIC endpoint in it, for the leg `quic+warp`.
pub struct WarpSession {
    /// The QUIC endpoint over the datagrams of the session.
    pub endpoint: noq::Endpoint,
    /// True when the session closed.
    pub closed: Box<dyn Fn() -> bool + Send + Sync>,
    /// What keeps the session open; dropped with the session.
    pub keep: Box<dyn std::any::Any + Send + Sync>,
}

/// The WARP sessions of the consumer (`pod-warp` through the consumer's
/// device and identity).
pub trait WarpSessions: Send + Sync {
    /// Open a new session. The first call registers the device or loads
    /// it; a later call opens a session with the same device.
    fn open(&self) -> BoxFuture<'_, Result<WarpSession, Failure>>;
}

/// The SRV records of a name from the system resolver of the consumer
/// (`/etc/resolv.conf`): the second step of the discovery of the edge.
pub trait SystemSrv: Send + Sync {
    /// The targets and the ports of the SRV records of `name`.
    fn srv<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<(String, u16)>, Failure>>;
}

/// The relay for the leg `http2+relay`.
#[derive(Clone)]
pub struct EdgeRelay {
    /// A TCP connection through the relay.
    pub dial: Arc<dyn Dial>,
    /// The run of the relay: the limits of the relay in use give the budget
    /// of a connection (`pod_relay::relay::byte_budget`, `time_budget`).
    pub context: Arc<pod_relay::Context>,
}

impl std::fmt::Debug for EdgeRelay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EdgeRelay").field("dial", &self.dial.label()).finish_non_exhaustive()
    }
}

/// The settings of a tunnel. A consumer fills them from its variables.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The edge addresses in place of the discovery (podnet's `PODNET_EDGE`,
    /// cloudflared's `--edge`).
    pub edge: Option<Vec<SocketAddr>>,
    /// What the tunnel does with UDP (podnet's `PODNET_UDP`).
    pub udp: crate::udp::Choice,
    /// The most time for the provision of a quick tunnel (podnet's
    /// `PODNET_READY_TIMEOUT` makes it shorter).
    pub provision_budget: Duration,
    /// The name and the version of the client that the edge sees
    /// (`podnet/0.4.0`).
    pub client_version: String,
    /// A check of the public URL that fails is an error, not a warning
    /// (podnet's `PODNET_VERIFY_STRICT`).
    pub verify_strict: bool,
    /// The path of the WebSocket of a forward rule that gives no path. The
    /// client of the consumer dials the same path: the default is podnet's
    /// wire, `/__podnet/ws` ([`crate::ws::DEFAULT_PATH`]).
    pub forward_path: String,
    /// The words of the consumer in the texts for the user.
    pub words: Words,
}

/// The words of the consumer in the texts for the user: its name, its flags
/// and its variables. The default words name no consumer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Words {
    /// The name of the client: in "NAME speaks cloudflared ...", in the
    /// hello page, in the realm of the credential in front of the tunnel and
    /// in the names of the metrics.
    pub name: String,
    /// The setting that chooses the protocol of the edge, before a value:
    /// `--protocol` gives "use --protocol http2".
    pub protocol: String,
    /// The setting that turns QUIC on UDP off: `PODNET_UDP=off`.
    pub udp_off: String,
    /// What makes a check of the URL that is not proved an error.
    pub strict: String,
    /// What helps when this host cannot resolve the public name.
    pub resolve: String,
    /// What helps when an HTTP request comes to a raw TCP origin.
    pub tcp_origin: String,
}

impl Default for Words {
    fn default() -> Self {
        Self {
            name: "pod-cloudflare".into(),
            protocol: "the protocol".into(),
            udp_off: "the UDP choice off".into(),
            strict: "the strict check of the URL makes this an error".into(),
            resolve: "give the address of the name to the resolver, or do not check the URL".into(),
            tcp_origin: "Use a forward and its client, or an HTTP origin.".into(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        let version = concat!("pod-cloudflare/", env!("CARGO_PKG_VERSION"));
        Self {
            edge: None,
            udp: crate::udp::Choice::Probe,
            provision_budget: crate::quicktunnel::PROVISION_BUDGET,
            client_version: version.to_string(),
            verify_strict: false,
            forward_path: crate::ws::DEFAULT_PATH.to_string(),
            words: Words::default(),
        }
    }
}

/// What the consumer gives the tunnel.
#[derive(Clone)]
pub struct Host {
    /// The TLS of the binary: the provider, the roots, the groups.
    pub tls: Arc<pod_tls::Context>,
    /// The way out of each TCP connection: direct or through the proxy.
    pub dial: Arc<dyn Dial>,
    /// The resolver: the SRV record of the edge by DNS over HTTPS, and the
    /// step that answers a name.
    pub resolver: Arc<pod_dns::Resolver>,
    /// The TLS configuration for an origin with `noTLSVerify`, which the
    /// consumer builds with the check that it chooses. The crate makes no
    /// TLS with no check: with `None`, the dial of such an origin fails
    /// (the class `Policy`).
    pub origin_no_verify: Option<Arc<rustls::ClientConfig>>,
    /// The SRV records of the system resolver; `None`: the discovery skips
    /// this step.
    pub system_srv: Option<Arc<dyn SystemSrv>>,
    /// The services of the consumer.
    pub services: Arc<dyn ServiceDispatch>,
    /// The lines for the user (events) and the counters of the decisions
    /// (the leg that won).
    pub report: Report,
    /// The relay of the leg `http2+relay`.
    pub relay: Option<EdgeRelay>,
    /// The WARP sessions of the leg `quic+warp`.
    pub warp: Option<Arc<dyn WarpSessions>>,
    pub settings: Settings,
}

impl Host {
    /// A host with the TLS `tls` and the way out `dial`, the resolver of
    /// `pod-dns` with its default settings over the same way, no services,
    /// no relay and no WARP.
    pub fn new(tls: Arc<pod_tls::Context>, dial: Arc<dyn Dial>) -> Host {
        let resolver = pod_dns::Resolver::new(pod_dns::Settings::default()).with_doh(tls.clone(), dial.clone());
        Host {
            tls,
            dial,
            resolver: Arc::new(resolver),
            origin_no_verify: None,
            system_srv: None,
            services: Arc::new(NoServices),
            report: Report::default(),
            relay: None,
            warp: None,
            settings: Settings::default(),
        }
    }

    /// Give the consumer a line for the user. The line goes to `tracing`
    /// at the level debug too.
    pub fn event(&self, event: Event) {
        tracing::debug!(kind = event.kind, "{}", event.text);
        self.report.event(event);
    }

    /// The HTTP client over the TLS and the way out of the host.
    pub fn client(&self) -> pod_proxy::Client {
        pod_proxy::Client::new(self.tls.clone(), self.dial.clone())
    }

    /// Send `request` with [`Host::client`]. A failure keeps its stage
    /// ([`crate::fail::stage_of`]) and names the request, with no query.
    pub async fn send(&self, request: pod_proxy::client::Request) -> Result<pod_proxy::http::Response, Failure> {
        let what = request.line();
        self.client().send(request).await.map_err(|failure| sent(failure, &what))
    }

    /// The step of the chain of the resolver that answers `host` now:
    /// `literal`, `pin`, `system`, `doh` or `none`.
    pub async fn answering_step(&self, host: &str) -> &'static str {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let found = self.resolver.find(host, 0, deadline, None).await;
        found.map_or("none", |f| f.source.name())
    }

    /// The label of the proxy that a TCP connection to `host:port` goes
    /// through, or `None` for a direct connection.
    pub fn proxy_for(&self, host: &str, port: u16) -> Option<String> {
        self.dial.proxy_label(&pod_core::Target::new(host, port))
    }

    /// Open TCP to `target` through the way out, in at most `limit`.
    pub async fn connect(&self, target: &pod_core::Target, limit: Duration) -> Result<BoxedIo, Failure> {
        let deadline = tokio::time::Instant::now() + limit;
        let connected = self.dial.dial(target, deadline).await?;
        Ok(connected.io)
    }
}

/// A failed request: the request and why, each once. A handshake that the
/// network ended is said in the words of [`crate::fail::tls_text`] (a reset
/// during the TLS handshake), not as a bare error of I/O; the cause of the
/// failure stays, without the words that this text says in its place.
fn sent(failure: Failure, what: &str) -> Failure {
    let Some(io) = failure.io_cause().filter(|_| failure.stage == Stage::Network) else {
        return crate::fail::wrap(failure, what);
    };
    let text = format!("{what}: TLS: {}", crate::fail::tls_text(io));
    let (stage, class) = (failure.stage, failure.class);
    match failure.into_source() {
        Some(cause) => Failure::caused(stage, class, text, cause),
        None => Failure::new(stage, class, text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reset of the connection during the TLS handshake of a request is
    /// said as such, with the request.
    #[tokio::test]
    async fn a_reset_in_the_tls_handshake_is_named() {
        pod_testkit::need_loopback!();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(conn) = conn else { break };
                // The ClientHello stays unread: the close sends a reset.
                std::thread::sleep(Duration::from_millis(200));
                drop(conn);
            }
        });
        let host = crate::testing::host();
        let url = format!("https://localhost:{port}/tunnel");
        let failure = host.send(pod_proxy::client::Request::new("POST", url)).await.unwrap_err();
        let text = failure.chain();
        assert!(
            text.contains("reset during the TLS handshake") || text.contains("closed during the TLS handshake"),
            "{text}"
        );
        assert_eq!(text.matches(&format!("POST https://localhost:{port}/tunnel")).count(), 1, "{text}");
        assert_eq!(failure.stage, Stage::Network, "{text}");
    }

    /// A request through a proxy that does not answer names the request
    /// and the proxy once each (PodUtils/podnet#658).
    #[tokio::test]
    async fn a_dead_proxy_names_the_request_once() {
        pod_testkit::need_loopback!();
        let (_held, at) = pod_testkit::refused::refused();
        let proxy = pod_proxy::Proxy::parse(&format!("http://{at}")).expect("a proxy");
        let settings = pod_proxy::Settings::new("pod-cloudflare", pod_proxy::ProxyChoice::Via(proxy));
        let dial = pod_proxy::Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new()));
        let host = Host::new(crate::testing::tls(), Arc::new(dial));
        let request = pod_proxy::client::Request::new("POST", "https://api.example/tunnel?x=1");
        let failure = host.send(request).await.unwrap_err();
        let text = failure.chain();
        assert_eq!(text.matches("POST https://api.example/tunnel").count(), 1, "{text}");
        assert_eq!(text.matches("could not reach the proxy").count(), 1, "{text}");
        assert_eq!(failure.stage, Stage::Proxy, "{text}");
    }

    /// The default words name no consumer; the path of a forward keeps
    /// podnet's wire, which its client dials.
    #[test]
    fn the_default_words_name_no_consumer() {
        let settings = Settings::default();
        let words = format!("{:?}", settings.words).to_ascii_lowercase();
        assert!(!words.contains("podnet"), "{words}");
        assert!(!words.contains("--"), "{words}");
        assert_eq!(settings.forward_path, "/__podnet/ws");
        assert!(settings.client_version.starts_with("pod-cloudflare/"));
    }
}

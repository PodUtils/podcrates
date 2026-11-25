//! Test support (feature `standin`): a stand-in of the quick-tunnel service
//! and of the Cloudflare edge, so that a test runs a quick tunnel with no
//! call to Cloudflare. A consumer runs its binary against it: the binary
//! takes the stand-in API as its service URL, the edge address in place of
//! the discovery and the CA file of the stand-in as its roots.
//!
//! * [`api`]: `POST /tunnel` on the loopback, with a 429 mode.
//! * [`edge`]: TLS on TCP, HTTP/2, `registerConnection`, visitor streams.
//! * [`front`]: an HTTP server that maps the `Host` of a visitor to a tunnel.
//! * [`ca`]: a test CA, made at run time. The host of a test trusts it as
//!   the CA file of its TLS context, as a consumer does with
//!   `SSL_CERT_FILE`. No hook is in the code of the crate.
//! * [`proxy`]: a CONNECT proxy that keeps the authorities.
//! * [`quic`]: a QUIC connection in memory, the edge on one side.
//!
//! The stand-in tests the crate against the protocol as the crate reads
//! it. It does not show that the real edge accepts a change. It has no QUIC
//! leg.
//!
//! Each test that uses it starts with `pod_testkit::need_loopback!()`.

pub mod api;
pub mod ca;
pub mod edge;
pub mod front;
pub mod proxy;
pub mod quic;

use std::io::{Read as _, Write as _};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::Host;
use crate::config::{Config, Protocol, Transport};
use crate::ingress::{Ingress, Service};
use crate::tunnel::{TunnelBuilder, TunnelHandle};

pub use api::Answer;
pub use edge::Reply;

/// The stand-ins, on the loopback, and the directory of a test.
pub struct Standin {
    runtime: Option<tokio::runtime::Runtime>,
    pub api: Arc<api::Api>,
    pub api_url: String,
    pub edge: Arc<edge::Edge>,
    pub edge_addr: SocketAddr,
    pub front_addr: SocketAddr,
    pub dir: tempfile::TempDir,
    pub ca_file: PathBuf,
}

impl Standin {
    pub fn start() -> Standin {
        Standin::start_at(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind the edge")
    }

    /// The stand-ins as [`Standin::start`] gives them, with the edge on
    /// `edge`. A bind error of the edge goes to the caller, which can take
    /// a different port.
    pub fn start_at(edge: SocketAddr) -> std::io::Result<Standin> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a runtime for the stand-in");
        let mut standin = runtime.block_on(Standin::start_here_at(edge))?;
        standin.runtime = Some(runtime);
        Ok(standin)
    }

    /// The stand-ins on the runtime of the caller, for a test with a
    /// paused clock. Use [`Standin::start_tunnel`] in place of
    /// [`Standin::run`], [`Standin::block_on`] and [`Standin::proxy`]: they
    /// need the runtime of [`Standin::start`].
    pub async fn start_here() -> Standin {
        Standin::start_here_at(SocketAddr::from(([127, 0, 0, 1], 0))).await.expect("bind the edge")
    }

    async fn start_here_at(edge_at: SocketAddr) -> std::io::Result<Standin> {
        let dir = tempfile::tempdir().expect("a directory");
        let ca = ca::Ca::new();
        let ca_file = dir.path().join("standin-ca.pem");
        std::fs::write(&ca_file, &ca.pem).unwrap();
        let api = api::Api::new();
        let edge = edge::Edge::new(api.clone());
        let api_addr = api::serve(api.clone()).await;
        let (listener, edge_addr) = edge::listen_at(edge_at).await?;
        let names = [crate::cloudflare::h2_sni(), crate::cloudflare::quic_sni()];
        edge::serve(edge.clone(), listener, ca.server_config(&names));
        let front_addr = front::serve(edge.clone()).await;
        Ok(Standin {
            runtime: None,
            api,
            api_url: format!("http://{api_addr}"),
            edge,
            edge_addr,
            front_addr,
            dir,
            ca_file,
        })
    }

    /// Run a future of the stand-in to its end.
    pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        self.runtime.as_ref().unwrap().block_on(future)
    }

    /// A CONNECT proxy on the loopback.
    pub fn proxy(&self) -> (SocketAddr, proxy::Log) {
        self.block_on(proxy::serve())
    }

    /// The host of a tunnel against the stand-ins: TLS on ring that trusts
    /// the test CA when `with_ca` (the CA file alone, as `SSL_CERT_FILE`
    /// does), the dial `dial` (a direct dial when `None`), the edge of the
    /// stand-in in place of the discovery, and no UDP. The events of the
    /// tunnel go to the list that it gives.
    pub fn host(&self, with_ca: bool, dial: Option<Arc<dyn pod_core::Dial>>) -> (Host, Events) {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut settings = pod_tls::Settings::new(pod_tls::Words::new("pod-cloudflare"));
        if with_ca {
            settings.ca_file = Some(pod_tls::CaFile::new("the test", &self.ca_file));
        }
        let tls = Arc::new(pod_tls::Context::new(provider, settings));
        let dial = dial.unwrap_or_else(|| proxied(pod_proxy::ProxyChoice::Direct));
        let mut host = Host::new(tls, dial);
        host.settings.edge = Some(vec![self.edge_addr]);
        host.settings.udp = crate::udp::Choice::Off;
        let events: Events = Arc::default();
        let kept = events.clone();
        let sink = move |event: pod_core::Event| kept.lock().unwrap().push(event);
        host.report = pod_core::Report { events: Arc::new(sink), observer: Arc::new(pod_core::NoObserver) };
        (host, events)
    }

    /// The configuration of a quick tunnel of the stand-in API to `service`:
    /// the HTTP/2 leg, direct, with no verify (the public name is not in
    /// the DNS).
    pub fn config(&self, service: Service) -> Config {
        Config {
            service_url: self.api_url.clone(),
            protocol: Protocol::Http2,
            transport: Transport::Direct,
            ingress: Ingress::default().catch_all(service),
            verify: false,
            ..Config::default()
        }
    }

    /// Start a tunnel of `host` with `config`, on the runtime of the
    /// stand-in.
    pub fn run(&self, host: Host, config: Config) -> Result<TunnelHandle, pod_core::Failure> {
        self.run_with(host, config, |builder| builder)
    }

    /// Start a tunnel as [`Standin::run`] does, with more settings of the
    /// builder from `more`.
    pub fn run_with(
        &self,
        host: Host,
        config: Config,
        more: impl FnOnce(TunnelBuilder) -> TunnelBuilder,
    ) -> Result<TunnelHandle, pod_core::Failure> {
        let builder = more(TunnelBuilder::quick(Arc::new(host)).config(config));
        self.block_on(async move { builder.build()?.start().await })
    }

    /// Start a tunnel of `host` with `config` on the runtime of the caller
    /// (see [`Standin::start_here`]).
    pub async fn start_tunnel(&self, host: Host, config: Config) -> Result<TunnelHandle, pod_core::Failure> {
        TunnelBuilder::quick(Arc::new(host)).config(config).build()?.start().await
    }

    /// A request from a visitor, through the front. `host` is the `Host`.
    /// Gives the status, the headers and the body.
    pub fn visit(&self, host: &str, path: &str, headers: &[(&str, &str)]) -> (u16, Vec<(String, String)>, String) {
        let mut stream = std::net::TcpStream::connect(self.front_addr).expect("the front");
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        let mut request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let text = String::from_utf8_lossy(&raw).to_string();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let mut lines = head.lines();
        let status = lines.next().and_then(|l| l.split_whitespace().nth(1)).and_then(|s| s.parse().ok()).unwrap_or(0);
        let headers = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
            .collect();
        (status, headers, body.to_string())
    }
}

impl Drop for Standin {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// The events of a tunnel, in order.
pub type Events = Arc<std::sync::Mutex<Vec<pod_core::Event>>>;

/// True when an event of `events` holds `text`.
pub fn said(events: &Events, text: &str) -> bool {
    events.lock().unwrap().iter().any(|e| e.text.contains(text))
}

/// A dial with the proxy choice `choice` and the system resolver.
pub fn proxied(choice: pod_proxy::ProxyChoice) -> Arc<dyn pod_core::Dial> {
    let settings = pod_proxy::Settings::new("pod-cloudflare", choice);
    Arc::new(pod_proxy::Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new())))
}

/// An origin on the loopback: it answers each request with `body`, and it
/// keeps the head of each request. Gives the port.
pub fn origin(body: &'static str) -> (u16, Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let kept = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let kept = kept.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    match stream.read(&mut byte) {
                        Ok(1) => head.push(byte[0]),
                        _ => return,
                    }
                }
                kept.lock().unwrap().push(String::from_utf8_lossy(&head).to_string());
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nX-Origin: standin\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    (port, seen)
}

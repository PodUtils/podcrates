//! The way out of the tests: a TLS stand-in for one name on the loopback,
//! a client that trusts its test CA alone, the pins of the names of the
//! tests, and a service of the test's own text.

use std::io;
use std::net::IpAddr;
use std::sync::{Arc, OnceLock};

use pod_proxy::resolve::System;
use pod_proxy::{Dialer, ProxyChoice, Settings};
use pod_services::{Catalog, Date};
use pod_services_watch::{Net, Report};
use pod_testkit::stand::{self, Answer, Io, Request, Stand, Wrap};
use pod_tls::{Context, TlsRoots, Trust, Words};
use rustls::{ServerConnection, StreamOwned};

// The certificates of the tests of pod-tls, minted in pure Rust.
#[allow(dead_code, unused_imports)]
mod cert {
    include!("../../../pod-tls/tests/common/cert_for_test.rs");
}

/// The name of each TLS stand-in; the pins send it to the loopback.
pub const NAME: &str = "stand.test";
/// The day of the tests.
pub fn today() -> Date {
    Date::new(2026, 11, 10).unwrap()
}

/// The server configuration of [`NAME`] and the roots that trust it, made
/// once for each test binary.
fn tls() -> &'static (Arc<rustls::ServerConfig>, rustls::RootCertStore) {
    static TLS: OnceLock<(Arc<rustls::ServerConfig>, rustls::RootCertStore)> = OnceLock::new();
    TLS.get_or_init(|| {
        let (acceptor, roots) = cert::server_for(NAME).expect("a test server");
        (acceptor.config().clone(), roots)
    })
}

/// The turn of each accepted connection into TLS for [`NAME`].
pub fn wrap() -> Wrap {
    let config = tls().0.clone();
    Arc::new(move |tcp| {
        let connection = ServerConnection::new(config.clone()).map_err(io::Error::other)?;
        Ok(Box::new(StreamOwned::new(connection, tcp)) as Box<dyn Io>)
    })
}

/// An HTTPS stand-in for [`NAME`].
pub fn https(handler: impl Fn(&Request) -> Answer + Send + Sync + 'static) -> Stand {
    Stand::http_over(handler, wrap()).expect("a stand-in")
}

/// A TLS stand-in for [`NAME`] that gives each connection to `serve`.
pub fn tls_raw(serve: impl Fn(&mut dyn Io) + Send + Sync + 'static) -> Stand {
    Stand::raw_over(serve, wrap()).expect("a stand-in")
}

/// A plain TCP stand-in.
pub fn raw(serve: impl Fn(&mut dyn Io) + Send + Sync + 'static) -> Stand {
    stand::raw(serve).expect("a stand-in")
}

/// The way out of a test: a direct dial whose resolver pins the names of
/// the tests to the loopback, and the trust of the test CA alone.
pub fn net() -> Net {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("pod-services-watch"))));
    let roots = tls().1.clone();
    let config = context
        .client_config(&TlsRoots { count: roots.len(), roots, source: "the test's own CA".into() })
        .expect("a configuration");
    let trust = Trust::caller(config).expect("no ALPN");
    let loopback = IpAddr::from([127, 0, 0, 1]);
    let pins = ["stand.test", "other.test"].iter().map(|n| (n.to_string(), loopback)).collect();
    let dialer =
        Dialer::new(Settings::new("pod-services-watch", ProxyChoice::Direct), Arc::new(System::with_pins(pins)));
    let mut net = Net::new(context, Arc::new(dialer), trust, today());
    net.limit = std::time::Duration::from_secs(10);
    net
}

/// A service file of the test: the head of a file with `id`, then `body`
/// (its endpoints, facts and watches).
pub fn file(id: &str, body: &str) -> String {
    format!("id = \"{id}\"\nkind = \"test\"\nparty = \"a test\"\nabout = \"a stand-in\"\n\n{body}")
}

/// The source of each value of a test file.
pub const SOURCE: &str = "source = { measured = \"a test\", read = 2026-11-10 }";

/// An endpoint of a test file.
pub fn endpoint(name: &str, scheme: &str, host: &str, port: u16, more: &str) -> String {
    format!(
        "[[endpoint]]\nname = \"{name}\"\nscheme = \"{scheme}\"\nhost = \"{host}\"\nport = {port}\n{more}\n{SOURCE}\n\n"
    )
}

/// A fact of a test file; `value` is TOML.
pub fn fact(name: &str, value: &str) -> String {
    format!("[[fact]]\nname = \"{name}\"\nvalue = {value}\n{SOURCE}\n\n")
}

/// The reports of each watch of the service of `text`, laid over the
/// built-in set of `net`.
pub async fn run(net: &mut Net, text: &str) -> Vec<Report> {
    let mut services = Catalog::new();
    services.lay("test.toml", text).expect("a valid test file");
    net.services = services.clone();
    let service = services.overlay().last().expect("the service").clone();
    let mut out = Vec::new();
    for watch in service.watches() {
        out.push(pod_services_watch::run(&service, watch, net).await);
    }
    out
}

/// The one report of the one watch of the service of `text`.
pub async fn one(text: &str) -> Report {
    one_in(&mut net(), text).await
}

/// [`one`] through `net`.
pub async fn one_in(net: &mut Net, text: &str) -> Report {
    let mut reports = run(net, text).await;
    assert_eq!(reports.len(), 1, "one watch");
    let report = reports.remove(0);
    eprintln!("{}", report.lines().join("\n"));
    report
}

/// The lines of a report, joined.
pub fn text(report: &Report) -> String {
    report.lines().join("\n")
}

//! The TLS, the dial and the host of the tests: ring, the default roots, a
//! direct dial with the system resolver.

use std::sync::Arc;

pub fn tls() -> Arc<pod_tls::Context> {
    let settings = pod_tls::Settings::new(pod_tls::Words::new("pod-cloudflare"));
    Arc::new(pod_tls::Context::new(Arc::new(rustls::crypto::ring::default_provider()), settings))
}

/// A direct dial with the system resolver.
pub fn dial() -> Arc<dyn pod_core::Dial> {
    let settings = pod_proxy::Settings::new("pod-cloudflare", pod_proxy::ProxyChoice::Direct);
    Arc::new(pod_proxy::Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new())))
}

/// A dial through the proxy `url`.
pub fn proxied(url: &str) -> Arc<dyn pod_core::Dial> {
    let proxy = pod_proxy::Proxy::parse(url).expect("a proxy URL");
    let settings = pod_proxy::Settings::new("pod-cloudflare", pod_proxy::ProxyChoice::Via(proxy));
    Arc::new(pod_proxy::Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new())))
}

/// The words of podnet, the first consumer: the tests check that its texts
/// come through.
pub fn podnet_words() -> crate::Words {
    crate::Words {
        name: "podnet".into(),
        protocol: "--protocol".into(),
        udp_off: "PODNET_UDP=off".into(),
        strict: "Set PODNET_VERIFY_STRICT=1 to make this an error".into(),
        resolve: "give the address with --resolve HOST=IP, or use --no-verify".into(),
        tcp_origin: "Use --forward with \"podnet connect\", or --port for an HTTP origin.".into(),
    }
}

/// The host of the tests: [`tls`], [`dial`], no services, the words of
/// podnet.
pub fn host() -> crate::Host {
    let mut host = crate::Host::new(tls(), dial());
    host.settings.words = podnet_words();
    host
}

/// A relay of the tests over `dial`, with a context of the relay that has
/// no cache.
pub fn relay(dial: Arc<dyn pod_core::Dial>) -> crate::host::EdgeRelay {
    let way = pod_ws::Way::new(tls(), self::dial(), "pod-cloudflare/test");
    let context = pod_relay::Context::new(way, pod_relay::cache::Cache::none());
    crate::host::EdgeRelay { dial, context: Arc::new(context) }
}

/// A dial that counts its calls and refuses each one.
#[derive(Default)]
pub struct Refusing {
    pub calls: std::sync::atomic::AtomicUsize,
}

impl pod_core::Dial for Refusing {
    fn label(&self) -> String {
        "refusing".into()
    }

    fn dial<'a>(
        &'a self,
        _target: &'a pod_core::Target,
        _deadline: tokio::time::Instant,
    ) -> pod_core::BoxFuture<'a, Result<pod_core::Connected, pod_core::Failure>> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async {
            Err(pod_core::Failure::new(
                pod_core::Stage::Relay,
                pod_core::Class::Refused,
                "the relay of the test refuses",
            ))
        })
    }
}

/// A report whose events the test reads back.
pub fn recorded() -> (pod_core::Report, Arc<std::sync::Mutex<Vec<pod_core::Event>>>) {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let kept = seen.clone();
    let events = move |event: pod_core::Event| kept.lock().unwrap().push(event);
    (pod_core::Report { events: Arc::new(events), observer: Arc::new(pod_core::NoObserver) }, seen)
}

/// Each text of the chain joined with `: `, as a consumer that walks the
/// sources writes it (anyhow's `{:#}`, PodUtils/podnet#707).
pub fn each_text(err: &(dyn std::error::Error + 'static)) -> String {
    pod_core::failure::joined(err)
}

/// A stand-in DoH resolver that accepts each TCP connection and never
/// answers, as a proxy that connects each resolver and gives no answer; and
/// a host whose `resolvers` resolvers are each this stand-in, with the
/// report of [`recorded`] and no system resolver. The returned counter
/// counts the connections.
pub async fn silent_doh_host(
    resolvers: usize,
) -> (crate::Host, Arc<std::sync::Mutex<Vec<pod_core::Event>>>, Arc<std::sync::atomic::AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = accepted.clone();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            held.push(stream);
        }
    });
    let ip = std::net::IpAddr::from([127, 0, 0, 1]);
    let one = pod_dns::Doh::new(ip, "doh.test").port(port).json("/dns-query");
    let settings = pod_dns::Settings { resolvers: vec![one; resolvers], memory: false, ..Default::default() };
    let mut host = host();
    host.resolver = Arc::new(pod_dns::Resolver::new(settings).with_doh(tls(), dial()));
    let (report, seen) = recorded();
    host.report = report;
    (host, seen, accepted)
}

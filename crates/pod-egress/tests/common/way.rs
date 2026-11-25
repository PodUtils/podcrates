//! The way out of the tests: TLS on ring, as each test of podcrates runs
//! it, a dialer through a proxy or none, and each name of the tests pinned
//! to its address. The words and the user agent are podssh's, as its binary
//! gives them.

#![allow(dead_code)]

use std::net::IpAddr;
use std::sync::Arc;

use pod_proxy::{Dialer, ProxyChoice, Settings};
use pod_tls::{Context, Words};
use pod_ws::Way;

/// The user agent of the tests.
pub const USER_AGENT: &str = "podssh/0.0.0-test";

/// A way through `proxy`, with each name of `pins` at its address.
pub fn through(proxy: ProxyChoice, pins: &[(&str, IpAddr)]) -> Way {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("podssh"))));
    let pins = pins.iter().map(|(name, ip)| (name.to_string(), *ip)).collect();
    let resolve = Arc::new(pod_proxy::resolve::System::with_pins(pins));
    Way::new(tls, Arc::new(Dialer::new(Settings::new("podssh", proxy), resolve)), USER_AGENT)
}

/// A direct way.
pub fn direct() -> Way {
    through(ProxyChoice::Direct, &[])
}

/// A direct way, with each name of `names` at the loopback.
pub fn loopback(names: &[&str]) -> Way {
    let lo: IpAddr = "127.0.0.1".parse().unwrap();
    let pins: Vec<(&str, IpAddr)> = names.iter().map(|n| (*n, lo)).collect();
    through(ProxyChoice::Direct, &pins)
}

/// What a way of [`aimed`] was asked to dial, each as `HOST:PORT`.
pub type Asked = Arc<std::sync::Mutex<Vec<String>>>;

/// A dialer that takes each target to one port of the loopback, as a
/// road whose far end has a fixed port (443) reaches a stand-in.
struct Aimed {
    port: u16,
    asked: Asked,
}

impl pod_core::Dial for Aimed {
    fn label(&self) -> String {
        format!("aimed at 127.0.0.1:{}", self.port)
    }

    fn dial<'a>(
        &'a self,
        target: &'a pod_core::Target,
        _deadline: tokio::time::Instant,
    ) -> pod_core::BoxFuture<'a, Result<pod_core::Connected, pod_core::Failure>> {
        Box::pin(async move {
            self.asked.lock().unwrap().push(target.to_string());
            let tcp = tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                .await
                .map_err(|e| pod_core::Failure::io(pod_core::Stage::Connect, "the connection", e))?;
            Ok(pod_core::Connected::new(Box::new(tcp)))
        })
    }
}

/// A way whose dialer takes each target to `port` of the loopback, and what
/// it was asked to dial.
pub fn aimed(port: u16) -> (Way, Asked) {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("podssh"))));
    let asked = Asked::default();
    let dial = Aimed { port, asked: asked.clone() };
    (Way::new(tls, Arc::new(dial), USER_AGENT), asked)
}

//! The proxy that an environment names, as the caller reads it with
//! `pod_proxy::Environment` (PodUtils/podssh@T-162): with `ALL_PROXY` alone,
//! the iroh road reaches its relay through the proxy. An endpoint with what
//! iroh's own selection takes of that environment, the planted defect (iroh
//! reads `HTTP_PROXY` and `HTTPS_PROXY` alone, so no proxy), finds no relay.
//! And a SOCKS5 proxy in `ALL_PROXY` (PodUtils/podssh@T-280), the only way
//! to the relay, which gets the relay's name. The environment is a reader
//! of the test's own: no variable of the process is set.

mod common;

use std::ffi::OsString;
use std::sync::atomic::Ordering;
use std::time::Duration;

use iroh::tls::CaTlsConfig;
use pod_iroh::test_relay;
use pod_iroh::{Options, Udp};
use pod_proxy::{Environment, ProxyChoice};
use pod_tls::Trust;

use common::{RELAY_ADDR, RELAY_NAME, limit, proxy, relay};

/// An environment where only `ALL_PROXY`, set to `url`, names a proxy.
fn all_proxy_alone(url: &str) -> ProxyChoice {
    let url = url.to_string();
    ProxyChoice::FromEnvironment(Environment::read(&|name| (name == "ALL_PROXY").then(|| OsString::from(&url))))
}

/// An endpoint's options with the proxy of `environment`: the relay, with
/// no UDP; the relay's certificate check skipped, as it is self-signed.
fn from_environment(relay: &iroh::RelayUrl, environment: ProxyChoice) -> Options {
    Options {
        relays: vec![relay.clone()],
        tls: test_relay::tls(),
        resolve: test_relay::resolver(),
        proxy: environment,
        trust: Trust::Default,
        udp: Udp::Off,
        secret: None,
        accepts: true,
        relay_tls: Some(CaTlsConfig::insecure_skip_verify()),
        qad_port: None,
        quic: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_proxy_reaches_the_relay_where_iroh_s_own_selection_does_not() {
    let (relay_url, _relay) = relay().await;
    let (proxy, tunnels) = proxy(relay_url.port().unwrap()).await;
    let environment = all_proxy_alone(&format!("http://{proxy}"));

    let endpoint = pod_iroh::bind(&from_environment(&relay_url, environment)).await.expect("an endpoint");
    tokio::time::timeout(limit(), endpoint.online()).await.expect("the caller's selection reached the relay");
    assert!(tunnels.load(Ordering::SeqCst) > 0, "through the proxy");
    endpoint.close().await;

    // The planted defect: iroh's own selection, which reads HTTP_PROXY and
    // HTTPS_PROXY only, for its relay dial: of this environment, no proxy.
    let planted = pod_iroh::bind(&from_environment(&relay_url, ProxyChoice::Direct)).await.expect("an endpoint");
    let online = tokio::time::timeout(Duration::from_secs(8), planted.online()).await;
    assert!(online.is_err(), "iroh's own selection reached the relay");
    planted.close().await;
}

/// A SOCKS5 proxy in `ALL_PROXY` is the only way to the relay, which it reaches
/// by name (PodUtils/podssh@T-280): the relay dial of the patched iroh-relay
/// and the HTTPS probe that picks the home relay each pass it, with the user
/// and the password of RFC 1929, and the relay's name with the address type 3.
/// With the refusal of SOCKS5 restored, the planted defect, no endpoint is
/// made.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_socks5_proxy_in_all_proxy_reaches_the_relay_by_name() {
    let (relay_url, _relay) = relay().await;
    let port = relay_url.port().unwrap();
    let (proxy, seen) = common::socks5::proxy(port, Some(("alice", "s3cret"))).await;
    let environment = all_proxy_alone(&format!("socks5h://alice:s3cret@{proxy}"));

    let options = from_environment(&relay_url, environment);
    let endpoint = pod_iroh::bind(&options).await.expect("an endpoint through SOCKS5");
    tokio::time::timeout(limit(), endpoint.online()).await.expect("the relay through the SOCKS5 proxy");
    endpoint.close().await;
    let seen = seen.lock().unwrap().clone();
    // The stand-in is the only way to the relay, so the relay dial and the
    // probe each came through it; each tunnel by name.
    assert!(!seen.is_empty(), "no tunnel through the stand-in");
    for each in &seen {
        assert_eq!((each.address_type, each.host.as_str(), each.port), (3, RELAY_NAME, port), "{seen:?}");
    }
}

/// A `socks5://` proxy gets the relay's address, which the client resolves
/// with the caller's resolver, as curl and pod-proxy take the scheme; only
/// `socks5h://` sends the name (T-808). The planted defect, the name for
/// both schemes, shows a tunnel by name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_socks5_proxy_without_h_gets_the_relay_s_address() {
    let (relay_url, _relay) = relay().await;
    let port = relay_url.port().unwrap();
    let (proxy, seen) = common::socks5::proxy(port, None).await;
    let environment = all_proxy_alone(&format!("socks5://{proxy}"));

    let mut options = from_environment(&relay_url, environment);
    let pinned: std::net::IpAddr = RELAY_ADDR.parse().unwrap();
    options.resolve =
        std::sync::Arc::new(pod_proxy::resolve::System::with_pins(vec![(RELAY_NAME.to_string(), pinned)]));
    let endpoint = pod_iroh::bind(&options).await.expect("an endpoint through SOCKS5");
    tokio::time::timeout(limit(), endpoint.online()).await.expect("the relay through the SOCKS5 proxy");
    endpoint.close().await;
    let seen = seen.lock().unwrap().clone();
    assert!(!seen.is_empty(), "no tunnel through the stand-in");
    for each in &seen {
        assert_eq!((each.address_type, each.host.as_str(), each.port), (1, RELAY_ADDR, port), "{seen:?}");
    }
}

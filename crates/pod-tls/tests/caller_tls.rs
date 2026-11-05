//! A `rustls::ClientConfig` that the caller supplies: the crate uses it as
//! it is, for the connection, and refuses one that offers ALPN. The caller's
//! configuration here is made with each provider of the tests; its only
//! root is a test CA that the default store does not hold.

use std::sync::Arc;
use std::time::Duration;

use pod_tls::groups::Kept;
use pod_tls::{Context, TlsRoots, Trust};
use rustls::crypto::CryptoProvider;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[allow(dead_code)]
mod cert {
    include!("common/cert_for_test.rs");
}

#[allow(dead_code)]
mod tls {
    include!("common/tls_for_test.rs");
}

const NAME: &str = "relay.test";
const TIMEOUT: Duration = Duration::from_secs(10);

/// A TLS server for `NAME` on loopback, which writes `hello` and closes; the
/// trust store of its test CA.
async fn server() -> (std::net::SocketAddr, rustls::RootCertStore, tokio::task::JoinHandle<()>) {
    let (acceptor, roots) = cert::server_for(NAME).expect("a test server");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("an address");
    let task = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await
            && let Ok(mut tls) = acceptor.accept(stream).await
        {
            let _ = tls.write_all(b"hello").await;
            let _ = tls.shutdown().await;
        }
    });
    (addr, roots, task)
}

fn callers_config(roots: rustls::RootCertStore, provider: Arc<CryptoProvider>) -> Arc<rustls::ClientConfig> {
    let count = roots.len();
    let roots = TlsRoots { roots, source: "the caller's test CA".into(), count };
    pod_tls::client_config(&roots, provider, Arc::new(Kept::new())).expect("a config")
}

async fn open(context: &Context, addr: std::net::SocketAddr, trust: &Trust) -> Result<String, String> {
    let tcp = tokio::net::TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
    let mut stream = context.tls_over(tcp, NAME, trust, TIMEOUT).await.map_err(|e| e.to_string())?;
    let mut got = String::new();
    stream.read_to_string(&mut got).await.map_err(|e| e.to_string())?;
    Ok(got)
}

#[tokio::test]
async fn a_callers_configuration_reaches_a_server_that_the_default_store_does_not_know() {
    pod_testkit::need_loopback!();
    for (each, provider) in tls::providers() {
        let context = tls::context(provider.clone());
        let (addr, roots, task) = server().await;
        let trust = Trust::caller(callers_config(roots, provider)).expect("no ALPN");
        let got = open(&context, addr, &trust).await;
        assert_eq!(got.expect("the caller's CA verifies the server"), "hello", "{each}");
        task.await.unwrap();

        // The control: the crate's own store does not hold the test CA.
        let (addr, _, task) = server().await;
        let error =
            open(&context, addr, &Trust::Default).await.expect_err("the default store does not know the test CA");
        assert!(error.contains("UnknownIssuer") || error.contains("issuer"), "{each}: {error}");
        task.await.unwrap();
    }
}

#[test]
fn a_callers_configuration_that_offers_alpn_is_refused() {
    let (_, roots) = cert::server_for(NAME).expect("a test CA");
    let mut config = (*callers_config(roots, tls::providers()[0].1.clone())).clone();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    match Trust::caller(Arc::new(config)) {
        Err(pod_tls::Error::Config(why)) => {
            assert!(why.contains("ALPN (h2, http/1.1)") && why.contains("HTTP/1.1"), "{why}")
        }
        other => panic!("a configuration with ALPN was accepted: {other:?}"),
    }
}

#[test]
fn a_callers_configuration_is_used_as_it_is() {
    let (_, roots) = cert::server_for(NAME).expect("a test CA");
    let provider = tls::providers()[0].1.clone();
    let context = tls::context(provider.clone());
    let config = callers_config(roots, provider);
    let trust = Trust::caller(config.clone()).expect("no ALPN");
    assert!(Arc::ptr_eq(&context.config_for(&trust).unwrap(), &config), "the same configuration");
    assert!(context.roots_for(&trust).is_err(), "the caller's roots cannot be listed");
    assert_eq!(trust.clone(), trust, "a configuration equals itself");
}

//! DNS over HTTPS through a proxy that takes names alone, as a sandbox's
//! proxy refuses CONNECT to an address literal (cfrs's GitHub #9 and #21)
//! and podssh's `scripts/box/proxy.py --take names` does: each resolver is
//! asked by its name first, then by its address, and a name once. The
//! resolver is h2's own server, which this crate did not write, with the
//! answer that Quad9 sent for the edge's SRV record (`tests/fixtures/doh/`).

use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_dns::{Doh, Settings, Srv, Way};
use pod_proxy::{Dialer, Proxy, ProxyChoice};
use pod_tls::{Context, TlsRoots, Trust, Words};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Instant;

// The certificates of the tests of pod-tls, minted in pure Rust.
#[allow(dead_code)]
mod cert {
    include!("../../pod-tls/tests/common/cert_for_test.rs");
}

/// The stand-in resolver's name, which its certificate names.
const NAME: &str = "doh.test";
const SRV_NAME: &str = "_v2-origintunneld._tcp.argotunnel.com";
const LIMIT: Duration = Duration::from_secs(10);

/// The stand-in, as the default resolvers list Quad9's: the wire form
/// alone, over HTTP/2; its address is one that no proxy here takes.
fn stand_in() -> Doh {
    Doh::new([192, 0, 2, 53].into(), NAME).wire("/dns-query").http2(true)
}

/// The TLS context of the tests, on ring.
fn context() -> Context {
    Context::new(Arc::new(rustls::crypto::ring::default_provider()), pod_tls::Settings::new(Words::new("podssh")))
}

/// A dial through `proxy` alone.
fn via(proxy: Proxy) -> Dialer {
    let settings = pod_proxy::Settings::new("podssh", ProxyChoice::Via(proxy));
    Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new()))
}

fn quad9_srv() -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/doh/quad9-srv.bin")).unwrap()
}

/// A trust of the test's root alone.
fn trust(roots: rustls::RootCertStore) -> Trust {
    let roots = TlsRoots { count: roots.len(), roots, source: "the test's own root".into() };
    Trust::caller(context().client_config(&roots).unwrap()).unwrap()
}

/// h2's server for one connection, for `NAME`, offering `h2`: Quad9's
/// answer to its GET, and the path that it was asked.
async fn resolver() -> (u16, rustls::RootCertStore, tokio::task::JoinHandle<Option<String>>) {
    let (acceptor, roots) = cert::server_for(NAME).unwrap();
    let mut config = (**acceptor.config()).clone();
    config.alpn_protocols = vec![b"h2".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = quad9_srv();
    let served = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.ok()?;
        let tls = acceptor.accept(tcp).await.ok()?;
        let mut connection = h2::server::handshake(tls).await.ok()?;
        let (request, mut respond) = connection.accept().await?.ok()?;
        let path = request.uri().path_and_query().map(|p| p.to_string()).unwrap_or_default();
        let response =
            http::Response::builder().status(200).header("content-type", "application/dns-message").body(()).ok()?;
        let mut send = respond.send_response(response, false).ok()?;
        send.send_data(body.into(), true).ok()?;
        // The connection is driven until the client goes.
        let _ = tokio::time::timeout(LIMIT, async { while connection.accept().await.is_some() {} }).await;
        Some(path)
    });
    (port, roots, served)
}

/// A proxy that takes names alone, as `scripts/box/proxy.py --take names`:
/// 403 to an address literal, a tunnel to the stand-in resolver at
/// `resolver_port` for `NAME`, 502 for any other name. It keeps each target
/// that it was asked, in order.
async fn names_alone(resolver_port: u16) -> (Proxy, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let kept = asked.clone();
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            tokio::spawn(tunnel(client, resolver_port, kept.clone()));
        }
    });
    (Proxy::parse(&format!("http://127.0.0.1:{port}")).unwrap(), asked)
}

async fn tunnel(mut client: TcpStream, resolver_port: u16, asked: Arc<Mutex<Vec<String>>>) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match client.read(&mut byte).await {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let target = head.split(' ').nth(1).unwrap_or_default().to_string();
    asked.lock().unwrap().push(target.clone());
    let host = target.rsplit_once(':').map_or("", |(host, _)| host).trim_matches(['[', ']']);
    let refusal = if host.parse::<IpAddr>().is_ok() {
        Some("403 not on the allowlist")
    } else if target != format!("{NAME}:443") {
        Some("502 Bad Gateway")
    } else {
        None
    };
    if let Some(status) = refusal {
        let _ = client.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n").as_bytes()).await;
        return;
    }
    let Ok(mut resolver) = TcpStream::connect(("127.0.0.1", resolver_port)).await else { return };
    if client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await.is_ok() {
        let _ = tokio::io::copy_bidirectional(&mut client, &mut resolver).await;
    }
}

/// The proxy refuses the resolver's address and carries its name: the
/// records come by the name, which was asked first, and the address is not
/// asked at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_proxy_of_names_alone_carries_the_question_by_the_resolver_s_name() {
    pod_testkit::need_loopback!();
    let (port, roots, served) = resolver().await;
    let (proxy, asked) = names_alone(port).await;
    let (settings, context, dial) = (Settings { trust: trust(roots), ..Settings::default() }, context(), via(proxy));
    let way = Way { settings: &settings, context: &context, dial: &dial };
    let resolvers = [stand_in()];
    let lookup = way.srv_with(&resolvers, SRV_NAME, Instant::now() + LIMIT);
    let records = tokio::time::timeout(LIMIT * 2, lookup).await.expect("in time").expect("the records, by the name");
    let region = |n: u16| Srv { priority: n, weight: 1, port: 7844, target: format!("region{n}.v2.argotunnel.com") };
    assert_eq!(records, [region(2), region(1)], "Quad9's answer, in its order");
    assert_eq!(*asked.lock().unwrap(), [format!("{NAME}:443")], "the name, and nothing after it");
    let path = tokio::time::timeout(LIMIT * 2, served).await.unwrap().unwrap().expect("the resolver was asked");
    assert!(path.starts_with("/dns-query?dns="), "{path}");
}

/// Where the proxy refuses the name too, the address is asked next; a name
/// that two addresses share is asked once; the failure names each that was
/// asked, the name's refusal and the address's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_address_follows_a_name_refused_and_a_name_is_asked_once() {
    pod_testkit::need_loopback!();
    // No resolver behind it: the name below is not the stand-in's.
    let (proxy, asked) = names_alone(0).await;
    let other = |ip: [u8; 4]| Doh::new(ip.into(), "other.test").json("/dns-query").wire("/dns-query");
    let resolvers = [other([192, 0, 2, 53]), other([192, 0, 2, 54])];
    let (settings, context, dial) = (Settings::default(), context(), via(proxy));
    let way = Way { settings: &settings, context: &context, dial: &dial };
    let lookup = way.srv_with(&resolvers, SRV_NAME, Instant::now() + LIMIT);
    let failed = tokio::time::timeout(LIMIT * 2, lookup).await.expect("in time").expect_err("no way to any");
    assert_eq!(*asked.lock().unwrap(), ["other.test:443", "192.0.2.53:443", "192.0.2.54:443"]);
    for host in ["other.test", "192.0.2.53", "192.0.2.54"] {
        assert!(failed.contains(&format!("{host}: ")), "{host}: {failed}");
    }
    assert!(failed.contains("502 Bad Gateway") && failed.contains("403 not on the allowlist"), "{failed}");
}

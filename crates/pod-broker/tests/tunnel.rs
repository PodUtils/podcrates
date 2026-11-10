//! The tunnel: a CONNECT round trip in memory, on the Unix socket and on
//! the TCP listener of the broker; the upstream chain through a stand-in
//! CONNECT proxy; and the seam of the TLS end of T-100.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use pod_broker::{Broker, Settings, Tunnel, TunnelEnd};
use pod_core::{BoxFuture, BoxedIo, Failure};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn a_connect_carries_bytes_both_ways_and_counts_them() {
    let far = Far::new(Kind::Echo);
    let (broker, audit) = broker(far, Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    // Bytes after the head belong to the tunnel.
    let request = format!("{}early", connect("echo.example:443"));
    let (head, mut tunnel) = ask(&broker, &request).await;
    assert_eq!(head, "HTTP/1.1 200 Connection Established\r\n\r\n");
    let mut early = [0u8; 5];
    tunnel.read_exact(&mut early).await.unwrap();
    assert_eq!(&early, b"early");
    assert_eq!(round_trip(&mut tunnel, &[7u8; 100_000]).await, vec![7u8; 100_000]);
    tunnel.shutdown().await.unwrap();
    assert_eq!(read_all(&mut tunnel).await, "");
    let line = audit.connection("allow").await;
    assert_eq!(line.fields["job"], "job1");
    assert_eq!((line.fields["host"].as_str(), line.fields["port"].as_u64()), (Some("echo.example"), Some(443)));
    assert_eq!((line.fields["up"].as_u64(), line.fields["down"].as_u64()), (Some(100_005), Some(100_005)));
    assert_eq!((line.fields["rule"].as_str(), line.fields["status"].as_u64()), (Some("open"), Some(200)));
    assert_eq!(line.fields["addr"], format!("{PUBLIC}:443"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_connect_round_trip_on_the_unix_socket_of_the_broker() {
    let dir = socket_dir("pod-broker-unix");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("broker.sock");
    let listener = pod_broker::control::bind_unix(&path, 0o600).unwrap();
    let (broker, _) = broker(Far::new(Kind::Echo), Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    let serving = tokio::spawn(broker.clone().serve_unix(listener));
    let mut stream = tokio::net::UnixStream::connect(&path).await.unwrap();
    stream.write_all(connect("echo.example:443").as_bytes()).await.unwrap();
    assert_eq!(status_of(&read_head(&mut stream).await), 200);
    assert_eq!(round_trip(&mut stream, b"over a unix socket").await, b"over a unix socket");
    serving.abort();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn a_connect_round_trip_on_the_tcp_listener_of_the_broker() {
    pod_testkit::need_loopback!();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    let (broker, _) = broker(Far::new(Kind::Echo), Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    let serving = tokio::spawn(broker.clone().serve_tcp(listener));
    let mut stream = tokio::net::TcpStream::connect(at).await.unwrap();
    stream.write_all(connect("echo.example:443").as_bytes()).await.unwrap();
    assert_eq!(status_of(&read_head(&mut stream).await), 200);
    assert_eq!(round_trip(&mut stream, b"over tcp").await, b"over tcp");
    serving.abort();
}

/// A stand-in CONNECT proxy on loopback: it keeps each request head,
/// answers `status`, and then echoes the tunnel.
async fn stand_in_proxy(status: u16) -> (std::net::SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let kept = heads.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let kept = kept.clone();
            tokio::spawn(async move {
                let head = read_head(&mut stream).await;
                kept.lock().unwrap().push(head);
                let answer = format!("HTTP/1.1 {status} Stand-in\r\nContent-Length: 0\r\n\r\n");
                stream.write_all(answer.as_bytes()).await.unwrap();
                if status == 200 {
                    let (mut r, mut w) = stream.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                }
            });
        }
    });
    (at, heads)
}

/// The broker dials the checked address through the upstream proxy: the
/// proxy gets the address, never the name, so that it resolves nothing.
#[tokio::test]
async fn the_upstream_chain_carries_the_checked_address() {
    pod_testkit::need_loopback!();
    let (proxy_at, heads) = stand_in_proxy(200).await;
    let proxy = pod_proxy::Proxy::parse(&format!("http://{proxy_at}")).unwrap();
    let answers = Answers::public();
    // The resolver of the way out resolves the name of the proxy only.
    let dial = pod_broker::upstream("pod-broker-test", Some(proxy), Arc::new(pod_proxy::resolve::System::new()));
    let audit = Arc::new(pod_broker::Memory::new());
    let broker = Arc::new(Broker::new(Settings::default(), answers.clone(), dial).with_audit(audit.clone()));
    load(&broker, "job1", TOKEN, OPEN);
    let (head, mut tunnel) = ask(&broker, &connect("chained.example:443")).await;
    assert_eq!(status_of(&head), 200, "{head}");
    assert_eq!(round_trip(&mut tunnel, b"through the chain").await, b"through the chain");
    let heads = heads.lock().unwrap().clone();
    assert_eq!(heads.len(), 1);
    assert!(heads[0].starts_with(&format!("CONNECT {PUBLIC}:443 HTTP/1.1\r\n")), "{}", heads[0]);
    assert!(!heads[0].contains("chained.example") && !heads[0].contains(TOKEN), "{}", heads[0]);
    assert_eq!(answers.calls(), 1);
}

/// An upstream proxy that refuses gets 502 for the job.
#[tokio::test]
async fn an_upstream_proxy_that_refuses_gets_502() {
    pod_testkit::need_loopback!();
    let (proxy_at, _) = stand_in_proxy(503).await;
    let proxy = pod_proxy::Proxy::parse(&format!("http://{proxy_at}")).unwrap();
    let answers = Answers::public();
    // The resolver of the way out resolves the name of the proxy only.
    let dial = pod_broker::upstream("pod-broker-test", Some(proxy), Arc::new(pod_proxy::resolve::System::new()));
    let broker =
        Arc::new(Broker::new(Settings::default(), answers, dial).with_audit(Arc::new(pod_broker::Memory::new())));
    load(&broker, "job1", TOKEN, OPEN);
    assert_eq!(status(&broker, &connect("chained.example:443")).await, 502);
}

/// A tunnel end that reads requests: the broker allows a CONNECT to a host
/// whose rules each have a path, and the end checks each path.
struct Reader {
    checked: Mutex<Vec<(String, Option<String>)>>,
}

impl TunnelEnd for Reader {
    fn reads_requests(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, tunnel: Tunnel<'a>, client: BoxedIo, upstream: BoxedIo) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            for path in ["/v1/repos/a", "/v1/reposx"] {
                self.checked.lock().unwrap().push((path.to_string(), tunnel.allows_path(path)));
            }
            assert_eq!(tunnel.rule, pod_broker::policy::PATH_NEEDS_TLS_END);
            pod_broker::Splice.run(tunnel, client, upstream).await
        })
    }
}

#[tokio::test]
async fn a_tunnel_end_that_reads_requests_checks_the_paths() {
    let end = Arc::new(Reader { checked: Mutex::new(Vec::new()) });
    let audit = Arc::new(pod_broker::Memory::new());
    let broker = Broker::new(Settings::default(), Answers::public(), Far::new(Kind::Echo))
        .with_audit(audit.clone())
        .with_tunnel_end(end.clone());
    let broker = Arc::new(broker);
    load(
        &broker,
        "job1",
        TOKEN,
        r#"{"version":1,"mode":"allow","allow":[{"host":"api.example.com","path":"/v1/repos"}]}"#,
    );
    let (head, mut tunnel) = ask(&broker, &connect("api.example.com:443")).await;
    assert_eq!(status_of(&head), 200, "{head}");
    assert_eq!(round_trip(&mut tunnel, b"x").await, b"x");
    let checked = end.checked.lock().unwrap().clone();
    assert_eq!(checked, [("/v1/repos/a".to_string(), Some("allow[0]".to_string())), ("/v1/reposx".to_string(), None)]);
}

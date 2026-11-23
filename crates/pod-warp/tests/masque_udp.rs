//! CONNECT-UDP (RFC 9298) over the in-process QUIC/Warp edge
//! (PodUtils/podnet#486): the `connect-udp` protocol, the URI template of
//! the target, a small payload in a QUIC datagram, a payload larger than a
//! datagram in a DATAGRAM capsule, and the real UDP echo server behind the
//! edge.

mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_core::Datagrams;
use pod_quic::noq;
use pod_warp::Identity;
use pod_warp::h3::wire::{self, FRAME_DATA, FRAME_HEADERS};
use pod_warp::h3::{self, Frames};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use tokio::net::UdpSocket;

/// The certificate of the front door, made for each handshake.
#[derive(Debug)]
struct Shown(Identity);

impl ResolvesServerCert for Shown {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        Some(Arc::new(self.0.certified(now)))
    }
}

/// What the edge saw of the request of the client.
#[derive(Default, Debug)]
struct Seen {
    protocol: Mutex<String>,
    path: Mutex<String>,
    forwarded: Mutex<Vec<usize>>,
    /// The kind of each answer of the edge, and its size.
    replies: Mutex<Vec<(&'static str, usize)>>,
}

/// The address of the real UDP echo server of the test.
async fn udp_echo() -> SocketAddr {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65535];
        loop {
            let Ok((n, from)) = socket.recv_from(&mut buf).await else { return };
            let _ = socket.send_to(&buf[..n], from).await;
        }
    });
    addr
}

/// The front door of HTTP/3: it reads the CONNECT-UDP request, answers 200,
/// and moves each payload to the echo server and back. The address, the
/// pinned key of the edge, and the record of the request.
async fn front_door(echo: SocketAddr) -> (SocketAddr, Vec<u8>, Arc<Seen>) {
    let server_id = Identity::generate().unwrap();
    let pin = server_id.spki_der().to_vec();
    let mut tls = rustls::ServerConfig::builder_with_provider(common::provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(Shown(server_id)));
    tls.alpn_protocols = vec![h3::ALPN.to_vec()];
    let crypto = noq::crypto::rustls::QuicServerConfig::try_from(Arc::new(tls)).unwrap();
    let config = pod_quic::endpoint::server_config(Arc::new(crypto)).unwrap();
    let server = pod_quic::endpoint::server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
    let addr = server.local_addr().unwrap();
    let seen = Arc::new(Seen::default());
    let kept = seen.clone();
    tokio::spawn(async move {
        let Some(incoming) = server.accept().await else { return };
        let Ok(conn) = incoming.await else { return };
        let control = conn.clone();
        tokio::spawn(async move {
            while let Ok(mut uni) = control.accept_uni().await {
                let _ = uni.read_chunk(65536).await;
            }
        });
        let Ok((mut send, mut recv)) = conn.accept_bi().await else { return };
        let mut frames = Frames::default();
        let fields = loop {
            if let Ok(Some((kind, payload))) = frames.next_frame() {
                if kind == FRAME_HEADERS {
                    break wire::request_fields(&payload).unwrap();
                }
                continue;
            }
            match recv.read_chunk(65536).await {
                Ok(Some(chunk)) => frames.push(&chunk),
                _ => return,
            }
        };
        let get = |name: &str| fields.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()).unwrap_or_default();
        *kept.protocol.lock().unwrap() = get(":protocol");
        *kept.path.lock().unwrap() = get(":path");
        assert_eq!(get(":method"), "CONNECT");
        send.write_all(&h3::frame(FRAME_HEADERS, &[0, 0, 0xc0 | 25])).await.unwrap();
        if get(":protocol") != "connect-udp" {
            // A stream CONNECT (RFC 9220): each DATA frame comes back.
            loop {
                match recv.read_chunk(65536).await {
                    Ok(Some(chunk)) => {
                        frames.push(&chunk);
                        while let Ok(Some((kind, payload))) = frames.next_frame() {
                            if kind == FRAME_DATA && send.write_all(&h3::frame(FRAME_DATA, &payload)).await.is_err() {
                                return;
                            }
                        }
                    }
                    _ => return,
                }
            }
        }
        // The payloads of the client go to the echo server, and its answers
        // come back in a datagram or a capsule.
        let out = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut buf = vec![0u8; 65535];
        let mut capsules = pod_warp::capsule::CapsuleDecoder::new();
        loop {
            tokio::select! {
                datagram = conn.read_datagram() => {
                    let Ok(d) = datagram else { return };
                    if let Some((payload, n)) = udp_payload(&d) {
                        kept.forwarded.lock().unwrap().push(n);
                        if let Some(reply) = echo_once(&out, echo, &payload, &conn, &mut send, &mut buf).await {
                            kept.replies.lock().unwrap().push(reply);
                        }
                    }
                }
                chunk = recv.read_chunk(65536) => match chunk {
                    Ok(Some(chunk)) => {
                        frames.push(&chunk);
                        while let Ok(Some((kind, payload))) = frames.next_frame() {
                            if kind != FRAME_DATA { continue; }
                            capsules.push(&payload);
                            while let Some((kind, value)) = capsules.next_capsule() {
                                if kind != pod_warp::capsule::CAPSULE_DATA { continue; }
                                let Some((_, n)) = pod_warp::capsule::get_varint(&value) else { continue };
                                kept.forwarded.lock().unwrap().push(value.len() - n);
                                if let Some(reply) = echo_once(&out, echo, &value[n..], &conn, &mut send, &mut buf).await {
                                    kept.replies.lock().unwrap().push(reply);
                                }
                            }
                        }
                    }
                    _ => return,
                },
            }
        }
    });
    (addr, pin, seen)
}

/// The payload of an HTTP datagram of the request stream: after the Quarter
/// Stream ID and the Context ID.
fn udp_payload(d: &[u8]) -> Option<(Vec<u8>, usize)> {
    let (_, a) = pod_warp::capsule::get_varint(d)?;
    let (_, b) = pod_warp::capsule::get_varint(&d[a..])?;
    Some((d[a + b..].to_vec(), d.len() - a - b))
}

/// Send one payload to the echo server and its answer back to the client.
async fn echo_once(
    out: &UdpSocket,
    echo: SocketAddr,
    payload: &[u8],
    conn: &noq::Connection,
    send: &mut noq::SendStream,
    buf: &mut [u8],
) -> Option<(&'static str, usize)> {
    if out.send_to(payload, echo).await.is_err() {
        return None;
    }
    let Ok((n, _)) = out.recv_from(buf).await else { return None };
    let max = conn.max_datagram_size().unwrap_or(0);
    if n + 2 <= max {
        let mut d = Vec::new();
        pod_warp::capsule::put_varint(&mut d, 0);
        pod_warp::capsule::put_varint(&mut d, 0);
        d.extend_from_slice(&buf[..n]);
        let _ = conn.send_datagram(d.into());
        Some(("datagram", n))
    } else {
        let mut value = Vec::new();
        pod_warp::capsule::put_varint(&mut value, 0);
        value.extend_from_slice(&buf[..n]);
        let mut body = Vec::new();
        pod_warp::capsule::put_capsule(&mut body, pod_warp::capsule::CAPSULE_DATA, &value);
        let _ = send.write_all(&h3::frame(FRAME_DATA, &body)).await;
        Some(("capsule", n))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_udp_reaches_a_local_udp_echo_server() {
    pod_testkit::need_loopback!();
    let echo = udp_echo().await;
    let (door, pin, seen) = front_door(echo).await;
    let client = Identity::generate().unwrap();
    let tls = pod_warp::tls::client_config(common::provider(), &client, pin, h3::ALPN).unwrap();
    let datagrams: Arc<dyn Datagrams> =
        Arc::new(pod_core::SystemDatagrams::bind("0.0.0.0:0".parse().unwrap()).await.unwrap());
    let authority = pod_warp::service::connect_uri();
    let mut session = h3::H3Udp::connect(
        door,
        datagrams,
        tls,
        "consumer-masque.cloudflareclient.com",
        authority.trim_start_matches("https://"),
        ("127.0.0.1", echo.port()),
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    // A payload that fits in a QUIC datagram, and one that does not.
    let small = b"a udp payload through the proxy".to_vec();
    let small_len = small.len();
    session.send(&[small.clone(), vec![0x5au8; 3000]]).await.unwrap();
    let mut got = Vec::new();
    for _ in 0..2 {
        let payload = tokio::time::timeout(Duration::from_secs(10), session.recv())
            .await
            .expect("an echo")
            .unwrap()
            .expect("a payload");
        got.push(payload);
    }
    got.sort_by_key(|p| p.len());
    assert_eq!(got, vec![small, vec![0x5au8; 3000]]);
    // The request names the protocol and the URI template of the target.
    assert_eq!(seen.protocol.lock().unwrap().as_str(), "connect-udp");
    assert_eq!(seen.path.lock().unwrap().as_str(), format!("/.well-known/masque/udp/127.0.0.1/{}/", echo.port()));
    let forwarded = seen.forwarded.lock().unwrap().clone();
    assert_eq!(forwarded.len(), 2, "{forwarded:?}");
    // The small payload came back in a QUIC datagram, and the large one in
    // a DATAGRAM capsule of the request stream.
    let mut replies = seen.replies.lock().unwrap().clone();
    replies.sort_by_key(|(_, n)| *n);
    assert_eq!(replies, vec![("datagram", small_len), ("capsule", 3000)]);
    session.close();
}

/// A stream CONNECT (RFC 9220) on the same path: the protocol of the
/// caller, no datagram, and the bytes of the request stream both ways.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_connect_carries_bytes_on_the_same_path() {
    pod_testkit::need_loopback!();
    let echo = udp_echo().await;
    let (door, pin, seen) = front_door(echo).await;
    let client = Identity::generate().unwrap();
    let tls = pod_warp::tls::client_config(common::provider(), &client, pin, h3::ALPN).unwrap();
    let datagrams: Arc<dyn Datagrams> =
        Arc::new(pod_core::SystemDatagrams::bind("0.0.0.0:0".parse().unwrap()).await.unwrap());
    let mode = h3::Mode::Stream { protocol: "pod-test".into() };
    let (mut tx, mut rx) = h3::connect_mode(
        door,
        datagrams,
        tls,
        "consumer-masque.cloudflareclient.com",
        "door.test",
        Duration::from_secs(10),
        &mode,
    )
    .await
    .unwrap();
    tx.write(b"a stream of the caller").await.unwrap();
    let back =
        tokio::time::timeout(Duration::from_secs(10), rx.read()).await.expect("an echo").unwrap().expect("bytes");
    assert_eq!(back, b"a stream of the caller");
    assert_eq!(seen.protocol.lock().unwrap().as_str(), "pod-test");
    tx.close();
}

/// The headers of the mode, and the template of an IPv6 target.
#[test]
fn the_template_and_the_headers_of_connect_udp() {
    assert_eq!(h3::udp_path("example.test", 443), "/.well-known/masque/udp/example.test/443/");
    assert_eq!(h3::udp_path("[2001:db8::1]", 53), "/.well-known/masque/udp/2001:db8::1/53/");
    assert_eq!(h3::UDP_TEMPLATE, "/.well-known/masque/udp/{target_host}/{target_port}/");
    let mode = h3::Mode::Udp { host: "example.test".into(), port: 443 };
    assert_eq!(mode.protocol(), "connect-udp");
    assert!(mode.datagrams());
    let section = h3::connect_headers("proxy.example", &mode);
    let fields = wire::request_fields(&section).unwrap();
    let get = |name: &str| fields.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
    assert_eq!(get(":method").as_deref(), Some("CONNECT"));
    assert_eq!(get(":scheme").as_deref(), Some("https"));
    assert_eq!(get(":protocol").as_deref(), Some("connect-udp"));
    assert_eq!(get(":path").as_deref(), Some("/.well-known/masque/udp/example.test/443/"));
    assert_eq!(get(":authority").as_deref(), Some("proxy.example"));
    // CONNECT-IP keeps its own protocol and the root path.
    let ip = h3::connect_headers("proxy.example", &h3::Mode::Ip);
    let fields = wire::request_fields(&ip).unwrap();
    let get = |name: &str| fields.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
    assert_eq!(get(":protocol").as_deref(), Some("cf-connect-ip"));
    assert_eq!(get(":path").as_deref(), Some("/"));
    // A stream mode uses the protocol of the caller and no datagram.
    let stream = h3::Mode::Stream { protocol: "webtransport".into() };
    assert_eq!(stream.protocol(), "webtransport");
    assert!(!stream.datagrams());
}

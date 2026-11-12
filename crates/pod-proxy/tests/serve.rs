//! The server face of a proxy (`pod_proxy::serve`): the SOCKS5 greeting,
//! the user and the password, the request and the reply; the request of an
//! HTTP proxy; the datagram of a UDP association. From podnet's
//! `crates/podnet-netns/src/socks.rs`.

use std::net::SocketAddr;

use pod_core::{Class, Failure, Stage, Target};
use pod_proxy::Credentials;
use pod_proxy::serve::{
    self, HttpProxyRequest, REPLY_GENERAL_FAILURE, REPLY_HOST_UNREACHABLE, REPLY_NOT_ALLOWED, REPLY_SUCCEEDED,
};
use pod_proxy::udp;
use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

#[test]
fn unwraps_socks5_udp_headers() {
    let mut v4 = vec![0, 0, 0, 1, 1, 1, 1, 1, 0, 53];
    v4.extend_from_slice(b"query");
    let (target, data) = udp::unwrap(&v4).unwrap();
    assert_eq!(target, Target::Addr("1.1.1.1:53".parse().unwrap()));
    assert_eq!(data, b"query");
    let mut name = vec![0, 0, 0, 3, 3];
    name.extend_from_slice(b"a.b");
    name.extend_from_slice(&[0x01, 0xbb]);
    name.extend_from_slice(b"x");
    let (target, data) = udp::unwrap(&name).unwrap();
    assert_eq!(target, Target::Name { host: "a.b".into(), port: 443 });
    assert_eq!(data, b"x");
    let v6 = udp::wrap(b"y", "2606:4700::1111".parse().unwrap(), 853);
    assert_eq!(udp::unwrap(&v6).unwrap(), (Target::Addr("[2606:4700::1111]:853".parse().unwrap()), &b"y"[..]));
    // A fragment, an unknown type, a short header and an empty name.
    assert!(udp::unwrap(&[0, 0, 1, 1, 1, 1, 1, 1, 0, 53]).is_none());
    assert!(udp::unwrap(&[0, 0, 0, 9]).is_none());
    assert!(udp::unwrap(&[0, 0, 0, 1, 1, 1, 1]).is_none());
    assert!(udp::unwrap(&[0, 0, 0, 3, 0, 0, 53]).is_none());
}

#[tokio::test]
async fn socks5_ipv4_connect() {
    let (mut client, mut server) = duplex(256);
    let task = tokio::spawn(async move {
        client.write_all(&[5, 1, 0]).await.unwrap();
        let mut reply = [0u8; 2];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply, [5, 0]);
        client.write_all(&[5, 1, 0, 1, 10, 66, 0, 2, 0x1f, 0x90]).await.unwrap();
        client
    });
    let (command, target) = serve::read_socks5(&mut server, None).await.unwrap();
    assert_eq!(command, serve::CMD_CONNECT);
    assert_eq!(target, Target::Addr("10.66.0.2:8080".parse().unwrap()));
    let bound: SocketAddr = "10.66.0.2:8080".parse().unwrap();
    serve::socks5_reply(&mut server, REPLY_SUCCEEDED, Some(bound)).await.unwrap();
    let mut client = task.await.unwrap();
    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], REPLY_SUCCEEDED);
    assert_eq!(&reply[4..8], &[10, 66, 0, 2]);
}

/// podnet issue 78: a request for an http:// URL is forwarded in origin
/// form, with no Proxy-* header and with Connection: close.
#[tokio::test]
async fn an_http_url_request_is_forwarded_in_origin_form() {
    let (mut client, mut server) = duplex(1024);
    client
        .write_all(
            b"GET http://deb.example:8080/dists/x?y=1 HTTP/1.1\r\nHost: deb.example:8080\r\nProxy-Authorization: Basic eA==\r\nProxy-Connection: keep-alive\r\nConnection: keep-alive\r\nUser-Agent: apt\r\n\r\nBODY",
        )
        .await
        .unwrap();
    let opening = serve::read_http_proxy(&mut server).await.unwrap();
    assert_eq!(opening.authorization.as_deref(), Some("Basic eA=="));
    assert_eq!(opening.rest, b"BODY");
    let HttpProxyRequest::Forward { target, head } = opening.request else { panic!("{:?}", opening.request) };
    assert_eq!(target, Target::Name { host: "deb.example".into(), port: 8080 });
    let head = String::from_utf8(head).unwrap();
    assert!(head.starts_with("GET /dists/x?y=1 HTTP/1.1\r\n"), "{head}");
    assert!(head.contains("\r\nuser-agent: apt\r\n"), "{head}");
    assert!(head.contains("host: deb.example:8080\r\n"), "{head}");
    assert!(!head.to_ascii_lowercase().contains("proxy-"), "{head}");
    assert!(head.ends_with("Connection: close\r\n\r\n"), "{head}");
    assert_eq!(head.to_ascii_lowercase().matches("connection:").count(), 1, "{head}");
    let (mut client, mut server) = duplex(1024);
    client.write_all(b"GET http://example.com HTTP/1.0\r\n\r\n").await.unwrap();
    let opening = serve::read_http_proxy(&mut server).await.unwrap();
    let HttpProxyRequest::Forward { target, head } = opening.request else { panic!("{:?}", opening.request) };
    assert_eq!(target.port(), 80);
    assert!(String::from_utf8(head).unwrap().starts_with("GET / HTTP/1.0\r\n"));
    // A URL that is not http:// is refused with 400.
    let (mut client, mut server) = duplex(1024);
    client.write_all(b"GET https://example.com/ HTTP/1.1\r\n\r\n").await.unwrap();
    assert_eq!(serve::read_http_proxy(&mut server).await.unwrap_err().status(), Some(400));
}

async fn socks5_request_with(client_to_server: &[u8]) -> Target {
    let (mut client, mut server) = duplex(256);
    let payload = client_to_server.to_vec();
    tokio::spawn(async move {
        client.write_all(&payload).await.unwrap();
        // Keep the connection open while the server reads.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    });
    serve::read_socks5(&mut server, None).await.unwrap().1
}

#[tokio::test]
async fn socks5_domain_target() {
    let target = socks5_request_with(&[5, 1, 0, 5, 1, 0, 3, 3, b'a', b'p', b'i', 0x00, 0x50]).await;
    assert_eq!(target, Target::Name { host: "api".into(), port: 80 });
}

#[tokio::test]
async fn socks5_ipv6_target() {
    let target =
        socks5_request_with(&[5, 1, 0, 5, 1, 0, 4, 0xfd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0x01, 0xbb])
            .await;
    assert_eq!(target, Target::Addr("[fd00::2]:443".parse().unwrap()));
}

#[tokio::test]
async fn socks5_rejects_missing_no_auth() {
    let (mut client, mut server) = duplex(64);
    let peer = tokio::spawn(async move {
        client.write_all(&[5, 1, 2]).await.unwrap();
        let mut reply = [0u8; 2];
        let _ = client.read_exact(&mut reply).await;
        reply
    });
    let err = serve::read_socks5(&mut server, None).await.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(peer.await.unwrap(), [5, 0xff]);
}

/// podnet issue 146: with credentials, the method of RFC 1929 is required,
/// and a wrong password gets the status 1.
#[tokio::test]
async fn socks5_checks_the_user_and_the_password() {
    let credentials = Credentials::new("alice", "pw").unwrap();
    for (pass, ok) in [("pw", true), ("no", false)] {
        let (mut client, mut server) = duplex(256);
        let peer = tokio::spawn(async move {
            client.write_all(&[5, 1, 2]).await.unwrap();
            let mut reply = [0u8; 2];
            client.read_exact(&mut reply).await.unwrap();
            assert_eq!(reply, [5, 2]);
            let mut auth = vec![1, 5];
            auth.extend_from_slice(b"alice");
            auth.push(pass.len() as u8);
            auth.extend_from_slice(pass.as_bytes());
            auth.extend_from_slice(&[5, 1, 0, 1, 10, 0, 0, 1, 0, 22]);
            client.write_all(&auth).await.unwrap();
            client.read_exact(&mut reply).await.unwrap();
            reply
        });
        let read = serve::read_socks5(&mut server, Some(&credentials)).await;
        assert_eq!(read.is_ok(), ok, "{pass}");
        assert_eq!(peer.await.unwrap(), [1, if ok { 0 } else { 1 }]);
    }
}

#[tokio::test]
async fn http_connect_reads_target() {
    let (mut client, mut server) = duplex(256);
    client.write_all(b"CONNECT [fd00::2]:443 HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
    let opening = serve::read_http_proxy(&mut server).await.unwrap();
    assert_eq!(opening.request, HttpProxyRequest::Connect(Target::Addr("[fd00::2]:443".parse().unwrap())));
    assert!(opening.rest.is_empty() && opening.authorization.is_none());
    let (mut client, mut server) = duplex(256);
    client.write_all(b"CONNECT example.com HTTP/1.1\r\n\r\n").await.unwrap();
    assert_eq!(serve::read_http_proxy(&mut server).await.unwrap_err().status(), Some(400));
}

#[test]
fn detects_socks() {
    assert!(serve::looks_like_socks5(5));
    assert!(!serve::looks_like_socks5(b'G'));
}

#[test]
fn the_reply_of_a_failed_dial_follows_its_class() {
    let of = |class| serve::reply_for(&Failure::new(Stage::Connect, class, "x"));
    assert_eq!(of(Class::Policy), REPLY_NOT_ALLOWED);
    assert_eq!(of(Class::Unavailable), REPLY_HOST_UNREACHABLE);
    assert_eq!(of(Class::Timeout), REPLY_HOST_UNREACHABLE);
    assert_eq!(of(Class::Protocol), REPLY_GENERAL_FAILURE);
}

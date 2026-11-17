//! The handshakes of a hop of a chain on a stream that the caller holds
//! (PodUtils/podnet#652), against a stand-in proxy at the other end of a
//! pipe.

use std::time::Duration;

use pod_proxy::{DialError, Proxy, hop};
use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

const WITHIN: Duration = Duration::from_secs(5);

fn proxy(url: &str) -> Proxy {
    Proxy::parse(url).expect("a proxy URL")
}

/// SOCKS5 with a name: the greeting offers no authentication, and the
/// request carries the name and the port for the proxy to resolve.
#[tokio::test]
async fn a_socks5_hop_sends_the_name_to_the_proxy() {
    let (mut ours, mut theirs) = duplex(1024);
    let far = tokio::spawn(async move {
        let mut greet = [0u8; 3];
        theirs.read_exact(&mut greet).await.unwrap();
        assert_eq!(greet, [5, 1, 0], "one method: none");
        theirs.write_all(&[5, 0]).await.unwrap();
        let mut head = [0u8; 5];
        theirs.read_exact(&mut head).await.unwrap();
        assert_eq!(&head[..4], &[5, 1, 0, 3], "CONNECT of a name");
        let mut name = vec![0u8; usize::from(head[4]) + 2];
        theirs.read_exact(&mut name).await.unwrap();
        assert_eq!(&name[..name.len() - 2], b"example.test");
        assert_eq!(&name[name.len() - 2..], &443u16.to_be_bytes());
        theirs.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
    });
    let p = proxy("socks5h://127.0.0.1:1080");
    hop::socks5(&mut ours, "the test", &p, "example.test", 443, WITHIN).await.expect("the hop");
    far.await.unwrap();
}

/// `socks5://` resolves a name on this host, which a hop cannot: a
/// refusal before a byte goes out.
#[tokio::test]
async fn a_socks5_hop_with_local_names_refuses_a_name() {
    let (mut ours, _theirs) = duplex(64);
    let e = hop::socks5(&mut ours, "the test", &proxy("socks5://127.0.0.1:1080"), "example.test", 443, WITHIN)
        .await
        .expect_err("a name over socks5://");
    assert!(matches!(e, DialError::InvalidTarget(_)), "{e}");
}

/// CONNECT: the request names the target, and a 200 opens the hop; a 403
/// is a refusal with the proxy's words.
#[tokio::test]
async fn a_connect_hop_opens_on_200_and_refuses_on_403() {
    for (status, open) in [("200 Connection established", true), ("403 Forbidden", false)] {
        let (mut ours, mut theirs) = duplex(4096);
        let far = tokio::spawn(async move {
            let mut got = Vec::new();
            let mut byte = [0u8; 1];
            while !got.ends_with(b"\r\n\r\n") {
                theirs.read_exact(&mut byte).await.unwrap();
                got.push(byte[0]);
            }
            theirs.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n").as_bytes()).await.unwrap();
            String::from_utf8(got).unwrap()
        });
        let ask = hop::Ask { app: "the test", user_agent: None };
        let r = hop::connect(&mut ours, ask, &proxy("http://127.0.0.1:3128"), "example.test", 22, WITHIN).await;
        let request = far.await.unwrap();
        assert!(request.starts_with("CONNECT example.test:22 HTTP/1.1\r\n"), "{request}");
        assert_eq!(r.is_ok(), open, "{status}: {r:?}");
        if !open {
            assert!(matches!(r, Err(DialError::ProxyRefused { status: 403, .. })), "{r:?}");
        }
    }
}

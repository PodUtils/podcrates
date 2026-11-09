//! The SOCKS5 datagram path against stand-in servers on local sockets: the
//! command 0x83 (UDP in the stream) with a round trip of one frame, the
//! reply 0x07 followed by `UDP ASSOCIATE`, a close after the command, the
//! refusal of a rule, a bound address of each type, and the bytes of the
//! frames and of the datagram header.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_proxy::udp::{self, associate, open_in_stream, read_frame, wrap, wrap_name, write_frame};
use pod_proxy::{DialError, Proxy};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What the stand-in does on one connection.
#[derive(Clone)]
struct Script {
    /// Its reply to the command, from the version to the end of the address;
    /// none: it closes the connection without a reply.
    reply: Option<Vec<u8>>,
    /// After a reply of success, it sends each frame back.
    echo: bool,
}

fn success(bound: &[u8]) -> Option<Vec<u8>> {
    Some([&[5u8, 0x00, 0x00][..], bound].concat())
}

const BOUND_ANY: [u8; 7] = [0x01, 0, 0, 0, 0, 0, 0];

/// A stand-in that runs `scripts` on its connections, one each, and records
/// the 10 bytes of the command that each got.
async fn stand_in(scripts: Vec<Script>) -> (Proxy, Arc<Mutex<Vec<[u8; 10]>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    tokio::spawn(async move {
        for script in scripts {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut head = [0u8; 2];
            sock.read_exact(&mut head).await.unwrap();
            let mut methods = vec![0u8; usize::from(head[1])];
            sock.read_exact(&mut methods).await.unwrap();
            sock.write_all(&[5, 0x00]).await.unwrap();
            let mut command = [0u8; 10];
            sock.read_exact(&mut command).await.unwrap();
            record.lock().unwrap().push(command);
            let Some(reply) = script.reply else { continue };
            sock.write_all(&reply).await.unwrap();
            if !script.echo || reply[1] != 0x00 {
                continue;
            }
            while let Ok(Some(frame)) = read_frame(&mut sock).await {
                write_frame(&mut sock, &frame).await.unwrap();
            }
        }
    });
    (Proxy::parse(&format!("socks5://127.0.0.1:{port}")).unwrap(), seen)
}

/// A step of a test, bounded: a frame with a wrong length must fail the test,
/// not wait for bytes that never come.
async fn within<T>(step: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), step).await.expect("the step ended in time")
}

async fn connect(proxy: &Proxy) -> TcpStream {
    TcpStream::connect(proxy.address()).await.unwrap()
}

#[tokio::test]
async fn a_datagram_goes_out_in_a_frame_and_comes_back_in_one() {
    let (proxy, seen) = stand_in(vec![Script { reply: success(&BOUND_ANY), echo: true }]).await;
    let mut stream = open_in_stream(connect(&proxy).await, "test", &proxy).await.unwrap();
    let datagram = wrap(b"query", IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 53);
    write_frame(&mut stream, &datagram).await.unwrap();
    assert_eq!(within(read_frame(&mut stream)).await.unwrap(), Some(datagram));
    assert_eq!(seen.lock().unwrap()[0], [5, 0x83, 0, 1, 0, 0, 0, 0, 0, 0]);
}

#[tokio::test]
async fn the_reply_7_is_a_refusal_of_the_command_and_associate_follows() {
    let relay = [&[0x04u8][..], &Ipv6Addr::LOCALHOST.octets(), &[0x14, 0xe9]].concat();
    let (proxy, seen) = stand_in(vec![
        Script { reply: Some(vec![5, 0x07, 0, 1, 0, 0, 0, 0, 0, 0]), echo: false },
        Script { reply: success(&relay), echo: false },
    ])
    .await;
    let refused = open_in_stream(connect(&proxy).await, "test", &proxy).await.err().unwrap();
    assert!(matches!(refused, DialError::UdpRefused { .. }), "{refused:?}");
    assert!(refused.to_string().contains("UDP in the stream"), "{refused}");
    let (_stream, at) = associate(connect(&proxy).await, "test", &proxy).await.unwrap();
    assert_eq!(at, SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 5353));
    let seen = seen.lock().unwrap();
    assert_eq!((seen[0][1], seen[1][1]), (0x83, 0x03));
}

#[tokio::test]
async fn a_close_after_the_command_is_an_error() {
    let (proxy, _) = stand_in(vec![Script { reply: None, echo: false }, Script { reply: None, echo: false }]).await;
    let error = open_in_stream(connect(&proxy).await, "test", &proxy).await.err().unwrap();
    assert!(matches!(error, DialError::ProxyProtocol { .. }), "{error:?}");
    let error = associate(connect(&proxy).await, "test", &proxy).await.err().unwrap();
    assert!(matches!(error, DialError::ProxyProtocol { .. }), "{error:?}");
}

#[tokio::test]
async fn a_refusal_of_a_rule_is_a_403_and_the_reply_7_of_associate_is_not_udp_refused() {
    let (proxy, _) = stand_in(vec![
        Script { reply: Some(vec![5, 0x02, 0, 1, 0, 0, 0, 0, 0, 0]), echo: false },
        Script { reply: Some(vec![5, 0x07, 0, 1, 0, 0, 0, 0, 0, 0]), echo: false },
    ])
    .await;
    let error = open_in_stream(connect(&proxy).await, "test", &proxy).await.err().unwrap();
    assert!(matches!(error, DialError::ProxyRefused { status: 403, .. }), "{error:?}");
    let error = associate(connect(&proxy).await, "test", &proxy).await.err().unwrap();
    assert!(matches!(error, DialError::ProxyRefused { status: 502, .. }), "{error:?}");
}

#[tokio::test]
async fn a_bound_name_that_is_an_address_in_text_is_read_to_its_end() {
    let name = [&[0x03u8, 9][..], b"127.0.0.1", &[0x00, 0x35]].concat();
    let (proxy, _) = stand_in(vec![Script { reply: success(&name), echo: false }]).await;
    let (_stream, at) = associate(connect(&proxy).await, "test", &proxy).await.unwrap();
    assert_eq!(at, "127.0.0.1:53".parse().unwrap());
    let word = [&[0x03u8, 5][..], b"a.b.c", &[0x00, 0x35]].concat();
    let (proxy, _) = stand_in(vec![Script { reply: success(&word), echo: false }]).await;
    let error = associate(connect(&proxy).await, "test", &proxy).await.err().unwrap();
    assert!(matches!(error, DialError::ProxyProtocol { .. }), "{error:?}");
}

#[tokio::test]
async fn a_frame_is_a_length_of_two_bytes_and_the_datagram() {
    let mut out = Vec::new();
    write_frame(&mut out, &[9, 8, 7]).await.unwrap();
    assert_eq!(out, [0, 3, 9, 8, 7]);
    let mut empty = Vec::new();
    write_frame(&mut empty, &[]).await.unwrap();
    assert_eq!(empty, [0, 0]);
    let too_long = vec![0u8; 65536];
    let error = write_frame(&mut Vec::new(), &too_long).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    let mut longest = Vec::new();
    write_frame(&mut longest, &too_long[..65535]).await.unwrap();
    assert_eq!(&longest[..2], [0xff, 0xff]);
}

#[tokio::test]
async fn the_end_of_the_stream_is_none_between_frames_and_an_error_inside_one() {
    let mut two: &[u8] = &[0, 1, 5, 0, 0];
    assert_eq!(read_frame(&mut two).await.unwrap(), Some(vec![5]));
    assert_eq!(read_frame(&mut two).await.unwrap(), Some(vec![]));
    assert_eq!(read_frame(&mut two).await.unwrap(), None);
    let mut none: &[u8] = &[];
    assert_eq!(read_frame(&mut none).await.unwrap(), None);
    let mut half_length: &[u8] = &[0];
    assert_eq!(read_frame(&mut half_length).await.unwrap(), None);
    let mut cut: &[u8] = &[0, 3, 1];
    let error = read_frame(&mut cut).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
}

#[test]
fn the_datagram_has_the_header_of_rfc_1928() {
    assert_eq!(wrap(b"x", IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)), 0x1234), [0, 0, 0, 1, 1, 2, 3, 4, 0x12, 0x34, b'x']);
    let v6 = wrap(b"", IpAddr::V6(Ipv6Addr::LOCALHOST), 53);
    assert_eq!((v6[3], v6.len()), (4, 4 + 16 + 2));
    assert_eq!(wrap_name(b"p", "a.b", 443).unwrap(), [0, 0, 0, 3, 3, b'a', b'.', b'b', 0x01, 0xbb, b'p']);
    let long = "n".repeat(256);
    assert!(matches!(wrap_name(b"", &long, 1), Err(DialError::InvalidTarget(_))));
    assert_eq!((udp::CMD_UDP_ASSOCIATE, udp::CMD_UDP_IN_STREAM, udp::REPLY_COMMAND_NOT_SUPPORTED), (3, 0x83, 7));
}

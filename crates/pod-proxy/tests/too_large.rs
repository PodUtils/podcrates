//! The refusal of a body over its bound keeps what the reader knows: the
//! status, the declared or seen size, the bound and the start of the body,
//! in each framing and in both decoders. The answer of a full storage pool
//! of incusd is the model: HTTP 200, one chunk of an `error` envelope.

use std::time::Duration;

use pod_proxy::http::{self, BodySize, KEPT_HEAD, TooLarge, decode_chunked};
use tokio::io::AsyncWriteExt;

const LIMIT: Duration = Duration::from_secs(30);
const BOUND: usize = 64 * 1024;

/// An `error` envelope of incusd of `n` bytes: the stderr of `tar`, one
/// line for each file.
fn envelope(n: usize) -> Vec<u8> {
    let start = r#"{"type":"error","status":"","status_code":0,"operation":"","error_code":500,"error":"Failed creating instance from image: Unpack failed: Failed to run: tar -xf -: exit status 2 (tar: rootfs/a: Cannot open: No space left on device"#;
    let mut out = start.as_bytes().to_vec();
    while out.len() + 40 < n {
        out.extend_from_slice(br"\ntar: rootfs/b: Cannot open: No space left on device");
    }
    out.extend_from_slice(br#")","metadata":null}"#);
    out
}

/// Read the answer `raw` with the bound; the error of the read.
async fn refused(raw: Vec<u8>, close: bool) -> std::io::Error {
    let (mut client, mut peer) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        let _ = peer.write_all(&raw).await;
        if !close {
            // Hold the stream open: the refusal must not wait for its end.
            tokio::time::sleep(LIMIT).await;
        }
    });
    let read = http::read_response(&mut client, Vec::new(), BOUND);
    let got = tokio::time::timeout(pod_testkit::host::scaled(LIMIT), read).await.expect("the read ends");
    server.abort();
    got.expect_err("a body over the bound")
}

#[tokio::test]
async fn one_chunk_over_the_bound_keeps_the_status_the_size_and_the_envelope() {
    let body = envelope(3 << 20);
    let mut raw = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n", body.len()).into_bytes();
    raw.extend_from_slice(&body);
    raw.extend_from_slice(b"\r\n0\r\n\r\n");
    let e = refused(raw, false).await;
    assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
    let refusal = TooLarge::of(&e).expect("a typed refusal");
    assert_eq!((refusal.status, refusal.reason.as_str()), (Some(200), "OK"));
    assert_eq!(refusal.size, BodySize::Chunk { before: 0, size: body.len() as u64 });
    assert_eq!(refusal.bound, BOUND as u64);
    assert_eq!(refusal.head, body[..KEPT_HEAD]);
    let text = e.to_string();
    let size = body.len();
    assert!(
        text.starts_with(&format!(
            "the response body is larger than allowed (HTTP 200 OK): a chunk of {size} bytes, over the bound of {BOUND} \
             bytes; the body starts with \"{{\\\"type\\\":\\\"error\\\""
        )),
        "{text}"
    );
}

#[tokio::test]
async fn a_chunk_after_others_names_the_bytes_before_it() {
    let body = envelope(256 * 1024);
    let (first, second) = body.split_at(1000);
    let mut raw = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n", first.len()).into_bytes();
    raw.extend_from_slice(first);
    raw.extend_from_slice(format!("\r\n{:x}\r\n", second.len()).as_bytes());
    raw.extend_from_slice(second);
    raw.extend_from_slice(b"\r\n0\r\n\r\n");
    let e = refused(raw, false).await;
    let refusal = TooLarge::of(&e).expect("a typed refusal");
    assert_eq!(refusal.size, BodySize::Chunk { before: 1000, size: second.len() as u64 });
    // The head is the start of the body, across the two chunks.
    assert_eq!(refusal.head, body[..KEPT_HEAD]);
}

#[tokio::test]
async fn a_content_length_over_the_bound_keeps_the_start_of_the_body() {
    let body = envelope(1 << 20);
    let mut raw = format!("HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    raw.extend_from_slice(&body);
    let e = refused(raw, false).await;
    let refusal = TooLarge::of(&e).expect("a typed refusal");
    assert_eq!((refusal.status, refusal.size), (Some(500), BodySize::Length(body.len() as u64)));
    assert_eq!(refusal.head, body[..KEPT_HEAD]);
    assert!(
        e.to_string().contains(&format!("(HTTP 500 Internal Server Error): a Content-Length of {} bytes", body.len()))
    );
}

#[tokio::test]
async fn a_chunk_size_past_any_width_says_so() {
    let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n10000000000000000\r\n{\"type\"".to_vec();
    let e = refused(raw, true).await;
    let refusal = TooLarge::of(&e).expect("a typed refusal");
    assert_eq!(refusal.size, BodySize::PastAnyBound { digits: 17 });
    assert_eq!(refusal.head, b"{\"type\"");
    assert!(e.to_string().contains("a chunk size of 17 hex digits, larger than any bound"), "{e}");
}

#[tokio::test]
async fn a_body_to_the_close_names_the_bytes_seen() {
    let body = envelope(200 * 1024);
    let mut raw = b"HTTP/1.1 200 OK\r\n\r\n".to_vec();
    raw.extend_from_slice(&body);
    let e = refused(raw, true).await;
    let refusal = TooLarge::of(&e).expect("a typed refusal");
    let BodySize::Seen(seen) = refusal.size else { panic!("{refusal:?}") };
    assert!(seen > BOUND as u64, "{seen}");
    assert_eq!(refusal.head, body[..KEPT_HEAD]);
}

/// The decoder of a whole body gives the text of the same refusal.
#[test]
fn the_decoder_of_a_whole_body_gives_the_same_refusal() {
    let body = envelope(200 * 1024);
    let mut raw = format!("{:x}\r\n", body.len()).into_bytes();
    raw.extend_from_slice(&body);
    raw.extend_from_slice(b"\r\n0\r\n\r\n");
    let text = decode_chunked(&raw, BOUND).unwrap_err();
    let same = TooLarge {
        status: None,
        reason: String::new(),
        size: BodySize::Chunk { before: 0, size: body.len() as u64 },
        bound: BOUND as u64,
        head: body[..KEPT_HEAD].to_vec(),
    };
    assert_eq!(text, same.to_string());
    let text = decode_chunked(b"10000000000000000\r\nab", BOUND).unwrap_err();
    assert!(text.contains("a chunk size of 17 hex digits, larger than any bound"), "{text}");
}

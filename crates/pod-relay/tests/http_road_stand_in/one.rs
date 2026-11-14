//! The one-POST form at the stand-in, as the role serves it: `200` at once
//! with the mark, then the body's chunks to the target as they come, and the
//! target's bytes (`Plan::down`) down in chunks as they come. The target
//! ends once the body's last chunk came and each byte down went: the
//! answer's last chunk.

use std::sync::Arc;

use pod_relay::http_road::MARK;
use pod_relay::http_road::answer::{Body, Framing};
use sha2::Digest;
use tokio::io::{AsyncWriteExt, DuplexStream};

use super::{One, StandIn, answer};

/// The bytes down in each chunk.
const CHUNK: usize = 64 * 1024;

/// Serve the form's request on `io`, whose body starts with the bytes of
/// `buf`.
pub(super) async fn serve(me: Arc<StandIn>, io: DuplexStream, mut buf: Vec<u8>, headers: &[(String, String)]) {
    let token = headers.iter().find(|(n, _)| n == "x-relay-token").map(|(_, v)| v.clone());
    me.record(|r| {
        r.ones += 1;
        r.one_token = token;
    });
    let (mut from, mut to) = tokio::io::split(io);
    let cut = match me.plan.one {
        One::Absent => {
            let _ = to.write_all(&answer(404, &[], b"not found\n")).await;
            return;
        }
        One::Serve => None,
        One::CutAfter(n) => Some(n),
    };
    let head = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
    let mut first = head.as_bytes().to_vec();
    first.extend_from_slice(&chunk(MARK));
    if to.write_all(&first).await.is_err() {
        return;
    }
    let target = me.clone();
    let reader = tokio::spawn(async move {
        let mut body = Body::new(&mut from, &mut buf, Framing::Chunked);
        while let Ok(Some(bytes)) = body.next().await {
            target.record(|r| {
                r.up.update(&bytes);
                r.up_bytes += bytes.len() as u64;
            });
        }
        target.record(|r| r.one_up_ended = body.finished());
    });
    let mut sent = 0;
    for piece in me.plan.down.chunks(CHUNK) {
        if cut.is_some_and(|cut| sent >= cut) {
            // The connection ends with no last chunk.
            reader.abort();
            return;
        }
        if to.write_all(&chunk(piece)).await.is_err() || to.flush().await.is_err() {
            reader.abort();
            return;
        }
        sent += piece.len();
    }
    let _ = reader.await;
    if me.record(|r| r.one_up_ended) {
        let _ = to.write_all(b"0\r\n\r\n").await;
    }
}

/// One chunk of a body in chunks.
pub fn chunk(bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("{:x}\r\n", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\r\n");
    out
}

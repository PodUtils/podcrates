//! A hop between the client of the road and the stand-in, in memory: one
//! that passes bytes as they come, or one that holds a request's body in
//! chunks to its last chunk before the request goes on, as a proxy that
//! buffers each request does. Answers pass as they come through each.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pod_relay::http_road::answer::{Body, Framing};
use pod_relay::http_road::client::Road;
use pod_relay::http_road::conn::{BoxIo, Dial, Dialing};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf};

use super::one::chunk;

/// How the hop passes a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hop {
    Streams,
    Holds,
}

/// `road` through a hop of `how`, and the count of the requests in chunks
/// that came to the hop.
pub fn through(road: Road, how: Hop) -> (Road, Arc<AtomicU64>) {
    let chunked = Arc::new(AtomicU64::new(0));
    let (inner, count) = (road.dial.clone(), chunked.clone());
    let dial: Dial = Arc::new(move || -> Dialing {
        let (inner, count) = (inner.clone(), count.clone());
        Box::pin(async move {
            let far = inner().await?;
            let (ours, theirs) = tokio::io::duplex(64 * 1024);
            tokio::spawn(pass(theirs, far, how, count));
            Ok(Box::new(ours) as BoxIo)
        })
    });
    (Road { dial, ..road }, chunked)
}

/// The hop of one connection: `near` the client's side, `far` the role's.
async fn pass(mut near: DuplexStream, mut far: BoxIo, how: Hop, chunked: Arc<AtomicU64>) {
    if how == Hop::Streams {
        let _ = tokio::io::copy_bidirectional(&mut near, &mut far).await;
        return;
    }
    let (near_read, mut near_write) = tokio::io::split(near);
    let (mut far_read, far_write) = tokio::io::split(far);
    let answers = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut far_read, &mut near_write).await;
        let _ = near_write.shutdown().await;
    });
    requests(near_read, far_write, chunked).await;
    answers.abort();
}

/// Each request, whole: its head, then its body of a length, or in chunks
/// to the last chunk, which goes on as one chunk and the last.
async fn requests(mut from: ReadHalf<DuplexStream>, mut to: WriteHalf<BoxIo>, chunked: Arc<AtomicU64>) {
    let mut buf = Vec::new();
    loop {
        let end = loop {
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break end + 4;
            }
            let mut more = vec![0u8; 64 * 1024];
            match from.read(&mut more).await {
                Ok(n) if n > 0 => buf.extend_from_slice(&more[..n]),
                _ => return,
            }
        };
        let head: Vec<u8> = buf.drain(..end).collect();
        let text = String::from_utf8_lossy(&head).to_ascii_lowercase();
        let mut whole = head.clone();
        if text.contains("transfer-encoding: chunked") {
            chunked.fetch_add(1, Ordering::SeqCst);
            let Ok(bytes) = Body::new(&mut from, &mut buf, Framing::Chunked).all(64 << 20).await else { return };
            if !bytes.is_empty() {
                whole.extend_from_slice(&chunk(&bytes));
            }
            whole.extend_from_slice(b"0\r\n\r\n");
        } else {
            let length = text
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let mut body = Body::new(&mut from, &mut buf, Framing::Length(length as u64));
            let Ok(bytes) = body.all(64 << 20).await else { return };
            whole.extend_from_slice(&bytes);
        }
        if to.write_all(&whole).await.is_err() {
            return;
        }
    }
}

//! The HTTP/2 part of a stand-in of WARP's MASQUE front door (feature
//! `masque`), from podssh's stand-in edge: h2's server over a stream of the
//! caller (its TLS, with its check of the client's key), a CONNECT that
//! must be connect-ip's as the edge reads it, and the capsules of the body.
//! Each IP packet of a DATA capsule goes to the far end of the caller, and
//! each packet of the far end goes back in a DATA capsule. The far end is
//! the caller's (a userspace stack, an echo): this kit takes no type of a
//! pod crate.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use bytes::{BufMut as _, Bytes, BytesMut};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

/// The capsule type of an IP packet.
pub const CAPSULE_DATA: u64 = 0;

/// The far end of one tunnel: the packets of the client go to `to_far`,
/// and the packets of `from_far` go to the client.
pub struct Far {
    pub to_far: mpsc::Sender<Vec<u8>>,
    pub from_far: mpsc::Receiver<Vec<u8>>,
}

/// What the stand-in saw, for each tunnel of a test.
#[derive(Debug, Default)]
pub struct Seen {
    /// The CONNECTs that it answered with 200.
    pub tunnels: AtomicUsize,
    /// The CONNECTs that it refused.
    pub refused: AtomicUsize,
    /// The headers of the last CONNECT, names in lower case, and its
    /// authority.
    pub headers: Mutex<Vec<(String, String)>>,
    pub authority: Mutex<Option<String>>,
    /// Set to relay nothing in each tunnel answered from then on: TLS,
    /// HTTP/2 and the 200 as before, then each packet read and dropped.
    pub silent: AtomicBool,
}

impl Seen {
    /// The value of the header `name` of the last CONNECT.
    pub fn header(&self, name: &str) -> Option<String> {
        let headers = self.headers.lock().unwrap_or_else(PoisonError::into_inner);
        headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone())
    }
}

/// A QUIC variable-length integer (RFC 9000, section 16).
pub fn put_varint(out: &mut BytesMut, value: u64) {
    match value {
        0..64 => out.put_u8(value as u8),
        64..16_384 => out.put_u16(0x4000 | value as u16),
        16_384..1_073_741_824 => out.put_u32(0x8000_0000 | value as u32),
        _ => out.put_u64(0xc000_0000_0000_0000 | value),
    }
}

/// The value of a varint at the start of `input`, and its length.
pub fn get_varint(input: &[u8]) -> Option<(u64, usize)> {
    let first = *input.first()?;
    let len = 1usize << (first >> 6);
    let bytes = input.get(..len)?;
    let value = bytes[1..].iter().fold(u64::from(first & 0x3f), |v, b| (v << 8) | u64::from(*b));
    Some((value, len))
}

/// The capsule of `kind` around `value`.
pub fn capsule(kind: u64, value: &[u8]) -> Bytes {
    let mut out = BytesMut::with_capacity(value.len() + 10);
    put_varint(&mut out, kind);
    put_varint(&mut out, value.len() as u64);
    out.put_slice(value);
    out.freeze()
}

/// Serve one HTTP/2 connection of a client on `io`: the CONNECT of
/// connect-ip (else 400), then the packets both ways with `far` until the
/// client ends the body or the far end ends.
pub async fn serve<IO>(io: IO, mut far: Far, seen: Arc<Seen>)
where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let Ok(mut conn) = h2::server::handshake(io).await else { return };
    let Some(Ok((request, mut respond))) = conn.accept().await else { return };
    // The connection runs in its own task: it answers the PINGs of the
    // client and moves the frames of the stream.
    tokio::spawn(async move { while conn.accept().await.is_some() {} });
    let connect_ip = request.method() == http::Method::CONNECT
        && request.headers().get("cf-connect-proto").is_some_and(|v| v == "cf-connect-ip");
    {
        let mut headers = seen.headers.lock().unwrap_or_else(PoisonError::into_inner);
        *headers = request
            .headers()
            .iter()
            .map(|(n, v)| (n.as_str().to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
            .collect();
        *seen.authority.lock().unwrap_or_else(PoisonError::into_inner) =
            request.uri().authority().map(|a| a.to_string());
    }
    if !connect_ip {
        seen.refused.fetch_add(1, Ordering::SeqCst);
        let answer = http::Response::builder().status(400).body(()).unwrap_or_default();
        let _ = respond.send_response(answer, true);
        return;
    }
    let answer = http::Response::builder().status(200).body(()).unwrap_or_default();
    let Ok(mut send) = respond.send_response(answer, false) else { return };
    seen.tunnels.fetch_add(1, Ordering::SeqCst);
    let silent = seen.silent.load(Ordering::SeqCst);
    let mut body = request.into_body();
    let to_far = far.to_far.clone();
    let inbound = tokio::spawn(async move {
        let mut buf = Vec::new();
        while let Some(Ok(data)) = body.data().await {
            let _ = body.flow_control().release_capacity(data.len());
            buf.extend_from_slice(&data);
            while let Some((kind, value, used)) = next_capsule(&buf) {
                if kind == CAPSULE_DATA && !silent && to_far.send(value.to_vec()).await.is_err() {
                    return;
                }
                buf.drain(..used);
            }
        }
    });
    while let Some(packet) = far.from_far.recv().await {
        if silent {
            continue;
        }
        let data = capsule(CAPSULE_DATA, &packet);
        send.reserve_capacity(data.len());
        while send.capacity() < data.len() {
            match std::future::poll_fn(|cx| send.poll_capacity(cx)).await {
                Some(Ok(_)) => {}
                _ => return,
            }
        }
        if send.send_data(data, false).is_err() {
            break;
        }
    }
    inbound.abort();
}

/// The first whole capsule of `buf`: its type, its value and the bytes that
/// it takes.
fn next_capsule(buf: &[u8]) -> Option<(u64, &[u8], usize)> {
    let (kind, a) = get_varint(buf)?;
    let (len, b) = get_varint(&buf[a..])?;
    let start = a + b;
    let end = start.checked_add(usize::try_from(len).ok()?)?;
    Some((kind, buf.get(start..end)?, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_varint_and_a_capsule_read_back() {
        for value in [0u64, 63, 64, 16_383, 16_384, 1_073_741_823, 1_073_741_824] {
            let mut out = BytesMut::new();
            put_varint(&mut out, value);
            assert_eq!(get_varint(&out), Some((value, out.len())), "{value}");
        }
        let one = capsule(CAPSULE_DATA, b"\x45packet");
        assert_eq!(next_capsule(&one), Some((CAPSULE_DATA, &b"\x45packet"[..], one.len())));
        assert_eq!(next_capsule(&one[..one.len() - 1]), None, "one byte short");
    }
}

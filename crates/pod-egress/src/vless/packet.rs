//! XHTTP's packet-up (Xray-core, `transport/internet/splithttp/`): a GET of
//! `PATH/SESSION` whose answer streams the target's bytes, and for each
//! chunk up a POST of `PATH/SESSION/SEQ` with a body of that chunk, numbered
//! from 0 (`upload_queue.go`, `nextSeq`). The first POST is VLESS's request.
//! POSTs go one at a time, each after the 200 of the last, so they come in
//! order and the server's queue of 30 (`GetNormalizedScMaxBufferedPosts`)
//! never fills; what waits meanwhile goes in the next, up to the link's
//! largest POST (1,000,000 bytes by default, `GetNormalizedScMaxEachPostBytes`).
//! Plain requests with whole bodies: the mode that passes a proxy or a front
//! end that refuses the upgrade and holds a streamed body. It has no
//! half-close: the end of this side's bytes ends the POSTs, and the session
//! ends with the GET.

use std::time::Duration;

use bytes::Bytes;
use h2::client::SendRequest;
use tokio::io::{AsyncReadExt, DuplexStream};

use super::VlessError;
use super::link::Server;
use super::split::{Failed, Halves};
use super::xhttp::{answer, at, pad_first, request, send, send_all, session_id};
use crate::coalesce;
use pod_ws::h2_road::Stream;

/// The pipe in memory to the task of the POSTs: a write waits while a POST
/// of this much waits for its 200.
const PIPE: usize = 1 << 20;

pub(super) async fn open(
    server: &Server,
    sender: SendRequest<Bytes>,
    first: Vec<u8>,
    limit: Duration,
    agent: &str,
) -> Result<(Halves<Stream, DuplexStream>, coalesce::Report), VlessError> {
    let id = session_id()?;
    let get = request(server, http::Method::GET, &at(server, &id), false, agent)?;
    let (down, closed) = send(&sender, get, true, limit).await?;
    let down = answer(down, limit, "the GET of packet-up").await?;
    let read = pod_ws::h2_road::duplex(closed, down.into_body());
    let at_server = At { server: server.clone(), agent: agent.to_string(), limit };
    let report = post(&at_server, &sender, &id, 0, Bytes::from(first)).await?;
    let (ours, theirs) = tokio::io::duplex(PIPE);
    let failed = Failed::default();
    tokio::spawn(uploads(at_server, sender, id, theirs, failed.clone()));
    Ok((Halves { read, write: ours, failed }, report))
}

/// The server of the POSTs, the user agent of each, and the bound of each.
struct At {
    server: Server,
    agent: String,
    limit: Duration,
}

/// One POST of packet-up, its body whole, and its 200. The first POST of a
/// session carries VLESS's request, and goes with the padding of the first
/// write; its report comes back.
async fn post(
    to: &At,
    sender: &SendRequest<Bytes>,
    id: &str,
    seq: u64,
    body: Bytes,
) -> Result<coalesce::Report, VlessError> {
    let (server, limit) = (&to.server, to.limit);
    let mut post = request(server, http::Method::POST, &at(server, &format!("{id}/{seq}")), false, &to.agent)?;
    post.headers_mut().insert(http::header::CONTENT_LENGTH, http::HeaderValue::from(body.len()));
    let report = match seq {
        0 => pad_first(&mut post, body.len()),
        _ => coalesce::Report { bytes: body.len(), short: false, coalesced: false },
    };
    let (answered, mut up) = send(sender, post, false, limit).await?;
    send_all(&mut up, body, true).await.map_err(VlessError::Carry)?;
    answer(answered, limit, "a POST of packet-up").await.map(drop)?;
    Ok(report)
}

/// The POSTs after the first, until this side's bytes end or one fails;
/// a failure is kept, so that the next write says it.
async fn uploads(to: At, sender: SendRequest<Bytes>, id: String, mut from: DuplexStream, failed: Failed) {
    let mut buf = vec![0u8; to.server.extra.max_post];
    for seq in 1u64.. {
        let n = match from.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        if let Err(e) = post(&to, &sender, &id, seq, Bytes::copy_from_slice(&buf[..n])).await {
            let shown = to.server.shown();
            *failed.lock().unwrap_or_else(|p| p.into_inner()) = Some(e.line(&shown, "the target"));
            return;
        }
    }
}

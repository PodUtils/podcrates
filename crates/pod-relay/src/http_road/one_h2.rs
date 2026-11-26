//! The one-POST form on one stream of HTTP/2, where the host chose `h2` by
//! ALPN, as podssh's role serves it: a POST to the form's path with the
//! token, its headers with no `END_STREAM`; the answer's head `200` at once,
//! then DATA whose first bytes are [`MARK`] and the rest the target's bytes.
//! Each read of the caller goes up as DATA, as the client's window lets it,
//! and the caller's end is an empty DATA with `END_STREAM`, which the role
//! makes TCP's half-close to the target. The answer's `END_STREAM` is the
//! target's end; a reset of the stream, or a connection that ends, is a cut.
//! HTTP/2 has no chunks: the frames carry the bytes, and the role makes the
//! chunks of its own HTTP/1.1.

use std::sync::Arc;

use bytes::Bytes;
use h2::client::{ResponseFuture, SendRequest};
use h2::{Reason, RecvStream, SendStream};
use tokio::io::{AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf};

use pod_ws::Extra;

use pod_core::abort::AbortOnDrop;

use super::client::{End, Ended, LIMIT, Opened, Road, gather};
use super::conn::BoxIo;
use super::one::{CHUNK, Failed, MARK_WAIT, PIPE, REFUSAL_MAX, no_mark, refused, words};
use super::{Form, MARK};

/// What the answer said up to its mark.
enum Heard {
    /// The mark came: the answer's body, and its bytes after the mark.
    Mark(RecvStream, Bytes),
    Failed(Failed),
    /// The role asks for the request again over HTTP/1.1 (RFC 9113, section
    /// 7): a role that serves the form on HTTP/1.1 alone.
    Http1,
}

/// Open the form at `path` on `io`, whose TLS chose `h2`: the session, once
/// the mark came within [`MARK_WAIT`]; `None` when the role asks for
/// HTTP/1.1.
pub(super) async fn open(
    road: &Road,
    path: &str,
    token: &str,
    extra: &Extra,
    io: BoxIo,
) -> Option<Result<Opened, Failed>> {
    let not_carried = |why: String| Some(Err(Failed::NotCarried(why)));
    let (client, connection) = match tokio::time::timeout(LIMIT, h2::client::handshake(io)).await {
        Ok(Ok(shaken)) => shaken,
        Ok(Err(e)) => {
            return not_carried(format!("the handshake of HTTP/2 failed: {}", pod_core::text::plain(&e.to_string())));
        }
        Err(_) => return not_carried("the handshake of HTTP/2 took too long".into()),
    };
    // The task that drives the connection: a drop ends the connection, and
    // so the session at the role.
    let driver = AbortOnDrop::new(tokio::spawn(async move {
        let _ = connection.await;
    }));
    let request = match request(road, path, token, extra) {
        Ok(request) => request,
        Err(e) => return not_carried(format!("the request of HTTP/2: {}", pod_core::text::plain(&e.to_string()))),
    };
    let sent = async {
        let mut client = client.ready().await?;
        let (response, send) = client.send_request(request, false)?;
        Ok::<_, h2::Error>((client, response, send))
    };
    let (client, response, send) = match tokio::time::timeout(LIMIT, sent).await {
        Ok(Ok(sent)) => sent,
        Ok(Err(e)) => {
            return not_carried(format!("the request of HTTP/2 did not go: {}", pod_core::text::plain(&e.to_string())));
        }
        Err(_) => return not_carried("the request of HTTP/2 did not go in time".into()),
    };
    let (recv, first) = match tokio::time::timeout(MARK_WAIT, heard(response)).await {
        Ok(Heard::Mark(recv, first)) => (recv, first),
        Ok(Heard::Failed(failed)) => return Some(Err(failed)),
        Ok(Heard::Http1) => return None,
        Err(_) => return Some(Err(no_mark())),
    };
    let (app, ours) = tokio::io::duplex(PIPE);
    let (from_app, to_app) = tokio::io::split(ours);
    let ended = Arc::new(Ended::default());
    tokio::spawn(down(recv, to_app, first, ended.clone(), (client, driver)));
    tokio::spawn(up(send, from_app, ended.clone()));
    Some(Ok(Opened { stream: app, ended, form: Form::One }))
}

/// The POST of the form at `path`, with `token` and the headers of the
/// caller in `extra` after it; each never indexed by HPACK.
fn request(road: &Road, path: &str, token: &str, extra: &Extra) -> Result<http::Request<()>, String> {
    let sensitive = |value: &str| {
        let mut value = http::HeaderValue::from_bytes(value.as_bytes()).map_err(|e| e.to_string())?;
        value.set_sensitive(true);
        Ok::<_, String>(value)
    };
    let mut request = http::Request::builder()
        .method("POST")
        .uri(format!("https://{}{path}", road.authority))
        .header("x-relay-token", sensitive(token)?);
    for (name, value) in extra.iter() {
        let name = http::HeaderName::from_bytes(name.as_bytes()).map_err(|e| e.to_string())?;
        request = request.header(name, sensitive(value)?);
    }
    request
        .header("user-agent", road.user_agent.as_str())
        .header("accept-encoding", "identity")
        .header("cache-control", "no-store")
        .body(())
        .map_err(|e| e.to_string())
}

/// The answer's head and its first bytes, up to the mark.
async fn heard(response: ResponseFuture) -> Heard {
    let response = match response.await {
        Ok(response) => response,
        Err(e) if e.reason() == Some(Reason::HTTP_1_1_REQUIRED) => return Heard::Http1,
        Err(e) => {
            return Heard::Failed(Failed::NotCarried(format!(
                "the answer of HTTP/2 failed: {}",
                pod_core::text::plain(&e.to_string())
            )));
        }
    };
    let status = response.status();
    let mut recv = response.into_body();
    if status != http::StatusCode::OK {
        let mut body = Vec::new();
        while body.len() < REFUSAL_MAX {
            let Some(Ok(data)) = recv.data().await else { break };
            let _ = recv.flow_control().release_capacity(data.len());
            body.extend_from_slice(&data);
        }
        body.truncate(REFUSAL_MAX);
        let words = words(&body, status.canonical_reason().unwrap_or_default());
        return Heard::Failed(refused(status.as_u16(), words));
    }
    let mut first = Vec::new();
    while first.len() < MARK.len() {
        let Some(Ok(data)) = recv.data().await else {
            return Heard::Failed(Failed::NotCarried("the answer ended before its mark".into()));
        };
        let _ = recv.flow_control().release_capacity(data.len());
        first.extend_from_slice(&data);
    }
    if !first.starts_with(MARK) {
        return Heard::Failed(Failed::NotCarried("the answer's first bytes are not the form's mark".into()));
    }
    Heard::Mark(recv, Bytes::from(first.split_off(MARK.len())))
}

/// The answer's bytes to the caller, `first` first, to the session's end;
/// `keep` holds the connection until then.
async fn down(
    mut recv: RecvStream,
    mut to_app: WriteHalf<DuplexStream>,
    first: Bytes,
    ended: Arc<Ended>,
    keep: (SendRequest<Bytes>, AbortOnDrop),
) {
    let end = pump(&mut recv, &mut to_app, first).await;
    ended.set_once(end);
    let _ = to_app.shutdown().await;
    drop(keep);
}

/// The answer's DATA to the caller: its `END_STREAM` is the target's end,
/// and a reset is a cut. The window of the stream opens again only after
/// the caller took the bytes, so a slow caller holds the role.
async fn pump(recv: &mut RecvStream, to_app: &mut WriteHalf<DuplexStream>, first: Bytes) -> End {
    let mut bytes = first;
    let mut taken = 0;
    loop {
        if !bytes.is_empty() && to_app.write_all(&bytes).await.is_err() {
            return End::Failed("the caller of the HTTP road's one-POST form is gone".into());
        }
        let _ = recv.flow_control().release_capacity(taken);
        bytes = match recv.data().await {
            Some(Ok(data)) => data,
            None => return End::Closed { code: 1000, reason: "target closed".into() },
            Some(Err(e)) => {
                return End::Failed(format!(
                    "the relay cut the answer of the HTTP road's one-POST form: {}",
                    pod_core::text::plain(&e.to_string())
                ));
            }
        };
        taken = bytes.len();
    }
}

/// The caller's bytes up, each read as DATA, until its end, which goes as
/// `END_STREAM`; then the stream stays for the bytes down until the
/// session's end.
async fn up(mut send: SendStream<Bytes>, mut from_app: ReadHalf<DuplexStream>, ended: Arc<Ended>) {
    let mut buf = vec![0u8; CHUNK];
    loop {
        let (n, fin) = tokio::select! {
            biased;
            () = ended.wait() => return,
            got = gather(&mut from_app, &mut buf) => got,
        };
        let sent = tokio::select! {
            biased;
            () = ended.wait() => return,
            sent = give(&mut send, Bytes::copy_from_slice(&buf[..n]), fin) => sent,
        };
        if let Err(e) = sent {
            return ended.set_once(End::Failed(format!(
                "the stream of the HTTP road's one-POST form failed: {}",
                pod_core::text::plain(&e.to_string())
            )));
        }
        if fin {
            return ended.wait().await;
        }
    }
}

/// `data` as DATA, each part once the role's window has room for it, then
/// `END_STREAM` when `fin`.
async fn give(send: &mut SendStream<Bytes>, mut data: Bytes, fin: bool) -> Result<(), h2::Error> {
    while !data.is_empty() {
        send.reserve_capacity(data.len());
        let room = std::future::poll_fn(|cx| send.poll_capacity(cx)).await;
        let room = room.ok_or_else(|| h2::Error::from(Reason::STREAM_CLOSED))??;
        if room > 0 {
            send.send_data(data.split_to(room.min(data.len())), false)?;
        }
    }
    if fin {
        send.send_data(Bytes::new(), true)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pod_ws::headers::Zeroizing;

    /// The POST carries the token, then the caller's headers, each never
    /// indexed by HPACK.
    #[test]
    fn the_post_carries_the_caller_s_headers_unindexed() {
        let dial: super::super::conn::Dial =
            Arc::new(|| Box::pin(async { Err(pod_ws::ConnectError::Config("no dial".into())) }));
        let road = Road {
            dial,
            dial_alpn: None,
            authority: "relay.test".into(),
            user_agent: "pod-relay/test".into(),
            forms: None,
            learned: Arc::default(),
            extra: Ok(Extra::default()),
        };
        let made_up = |v: &str| Zeroizing::new(v.to_string());
        let extra =
            Extra::checked(vec![("CF-Access-Client-Id", made_up("id")), ("CF-Access-Client-Secret", made_up("s"))]);
        let request = request(&road, "/v1/http/one/t/22", "tok", &extra.unwrap()).unwrap();
        let headers = request.headers();
        for (name, value) in [("x-relay-token", "tok"), ("cf-access-client-id", "id"), ("cf-access-client-secret", "s")]
        {
            assert_eq!(headers[name], value);
            assert!(headers[name].is_sensitive(), "{name}");
        }
    }
}

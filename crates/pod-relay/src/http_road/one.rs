//! The one-POST form (podssh T-622): one POST on one connection, whose body
//! in chunks is the way up and whose answer in chunks is the way down. The
//! client sends the request's head, then waits [`MARK_WAIT`] for the
//! answer's [`MARK`]: a hop that holds a request's body to its end holds the
//! request, and so the mark, and the client then takes the split form
//! ([`super::client::open`]). Each write of the caller goes up as a chunk;
//! the caller's end is the last chunk, which the role makes TCP's half-close
//! to the target. The answer's last chunk is the target's end; an answer
//! that ends with no last chunk is a cut, never a clean end. The drop of the
//! connection ends the session at the role, so the form needs no `close`.
//!
//! Where the road's dial offers ALPN ([`super::client::Road::dial_alpn`])
//! and the host chooses `h2`, the form is one stream of HTTP/2
//! (`one_h2.rs`): its DATA up is the body, its DATA down the answer,
//! the request's `END_STREAM` the caller's end and the answer's the
//! target's; a reset of the stream is a cut. A role that asks for HTTP/1.1
//! on that stream (`HTTP_1_1_REQUIRED`) gets the form again on a connection
//! of HTTP/1.1.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_ws::ConnectError;
use tokio::io::{AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf};
use tokio::sync::oneshot;

use super::answer::{self, Body, Framing};
use super::client::{End, Ended, LIMIT, Opened, Road, gather};
use super::conn::{BoxIo, H2, request};
use super::{Form, MARK, one_h2};

/// How long the mark may take after the request's head went. The role
/// sends it before it reads the body, so only a hop that holds the request
/// keeps it longer.
pub const MARK_WAIT: Duration = Duration::from_secs(2);
/// The most bytes of the caller in one chunk up.
pub(super) const CHUNK: usize = 64 * 1024;
/// The pipe in memory, each way.
pub(super) const PIPE: usize = 256 * 1024;
/// The longest body of a refusal that is read.
pub(super) const REFUSAL_MAX: usize = 16 * 1024;
/// The body's last chunk, with no trailer.
const LAST: &[u8] = b"0\r\n\r\n";

/// The hosts whose way did not carry the form in a run, each with why: the
/// client of the run takes the split form there at once.
#[derive(Debug, Default)]
pub struct Learned(Mutex<HashMap<String, String>>);

impl Learned {
    /// Why the form did not go to `authority`, once it failed there.
    pub fn held(&self, authority: &str) -> Option<String> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).get(authority).cloned()
    }

    /// Keep that the form did not go to `authority`, and why.
    pub fn keep(&self, authority: &str, why: &str) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).insert(authority.to_string(), why.to_string());
    }
}

/// Why the form did not open.
pub(super) enum Failed {
    /// The dial's error or the role's refusal: another form gets the same.
    Stop(ConnectError),
    /// The way or the host does not carry the form; another form may go.
    NotCarried(String),
}

/// What the answer's head and its first bytes said.
enum Heard {
    Mark,
    Refused { status: u16, body: String },
    NotCarried(String),
}

/// The statuses of a role or a hop that does not serve the form, which the
/// split form may pass.
pub(super) fn not_served(status: u16) -> bool {
    matches!(status, 400 | 404 | 405 | 411 | 413 | 501 | 505)
}

/// The words of a dial that took too long.
fn dial_late() -> Failed {
    Failed::Stop(ConnectError::Timeout { step: "the connection to the relay host", after: LIMIT })
}

/// The words of a refusal whose body is `body`, as the open of the split
/// form says them: the body on one line, else the reason of the status.
pub(super) fn words(body: &[u8], reason: &str) -> String {
    let words: String = pod_core::text::one_line(&String::from_utf8_lossy(body)).chars().take(200).collect();
    if words.is_empty() { reason.to_string() } else { words }
}

/// A refusal of the form: one that the split form may pass, or the role's
/// refusal, which another form gets too.
pub(super) fn refused(status: u16, body: String) -> Failed {
    if not_served(status) {
        Failed::NotCarried(format!("HTTP {status} {body}"))
    } else {
        Failed::Stop(ConnectError::Refused { status, body })
    }
}

/// No mark came within [`MARK_WAIT`].
pub(super) fn no_mark() -> Failed {
    let secs = MARK_WAIT.as_secs();
    Failed::NotCarried(format!(
        "no mark of the relay in {secs} s: a hop on the way holds the request's body to its end"
    ))
}

/// Open the form at `path` on `road` with `token`: the session, once the
/// mark came within [`MARK_WAIT`]. One stream of HTTP/2 where the road's
/// dial offers ALPN and the host chooses `h2`; else HTTP/1.1.
pub(super) async fn open(road: &Road, path: &str, token: &str) -> Result<Opened, Failed> {
    if let Some(dial) = &road.dial_alpn {
        let dialed = tokio::time::timeout(LIMIT, dial()).await.map_err(|_| dial_late())?.map_err(Failed::Stop)?;
        if dialed.alpn.as_deref() != Some(H2) {
            return on_http1(road, path, token, dialed.io).await;
        }
        // `None`: the role asks for HTTP/1.1 on this stream.
        if let Some(opened) = one_h2::open(road, path, token, dialed.io).await {
            return opened;
        }
    }
    let dialed = tokio::time::timeout(LIMIT, (road.dial)()).await;
    on_http1(road, path, token, dialed.map_err(|_| dial_late())?.map_err(Failed::Stop)?).await
}

/// The form on `io`, a connection of HTTP/1.1.
async fn on_http1(road: &Road, path: &str, token: &str, mut io: BoxIo) -> Result<Opened, Failed> {
    let headers = [("X-Relay-Token", token), ("Transfer-Encoding", "chunked")];
    let head = request("POST", &road.user_agent, &road.authority, path, &headers, None);
    let sent = tokio::time::timeout(LIMIT, async {
        io.write_all(&head).await?;
        io.flush().await
    })
    .await;
    if !matches!(sent, Ok(Ok(()))) {
        return Err(Failed::NotCarried("the request's head did not go".into()));
    }
    let (read, write) = tokio::io::split(io);
    let (app, ours) = tokio::io::duplex(PIPE);
    let (from_app, to_app) = tokio::io::split(ours);
    let ended = Arc::new(Ended::default());
    let (tell, told) = oneshot::channel();
    let down = tokio::spawn(down(read, to_app, tell, ended.clone()));
    let heard = match tokio::time::timeout(MARK_WAIT, told).await {
        Ok(Ok(heard)) => heard,
        Ok(Err(_)) => Heard::NotCarried("the answer ended before its mark".into()),
        Err(_) => {
            down.abort();
            return Err(no_mark());
        }
    };
    match heard {
        Heard::Mark => {
            tokio::spawn(up(write, from_app, ended.clone()));
            Ok(Opened { stream: app, ended, form: Form::One })
        }
        Heard::Refused { status, body } => {
            down.abort();
            Err(refused(status, body))
        }
        Heard::NotCarried(why) => {
            down.abort();
            Err(Failed::NotCarried(why))
        }
    }
}

/// The answer: its head and the mark, which `tell` says, then its bytes to
/// the caller as they come, to its last chunk.
async fn down(
    mut read: ReadHalf<BoxIo>,
    mut to_app: WriteHalf<DuplexStream>,
    tell: oneshot::Sender<Heard>,
    ended: Arc<Ended>,
) {
    let mut buf = Vec::new();
    let head = match answer::read_head(&mut read, &mut buf).await {
        Ok(head) => head,
        Err(why) => {
            let _ = tell.send(Heard::NotCarried(why));
            return;
        }
    };
    let mut body = Body::new(&mut read, &mut buf, head.framing);
    if head.status != 200 {
        let read = tokio::time::timeout(LIMIT, body.all(REFUSAL_MAX)).await;
        let body = words(&read.ok().and_then(Result::ok).unwrap_or_default(), &head.reason);
        let _ = tell.send(Heard::Refused { status: head.status, body });
        return;
    }
    if head.framing != Framing::Chunked {
        let _ = tell.send(Heard::NotCarried("an answer that is not in chunks".into()));
        return;
    }
    let mut first = Vec::new();
    while first.len() < MARK.len() {
        match body.next().await {
            Ok(Some(bytes)) => first.extend_from_slice(&bytes),
            Ok(None) | Err(_) => {
                let _ = tell.send(Heard::NotCarried("the answer ended before its mark".into()));
                return;
            }
        }
    }
    if !first.starts_with(MARK) {
        let _ = tell.send(Heard::NotCarried("the answer's first bytes are not the form's mark".into()));
        return;
    }
    if tell.send(Heard::Mark).is_err() {
        return;
    }
    let end = pump(&mut body, &mut to_app, first.split_off(MARK.len())).await;
    ended.set_once(end);
    let _ = to_app.shutdown().await;
}

/// The answer's bytes to the caller, `first` first: its last chunk is the
/// target's end, and an answer that ends with none is a cut.
async fn pump(body: &mut Body<'_, ReadHalf<BoxIo>>, to_app: &mut WriteHalf<DuplexStream>, first: Vec<u8>) -> End {
    let mut bytes = first;
    loop {
        if !bytes.is_empty() && to_app.write_all(&bytes).await.is_err() {
            return End::Failed("the caller of the HTTP road's one-POST form is gone".into());
        }
        bytes = match body.next().await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return End::Closed { code: 1000, reason: "target closed".into() },
            Err(why) => {
                return End::Failed(format!("the relay cut the answer of the HTTP road's one-POST form: {why}"));
            }
        };
    }
}

/// The caller's bytes up, each read a chunk, until its end, which goes as
/// the last chunk; then the connection stays for the bytes down until the
/// session's end.
async fn up(mut to: WriteHalf<BoxIo>, mut from_app: ReadHalf<DuplexStream>, ended: Arc<Ended>) {
    let mut buf = vec![0u8; CHUNK];
    loop {
        let (n, fin) = tokio::select! {
            biased;
            () = ended.wait() => return,
            got = gather(&mut from_app, &mut buf) => got,
        };
        let mut wire = Vec::with_capacity(n + 16);
        if n > 0 {
            wire.extend_from_slice(&chunk(&buf[..n]));
        }
        if fin {
            wire.extend_from_slice(LAST);
        }
        let wrote = tokio::select! {
            biased;
            () = ended.wait() => return,
            wrote = async {
                to.write_all(&wire).await?;
                to.flush().await
            } => wrote,
        };
        if let Err(e) = wrote {
            return ended.set_once(End::Failed(format!("the connection of the HTTP road's one-POST form failed: {e}")));
        }
        if fin {
            return ended.wait().await;
        }
    }
}

/// One chunk of a body in chunks: its size in hex, its bytes, CRLF.
fn chunk(bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("{:x}\r\n", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chunk_names_its_size_in_hex_and_the_last_has_none() {
        assert_eq!(chunk(MARK), b"12\r\npodssh-http-one 1\n\r\n");
        assert_eq!(chunk(&[7u8; 255])[..4], *b"ff\r\n");
        assert_eq!(LAST, b"0\r\n\r\n");
    }

    #[test]
    fn a_host_learned_once_stays_learned_and_another_is_not() {
        let learned = Learned::default();
        assert_eq!(learned.held("relay.test"), None);
        learned.keep("relay.test", "no mark");
        assert_eq!(learned.held("relay.test").as_deref(), Some("no mark"));
        assert_eq!(learned.held("other.test"), None);
        assert!(not_served(404) && not_served(411) && !not_served(403) && !not_served(503));
    }
}

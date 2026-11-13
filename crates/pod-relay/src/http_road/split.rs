//! The split form (podssh Q44): the caller's bytes up in numbered batches, one POST
//! after another on the session's first connection, and the target's bytes
//! down, each answer from the offset that came, on its second connection.
//! Each request goes again, as it was, until an answer comes.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf};

use super::answer::Head;
use super::client::{End, LIMIT, Retry, Shared, Then, closed, gather, refused};
use super::conn::Conn;
use super::{OFFSET, SEQ};

/// How much longer than the wait that it asks an answer down may take.
const SLACK: Duration = Duration::from_secs(15);

/// The caller's bytes up, until its stream ends or the session does.
pub(super) async fn up(mut conn: Conn, s: Arc<Shared>, mut from_app: ReadHalf<DuplexStream>) {
    let mut buf = vec![0u8; s.batch];
    let (mut seq, mut sent) = (0u64, 0u64);
    loop {
        let (n, fin) = gather(&mut from_app, &mut buf).await;
        sent += n as u64;
        // The last batch carries the count of the bytes up (invariant 9).
        let query = if fin { format!("seq={seq}&fin=1&total={sent}") } else { format!("seq={seq}") };
        if let Err(end) = batch(&mut conn, &s, seq, &s.path("up", &query), &buf[..n]).await {
            return s.end(end);
        }
        seq += 1;
        if fin || s.ended.get().is_some() {
            return;
        }
    }
}

/// The batch `seq` up, sent again, the same, until the role acknowledged
/// it: the acknowledgement names the batch (invariant 6).
pub(super) async fn batch(conn: &mut Conn, s: &Shared, seq: u64, path: &str, body: &[u8]) -> Result<(), End> {
    let head = s.request("POST", path, Some(body.len()));
    let mut tries = Retry::start();
    loop {
        let answer = match conn.ask(&head, body, LIMIT).await {
            Ok(answer) => answer,
            Err(e) => {
                tries.again(&e.to_string(), None).await?;
                continue;
            }
        };
        let words = conn.drop_body(&answer, LIMIT).await;
        s.heard(&answer);
        match answer.status {
            200 | 204 => return acknowledged(&answer, seq),
            410 => return Err(closed(&answer, s)),
            // The role's window is full, or a hop's own trouble: the same
            // batch again.
            _ => match refused(&answer, &words, "a batch") {
                Then::Again(why, at_least) => tries.again(&why, at_least).await?,
                Then::Stop(end) => return Err(end),
            },
        }
    }
}

/// Whether the answer to the batch `seq` acknowledges that batch.
pub(super) fn acknowledged(answer: &Head, seq: u64) -> Result<(), End> {
    match answer.number(SEQ) {
        Some(acked) if acked == seq => Ok(()),
        Some(acked) => Err(End::Failed(format!("the relay acknowledged the batch {acked}, where {seq} went"))),
        None => Err(End::Failed(format!("the relay's answer to the batch {seq} names no batch"))),
    }
}

/// The target's bytes down to the caller, until the session ends or the
/// caller is gone. An end that the bytes up found stops them too, so that
/// the caller's stream ends.
pub(super) async fn down(conn: Conn, s: Arc<Shared>, mut to_app: WriteHalf<DuplexStream>) {
    let ended = s.ended.clone();
    tokio::select! {
        biased;
        () = ended.wait() => {}
        pumped = pump(conn, &s, &mut to_app) => match pumped {
            Some(end) => s.end(end),
            None => return,
        },
    }
    let _ = to_app.shutdown().await;
}

/// The requests down, until the session's end; `None` once the caller is
/// gone.
async fn pump(mut conn: Conn, s: &Shared, to_app: &mut WriteHalf<DuplexStream>) -> Option<End> {
    let mut at: u64 = 0;
    let mut tries = Retry::start();
    let limit = s.wait + SLACK;
    let wait_ms = s.wait.as_millis();
    let end = loop {
        let head = s.request("GET", &s.path("down", &format!("at={at}&wait={wait_ms}")), None);
        let answer = match conn.ask(&head, &[], limit).await {
            Ok(answer) => answer,
            Err(e) => match tries.again(&e.to_string(), None).await {
                Ok(()) => continue,
                Err(end) => break end,
            },
        };
        s.heard(&answer);
        match answer.status {
            200 => {
                if answer.number(OFFSET) != Some(at) {
                    conn.after(false);
                    break End::Failed(format!(
                        "the relay sent the bytes of the HTTP road from another offset than {at}"
                    ));
                }
                let came = carried(&mut conn, &answer, to_app, limit).await?;
                at += came;
                s.came_down(at);
                // A body cut before its first byte is no answer.
                if came > 0 {
                    tries.reset();
                } else if let Err(end) = tries.again("an answer down that a hop cut", None).await {
                    break end;
                }
            }
            204 => {
                conn.after(answer.keep);
                tries.reset();
            }
            410 => {
                let _ = conn.drop_body(&answer, LIMIT).await;
                break closed(&answer, s);
            }
            _ => {
                let words = conn.drop_body(&answer, LIMIT).await;
                match refused(&answer, &words, "the bytes down") {
                    Then::Again(why, at_least) => {
                        if let Err(end) = tries.again(&why, at_least).await {
                            break end;
                        }
                    }
                    Then::Stop(end) => break end,
                }
            }
        }
    };
    Some(end)
}

/// The bytes of an answer down, to the caller as they come: how many came,
/// or `None` once the caller is gone. A body that stops for `limit` is cut.
async fn carried(conn: &mut Conn, answer: &Head, to_app: &mut WriteHalf<DuplexStream>, limit: Duration) -> Option<u64> {
    let mut came = 0u64;
    let mut whole = false;
    if let Some(mut body) = conn.body(answer) {
        loop {
            match tokio::time::timeout(limit, body.next()).await {
                Ok(Ok(Some(bytes))) => {
                    if to_app.write_all(&bytes).await.is_err() {
                        return None;
                    }
                    came += bytes.len() as u64;
                }
                Ok(Ok(None)) => {
                    whole = body.finished();
                    break;
                }
                Ok(Err(_)) | Err(_) => break,
            }
        }
    }
    conn.after(answer.keep && whole);
    Some(came)
}

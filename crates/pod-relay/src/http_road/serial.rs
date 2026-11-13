//! The serial form (podssh Q44), the split form's fallback: one exchange after
//! another on one connection, each a POST that carries a batch up when one
//! waits, answered whole with the target's bytes from the offset that it
//! names, as meek carries Tor's bytes. With nothing either way, the pause
//! before the next exchange grows from 50 ms to the tuning's longest; a byte
//! either way makes it none.
//!
//! A task of its own writes the bytes down to the caller, from a channel of
//! [`HELD`] answers. A caller that reads no more until its own bytes up went,
//! as SSH's client does with its window adjustments, then still gets its
//! bytes up through: while the channel is full, no exchange goes, and each
//! batch up goes alone (`up`), with no bytes down.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, WriteHalf};
use tokio::sync::mpsc::{self, error::TrySendError};

use super::OFFSET;
use super::client::{End, LIMIT, Retry, Shared, Then, closed, gather_more, refused};
use super::conn::Conn;
use super::split::{acknowledged, batch};

/// The first pause after an exchange that carried nothing.
const PAUSE_FIRST: Duration = Duration::from_millis(50);
/// The answers down that wait for the caller, each `down_max` bytes at most:
/// 4 MiB at the role's largest bound.
const HELD: usize = 4;

/// The session's exchanges, until it ends or the caller is gone.
pub(super) async fn run(mut conn: Conn, s: Arc<Shared>, ours: DuplexStream) {
    let (mut from_app, to_app) = tokio::io::split(ours);
    let (down, downs) = mpsc::channel(HELD);
    let writer = tokio::spawn(write_down(downs, to_app));
    let mut buf = vec![0u8; s.batch];
    let (mut seq, mut at, mut sent) = (0u64, 0u64, 0u64);
    // A batch in `buf` that the role has not stored: its length, and whether
    // it is the last.
    let mut waiting: Option<(usize, bool)> = None;
    let mut open = true;
    let mut pause = Duration::ZERO;
    let mut tries = Retry::start();
    let end = loop {
        // Room for one more answer down, kept through the exchange.
        let room = match down.try_reserve() {
            Ok(permit) => Some(permit),
            Err(TrySendError::Full(())) => None,
            Err(TrySendError::Closed(())) => return,
        };
        if waiting.is_none() {
            // The caller's bytes; else the pause, or room in the channel.
            tokio::select! {
                biased;
                read = from_app.read(&mut buf), if open => {
                    let (n, fin) = match read {
                        Ok(0) | Err(_) => (0, true),
                        Ok(n) => gather_more(&mut from_app, &mut buf, n).await,
                    };
                    open = !fin;
                    waiting = Some((n, fin));
                    sent += n as u64;
                }
                () = tokio::time::sleep(pause), if room.is_some() => {}
                freed = down.reserve(), if room.is_none() => {
                    if freed.is_err() {
                        return;
                    }
                    continue;
                }
            }
        }
        let Some(permit) = room else {
            // The caller has not read the bytes down: the batch goes up
            // alone, so that its next bytes up find room.
            let Some((n, fin)) = waiting else { continue };
            // The last batch carries the count of the bytes up (invariant 9).
            let query = if fin { format!("seq={seq}&fin=1&total={sent}") } else { format!("seq={seq}") };
            if let Err(end) = batch(&mut conn, &s, seq, &s.path("up", &query), &buf[..n]).await {
                break end;
            }
            (waiting, seq, pause) = (None, seq + 1, Duration::ZERO);
            tries.reset();
            continue;
        };
        let query = match waiting {
            // The last batch carries the count of the bytes up (invariant 9).
            Some((_, true)) => format!("seq={seq}&at={at}&wait=0&fin=1&total={sent}"),
            Some((_, false)) => format!("seq={seq}&at={at}&wait=0"),
            None => format!("at={at}&wait=0"),
        };
        let length = waiting.map_or(0, |(n, _)| n);
        let head = s.request("POST", &s.path("x", &query), Some(length));
        let answer = match conn.ask(&head, &buf[..length], LIMIT).await {
            Ok(answer) => answer,
            Err(e) => match tries.again(&e.to_string(), None).await {
                Ok(()) => continue,
                Err(end) => break end,
            },
        };
        s.heard(&answer);
        match answer.status {
            200 | 204 => {
                if answer.status == 200 && answer.number(OFFSET) != Some(at) {
                    conn.after(false);
                    break End::Failed(format!(
                        "the relay sent the bytes of the HTTP road from another offset than {at}"
                    ));
                }
                // The answer's head says that the role stored the batch, if
                // one went; the bytes of a body cut on the way come again
                // from the same offset.
                let took = waiting.take().is_some();
                if took {
                    if let Err(end) = acknowledged(&answer, seq) {
                        conn.after(false);
                        break end;
                    }
                    seq += 1;
                }
                let came = match answer.status {
                    200 => match conn.whole(&answer, s.down_max, LIMIT).await {
                        Ok(bytes) => Some(bytes),
                        Err(e) => {
                            if let Err(end) = tries.again(&e.to_string(), None).await {
                                break end;
                            }
                            None
                        }
                    },
                    _ => {
                        conn.after(answer.keep);
                        Some(Vec::new())
                    }
                };
                let Some(came) = came else { continue };
                let any = !came.is_empty();
                if any {
                    at += came.len() as u64;
                    permit.send(came);
                    s.came_down(at);
                }
                tries.reset();
                pause = if took || any { Duration::ZERO } else { next_pause(pause, s.poll_max) };
            }
            410 => {
                let _ = conn.drop_body(&answer, LIMIT).await;
                break closed(&answer, &s);
            }
            _ => {
                let words = conn.drop_body(&answer, LIMIT).await;
                match refused(&answer, &words, "an exchange") {
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
    s.end(end);
    // The caller reads the end after the last byte that the channel holds.
    drop(down);
    let _ = writer.await;
}

/// The answers down to the caller, in their order, then the end of its
/// stream; it stops once the caller is gone.
async fn write_down(mut downs: mpsc::Receiver<Vec<u8>>, mut to_app: WriteHalf<DuplexStream>) {
    while let Some(bytes) = downs.recv().await {
        if to_app.write_all(&bytes).await.is_err() {
            return;
        }
    }
    let _ = to_app.shutdown().await;
}

/// The pause after `now`, in an exchange that carried nothing: 50 ms, then
/// half as long again each time, up to `most`.
fn next_pause(now: Duration, most: Duration) -> Duration {
    if now.is_zero() { PAUSE_FIRST } else { (now * 3 / 2).min(most).max(PAUSE_FIRST) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pause_grows_from_50_ms_to_its_longest() {
        let most = Duration::from_secs(1);
        let mut pauses = vec![next_pause(Duration::ZERO, most)];
        for _ in 0..10 {
            pauses.push(next_pause(*pauses.last().unwrap(), most));
        }
        let ms: Vec<u128> = pauses.iter().map(Duration::as_millis).collect();
        assert_eq!(&ms[..4], &[50, 75, 112, 168]);
        assert_eq!(*ms.last().unwrap(), 1000);
    }
}

//! The protocol of the IMAP retrieval carrier: the greeting, `CAPABILITY`,
//! STARTTLS, `LOGIN`, `SELECT`, `APPEND` and `UID FETCH`. The carrier and its
//! settings are [`super`].

use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::ClientConfig;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::time::timeout;

use crate::carrier::{CarrierError, CarrierErrorKind, Limit};
use crate::mail;
use crate::pipes::Far;

use super::{Endpoint, Imap, MIN_BATCH, Mode, NAME, PENDING, PIPE, err};

/// Run one command of the carrier within [`Imap::command_timeout`]. A
/// server that accepts the command and never answers is the named timeout
/// error, not a task that waits forever.
async fn bounded<T>(
    imap: &Imap,
    what: &str,
    work: impl Future<Output = Result<T, CarrierError>>,
) -> Result<T, CarrierError> {
    match timeout(imap.command_timeout, work).await {
        Ok(result) => result,
        Err(_) => Err(err(
            CarrierErrorKind::Io(io::ErrorKind::TimedOut),
            format!("the {what} took more than {:?}", imap.command_timeout),
        )),
    }
}

/// mailbox for the bytes down.
/// The session: `LOGIN`, the appends of the bytes up, a poll for the bytes
/// down, and `LOGOUT` at the end of the stream.
pub(super) async fn run(
    imap: Imap,
    endpoint: Endpoint,
    mode: Mode,
    config: Arc<ClientConfig>,
    mut far: Far,
) -> Result<(), CarrierError> {
    let (io, name) = mail::open(NAME, &endpoint, imap.connect_timeout).await?;
    let lines = if mode == Mode::Implicit {
        mail::wrap(NAME, io, &name, config.clone(), imap.handshake_timeout).await?
    } else {
        tokio::io::BufReader::new(io)
    };
    let mut tag = 0u32;
    let (mut lines, caps) = timeout(imap.handshake_timeout, handshake(&imap, lines, mode, config, &name, &mut tag))
        .await
        .map_err(|_| err(CarrierErrorKind::Io(io::ErrorKind::TimedOut), "the handshake took too long"))??;
    let mut payload = imap.batch;
    if let Some(bytes) = append_limit(&caps) {
        mail::report(&imap.limits, Limit::declared(bytes));
        payload = payload_at_most(payload, bytes);
        if payload == 0 {
            return Err(err(CarrierErrorKind::Usage, "the APPENDLIMIT of the server takes no message"));
        }
    }
    let mut last = 0u32;
    let mut batch: Vec<u8> = Vec::with_capacity(payload + PIPE);
    let mut pending: Vec<u8> = Vec::new();
    let mut written = 0usize;
    let mut chunk = vec![0u8; PIPE];
    let mut next = Instant::now();
    loop {
        let wait = next.saturating_duration_since(Instant::now());
        let read = tokio::select! {
            read = far.to_peer.read(&mut chunk) => {
                read.map_err(|e| CarrierError::io(NAME, "the stream", e))?
            }
            () = tokio::time::sleep(wait) => {
                if !batch.is_empty() {
                    append(&imap, &mut lines, &mut tag, &mut batch, &mut payload).await?;
                }
                if pending.len() - written < PENDING {
                    let poll = fetch(&imap, &mut lines, &mut tag, &mut last, &mut pending);
                    bounded(&imap, "UID FETCH", poll).await?;
                }
                // The link reads at its own pace: a full pipe waits for the
                // next turn, so no append waits on it.
                written = drain(&mut far, &pending, written).await?;
                if written == pending.len() {
                    pending.clear();
                    written = 0;
                }
                next = Instant::now() + imap.poll;
                continue;
            }
        };
        batch.extend_from_slice(&chunk[..read]);
        while batch.len() >= payload {
            append(&imap, &mut lines, &mut tag, &mut batch, &mut payload).await?;
        }
        if read == 0 {
            break;
        }
    }
    while !batch.is_empty() {
        append(&imap, &mut lines, &mut tag, &mut batch, &mut payload).await?;
    }
    let _ = bounded(&imap, "LOGOUT", async {
        mail::write(&mut lines, NAME, format!("{} LOGOUT\r\n", tag_of(&mut tag)).as_bytes()).await
    })
    .await;
    Ok(())
}

/// Give the fetched bytes to the link, within a short bound: a pipe that
/// the link does not read now keeps them for its next turn.
async fn drain(far: &mut Far, pending: &[u8], mut written: usize) -> Result<usize, CarrierError> {
    while written < pending.len() {
        let sent = timeout(Duration::from_millis(50), far.from_peer.write(&pending[written..])).await;
        match sent {
            Ok(Ok(n)) => written += n,
            Ok(Err(e)) => return Err(CarrierError::io(NAME, "the stream", e)),
            Err(_) => break,
        }
    }
    Ok(written)
}

/// The greeting, `CAPABILITY`, STARTTLS when the mode says so, `LOGIN`, and
/// `SELECT` of `inbox`. It gives the capabilities of the server.
async fn handshake(
    imap: &Imap,
    mut lines: mail::Lines,
    mode: Mode,
    config: Arc<ClientConfig>,
    name: &str,
    tag: &mut u32,
) -> Result<(mail::Lines, Vec<String>), CarrierError> {
    let greeting = mail::next_line(&mut lines, NAME).await?;
    if !greeting.starts_with(b"* OK") && !greeting.starts_with(b"* PREAUTH") {
        return Err(err(CarrierErrorKind::Refused, format!("the server greets with {:?}", mail::text(&greeting))));
    }
    let mut caps = capability(imap, &mut lines, tag).await?;
    if mode == Mode::Starttls {
        if let Some(why) = command(imap, &mut lines, tag, "STARTTLS").await? {
            return Err(err(CarrierErrorKind::Refused, format!("STARTTLS: {why}")));
        }
        let io = lines.into_inner();
        lines = mail::wrap(NAME, io, name, config, imap.handshake_timeout).await?;
        caps = capability(imap, &mut lines, tag).await?;
    }
    let login = format!("LOGIN {} {}", quoted(imap.credential.user()), quoted(imap.credential.secret()));
    if let Some(why) = command(imap, &mut lines, tag, &login).await? {
        return Err(err(CarrierErrorKind::Refused, format!("LOGIN: {why}")));
    }
    if !imap.inbox.is_empty() {
        let select = format!("SELECT {}", quoted(&imap.inbox));
        if let Some(why) = command(imap, &mut lines, tag, &select).await? {
            return Err(err(CarrierErrorKind::Refused, format!("SELECT {}: {why}", imap.inbox)));
        }
    }
    Ok((lines, caps))
}

/// `APPEND` the bytes that wait; a refusal halves the payload and sends
/// them again, and no byte is appended twice (MAIL-4).
async fn append(
    imap: &Imap,
    lines: &mut mail::Lines,
    tag: &mut u32,
    batch: &mut Vec<u8>,
    payload: &mut usize,
) -> Result<(), CarrierError> {
    for _ in 0..8 {
        let take = batch.len().min((*payload).max(1));
        if take == 0 {
            return Ok(());
        }
        let tag_text = tag_of(tag);
        let head = format!("APPEND {} (\\Seen) {{{take}}}\r\n", quoted(&imap.outbox));
        let refusal = bounded(imap, "APPEND", async {
            mail::write(lines, NAME, format!("{tag_text} {head}").as_bytes()).await?;
            let first = mail::next_line(lines, NAME).await?;
            if first.starts_with(b"+") {
                mail::write(lines, NAME, &batch[..take]).await?;
                mail::write(lines, NAME, b"\r\n").await?;
                wait_tagged(lines, &tag_text).await
            } else {
                match tagged_of(&first, &tag_text) {
                    Some(refusal) => Ok(refusal),
                    None => Err(err(
                        CarrierErrorKind::Io(io::ErrorKind::InvalidData),
                        format!("an APPEND answered with {:?}", mail::text(&first)),
                    )),
                }
            }
        })
        .await?;
        match refusal {
            None => {
                batch.drain(..take);
                return Ok(());
            }
            Some(why) => {
                mail::report(&imap.limits, Limit::refused(refused_size(&why)));
                if take <= MIN_BATCH {
                    return Err(err(
                        CarrierErrorKind::Refused,
                        format!("the server refuses an APPEND of {take} bytes, at the smallest batch"),
                    ));
                }
                *payload = (take / 2).max(MIN_BATCH);
            }
        }
    }
    Err(err(CarrierErrorKind::Refused, "the server refuses each size of an APPEND"))
}

/// A poll of the mailbox: each message after `last` that the carrier has
/// not read goes into `out`, and `last` moves with it. A body over
/// [`Imap::max_fetch`] is read and dropped without being allocated whole,
/// and `last` does not move past it.
async fn fetch(
    imap: &Imap,
    lines: &mut mail::Lines,
    tag: &mut u32,
    last: &mut u32,
    out: &mut Vec<u8>,
) -> Result<(), CarrierError> {
    let tag_text = tag_of(tag);
    let command = format!("UID FETCH {}:* (UID BODY.PEEK[])", last.saturating_add(1));
    mail::write(lines, NAME, format!("{tag_text} {command}\r\n").as_bytes()).await?;
    loop {
        let line = mail::next_line(lines, NAME).await?;
        if line.is_empty() {
            return Err(err(CarrierErrorKind::Io(io::ErrorKind::UnexpectedEof), "the server closed the connection"));
        }
        if let Some(refusal) = tagged_of(&line, &tag_text) {
            return match refusal {
                None => Ok(()),
                Some(why) => Err(err(CarrierErrorKind::Refused, format!("UID FETCH: {why}"))),
            };
        }
        let Some(uid) = fetch_uid(&line) else { continue };
        let Some(length) = literal(&line) else { continue };
        // The length is the server's word: compare it with the limit
        // before the allocation, and drop an over-limit literal without
        // keeping it.
        if length > imap.max_fetch {
            mail::report(&imap.limits, Limit::truncated(length as u64));
            discard(lines, length).await?;
            continue;
        }
        let mut raw = vec![0u8; length];
        lines.read_exact(&mut raw).await.map_err(|e| CarrierError::io(NAME, "a fetched body", e))?;
        if uid > *last {
            // The message of the submission carrier carries the payload in
            // base64; a message of another tool is taken as it is.
            out.extend_from_slice(&mail::body(&raw));
            *last = uid;
        }
    }
}

/// `CAPABILITY`, in lower case, from the untagged line or from the
/// greeting.
async fn capability(imap: &Imap, lines: &mut mail::Lines, tag: &mut u32) -> Result<Vec<String>, CarrierError> {
    bounded(imap, "CAPABILITY", async {
        let tag_text = tag_of(tag);
        mail::write(lines, NAME, format!("{tag_text} CAPABILITY\r\n").as_bytes()).await?;
        let mut caps = Vec::new();
        loop {
            let line = mail::next_line(lines, NAME).await?;
            if line.is_empty() {
                return Err(err(
                    CarrierErrorKind::Io(io::ErrorKind::UnexpectedEof),
                    "the server closed the connection",
                ));
            }
            if let Some(text) = line.strip_prefix(b"* CAPABILITY ") {
                caps.extend(mail::text(text).split_whitespace().map(str::to_lowercase));
            }
            if let Some(refusal) = tagged_of(&line, &tag_text) {
                return match refusal {
                    None => Ok(caps),
                    Some(why) => Err(err(CarrierErrorKind::Refused, format!("CAPABILITY: {why}"))),
                };
            }
        }
    })
    .await
}

/// One command with its tag: `None` when the server took it, or the text
/// of its refusal.
async fn command(
    imap: &Imap,
    lines: &mut mail::Lines,
    tag: &mut u32,
    text: &str,
) -> Result<Option<String>, CarrierError> {
    let what = text.split_whitespace().next().unwrap_or("command");
    bounded(imap, what, async {
        let tag_text = tag_of(tag);
        mail::write(lines, NAME, format!("{tag_text} {text}\r\n").as_bytes()).await?;
        wait_tagged(lines, &tag_text).await
    })
    .await
}

/// Wait for the tagged answer of `tag`: `None` for `OK`, or the text of
/// the refusal.
async fn wait_tagged(lines: &mut mail::Lines, tag: &str) -> Result<Option<String>, CarrierError> {
    for _ in 0..1024 {
        let line = mail::next_line(lines, NAME).await?;
        if line.is_empty() {
            return Err(err(CarrierErrorKind::Io(io::ErrorKind::UnexpectedEof), "the server closed the connection"));
        }
        if let Some(refusal) = tagged_of(&line, tag) {
            return Ok(refusal);
        }
    }
    Err(err(CarrierErrorKind::Io(io::ErrorKind::InvalidData), "a reply of more than 1024 lines"))
}

/// The next tag, `a1`, `a2`, and so on.
fn tag_of(tag: &mut u32) -> String {
    *tag += 1;
    format!("a{tag}")
}

/// The size that a refusal names, or 0 when it names none.
fn refused_size(text: &str) -> Option<u64> {
    text.split_whitespace().find_map(|word| word.trim_start_matches("[TOOBIG").trim_end_matches(']').parse().ok())
}

/// The answer that a tagged line of `tag` carries: `Some(None)` for an
/// `OK`, `Some(Some(text))` for a `NO` or a `BAD`, and `None` for another
/// line.
fn tagged_of(line: &[u8], tag: &str) -> Option<Option<String>> {
    let text = mail::text(line);
    let rest = text.strip_prefix(tag)?.strip_prefix(' ')?;
    if rest.starts_with("OK") {
        return Some(None);
    }
    if !rest.starts_with("NO") && !rest.starts_with("BAD") {
        return None;
    }
    Some(Some(text))
}

/// Read and drop `length` bytes of a literal without keeping them.
async fn discard(lines: &mut mail::Lines, length: usize) -> Result<(), CarrierError> {
    let mut buf = vec![0u8; PIPE.min(length.max(1))];
    let mut left = length;
    while left > 0 {
        let take = left.min(buf.len());
        lines.read_exact(&mut buf[..take]).await.map_err(|e| CarrierError::io(NAME, "a fetched body", e))?;
        left -= take;
    }
    Ok(())
}

/// The uid of a fetched message, from `* 1 FETCH (UID 5 BODY[] {300}`.
fn fetch_uid(line: &[u8]) -> Option<u32> {
    let text = mail::text(line);
    let mut words = text.split_whitespace();
    while let Some(word) = words.next() {
        if word.trim_start_matches('(').eq_ignore_ascii_case("UID") {
            return words.next()?.trim_end_matches(')').parse().ok();
        }
    }
    None
}

/// The length of the literal at the end of a line, `{300}`.
fn literal(line: &[u8]) -> Option<usize> {
    let text = mail::text(line);
    let end = text.rfind('}')?;
    let start = text[..end].rfind('{')?;
    text[start + 1..end].parse().ok()
}

/// The `APPENDLIMIT` capability of the server, in bytes.
fn append_limit(caps: &[String]) -> Option<u64> {
    caps.iter().find_map(|cap| cap.strip_prefix("appendlimit=")?.parse().ok())
}

/// The largest payload that a server of the `APPENDLIMIT` `limit` takes, at
/// most `payload`.
fn payload_at_most(payload: usize, limit: u64) -> usize {
    payload.min(limit.min(usize::MAX as u64) as usize)
}

/// `text` as an IMAP quoted string.
fn quoted(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capabilities_and_the_lines_of_a_fetch_are_read() {
        assert_eq!(append_limit(&["imap4rev1".into(), "appendlimit=1024".into()]), Some(1024));
        assert_eq!(append_limit(&["imap4rev1".into()]), None);
        let line = b"* 1 FETCH (UID 5 BODY[] {300}\r\n";
        assert_eq!(fetch_uid(line), Some(5));
        assert_eq!(literal(line), Some(300));
        assert_eq!(fetch_uid(b"* 2 EXISTS\r\n"), None);
        assert_eq!(tagged_of(b"a4 OK SELECT done\r\n", "a4"), Some(None));
        let refused = tagged_of(b"a4 NO [TOOBIG 1000] too large\r\n", "a4").expect("a refusal");
        assert_eq!(refused_size(&refused.expect("a refusal text")), Some(1000));
        assert_eq!(tagged_of(b"* 1 EXISTS\r\n", "a4"), None);
    }

    #[test]
    fn a_quoted_mailbox_escapes_its_quote() {
        assert_eq!(quoted("a\"b"), "\"a\\\"b\"");
    }
}

//! The protocol of the SMTP submission carrier: the greeting, `EHLO`,
//! STARTTLS, `AUTH PLAIN`, and one message for each batch. The carrier and
//! its settings are [`super`].

use std::io;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use base64::Engine as _;
use rustls::ClientConfig;
use tokio::io::AsyncReadExt as _;
use tokio::time::timeout;

use crate::carrier::{CarrierError, CarrierErrorKind, Limit};
use crate::mail;
use crate::pipes::Far;

use super::{Credential, NAME, PIPE, Smtp};

/// The greeting, STARTTLS, AUTH, and then one message for each batch until
/// the stream ends.
pub(super) async fn run(
    smtp: Smtp,
    endpoint: mail::Endpoint,
    mode: mail::Mode,
    config: Arc<ClientConfig>,
    session: String,
    mut far: Far,
) -> Result<(), CarrierError> {
    let bound = smtp.handshake_timeout;
    let (io, name) = mail::open(NAME, &endpoint, smtp.connect_timeout).await?;
    let lines = if mode == mail::Mode::Implicit {
        mail::wrap(NAME, io, &name, config.clone(), bound).await?
    } else {
        tokio::io::BufReader::new(io)
    };
    let (mut lines, caps) = match timeout(bound, handshake(&smtp, lines, mode, config, &name)).await {
        Err(_) => {
            return Err(err(
                CarrierErrorKind::Io(io::ErrorKind::TimedOut),
                format!("the handshake with {} took more than {bound:?}", endpoint.text()),
            ));
        }
        Ok(reply) => reply?,
    };
    let mut payload = smtp.batch;
    if let Some(bytes) = declared_size(&caps) {
        mail::report(&smtp.limits, Limit::declared(bytes));
        payload = payload_at_most(&smtp, &session, payload, bytes);
        if payload == 0 {
            return Err(err(
                CarrierErrorKind::Usage,
                format!("the SIZE of the server leaves no room for a message of this envelope ({bytes} bytes)"),
            ));
        }
    }
    let mut seq = 0u64;
    let mut batch: Vec<u8> = Vec::with_capacity(payload + PIPE);
    loop {
        while batch.len() >= payload.max(1) {
            drain(&smtp, &mut lines, &session, &mut seq, &mut batch, &mut payload).await?;
        }
        let mut chunk = vec![0u8; PIPE.min(payload.max(1))];
        let read = match timeout(smtp.flush_after, far.to_peer.read(&mut chunk)).await {
            Ok(read) => read.map_err(|e| CarrierError::io(NAME, "the stream", e))?,
            Err(_) => {
                // A partial batch goes after a silence, not at the end of
                // the stream.
                if !batch.is_empty() {
                    drain(&smtp, &mut lines, &session, &mut seq, &mut batch, &mut payload).await?;
                }
                continue;
            }
        };
        batch.extend_from_slice(&chunk[..read]);
        if read == 0 {
            break;
        }
    }
    while !batch.is_empty() {
        drain(&smtp, &mut lines, &session, &mut seq, &mut batch, &mut payload).await?;
    }
    let _ = mail::write(&mut lines, NAME, b"QUIT\r\n").await;
    Ok(())
}

/// The greeting, `EHLO`, STARTTLS when the mode says so, `EHLO` again,
/// and `AUTH`. It gives the capabilities of the server after the last
/// `EHLO`.
async fn handshake(
    smtp: &Smtp,
    mut lines: mail::Lines,
    mode: mail::Mode,
    config: Arc<ClientConfig>,
    name: &str,
) -> Result<(mail::Lines, Vec<String>), CarrierError> {
    let (code, text) = reply(&mut lines).await?;
    if code != 220 {
        return Err(err(CarrierErrorKind::Refused, format!("the server greets with {code}: {}", text.join(" "))));
    }
    let mut caps = ehlo(&mut lines).await?;
    if mode == mail::Mode::Starttls {
        mail::write(&mut lines, NAME, b"STARTTLS\r\n").await?;
        let (code, text) = reply(&mut lines).await?;
        if code != 220 {
            return Err(err(CarrierErrorKind::Refused, format!("STARTTLS: {code} {}", text.join(" "))));
        }
        let io = lines.into_inner();
        lines = mail::wrap(NAME, io, name, config, smtp.handshake_timeout).await?;
        caps = ehlo(&mut lines).await?;
    }
    authenticate(&mut lines, &smtp.credential).await?;
    Ok((lines, caps))
}

/// Send one message of the bytes that wait, and remove them from `batch`
/// when the server took them. A refusal is a carrier limit: the carrier
/// reports it, halves the payload and sends the same bytes again, and no
/// byte was accepted twice (MAIL-4).
async fn drain(
    smtp: &Smtp,
    lines: &mut mail::Lines,
    session: &str,
    seq: &mut u64,
    batch: &mut Vec<u8>,
    payload: &mut usize,
) -> Result<(), CarrierError> {
    for _ in 0..8 {
        let take = batch.len().min((*payload).max(1));
        if take == 0 {
            return Ok(());
        }
        let message = message(smtp, session, *seq, &batch[..take]);
        let started = Instant::now();
        let answered = timeout(smtp.submit_timeout, send_message(lines, &smtp.from, &smtp.to, &message)).await;
        let refused = match answered {
            Err(_) => {
                return Err(err(
                    CarrierErrorKind::Io(io::ErrorKind::TimedOut),
                    format!("no answer to a message of {} bytes in {:?}", message.len(), smtp.submit_timeout),
                ));
            }
            Ok(Err(e)) => return Err(e),
            Ok(Ok(answered)) => answered,
        };
        match refused {
            None => {
                smtp.measured.store(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
                batch.drain(..take);
                *seq += 1;
                return Ok(());
            }
            Some(bytes) => {
                mail::report(&smtp.limits, Limit::refused(Some(bytes)));
                if take <= super::MIN_BATCH {
                    return Err(err(
                        CarrierErrorKind::Refused,
                        format!("the server refuses a message of {} bytes, at the smallest batch", message.len()),
                    ));
                }
                *payload = (take / 2).max(super::MIN_BATCH);
            }
        }
    }
    Err(err(CarrierErrorKind::Refused, "the server refuses each size of the message"))
}

/// The envelope, the body and the answer: `None` when the server took the
/// message, or the size that it refused.
async fn send_message(
    lines: &mut mail::Lines,
    from: &str,
    to: &str,
    message: &[u8],
) -> Result<Option<u64>, CarrierError> {
    let mail_from = format!("MAIL FROM:<{from}> SIZE={}\r\n", message.len());
    mail::write(lines, NAME, mail_from.as_bytes()).await?;
    match reply(lines).await? {
        (250, _) => {}
        (552, text) | (554, text) => return Ok(Some(refused_size(&text))),
        (code, text) => {
            return Err(err(CarrierErrorKind::Refused, format!("MAIL FROM: {code} {}", text.join(" "))));
        }
    }
    mail::write(lines, NAME, format!("RCPT TO:<{to}>\r\n").as_bytes()).await?;
    let (code, text) = reply(lines).await?;
    if code != 250 && code != 251 {
        return Err(err(CarrierErrorKind::Refused, format!("RCPT TO: {code} {}", text.join(" "))));
    }
    mail::write(lines, NAME, b"DATA\r\n").await?;
    let (code, text) = reply(lines).await?;
    if code != 354 {
        return Err(err(CarrierErrorKind::Refused, format!("DATA: {code} {}", text.join(" "))));
    }
    mail::write(lines, NAME, message).await?;
    mail::write(lines, NAME, b".\r\n").await?;
    match reply(lines).await? {
        (250, _) => Ok(None),
        (552, text) | (554, text) => Ok(Some(refused_size(&text))),
        (code, text) => Err(err(CarrierErrorKind::Refused, format!("the body: {code} {}", text.join(" ")))),
    }
}

/// The size that a refusal names, or 0 when it names none.
fn refused_size(text: &[String]) -> u64 {
    text.iter().find_map(|line| line.split_whitespace().find_map(|word| word.parse().ok())).unwrap_or(0)
}

/// `EHLO`, and the capabilities of the server in lower case.
async fn ehlo(lines: &mut mail::Lines) -> Result<Vec<String>, CarrierError> {
    mail::write(lines, NAME, format!("EHLO {}\r\n", mail::CLIENT_NAME).as_bytes()).await?;
    let (code, text) = reply(lines).await?;
    if code != 250 {
        return Err(err(CarrierErrorKind::Refused, format!("EHLO: {code} {}", text.join(" "))));
    }
    Ok(text.into_iter().map(|line| line.to_lowercase()).collect())
}

/// `AUTH PLAIN` with the credential of the session (MAIL-3).
async fn authenticate(lines: &mut mail::Lines, credential: &Credential) -> Result<(), CarrierError> {
    let mut plain = Vec::with_capacity(credential.user().len() + credential.secret().len() + 2);
    plain.push(0);
    plain.extend_from_slice(credential.user().as_bytes());
    plain.push(0);
    plain.extend_from_slice(credential.secret().as_bytes());
    let token = base64::engine::general_purpose::STANDARD.encode(&plain);
    mail::write(lines, NAME, format!("AUTH PLAIN {token}\r\n").as_bytes()).await?;
    let (code, text) = reply(lines).await?;
    if code != 235 {
        return Err(err(CarrierErrorKind::Refused, format!("AUTH PLAIN: {code} {}", text.join(" "))));
    }
    Ok(())
}

/// The `SIZE` capability of the server, in bytes.
fn declared_size(caps: &[String]) -> Option<u64> {
    caps.iter().find_map(|line| line.strip_prefix("size")?.trim().parse().ok())
}

/// One message: the envelope headers, and the body in base64.
fn message(smtp: &Smtp, session: &str, seq: u64, body: &[u8]) -> Vec<u8> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(body);
    let mut out = header(smtp, session, seq);
    if encoded.is_empty() {
        out.extend_from_slice(b"\r\n");
        return out;
    }
    for line in encoded.as_bytes().chunks(76) {
        out.extend_from_slice(line);
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// The size of the message of `body` bytes, without its ending dot.
fn message_size(smtp: &Smtp, session: &str, body: usize) -> usize {
    let encoded = body.div_ceil(3) * 4;
    let lines = encoded.max(1).div_ceil(76);
    header(smtp, session, 0).len() + encoded + 2 * lines
}

/// The headers of one message.
fn header(smtp: &Smtp, session: &str, seq: u64) -> Vec<u8> {
    format!(
        "From: {}\r\nTo: {}\r\nSubject: {}\r\nX-Podlink-Batch: {session}/{seq}\r\nMIME-Version: 1.0\r\n\
         Content-Type: application/octet-stream\r\nContent-Transfer-Encoding: base64\r\n\r\n",
        smtp.from, smtp.to, smtp.subject
    )
    .into_bytes()
}

/// The largest payload whose message fits `limit` bytes, at most
/// `payload`.
pub fn payload_at_most(smtp: &Smtp, session: &str, payload: usize, limit: u64) -> usize {
    let (mut low, mut high) = (0usize, payload);
    while low < high {
        let middle = (low + high).div_ceil(2);
        if (message_size(smtp, session, middle) as u64) <= limit {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    low
}

/// One reply of SMTP: its code and its lines. A multi-line reply ends at
/// the line whose fourth byte is a space.
async fn reply(lines: &mut mail::Lines) -> Result<(u16, Vec<String>), CarrierError> {
    let mut code = None;
    let mut text = Vec::new();
    for _ in 0..64 {
        let line = mail::next_line(lines, NAME).await?;
        if line.is_empty() {
            return Err(err(CarrierErrorKind::Io(io::ErrorKind::UnexpectedEof), "the server closed the connection"));
        }
        if line.len() < 4 || !line[..3].iter().all(|b| b.is_ascii_digit()) {
            return Err(err(
                CarrierErrorKind::Io(io::ErrorKind::InvalidData),
                format!("a reply {:?}", mail::text(&line)),
            ));
        }
        let digits = (line[0] - b'0') as u16 * 100 + (line[1] - b'0') as u16 * 10 + (line[2] - b'0') as u16;
        if code.is_some_and(|code| code != digits) {
            return Err(err(CarrierErrorKind::Io(io::ErrorKind::InvalidData), "a reply of two codes"));
        }
        code = Some(digits);
        text.push(mail::text(&line[4..]));
        match line[3] {
            b' ' => return Ok((digits, text)),
            b'-' => {}
            other => {
                return Err(err(
                    CarrierErrorKind::Io(io::ErrorKind::InvalidData),
                    format!("a reply with the byte {other} after its code"),
                ));
            }
        }
    }
    Err(err(CarrierErrorKind::Io(io::ErrorKind::InvalidData), "a reply of more than 64 lines"))
}

fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carrier::{Carrier, Cost};
    use std::time::Duration;

    fn carrier(batch: usize) -> Smtp {
        let credential = Credential::new("session-user", "made-up-secret").expect("a credential");
        Smtp { batch, ..Smtp::new(credential, "from@example.invalid", "to@example.invalid") }
    }

    #[test]
    fn a_message_fits_its_declared_size_and_the_batch_is_bounded() {
        let smtp = carrier(super::super::BATCH);
        let message = message(&smtp, "session", 3, &[7u8; 300]);
        assert_eq!(message.len(), message_size(&smtp, "session", 300), "the size of the message is exact");
        assert!(message.starts_with(b"From: from@example.invalid\r\n"), "the envelope");
        let needle = b"X-Podlink-Batch: ";
        assert!(message.windows(needle.len()).any(|window| window == needle), "the sequence header");
        let payload = payload_at_most(&smtp, "session", smtp.batch, 1024);
        assert!(payload < smtp.batch, "a size limit reduces the payload: {payload}");
        assert!(message_size(&smtp, "session", payload) <= 1024);
        assert!(payload_at_most(&smtp, "session", smtp.batch, u64::from(u32::MAX)) == smtp.batch);
        assert_eq!(smtp.facts().cost, Cost::Medium);
    }

    #[test]
    fn a_smaller_payload_takes_the_bytes_of_a_message_above_the_limit() {
        let smtp = carrier(4096);
        let payload = payload_at_most(&smtp, "s", 4096, 1024);
        assert!(payload >= super::super::MIN_BATCH, "a payload of {payload}");
        let whole = vec![3u8; 1000];
        let first = message(&smtp, "s", 0, &whole[..payload]);
        assert!(first.len() <= 1024, "the message fits the limit: {}", first.len());
    }

    #[test]
    fn a_duration_zero_is_refused() {
        let mut smtp = carrier(super::super::BATCH);
        smtp.flush_after = Duration::ZERO;
        assert!(smtp.check().is_err());
    }
}

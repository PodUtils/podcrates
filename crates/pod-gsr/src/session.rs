//! The control handshake with the relay and the TLS session that follows.
//!
//! A listener sends `LISTEN` with a random token, a connector sends
//! `CONNECT`. Both wait for `START`, and both send `ACCEPT` when it arrives.
//! The flags of `START` name the side of the TLS handshake. A peer that waits
//! sends `PING` every `settings.ping`, because an idle registration ends at
//! the relay.

use std::pin::Pin;

use pod_core::entropy::Entropy;
use pod_core::{BoxedIo, Class, Dial, Failure, Stage, Target};
use tokio::io::AsyncWriteExt;
use tokio::time::{Instant, timeout};

use crate::Role;
use crate::identity::Identity;
use crate::settings::Settings;
use crate::srp::{Side, Srp};
use crate::stream::Stream;
use crate::wire::{self, Message, Reader};

/// Open a tunnel through the relay.
///
/// The dial and the wait for the peer end at `deadline`; a caller that needs a
/// limit on the TLS handshake wraps this call in its own timeout. A listener
/// pings the relay every `settings.ping` while it waits.
pub async fn open(
    dial: &dyn Dial,
    settings: &Settings,
    role: Role,
    secret: &str,
    entropy: &mut (dyn Entropy + Send),
    deadline: Instant,
) -> Result<Stream, Failure> {
    check_secret(secret, settings)?;
    let identity = Identity::new(secret, settings);
    let target = Target::new(&identity.host(settings), settings.port);
    let connected = dial.dial(&target, deadline).await?;
    let mut io = connected.io;

    match role {
        Role::Listen => {
            let mut token = [0u8; wire::TOKEN_LEN];
            entropy.fill(&mut token).map_err(|e| Failure::new(Stage::Relay, Class::Unavailable, e.to_string()))?;
            write(&mut io, &wire::listen(identity.addr(), &token, settings.version)).await?;
        }
        Role::Connect => {
            write(&mut io, &wire::connect(identity.addr(), settings.version)).await?;
        }
    }

    let mut reader = Reader::new();
    let server = wait_for_start(&mut io, &mut reader, settings, deadline).await?;
    write(&mut io, &wire::accept()).await?;

    let side = if server { Side::Server } else { Side::Client };
    let srp = Srp::new(settings, &identity, side)?;
    let ssl = srp.ssl()?;
    let mut stream = tokio_openssl::SslStream::new(ssl, io)
        .map_err(|e| Failure::new(Stage::Tls, Class::Config, format!("the TLS session of the relay: {e}")))?;
    let result = match side {
        Side::Server => Pin::new(&mut stream).accept().await,
        Side::Client => Pin::new(&mut stream).connect().await,
    };
    result.map_err(handshake_failure)?;
    Ok(Stream::new(stream, srp))
}

/// Wait for `START`, pinging the relay while it waits. The answer is the side
/// of the TLS handshake that the relay named.
async fn wait_for_start(
    io: &mut BoxedIo,
    reader: &mut Reader,
    settings: &Settings,
    deadline: Instant,
) -> Result<bool, Failure> {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(timeout_failure());
        }
        let until = (now + settings.ping).min(deadline);
        match timeout(until - now, reader.next(io)).await {
            Ok(Ok(Some(Message::Start { server }))) => return Ok(server),
            Ok(Ok(Some(Message::Pong))) => {}
            Ok(Ok(Some(Message::Status { fatal: false, .. }))) => {}
            Ok(Ok(Some(Message::Status { fatal: true, code }))) => return Err(status_failure(code)),
            Ok(Ok(None)) => return Err(closed_failure()),
            Ok(Err(failure)) => return Err(failure),
            Err(_elapsed) => {
                if Instant::now() >= deadline {
                    return Err(timeout_failure());
                }
                write(io, &wire::ping()).await?;
            }
        }
    }
}

async fn write(io: &mut BoxedIo, bytes: &[u8]) -> Result<(), Failure> {
    io.write_all(bytes).await.map_err(|e| Failure::io(Stage::Relay, e))
}

fn check_secret(secret: &str, settings: &Settings) -> Result<(), Failure> {
    if secret.is_empty() {
        return Err(Failure::new(Stage::Relay, Class::Usage, "the secret is empty"));
    }
    if secret.len() > settings.secret_max {
        return Err(Failure::new(
            Stage::Relay,
            Class::Usage,
            format!("the secret is {} bytes, and the service takes at most {}", secret.len(), settings.secret_max),
        ));
    }
    Ok(())
}

fn timeout_failure() -> Failure {
    Failure::new(Stage::Relay, Class::Timeout, "the relay gave no peer before the time limit")
}

fn closed_failure() -> Failure {
    Failure::new(Stage::Relay, Class::Unavailable, "the relay closed the connection before a peer arrived")
}

/// The failure of a fatal status, with the words of gsocket for each code.
fn status_failure(code: u8) -> Failure {
    let (class, text) = match code {
        0x01 => (Class::Unavailable, "the relay says that the address is in use"),
        0x02 => (Class::Unavailable, "no peer is listening for this secret"),
        0x03 => (Class::Timeout, "the relay ended the registration at its idle limit"),
        0x04 => (Class::Refused, "the relay denied the connection"),
        0x05 => (Class::Protocol, "the relay reported a protocol error"),
        0x07 => (Class::Unavailable, "the relay could not reach the peer"),
        0x2a => (Class::Protocol, "the relay is newer than this client"),
        _ => (Class::Protocol, "the relay reported a failure"),
    };
    Failure::new(Stage::Relay, class, format!("{text} (code {code})"))
}

/// A failed handshake, with the words from the OpenSSL error stack.
///
/// The stack names what OpenSSL objected to; the `openssl::ssl::Error` alone
/// says "SSL error" for most SRP failures, which sends a reader looking at the
/// transport instead of at the secret.
fn handshake_failure(error: openssl::ssl::Error) -> Failure {
    if let Some(io) = error.io_error() {
        return Failure::io(Stage::Tls, std::io::Error::new(io.kind(), io.to_string()));
    }
    let detail = error.ssl_error().map(stack_text).unwrap_or_else(|| error.to_string());
    Failure::new(
        Stage::Tls,
        Class::Refused,
        format!("the SRP handshake failed; the two ends may not share the secret: {detail}"),
    )
}

/// The reasons of an error stack, joined.
fn stack_text(stack: &openssl::error::ErrorStack) -> String {
    let reasons: Vec<String> = stack.errors().iter().map(|error| error.to_string()).collect();
    if reasons.is_empty() { "no OpenSSL detail".to_string() } else { reasons.join("; ") }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};
    use std::time::Duration;

    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    use super::*;

    /// A stream of bytes that stays pending after its data, and records what
    /// the crate writes.
    struct Fake {
        data: Vec<u8>,
        at: usize,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl AsyncRead for Fake {
        fn poll_read(mut self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
            if self.at >= self.data.len() {
                return Poll::Pending;
            }
            let n = (self.data.len() - self.at).min(buf.remaining());
            let (start, end) = (self.at, self.at + n);
            buf.put_slice(&self.data[start..end]);
            self.at = end;
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for Fake {
        fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
            self.get_mut().written.lock().expect("the record").extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    fn settings() -> Settings {
        Settings::of_data().expect("the data file")
    }

    fn io_with(data: Vec<u8>) -> (BoxedIo, Arc<Mutex<Vec<u8>>>) {
        let written = Arc::new(Mutex::new(Vec::new()));
        let fake = Fake { data, at: 0, written: Arc::clone(&written) };
        (Box::new(fake), written)
    }

    fn start(flags: u8) -> [u8; wire::MESSAGE_LEN] {
        let mut message = [0u8; wire::MESSAGE_LEN];
        message[0] = 0x05;
        message[1] = flags;
        message
    }

    #[tokio::test(start_paused = true)]
    async fn a_pong_before_start_is_discarded_and_start_names_the_side() {
        let mut settings = settings();
        settings.ping = Duration::from_secs(45);
        let mut data = vec![0u8; wire::MESSAGE_LEN];
        data[0] = 0x04;
        data.extend_from_slice(&start(0x01));
        let (mut io, written) = io_with(data);
        let server = wait_for_start(&mut io, &mut Reader::new(), &settings, Instant::now() + Duration::from_secs(10))
            .await
            .unwrap();
        assert!(server);
        assert!(written.lock().expect("the record").is_empty(), "no ping while the peer arrives");
    }

    #[tokio::test(start_paused = true)]
    async fn the_wait_pings_the_relay_and_ends_at_the_deadline() {
        let mut settings = settings();
        settings.ping = Duration::from_millis(10);
        let (mut io, written) = io_with(Vec::new());
        let failure =
            wait_for_start(&mut io, &mut Reader::new(), &settings, Instant::now() + Duration::from_millis(25))
                .await
                .expect_err("no peer");
        assert_eq!((failure.stage, failure.class), (Stage::Relay, Class::Timeout));
        let written = written.lock().expect("the record");
        assert!(written.len() >= 2 * wire::MESSAGE_LEN, "two pings at least");
        for message in written.chunks(wire::MESSAGE_LEN) {
            assert_eq!(message[0], 0x03, "a PING");
            assert_eq!(message.len(), wire::MESSAGE_LEN);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_fatal_status_ends_the_wait() {
        let settings = settings();
        let mut data = vec![0u8; wire::MESSAGE_LEN];
        data[0] = 0x07;
        data[1] = 0x02;
        data[2] = 0x02;
        let (mut io, _) = io_with(data);
        let failure = wait_for_start(&mut io, &mut Reader::new(), &settings, Instant::now() + Duration::from_secs(10))
            .await
            .expect_err("refused");
        assert_eq!(failure.class, Class::Unavailable);
        assert!(failure.text().contains("no peer"), "{}", failure.text());
    }

    #[test]
    fn the_secret_has_a_length_that_the_service_takes() {
        let settings = settings();
        assert!(check_secret("", &settings).is_err());
        assert!(check_secret(&"x".repeat(settings.secret_max + 1), &settings).is_err());
        assert!(check_secret(&"x".repeat(settings.secret_max), &settings).is_ok());
    }
}

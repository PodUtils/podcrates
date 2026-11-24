//! The poll loop of the NTP carrier's dial: each poll sends the bytes up in
//! one request, waits for the reply of the server, and gives the bytes down
//! to the link. The first poll waits [`FIRST`] for the bytes that a link
//! gives at once; each later poll waits the poll interval (NTP-3).

use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::UdpSocket;
use tokio::time::{Instant, timeout};

use crate::carrier::{CarrierError, CarrierErrorKind, Limits};

use super::NAME;
use super::session::Session;
use super::wire::Timestamp;

/// The wait of the first poll, for the bytes of a link that starts at once.
pub const FIRST: Duration = Duration::from_millis(50);

/// The ring of the byte stream between the link and the task of the
/// carrier.
pub(super) const PIPE: usize = 64 << 10;

/// The most time that one turn gives the link to read its bytes down: a
/// link that is busy keeps them for its next turn, so no poll waits on it.
const GIVE: Duration = Duration::from_millis(50);

fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

/// Poll the server of `socket` until the link drops its stream: the bytes
/// of the link go up, and the bytes of the server come down.
pub(super) async fn run(
    socket: UdpSocket,
    mut session: Session,
    far: &mut crate::pipes::Far,
    poll: Duration,
    reply_timeout: Duration,
    limits: Limits,
) -> Result<(), CarrierError> {
    let mut chunk = vec![0u8; PIPE];
    let mut down = Down::default();
    let mut next = Instant::now() + FIRST;
    loop {
        let wait = next.saturating_duration_since(Instant::now());
        let read = tokio::select! {
            read = far.to_peer.read(&mut chunk) => {
                read.map_err(|e| CarrierError::io(NAME, "the stream", e))?
            }
            () = tokio::time::sleep(wait) => {
                exchange(&socket, &mut session, &mut down, far, reply_timeout, &limits).await?;
                next = Instant::now() + poll;
                continue;
            }
        };
        session.write(&chunk[..read]);
        if read == 0 {
            // The link ended its bytes: send the last payload before the
            // close.
            while session.pending_up() > 0 {
                exchange(&socket, &mut session, &mut down, far, reply_timeout, &limits).await?;
            }
            return Ok(());
        }
    }
}

/// One request, one reply, and the bytes down to the link.
async fn exchange(
    socket: &UdpSocket,
    session: &mut Session,
    down: &mut Down,
    far: &mut crate::pipes::Far,
    reply_timeout: Duration,
    limits: &Limits,
) -> Result<(), CarrierError> {
    let request = session
        .request(Timestamp::now())
        .map_err(|e| err(CarrierErrorKind::Usage, format!("the request of the carrier: {e}")))?;
    socket.send(&request).await.map_err(|e| CarrierError::io(NAME, "the request of the carrier", e))?;
    let deadline = Instant::now() + reply_timeout;
    let mut buffer = vec![0u8; 2048];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            session.retry();
            break;
        }
        match timeout(left, socket.recv(&mut buffer)).await {
            Ok(Ok(bytes)) => {
                let at = Timestamp::now();
                match session.reply(&buffer[..bytes], at) {
                    Ok(Some(_)) => break,
                    Ok(None) => {}
                    Err(e) => {
                        return Err(err(
                            CarrierErrorKind::Io(std::io::ErrorKind::InvalidData),
                            format!("a reply of the server: {e}"),
                        ));
                    }
                }
            }
            Ok(Err(e)) => return Err(CarrierError::io(NAME, "the reply of the server", e)),
            Err(_) => {
                // No reply: the bytes of the request go again at the next
                // poll. The carrier is not exactly once: the ARQ above it
                // takes the duplicates.
                session.retry();
                break;
            }
        }
    }
    if let Some(limit) = session.limit() {
        limits.set(limit);
    }
    down.gather(session);
    down.give(far).await
}

/// The bytes down that wait for the link to read them.
#[derive(Debug, Default)]
struct Down {
    bytes: Vec<u8>,
    written: usize,
}

impl Down {
    /// Take the bytes of the session down.
    fn gather(&mut self, session: &mut Session) {
        let mut chunk = vec![0u8; PIPE];
        loop {
            let read = session.read(&mut chunk);
            if read == 0 {
                return;
            }
            self.bytes.extend_from_slice(&chunk[..read]);
        }
    }

    /// Give what the link takes now, for at most [`GIVE`] of one turn.
    async fn give(&mut self, far: &mut crate::pipes::Far) -> Result<(), CarrierError> {
        while self.written < self.bytes.len() {
            match timeout(GIVE, far.from_peer.write(&self.bytes[self.written..])).await {
                Ok(Ok(bytes)) => self.written += bytes,
                Ok(Err(e)) => return Err(CarrierError::io(NAME, "the stream", e)),
                Err(_) => break,
            }
        }
        if self.written == self.bytes.len() {
            self.bytes.clear();
            self.written = 0;
        }
        Ok(())
    }
}

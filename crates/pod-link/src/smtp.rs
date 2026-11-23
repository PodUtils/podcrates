//! The SMTP submission carrier (feature `smtp`, PodUtils/podnet#484): the
//! bytes of the dialer go up to the submission server of the network as
//! messages, one message for each batch. The bytes down come from the
//! retrieval carrier ([`crate::imap`]); a caller joins the two with
//! [`crate::mail_pair`], or runs each one where its network permits it.
//!
//! * The dial address is `HOST:PORT`. The port gives the mode: 587 with
//!   STARTTLS, or 465 with TLS from the first byte. Port 25 is refused by
//!   name before any connection (MAIL-5, PodUtils/podnet#484): the egress
//!   broker refuses it by a fixed rule, and a carrier that needs that rule
//!   relaxed is a carrier that changes the policy. A test port takes
//!   [`Smtp::with_mode`].
//! * [`Smtp::credential`] is a credential of the session, never the
//!   password of the account (MAIL-3). The client sends it with `AUTH
//!   PLAIN` over TLS, and the `Debug` of a credential hides its secret.
//! * One batch is one message (MAIL-1): at most [`Smtp::batch`] payload
//!   bytes, base64 in the body, with a sequence header of the session. The
//!   carrier sends no message again after the server took it: a lost
//!   message is the business of the session ARQ above the mail pair
//!   (MAIL-2).
//! * A server `SIZE` limit, or a refused message (552 or 554), is a
//!   carrier limit ([`crate::Limit`]), not a protocol error (MAIL-4): the
//!   carrier reports it, sends the same bytes at a smaller size, and the
//!   SMTP session stays open. A message that the server refused was not
//!   accepted, so no byte of it is in the mailbox twice.
//! * [`Carrier::facts`] declares the cost class `Medium` (a message for
//!   each batch, with its envelope and its base64) and the rate that the
//!   last submission measured.
//! * A dial gives a write-only stream. A read of it is an error of
//!   `Unsupported`, or the error of a session that failed: the bytes down
//!   are the retrieval carrier's.
//!
//! The server of this carrier is the network's. The tests run a submission
//! server of their own on the loopback (`tests/common/mail.rs`), over a
//! plain connection with TLS through `pod-tls`.

use std::fmt;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use pod_core::entropy::{Entropy, OsEntropy};
use rustls::ClientConfig;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::carrier::{
    BoxIo, Carrier, CarrierError, CarrierErrorKind, Cost, Facts, Limit, Limits, Listener, Pending, Rate,
};
use crate::mail;
pub use crate::mail::{Credential, Endpoint, Mode};
use crate::pipes::{Pipes, pipes};

/// The name of the carrier.
pub const NAME: &str = "smtp";

/// The payload bytes of one message by default.
pub const BATCH: usize = 32 << 10;

/// The smallest payload of one message.
pub const MIN_BATCH: usize = 256;

/// The largest payload of one message.
pub const MAX_BATCH: usize = 256 << 10;

/// The time limit of a connection to the submission server.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// The time limit of the greeting, of STARTTLS, of the TLS handshake and
/// of `AUTH`.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

/// The time limit of one message: its envelope, its body and the answer.
pub const SUBMIT_TIMEOUT: Duration = Duration::from_secs(120);

/// The wait of a partial batch: a write smaller than the batch goes to the
/// server after this silence, not at the end of the stream.
pub const FLUSH_AFTER: Duration = Duration::from_millis(250);

/// The ring of the byte stream between the link and the task of the
/// carrier.
const PIPE: usize = 64 << 10;

/// The SMTP submission carrier: the credential of the session, the
/// envelope of each message, the port mode, the TLS of the caller and the
/// rate that the last submission measured. A clone shares the limits and
/// the measurement.
#[derive(Clone)]
pub struct Smtp {
    /// The mode of TLS. `None` reads it from the port ([`Mode::of_smtp`]).
    pub mode: Option<Mode>,
    /// The credential of the session, never the account password.
    pub credential: Credential,
    /// The envelope sender of each message.
    pub from: String,
    /// The envelope recipient of each message: the mailbox of the session.
    pub to: String,
    /// The subject of each message.
    pub subject: String,
    /// The payload bytes of one message.
    pub batch: usize,
    /// The wait of a partial batch before the server takes it.
    pub flush_after: Duration,
    pub connect_timeout: Duration,
    pub handshake_timeout: Duration,
    pub submit_timeout: Duration,
    /// The TLS configuration of the caller, with its roots.
    pub tls: Option<Arc<ClientConfig>>,
    limits: Limits,
    measured: Arc<AtomicU64>,
}

impl fmt::Debug for Smtp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Smtp")
            .field("mode", &self.mode)
            .field("credential", &self.credential)
            .field("from", &self.from)
            .field("to", &self.to)
            .field("subject", &self.subject)
            .field("batch", &self.batch)
            .field("limit", &self.limits.get())
            .field("tls", &self.tls.is_some())
            .finish()
    }
}

fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

impl Smtp {
    /// A carrier that submits for `credential` from `from` to `to`.
    pub fn new(credential: Credential, from: impl Into<String>, to: impl Into<String>) -> Smtp {
        Smtp {
            mode: None,
            credential,
            from: from.into(),
            to: to.into(),
            subject: "podlink".into(),
            batch: BATCH,
            flush_after: FLUSH_AFTER,
            connect_timeout: CONNECT_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            submit_timeout: SUBMIT_TIMEOUT,
            tls: None,
            limits: Limits::new(),
            measured: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The TLS configuration of the caller, with its roots.
    pub fn with_tls(mut self, config: Arc<ClientConfig>) -> Smtp {
        self.tls = Some(config);
        self
    }

    /// The mode of TLS, which a test port needs.
    pub fn with_mode(mut self, mode: Mode) -> Smtp {
        self.mode = Some(mode);
        self
    }

    /// The last limit that a session of this carrier met.
    pub fn limit(&self) -> Option<Limit> {
        self.limits.get()
    }

    /// The endpoint and the mode of a dial address `HOST:PORT`. Port 25 is
    /// refused here, before any connection.
    pub fn address(&self, address: &str) -> Result<(Endpoint, Mode), CarrierError> {
        let endpoint = Endpoint::parse(address).map_err(|e| err(CarrierErrorKind::Usage, e))?;
        if endpoint.port == mail::SMTP_FORBIDDEN {
            return Err(err(
                CarrierErrorKind::Usage,
                "port 25 is never used for submission: use 587 with STARTTLS or 465 with TLS (MAIL-5, \
                 PodUtils/podnet#484)",
            ));
        }
        let mode = match self.mode {
            Some(mode) => mode,
            None => Mode::of_smtp(endpoint.port).map_err(|e| err(CarrierErrorKind::Usage, e))?,
        };
        Ok((endpoint, mode))
    }

    /// The settings and the envelope are of their form.
    pub fn check(&self) -> Result<(), CarrierError> {
        if !(MIN_BATCH..=MAX_BATCH).contains(&self.batch) {
            return Err(err(
                CarrierErrorKind::Usage,
                format!("a batch of {} bytes: give {MIN_BATCH} to {MAX_BATCH}", self.batch),
            ));
        }
        for (what, value) in [("from", &self.from), ("to", &self.to), ("subject", &self.subject)] {
            if value.is_empty() {
                return Err(err(CarrierErrorKind::Usage, format!("the {what} is empty")));
            }
            if value.bytes().any(|b| b == b'\r' || b == b'\n') {
                return Err(err(CarrierErrorKind::Usage, format!("the {what} has a line break")));
            }
        }
        if self.connect_timeout.is_zero()
            || self.handshake_timeout.is_zero()
            || self.submit_timeout.is_zero()
            || self.flush_after.is_zero()
        {
            return Err(err(CarrierErrorKind::Usage, "a time of the carrier is 0"));
        }
        Ok(())
    }

    /// The rate that the last submission measured, with the cost class
    /// `Medium`.
    pub fn rate(&self) -> Rate {
        let nanos = self.measured.load(Ordering::Relaxed);
        let (bytes_per_second, poll_interval) = if nanos == 0 {
            (None, None)
        } else {
            let secs = nanos as f64 / 1e9;
            (Some((self.batch as f64 / secs) as u64), Some(Duration::from_nanos(nanos)))
        };
        Rate { bytes_per_second, poll_interval, cost: Cost::Medium }
    }

    /// The session id of a dial: 16 random bytes in hex.
    fn session(&self) -> Result<String, CarrierError> {
        let mut id = [0u8; 16];
        OsEntropy.fill(&mut id).map_err(|e| err(CarrierErrorKind::Unavailable, format!("no random bytes: {e}")))?;
        Ok(id.iter().map(|b| format!("{b:02x}")).collect())
    }
}

impl Carrier for Smtp {
    fn name(&self) -> &'static str {
        NAME
    }

    /// Ordered and reliable, one direction: an accepted message is in the
    /// mailbox of the recipient. Duplex `false`: the bytes down come from
    /// the retrieval carrier.
    fn facts(&self) -> Facts {
        Facts { ordered: true, reliable: true, duplex: false, mtu: None, cost: Cost::Medium, rate: self.rate() }
    }

    /// A TCP socket binds on the loopback: the host allows the family.
    fn available(&self) -> Result<(), CarrierError> {
        mail::tcp_available(NAME)
    }

    /// Submit to `HOST:PORT` ([`Smtp::address`]); the stream takes the
    /// bytes up.
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let (endpoint, mode) = self.address(address)?;
            self.check()?;
            let config = mail::tls_config(NAME, &self.tls, mode)?;
            let session = self.session()?;
            let ends = crate::pipes::Ends::new();
            let (stream, far) = pipes(PIPE, ends.clone());
            let smtp = self.clone();
            let task = client::run(smtp, endpoint, mode, config, session, far);
            tokio::spawn(async move {
                if let Err(e) = task.await {
                    ends.fail_with(Arc::new(move || io::Error::new(io::ErrorKind::InvalidData, e.to_string())));
                }
            });
            Ok(Box::new(UpOnly { inner: stream }) as BoxIo)
        })
    }

    /// The SMTP carrier submits to the server of the network; it runs no
    /// submission server of its own.
    fn listen<'a>(&'a self, _address: &'a str) -> Pending<'a, Box<dyn Listener>> {
        Box::pin(async move {
            Err(err(
                CarrierErrorKind::Unavailable,
                "the smtp carrier submits to a server: the bytes down come from the imap carrier",
            ))
        })
    }
}

/// The stream of a submission: the bytes up go to the server, and a read
/// says that this direction carries no bytes.
struct UpOnly {
    inner: Pipes,
}

impl AsyncRead for UpOnly {
    fn poll_read(self: Pin<&mut Self>, _: &mut Context<'_>, _: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        // A failed session gives its error here, so that a caller that only
        // writes still learns it.
        if let Some(error) = self.inner.fault() {
            return Poll::Ready(Err(error));
        }
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the smtp carrier carries bytes up: the bytes down are the imap carrier's",
        )))
    }
}

impl AsyncWrite for UpOnly {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, data)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

mod client;

pub use client::payload_at_most;

#[cfg(test)]
mod tests {
    use super::*;

    fn carrier(batch: usize) -> Smtp {
        let credential = Credential::new("session-user", "made-up-secret").expect("a credential");
        Smtp { batch, ..Smtp::new(credential, "from@example.invalid", "to@example.invalid") }
    }

    #[test]
    fn port_25_is_refused_by_name_before_a_dial_and_the_other_ports_give_a_mode() {
        let smtp = carrier(BATCH);
        let refused = smtp.address("127.0.0.1:25").expect_err("port 25");
        assert_eq!(refused.kind, CarrierErrorKind::Usage, "{refused}");
        assert!(refused.to_string().contains("port 25"), "{refused}");
        assert_eq!(smtp.address("mail.example:587").expect("587").1, Mode::Starttls);
        assert_eq!(smtp.address("mail.example:465").expect("465").1, Mode::Implicit);
        let test = smtp.clone().with_mode(Mode::Starttls);
        assert_eq!(test.address("127.0.0.1:2525").expect("a test port").1, Mode::Starttls);
        assert!(smtp.address("127.0.0.1:2525").is_err(), "a port with no mode of its own");
    }

    #[test]
    fn the_facts_declare_the_cost_class_and_the_rate_names_it_too() {
        let facts = carrier(BATCH).facts();
        assert_eq!((facts.ordered, facts.reliable, facts.duplex), (true, true, false));
        assert_eq!(facts.cost, Cost::Medium);
        assert_eq!(facts.rate.cost, facts.cost, "one value names the price");
        assert_eq!(facts.rate.poll_interval, None, "no submission measured yet");
    }
}

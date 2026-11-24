//! The IMAP retrieval carrier (feature `imap`, PodUtils/podnet#484): the
//! bytes of the peer come down from the retrieval server of the network as
//! the bodies of fetched messages, and the bytes of this end go up as
//! appended messages in the mailbox of the session. One carrier carries
//! both directions; a caller that pairs it with [`crate::smtp`] joins the
//! two with [`crate::mail_pair`].
//!
//! * The dial address is `HOST:PORT`. The port gives the mode: 143 with
//!   STARTTLS, or 993 with TLS from the first byte. A test port takes
//!   [`Imap::with_mode`].
//! * [`Imap::credential`] is a credential of the session, never the
//!   password of the account (MAIL-3). The client sends it with `LOGIN`
//!   over TLS, and the `Debug` of a credential hides its secret.
//! * The bytes down are the messages of [`Imap::inbox`], fetched by `UID
//!   FETCH` from the uid after the last one that this session read, so a
//!   poll gives each message once and in order. The bytes up are appended
//!   to [`Imap::outbox`], one `APPEND` for each batch.
//! * The carrier sends nothing again: a message that the server took is
//!   not appended twice, and a message that was fetched is not requested
//!   again (MAIL-2). A lost unit is the business of the session ARQ above.
//! * A server `APPENDLIMIT`, a refused `APPEND` (`NO [TOOBIG ...]`), and a
//!   fetched body over [`Imap::max_fetch`] are carrier limits
//!   ([`crate::Limit`]), not protocol errors (MAIL-4): the carrier reports
//!   them and continues.
//! * [`Carrier::facts`] declares the cost class `High` (a poll each
//!   interval, a command and a literal for each batch) and the rate of the
//!   poll.
//!
//! The server of this carrier is the network's. The tests run a retrieval
//! server of their own on the loopback (`tests/common/mail.rs`), over a
//! plain connection with TLS through `pod-tls`.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use rustls::ClientConfig;

use crate::carrier::{
    BoxIo, Carrier, CarrierError, CarrierErrorKind, Cost, Facts, Limit, Limits, Listener, Pending, Rate,
};
use crate::mail;
pub use crate::mail::{Credential, Endpoint, Mode};
mod client;

/// The name of the carrier.
pub const NAME: &str = "imap";

/// The payload bytes of one `APPEND` by default.
pub const BATCH: usize = 32 << 10;

/// The smallest payload of one `APPEND`.
pub const MIN_BATCH: usize = 256;

/// The largest payload of one `APPEND`.
pub const MAX_BATCH: usize = 256 << 10;

/// The largest body that a fetch takes by default.
pub const MAX_FETCH: usize = 1 << 20;

/// The time between two polls of the mailbox by default.
pub const POLL: Duration = Duration::from_secs(5);

/// The shortest poll that the carrier takes.
pub const MIN_POLL: Duration = Duration::from_millis(100);

/// The longest poll that the carrier takes.
pub const MAX_POLL: Duration = Duration::from_secs(300);

/// The time limit of a connection to the retrieval server.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// The time limit of the greeting, of STARTTLS, of the TLS handshake and
/// of `LOGIN`.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

/// The time limit of one command.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

/// The ring of the byte stream between the link and the task of the
/// carrier.
const PIPE: usize = 64 << 10;

/// The bytes fetched that wait for the link to read them.
const PENDING: usize = 1 << 20;

/// The IMAP retrieval carrier: the credential of the session, the two
/// mailboxes, the port mode, the TLS of the caller and the rate of its
/// poll. A clone shares the limits.
#[derive(Clone)]
pub struct Imap {
    /// The mode of TLS. `None` reads it from the port ([`Mode::of_imap`]).
    pub mode: Option<Mode>,
    /// The credential of the session, never the account password.
    pub credential: Credential,
    /// The mailbox that this end fetches: the other end appends there.
    pub inbox: String,
    /// The mailbox that this end appends to: the other end fetches there.
    pub outbox: String,
    /// The payload bytes of one `APPEND`.
    pub batch: usize,
    /// The largest fetched body that the carrier takes.
    pub max_fetch: usize,
    /// The time between two polls of the mailbox.
    pub poll: Duration,
    pub connect_timeout: Duration,
    pub handshake_timeout: Duration,
    pub command_timeout: Duration,
    /// The TLS configuration of the caller, with its roots.
    pub tls: Option<Arc<ClientConfig>>,
    limits: Limits,
}

impl fmt::Debug for Imap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Imap")
            .field("mode", &self.mode)
            .field("credential", &self.credential)
            .field("inbox", &self.inbox)
            .field("outbox", &self.outbox)
            .field("batch", &self.batch)
            .field("max_fetch", &self.max_fetch)
            .field("poll", &self.poll)
            .field("limit", &self.limits.get())
            .field("tls", &self.tls.is_some())
            .finish()
    }
}

fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

impl Imap {
    /// A carrier that reads `inbox` and writes `outbox` for `credential`.
    pub fn new(credential: Credential, inbox: impl Into<String>, outbox: impl Into<String>) -> Imap {
        Imap {
            mode: None,
            credential,
            inbox: inbox.into(),
            outbox: outbox.into(),
            batch: BATCH,
            max_fetch: MAX_FETCH,
            poll: POLL,
            connect_timeout: CONNECT_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            command_timeout: COMMAND_TIMEOUT,
            tls: None,
            limits: Limits::new(),
        }
    }

    /// The TLS configuration of the caller, with its roots.
    pub fn with_tls(mut self, config: Arc<ClientConfig>) -> Imap {
        self.tls = Some(config);
        self
    }

    /// The mode of TLS, which a test port needs.
    pub fn with_mode(mut self, mode: Mode) -> Imap {
        self.mode = Some(mode);
        self
    }

    /// The two mailboxes of the session, which a test gives.
    pub fn with_mailboxes(mut self, inbox: impl Into<String>, outbox: impl Into<String>) -> Imap {
        self.inbox = inbox.into();
        self.outbox = outbox.into();
        self
    }

    /// The last limit that a session of this carrier met.
    pub fn limit(&self) -> Option<Limit> {
        self.limits.get()
    }

    /// The endpoint and the mode of a dial address `HOST:PORT`.
    pub fn address(&self, address: &str) -> Result<(Endpoint, Mode), CarrierError> {
        let endpoint = Endpoint::parse(address).map_err(|e| err(CarrierErrorKind::Usage, e))?;
        let mode = match self.mode {
            Some(mode) => mode,
            None => Mode::of_imap(endpoint.port).map_err(|e| err(CarrierErrorKind::Usage, e))?,
        };
        Ok((endpoint, mode))
    }

    /// The settings and the mailboxes are of their form.
    pub fn check(&self) -> Result<(), CarrierError> {
        if !(MIN_BATCH..=MAX_BATCH).contains(&self.batch) {
            return Err(err(
                CarrierErrorKind::Usage,
                format!("a batch of {} bytes: give {MIN_BATCH} to {MAX_BATCH}", self.batch),
            ));
        }
        if self.max_fetch < MIN_BATCH {
            return Err(err(CarrierErrorKind::Usage, format!("a fetch limit under {MIN_BATCH} bytes")));
        }
        if !(MIN_POLL..=MAX_POLL).contains(&self.poll) {
            return Err(err(
                CarrierErrorKind::Usage,
                format!("a poll of {:?}: give {MIN_POLL:?} to {MAX_POLL:?}", self.poll),
            ));
        }
        for (what, value) in [("inbox", &self.inbox), ("outbox", &self.outbox)] {
            if value.is_empty() || value.bytes().any(|b| b == b'\r' || b == b'\n' || b == b'"') {
                return Err(err(CarrierErrorKind::Usage, format!("the {what} is empty or has a quote")));
            }
        }
        if self.connect_timeout.is_zero() || self.handshake_timeout.is_zero() || self.command_timeout.is_zero() {
            return Err(err(CarrierErrorKind::Usage, "a time of the carrier is 0"));
        }
        Ok(())
    }

    /// The rate of the poll, with the cost class `High`.
    pub fn rate(&self) -> Rate {
        let secs = self.poll.as_secs_f64();
        let bytes_per_second = Some((self.batch as f64 / secs) as u64);
        Rate { bytes_per_second, poll_interval: Some(self.poll), cost: Cost::High }
    }
}

impl Carrier for Imap {
    fn name(&self) -> &'static str {
        NAME
    }

    /// Ordered and reliable in each direction, duplex: `APPEND` up and
    /// `UID FETCH` down over one session.
    fn facts(&self) -> Facts {
        Facts { ordered: true, reliable: true, duplex: true, mtu: None, cost: Cost::High, rate: self.rate() }
    }

    /// A TCP socket binds on the loopback: the host allows the family.
    fn available(&self) -> Result<(), CarrierError> {
        mail::tcp_available(NAME)
    }

    /// Retrieve from `HOST:PORT` ([`Imap::address`]): both directions of
    /// the session over one connection.
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let (endpoint, mode) = self.address(address)?;
            self.check()?;
            let config = mail::tls_config(NAME, &self.tls, mode)?;
            let ends = crate::pipes::Ends::new();
            let (stream, far) = crate::pipes::pipes(PIPE, ends.clone());
            let imap = self.clone();
            crate::pipes::spawn_task(ends, far, move |far| Box::pin(client::run(imap, endpoint, mode, config, far)));
            Ok(Box::new(stream) as BoxIo)
        })
    }

    /// The IMAP carrier retrieves from the server of the network; it runs no
    /// retrieval server of its own.
    fn listen<'a>(&'a self, _address: &'a str) -> Pending<'a, Box<dyn Listener>> {
        Box::pin(async move {
            Err(err(
                CarrierErrorKind::Unavailable,
                "the imap carrier retrieves from a server: give its address to dial",
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_and_the_mailboxes_are_of_their_form() {
        let credential = Credential::new("session-user", "made-up-secret").expect("a credential");
        let imap = Imap::new(credential, "in", "out");
        assert!(imap.check().is_ok());
        assert!(imap.clone().with_mailboxes("in\"x", "out").check().is_err(), "a quote in a mailbox");
        assert!(Imap { batch: 1, ..imap.clone() }.check().is_err());
        assert!(Imap { poll: Duration::from_millis(1), ..imap.clone() }.check().is_err());
        assert!(Imap { max_fetch: 1, ..imap.clone() }.check().is_err());
        assert_eq!(imap.address("mail.example:993").expect("993").1, Mode::Implicit);
        assert_eq!(imap.address("mail.example:143").expect("143").1, Mode::Starttls);
    }

    #[test]
    fn the_facts_declare_the_poll_and_its_cost() {
        let credential = Credential::new("session-user", "made-up-secret").expect("a credential");
        let facts = Imap::new(credential, "in", "out").facts();
        assert_eq!((facts.ordered, facts.reliable, facts.duplex), (true, true, true));
        assert_eq!(facts.cost, Cost::High);
        assert_eq!(facts.rate.cost, facts.cost);
        assert_eq!(facts.rate.poll_interval, Some(POLL));
        assert!(facts.rate.bytes_per_second.is_some());
    }
}

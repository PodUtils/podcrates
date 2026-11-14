//! What the caller gives a connection to a mailbox: the application and the
//! client it names, the way out, and the bounds.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Events, NoEvents};
use pod_ws::{Limits, Trust, Way};

use crate::phrase::WORDS;

/// How long a mailbox may take to answer a command.
pub const ANSWER_LIMIT: Duration = Duration::from_secs(30);
/// How long the dial of a mailbox, its TLS and its upgrade may take, each.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// The largest message that a mailbox may send: a message of the protocol
/// is a few hundred bytes, and the net's documents go in parts.
pub const LIMITS: Limits = Limits { message: 1 << 20, frame: pod_ws::frame::FORWARD_MAX_FRAME };

/// The settings of each connection to a mailbox. Nothing is read from the
/// environment.
#[derive(Clone)]
pub struct Config {
    /// The application, as magic-wormhole asks each to name one: the
    /// nameplates and the mailboxes of another application never meet it,
    /// and SPAKE2 takes it as its identity.
    pub appid: String,
    /// The client's name and version, as the mailbox's `client_version`.
    pub client_name: String,
    pub client_version: String,
    /// The way out of each connection.
    pub way: Way,
    /// The roots that a `wss://` mailbox is checked against.
    pub trust: Trust,
    /// The bound of each step of a dial ([`CONNECT_TIMEOUT`]).
    pub connect_timeout: Duration,
    /// The bound of the answer to a command ([`ANSWER_LIMIT`]).
    pub answer_limit: Duration,
    /// The largest frame and message of a mailbox ([`LIMITS`]).
    pub limits: Limits,
    /// The words of a new code after its nameplate ([`WORDS`]), as
    /// magic-wormhole's `--code-length`: [`crate::phrase::check_words`] holds it.
    /// A taker reads the count from the code.
    pub words: usize,
    /// Where the lines for the user go: the words of a mailbox, a mailbox
    /// that failed. No event holds the code.
    pub events: Arc<dyn Events>,
}

impl Config {
    /// The settings for `appid`, the client `name` and `version`, through
    /// `way`, with the roots of the default trust, the bounds above, codes
    /// of [`WORDS`] words and no event sink.
    pub fn new(
        appid: impl Into<String>,
        client_name: impl Into<String>,
        client_version: impl Into<String>,
        way: Way,
    ) -> Config {
        Config {
            appid: appid.into(),
            client_name: client_name.into(),
            client_version: client_version.into(),
            way,
            trust: Trust::Default,
            connect_timeout: CONNECT_TIMEOUT,
            answer_limit: ANSWER_LIMIT,
            limits: LIMITS,
            words: WORDS,
            events: Arc::new(NoEvents),
        }
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let client = &self.client_name;
        f.debug_struct("Config")
            .field("appid", &self.appid)
            .field("client", client)
            .field("words", &self.words)
            .finish()
    }
}

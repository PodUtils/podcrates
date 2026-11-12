//! The client of a magic-wormhole mailbox, for a rendezvous by a short code:
//! a person reads a code to another, and the two ends meet at a mailbox, a
//! WebSocket that needs no account, and move what they must with no secret
//! that the mailbox sees.
//!
//! * [`mailbox`]: the mailbox's server protocol of version 1 (JSON in text
//!   messages): `bind`, `list`, `allocate`, `claim`, `open`, `add`,
//!   `release` and `close`, over a WebSocket of `pod-ws` through the
//!   caller's way out. The mailbox stores and forwards.
//! * [`wormhole`]: the client protocol: the two ends run SPAKE2 keyed by the
//!   whole code over the mailbox, then seal each later message in NaCl's
//!   SecretBox under a key of the shared key, as magic-wormhole's own
//!   client does, so that a peer of its Python library and this crate meet.
//!   The mailbox learns nothing of the code and can change nothing, and an
//!   attacker who claims the code's nameplate gets one guess of its 24
//!   bits: the giver ends the code at the first exchange, whatever its end.
//! * [`meet`]: the giver's new code and the taker's search for its giver,
//!   over a list of mailboxes.
//! * [`mqtt`]: a second meeting point for a caller whose mailboxes do not
//!   answer: the topic of the code at a public MQTT broker, MQTT 3.1.1 over
//!   a WebSocket, with the same messages as a mailbox carries; [`place`]:
//!   the mailbox or the broker under a [`Wormhole`].
//! * [`phrase`] and [`words`]: the code, the nameplate and the words of the
//!   PGP list; [`message`]: the pure reading of a mailbox's messages.
//!
//! **The names are the caller's.** The AppID (nameplates and mailboxes of
//! another application never meet it), the client's name and version, the
//! mailbox URLs and the way out are a [`Config`] and a list of URLs: the
//! crate reads no variable of the environment, keeps no process state, and
//! prints nothing. The lines for the user are `pod_core::Event`s of the
//! kinds `wormhole.mailbox`, `wormhole.broker` and `wormhole.motd`; none
//! holds the code.
//!
//! **Typed failures.** [`Failed`] keeps its variants, which a caller
//! matches, and is also a `pod_core::Failure` with its stage and its class.
//!
//! **A code is a credential.** No `Debug` shows its words, and its text is
//! wiped on drop.

pub mod config;
pub mod failed;
pub mod mailbox;
pub mod meet;
pub mod message;
pub mod mqtt;
pub mod phrase;
pub mod place;
pub mod words;
pub mod wormhole;

pub use config::Config;
pub use failed::Failed;
pub use meet::{Ends, joined, opened};
pub use message::{hex, unhex};
pub use phrase::{Code, WORDS};
pub use wormhole::Wormhole;

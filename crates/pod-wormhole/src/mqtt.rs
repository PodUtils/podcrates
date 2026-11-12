//! A second meeting point of the two ends of a code, for a caller whose
//! mailboxes do not answer: a topic of a public MQTT broker, MQTT 3.1.1
//! over a WebSocket (subprotocol `mqtt`) on port 443, with no account.
//!
//! * The giver draws a nameplate of [`NAMEPLATE_DIGITS`] digits itself (no
//!   mailbox allocates one) and makes the code. The topic of the code is
//!   [`topic`]: letters and digits from the AppID and the nameplate, never
//!   from the words, as each subscriber of a public broker can read it.
//! * The giver (the waiting end) subscribes at the first broker that
//!   answers and waits; the taker subscribes at each broker in order and
//!   publishes first. Each end reads the topic of the code with the tag of
//!   its direction ([`Role::topics`]).
//! * A message at QoS 0 that reaches no subscriber is gone: the taker sends
//!   its messages again each [`Brokers::resend`] until the giver answers
//!   ([`broker`]).
//! * The ends say the messages of the mailbox ([`crate::wormhole`]): SPAKE2
//!   keyed by the whole code, then sealed phases. A stranger on the topic
//!   gets one guess of the words, as at a mailbox, and the broker sees
//!   ciphertext.
//!
//! The brokers and their public logins are the caller's
//! (`pod-services` has the public ones, of the kind `mqtt-broker`).

pub mod broker;
pub mod packet;

use std::fmt;
use std::time::Duration;

use pod_core::Event;
use pod_core::entropy::{Entropy, NoRandom, OsEntropy};
use pod_ws::url::WsUrl;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use broker::Broker;

use crate::config::Config;
use crate::mailbox::Mood;
use crate::meet::{new_side, reason};
use crate::message::hex;
use crate::wormhole::Wormhole;
use crate::{Code, Failed};

/// The subprotocol of MQTT over a WebSocket (section 6 of the standard).
pub const PROTOCOL: &str = "mqtt";
/// The keep-alive of each `CONNECT`.
pub const KEEP_ALIVE: Duration = Duration::from_secs(60);
/// How often the taker sends its messages again until the giver answers.
pub const RESEND: Duration = Duration::from_secs(2);
/// The digits of a nameplate that the giver draws: 1000 to 9999.
pub const NAMEPLATE_DIGITS: usize = 4;
/// The largest payload of a `PUBLISH`: shiftr.io takes 64 KB.
pub const PAYLOAD_MOST: usize = 64_000;
/// The tags of direction after the topic of a code: to the giver, to the
/// taker.
pub const TO_GIVER: &str = "g";
pub const TO_TAKER: &str = "t";
/// The label of the hash of a topic.
const TOPIC_LABEL: &[u8] = b"pod-wormhole mqtt topic v1";
/// The hex digits of the hash in a topic.
const TOPIC_DIGITS: usize = 40;

/// The login of a broker: a user name and a password in the `CONNECT`.
#[derive(Clone, PartialEq, Eq)]
pub struct Login {
    pub user: String,
    pub password: String,
}

impl fmt::Debug for Login {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A password of a broker of the user's own is a credential.
        f.debug_struct("Login").field("user", &self.user).finish_non_exhaustive()
    }
}

/// A broker, as the caller names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    /// `wss://HOST/PATH` (or `ws://` on the loopback of a test).
    pub url: WsUrl,
    pub login: Option<Login>,
}

/// Where the ends meet over MQTT, and how they are reached.
pub struct Brokers {
    /// The brokers, in order.
    pub points: Vec<Point>,
    pub config: Config,
    /// How often the taker sends its messages again ([`RESEND`]).
    pub resend: Duration,
}

impl Brokers {
    /// The brokers `points` with `config`, the taker sending again each
    /// [`RESEND`].
    pub fn new(points: Vec<Point>, config: Config) -> Brokers {
        Brokers { points, config, resend: RESEND }
    }
}

/// Which end a connection is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The end that made the code and waits.
    Giver,
    /// The end that typed the code and publishes first.
    Taker,
}

impl Role {
    /// The topic that this end reads and the one that it writes: the topic
    /// of the code with the tag of each direction.
    pub fn topics(self, topic: &str) -> (String, String) {
        let (to_giver, to_taker) = (format!("{topic}{TO_GIVER}"), format!("{topic}{TO_TAKER}"));
        match self {
            Role::Giver => (to_giver, to_taker),
            Role::Taker => (to_taker, to_giver),
        }
    }
}

/// The topic of the code of `nameplate` for `appid`: `pw` and 40 hex
/// digits of SHA-256 over both. A code of another nameplate, or of another
/// application, has a topic of its own. The words of the code are not in
/// it: each subscriber of the broker sees the topic, and 16 or 24 bits of
/// words under a hash are found by trial.
pub fn topic(appid: &str, nameplate: &str) -> String {
    let mut hash = Sha256::new();
    for part in [TOPIC_LABEL, appid.as_bytes(), nameplate.as_bytes()] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    let digits = hex(&hash.finalize());
    format!("pw{}", &digits[..TOPIC_DIGITS])
}

/// A nameplate of [`NAMEPLATE_DIGITS`] digits, with no zero first, from
/// `entropy`, with no bias.
pub fn nameplate(entropy: &mut dyn Entropy) -> Result<String, NoRandom> {
    const LOW: u16 = 1000;
    const COUNT: u16 = 9000;
    // The largest multiple of the count under 2^16: a draw past it is
    // drawn again, so that each nameplate is as likely as each other.
    const FAIR: u16 = COUNT * (u16::MAX / COUNT);
    loop {
        let mut bytes = [0u8; 2];
        entropy.fill(&mut bytes)?;
        let drawn = u16::from_be_bytes(bytes);
        if drawn < FAIR {
            return Ok((LOW + drawn % COUNT).to_string());
        }
    }
}

/// A new code, and its wormhole at the first broker of `brokers` that
/// answers, subscribed and waiting for the taker: what a giver shows when
/// no mailbox answers. `versions` goes in its version phase.
pub async fn opened(brokers: &Brokers, versions: Value) -> Result<(Wormhole, Code), Failed> {
    let side = new_side()?;
    let plate = nameplate(&mut OsEntropy).map_err(|_| Failed::NoRandom)?;
    let code = Code::make(&plate, &mut OsEntropy).map_err(|_| Failed::NoRandom)?;
    let topic = topic(&brokers.config.appid, &plate);
    let mut missed = Vec::new();
    for point in &brokers.points {
        let connected = Broker::connect(point, &brokers.config, &side, &topic, Role::Giver, brokers.resend).await;
        let broker = match connected {
            Ok(broker) => broker,
            Err(why) => {
                say(brokers, Event::warn("wormhole.broker", reason(&why)));
                missed.push(reason(&why));
                continue;
            }
        };
        let hole = Wormhole::open(broker, side, plate.clone(), topic, &code, versions).await?;
        return Ok((hole, code));
    }
    Err(Failed::Unreachable(missed.join("; ")))
}

/// The wormhole of `code` at the broker where its giver waits, the key
/// made: at each broker of `brokers` in order, the taker's first message
/// published and sent again until the giver answers within `giver`.
pub async fn joined(brokers: &Brokers, code: &Code, versions: Value, giver: Duration) -> Result<Wormhole, Failed> {
    let side = new_side()?;
    let topic = topic(&brokers.config.appid, code.nameplate());
    let (mut missed, mut reached) = (Vec::new(), false);
    for point in &brokers.points {
        let connected = Broker::connect(point, &brokers.config, &side, &topic, Role::Taker, brokers.resend).await;
        let broker = match connected {
            Ok(broker) => broker,
            Err(why) => {
                say(brokers, Event::warn("wormhole.broker", reason(&why)));
                missed.push(reason(&why));
                continue;
            }
        };
        reached = true;
        let nameplate = code.nameplate().to_string();
        let mut hole = Wormhole::open(broker, side.clone(), nameplate, topic.clone(), code, versions.clone()).await?;
        if hole.agree(giver).await? {
            return Ok(hole);
        }
        hole.close(Mood::Lonely).await;
    }
    if reached { Err(Failed::Nobody) } else { Err(Failed::Unreachable(missed.join("; "))) }
}

fn say(brokers: &Brokers, event: Event) {
    brokers.config.events.event(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_topic_is_letters_and_digits_of_the_appid_and_the_nameplate() {
        let one = topic("example.org/app", "1234");
        assert_eq!(one.len(), 2 + TOPIC_DIGITS);
        assert!(one.bytes().all(|b| b.is_ascii_alphanumeric()), "{one}");
        assert_eq!(one, topic("example.org/app", "1234"), "both ends derive the same topic");
        assert_ne!(one, topic("example.org/app", "1235"), "another nameplate");
        assert_ne!(one, topic("example.org/other", "1234"), "another application");
        assert_ne!(topic("ab", "1"), topic("a", "b1"), "the parts do not run together");
        // shiftr.io takes topics of 128 characters; each tag adds one.
        let (inbox, outbox) = Role::Giver.topics(&one);
        assert!(inbox.len() <= 128 && inbox != outbox);
        assert_eq!(Role::Taker.topics(&one), (outbox, inbox));
    }

    struct Counting(u16);

    impl Entropy for Counting {
        fn fill(&mut self, bytes: &mut [u8]) -> Result<(), NoRandom> {
            bytes.copy_from_slice(&self.0.to_be_bytes());
            self.0 = self.0.wrapping_add(1);
            Ok(())
        }
    }

    #[test]
    fn a_nameplate_is_four_digits_with_no_bias() {
        assert_eq!(nameplate(&mut Counting(0)).unwrap(), "1000");
        assert_eq!(nameplate(&mut Counting(8999)).unwrap(), "9999");
        assert_eq!(nameplate(&mut Counting(9000)).unwrap(), "1000");
        // A draw past the last whole round of 9000 is drawn again.
        assert_eq!(nameplate(&mut Counting(u16::MAX)).unwrap(), "1000", "65535, then 0");
        for _ in 0..200 {
            let plate = nameplate(&mut OsEntropy).unwrap();
            assert!(plate.len() == NAMEPLATE_DIGITS && crate::phrase::nameplate_ok(&plate), "{plate}");
        }
    }

    #[test]
    fn no_debug_shows_a_login_password() {
        let login = Login { user: "public".into(), password: "SECRET".into() };
        let shown = format!("{login:?}");
        assert!(shown.contains("public") && !shown.contains("SECRET"), "{shown}");
    }
}

//! A meeting point of the two ends of a code at a public Nostr relay, for
//! a caller whose mailboxes do not answer: events of one kind of the
//! ephemeral range (NIP-01: 20000 to 29999, which a relay passes to the
//! subscribers of the moment and does not store for long), over a WebSocket
//! on port 443, with no account.
//!
//! * The giver draws a nameplate of [`crate::mqtt::NAMEPLATE_DIGITS`] digits
//!   ([`crate::mqtt::nameplate`]) and makes the code. The tag of the code is
//!   [`crate::mqtt::topic`] of the AppID and the nameplate, never of the words, as
//!   each client of a public relay can read each event. Each end reads the
//!   tag with the letter of its direction ([`Role::topics`]): one
//!   single-letter tag [`TAG`], which each relay indexes.
//! * Each event is signed (BIP-340 Schnorr over secp256k1) by a key drawn
//!   for this pairing alone ([`event::Key`]): no key links two pairings.
//! * The giver (the waiting end) subscribes at the first relay that
//!   answers and waits; the taker subscribes at each relay in order and
//!   sends first, again each [`Relays::resend`] until the giver answers
//!   ([`relay`]).
//! * The ends say the messages of the mailbox ([`crate::wormhole`]) as the
//!   content of the events: SPAKE2 keyed by the whole code, then sealed
//!   phases. A stranger at the tag gets one guess of the words, as at a
//!   mailbox, and the relay sees ciphertext.
//!
//! The relays are the caller's (`pod-services` has the public ones, of the
//! kind `nostr-relay`).

pub mod event;
pub mod relay;

use std::time::Duration;

use pod_core::Event;
use pod_core::entropy::OsEntropy;
use pod_ws::url::WsUrl;
use serde_json::Value;

pub use relay::Relay;

use crate::config::Config;
use crate::mailbox::Mood;
use crate::meet::{new_side, reason};
use crate::mqtt::{Role, nameplate, topic};
use crate::wormhole::Wormhole;
use crate::{Code, Failed, Missed};

/// The kind of each event: of the ephemeral range, and of no NIP of
/// `nostr-protocol/nips` (its list of kinds).
pub const KIND: u16 = 25431;
/// The one tag of each event, which holds the tag of the code and its
/// direction: NIP-01 asks each relay to index each single-letter tag.
pub const TAG: &str = "t";
/// How often the taker sends its messages again until the giver answers.
pub const RESEND: Duration = Duration::from_secs(2);
/// How far before this end's start the subscription reads: the clocks of
/// two hosts differ. strfry refuses an ephemeral event older than 60 s.
pub const SINCE_SLACK: Duration = Duration::from_secs(60);
/// The largest content of an event: strfry takes an event of 65,536 bytes
/// of JSON, and the fields around the content take less than 600.
pub const CONTENT_MOST: usize = 64_000;

/// Where the ends meet over Nostr, and how they are reached.
pub struct Relays {
    /// The relays (`wss://HOST/`, or `ws://` on the loopback of a test), in
    /// order.
    pub points: Vec<WsUrl>,
    pub config: Config,
    /// How often the taker sends its messages again ([`RESEND`]).
    pub resend: Duration,
}

impl Relays {
    /// The relays `points` with `config`, the taker sending again each
    /// [`RESEND`].
    pub fn new(points: Vec<WsUrl>, config: Config) -> Relays {
        Relays { points, config, resend: RESEND }
    }
}

/// A new code, and its wormhole at the first relay of `relays` that
/// answers, subscribed and waiting for the taker: what a giver shows when
/// no mailbox answers. `versions` goes in its version phase.
pub async fn opened(relays: &Relays, versions: Value) -> Result<(Wormhole, Code), Failed> {
    let side = new_side()?;
    let plate = nameplate(&mut OsEntropy).map_err(|_| Failed::NoRandom)?;
    // A count of words out of its bounds fails here, before a relay.
    let code = Code::make(&plate, relays.config.words, &mut OsEntropy)?;
    let tag = topic(&relays.config.appid, &plate);
    let mut missed = Vec::new();
    for url in &relays.points {
        let relay = match Relay::connect(url, &relays.config, &side, &tag, Role::Giver, relays.resend).await {
            Ok(relay) => relay,
            Err(why) => {
                say(relays, Event::warn("wormhole.nostr", reason(&why)));
                missed.push(Missed::of(why));
                continue;
            }
        };
        let hole = Wormhole::open(relay, side, plate.clone(), tag, &code, versions).await?;
        return Ok((hole, code));
    }
    Err(Failed::Unreachable(Missed::join(missed)))
}

/// The wormhole of `code` at the relay where its giver waits, the key
/// made: at each relay of `relays` in order, the taker's first message sent
/// and sent again until the giver answers within `giver`.
pub async fn joined(relays: &Relays, code: &Code, versions: Value, giver: Duration) -> Result<Wormhole, Failed> {
    let side = new_side()?;
    let tag = topic(&relays.config.appid, code.nameplate());
    let (mut missed, mut reached) = (Vec::new(), false);
    for url in &relays.points {
        let relay = match Relay::connect(url, &relays.config, &side, &tag, Role::Taker, relays.resend).await {
            Ok(relay) => relay,
            Err(why) => {
                say(relays, Event::warn("wormhole.nostr", reason(&why)));
                missed.push(Missed::of(why));
                continue;
            }
        };
        reached = true;
        let nameplate = code.nameplate().to_string();
        let mut hole = Wormhole::open(relay, side.clone(), nameplate, tag.clone(), code, versions.clone()).await?;
        if hole.agree(giver).await? {
            return Ok(hole);
        }
        hole.close(Mood::Lonely).await;
    }
    if reached { Err(Failed::Nobody) } else { Err(Failed::Unreachable(Missed::join(missed))) }
}

/// The seconds since the Unix epoch, as `created_at` and `since` count.
pub(crate) fn unix_now() -> u64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map(|d| d.as_secs()).unwrap_or(0)
}

fn say(relays: &Relays, event: Event) {
    relays.config.events.event(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mqtt;

    #[test]
    fn the_kind_is_ephemeral_and_the_tag_is_one_letter() {
        assert!((20_000..30_000).contains(&KIND), "NIP-01's ephemeral range");
        assert!(TAG.len() == 1 && TAG.bytes().all(|b| b.is_ascii_alphabetic()));
        assert!(SINCE_SLACK <= Duration::from_secs(60));
    }

    #[test]
    fn the_tag_of_a_code_is_the_topic_of_its_nameplate_with_its_direction() {
        let (inbox, outbox) = Role::Giver.topics(&topic("example.org/app", "1234"));
        assert!(inbox.ends_with(mqtt::TO_GIVER) && outbox.ends_with(mqtt::TO_TAKER));
        assert!(inbox.bytes().all(|b| b.is_ascii_alphanumeric()), "{inbox}");
    }
}

//! A meeting point of the two ends of a code at a public WebTorrent
//! tracker, for a caller whose mailboxes and Nostr relays do not answer:
//! the swarm of one `info_hash`, over a WebSocket on port 443, with no
//! account. A tracker passes the offers and the answers of WebRTC between
//! the peers of a swarm and reads no field of their `sdp`.
//!
//! * The giver draws a nameplate of [`crate::mqtt::NAMEPLATE_DIGITS`]
//!   digits ([`crate::mqtt::nameplate`]) and makes the code. The
//!   `info_hash` is [`info_hash`]: 20 hex digits of the hash of
//!   [`crate::mqtt::topic`] of the AppID and the nameplate, never of the
//!   words, as each peer of the swarm can read it. Each end draws a new
//!   `peer_id` for each pairing.
//! * The giver (the waiting end) announces with no offer at the first
//!   tracker that answers, again each interval of the tracker, and waits.
//!   The taker announces at each tracker in order with its messages as an
//!   offer, and again each [`Trackers::resend`]. A tracker sends an offer
//!   at once to a peer of the swarm and keeps no copy: an offer that
//!   reaches no peer is gone, and the next one carries the messages again.
//! * The giver can speak only in an answer to an offer: it answers each
//!   offer of the taker with its messages that the taker does not have.
//!   Each offer and each answer holds a [`wire::Note`]: the tag of the
//!   code, the side, the messages from an index, and the count of the
//!   other end's messages that the sender has ([`link`]).
//! * The ends say the messages of the mailbox ([`crate::wormhole`]):
//!   SPAKE2 keyed by the whole code, then sealed phases. A stranger in the
//!   swarm gets one guess of the words, as at a mailbox, and the tracker
//!   sees ciphertext.
//!
//! The trackers are the caller's (`pod-services` has the public ones, of
//! the kind `webtorrent-tracker`).

pub mod link;
pub mod wire;

use std::time::Duration;

use pod_core::Event;
use pod_core::entropy::OsEntropy;
use pod_ws::url::WsUrl;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use link::Tracker;

use crate::config::Config;
use crate::mailbox::Mood;
use crate::meet::{new_side, reason};
use crate::message::hex;
use crate::mqtt::{Role, nameplate, topic};
use crate::wormhole::Wormhole;
use crate::{Code, Failed, Missed};

/// How often the taker sends its offer again until the giver answers, and
/// asks for the giver's next messages meanwhile.
pub const RESEND: Duration = Duration::from_secs(2);
/// The largest `sdp` of an offer or an answer: tracker.webtorrent.dev
/// carried one of 12,000 bytes and not one of 14,000.
pub const SDP_MOST: usize = 10_000;
/// The bounds of the giver's announces again: the tracker's interval,
/// which bittorrent-tracker sets to 120 s on a WebSocket, within these.
pub const ANNOUNCE_LEAST: Duration = Duration::from_secs(1);
pub const ANNOUNCE_MOST: Duration = Duration::from_secs(120);
/// How long the giver's close answers the taker's offers until the taker
/// has each of its messages: the taker offers each [`RESEND`].
pub const LINGER: Duration = Duration::from_secs(10);
/// The label of the hash of an `info_hash`.
const INFO_HASH_LABEL: &[u8] = b"pod-wormhole tracker info_hash v1";

/// Where the ends meet at WebTorrent trackers, and how they are reached.
pub struct Trackers {
    /// The trackers (`wss://HOST/`, or `ws://` on the loopback of a test),
    /// in order.
    pub points: Vec<WsUrl>,
    pub config: Config,
    /// How often the taker sends its offer again ([`RESEND`]).
    pub resend: Duration,
}

impl Trackers {
    /// The trackers `points` with `config`, the taker sending again each
    /// [`RESEND`].
    pub fn new(points: Vec<WsUrl>, config: Config) -> Trackers {
        Trackers { points, config, resend: RESEND }
    }
}

/// The `info_hash` of the code whose tag is `tag` ([`topic`]): 20 hex
/// digits of SHA-256 over a label and the tag. The swarm of a code of
/// another nameplate or of another application is another one.
pub fn info_hash(tag: &str) -> String {
    let mut hash = Sha256::new();
    for part in [INFO_HASH_LABEL, tag.as_bytes()] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    hex(&hash.finalize())[..wire::ID_CHARS].to_string()
}

/// A new code, and its wormhole at the first tracker of `trackers` that
/// answers, announced and waiting for the taker: what a giver shows when
/// no mailbox answers. `versions` goes in its version phase.
pub async fn opened(trackers: &Trackers, versions: Value) -> Result<(Wormhole, Code), Failed> {
    let side = new_side()?;
    let plate = nameplate(&mut OsEntropy).map_err(|_| Failed::NoRandom)?;
    // A count of words out of its bounds fails here, before a tracker.
    let code = Code::make(&plate, trackers.config.words, &mut OsEntropy)?;
    let tag = topic(&trackers.config.appid, &plate);
    let mut missed = Vec::new();
    for url in &trackers.points {
        let tracker = match Tracker::connect(url, &trackers.config, &side, &tag, Role::Giver, trackers.resend).await {
            Ok(tracker) => tracker,
            Err(why) => {
                say(trackers, Event::warn("wormhole.tracker", reason(&why)));
                missed.push(Missed::of(why));
                continue;
            }
        };
        let hole = Wormhole::open(tracker, side, plate.clone(), tag, &code, versions).await?;
        return Ok((hole, code));
    }
    Err(Failed::Unreachable(Missed::join(missed)))
}

/// The wormhole of `code` at the tracker where its giver waits, the key
/// made: at each tracker of `trackers` in order, the taker's first message
/// offered and offered again until the giver answers within `giver`.
pub async fn joined(trackers: &Trackers, code: &Code, versions: Value, giver: Duration) -> Result<Wormhole, Failed> {
    let side = new_side()?;
    let tag = topic(&trackers.config.appid, code.nameplate());
    let (mut missed, mut reached) = (Vec::new(), false);
    for url in &trackers.points {
        let tracker = match Tracker::connect(url, &trackers.config, &side, &tag, Role::Taker, trackers.resend).await {
            Ok(tracker) => tracker,
            Err(why) => {
                say(trackers, Event::warn("wormhole.tracker", reason(&why)));
                missed.push(Missed::of(why));
                continue;
            }
        };
        reached = true;
        let nameplate = code.nameplate().to_string();
        let mut hole = Wormhole::open(tracker, side.clone(), nameplate, tag.clone(), code, versions.clone()).await?;
        if hole.agree(giver).await? {
            return Ok(hole);
        }
        hole.close(Mood::Lonely).await;
    }
    if reached { Err(Failed::Nobody) } else { Err(Failed::Unreachable(Missed::join(missed))) }
}

fn say(trackers: &Trackers, event: Event) {
    trackers.config.events.event(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_info_hash_is_20_hex_digits_of_the_tag_alone() {
        let one = info_hash(&topic("example.org/app", "1234"));
        assert_eq!(one.len(), wire::ID_CHARS);
        assert!(one.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{one}");
        assert_eq!(one, info_hash(&topic("example.org/app", "1234")));
        assert_ne!(one, info_hash(&topic("example.org/app", "1235")));
        assert_ne!(one, info_hash(&topic("example.org/other", "1234")));
        let tag = topic("example.org/app", "1234");
        assert!(!tag.contains(&one), "the info_hash is no part of the topic of a broker");
    }

    #[test]
    fn the_bounds_of_an_announce_again_hold_the_interval_of_bittorrent_tracker() {
        assert!(ANNOUNCE_LEAST < RESEND && Duration::from_secs(120) <= ANNOUNCE_MOST);
        assert!(LINGER >= RESEND * 4, "the giver's close sees four offers of the taker");
        const { assert!(SDP_MOST < 12_000, "tracker.webtorrent.dev carried 12,000 bytes") };
    }
}

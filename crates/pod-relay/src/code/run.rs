//! The two runs of a pairing by code (podssh T-086). The giver allocates a
//! nameplate at the first mailbox that answers, makes the code, and waits
//! for one taker within the code's life. The taker looks for its giver at
//! each mailbox in order: one that lists the code's nameplate has it, one
//! that lists others does not, and at one that lists none the giver's
//! `pake`, which the mailbox keeps, comes at once or not at all. Each end
//! makes the key, checks the other's version, and moves its part. The giver
//! leaves after one exchange, whatever its end, so that a wrong code, a
//! guess or a success spends the code.
//!
//! Where no mailbox answers, the ends meet at the next places of [`Ends`]:
//! the ntfy servers (T-554, PodUtils/podssh#237), then in the order of
//! podssh's ruling Q77 the Nostr relays (T-550), the WebTorrent trackers
//! (T-551), then the MQTT brokers (podssh T-737). Where the caller's probe
//! found that the way out refuses the WebSocket upgrade
//! ([`Ends::upgrade_refused`]), the ntfy servers come first, as their
//! requests are of plain HTTPS. The giver goes on after each place of a
//! kind failed to answer; the taker goes on after it found no giver there
//! too (call C115). Each kind but the mailboxes starts with an event that a
//! third party carries the messages ([`NTFY_WORDS`], [`NOSTR_WORDS`],
//! [`TRACKER_WORDS`], [`BROKER_WORDS`]).
//!
//! The runs are one for each [`Kind`] of exchange: [`give_with`] and
//! [`take_with`] move an offer, an answer and the giver's last words, as
//! bytes, and the kind's own words are its caller's. The pairing's
//! ([`give`], [`take`]) is one; the net's invitation (podssh T-616), whose words
//! the binary reads, another.

use std::time::Duration;

use pod_core::Event;
use pod_identity::PublicKey;
use pod_wormhole::Missed;
use pod_wormhole::meet::or_close;
use pod_wormhole::mqtt::{self, Brokers, Point};
use pod_wormhole::nostr::{self, Relays};
use pod_wormhole::ntfy::{self, Servers};
use pod_wormhole::tracker::{self, Trackers};
use pod_ws::url::WsUrl;
use serde_json::Value;

use super::exchange::{self, Answer, Done, Kind, Offer};
use super::mailbox::Mood;
use super::wormhole::Wormhole;
use super::{Code, Failed, LIFETIME};

/// Where the ends meet, and how they are reached.
pub struct Ends {
    /// The mailboxes, in order.
    pub mailboxes: Vec<WsUrl>,
    /// The ntfy servers, in order, after the mailboxes; none: no ntfy.
    pub ntfy: Vec<ntfy::Point>,
    /// The caller's probe found that the way out refuses the WebSocket
    /// upgrade: the ntfy servers come before the mailboxes.
    pub upgrade_refused: bool,
    /// The Nostr relays, in order, after the ntfy servers; none: no Nostr.
    pub relays: Vec<WsUrl>,
    /// The WebTorrent trackers, in order, after the relays; none: no
    /// tracker.
    pub trackers: Vec<WsUrl>,
    /// The MQTT brokers, in order, after the trackers; none: no MQTT.
    pub brokers: Vec<Point>,
    /// How often a taker at a relay, a tracker or a broker sends its
    /// messages again ([`mqtt::RESEND`], [`nostr::RESEND`],
    /// [`tracker::RESEND`]).
    pub resend: Duration,
    /// How often an end at an ntfy server polls on HTTP/1.1.
    pub pace: ntfy::Pace,
    /// The caller's AppID, name, way out, trust and events.
    pub config: pod_wormhole::Config,
}

impl Ends {
    /// The ends at `mailboxes` alone, with no ntfy server, no relay, no
    /// tracker and no broker.
    pub fn new(mailboxes: Vec<WsUrl>, config: pod_wormhole::Config) -> Ends {
        let (ntfy, relays, trackers, brokers) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let upgrade_refused = false;
        let (resend, pace) = (mqtt::RESEND, ntfy::Pace::default());
        Ends { mailboxes, ntfy, upgrade_refused, relays, trackers, brokers, resend, pace, config }
    }

    /// The ends as `pod-wormhole` takes them.
    pub fn wormhole(&self) -> pod_wormhole::Ends {
        pod_wormhole::Ends { mailboxes: self.mailboxes.clone(), config: self.config.clone() }
    }

    /// The ntfy servers as `pod_wormhole::ntfy` takes them.
    pub fn ntfy(&self) -> Servers {
        Servers { pace: self.pace, ..Servers::new(self.ntfy.clone(), self.config.clone()) }
    }

    /// The relays as `pod_wormhole::nostr` takes them.
    pub fn relays(&self) -> Relays {
        Relays { points: self.relays.clone(), config: self.config.clone(), resend: self.resend }
    }

    /// The trackers as `pod_wormhole::tracker` takes them.
    pub fn trackers(&self) -> Trackers {
        Trackers { points: self.trackers.clone(), config: self.config.clone(), resend: self.resend }
    }

    /// The brokers as `pod_wormhole::mqtt` takes them.
    pub fn brokers(&self) -> Brokers {
        Brokers { points: self.brokers.clone(), config: self.config.clone(), resend: self.resend }
    }

    /// Whether the ntfy servers come before the mailboxes.
    fn ntfy_first(&self) -> bool {
        self.upgrade_refused && !self.ntfy.is_empty()
    }

    /// The places after the mailboxes, in order, each with its event:
    /// the ntfy servers where they do not come first, then the relays, the
    /// trackers and the brokers (podssh's ruling Q77), those that the
    /// caller gave.
    fn next_places(&self) -> Vec<Next> {
        let all = [
            (Next::Ntfy, !self.ntfy.is_empty() && !self.ntfy_first()),
            (Next::Relays, !self.relays.is_empty()),
            (Next::Trackers, !self.trackers.is_empty()),
            (Next::Brokers, !self.brokers.is_empty()),
        ];
        all.into_iter().filter(|(_, given)| *given).map(|(next, _)| next).collect()
    }

    /// The event before the first place of `next`.
    fn say_next(&self, next: Next, why: &str) {
        let (kind, words) = match next {
            Next::Ntfy => ("relay.code.ntfy", NTFY_WORDS),
            Next::Relays => ("relay.code.nostr", NOSTR_WORDS),
            Next::Trackers => ("relay.code.tracker", TRACKER_WORDS),
            Next::Brokers => ("relay.code.broker", BROKER_WORDS),
        };
        self.config.events.event(Event::warn(kind, format!("{why}; {words}")));
    }
}

/// A kind of place after the mailboxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    Ntfy,
    Relays,
    Trackers,
    Brokers,
}

impl Next {
    /// The noun of a place of the kind, for the words of an event.
    fn noun(self) -> &'static str {
        match self {
            Next::Ntfy => "ntfy server",
            Next::Relays => "Nostr relay",
            Next::Trackers => "WebTorrent tracker",
            Next::Brokers => "broker",
        }
    }
}

/// What the event before the first ntfy server says.
pub const NTFY_WORDS: &str = "the next meeting point is a public ntfy server: a third party that nobody vouches \
     for carries the messages of the pairing, and its proof keeps the secret";

/// Why the ntfy servers come first, before [`NTFY_WORDS`].
pub const UPGRADE_REFUSED: &str = "the way out refuses the WebSocket upgrade";

/// What the event before the first Nostr relay says.
pub const NOSTR_WORDS: &str = "the next meeting point is a public Nostr relay: a third party that nobody vouches \
     for carries the messages of the pairing, and its proof keeps the secret";

/// What the event before the first WebTorrent tracker says.
pub const TRACKER_WORDS: &str = "the next meeting point is a public WebTorrent tracker: a third party that nobody \
     vouches for carries the messages of the pairing, and its proof keeps the secret";

/// What the event before the first broker says.
pub const BROKER_WORDS: &str = "the next meeting point is a public MQTT broker: a third party that nobody vouches \
     for carries the messages of the pairing, and its proof keeps the secret";

/// The bounds of a run; a test shortens them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waits {
    /// The code's life at the giver.
    pub life: Duration,
    /// The taker's wait for its giver's `pake`, which the mailbox keeps.
    pub giver: Duration,
    /// Each step of the exchange once the key is made.
    pub step: Duration,
}

impl Default for Waits {
    fn default() -> Self {
        Waits { life: LIFETIME, giver: Duration::from_secs(20), step: Duration::from_secs(60) }
    }
}

/// What the giver's run gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Given {
    /// The taker's key.
    pub operator: PublicKey,
    /// What the giver did with it.
    pub done: Done,
    /// The mailbox where they met, as lines show it.
    pub mailbox: String,
}

/// What the taker's run gave.
pub struct Taken {
    pub offer: Offer,
    pub done: Done,
    pub mailbox: String,
}

/// The most last words that a taker reads after its answer: a mailbox holds
/// 32 messages of one code, and the net's document goes in a dozen parts.
pub const LAST_MOST: usize = 24;

/// What a taker says to the giver's offer.
pub enum Reply {
    /// Its answer; the giver's last words follow.
    Answer(Vec<u8>),
    /// Its refusal, said to the giver so that it ends at once rather than
    /// wait out a step; the run then ends with this failure.
    Refused(Vec<u8>, Failed),
}

/// The giver's run: a new code at the first mailbox that answers, given to
/// `show` with that mailbox, for a terminal alone; then one taker within
/// `waits.life`, which gets `offer` and answers with its key, which `admit`
/// takes. The code is spent at the first exchange, whatever its end.
pub async fn give(
    ends: &Ends,
    waits: &Waits,
    offer: &Offer,
    show: &mut (dyn FnMut(&Code, &str) + Send),
    admit: &mut (dyn FnMut(&PublicKey) -> Done + Send),
) -> Result<Given, Failed> {
    let offered = offer.to_bytes();
    let mut taken = None;
    let mut answer = |bytes: &[u8]| -> Result<Vec<Vec<u8>>, Failed> {
        let theirs = Answer::from_bytes(bytes).map_err(Failed::Peer)?;
        let done = admit(&theirs.operator);
        let last = done.to_bytes();
        taken = Some((theirs.operator, done));
        Ok(vec![last])
    };
    let mailbox = give_with(ends, waits, exchange::PAIR, &offered, show, &mut answer).await?;
    let Some((operator, done)) = taken else { return Err(Failed::Broken("the exchange ended with no answer".into())) };
    Ok(Given { operator, done, mailbox })
}

/// The taker's run: the giver of `code`, its offer, which `check` takes or
/// refuses before this end says a word of its own, then `me`, this end's
/// key, and the giver's last word.
pub async fn take(
    ends: &Ends,
    waits: &Waits,
    code: &Code,
    me: &PublicKey,
    check: &mut (dyn FnMut(&Offer) -> Result<(), String> + Send),
) -> Result<Taken, Failed> {
    let mut offered = None;
    let mut reply = |bytes: &[u8]| -> Result<Reply, Failed> {
        let offer = Offer::from_bytes(bytes, now_ms()).map_err(Failed::Peer)?;
        check(&offer).map_err(Failed::Mine)?;
        offered = Some(offer);
        Ok(Reply::Answer(Answer { operator: *me }.to_bytes()))
    };
    let mut last = |bytes: &[u8]| -> Result<Option<Done>, String> { Done::from_bytes(bytes).map(Some) };
    let (done, mailbox) = take_with(ends, waits, exchange::PAIR, code, &mut reply, &mut last).await?;
    let Some(offer) = offered else { return Err(Failed::Broken("the exchange ended with no offer".into())) };
    Ok(Taken { offer, done, mailbox })
}

/// The giver's reading of the taker's answer: its last words, in order, or
/// the failure that ends the run.
pub type AnswerRead<'a> = dyn FnMut(&[u8]) -> Result<Vec<Vec<u8>>, Failed> + Send + 'a;

/// The taker's reading of one of the giver's last words: what the run gives,
/// once a word completes it.
pub type LastRead<'a, T> = dyn FnMut(&[u8]) -> Result<Option<T>, String> + Send + 'a;

/// The giver's run of an exchange of `kind`: a new code, given to `show`
/// with its mailbox, for a terminal alone; then one taker within
/// `waits.life`, whose version must be of `kind`, which gets `offer`. Its
/// answer goes to `answer`, which gives the last words, sent in order, or
/// the failure that ends the run. The code is spent at the first exchange,
/// whatever its end. The mailbox where they met, as lines show it.
pub async fn give_with(
    ends: &Ends,
    waits: &Waits,
    kind: Kind,
    offer: &[u8],
    show: &mut (dyn FnMut(&Code, &str) + Send),
    answer: &mut AnswerRead<'_>,
) -> Result<String, Failed> {
    let (mut hole, code) = opened(ends, kind.versions()).await?;
    show(&code, hole.shown());
    drop(code);
    let mailbox = hole.shown().to_string();
    if !hole.agree(waits.life).await? {
        hole.close(Mood::Lonely).await;
        return Err(Failed::Expired(waits.life));
    }
    let theirs = hole.confirm(waits.step).await?;
    if let Err(why) = kind.ours(&theirs) {
        return Err(hole.fail(Mood::Errory, Failed::Peer(why)).await);
    }
    let sent = hole.send(offer).await;
    or_close(&mut hole, sent).await?;
    let theirs = hole.receive(waits.step).await?;
    let words = match answer(&theirs) {
        Ok(words) => words,
        Err(failed) => return Err(hole.fail(Mood::Errory, failed).await),
    };
    for word in &words {
        let sent = hole.send(word).await;
        or_close(&mut hole, sent).await?;
    }
    hole.close(Mood::Happy).await;
    Ok(mailbox)
}

/// The taker's run of an exchange of `kind`: the giver of `code`, whose
/// version must be of `kind`; its offer, which `reply` answers or refuses
/// before this end says a word of its own; then the giver's last words, each
/// to `last`, until one gives what the run gave, at most [`LAST_MOST`]. That,
/// and the mailbox where they met.
pub async fn take_with<T>(
    ends: &Ends,
    waits: &Waits,
    kind: Kind,
    code: &Code,
    reply: &mut (dyn FnMut(&[u8]) -> Result<Reply, Failed> + Send),
    last: &mut LastRead<'_, T>,
) -> Result<(T, String), Failed> {
    let mut hole = joined(ends, code, kind.versions(), waits).await?;
    let mailbox = hole.shown().to_string();
    let theirs = hole.confirm(waits.step).await?;
    if let Err(why) = kind.ours(&theirs) {
        return Err(hole.fail(Mood::Errory, Failed::Peer(why)).await);
    }
    let offer = hole.receive(waits.step).await?;
    let said = match reply(&offer) {
        Ok(Reply::Answer(said)) => said,
        Ok(Reply::Refused(said, failed)) => {
            // The refusal is a courtesy to the giver: unsent, it waits a step.
            let _ = hole.send(&said).await;
            return Err(hole.fail(Mood::Errory, failed).await);
        }
        Err(failed) => return Err(hole.fail(Mood::Errory, failed).await),
    };
    let sent = hole.send(&said).await;
    or_close(&mut hole, sent).await?;
    for _ in 0..LAST_MOST {
        let word = hole.receive(waits.step).await?;
        match last(&word) {
            Ok(Some(done)) => {
                hole.close(Mood::Happy).await;
                return Ok((done, mailbox));
            }
            Ok(None) => {}
            Err(why) => return Err(hole.fail(Mood::Errory, Failed::Peer(why)).await),
        }
    }
    let why = format!("the other end said more than {LAST_MOST} last words");
    Err(hole.fail(Mood::Errory, Failed::Peer(why)).await)
}

/// A new code at the first mailbox of `ends` that answers, and its
/// wormhole, this end's `pake` added: what a giver shows. `versions` goes in
/// its version phase. Where no mailbox answers, the first ntfy server
/// that answers, then the first relay, the first tracker, then the first
/// broker; the ntfy servers first where the upgrade is refused.
pub async fn opened(ends: &Ends, versions: Value) -> Result<(Wormhole, Code), Failed> {
    let mut missed = Vec::new();
    if ends.ntfy_first() {
        ends.say_next(Next::Ntfy, UPGRADE_REFUSED);
        match ntfy::opened(&ends.ntfy(), versions.clone()).await {
            Err(Failed::Unreachable(why)) => missed.push(why),
            other => return other,
        }
    }
    match pod_wormhole::opened(&ends.wormhole(), versions.clone()).await {
        Err(Failed::Unreachable(why)) => missed.push(why),
        other => return other,
    };
    let mut noun = "mailbox";
    for next in ends.next_places() {
        ends.say_next(next, &format!("no {noun} answered"));
        let opened = match next {
            Next::Ntfy => ntfy::opened(&ends.ntfy(), versions.clone()).await,
            Next::Relays => nostr::opened(&ends.relays(), versions.clone()).await,
            Next::Trackers => tracker::opened(&ends.trackers(), versions.clone()).await,
            Next::Brokers => mqtt::opened(&ends.brokers(), versions.clone()).await,
        };
        match opened {
            Err(Failed::Unreachable(why)) => missed.push(why),
            other => return other,
        }
        noun = next.noun();
    }
    Err(Failed::Unreachable(Missed::join(missed)))
}

/// The wormhole of `code` at the mailbox where its giver waits, the key
/// made: each mailbox of `ends` in order, by its list of nameplates, and
/// where it lists none, by the giver's `pake` within `waits.giver`.
/// Where no mailbox has the giver, each ntfy server in order, then each
/// relay, each tracker, then each broker, by the giver's answer within
/// `waits.giver`; the ntfy servers first where the upgrade is refused.
pub async fn joined(ends: &Ends, code: &Code, versions: Value, waits: &Waits) -> Result<Wormhole, Failed> {
    let (mut missed, mut reached) = (Vec::new(), false);
    if ends.ntfy_first() {
        ends.say_next(Next::Ntfy, UPGRADE_REFUSED);
        match ntfy::joined(&ends.ntfy(), code, versions.clone(), waits.giver).await {
            Err(Failed::Unreachable(why)) => missed.push(why),
            Err(Failed::Nobody) => reached = true,
            other => return other,
        }
    }
    let at_mailboxes = pod_wormhole::joined(&ends.wormhole(), code, versions.clone(), waits.giver).await;
    // A giver that reached no mailbox waits at a later place, which this end
    // may not reach from a mailbox (call C115).
    let at_mailbox = match at_mailboxes {
        Err(Failed::Unreachable(why)) => {
            missed.push(why);
            false
        }
        Err(Failed::Nobody) => true,
        other => return other,
    };
    reached |= at_mailbox;
    let (mut noun, mut answered) = ("mailbox", at_mailbox);
    for next in ends.next_places() {
        let why = if answered { format!("no {noun} has the giver") } else { format!("no {noun} answered") };
        ends.say_next(next, &why);
        let joined = match next {
            Next::Ntfy => ntfy::joined(&ends.ntfy(), code, versions.clone(), waits.giver).await,
            Next::Relays => nostr::joined(&ends.relays(), code, versions.clone(), waits.giver).await,
            Next::Trackers => tracker::joined(&ends.trackers(), code, versions.clone(), waits.giver).await,
            Next::Brokers => mqtt::joined(&ends.brokers(), code, versions.clone(), waits.giver).await,
        };
        answered = match joined {
            Err(Failed::Unreachable(why)) => {
                missed.push(why);
                false
            }
            Err(Failed::Nobody) => true,
            other => return other,
        };
        reached |= answered;
        noun = next.noun();
    }
    if reached { Err(Failed::Nobody) } else { Err(Failed::Unreachable(Missed::join(missed))) }
}

fn now_ms() -> i64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_lives_10_minutes_and_a_taker_waits_less_for_a_giver_that_is_there() {
        let waits = Waits::default();
        assert_eq!(waits.life, Duration::from_secs(600));
        assert!(waits.giver < waits.life && waits.step < waits.life);
    }
}

//! A connection to one WebTorrent tracker for one code: JSON in text
//! messages of a WebSocket, over pod-ws through the caller's way out.
//!
//! Before the code is shown or typed, the connection proves the tracker:
//! an announce with no offer gets the tracker's answer for the code's
//! `info_hash`. A tracker that refuses (`failure reason`) is passed over
//! with its words.
//!
//! Then each end sends its messages in a [`Note`]: the taker in an offer,
//! the giver in the answer to an offer of the taker. A note carries each
//! message that the other end does not have, by the count of this
//! end's messages in the other end's last note; a message that the other
//! end has twice is dropped by its index. The taker offers again each
//! `resend` while it waits, so that a lost offer is sent again and the
//! giver has an offer to answer. The giver keeps the last offer that it
//! had no reason to answer, and answers it when it has a message: a
//! tracker passes one answer to each offer (tracker.webtorrent.dev refuses
//! a second one), so the giver delivers a message while it reads, or at
//! its close.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use pod_core::entropy::{Entropy, OsEntropy};
use pod_core::{Event, Events};
use pod_ws::close::close_code_and_reason;
use pod_ws::url::{self, Asking, WsUrl};
use pod_ws::{RelaySession, SessionError, frame};
use serde_json::{Value, json};
use tokio::time::Instant;

use super::wire::{self, Note, Said};
use super::{ANNOUNCE_LEAST, ANNOUNCE_MOST, LINGER, SDP_MOST, info_hash};
use crate::Failed;
use crate::config::Config;
use crate::mailbox::Mood;
use crate::message::{Message, hex};
use crate::mqtt::Role;

/// The most messages kept while a step waits for its answer, as a mailbox.
pub const KEPT_MOST: usize = crate::mailbox::KEPT_MOST;

/// A connection to one tracker, announced in the swarm of one code.
pub struct Tracker {
    session: RelaySession<url::Stream>,
    /// The URL, as lines show it.
    pub shown: String,
    appid: String,
    side: String,
    tag: String,
    info_hash: String,
    peer_id: String,
    role: Role,
    resend: Duration,
    /// This end's messages: the phase and the body of each.
    sent: Vec<(String, Vec<u8>)>,
    /// How many of them the other end has, by its last note.
    acked: usize,
    /// How many messages of the other end this end has.
    got: u64,
    /// The messages that came and that no step has read.
    kept: VecDeque<Message>,
    /// The `peer_id` of the other end, once it was heard: a note of
    /// another peer is a stranger's.
    peer: Option<String>,
    /// The giver's last offer of the taker that it did not answer: its
    /// `offer_id`.
    held: Option<String>,
    /// When the taker offers again, or the giver announces again.
    next: Instant,
    interval: Duration,
    answer_limit: Duration,
    events: Arc<dyn Events>,
}

impl Tracker {
    /// Dial the tracker at `url` and announce this end as `role` in the
    /// swarm of `tag` as `side`; the tracker's answer proves it.
    pub async fn connect(
        url: &WsUrl,
        config: &Config,
        side: &str,
        tag: &str,
        role: Role,
        resend: Duration,
    ) -> Result<Tracker, Failed> {
        let asking = Asking { limits: config.limits, ..Asking::default() };
        let dialed = url::dial_asking(url, &asking, &config.way, &config.trust, config.connect_timeout, None);
        let session = dialed.await.map_err(|e| Failed::Unreachable(crate::Missed::caused(url::failure(url, &e), e)))?;
        let mut tracker = Tracker {
            session,
            shown: url.shown(),
            appid: config.appid.clone(),
            side: side.to_string(),
            tag: tag.to_string(),
            info_hash: info_hash(tag),
            peer_id: drawn_id()?,
            role,
            resend,
            sent: Vec::new(),
            acked: 0,
            got: 0,
            kept: VecDeque::new(),
            peer: None,
            held: None,
            next: Instant::now(),
            interval: ANNOUNCE_MOST,
            answer_limit: config.answer_limit,
            events: config.events.clone(),
        };
        match tracker.greet().await {
            Ok(()) => Ok(tracker),
            Err(failed) => {
                tracker.end().await;
                Err(failed)
            }
        }
    }

    async fn greet(&mut self) -> Result<(), Failed> {
        self.announce(Some("started")).await?;
        let deadline = Instant::now() + self.answer_limit;
        loop {
            let read = tokio::time::timeout_at(deadline, self.read_said()).await;
            let Ok(said) = read else {
                let limit = self.answer_limit;
                let why = format!("the WebTorrent tracker {} did not answer the announce within {limit:?}", self.shown);
                return Err(Failed::Broken(why));
            };
            match said? {
                Said::Announced { info_hash, interval } if info_hash == self.info_hash => {
                    self.heed_interval(interval);
                    return Ok(());
                }
                said => self.heed(said).await?,
            }
        }
    }

    /// The AppID of the ends.
    pub fn appid(&self) -> &str {
        &self.appid
    }

    /// The `info_hash` of the swarm of the code.
    pub fn info_hash(&self) -> &str {
        &self.info_hash
    }

    /// The `peer_id` of this end, drawn for this pairing.
    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }

    /// Send a message of `phase` to the other end: the taker offers it at
    /// once; the giver answers the offer that it keeps, or the next one.
    pub async fn add(&mut self, phase: &str, body: &[u8]) -> Result<(), Failed> {
        let alone = Note { tag: self.tag.clone(), side: self.side.clone(), from: 0, got: 0, messages: Vec::new() };
        let size = alone.to_sdp().len() + phase.len() + body.len() * 2 + 32;
        if size > SDP_MOST {
            return Err(Failed::Broken(format!(
                "a message of {size} bytes is over the {SDP_MOST} that a tracker carries"
            )));
        }
        self.sent.push((phase.to_string(), body.to_vec()));
        match self.role {
            Role::Taker => {
                self.offer().await?;
                self.next = Instant::now() + self.resend;
                Ok(())
            }
            Role::Giver => match self.held.take() {
                Some(offer_id) => self.answer(&offer_id).await,
                None => Ok(()),
            },
        }
    }

    /// The next message of the other end, the kept ones first; `None` when
    /// none comes within `wait`. Meanwhile the taker offers again and the
    /// giver announces again.
    pub async fn message(&mut self, wait: Duration) -> Result<Option<Message>, Failed> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(kept) = self.kept.pop_front() {
                return Ok(Some(kept));
            }
            // A message that is there goes before the timer.
            let due = deadline.min(self.next);
            let read = tokio::select! {
                biased;
                read = self.read_said() => Some(read),
                () = tokio::time::sleep_until(due) => None,
            };
            if let Some(said) = read.transpose()? {
                self.heed(said).await?;
            }
            // After each message too: a flood holds neither the timer nor
            // the deadline.
            let now = Instant::now();
            if now >= self.next {
                match self.role {
                    Role::Taker => {
                        self.next = now + self.resend;
                        self.offer().await?;
                    }
                    Role::Giver => {
                        self.next = now + self.interval;
                        self.announce(None).await?;
                    }
                }
            }
            if now >= deadline && self.kept.is_empty() {
                return Ok(None);
            }
        }
    }

    /// Leave the swarm. After a pairing, the giver first answers the
    /// taker's offers until the taker has each of its messages, within
    /// [`LINGER`], as it can speak only in an answer; the taker first
    /// offers the count of the giver's messages that it has.
    pub async fn close(&mut self, mood: Mood) {
        if mood == Mood::Happy && self.peer.is_some() {
            let _ = match self.role {
                Role::Giver => self.linger().await,
                Role::Taker => self.offer().await,
            };
        }
        let _ = self.announce(Some("stopped")).await;
    }

    async fn linger(&mut self) -> Result<(), Failed> {
        let deadline = Instant::now() + LINGER;
        if let Some(offer_id) = self.held.take() {
            self.answer(&offer_id).await?;
        }
        while self.acked < self.sent.len() {
            let Ok(said) = tokio::time::timeout_at(deadline, self.read_said()).await else { return Ok(()) };
            self.heed(said?).await?;
        }
        Ok(())
    }

    /// End the connection with a Close.
    pub async fn end(&self) {
        let _ = self.session.send_close(1000, "").await;
    }

    /// What one message of the tracker means for this end.
    async fn heed(&mut self, said: Said) -> Result<(), Failed> {
        match said {
            Said::Offer { info_hash, peer_id, offer_id, sdp } if self.role == Role::Giver => {
                if info_hash != self.info_hash {
                    return Ok(());
                }
                match self.take(&peer_id, &sdp)? {
                    // New messages to count, or messages that the taker lacks.
                    Some(new) if new > 0 || self.acked < self.sent.len() => self.answer(&offer_id).await,
                    Some(_) => {
                        self.held = Some(offer_id);
                        Ok(())
                    }
                    None => Ok(()),
                }
            }
            Said::Answer { info_hash, peer_id, sdp, .. } if self.role == Role::Taker => {
                if info_hash == self.info_hash && self.take(&peer_id, &sdp)?.is_some_and(|new| new > 0) {
                    // The giver's next messages, and the count of these, at once.
                    self.next = Instant::now();
                }
                Ok(())
            }
            Said::Announced { info_hash, interval } if info_hash == self.info_hash => {
                self.heed_interval(interval);
                Ok(())
            }
            Said::Failure(words) if self.peer.is_none() => {
                Err(Failed::Refused(format!("the WebTorrent tracker {} refused: {words}", self.shown)))
            }
            // After the other end was heard, a refusal is of an answer to a
            // peer that left the swarm.
            Said::Failure(words) | Said::Warning(words) => {
                let line = format!("the WebTorrent tracker {} says: {words}", self.shown);
                self.events.event(Event::info("wormhole.tracker", line));
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl Tracker {
    /// The new messages of the note in `sdp` of `peer_id`, kept, and the
    /// count of this end's messages that it states; `None` for a note of
    /// another code, of this end, of a stranger, or for no note. The first
    /// peer whose note brings a message is the other end for good.
    fn take(&mut self, peer_id: &str, sdp: &str) -> Result<Option<usize>, Failed> {
        let Some(note) = wire::note_of(sdp) else { return Ok(None) };
        if note.tag != self.tag || note.side == self.side {
            return Ok(None);
        }
        match &self.peer {
            Some(peer) if peer != peer_id => return Ok(None),
            Some(_) => {}
            None if note.indexed().any(|(n, _)| n == self.got) => self.peer = Some(peer_id.to_string()),
            None => return Ok(None),
        }
        let mut new = 0;
        for (index, message) in note.indexed() {
            if index != self.got {
                continue;
            }
            if self.kept.len() >= KEPT_MOST {
                let why = format!("the WebTorrent tracker {} sent more than {KEPT_MOST} messages at once", self.shown);
                return Err(Failed::Broken(why));
            }
            self.kept.push_back(message);
            self.got += 1;
            new += 1;
        }
        if let Ok(has) = usize::try_from(note.got)
            && has > self.acked
            && has <= self.sent.len()
        {
            self.acked = has;
        }
        Ok(Some(new))
    }

    /// This end's note: its messages that the other end does not have, as
    /// many as an `sdp` carries.
    fn note(&self) -> Note {
        let mut note = Note {
            tag: self.tag.clone(),
            side: self.side.clone(),
            from: self.acked as u64,
            got: self.got,
            messages: Vec::new(),
        };
        for message in &self.sent[self.acked..] {
            note.messages.push(message.clone());
            if note.to_sdp().len() > SDP_MOST {
                note.messages.pop();
                break;
            }
        }
        note
    }

    /// Offer this end's note to a peer of the swarm.
    async fn offer(&mut self) -> Result<(), Failed> {
        let offer = json!({ "type": "offer", "sdp": self.note().to_sdp() });
        let mut message = self.announcing(None);
        message["numwant"] = json!(1);
        message["offers"] = json!([{ "offer": offer, "offer_id": drawn_id()? }]);
        self.send(&message).await
    }

    /// Answer the offer `offer_id` of the other end with this end's note.
    async fn answer(&mut self, offer_id: &str) -> Result<(), Failed> {
        let Some(peer) = self.peer.clone() else { return Ok(()) };
        self.held = None;
        let answer = json!({ "type": "answer", "sdp": self.note().to_sdp() });
        let mut message = self.announcing(None);
        message["to_peer_id"] = json!(peer);
        message["offer_id"] = json!(offer_id);
        message["answer"] = answer;
        self.send(&message).await
    }

    /// Announce this end with no offer, with `event` where it has one.
    async fn announce(&mut self, event: Option<&str>) -> Result<(), Failed> {
        let mut message = self.announcing(event);
        message["offers"] = json!([]);
        self.send(&message).await
    }

    /// The fields of each announce, as WebTorrent's client sends them.
    fn announcing(&self, event: Option<&str>) -> Value {
        let mut message = json!({
            "action": "announce",
            "info_hash": self.info_hash,
            "peer_id": self.peer_id,
            "numwant": 0,
            "uploaded": 0,
            "downloaded": 0,
        });
        if let Some(event) = event {
            message["event"] = json!(event);
        }
        message
    }

    fn heed_interval(&mut self, interval: Option<u64>) {
        let seconds = Duration::from_secs(interval.unwrap_or(ANNOUNCE_MOST.as_secs()));
        self.interval = seconds.clamp(ANNOUNCE_LEAST, ANNOUNCE_MOST);
        if self.role == Role::Giver {
            self.next = Instant::now() + self.interval;
        }
    }

    /// The next message of the tracker; the connection pinged meanwhile, so
    /// that a dead one is found while this end waits.
    async fn read_said(&mut self) -> Result<Said, Failed> {
        let read = {
            let liveness = self.session.watch_liveness(pod_ws::LIVENESS_EVERY, pod_ws::LIVENESS_ALLOWED);
            tokio::select! {
                read = self.session.read_frame() => read,
                dead = liveness => Err(dead),
            }
        };
        let f = read.map_err(|e| self.broken(&e))?;
        match f.opcode {
            frame::OPCODE_TEXT | frame::OPCODE_BINARY => wire::said(&f.payload).ok_or_else(|| {
                Failed::Broken(format!("the WebTorrent tracker {} sent a message that is no tracker's", self.shown))
            }),
            frame::OPCODE_CLOSE => {
                let (code, reason) = close_code_and_reason(&f.payload);
                let code = code.map_or_else(|| "no code".to_string(), |c| c.to_string());
                let reason = pod_core::text::one_line(&reason);
                let why = format!("the WebTorrent tracker {} closed the connection ({code} {reason})", self.shown);
                Err(Failed::Broken(why))
            }
            other => Err(Failed::Broken(format!(
                "the WebTorrent tracker {} sent an unexpected frame (opcode {other:#x})",
                self.shown
            ))),
        }
    }

    async fn send(&mut self, message: &Value) -> Result<(), Failed> {
        self.session.send_text(&message.to_string()).await.map_err(|e| self.broken(&e))
    }

    fn broken(&self, e: &SessionError) -> Failed {
        Failed::Broken(format!("the WebTorrent tracker {}: {e}", self.shown))
    }
}

/// 20 hex digits from the system's random bytes: a `peer_id` or an
/// `offer_id`.
fn drawn_id() -> Result<String, Failed> {
    let mut drawn = [0u8; wire::ID_CHARS / 2];
    OsEntropy.fill(&mut drawn).map_err(|_| Failed::NoRandom)?;
    Ok(hex(&drawn))
}

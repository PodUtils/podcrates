//! A connection to one Nostr relay for one code: NIP-01 in text messages
//! of a WebSocket, over pod-ws through the caller's way out. One `REQ` for
//! the events of [`KIND`] whose tag [`TAG`] is the tag that this end reads,
//! since the start of this end less [`SINCE_SLACK`]; then each message of
//! the mailbox as the content of an event of that kind, tagged with the
//! tag that the other end reads, signed by a key of this pairing alone.
//!
//! Before the code is shown or typed, the connection proves the relay: the
//! `REQ` ends its stored events (`EOSE`), and an empty event to this end's
//! own tag comes back with `OK` true. A relay that refuses (`CLOSED`, `OK`
//! false), or that keeps the event from its subscriber, is passed over
//! with its words.
//!
//! A relay keeps no ephemeral event, or keeps it a short time: one that
//! reaches no subscriber can be gone. So the giver holds its messages
//! until the taker is heard, and the taker sends its messages again each
//! `resend` until the giver is heard, each time as a new event (a later
//! `created_at`, so a new id that no relay drops as a duplicate).
//!
//! A public relay checks an event in its own time, and drops one that it
//! had not passed on when the connection of its sender ends. So an end
//! closes only after the relay answered each of its events (`OK`), within
//! the answer limit: the giver's last word reaches the taker.

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

use super::event::{self, Key, Said, harmless};
use super::{CONTENT_MOST, KIND, SINCE_SLACK, TAG, unix_now};
use crate::Failed;
use crate::config::Config;
use crate::message::{self, Message, hex};
use crate::mqtt::Role;

/// The most messages kept while a step waits for its answer, as a mailbox.
pub const KEPT_MOST: usize = crate::mailbox::KEPT_MOST;
/// The most events of this end that wait for the relay's `OK`: a relay
/// that does not answer holds the close no longer for more.
pub const UNANSWERED_MOST: usize = 64;

/// A connection to one relay, subscribed to the tag of one code.
pub struct Relay {
    session: RelaySession<url::Stream>,
    /// The URL, as lines show it.
    pub shown: String,
    appid: String,
    side: String,
    key: Key,
    /// The id of the one subscription.
    sub: String,
    /// The tag that this end reads, and the one that the other end reads.
    inbox: String,
    outbox: String,
    role: Role,
    resend: Duration,
    /// The messages that came while a step waited for its answer.
    kept: VecDeque<Message>,
    /// The contents of this end's messages before the other end was heard.
    unanswered: Vec<String>,
    heard: bool,
    next_resend: Instant,
    /// The ids of this end's events that the relay did not answer yet.
    pending: VecDeque<String>,
    /// The `created_at` of the last event: each new one is later.
    last_created: u64,
    answer_limit: Duration,
    events: Arc<dyn Events>,
    /// The relay's last `NOTICE`, for the words of a failure.
    notice: Option<String>,
    /// What this end sent and heard, for the words of a quiet other end.
    tally: Tally,
}

/// The events of one end at a relay: they tell a relay that drops the
/// events of the other end from one that holds them, or from an other end
/// that sent nothing.
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    /// The events of messages that this end sent.
    sent: u32,
    /// Of them, the ones that the relay answered with `OK` true.
    accepted: u32,
    /// The events of another key that the relay passed to this end.
    passed: u32,
    /// Of them, the messages of the other end.
    taken: u32,
}

impl Relay {
    /// Dial the relay at `url`, subscribe to the tag that `role` reads of
    /// `tag` as `side`, and prove that the relay passes an event of this
    /// end back to it.
    pub async fn connect(
        url: &WsUrl,
        config: &Config,
        side: &str,
        tag: &str,
        role: Role,
        resend: Duration,
    ) -> Result<Relay, Failed> {
        let asking = Asking { limits: config.limits, ..Asking::default() };
        let dialed = url::dial_asking(url, &asking, &config.way, &config.trust, config.connect_timeout, None);
        let session = dialed.await.map_err(|e| Failed::Unreachable(crate::Missed::caused(url::failure(url, &e), e)))?;
        let mut drawn = [0u8; 8];
        OsEntropy.fill(&mut drawn).map_err(|_| Failed::NoRandom)?;
        let (inbox, outbox) = role.topics(tag);
        let mut relay = Relay {
            session,
            shown: url.shown(),
            appid: config.appid.clone(),
            side: side.to_string(),
            key: Key::new()?,
            sub: format!("pw{}", hex(&drawn)),
            inbox,
            outbox,
            role,
            resend,
            kept: VecDeque::new(),
            unanswered: Vec::new(),
            heard: false,
            next_resend: Instant::now(),
            pending: VecDeque::new(),
            last_created: 0,
            answer_limit: config.answer_limit,
            events: config.events.clone(),
            notice: None,
            tally: Tally::default(),
        };
        match relay.greet().await {
            Ok(()) => Ok(relay),
            Err(failed) => {
                relay.end().await;
                Err(failed)
            }
        }
    }

    async fn greet(&mut self) -> Result<(), Failed> {
        let since = unix_now().saturating_sub(SINCE_SLACK.as_secs());
        let filter = json!({ "kinds": [KIND], format!("#{TAG}"): [self.inbox], "since": since });
        self.send(&json!(["REQ", self.sub, filter])).await?;
        self.until("EOSE", |relay, said| matches!(said, Said::Eose(sub) if *sub == relay.sub)).await?;
        let tags = json!([[TAG, self.inbox]]);
        let created_at = self.stamp();
        let probe = self.key.sign(created_at, KIND, &tags, "")?;
        let id = probe["id"].as_str().unwrap_or_default().to_string();
        self.send(&json!(["EVENT", probe])).await?;
        let (mut accepted, mut echoed) = (false, false);
        self.until("its own event back", |_, said| {
            match said {
                Said::Ok { id: of, accepted: true, .. } if *of == id => accepted = true,
                Said::Event { event, .. } if event["id"].as_str() == Some(&id) => echoed = true,
                _ => {}
            }
            accepted && echoed
        })
        .await
    }

    /// Read the relay until `done` holds of a message, within the answer
    /// limit: the messages of the other end are kept, a refusal fails.
    async fn until(&mut self, what: &str, mut done: impl FnMut(&Relay, &Said) -> bool) -> Result<(), Failed> {
        let deadline = Instant::now() + self.answer_limit;
        loop {
            let read = tokio::time::timeout_at(deadline, self.read_said()).await;
            let Ok(said) = read else {
                let limit = self.answer_limit;
                let notice = self.notice.as_ref().map(|n| format!("; its last notice: {n}")).unwrap_or_default();
                return Err(Failed::Broken(format!(
                    "the Nostr relay {} did not send {what} within {limit:?}{notice}",
                    self.shown
                )));
            };
            let said = said?;
            if done(self, &said) {
                return Ok(());
            }
            if let Some(message) = self.heed(said).await? {
                if self.kept.len() >= KEPT_MOST {
                    let why = format!("the Nostr relay {} sent more than {KEPT_MOST} messages at once", self.shown);
                    return Err(Failed::Broken(why));
                }
                self.kept.push_back(message);
            }
        }
    }

    /// The AppID of the ends.
    pub fn appid(&self) -> &str {
        &self.appid
    }

    /// The tag that this end reads.
    pub fn inbox(&self) -> &str {
        &self.inbox
    }

    /// The public key of this pairing at the relay.
    pub fn pubkey(&self) -> &str {
        self.key.pubkey()
    }

    /// Send a message of `phase` to the other end; before the other end was
    /// heard, the giver holds it and the taker sends it again.
    pub async fn add(&mut self, phase: &str, body: &[u8]) -> Result<(), Failed> {
        let content = json!({ "side": self.side, "phase": phase, "body": hex(body) }).to_string();
        if content.len() > CONTENT_MOST {
            let why = format!("a message of {} bytes is over the {CONTENT_MOST} that a relay carries", content.len());
            return Err(Failed::Broken(why));
        }
        if !self.heard {
            self.unanswered.push(content.clone());
            if self.role == Role::Giver {
                return Ok(());
            }
            if self.unanswered.len() == 1 {
                self.next_resend = Instant::now() + self.resend;
            }
        }
        self.publish(&content).await
    }

    /// The next message of the other end, the kept ones first; `None` when
    /// none comes within `wait`. Meanwhile the taker sends its unanswered
    /// messages again.
    pub async fn message(&mut self, wait: Duration) -> Result<Option<Message>, Failed> {
        if let Some(kept) = self.kept.pop_front() {
            return Ok(Some(kept));
        }
        let deadline = Instant::now() + wait;
        loop {
            let resending = self.role == Role::Taker && !self.heard && !self.unanswered.is_empty();
            let next = if resending { deadline.min(self.next_resend) } else { deadline };
            // A message that is there goes before the timer.
            let read = tokio::select! {
                biased;
                read = self.read_said() => Some(read),
                () = tokio::time::sleep_until(next) => None,
            };
            if let Some(said) = read.transpose()?
                && let Some(message) = self.heed(said).await?
            {
                return Ok(Some(message));
            }
            // After each message too: a flood holds neither the sending
            // again nor the deadline.
            let now = Instant::now();
            if resending && now >= self.next_resend {
                self.next_resend = now + self.resend;
                for content in self.unanswered.clone() {
                    self.publish(&content).await?;
                }
            }
            if now >= deadline {
                return Ok(None);
            }
        }
    }

    /// End the subscription, once the relay answered each event of this
    /// end, within the answer limit: an event that it had not passed on is
    /// lost when the connection ends.
    pub async fn close(&mut self) {
        let deadline = Instant::now() + self.answer_limit;
        while !self.pending.is_empty() {
            let Ok(Ok(said)) = tokio::time::timeout_at(deadline, self.read_said()).await else { break };
            // A refusal ends nothing now: its event is answered.
            if let Ok(Some(message)) = self.heed(said).await
                && self.kept.len() < KEPT_MOST
            {
                self.kept.push_back(message);
            }
        }
        let _ = self.send(&json!(["CLOSE", self.sub])).await;
    }

    /// What this end sent and heard here, for the words of an other end
    /// that said nothing: a relay that accepted each event of this end and
    /// passed none of the other end drops or holds them.
    pub fn tally(&self) -> String {
        let Tally { sent, accepted, passed, taken } = self.tally;
        format!(
            "at the Nostr relay {} this end sent {sent} events and the relay accepted {accepted}; \
             it passed {passed} events of the other key, {taken} of them messages of the other end",
            self.shown
        )
    }

    /// End the connection with a Close.
    pub async fn end(&self) {
        let _ = self.session.send_close(1000, "").await;
    }

    /// What one message of the relay means: a message of the other end, a
    /// refusal, or nothing.
    async fn heed(&mut self, said: Said) -> Result<Option<Message>, Failed> {
        if let Said::Ok { id, accepted, .. } = &said {
            if *accepted && self.pending.contains(id) {
                self.tally.accepted += 1;
            }
            self.pending.retain(|pending| pending != id);
        }
        match said {
            Said::Event { sub, event } if sub == self.sub => self.take(&event).await,
            Said::Ok { accepted: false, message, .. } if !harmless(&message) => {
                Err(Failed::Refused(format!("the Nostr relay {} refused an event: {message}", self.shown)))
            }
            Said::Closed { sub, message } if sub == self.sub => {
                Err(Failed::Refused(format!("the Nostr relay {} ended the subscription: {message}", self.shown)))
            }
            Said::Notice(words) => {
                let line = format!("the Nostr relay {} says: {words}", self.shown);
                self.events.event(Event::info("wormhole.nostr", line));
                self.notice = Some(words);
                Ok(None)
            }
            // An OK, the end of stored events, an AUTH, a type of a later
            // NIP, a message of another subscription.
            _ => Ok(None),
        }
    }

    /// The message of an event of this end's tag; `None` for an event that
    /// does not check, of another kind or tag, of this end's key, or whose
    /// content is no message of the other side. The first one ends the hold
    /// and the sending again.
    async fn take(&mut self, event: &Value) -> Result<Option<Message>, Failed> {
        let Some(checked) = event::checked(event) else { return Ok(None) };
        if checked.kind != KIND || checked.tag(TAG) != Some(self.inbox.as_str()) || checked.pubkey == self.pubkey() {
            return Ok(None);
        }
        self.tally.passed += 1;
        let read = message::object(checked.content.as_bytes()).as_ref().and_then(message::message_of);
        let Some(message) = read.filter(|m| m.side != self.side) else { return Ok(None) };
        self.tally.taken += 1;
        if !self.heard {
            self.heard = true;
            let held = std::mem::take(&mut self.unanswered);
            if self.role == Role::Giver {
                for content in held {
                    self.publish(&content).await?;
                }
            }
        }
        Ok(Some(message))
    }

    async fn publish(&mut self, content: &str) -> Result<(), Failed> {
        let created_at = self.stamp();
        let event = self.key.sign(created_at, KIND, &json!([[TAG, self.outbox]]), content)?;
        if self.pending.len() >= UNANSWERED_MOST {
            self.pending.pop_front();
        }
        self.pending.push_back(event["id"].as_str().unwrap_or_default().to_string());
        self.tally.sent += 1;
        self.send(&json!(["EVENT", event])).await
    }

    /// The `created_at` of a new event: now, or one second after the last.
    fn stamp(&mut self) -> u64 {
        self.last_created = unix_now().max(self.last_created + 1);
        self.last_created
    }

    /// The next message of the relay; the connection pinged meanwhile, so
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
            frame::OPCODE_TEXT | frame::OPCODE_BINARY => event::said(&f.payload).ok_or_else(|| {
                Failed::Broken(format!("the Nostr relay {} sent a message that is not of NIP-01", self.shown))
            }),
            frame::OPCODE_CLOSE => {
                let (code, reason) = close_code_and_reason(&f.payload);
                let code = code.map_or_else(|| "no code".to_string(), |c| c.to_string());
                let reason = pod_core::text::one_line(&reason);
                Err(Failed::Broken(format!("the Nostr relay {} closed the connection ({code} {reason})", self.shown)))
            }
            other => Err(Failed::Broken(format!(
                "the Nostr relay {} sent an unexpected frame (opcode {other:#x})",
                self.shown
            ))),
        }
    }

    async fn send(&mut self, message: &Value) -> Result<(), Failed> {
        self.session.send_text(&message.to_string()).await.map_err(|e| self.broken(&e))
    }

    fn broken(&self, e: &SessionError) -> Failed {
        Failed::Broken(format!("the Nostr relay {}: {e}", self.shown))
    }
}

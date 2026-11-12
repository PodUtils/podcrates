//! A connection to one MQTT broker for one code: MQTT 3.1.1 in binary
//! messages of a WebSocket that offers the subprotocol `mqtt` (section 6 of
//! the standard), over pod-ws through the caller's way out. A packet may
//! span messages, and a message may hold more than one packet. A `CONNECT`
//! with a clean session, a random client id and a keep-alive of 60 s; one
//! `SUBSCRIBE` at QoS 0 to the topic that this end reads; then `PUBLISH`
//! at QoS 0 with no retain to the topic that the other end reads, and
//! `PINGREQ` when this end sent nothing for half the keep-alive.
//!
//! Each message is a JSON object of a mailbox's message (the side, the
//! phase, the body in hex), so the wormhole reads the same messages at a
//! broker as at a mailbox. A broker does not keep a message at QoS 0 with
//! no retain: one that reaches no subscriber is gone. So the giver holds
//! its messages until the taker is heard, and the taker sends its messages
//! again each `resend` until the giver is heard. Each end subscribes
//! before it publishes, so after the first message of the other end each
//! message reaches it.

use std::collections::VecDeque;
use std::time::Duration;

use pod_core::entropy::{Entropy, OsEntropy};
use pod_ws::close::close_code_and_reason;
use pod_ws::url::{self, Asking};
use pod_ws::{RelaySession, SessionError, frame};
use serde_json::json;
use tokio::time::Instant;

use super::packet::{self, Connect, Packet, REFUSED};
use super::{KEEP_ALIVE, PAYLOAD_MOST, PROTOCOL, Point, Role};
use crate::Failed;
use crate::config::Config;
use crate::message::{self, Message, hex};

/// A `PINGREQ` goes when this end sent nothing for this long: half the
/// keep-alive, so that no broker ends the connection.
pub const PING_EVERY: Duration = Duration::from_secs(KEEP_ALIVE.as_secs() / 2);
/// The most messages kept while a step waits for its answer, as a mailbox.
pub const KEPT_MOST: usize = crate::mailbox::KEPT_MOST;
/// The packet identifier of the one `SUBSCRIBE`.
const SUBSCRIBE_ID: u16 = 1;

/// A connection to one broker, subscribed to the topic of one code.
pub struct Broker {
    session: RelaySession<url::Stream>,
    /// The URL, as lines show it.
    pub shown: String,
    appid: String,
    side: String,
    /// The topic that this end reads, and the one that the other end reads.
    inbox: String,
    outbox: String,
    role: Role,
    resend: Duration,
    /// The bytes of the broker that are no whole packet yet.
    pending: Vec<u8>,
    /// The messages that came while a step waited for its answer.
    kept: VecDeque<Message>,
    /// This end's messages before the other end was heard.
    unanswered: Vec<Vec<u8>>,
    heard: bool,
    next_resend: Instant,
    last_sent: Instant,
    /// A `PINGREQ` went, and no `PINGRESP` came.
    ping_waits: bool,
    answer_limit: Duration,
    packet_most: usize,
}

impl Broker {
    /// Dial the broker at `point`, connect as a new client, and subscribe
    /// to the topic that `role` reads of `topic`, as `side`. A `CONNACK`
    /// with a code other than 0, or a refused subscription, is
    /// [`Failed::Refused`].
    pub async fn connect(
        point: &Point,
        config: &Config,
        side: &str,
        topic: &str,
        role: Role,
        resend: Duration,
    ) -> Result<Broker, Failed> {
        let asking = Asking { protocol: Some(PROTOCOL), limits: config.limits, ..Asking::default() };
        let dialed = url::dial_asking(&point.url, &asking, &config.way, &config.trust, config.connect_timeout, None);
        let session = dialed.await.map_err(|e| Failed::Unreachable(url::failure(&point.url, &e)))?;
        let (inbox, outbox) = role.topics(topic);
        let now = Instant::now();
        let mut broker = Broker {
            session,
            shown: point.url.shown(),
            appid: config.appid.clone(),
            side: side.to_string(),
            inbox,
            outbox,
            role,
            resend,
            pending: Vec::new(),
            kept: VecDeque::new(),
            unanswered: Vec::new(),
            heard: false,
            next_resend: now,
            last_sent: now,
            ping_waits: false,
            answer_limit: config.answer_limit,
            packet_most: config.limits.message,
        };
        let login = point.login.as_ref().map(|l| (l.user.clone(), l.password.as_bytes().to_vec()));
        match broker.greet(login).await {
            Ok(()) => Ok(broker),
            Err(failed) => {
                broker.end().await;
                Err(failed)
            }
        }
    }

    async fn greet(&mut self, login: Option<(String, Vec<u8>)>) -> Result<(), Failed> {
        let client_id = client_id()?;
        let keep_alive = u16::try_from(KEEP_ALIVE.as_secs()).unwrap_or(u16::MAX);
        self.write(&Packet::Connect(Connect { client_id, keep_alive, clean: true, login })).await?;
        match self.answer("CONNACK").await? {
            Packet::ConnAck { code: 0, .. } => {}
            Packet::ConnAck { code, .. } => {
                let why = format!("the broker {} refused this client: {}", self.shown, refusal(code));
                return Err(Failed::Refused(why));
            }
            other => return Err(self.unexpected(&other)),
        }
        let filters = vec![(self.inbox.clone(), 0)];
        self.write(&Packet::Subscribe { id: SUBSCRIBE_ID, filters }).await?;
        match self.answer("SUBACK").await? {
            Packet::SubAck { id: SUBSCRIBE_ID, codes } if codes.first() != Some(&REFUSED) => Ok(()),
            Packet::SubAck { id: SUBSCRIBE_ID, .. } => {
                Err(Failed::Refused(format!("the broker {} refused the subscription to the code's topic", self.shown)))
            }
            other => Err(self.unexpected(&other)),
        }
    }

    /// The AppID of the ends.
    pub fn appid(&self) -> &str {
        &self.appid
    }

    /// The topic that this end reads.
    pub fn inbox(&self) -> &str {
        &self.inbox
    }

    /// Publish a message of `phase` to the other end; before the other end
    /// was heard, the giver holds it and the taker sends it again.
    pub async fn add(&mut self, phase: &str, body: &[u8]) -> Result<(), Failed> {
        let payload = json!({ "side": self.side, "phase": phase, "body": hex(body) }).to_string().into_bytes();
        if payload.len() > PAYLOAD_MOST {
            let why = format!("a message of {} bytes is over the {PAYLOAD_MOST} that a broker carries", payload.len());
            return Err(Failed::Broken(why));
        }
        if !self.heard {
            self.unanswered.push(payload.clone());
            if self.role == Role::Giver {
                return Ok(());
            }
            if self.unanswered.len() == 1 {
                self.next_resend = Instant::now() + self.resend;
            }
        }
        self.publish(payload).await
    }

    /// The next message of the other end, the kept ones first; `None` when
    /// none comes within `wait`. Meanwhile the taker sends its unanswered
    /// messages again, and a `PINGREQ` keeps the connection.
    pub async fn message(&mut self, wait: Duration) -> Result<Option<Message>, Failed> {
        if let Some(kept) = self.kept.pop_front() {
            return Ok(Some(kept));
        }
        let deadline = Instant::now() + wait;
        loop {
            let resending = self.role == Role::Taker && !self.heard && !self.unanswered.is_empty();
            let mut next = deadline.min(self.last_sent + PING_EVERY);
            if resending {
                next = next.min(self.next_resend);
            }
            let read = tokio::select! {
                read = self.read_packet() => Some(read),
                () = tokio::time::sleep_until(next) => None,
            };
            match read.transpose()? {
                Some(Packet::Publish { topic, payload }) => {
                    if let Some(message) = self.take(&topic, &payload).await? {
                        return Ok(Some(message));
                    }
                }
                Some(Packet::PingResp) => self.ping_waits = false,
                Some(other) => return Err(self.unexpected(&other)),
                None => {
                    let now = Instant::now();
                    if now >= deadline {
                        return Ok(None);
                    }
                    if resending && now >= self.next_resend {
                        self.next_resend = now + self.resend;
                        for payload in self.unanswered.clone() {
                            self.publish(payload).await?;
                        }
                    }
                    if now >= self.last_sent + PING_EVERY {
                        self.ping().await?;
                    }
                }
            }
        }
    }

    /// Send `DISCONNECT`: the broker drops the session with no will.
    pub async fn disconnect(&mut self) {
        let _ = self.write(&Packet::Disconnect).await;
    }

    /// End the connection with a Close.
    pub async fn end(&self) {
        let _ = self.session.send_close(1000, "").await;
    }

    /// The message of a `PUBLISH` to this end's topic; `None` for another
    /// topic, for a payload that is no message (a stranger's bytes) and
    /// for this end's own. The first one ends the hold and the sending
    /// again.
    async fn take(&mut self, topic: &str, payload: &[u8]) -> Result<Option<Message>, Failed> {
        if topic != self.inbox {
            return Ok(None);
        }
        let read = message::object(payload).as_ref().and_then(message::message_of);
        let Some(message) = read.filter(|m| m.side != self.side) else { return Ok(None) };
        if !self.heard {
            self.heard = true;
            let held = std::mem::take(&mut self.unanswered);
            if self.role == Role::Giver {
                for payload in held {
                    self.publish(payload).await?;
                }
            }
        }
        Ok(Some(message))
    }

    async fn publish(&mut self, payload: Vec<u8>) -> Result<(), Failed> {
        self.write(&Packet::Publish { topic: self.outbox.clone(), payload }).await
    }

    /// A `PINGREQ`; one that got no `PINGRESP` since the last ends the
    /// connection, which is dead.
    async fn ping(&mut self) -> Result<(), Failed> {
        if self.ping_waits {
            let why = format!("the broker {} did not answer a PINGREQ within {} s", self.shown, PING_EVERY.as_secs());
            return Err(Failed::Broken(why));
        }
        self.ping_waits = true;
        self.write(&Packet::PingReq).await
    }

    /// The answer of type `kind` (a `CONNACK` or a `SUBACK`), within
    /// [`Config::answer_limit`]; a `PUBLISH` that comes meanwhile is kept.
    async fn answer(&mut self, kind: &str) -> Result<Packet, Failed> {
        let limit = self.answer_limit;
        match tokio::time::timeout(limit, self.answer_now()).await {
            Ok(answer) => answer,
            Err(_) => Err(Failed::Broken(format!("the broker {} did not answer {kind} within {limit:?}", self.shown))),
        }
    }

    async fn answer_now(&mut self) -> Result<Packet, Failed> {
        loop {
            match self.read_packet().await? {
                Packet::Publish { topic, payload } => {
                    let Some(message) = self.take(&topic, &payload).await? else { continue };
                    if self.kept.len() >= KEPT_MOST {
                        let why = format!("the broker {} sent more than {KEPT_MOST} messages at once", self.shown);
                        return Err(Failed::Broken(why));
                    }
                    self.kept.push_back(message);
                }
                Packet::PingResp => self.ping_waits = false,
                other => return Ok(other),
            }
        }
    }

    /// The next packet: from the bytes read, else from the next messages.
    async fn read_packet(&mut self) -> Result<Packet, Failed> {
        loop {
            match packet::decode(&self.pending, self.packet_most) {
                Ok(Some((packet, used))) => {
                    self.pending.drain(..used);
                    return Ok(packet);
                }
                Ok(None) => {}
                Err(why) => {
                    return Err(Failed::Broken(format!("the broker {} sent a malformed packet: {why}", self.shown)));
                }
            }
            let bytes = self.read_message().await?;
            self.pending.extend_from_slice(&bytes);
        }
    }

    /// The bytes of the next binary message; the connection pinged
    /// meanwhile, so that a dead one is found while this end waits.
    async fn read_message(&mut self) -> Result<Vec<u8>, Failed> {
        let read = {
            let liveness = self.session.watch_liveness(pod_ws::LIVENESS_EVERY, pod_ws::LIVENESS_ALLOWED);
            tokio::select! {
                read = self.session.read_frame() => read,
                dead = liveness => Err(dead),
            }
        };
        let f = read.map_err(|e| self.broken(&e))?;
        match f.opcode {
            frame::OPCODE_BINARY => Ok(f.payload),
            frame::OPCODE_CLOSE => {
                let (code, reason) = close_code_and_reason(&f.payload);
                let code = code.map_or_else(|| "no code".to_string(), |c| c.to_string());
                let reason = pod_core::text::one_line(&reason);
                Err(Failed::Broken(format!("the broker {} closed the connection ({code} {reason})", self.shown)))
            }
            other => Err(Failed::Broken(format!(
                "the broker {} sent a frame of opcode {other:#x}: MQTT goes in binary messages",
                self.shown
            ))),
        }
    }

    async fn write(&mut self, packet: &Packet) -> Result<(), Failed> {
        let bytes = packet::encode(packet).map_err(|why| Failed::Broken(why.0))?;
        self.session.send_binary(&bytes).await.map_err(|e| self.broken(&e))?;
        self.last_sent = Instant::now();
        Ok(())
    }

    fn unexpected(&self, packet: &Packet) -> Failed {
        Failed::Broken(format!("the broker {} sent a {} that this client did not ask for", self.shown, packet.name()))
    }

    fn broken(&self, e: &SessionError) -> Failed {
        Failed::Broken(format!("the broker {}: {e}", self.shown))
    }
}

/// A client id of its own for each connection: letters and digits, 22 of
/// them, within the 23 that each broker must take (section 3.1.3.1).
pub fn client_id() -> Result<String, Failed> {
    let mut bytes = [0u8; 10];
    OsEntropy.fill(&mut bytes).map_err(|_| Failed::NoRandom)?;
    Ok(format!("pw{}", hex(&bytes)))
}

/// The words of a refusing `CONNACK` (section 3.2.2.3).
pub fn refusal(code: u8) -> String {
    match code {
        1 => "it does not take MQTT 3.1.1".into(),
        2 => "it refused the client id".into(),
        3 => "the service is not available".into(),
        4 => "a bad user name or password".into(),
        5 => "not authorized".into(),
        other => format!("the code {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_id_is_22_letters_and_digits_of_its_own() {
        let (one, two) = (client_id().unwrap(), client_id().unwrap());
        assert!(one.len() == 22 && one.bytes().all(|b| b.is_ascii_alphanumeric()), "{one}");
        assert_ne!(one, two);
    }

    #[test]
    fn each_refusal_of_a_connack_has_its_words() {
        assert_eq!(refusal(4), "a bad user name or password");
        assert_eq!(refusal(5), "not authorized");
        assert_eq!(refusal(9), "the code 9");
        assert_eq!(PING_EVERY, Duration::from_secs(30));
    }
}

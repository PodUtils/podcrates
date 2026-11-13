//! A board of retained messages at one MQTT broker, for a caller that holds
//! one connection at a time (PodUtils/podssh#220): a note at a topic stays
//! at the broker until another note or a clear takes its place.
//!
//! Each call is one connection of its own, MQTT 3.1.1 over a WebSocket that
//! offers the subprotocol `mqtt`, as [`super::broker`]: a `CONNECT` with a
//! clean session and a random client id, then
//!
//! * [`Board::put`]: a `PUBLISH` at QoS 0 with RETAIN;
//! * [`Board::take`]: a `SUBSCRIBE` at QoS 0, the first `PUBLISH` of the
//!   topic within the wait (the broker sends the retained one after the
//!   `SUBACK`);
//! * [`Board::clear`]: a `PUBLISH` at QoS 0 with RETAIN and no payload
//!   (section 3.3.1.3: the broker drops the retained message);
//!
//! and a `DISCONNECT`. QoS 0 has no acknowledgement: after a `PUBLISH`, a
//! `PINGREQ` and its `PINGRESP` show that the broker read the packets
//! before it, as it reads the packets of one connection in order.
//!
//! A topic is letters and digits alone, at most [`TOPIC_MOST`]; a payload
//! is at most [`PAYLOAD_MOST`] bytes. Each broker reads each topic: the
//! caller seals each note. A topic or a payload out of these bounds is
//! refused before a connection.

use std::time::Duration;

use pod_core::text::one_line;
use pod_ws::close::close_code_and_reason;
use pod_ws::url::{self, Asking};
use pod_ws::{RelaySession, frame};
use tokio::time::Instant;

use super::broker::{PING_EVERY, client_id, refusal};
use super::packet::{self, Connect, Packet, REFUSED};
use super::{KEEP_ALIVE, PAYLOAD_MOST, PROTOCOL, Point};
use crate::Failed;
use crate::config::Config;

/// The most characters of a topic of a board: the 42 of a topic of a code
/// ([`super::topic`]).
pub const TOPIC_MOST: usize = 42;
/// The packet identifier of the one `SUBSCRIBE` of a take.
const SUBSCRIBE_ID: u16 = 1;

/// A board at one broker: the broker and the settings of each connection.
#[derive(Clone)]
pub struct Board {
    pub point: Point,
    pub config: Config,
}

impl Board {
    pub fn new(point: Point, config: Config) -> Board {
        Board { point, config }
    }

    /// The URL of the broker, as lines show it.
    pub fn shown(&self) -> String {
        self.point.url.shown()
    }

    /// Put `payload` at `topic`: the broker keeps it for each later take,
    /// in place of the note that was there.
    pub async fn put(&self, topic: &str, payload: &[u8]) -> Result<(), Failed> {
        check_topic(topic)?;
        check_payload(payload)?;
        self.publish(topic, payload.to_vec()).await
    }

    /// The note at `topic`: the first `PUBLISH` of the topic within `wait`
    /// after the subscription; `None` when none comes. An empty `PUBLISH`
    /// (a clear while this end waits) is no note.
    pub async fn take(&self, topic: &str, wait: Duration) -> Result<Option<Vec<u8>>, Failed> {
        check_topic(topic)?;
        let mut link = Link::open(&self.point, &self.config).await?;
        let taken = link.take(topic, wait).await;
        link.end().await;
        taken
    }

    /// Clear `topic`: an empty retained `PUBLISH`, after which the broker
    /// keeps no note there.
    pub async fn clear(&self, topic: &str) -> Result<(), Failed> {
        check_topic(topic)?;
        self.publish(topic, Vec::new()).await
    }

    async fn publish(&self, topic: &str, payload: Vec<u8>) -> Result<(), Failed> {
        let mut link = Link::open(&self.point, &self.config).await?;
        let published = link.publish(topic, payload).await;
        link.end().await;
        published
    }
}

/// A topic of a board: 1 to [`TOPIC_MOST`] letters and digits, so that no
/// level, no wildcard and no `$` reaches the broker.
pub fn check_topic(topic: &str) -> Result<(), Failed> {
    if topic.is_empty() || topic.len() > TOPIC_MOST || !topic.bytes().all(|b| b.is_ascii_alphanumeric()) {
        let why = format!("a topic of a board is 1 to {TOPIC_MOST} letters and digits, not {}", one_line(topic));
        return Err(Failed::Mine(why));
    }
    Ok(())
}

/// A payload of a note: 1 to [`PAYLOAD_MOST`] bytes. An empty one is a
/// clear, which is [`Board::clear`].
pub fn check_payload(payload: &[u8]) -> Result<(), Failed> {
    if payload.is_empty() || payload.len() > PAYLOAD_MOST {
        let why = format!("a note is 1 to {PAYLOAD_MOST} bytes, not {}", payload.len());
        return Err(Failed::Mine(why));
    }
    Ok(())
}

/// One connection of a board.
struct Link {
    session: RelaySession<url::Stream>,
    shown: String,
    /// The bytes of the broker that are no whole packet yet.
    pending: Vec<u8>,
    answer_limit: Duration,
    packet_most: usize,
    last_sent: Instant,
}

impl Link {
    /// Dial the broker at `point` and connect as a new client with a clean
    /// session. A `CONNACK` with a code other than 0 is [`Failed::Refused`].
    async fn open(point: &Point, config: &Config) -> Result<Link, Failed> {
        let asking = Asking { protocol: Some(PROTOCOL), limits: config.limits, ..Asking::default() };
        let dialed = url::dial_asking(&point.url, &asking, &config.way, &config.trust, config.connect_timeout, None);
        let session = dialed.await.map_err(|e| Failed::Unreachable(url::failure(&point.url, &e)))?;
        let mut link = Link {
            session,
            shown: point.url.shown(),
            pending: Vec::new(),
            answer_limit: config.answer_limit,
            packet_most: config.limits.message,
            last_sent: Instant::now(),
        };
        let login = point.login.as_ref().map(|l| (l.user.clone(), l.password.as_bytes().to_vec()));
        match link.greet(login).await {
            Ok(()) => Ok(link),
            Err(failed) => {
                let _ = link.session.send_close(1000, "").await;
                Err(failed)
            }
        }
    }

    async fn greet(&mut self, login: Option<(String, Vec<u8>)>) -> Result<(), Failed> {
        let keep_alive = u16::try_from(KEEP_ALIVE.as_secs()).unwrap_or(u16::MAX);
        self.write(&Packet::Connect(Connect { client_id: client_id()?, keep_alive, clean: true, login })).await?;
        match self.within("CONNACK", self.answer_limit, |p| matches!(p, Packet::ConnAck { .. })).await? {
            Packet::ConnAck { code: 0, .. } => Ok(()),
            Packet::ConnAck { code, .. } => {
                Err(Failed::Refused(format!("the broker {} refused this client: {}", self.shown, refusal(code))))
            }
            other => Err(self.unexpected(&other)),
        }
    }

    /// A retained `PUBLISH`, then a `PINGREQ` whose `PINGRESP` shows that
    /// the broker read it.
    async fn publish(&mut self, topic: &str, payload: Vec<u8>) -> Result<(), Failed> {
        self.write(&Packet::Publish { topic: topic.to_string(), payload, retain: true }).await?;
        self.write(&Packet::PingReq).await?;
        self.within("PINGRESP", self.answer_limit, |p| matches!(p, Packet::PingResp)).await?;
        Ok(())
    }

    async fn take(&mut self, topic: &str, wait: Duration) -> Result<Option<Vec<u8>>, Failed> {
        self.write(&Packet::Subscribe { id: SUBSCRIBE_ID, filters: vec![(topic.to_string(), 0)] }).await?;
        // A broker may send the retained note before the SUBACK.
        let mut early = None;
        let answer_limit = self.answer_limit;
        let deadline = Instant::now() + answer_limit;
        loop {
            match self.next_before(deadline, "SUBACK").await? {
                Packet::SubAck { id: SUBSCRIBE_ID, codes } if codes.first() != Some(&REFUSED) => break,
                Packet::SubAck { id: SUBSCRIBE_ID, .. } => {
                    let why = format!("the broker {} refused the subscription to the board's topic", self.shown);
                    return Err(Failed::Refused(why));
                }
                Packet::Publish { topic: at, payload, .. } if at == topic => {
                    early = early.or((!payload.is_empty()).then_some(payload));
                }
                Packet::Publish { .. } | Packet::PingResp => {}
                other => return Err(self.unexpected(&other)),
            }
        }
        if early.is_some() {
            return Ok(early);
        }
        let deadline = Instant::now() + wait;
        loop {
            let ping = self.last_sent + PING_EVERY;
            let read = tokio::select! {
                biased;
                read = self.read_packet() => Some(read?),
                () = tokio::time::sleep_until(deadline.min(ping)) => None,
            };
            match read {
                Some(Packet::Publish { topic: at, payload, .. }) if at == topic && !payload.is_empty() => {
                    return Ok(Some(payload));
                }
                Some(Packet::Publish { .. } | Packet::PingResp) => {}
                Some(other) => return Err(self.unexpected(&other)),
                None if Instant::now() >= deadline => return Ok(None),
                // A wait past half the keep-alive keeps the connection.
                None => self.write(&Packet::PingReq).await?,
            }
        }
    }

    /// The first packet that `wanted` takes within `limit`; a `PUBLISH` and
    /// a `PINGRESP` that come meanwhile are dropped.
    async fn within(
        &mut self,
        kind: &str,
        limit: Duration,
        wanted: impl Fn(&Packet) -> bool,
    ) -> Result<Packet, Failed> {
        let deadline = Instant::now() + limit;
        loop {
            let packet = self.next_before(deadline, kind).await?;
            if wanted(&packet) {
                return Ok(packet);
            }
            if !matches!(packet, Packet::Publish { .. } | Packet::PingResp) {
                return Err(self.unexpected(&packet));
            }
        }
    }

    async fn next_before(&mut self, deadline: Instant, kind: &str) -> Result<Packet, Failed> {
        match tokio::time::timeout_at(deadline, self.read_packet()).await {
            Ok(read) => read,
            Err(_) => Err(Failed::Broken(format!(
                "the broker {} did not answer {kind} within {:?}",
                self.shown, self.answer_limit
            ))),
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

    async fn read_message(&mut self) -> Result<Vec<u8>, Failed> {
        let f = self.session.read_frame().await.map_err(|e| self.broken(&e))?;
        match f.opcode {
            frame::OPCODE_BINARY => Ok(f.payload),
            frame::OPCODE_CLOSE => {
                let (code, reason) = close_code_and_reason(&f.payload);
                let code = code.map_or_else(|| "no code".to_string(), |c| c.to_string());
                let reason = one_line(&reason);
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

    /// `DISCONNECT`, then a Close: the broker drops the session with no
    /// will.
    async fn end(&mut self) {
        let _ = self.write(&Packet::Disconnect).await;
        let _ = self.session.send_close(1000, "").await;
    }

    fn unexpected(&self, packet: &Packet) -> Failed {
        Failed::Broken(format!("the broker {} sent a {} that this client did not ask for", self.shown, packet.name()))
    }

    fn broken(&self, e: &pod_ws::SessionError) -> Failed {
        Failed::Broken(format!("the broker {}: {e}", self.shown))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_topic_is_letters_and_digits_of_at_most_42() {
        assert!(check_topic("pw0123456789abcdef").is_ok());
        assert!(check_topic(&"a".repeat(TOPIC_MOST)).is_ok());
        for bad in ["", "a/b", "a+", "#", "$SYS", "a b", "é", &"a".repeat(TOPIC_MOST + 1)] {
            assert!(matches!(check_topic(bad), Err(Failed::Mine(_))), "{bad:?}");
        }
    }

    #[test]
    fn a_note_is_1_to_64000_bytes() {
        assert!(check_payload(&[0]).is_ok());
        assert!(check_payload(&vec![0; PAYLOAD_MOST]).is_ok());
        assert!(matches!(check_payload(&[]), Err(Failed::Mine(_))));
        assert!(matches!(check_payload(&vec![0; PAYLOAD_MOST + 1]), Err(Failed::Mine(_))));
    }
}

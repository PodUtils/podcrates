//! A client of a magic-wormhole mailbox, by its server protocol of version 1
//! (magic-wormhole/magic-wormhole-protocols:server-protocol.md, as
//! magic-wormhole-mailbox-server's `server_websocket.py` serves it): a JSON
//! object in each text message; `bind` with the caller's AppID and this end's
//! side; then `list`, `allocate`, `claim`, `open`, `add`, `release` and
//! `close` with a mood. Each command but `open` and `add` has an answer of
//! its own after its `ack`, and a `message` that comes meanwhile is kept for
//! the reader. It rides pod-ws's WebSocket through the caller's way out
//! ([`Config::way`]). The mailbox's own words, in a `welcome` or an
//! `error`, are shown on one line, cut short.

use std::collections::VecDeque;
use std::time::Duration;

use pod_ws::close::close_code_and_reason;
use pod_ws::url::{self, Asking, WsUrl};
use pod_ws::{RelaySession, SessionError, frame};
use serde_json::{Value, json};

use crate::config::Config;
use crate::message::{self, hex, words};
use crate::{Failed, phrase};

pub use crate::config::ANSWER_LIMIT;
pub use crate::message::Message;

/// The longest id of a mailbox taken: the server's own are 13 characters.
const MAILBOX_ID_MOST: usize = 128;

/// The most messages kept for the reader while a command waits for its
/// answer: a mailbox of magic-wormhole's server holds 32.
pub const KEPT_MOST: usize = 256;

/// How an end's exchange went, which `close` tells the mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    /// The keys agreed, and a sealed message of the peer opened.
    Happy,
    /// Nobody came.
    Lonely,
    /// A message of the peer did not open: a wrong code, or a guess.
    Scary,
    /// Anything else went wrong.
    Errory,
}

impl Mood {
    /// The word of the protocol.
    pub fn word(self) -> &'static str {
        match self {
            Mood::Happy => "happy",
            Mood::Lonely => "lonely",
            Mood::Scary => "scary",
            Mood::Errory => "errory",
        }
    }
}

/// A connection to one mailbox, bound to the caller's AppID and one side.
pub struct Mailbox {
    session: RelaySession<url::Stream>,
    /// The URL, as lines show it.
    pub shown: String,
    /// What the mailbox's welcome says to people, on one line.
    pub motd: Option<String>,
    /// The messages that came while a command waited for its answer.
    kept: VecDeque<Message>,
    sent: u16,
    answer_limit: Duration,
    appid: String,
}

impl Mailbox {
    /// Dial the mailbox at `url`, read its welcome, and bind as `side` to
    /// the AppID of `config`. A welcome with an `error`, or one that asks
    /// for a permission that this client does not give, is refused.
    pub async fn connect(url: &WsUrl, config: &Config, side: &str) -> Result<Mailbox, Failed> {
        let asking = Asking { limits: config.limits, ..Asking::default() };
        let dialed = url::dial_asking(url, &asking, &config.way, &config.trust, config.connect_timeout, None).await;
        let session = dialed.map_err(|e| Failed::Unreachable(url::failure(url, &e)))?;
        let mut mailbox = Mailbox {
            session,
            shown: url.shown(),
            motd: None,
            kept: VecDeque::new(),
            sent: 0,
            answer_limit: config.answer_limit,
            appid: config.appid.clone(),
        };
        let welcome = mailbox.answer("welcome").await?;
        mailbox.welcomed(&welcome)?;
        let version = json!([config.client_name, config.client_version]);
        mailbox.send(json!({ "type": "bind", "appid": config.appid, "side": side, "client_version": version })).await?;
        Ok(mailbox)
    }

    /// The AppID that this connection is bound to.
    pub fn appid(&self) -> &str {
        &self.appid
    }

    fn welcomed(&mut self, message: &Value) -> Result<(), Failed> {
        let welcome = &message["welcome"];
        if let Some(error) = welcome.get("error") {
            return Err(Failed::Refused(format!("the mailbox {} refused this client: {}", self.shown, words(error))));
        }
        // A server under load may ask for a proof of work (hashcash), which
        // this client does not make: the next mailbox is tried. One that takes
        // `none` too is used as it is.
        if let Some(asked) = welcome.get("permission-required")
            && asked.get("none").is_none()
        {
            let why = format!("the mailbox {} asks for a permission that this client does not give", self.shown);
            return Err(Failed::Refused(why));
        }
        self.motd = welcome.get("motd").map(words);
        Ok(())
    }

    /// The nameplates that the mailbox lists for the AppID. A server
    /// that lists none (`--disallow-list`) answers an empty list.
    pub async fn list(&mut self) -> Result<Vec<String>, Failed> {
        self.send(json!({ "type": "list" })).await?;
        let answer = self.answer("nameplates").await?;
        let ids = answer["nameplates"]
            .as_array()
            .map(|all| all.iter().filter_map(|nameplate| nameplate["id"].as_str()).map(str::to_string).collect());
        Ok(ids.unwrap_or_default())
    }

    /// A new nameplate, which the mailbox claims for this side too.
    pub async fn allocate(&mut self) -> Result<String, Failed> {
        self.send(json!({ "type": "allocate" })).await?;
        let answer = self.answer("allocated").await?;
        match answer["nameplate"].as_str() {
            Some(nameplate) if phrase::nameplate_ok(nameplate) => Ok(nameplate.to_string()),
            _ => Err(Failed::Broken(format!("the mailbox {} allocated no nameplate of digits", self.shown))),
        }
    }

    /// Claim `nameplate`: the id of its mailbox. A third side is refused
    /// (`crowded`, [`crowded`]).
    pub async fn claim(&mut self, nameplate: &str) -> Result<String, Failed> {
        self.send(json!({ "type": "claim", "nameplate": nameplate })).await?;
        let answer = self.answer("claimed").await?;
        match answer["mailbox"].as_str() {
            Some(id) if !id.is_empty() && id.len() <= MAILBOX_ID_MOST && id.bytes().all(|b| b.is_ascii_graphic()) => {
                Ok(id.to_string())
            }
            _ => Err(Failed::Broken(format!("the mailbox {} named no mailbox for the nameplate", self.shown))),
        }
    }

    /// Open the mailbox `id`: each message in it comes, then each new one.
    pub async fn open(&mut self, id: &str) -> Result<(), Failed> {
        self.send(json!({ "type": "open", "mailbox": id })).await
    }

    /// Add a message of `phase` to the open mailbox.
    pub async fn add(&mut self, phase: &str, body: &[u8]) -> Result<(), Failed> {
        self.send(json!({ "type": "add", "phase": phase, "body": hex(body) })).await
    }

    /// Release `nameplate`: once each side released it, it is free.
    pub async fn release(&mut self, nameplate: &str) -> Result<(), Failed> {
        self.send(json!({ "type": "release", "nameplate": nameplate })).await?;
        self.answer("released").await.map(|_| ())
    }

    /// Close the mailbox `id` with `mood`: once each side closed it, it is
    /// gone with its messages.
    pub async fn close(&mut self, id: &str, mood: Mood) -> Result<(), Failed> {
        self.send(json!({ "type": "close", "mailbox": id, "mood": mood.word() })).await?;
        self.answer("closed").await.map(|_| ())
    }

    /// End the connection with a Close.
    pub async fn end(&self) {
        let _ = self.session.send_close(1000, "").await;
    }

    /// The next message of the open mailbox, the ones kept first; `None`
    /// when none comes within `wait`. This end's own come back too, as the
    /// mailbox sends each message to each side.
    pub async fn message(&mut self, wait: Duration) -> Result<Option<Message>, Failed> {
        if let Some(kept) = self.kept.pop_front() {
            return Ok(Some(kept));
        }
        let next = tokio::time::timeout(wait, self.message_now()).await;
        match next {
            Ok(read) => read.map(Some),
            Err(_) => Ok(None),
        }
    }

    async fn message_now(&mut self) -> Result<Message, Failed> {
        loop {
            let message = self.read().await?;
            match message["type"].as_str().unwrap_or_default() {
                "message" => return self.message_of(&message),
                "error" => return Err(self.refused(&message)),
                // An ack, a pong, and each type that a later server may add.
                _ => {}
            }
        }
    }

    /// The answer of type `kind`, within [`Config::answer_limit`].
    async fn answer(&mut self, kind: &str) -> Result<Value, Failed> {
        let answered = tokio::time::timeout(self.answer_limit, self.answer_now(kind)).await;
        match answered {
            Ok(answer) => answer,
            Err(_) => {
                let limit = self.answer_limit;
                Err(Failed::Broken(format!("the mailbox {} did not answer {kind} within {limit:?}", self.shown)))
            }
        }
    }

    async fn answer_now(&mut self, kind: &str) -> Result<Value, Failed> {
        loop {
            let message = self.read().await?;
            let found = message["type"].as_str().unwrap_or_default().to_string();
            if found == kind {
                return Ok(message);
            }
            match found.as_str() {
                "message" => {
                    let kept = self.message_of(&message)?;
                    if self.kept.len() >= KEPT_MOST {
                        let why = format!("the mailbox {} sent more than {KEPT_MOST} messages at once", self.shown);
                        return Err(Failed::Broken(why));
                    }
                    self.kept.push_back(kept);
                }
                "error" => return Err(self.refused(&message)),
                // An ack of each command, a pong, a welcome not asked for,
                // and each type that a later server may add.
                _ => {}
            }
        }
    }

    /// The next JSON object of the mailbox; the connection pinged meanwhile,
    /// so that a dead one is found while this end waits.
    async fn read(&mut self) -> Result<Value, Failed> {
        let read = {
            let liveness = self.session.watch_liveness(pod_ws::LIVENESS_EVERY, pod_ws::LIVENESS_ALLOWED);
            tokio::select! {
                read = self.session.read_frame() => read,
                dead = liveness => Err(dead),
            }
        };
        let f = read.map_err(|e| self.broken(&e))?;
        match f.opcode {
            frame::OPCODE_TEXT | frame::OPCODE_BINARY => message::object(&f.payload)
                .ok_or_else(|| Failed::Broken(format!("the mailbox {} sent a message that is not JSON", self.shown))),
            frame::OPCODE_CLOSE => {
                let (code, reason) = close_code_and_reason(&f.payload);
                let code = code.map_or_else(|| "no code".to_string(), |c| c.to_string());
                let reason = pod_core::text::one_line(&reason);
                Err(Failed::Broken(format!("the mailbox {} closed the connection ({code} {reason})", self.shown)))
            }
            other => {
                Err(Failed::Broken(format!("the mailbox {} sent an unexpected frame (opcode {other:#x})", self.shown)))
            }
        }
    }

    async fn send(&mut self, mut command: Value) -> Result<(), Failed> {
        // The id is the timing tool's, copied into the ack: a count does.
        self.sent = self.sent.wrapping_add(1);
        command["id"] = Value::String(format!("{:04x}", self.sent));
        let text = command.to_string();
        self.session.send_text(&text).await.map_err(|e| self.broken(&e))
    }

    fn message_of(&self, message: &Value) -> Result<Message, Failed> {
        message::message_of(message).ok_or_else(|| {
            Failed::Broken(format!("the mailbox {} sent a message without its side, phase or body", self.shown))
        })
    }

    fn refused(&self, message: &Value) -> Failed {
        Failed::Refused(format!("the mailbox {} refused: {}", self.shown, words(&message["error"])))
    }

    fn broken(&self, e: &SessionError) -> Failed {
        Failed::Broken(format!("the mailbox {}: {e}", self.shown))
    }
}

/// Whether `failed` is the mailbox's refusal of a third side: two ends hold
/// the code already.
pub fn crowded(failed: &Failed) -> bool {
    matches!(failed, Failed::Refused(why) if why.ends_with(": crowded"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_moods_are_the_protocols_words() {
        let all = [Mood::Happy, Mood::Lonely, Mood::Scary, Mood::Errory].map(Mood::word);
        assert_eq!(all, ["happy", "lonely", "scary", "errory"]);
    }

    #[test]
    fn a_third_side_is_told_apart_from_each_other_refusal() {
        assert!(crowded(&Failed::Refused("the mailbox ws://h/v1 refused: crowded".into())));
        assert!(!crowded(&Failed::Refused("the mailbox ws://h/v1 refused: reclaimed".into())));
        assert!(!crowded(&Failed::Broken("crowded".into())));
    }
}

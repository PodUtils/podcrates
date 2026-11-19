//! Where the two ends of a code meet: a mailbox of magic-wormhole, the
//! topic of the code at an ntfy server ([`crate::ntfy`]), the tag of the
//! code at a Nostr relay ([`crate::nostr`]), the swarm of the code at a
//! WebTorrent tracker ([`crate::tracker`]), or the topic of the code at an
//! MQTT broker ([`crate::mqtt`]). The wormhole says the same messages at
//! each; a server, a relay, a tracker and a broker hold no nameplate and
//! have no mood.

use std::time::Duration;

use crate::Failed;
use crate::mailbox::{Mailbox, Message, Mood};
use crate::mqtt::Broker;
use crate::nostr::Relay;
use crate::ntfy::Topic;
use crate::tracker::Tracker;

/// A meeting point of one code.
pub enum Place {
    Mailbox(Mailbox),
    Ntfy(Topic),
    Relay(Relay),
    Tracker(Tracker),
    Broker(Broker),
}

impl From<Mailbox> for Place {
    fn from(mailbox: Mailbox) -> Place {
        Place::Mailbox(mailbox)
    }
}

impl From<Topic> for Place {
    fn from(topic: Topic) -> Place {
        Place::Ntfy(topic)
    }
}

impl From<Relay> for Place {
    fn from(relay: Relay) -> Place {
        Place::Relay(relay)
    }
}

impl From<Tracker> for Place {
    fn from(tracker: Tracker) -> Place {
        Place::Tracker(tracker)
    }
}

impl From<Broker> for Place {
    fn from(broker: Broker) -> Place {
        Place::Broker(broker)
    }
}

impl Place {
    /// The URL, as lines show it.
    pub fn shown(&self) -> &str {
        match self {
            Place::Mailbox(m) => &m.shown,
            Place::Ntfy(n) => &n.shown,
            Place::Relay(r) => &r.shown,
            Place::Tracker(t) => &t.shown,
            Place::Broker(b) => &b.shown,
        }
    }

    /// The word of the kind, for a line: `mailbox`, `ntfy server`, `Nostr
    /// relay`, `WebTorrent tracker` or `broker`.
    /// What this end sent and heard at the meeting point, where the
    /// point counts it: a Nostr relay.
    pub fn tally(&self) -> Option<String> {
        match self {
            Place::Relay(r) => Some(r.tally()),
            Place::Mailbox(_) | Place::Ntfy(_) | Place::Tracker(_) | Place::Broker(_) => None,
        }
    }

    pub fn noun(&self) -> &'static str {
        match self {
            Place::Mailbox(_) => "mailbox",
            Place::Ntfy(_) => "ntfy server",
            Place::Relay(_) => "Nostr relay",
            Place::Tracker(_) => "WebTorrent tracker",
            Place::Broker(_) => "broker",
        }
    }

    /// The AppID of the ends: SPAKE2's identity.
    pub fn appid(&self) -> &str {
        match self {
            Place::Mailbox(m) => m.appid(),
            Place::Ntfy(n) => n.appid(),
            Place::Relay(r) => r.appid(),
            Place::Tracker(t) => t.appid(),
            Place::Broker(b) => b.appid(),
        }
    }

    /// Open the mailbox `id`; a server, a relay, a tracker and a broker
    /// read the code's topic, tag or swarm from their connection.
    pub async fn open(&mut self, id: &str) -> Result<(), Failed> {
        match self {
            Place::Mailbox(m) => m.open(id).await,
            Place::Ntfy(_) | Place::Relay(_) | Place::Tracker(_) | Place::Broker(_) => Ok(()),
        }
    }

    /// Add a message of `phase` for the other end.
    pub async fn add(&mut self, phase: &str, body: &[u8]) -> Result<(), Failed> {
        match self {
            Place::Mailbox(m) => m.add(phase, body).await,
            Place::Ntfy(n) => n.add(phase, body).await,
            Place::Relay(r) => r.add(phase, body).await,
            Place::Tracker(t) => t.add(phase, body).await,
            Place::Broker(b) => b.add(phase, body).await,
        }
    }

    /// The next message, within `wait`. A mailbox gives this end's own
    /// messages too; a server, a relay, a tracker and a broker do not.
    pub async fn message(&mut self, wait: Duration) -> Result<Option<Message>, Failed> {
        match self {
            Place::Mailbox(m) => m.message(wait).await,
            Place::Ntfy(n) => n.message(wait).await,
            Place::Relay(r) => r.message(wait).await,
            Place::Tracker(t) => t.message(wait).await,
            Place::Broker(b) => b.message(wait).await,
        }
    }

    /// Release `nameplate` at a mailbox.
    pub async fn release(&mut self, nameplate: &str) -> Result<(), Failed> {
        match self {
            Place::Mailbox(m) => m.release(nameplate).await,
            Place::Ntfy(_) | Place::Relay(_) | Place::Tracker(_) | Place::Broker(_) => Ok(()),
        }
    }

    /// Close the mailbox `id` with `mood`, end the stream of the server or
    /// the subscription at the relay, leave the swarm at the tracker, or
    /// disconnect from the broker.
    pub async fn close(&mut self, id: &str, mood: Mood) -> Result<(), Failed> {
        match self {
            Place::Mailbox(m) => m.close(id, mood).await,
            Place::Ntfy(n) => {
                n.close().await;
                Ok(())
            }
            Place::Relay(r) => {
                r.close().await;
                Ok(())
            }
            Place::Tracker(t) => {
                t.close(mood).await;
                Ok(())
            }
            Place::Broker(b) => {
                b.disconnect().await;
                Ok(())
            }
        }
    }

    /// End the connection with a Close.
    pub async fn end(&mut self) {
        match self {
            Place::Mailbox(m) => m.end().await,
            Place::Ntfy(n) => n.end().await,
            Place::Relay(r) => r.end().await,
            Place::Tracker(t) => t.end().await,
            Place::Broker(b) => b.end().await,
        }
    }
}

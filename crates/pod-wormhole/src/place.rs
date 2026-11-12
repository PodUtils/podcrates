//! Where the two ends of a code meet: a mailbox of magic-wormhole, or the
//! topic of the code at an MQTT broker ([`crate::mqtt`]). The wormhole says
//! the same messages at each; a broker holds no nameplate and has no mood.

use std::time::Duration;

use crate::Failed;
use crate::mailbox::{Mailbox, Message, Mood};
use crate::mqtt::Broker;

/// A meeting point of one code.
pub enum Place {
    Mailbox(Mailbox),
    Broker(Broker),
}

impl From<Mailbox> for Place {
    fn from(mailbox: Mailbox) -> Place {
        Place::Mailbox(mailbox)
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
            Place::Broker(b) => &b.shown,
        }
    }

    /// The word of the kind, for a line: `mailbox` or `broker`.
    pub fn noun(&self) -> &'static str {
        match self {
            Place::Mailbox(_) => "mailbox",
            Place::Broker(_) => "broker",
        }
    }

    /// The AppID of the ends: SPAKE2's identity.
    pub fn appid(&self) -> &str {
        match self {
            Place::Mailbox(m) => m.appid(),
            Place::Broker(b) => b.appid(),
        }
    }

    /// Open the mailbox `id`; a broker subscribed to the code's topic at its
    /// connection.
    pub async fn open(&mut self, id: &str) -> Result<(), Failed> {
        match self {
            Place::Mailbox(m) => m.open(id).await,
            Place::Broker(_) => Ok(()),
        }
    }

    /// Add a message of `phase` for the other end.
    pub async fn add(&mut self, phase: &str, body: &[u8]) -> Result<(), Failed> {
        match self {
            Place::Mailbox(m) => m.add(phase, body).await,
            Place::Broker(b) => b.add(phase, body).await,
        }
    }

    /// The next message, within `wait`. A mailbox gives this end's own
    /// messages too; a broker does not.
    pub async fn message(&mut self, wait: Duration) -> Result<Option<Message>, Failed> {
        match self {
            Place::Mailbox(m) => m.message(wait).await,
            Place::Broker(b) => b.message(wait).await,
        }
    }

    /// Release `nameplate` at a mailbox.
    pub async fn release(&mut self, nameplate: &str) -> Result<(), Failed> {
        match self {
            Place::Mailbox(m) => m.release(nameplate).await,
            Place::Broker(_) => Ok(()),
        }
    }

    /// Close the mailbox `id` with `mood`, or disconnect from the broker.
    pub async fn close(&mut self, id: &str, mood: Mood) -> Result<(), Failed> {
        match self {
            Place::Mailbox(m) => m.close(id, mood).await,
            Place::Broker(b) => {
                b.disconnect().await;
                Ok(())
            }
        }
    }

    /// End the connection with a Close.
    pub async fn end(&self) {
        match self {
            Place::Mailbox(m) => m.end().await,
            Place::Broker(b) => b.end().await,
        }
    }
}

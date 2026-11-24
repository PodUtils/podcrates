//! A meeting point of the two ends of a code at a public ntfy server, for a
//! caller whose way out refuses the WebSocket upgrade: a topic of the
//! server, written by a POST and read by a GET of plain HTTPS on port 443,
//! with no account. A proxy that refuses the upgrade still passes both.
//!
//! * The giver draws a nameplate of [`crate::mqtt::NAMEPLATE_DIGITS`]
//!   digits ([`crate::mqtt::nameplate`]) and makes the code. The topic of
//!   the code is [`crate::mqtt::topic`] of the AppID and the nameplate,
//!   never of the words, as each client of a public server can read each
//!   topic. Each end reads the topic with the letter of its direction
//!   ([`Role::topics`]): 43 letters and digits, which ntfy takes (64 at
//!   most).
//! * Each message of the mailbox ([`crate::wormhole`]) is one POST whose
//!   body is its JSON ([`wire::content`]): the side, the phase and the body
//!   in hex, at most [`MESSAGE_MOST`] bytes; a longer one fails before the
//!   send. The server keeps each message ([`link`]): a message that came
//!   before the other end read is not lost.
//! * On HTTP/2 one connection holds the stream of `TOPIC/json` and the
//!   POSTs. On HTTP/1.1 the polls of `TOPIC/json?poll=1` and the POSTs take
//!   turns on one connection, each [`Pace::poll`] (the giver waits
//!   [`Pace::waiting`] between two polls, the rate at which a visitor of
//!   ntfy gets its requests back).
//! * The giver (the waiting end) reads at the first server that answers
//!   and holds its messages until the taker is heard; the taker reads and
//!   sends at each server in order. A stranger at the topic gets one guess
//!   of the words, as at a mailbox, and the server sees ciphertext.
//!
//! The servers are the caller's (`pod-services` has the public ones, of the
//! kind `ntfy-server`).

pub mod conn;
pub mod link;
pub mod wire;

use std::time::Duration;

use pod_core::Event;
use pod_core::entropy::OsEntropy;
use pod_ws::url::WsUrl;
use serde_json::Value;

pub use link::Topic;

use crate::config::Config;
use crate::mailbox::Mood;
use crate::meet::{new_side, reason};
use crate::mqtt::{Role, nameplate, topic};
use crate::wormhole::Wormhole;
use crate::{Code, Failed, Missed};

/// The largest message of a topic: one byte under ntfy's
/// `message-size-limit` (4,096 bytes), as ntfy keeps a body of the whole
/// limit as an attachment (`util.Peek` marks the limit reached), and a
/// server with no attachments refuses it.
pub const MESSAGE_MOST: usize = 4095;
/// The wait between two polls on HTTP/1.1 while the other end speaks.
pub const POLL: Duration = Duration::from_secs(1);
/// The wait between two polls of the giver on HTTP/1.1 until the taker is
/// heard: a visitor of ntfy gets one request back each 5 s, after a burst
/// of 60.
pub const WAITING: Duration = Duration::from_secs(5);
/// The wait after an answer `429` before the same request again.
pub const RETRY: Duration = Duration::from_secs(5);
/// How far before this end's start the reads go: the clocks of the host
/// and of the server differ.
pub const SINCE_SLACK: Duration = Duration::from_secs(60);

/// An ntfy server, as the caller names it: `https://HOST[:PORT][/PATH]`,
/// or `http://` on the loopback of a test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    /// TLS, checked for `host`.
    pub tls: bool,
    pub host: String,
    pub port: u16,
    /// The path before each topic, with no `/` at its end: empty at the
    /// root.
    pub base: String,
}

impl Point {
    /// `https://` or `http://`, a host, a port when it is not the scheme's
    /// own, and a path. A user, a password, a query and a fragment are
    /// refused: a credential does not go in a URL.
    pub fn parse(text: &str) -> Result<Point, String> {
        let lower = text.get(..8).unwrap_or(text).to_ascii_lowercase();
        let (tls, rest) = if lower.starts_with("https://") {
            (true, &text[8..])
        } else if lower.starts_with("http://") {
            (false, &text[7..])
        } else {
            return Err("not the URL of an ntfy server, which is https://HOST[:PORT][/PATH]".into());
        };
        if rest.contains(['?', '#']) {
            return Err("the URL of an ntfy server takes no query and no fragment".into());
        }
        let ws = WsUrl::parse(&format!("{}://{rest}", if tls { "wss" } else { "ws" }))
            .map_err(|why| why.replace("a WebSocket's URL", "the URL of an ntfy server"))?;
        let base = ws.target.trim_end_matches('/').to_string();
        Ok(Point { tls, host: ws.host, port: ws.port, base })
    }

    /// The URL, as lines show it.
    pub fn shown(&self) -> String {
        format!("{}://{}{}/", if self.tls { "https" } else { "http" }, self.host_header(), self.base)
    }

    /// The value of `Host`: the port only when it is not the scheme's own,
    /// an IPv6 address in brackets.
    pub fn host_header(&self) -> String {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        if self.port == if self.tls { 443 } else { 80 } { host } else { format!("{host}:{}", self.port) }
    }

    /// The request-target of `rest` (a topic and what follows it) under
    /// the path of the server.
    pub fn target(&self, rest: &str) -> String {
        format!("{}/{rest}", self.base)
    }
}

/// How often an end asks on HTTP/1.1, and how long it waits after a
/// limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pace {
    /// The wait between two polls while the other end speaks ([`POLL`]).
    pub poll: Duration,
    /// The giver's wait between two polls until the taker is heard
    /// ([`WAITING`]).
    pub waiting: Duration,
    /// The wait after an answer `429` ([`RETRY`]).
    pub retry: Duration,
}

impl Default for Pace {
    fn default() -> Pace {
        Pace { poll: POLL, waiting: WAITING, retry: RETRY }
    }
}

/// Where the ends meet at ntfy servers, and how they are reached.
pub struct Servers {
    /// The servers, in order.
    pub points: Vec<Point>,
    pub config: Config,
    /// The waits of the polls and after a limit.
    pub pace: Pace,
    /// Offer HTTP/2 by ALPN (the default); `false` asks for HTTP/1.1, as
    /// a test of the polls does.
    pub h2: bool,
}

impl Servers {
    /// The servers `points` with `config`, at the default [`Pace`], HTTP/2
    /// offered.
    pub fn new(points: Vec<Point>, config: Config) -> Servers {
        Servers { points, config, pace: Pace::default(), h2: true }
    }
}

/// A new code, and its wormhole at the first server of `servers` that
/// answers, reading the topic and waiting for the taker: what a giver
/// shows when no mailbox answers. `versions` goes in its version phase.
pub async fn opened(servers: &Servers, versions: Value) -> Result<(Wormhole, Code), Failed> {
    let side = new_side()?;
    let plate = nameplate(&mut OsEntropy).map_err(|_| Failed::NoRandom)?;
    // A count of words out of its bounds fails here, before a server.
    let code = Code::make(&plate, servers.config.words, &mut OsEntropy)?;
    let tag = topic(&servers.config.appid, &plate);
    let mut missed = Vec::new();
    for point in &servers.points {
        let topic = match Topic::connect(point, servers, &side, &tag, Role::Giver).await {
            Ok(topic) => topic,
            Err(why) => {
                say(servers, Event::warn("wormhole.ntfy", reason(&why)));
                missed.push(Missed::of(why));
                continue;
            }
        };
        let hole = Wormhole::open(topic, side, plate.clone(), tag, &code, versions).await?;
        return Ok((hole, code));
    }
    Err(Failed::Unreachable(Missed::join(missed)))
}

/// The wormhole of `code` at the server where its giver waits, the key
/// made: at each server of `servers` in order, the taker's first message
/// sent and the giver's answer read within `giver`.
pub async fn joined(servers: &Servers, code: &Code, versions: Value, giver: Duration) -> Result<Wormhole, Failed> {
    let side = new_side()?;
    let tag = topic(&servers.config.appid, code.nameplate());
    let (mut missed, mut reached) = (Vec::new(), false);
    for point in &servers.points {
        let topic = match Topic::connect(point, servers, &side, &tag, Role::Taker).await {
            Ok(topic) => topic,
            Err(why) => {
                say(servers, Event::warn("wormhole.ntfy", reason(&why)));
                missed.push(Missed::of(why));
                continue;
            }
        };
        reached = true;
        let nameplate = code.nameplate().to_string();
        let mut hole = Wormhole::open(topic, side.clone(), nameplate, tag.clone(), code, versions.clone()).await?;
        if hole.agree(giver).await? {
            return Ok(hole);
        }
        hole.close(Mood::Lonely).await;
    }
    if reached { Err(Failed::Nobody) } else { Err(Failed::Unreachable(Missed::join(missed))) }
}

fn say(servers: &Servers, event: Event) {
    servers.config.events.event(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mqtt;

    #[test]
    fn a_point_is_an_https_url_with_no_credential() {
        let p = Point::parse("https://ntfy.sh").unwrap();
        assert_eq!((p.tls, p.host.as_str(), p.port, p.base.as_str()), (true, "ntfy.sh", 443, ""));
        assert_eq!((p.shown(), p.target("pwabc/json")), ("https://ntfy.sh/".to_string(), "/pwabc/json".to_string()));
        let p = Point::parse("HTTP://127.0.0.1:8080/ntfy/").unwrap();
        assert_eq!(
            (p.tls, p.port, p.base.as_str(), p.shown()),
            (false, 8080, "/ntfy", "http://127.0.0.1:8080/ntfy/".into())
        );
        assert_eq!(p.target("t"), "/ntfy/t");
        for bad in
            ["wss://ntfy.sh/", "https://user:pw@ntfy.sh/", "https://ntfy.sh/?auth=x", "https://ntfy.sh/#x", "ntfy.sh"]
        {
            assert!(Point::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_topic_of_a_code_is_one_that_ntfy_takes_and_holds_no_word() {
        // ntfy's topicRegex: 1 to 64 of [-_A-Za-z0-9].
        let tag = topic("example.org/app", "1234");
        for t in [Role::Giver.topics(&tag), Role::Taker.topics(&tag)].iter().flat_map(|(a, b)| [a, b]) {
            assert!(t.len() <= 64 && t.bytes().all(|b| b.is_ascii_alphanumeric()), "{t}");
        }
        assert!(Role::Giver.topics(&tag).0.ends_with(mqtt::TO_GIVER));
        const { assert!(SINCE_SLACK.as_secs() <= 60 && WAITING.as_secs() >= 5 && POLL.as_secs() >= 1) };
    }
}

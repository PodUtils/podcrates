//! The facts of an SSH forward relay, and the reader of its banner. A
//! forward relay takes a remote forward (`ssh -R`) and writes the URL of
//! the tunnel in the text of a session channel, its banner (README, "SSH
//! forward relays"). The facts are data of the service: this code knows no
//! relay.

use std::time::Duration;

use crate::{Endpoint, Service};

/// The scheme of an endpoint of plain SSH on TCP.
const SSH: &str = "ssh";

/// A service read as an SSH forward relay.
#[derive(Clone, Copy, Debug)]
pub struct Relay<'a> {
    service: &'a Service,
}

impl<'a> Relay<'a> {
    /// The service as a forward relay: it has an endpoint of plain SSH and
    /// the fact `url-match` or `url-skip`. None for another service.
    pub fn of(service: &'a Service) -> Option<Relay<'a>> {
        let rule = service.texts("url-match").is_some() || service.texts("url-skip").is_some();
        let ssh = service.endpoints().iter().any(|e| e.scheme() == SSH);
        (rule && ssh).then_some(Relay { service })
    }

    pub fn service(&self) -> &'a Service {
        self.service
    }

    /// The endpoints of plain SSH, in the order of the file: the order to
    /// try. An endpoint on port 443 comes first where the relay has one.
    pub fn endpoints(&self) -> impl Iterator<Item = &'a Endpoint> + 'a {
        self.service.endpoints().iter().filter(|e| e.scheme() == SSH)
    }

    /// The SSH user of a free tunnel (the fact `user`). None: the relay
    /// takes any user, and the consumer gives its own word.
    pub fn user(&self) -> Option<&'a str> {
        self.service.text("user")
    }

    /// The rule of the URL of the tunnel (the facts `url-match` and
    /// `url-skip`; a missing fact is an empty list).
    pub fn url_rule(&self) -> UrlRule {
        let texts = |name| self.service.texts(name).unwrap_or_default().iter().map(|t| t.to_string()).collect();
        UrlRule { matches: texts("url-match"), skips: texts("url-skip") }
    }

    /// The time after which the relay ends a free tunnel (the fact
    /// `free-tunnel-minutes`). None: no source states a limit.
    pub fn session_limit(&self) -> Option<Duration> {
        let minutes = u64::try_from(self.service.int("free-tunnel-minutes")?).ok()?;
        minutes.checked_mul(60).map(Duration::from_secs)
    }

    /// The port of the remote forward that the relay takes (the fact
    /// `remote-port`): a relay that refuses port 0 names it. None: the
    /// relay states none, and the consumer's own choice stands.
    pub fn remote_port(&self) -> Option<u16> {
        u16::try_from(self.service.int("remote-port")?).ok()
    }

    /// The interval of the keepalive of the relay's own command (the fact
    /// `keepalive-seconds`). None: no source states one.
    pub fn keepalive(&self) -> Option<Duration> {
        u64::try_from(self.service.int("keepalive-seconds")?).ok().map(Duration::from_secs)
    }

    /// The command of the session channel in place of a shell (the fact
    /// `command`): the relay writes the URL in a form for programs. None:
    /// a shell.
    pub fn command(&self) -> Option<&'a str> {
        self.service.text("command")
    }

    /// Whether the session channel must open before the remote forward
    /// (the fact `session-first`): the relay answers the forward only after
    /// a session is open. False when the fact is not there.
    pub fn session_first(&self) -> bool {
        self.service.bool("session-first").unwrap_or(false)
    }
}

/// Which URL of a banner is the URL of the tunnel: a URL that holds each
/// text of `matches` and no text of `skips`. These are podnet's flags
/// `--url-match` (each one held) and `--url-skip` (none held). An empty
/// rule takes each URL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UrlRule {
    pub matches: Vec<String>,
    pub skips: Vec<String>,
}

impl UrlRule {
    /// Whether `url` is the URL of the tunnel.
    pub fn accepts(&self, url: &str) -> bool {
        self.matches.iter().all(|m| url.contains(m.as_str())) && !self.skips.iter().any(|s| url.contains(s.as_str()))
    }

    /// The URL of the tunnel in a whole text, as [`Banner`] reads it, or
    /// none.
    pub fn pick(&self, text: &str) -> Option<String> {
        let mut banner = Banner::new(self.clone());
        banner.push(text.as_bytes());
        banner.finish().map(str::to_string)
    }
}

/// The reader of the banner of a relay, which comes in chunks: it gives the
/// first URL that the rule accepts.
///
/// A URL is one word, so the reader holds the word that a chunk cut, and no
/// more of its line, until a later chunk ends it. A word ends at a space,
/// an end of line or another control byte (also the escape of a color).
/// The URL of a word starts at its first `http://` or `https://`, and the
/// marks of a sentence at its end are not part of it.
#[derive(Clone, Debug)]
pub struct Banner {
    rule: UrlRule,
    word: Vec<u8>,
    /// The word grew past [`Banner::MOST_HELD`]: its rest is passed over.
    over: bool,
    url: Option<String>,
}

impl Banner {
    /// The most bytes of one word that the reader holds. A longer word is
    /// no URL of a tunnel: the reader passes over it to its end, so that a
    /// banner with no end of line holds no more than this.
    pub const MOST_HELD: usize = 2048;

    pub fn new(rule: UrlRule) -> Banner {
        Banner { rule, word: Vec::new(), over: false, url: None }
    }

    /// Read the next chunk. The URL, when this chunk or an earlier one
    /// ended it; after that, the reader reads no more.
    pub fn push(&mut self, chunk: &[u8]) -> Option<&str> {
        let mut rest = chunk;
        while self.url.is_none() {
            let Some(at) = rest.iter().position(|&b| ends_word(b)) else {
                self.hold(rest);
                break;
            };
            self.hold(&rest[..at]);
            self.end_word();
            rest = &rest[at + 1..];
        }
        self.url.as_deref()
    }

    /// The end of the banner (the channel closed): the held word is whole.
    /// The URL, or none.
    pub fn finish(&mut self) -> Option<&str> {
        if self.url.is_none() {
            self.end_word();
        }
        self.url.as_deref()
    }

    /// The URL, once a chunk ended it.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// The bytes of the word that the reader holds now.
    pub fn held(&self) -> usize {
        self.word.len()
    }

    fn hold(&mut self, bytes: &[u8]) {
        if self.over || bytes.is_empty() {
            return;
        }
        if self.word.len() + bytes.len() > Self::MOST_HELD {
            self.word.clear();
            self.over = true;
        } else {
            self.word.extend_from_slice(bytes);
        }
    }

    fn end_word(&mut self) {
        if !self.over
            && let Some(url) = url_of(&self.word)
            && self.rule.accepts(url)
        {
            self.url = Some(url.to_string());
        }
        self.word.clear();
        self.over = false;
    }
}

/// A byte that ends a word: a space, or an ASCII control byte (an end of
/// line, a tab, the escape of a color).
fn ends_word(byte: u8) -> bool {
    byte == b' ' || byte.is_ascii_control()
}

/// The bytes that RFC 3986 keeps out of a URL. One ends the URL of a word:
/// a JSON line has no space after the quote that closes a URL.
const NOT_IN_URL: &[u8] = b"\"<>\\^`{|}";

/// The URL in a word: from its first `http://` or `https://` to its end or
/// to a byte of [`NOT_IN_URL`], with no mark of a sentence at the end, and
/// a host after the scheme. None for a word with no URL, or with a URL that
/// is not UTF-8.
fn url_of(word: &[u8]) -> Option<&str> {
    let start = [&b"http://"[..], b"https://"].iter().filter_map(|scheme| find(word, scheme)).min()?;
    let word = &word[start..];
    let end = word.iter().position(|b| NOT_IN_URL.contains(b)).unwrap_or(word.len());
    let url = std::str::from_utf8(&word[..end]).ok()?;
    let url = url.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '>', '"', '\'']);
    let (_, host) = url.split_once("://")?;
    (!host.is_empty() && !host.starts_with('/')).then_some(url)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|window| window == needle)
}

//! The client of the road: the open, then the tasks of the session in the
//! one-POST form ([`super::one`]), the split form (`split.rs`) or the serial
//! form (`serial.rs`), between a pipe in memory, whose other end is the
//! caller's stream, and the relay host. With no form asked, the one-POST
//! form goes first where the host may serve it and the run did not learn
//! that its way holds it; then the split form: its second connection must
//! answer a first request within a bound, else the session goes on in the
//! serial form on the first connection (podssh Q44).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_ws::{ConnectError, Trust, Way};
use tokio::io::{AsyncRead, AsyncReadExt, DuplexStream};
use tokio::time::Instant;
use zeroize::Zeroizing;

use super::answer::Head;
use super::conn::{ALPN_OFFER, BoxIo, Conn, Dial, DialAlpn, Dialed, Dialing, DialingAlpn, request};
use super::one::{self, Learned};
use super::window::Refusal;
use super::{DOWN_MAX, Form, Forms, KEY, ONE, OPEN, REFUSE, SESSION, TOTAL, Tuning, WAIT_MAX, is_id, is_key};
use crate::context::Context;
use crate::relay::Relay;

/// The bound of each step of the open, and of the split form's first
/// request on its second connection.
pub const LIMIT: Duration = Duration::from_secs(20);
/// The pipe in memory, each way.
const PIPE: usize = 256 * 1024;
/// The longest answer to the open.
const OPEN_ANSWER_MAX: usize = 16 * 1024;
/// How long more bytes of the caller may take to join a batch.
const GATHER: Duration = Duration::from_millis(2);
/// How long a request is tried again with no answer before the session ends.
pub const RETRY_FOR: Duration = Duration::from_secs(60);
/// The first pause between two tries, which doubles to [`PAUSE_MAX`].
const PAUSE_FIRST: Duration = Duration::from_millis(100);
const PAUSE_MAX: Duration = Duration::from_secs(2);

/// Where the road goes: how to reach the relay host, and its name in `Host`.
#[derive(Clone)]
pub struct Road {
    /// The dial of each form on HTTP/1.1: it offers no ALPN.
    pub dial: Dial,
    /// The dial of the one-POST form, which offers `h2` and `http/1.1` and
    /// says which the host chose: one stream of HTTP/2 where it chose `h2`.
    /// None: the form goes on [`Road::dial`], on HTTP/1.1.
    pub dial_alpn: Option<DialAlpn>,
    pub authority: String,
    /// The `User-Agent` of each request: the caller's.
    pub user_agent: String,
    /// The forms that the host's `limits.http.forms` names; none while the
    /// host is not known.
    pub forms: Option<Forms>,
    /// The hosts whose way held the one-POST form: the run's, where the
    /// road takes them from its context.
    pub learned: Arc<Learned>,
}

impl Road {
    /// TLS to `relay` through `way`, checked with `trust`, as the WebSocket
    /// road reaches it.
    pub fn tls(way: &Way, relay: &Relay, trust: &Trust, limit: Duration) -> Road {
        let (host, port, trust, way) = (relay.host.clone(), relay.port, trust.clone(), way.clone());
        let user_agent = way.user_agent.clone();
        let (dial_host, dial_trust, dial_way) = (host.clone(), trust.clone(), way.clone());
        let dial: Dial = Arc::new(move || -> Dialing {
            let (host, trust, way) = (dial_host.clone(), dial_trust.clone(), dial_way.clone());
            Box::pin(async move {
                let (tls, _) = way.open_tls(&host, port, &host, &trust, limit, &[]).await?;
                Ok(Box::new(tls) as BoxIo)
            })
        });
        let dial_alpn: DialAlpn = Arc::new(move || -> DialingAlpn {
            let (host, trust, way) = (host.clone(), trust.clone(), way.clone());
            Box::pin(async move {
                let (tls, _) = way.open_tls(&host, port, &host, &trust, limit, ALPN_OFFER).await?;
                let alpn = tls.get_ref().1.alpn_protocol().map(<[u8]>::to_vec);
                Ok(Dialed { io: Box::new(tls) as BoxIo, alpn })
            })
        });
        let (authority, forms, learned) = (relay.name(), None, Arc::default());
        Road { dial, dial_alpn: Some(dial_alpn), authority, user_agent, forms, learned }
    }

    /// The same road with the forms that the host's `limits.http.forms`
    /// names (`pool::Limits::http_forms`).
    pub fn with_forms(mut self, forms: Option<Forms>) -> Road {
        self.forms = forms;
        self
    }

    /// The same road with what the run of `ctx` learned of each host, so
    /// that a way that held the one-POST form takes the split form at once
    /// for the rest of the run.
    pub fn in_run(mut self, ctx: &Context) -> Road {
        self.learned = ctx.http_road_learned();
        self
    }
}

/// The target of a session.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    pub host: &'a str,
    pub port: u16,
    /// 4 or 6, as `?family` asks the role.
    pub family: Option<u8>,
}

/// How a session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// The role's `X-Close`: the forward road's code and reason.
    Closed { code: u16, reason: String },
    /// The road failed: why.
    Failed(String),
}

/// The end of a session, once it came; the first word wins.
#[derive(Debug, Default)]
pub struct Ended(Mutex<Option<End>>, tokio::sync::Notify);

impl Ended {
    pub fn set_once(&self, end: End) {
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            *slot = Some(end);
            self.1.notify_waiters();
        }
    }

    pub fn get(&self) -> Option<End> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Wait until the session ended.
    pub async fn wait(&self) {
        loop {
            // Made before the look, so that an end between the two wakes it.
            let woken = self.1.notified();
            if self.get().is_some() {
                return;
            }
            woken.await;
        }
    }
}

/// An open session: the caller's end of the pipe, the session's end once it
/// comes, and the form that carries it.
pub struct Opened {
    pub stream: DuplexStream,
    pub ended: Arc<Ended>,
    pub form: Form,
}

/// What the tasks of one session share.
pub(super) struct Shared {
    pub id: String,
    /// Sent in a header alone, never in a URL.
    pub key: Zeroizing<String>,
    pub authority: String,
    pub user_agent: String,
    pub batch: usize,
    pub wait: Duration,
    pub down_max: usize,
    pub poll_max: Duration,
    pub ended: Arc<Ended>,
    /// The role's last `X-Ack`: the batches that the target took.
    pub acked: AtomicU64,
    /// The bytes down that the caller's pipe holds, or the serial form's
    /// channel to it: the offset of the next request down, which the role's
    /// count at its end must equal.
    pub down_at: AtomicU64,
}

impl Shared {
    /// The path of `what` of this session, with `query`.
    pub fn path(&self, what: &str, query: &str) -> String {
        format!("{SESSION}{}/{what}?{query}", self.id)
    }

    /// A request of this session, with its key.
    pub fn request(&self, method: &str, path: &str, length: Option<usize>) -> Vec<u8> {
        request(method, &self.user_agent, &self.authority, path, &[(KEY, self.key.as_str())], length)
    }

    /// What an answer says of the batches that the target took.
    pub fn heard(&self, answer: &Head) {
        if let Some(acked) = answer.number(super::ACK) {
            self.acked.fetch_max(acked, Ordering::Relaxed);
        }
    }

    pub fn end(&self, end: End) {
        self.ended.set_once(end);
    }

    /// The caller's pipe holds each byte down before `at`.
    pub fn came_down(&self, at: u64) {
        self.down_at.store(at, Ordering::Release);
    }
}

/// The end that a `410` says, or a failure that names it. The role's
/// `1000` comes once the client has each byte down; its count must be the
/// bytes that came (invariant 9), else the end is a failure, never a clean
/// one.
pub(super) fn closed(answer: &Head, s: &Shared) -> End {
    match answer.header(super::CLOSE).and_then(super::parse_close) {
        Some((1000, reason)) => {
            let at = s.down_at.load(Ordering::Acquire);
            match answer.number(TOTAL) {
                Some(total) if total == at => End::Closed { code: 1000, reason },
                Some(total) => End::Failed(format!(
                    "the relay ended the session of the HTTP road after {total} bytes down, where {at} came: \
                     the counts differ"
                )),
                None => {
                    End::Failed("the relay ended the session of the HTTP road with no count of its bytes down".into())
                }
            }
        }
        Some((code, reason)) => End::Closed { code, reason },
        None => End::Failed("the relay ended the session of the HTTP road, and said no reason".into()),
    }
}

/// What a client does after an answer that is no success and no `410`.
pub(super) enum Then {
    /// The same request again, after a pause of this length at least.
    Again(String, Option<Duration>),
    Stop(End),
}

/// The next step after `answer`, whose body said `words`, to `what`. The
/// role's refusal names its reason (invariant 8): `full` asks for the same
/// request later; `unknown session` and `not allowed` stop the session at
/// once. With no reason, the status tells a hop's trouble from a refusal.
pub(super) fn refused(answer: &Head, words: &str, what: &str) -> Then {
    let reason = answer.header(REFUSE).and_then(Refusal::parse);
    let said = match reason {
        Some(reason) => format!("HTTP {} ({}) {words}", answer.status, reason.word()),
        None => format!("HTTP {} {words}", answer.status),
    };
    match (reason, answer.status) {
        (Some(Refusal::UnknownSession), _) | (None, 404) => Then::Stop(forgotten()),
        (Some(Refusal::Full), _) | (None, 408 | 429 | 502 | 503 | 504) => Then::Again(said, retry_after(answer)),
        _ => Then::Stop(End::Failed(format!("the relay refused {what} of the HTTP road: {said}"))),
    }
}

/// The failure that a `404` of a session says.
pub(super) fn forgotten() -> End {
    End::Failed("the relay no longer knows the session of the HTTP road: it may have restarted".into())
}

/// The tries of a request after it failed: a pause that doubles from 100 ms
/// to 2 s, for [`RETRY_FOR`] since the last answer at most.
pub(super) struct Retry {
    since: Instant,
    pause: Duration,
}

impl Retry {
    pub fn start() -> Retry {
        Retry { since: Instant::now(), pause: PAUSE_FIRST }
    }

    /// An answer came.
    pub fn reset(&mut self) {
        *self = Retry::start();
    }

    /// The pause before the next try, `at_least` as a server asked; `Err`
    /// once the tries ran past their bound, with `why`.
    pub async fn again(&mut self, why: &str, at_least: Option<Duration>) -> Result<(), End> {
        if self.since.elapsed() >= RETRY_FOR {
            let secs = RETRY_FOR.as_secs();
            // A refusal `full` is an answer too, but it does not let the
            // session go on.
            return Err(End::Failed(format!("the HTTP road made no progress in {secs} s: {why}")));
        }
        tokio::time::sleep(at_least.map_or(self.pause, |asked| asked.max(self.pause))).await;
        self.pause = (self.pause * 2).min(PAUSE_MAX);
        Ok(())
    }
}

/// The seconds of `Retry-After`, 5 at most.
pub(super) fn retry_after(answer: &Head) -> Option<Duration> {
    answer.number("retry-after").map(|s| Duration::from_secs(s.min(5)))
}

/// The next batch of the caller's bytes into `buf`: what the first read
/// gives, and what more comes within a moment, up to `buf`'s size. `true`
/// with it once the caller's stream ended.
pub(super) async fn gather<R: AsyncRead + Unpin>(from: &mut R, buf: &mut [u8]) -> (usize, bool) {
    match from.read(buf).await {
        Ok(0) | Err(_) => (0, true),
        Ok(n) => gather_more(from, buf, n).await,
    }
}

/// [`gather`] after its first `n` bytes.
pub(super) async fn gather_more<R: AsyncRead + Unpin>(from: &mut R, buf: &mut [u8], mut n: usize) -> (usize, bool) {
    while n < buf.len() {
        match tokio::time::timeout(GATHER, from.read(&mut buf[n..])).await {
            Ok(Ok(0)) | Ok(Err(_)) => return (n, true),
            Ok(Ok(more)) => n += more,
            Err(_) => break,
        }
    }
    (n, false)
}

/// The path of the open of `target`, checked as the forward road's.
fn open_path(target: &Target<'_>) -> Result<String, ConnectError> {
    let forward = crate::relay::forward_path(target.host, target.port).map_err(ConnectError::Config)?;
    let mut path = forward.replacen("/connect/", OPEN, 1);
    if let Some(family) = target.family {
        path.push_str(&format!("?family={family}"));
    }
    Ok(path)
}

/// Open a session to `target` on `road` with `token`, in `form`, or, when
/// none is asked, the one-POST form, then the split form, then the serial
/// one. A refusal of the role keeps its status and words, as the WebSocket
/// road's does. The one-POST form that gets no mark in time is kept as held
/// for the host in `road.learned`.
pub async fn open(
    road: &Road,
    target: &Target<'_>,
    token: &str,
    form: Option<Form>,
    tuning: Tuning,
) -> Result<Opened, ConnectError> {
    if token.contains(['\r', '\n']) {
        return Err(ConnectError::Config("the relay token contains a line break".into()));
    }
    let path = open_path(target)?;
    let one_first = match form {
        Some(form) => form == Form::One,
        None => road.forms.is_none_or(|forms| forms.has(Form::One)) && road.learned.held(&road.authority).is_none(),
    };
    if one_first {
        match one::open(road, &path.replacen(OPEN, ONE, 1), token).await {
            Ok(opened) => return Ok(opened),
            Err(one::Failed::Stop(e)) => return Err(e),
            Err(one::Failed::NotCarried(why)) if form == Some(Form::One) => {
                return Err(ConnectError::Http(format!("the one-POST form of the HTTP road did not go: {why}")));
            }
            Err(one::Failed::NotCarried(why)) => road.learned.keep(&road.authority, &why),
        }
    }
    let mut first = Conn::new(road.dial.clone());
    let head = request("POST", &road.user_agent, &road.authority, &path, &[("X-Relay-Token", token)], Some(0));
    let answer = first.ask(&head, &[], LIMIT).await?;
    let body = first.whole(&answer, OPEN_ANSWER_MAX, LIMIT).await?;
    if answer.status != 200 {
        let words = pod_core::text::one_line(&String::from_utf8_lossy(&body));
        let words: String = words.chars().take(200).collect();
        let body = if words.is_empty() { answer.reason.clone() } else { words };
        return Err(ConnectError::Refused { status: answer.status, body });
    }
    let doc: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| ConnectError::Http(format!("the answer to the open of the HTTP road: {e}")))?;
    let id = doc["session"].as_str().filter(|id| is_id(id));
    let key = doc["key"].as_str().filter(|key| is_key(key));
    let (Some(id), Some(key)) = (id, key) else {
        return Err(ConnectError::Http("the answer to the open of the HTTP road names no session".into()));
    };
    let bound = |name: &str, most: u64| doc[name].as_u64().filter(|n| *n > 0).unwrap_or(most).min(most);
    let shared = Arc::new(Shared {
        id: id.to_string(),
        key: Zeroizing::new(key.to_string()),
        authority: road.authority.clone(),
        user_agent: road.user_agent.clone(),
        batch: tuning.batch.min(bound("batch_max", super::BATCH_MAX as u64) as usize).max(1),
        wait: tuning.wait.min(Duration::from_millis(bound("wait_max_ms", WAIT_MAX.as_millis() as u64))),
        down_max: bound("down_max", DOWN_MAX as u64) as usize,
        poll_max: tuning.poll_max,
        ended: Arc::default(),
        acked: AtomicU64::new(0),
        down_at: AtomicU64::new(0),
    });
    let (app, ours) = tokio::io::duplex(PIPE);
    let chosen = match form {
        Some(Form::Serial) => None,
        Some(Form::Split) => Some(Conn::new(road.dial.clone())),
        Some(Form::One) | None => second(road, &shared).await,
    };
    let form = match chosen {
        Some(second) => {
            let (from_app, to_app) = tokio::io::split(ours);
            tokio::spawn(super::split::up(first, shared.clone(), from_app));
            tokio::spawn(super::split::down(second, shared.clone(), to_app));
            Form::Split
        }
        None => {
            tokio::spawn(super::serial::run(first, shared.clone(), ours));
            Form::Serial
        }
    };
    Ok(Opened { stream: app, ended: shared.ended.clone(), form })
}

/// The split form's second connection, once it answered a first request
/// within [`LIMIT`]: a request of the bytes down with no wait, whose bytes
/// the next one asks again, as none is acknowledged by it.
async fn second(road: &Road, shared: &Shared) -> Option<Conn> {
    let mut conn = Conn::new(road.dial.clone());
    let head = shared.request("GET", &shared.path("down", "at=0&wait=0"), None);
    let answer = conn.ask(&head, &[], LIMIT).await.ok()?;
    let _ = conn.drop_body(&answer, LIMIT).await;
    matches!(answer.status, 200 | 204 | 410).then_some(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_s_path_is_the_forward_road_s_under_the_road_s_prefix() {
        let to = |host, port, family| open_path(&Target { host, port, family }).unwrap();
        assert_eq!(to("github.com", 22, None), "/v1/http/open/github.com/22");
        assert_eq!(to("2001:db8::1", 22, Some(6)), "/v1/http/open/2001:db8::1/22?family=6");
        assert!(open_path(&Target { host: "a/b", port: 22, family: None }).is_err());
        assert!(open_path(&Target { host: "h", port: 0, family: None }).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn the_tries_pause_longer_each_time_and_end_at_their_bound() {
        let mut tries = Retry::start();
        let started = Instant::now();
        tries.again("x", None).await.unwrap();
        tries.again("x", Some(Duration::from_secs(1))).await.unwrap();
        assert_eq!(started.elapsed(), Duration::from_millis(1100));
        tokio::time::sleep(RETRY_FOR).await;
        let failed = tries.again("the last error", None).await.unwrap_err();
        assert!(matches!(&failed, End::Failed(why) if why.contains("the last error")), "{failed:?}");
        tries.reset();
        assert!(tries.again("x", None).await.is_ok());
    }

    #[tokio::test]
    async fn a_batch_takes_what_comes_at_once_and_says_the_end() {
        use tokio::io::AsyncWriteExt;
        let (mut near, mut far) = tokio::io::duplex(1024);
        near.write_all(b"abc").await.unwrap();
        near.write_all(b"def").await.unwrap();
        let mut buf = [0u8; 4];
        assert_eq!(gather(&mut far, &mut buf).await, (4, false));
        drop(near);
        let mut rest = [0u8; 8];
        assert_eq!(gather(&mut far, &mut rest).await, (2, true));
        assert_eq!(&rest[..2], b"ef");
        assert_eq!(gather(&mut far, &mut rest).await, (0, true));
    }
}

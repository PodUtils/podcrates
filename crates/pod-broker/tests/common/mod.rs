//! The stand-ins of the tests of pod-broker: a far end in memory (an echo,
//! an HTTP server, a failure, a wait with no end), a resolver with fixed
//! answers that counts its calls, and a client of the broker over a pipe in
//! memory. The far end takes each address: the fixed denies hold loopback,
//! so the tests give public documentation addresses (RFC 5737) and the far
//! end answers for them, with no socket.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use pod_broker::{Audit, Broker, Line, Load, Policy, Settings, Token};
use pod_core::{BoxFuture, Class, Connected, Dial, Failure, Found, Resolve, Source, Stage, Target};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::time::Instant;

/// A made-up job token (`.github/secrets-allow.txt` needs no line: it is no
/// credential of a service).
pub const TOKEN: &str = "made-up-token-of-job-one";
pub const TOKEN_TWO: &str = "made-up-token-of-job-two";

/// A public documentation address (RFC 5737), which no fixed deny holds.
pub const PUBLIC: &str = "192.0.2.10";
pub const PUBLIC_TWO: &str = "198.51.100.20";

/// What the far end does.
#[derive(Clone)]
pub enum Kind {
    Echo,
    /// Read one request head (and `body` more bytes), keep it, answer.
    Http {
        answer: Vec<u8>,
        body: usize,
    },
    Fail(Class),
    Hang,
}

/// The far end of each dial.
pub struct Far {
    pub kind: Kind,
    pub dialed: Mutex<Vec<Target>>,
    pub seen: Arc<Mutex<Vec<u8>>>,
}

impl Far {
    pub fn new(kind: Kind) -> Arc<Far> {
        Arc::new(Far { kind, dialed: Mutex::new(Vec::new()), seen: Arc::new(Mutex::new(Vec::new())) })
    }

    pub fn dialed(&self) -> Vec<Target> {
        self.dialed.lock().unwrap().clone()
    }

    pub fn seen(&self) -> String {
        String::from_utf8_lossy(&self.seen.lock().unwrap()).into_owned()
    }
}

impl Dial for Far {
    fn label(&self) -> String {
        "far".into()
    }

    fn dial<'a>(&'a self, target: &'a Target, _deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        self.dialed.lock().unwrap().push(target.clone());
        let kind = self.kind.clone();
        let seen = self.seen.clone();
        Box::pin(async move {
            let (ours, theirs) = tokio::io::duplex(64 * 1024);
            match kind {
                Kind::Echo => {
                    tokio::spawn(async move {
                        let (mut r, mut w) = tokio::io::split(theirs);
                        let _ = tokio::io::copy(&mut r, &mut w).await;
                    });
                }
                Kind::Http { answer, body } => {
                    tokio::spawn(serve_http(theirs, answer, body, seen));
                }
                Kind::Fail(class) => return Err(Failure::new(Stage::Connect, class, "the far end refused")),
                Kind::Hang => std::future::pending::<()>().await,
            }
            Ok(Connected::new(Box::new(ours)))
        })
    }
}

async fn serve_http(mut io: DuplexStream, answer: Vec<u8>, body: usize, seen: Arc<Mutex<Vec<u8>>>) {
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    while !got.windows(4).any(|w| w == b"\r\n\r\n") {
        match io.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    let head_end = got.windows(4).position(|w| w == b"\r\n\r\n").map_or(got.len(), |p| p + 4);
    while got.len() < head_end + body {
        match io.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    seen.lock().unwrap().extend_from_slice(&got);
    let _ = io.write_all(&answer).await;
    let _ = io.shutdown().await;
}

/// A resolver that gives its answers in order, one for each call, and the
/// last one again.
pub struct Answers {
    answers: Mutex<VecDeque<Vec<SocketAddr>>>,
    pub calls: AtomicUsize,
}

impl Answers {
    pub fn new(answers: &[&[&str]]) -> Arc<Answers> {
        let answers =
            answers.iter().map(|a| a.iter().map(|ip| SocketAddr::new(ip.parse().unwrap(), 0)).collect()).collect();
        Arc::new(Answers { answers: Mutex::new(answers), calls: AtomicUsize::new(0) })
    }

    pub fn public() -> Arc<Answers> {
        Answers::new(&[&[PUBLIC]])
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl Resolve for Answers {
    fn label(&self) -> String {
        "answers".into()
    }

    fn resolve_from<'a>(
        &'a self,
        _host: &'a str,
        port: u16,
        _deadline: Instant,
        _after: Option<Source>,
    ) -> BoxFuture<'a, Result<Found, Failure>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut answers = self.answers.lock().unwrap();
        let addrs = if answers.len() > 1 { answers.pop_front().unwrap() } else { answers[0].clone() };
        let addrs = addrs.into_iter().map(|a| SocketAddr::new(a.ip(), port)).collect();
        Box::pin(async move { Ok(Found { addrs, source: Source { rank: 0, name: "answers" }, more: false }) })
    }
}

/// The audit lines in memory, and a signal for each new line.
#[derive(Default)]
pub struct Seen {
    lines: Mutex<Vec<Line>>,
    new: tokio::sync::Notify,
}

impl Seen {
    pub fn lines(&self) -> Vec<Line> {
        self.lines.lock().unwrap().clone()
    }

    /// The first line that `want` takes, within 10 s.
    pub async fn wait(&self, want: impl Fn(&Line) -> bool) -> Line {
        let wait = async {
            loop {
                let new = self.new.notified();
                tokio::pin!(new);
                new.as_mut().enable();
                if let Some(line) = self.lines().into_iter().find(|l| want(l)) {
                    return line;
                }
                new.await;
            }
        };
        tokio::time::timeout(pod_testkit::host::scaled(std::time::Duration::from_secs(10)), wait)
            .await
            .expect("an audit line in time")
    }

    /// The line of the connection that ended with `decision`.
    pub async fn connection(&self, decision: &str) -> Line {
        self.wait(|l| l.event == "broker.connection" && l.fields["decision"] == decision).await
    }
}

impl Audit for Seen {
    fn line(&self, line: &Line) {
        self.lines.lock().unwrap().push(line.clone());
        self.new.notify_waiters();
    }
}

/// A broker with `far`, `answers` and an audit in memory.
pub fn broker(far: Arc<Far>, answers: Arc<Answers>) -> (Arc<Broker>, Arc<Seen>) {
    broker_with(Settings::default(), far, answers)
}

pub fn broker_with(settings: Settings, far: Arc<Far>, answers: Arc<Answers>) -> (Arc<Broker>, Arc<Seen>) {
    let audit = Arc::new(Seen::default());
    let broker = Broker::new(settings, answers, far).with_audit(audit.clone());
    (Arc::new(broker), audit)
}

/// Load the job `job` with `token` and the policy in JSON, for 600 s.
pub fn load(broker: &Broker, job: &str, token: &str, policy: &str) {
    let policy = Policy::from_json(policy).unwrap();
    broker
        .load(Load { job: job.into(), token: Token::new(token), ttl: std::time::Duration::from_secs(600), policy })
        .unwrap();
}

pub const OPEN: &str = r#"{"version":1,"mode":"open"}"#;

/// `Proxy-Authorization` with the token as the password of Basic.
pub fn basic(user: &str, token: &str) -> String {
    format!(
        "Proxy-Authorization: Basic {}\r\n",
        base64::engine::general_purpose::STANDARD.encode(format!("{user}:{token}"))
    )
}

/// Open a connection to `broker`, send `request`, and read the head of the
/// answer: the head, and the stream for the rest.
pub async fn ask(broker: &Arc<Broker>, request: &str) -> (String, DuplexStream) {
    let (mut ours, theirs) = tokio::io::duplex(64 * 1024);
    let b = broker.clone();
    tokio::spawn(async move { b.handle(Box::new(theirs)).await });
    ours.write_all(request.as_bytes()).await.unwrap();
    let head = read_head(&mut ours).await;
    (head, ours)
}

/// The status of the answer to `request`.
pub async fn status(broker: &Arc<Broker>, request: &str) -> u16 {
    let (head, _) = ask(broker, request).await;
    status_of(&head)
}

pub fn status_of(head: &str) -> u16 {
    head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Read up to the end of a head, byte by byte, so that no byte after it is
/// taken.
pub async fn read_head<S: AsyncRead + Unpin>(io: &mut S) -> String {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match io.read(&mut byte).await {
            Ok(1) => head.push(byte[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&head).into_owned()
}

/// Read to the end of the stream.
pub async fn read_all<S: AsyncRead + Unpin>(io: &mut S) -> String {
    let mut out = Vec::new();
    let _ = io.read_to_end(&mut out).await;
    String::from_utf8_lossy(&out).into_owned()
}

/// Write `data`, and read as many bytes back.
pub async fn round_trip<S: AsyncRead + AsyncWrite + Unpin>(io: &mut S, data: &[u8]) -> Vec<u8> {
    io.write_all(data).await.unwrap();
    let mut back = vec![0u8; data.len()];
    io.read_exact(&mut back).await.unwrap();
    back
}

/// A CONNECT to `target` with the token of job one.
pub fn connect(target: &str) -> String {
    format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n{}\r\n", basic("job1", TOKEN))
}

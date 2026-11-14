//! A stand-in of the relay role's HTTP road, in memory: each dial of the
//! road is a pipe whose other end this role serves. Its batches up go
//! through `http_road::window::Up`, the role's window; its target takes each
//! batch at once into a digest, and sends the bytes of `Plan::down`. It
//! keeps a record of each request, so that a test can count the batches
//! sent again and the acknowledgements that it dropped. It serves the
//! one-POST form too (`one.rs`), and `hop.rs` puts a hop between the client
//! and it: one that passes bytes as they come, or one that holds a body in
//! chunks to its end.

// Each test file of the road takes a part of the stand-in.
#![allow(dead_code)]

pub mod hop;
mod one;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_relay::http_road::client::Road;
use pod_relay::http_road::conn::{BoxIo, Dial, Dialing};
use pod_relay::http_road::window::{Refusal, Stored, Up};
use pod_relay::http_road::{ACK, CLOSE, Kind, OFFSET, REFUSE, Route, SEQ, TOTAL, close_value, query, route};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

pub const ID: &str = "0123456789abcdef0123456789abcdef";
pub const KEY: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJ-_01234";
/// The largest answer down of the stand-in.
const DOWN_MAX: usize = 1 << 20;

/// `n` bytes of a pattern that `seed` picks.
pub fn pattern(n: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

/// How the stand-in behaves.
pub struct Plan {
    /// The target's bytes down.
    pub down: Vec<u8>,
    /// Drop each n-th answer that acknowledges a batch, after the batch is
    /// stored; 0 drops none.
    pub drop_every: u64,
    /// The batches that wait for the target.
    pub room: usize,
    /// Whether the target reads the batches up.
    pub target_reads: bool,
    /// Refuse each batch with this reason.
    pub refuse_up: Option<Refusal>,
    /// Added to the count of the bytes down at the target's end.
    pub total_skew: u64,
    /// How the stand-in answers the one-POST form.
    pub one: One,
}

/// How the stand-in answers the one-POST form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum One {
    /// As the role: the mark, then the bytes each way.
    Serve,
    /// `404`, as a role that serves no such form.
    Absent,
    /// As the role, but the connection ends with no last chunk after this
    /// many bytes down.
    CutAfter(usize),
}

impl Default for Plan {
    fn default() -> Self {
        Plan {
            down: Vec::new(),
            drop_every: 0,
            room: 8,
            target_reads: true,
            refuse_up: None,
            total_skew: 0,
            one: One::Serve,
        }
    }
}

/// What the stand-in saw.
#[derive(Default)]
pub struct Record {
    /// For each (session, `seq`): the requests that carried it, and the
    /// digest of each of their bodies.
    pub sent: HashMap<(String, u64), Vec<[u8; 32]>>,
    /// The answers that acknowledged a batch, and those dropped.
    pub acks: u64,
    pub dropped: u64,
    /// The batches that the target read.
    pub read: u64,
    /// The digest of the bytes that the target read, and their count.
    pub up: Sha256,
    pub up_bytes: u64,
    /// The `total` of the last batch.
    pub total: Option<u64>,
    /// The largest offset that a request down named.
    pub most_at: u64,
    /// The requests `up`: the batches that went with no bytes down.
    pub ups: u64,
    pub ended: Option<(u16, String)>,
    /// The requests of the one-POST form, the token of the last one, and
    /// whether a body of the form came to its last chunk.
    pub ones: u64,
    pub one_token: Option<String>,
    pub one_up_ended: bool,
}

/// The stand-in: its plan, the role's window and its record.
pub struct StandIn {
    plan: Plan,
    up: Mutex<Up>,
    record: Mutex<Record>,
}

impl StandIn {
    pub fn new(plan: Plan) -> Arc<StandIn> {
        let up = Mutex::new(Up::new(plan.room));
        Arc::new(StandIn { plan, up, record: Mutex::default() })
    }

    /// The road to this stand-in.
    pub fn road(self: &Arc<Self>) -> Road {
        let me = self.clone();
        let dial: Dial = Arc::new(move || -> Dialing {
            let me = me.clone();
            Box::pin(async move {
                let (ours, theirs) = tokio::io::duplex(64 * 1024);
                tokio::spawn(me.serve(theirs));
                Ok(Box::new(ours) as BoxIo)
            })
        });
        Road {
            dial,
            authority: "relay.test".into(),
            user_agent: "pod-relay/test".into(),
            forms: None,
            learned: Arc::default(),
        }
    }

    /// `f` with the record.
    pub fn record<T>(&self, f: impl FnOnce(&mut Record) -> T) -> T {
        f(&mut self.record.lock().unwrap())
    }

    /// The batches sent again: each request of a (session, `seq`) after its
    /// first.
    pub fn resent(&self) -> u64 {
        self.record(|r| r.sent.values().map(|bodies| bodies.len() as u64 - 1).sum())
    }

    /// The requests of one connection, until it ends or an answer is dropped.
    async fn serve(self: Arc<Self>, mut io: DuplexStream) {
        let mut buf = Vec::new();
        while let Some((target, headers, body)) = read_request(&mut io, &mut buf).await {
            if route(target.split('?').next().unwrap_or_default()) == Some(Route::One) {
                return one::serve(self, io, buf, &headers).await;
            }
            let Some(answer) = self.answer(&target, &headers, body).await else { return };
            if io.write_all(&answer).await.is_err() {
                return;
            }
        }
    }

    /// The answer to a request; `None` drops it with the connection.
    async fn answer(&self, target: &str, headers: &[(String, String)], body: Vec<u8>) -> Option<Vec<u8>> {
        let path = target.split('?').next().unwrap_or_default();
        let key = headers.iter().find(|(n, _)| n == "x-session-key").map(|(_, v)| v.as_str());
        match route(path) {
            Some(Route::Open) => {
                let doc = format!(r#"{{"session":"{ID}","key":"{KEY}","down_max":{DOWN_MAX}}}"#);
                Some(answer(200, &[], doc.as_bytes()))
            }
            Some(Route::Session(id, kind)) if id == ID && key == Some(KEY) => {
                if let Some(gone) = self.gone() {
                    return Some(gone);
                }
                let q = query(target);
                // A last batch with no count is a request that the role refuses.
                let total = match (q.fin, q.total) {
                    (false, _) => None,
                    (true, Some(total)) => Some(total),
                    (true, None) => return Some(answer(400, &[], b"http: a last batch takes total=BYTES")),
                };
                match kind {
                    Kind::Up => {
                        self.record(|r| r.ups += 1);
                        self.up(&id, q.seq?, body, total)
                    }
                    Kind::Down => Some(self.down(q.at?, q.wait_ms.unwrap_or(0)).await),
                    Kind::Exchange => {
                        if let Some(seq) = q.seq {
                            let stored = self.up(&id, seq, body, total)?;
                            if !stored.starts_with(b"HTTP/1.1 204") {
                                return Some(stored);
                            }
                            let mut reply = self.down(q.at?, 0).await;
                            let seq_line = format!("{SEQ}: {seq}\r\n");
                            let head_end = reply.windows(2).position(|w| w == b"\r\n")? + 2;
                            reply.splice(head_end..head_end, seq_line.into_bytes());
                            return Some(reply);
                        }
                        Some(self.down(q.at?, 0).await)
                    }
                    Kind::Close => {
                        self.end(1000, "client closed");
                        Some(answer(204, &[], b""))
                    }
                }
            }
            _ => Some(refusal(Refusal::UnknownSession, "http: no such session")),
        }
    }

    /// A batch up, through the role's window; each n-th acknowledgement is
    /// dropped after the batch is stored.
    fn up(&self, id: &str, seq: u64, body: Vec<u8>, total: Option<u64>) -> Option<Vec<u8>> {
        let digest: [u8; 32] = Sha256::digest(&body).into();
        self.record(|r| r.sent.entry((id.to_string(), seq)).or_default().push(digest));
        if let Some(reason) = self.plan.refuse_up {
            return Some(refusal(reason, "http: refused by the plan"));
        }
        let stored = self.up.lock().unwrap().store(seq, body, total);
        let acked = match stored {
            Stored::Taken(n) | Stored::Again(n) => n,
            Stored::Full => {
                return Some(refusal(Refusal::Full, "http: the target has not taken the batches before it"));
            }
            Stored::Conflict(next) => {
                return Some(answer(
                    409,
                    &[],
                    format!("http: the batch {seq} came, where the next is {next}").as_bytes(),
                ));
            }
            Stored::Mismatch { said, stored } => {
                self.end(1002, &format!("count mismatch: {said} said, {stored} stored"));
                return self.gone();
            }
        };
        if total.is_some() {
            self.record(|r| r.total = total);
        }
        self.target_reads();
        let drop = self.record(|r| {
            r.acks += 1;
            let drop = self.plan.drop_every > 0 && r.acks % self.plan.drop_every == 0;
            r.dropped += u64::from(drop);
            drop
        });
        if drop {
            return None;
        }
        let read = self.record(|r| r.read);
        Some(answer(204, &[(SEQ, acked.to_string()), (ACK, read.to_string())], b""))
    }

    /// The target reads each batch that waits, in number order.
    fn target_reads(&self) {
        if !self.plan.target_reads {
            return;
        }
        let mut up = self.up.lock().unwrap();
        while let Some(batch) = up.take() {
            self.record(|r| {
                r.read += 1;
                r.up.update(&batch);
                r.up_bytes += batch.len() as u64;
            });
        }
    }

    /// The bytes down from `at`; at their end, once the last batch up came,
    /// the target's end with its count.
    async fn down(&self, at: u64, wait_ms: u64) -> Vec<u8> {
        self.record(|r| r.most_at = r.most_at.max(at));
        let len = self.plan.down.len() as u64;
        if at > len {
            return answer(409, &[], b"http: an offset past the bytes sent");
        }
        if at < len {
            let end = (at as usize + DOWN_MAX).min(len as usize);
            return answer(200, &[(OFFSET, at.to_string())], &self.plan.down[at as usize..end]);
        }
        // Each byte down came: the end, once the target read the last batch.
        for _ in 0..=wait_ms.min(1000) / 10 {
            if self.up.lock().unwrap().done() {
                self.end(1000, "target closed");
                return self.gone().unwrap_or_default();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        answer(204, &[(OFFSET, at.to_string())], b"")
    }

    fn end(&self, code: u16, why: &str) {
        self.record(|r| {
            r.ended.get_or_insert_with(|| (code, why.to_string()));
        });
    }

    /// `410` once the session ended: its close, and at a clean end the
    /// count of the bytes down.
    fn gone(&self) -> Option<Vec<u8>> {
        let (code, why) = self.record(|r| r.ended.clone())?;
        let mut headers = vec![(CLOSE, close_value(code, &why))];
        if code == 1000 {
            headers.push((TOTAL, (self.plan.down.len() as u64 + self.plan.total_skew).to_string()));
        }
        Some(answer(410, &headers, b"http: the session ended"))
    }
}

/// A whole answer, kept.
fn answer(status: u16, headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} X\r\n");
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    if status != 204 {
        out.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    out.push_str("\r\n");
    let mut out = out.into_bytes();
    out.extend_from_slice(body);
    out
}

/// A refusal that names its reason.
fn refusal(reason: Refusal, words: &str) -> Vec<u8> {
    let mut headers = vec![(REFUSE, reason.word().to_string())];
    if reason == Refusal::Full {
        headers.push(("Retry-After", "1".to_string()));
    }
    answer(reason.status(), &headers, format!("{words}\n").as_bytes())
}

/// The next request: its target, its headers (names in lower case) and its
/// body; `None` at the end of the connection.
async fn read_request(io: &mut DuplexStream, buf: &mut Vec<u8>) -> Option<(String, Vec<(String, String)>, Vec<u8>)> {
    let mut chunk = vec![0u8; 64 * 1024];
    let end = loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
        let n = io.read(&mut chunk).await.ok().filter(|n| *n > 0)?;
        buf.extend_from_slice(&chunk[..n]);
    };
    let text = String::from_utf8(buf.drain(..end).collect()).ok()?;
    let mut lines = text.split("\r\n");
    let target = lines.next()?.split(' ').nth(1)?.to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let length: usize = headers.iter().find(|(n, _)| n == "content-length").map_or(Some(0), |(_, v)| v.parse().ok())?;
    while buf.len() < length {
        let n = io.read(&mut chunk).await.ok().filter(|n| *n > 0)?;
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = buf.drain(..length).collect();
    Some((target, headers, body))
}

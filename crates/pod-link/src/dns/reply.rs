//! The answers of the DNS listener, and the polls that it holds. A poll (a
//! query with no datagram up) of a session with no datagram down waits in
//! the session for up to [`DnsSettings::hold`](super::DnsSettings::hold):
//! the first datagram down answers it at once, as dnstt's
//! `maxResponseDelay` does. The reader of the socket does not wait: the held
//! query goes to the session, and the task of the listener answers it at a
//! wake of the session or at its time.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pod_core::Datagrams as AddressedDatagrams;
use pod_dns::txt;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// The most polls that one session holds. A further poll answers the oldest
/// at once, so a dialer that polls faster than the hold stacks no queries,
/// and a resolver that sends a held poll again (Unbound, after 376 ms) has
/// room for it. dnstt holds one query.
pub(super) const MAX_HELD: usize = 2;

/// A query that waits for its answer: its bytes, its sender, the size of its
/// answer, and the end of its hold.
pub(super) struct Reply {
    query: Vec<u8>,
    from: SocketAddr,
    size: u16,
    edns: Option<u16>,
    name: usize,
    until: Instant,
}

impl Reply {
    /// The query `query` of the name of `name` bytes from `from`, whose
    /// answer has at most `size` bytes and an OPT record of `edns` bytes
    /// when given, held until `until`.
    pub(super) fn new(
        query: &[u8],
        from: SocketAddr,
        size: u16,
        edns: Option<u16>,
        name: usize,
        until: Instant,
    ) -> Reply {
        Reply { query: query.to_vec(), from, size, edns, name, until }
    }
}

/// The half down of one session: the queue that the ARQ fills, the
/// datagrams that wait for a query, and the polls that wait for a datagram.
pub(super) struct Down {
    rx: mpsc::Receiver<Vec<u8>>,
    held: VecDeque<Vec<u8>>,
    polls: VecDeque<Reply>,
    /// Set when the session sent a datagram down since the last wake.
    woken: Arc<AtomicBool>,
}

/// What the half of the ARQ uses to wake the task of the listener when it
/// sends a datagram down: one wake for each session at a time.
#[derive(Clone)]
pub(super) struct Wake {
    session: u32,
    tx: mpsc::UnboundedSender<u32>,
    woken: Arc<AtomicBool>,
}

impl Wake {
    pub(super) fn ring(&self) {
        if !self.woken.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(self.session);
        }
    }
}

impl Down {
    /// The half down over `rx` of the session `session`, and its wake on
    /// `tx`.
    pub(super) fn new(rx: mpsc::Receiver<Vec<u8>>, session: u32, tx: mpsc::UnboundedSender<u32>) -> (Down, Wake) {
        let woken = Arc::new(AtomicBool::new(false));
        let wake = Wake { session, tx, woken: woken.clone() };
        (Down { rx, held: VecDeque::new(), polls: VecDeque::new(), woken }, wake)
    }

    /// A datagram down waits.
    pub(super) fn has_data(&mut self) -> bool {
        while let Ok(datagram) = self.rx.try_recv() {
            self.held.push_back(datagram);
        }
        !self.held.is_empty()
    }

    /// Answer the query `query` now with the datagrams down that fit.
    pub(super) async fn answer(
        &mut self,
        socket: &Arc<dyn AddressedDatagrams>,
        query: &[u8],
        from: SocketAddr,
        size: u16,
        edns: Option<u16>,
        name: usize,
    ) {
        let records = self.drain(name, size, edns.is_some());
        let refs: Vec<&[u8]> = records.iter().map(Vec::as_slice).collect();
        if let Ok(message) = txt::answer_to(query, &refs, edns) {
            send(socket, message, true, from).await;
        }
    }

    /// Hold the poll `reply`. Past [`MAX_HELD`], answer the oldest now.
    pub(super) async fn hold(&mut self, socket: &Arc<dyn AddressedDatagrams>, reply: Reply) {
        self.polls.push_back(reply);
        if self.polls.len() > MAX_HELD {
            self.pop(socket).await;
        }
    }

    /// Answer the oldest held poll.
    async fn pop(&mut self, socket: &Arc<dyn AddressedDatagrams>) {
        if let Some(r) = self.polls.pop_front() {
            self.answer(socket, &r.query, r.from, r.size, r.edns, r.name).await;
        }
    }

    /// At a wake: answer the held polls while datagrams down wait.
    pub(super) async fn release(&mut self, socket: &Arc<dyn AddressedDatagrams>) {
        self.woken.store(false, Ordering::Release);
        while !self.polls.is_empty() && self.has_data() {
            self.pop(socket).await;
        }
    }

    /// Answer each held poll whose hold ended at `now`.
    pub(super) async fn expire(&mut self, socket: &Arc<dyn AddressedDatagrams>, now: Instant) {
        while self.polls.front().is_some_and(|r| r.until <= now) {
            self.pop(socket).await;
        }
    }

    /// Answer each held poll: the session ends.
    pub(super) async fn flush(&mut self, socket: &Arc<dyn AddressedDatagrams>) {
        while !self.polls.is_empty() {
            self.pop(socket).await;
        }
    }

    /// The datagrams down that fit in an answer of `answer` bytes for a name
    /// of `name` bytes, with an OPT record when `opt`. Leftover datagrams
    /// wait in the session for the next query. The first datagram goes also
    /// over the size: a name of 253 bytes, the OPT record and the largest
    /// datagram of the carrier (147 bytes, `wire::payload_mtu`) make 442
    /// bytes, so it fits in 512.
    fn drain(&mut self, name: usize, answer: u16, opt: bool) -> Vec<Vec<u8>> {
        self.has_data();
        // A pointer to the name costs two bytes; the header, the question and
        // the OPT record are the fixed part.
        let fixed = 12 + name + 2 + 4 + if opt { 11 } else { 0 };
        let budget = usize::from(answer).saturating_sub(fixed);
        let mut used = 0;
        let mut records = Vec::new();
        while let Some(datagram) = self.held.front() {
            let cost = 12 + datagram.len() + datagram.len().div_ceil(255).max(1);
            if used + cost > budget && !records.is_empty() {
                break;
            }
            used += cost;
            records.push(self.held.pop_front().expect("a datagram at the front"));
        }
        records
    }
}

/// The ends of the holds of all sessions, in order: the hold is the same
/// for each poll, so each new end comes last.
#[derive(Default)]
pub(super) struct Holds {
    ends: VecDeque<(Instant, u32)>,
}

impl Holds {
    pub(super) fn push(&mut self, until: Instant, session: u32) {
        self.ends.push_back((until, session));
    }

    /// The next end of a hold.
    pub(super) fn next(&self) -> Option<Instant> {
        self.ends.front().map(|(until, _)| *until)
    }

    /// The sessions whose holds end at `now`. A session can come for a poll
    /// that a datagram down answered before: [`Down::expire`] then answers
    /// nothing.
    pub(super) fn due(&mut self, now: Instant) -> Vec<u32> {
        let mut due = Vec::new();
        while self.ends.front().is_some_and(|(until, _)| *until <= now) {
            let (_, session) = self.ends.pop_front().expect("an end at the front");
            due.push(session);
        }
        due
    }
}

/// Send the answer of `rcode` with no record to `query`, with the AA bit when
/// `authoritative` and an OPT record of `edns` bytes when given.
pub(super) async fn empty(
    socket: &Arc<dyn AddressedDatagrams>,
    query: &[u8],
    rcode: u8,
    authoritative: bool,
    edns: Option<u16>,
    from: SocketAddr,
) {
    if let Ok(message) = txt::empty_answer(query, rcode, edns) {
        send(socket, message, authoritative, from).await;
    }
}

/// Send the answer `message` to `from` with RA clear, as the listener does
/// no recursion, and with the AA bit when `authoritative`.
async fn send(socket: &Arc<dyn AddressedDatagrams>, mut message: Vec<u8>, authoritative: bool, from: SocketAddr) {
    txt::set_no_recursion(&mut message);
    if authoritative {
        txt::set_authoritative(&mut message);
    }
    let _ = pod_core::datagrams::send_to(&**socket, &message, from).await;
}

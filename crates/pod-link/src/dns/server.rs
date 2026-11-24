//! The listener's side of the DNS carrier: an authoritative server on a UDP
//! socket. One task reads the socket, reads the question of each query, and
//! takes the session, the counter and the datagram up from the labels of the
//! name under the zone. The answer of each query holds the datagrams down of
//! that session in TXT records.
//!
//! A new session id takes one of [`crate::dns::Dns::max_handshakes`] slots
//! only at a part of its first record, the ClientHello of TLS (`dns::hello`).
//! The session frees its slot at once when its first record is no
//! ClientHello that offers the ALPN of the link, and when no query of the
//! session comes for [`crate::dns::Dns::handshake_quiet`] before the end of
//! its handshake (checked each [`PRUNE`]). A query of a new session id with
//! no such part (a poll, junk) takes no slot and gets NOERROR with no record.
//!
//! Each other query gets an answer, so a resolver on the path does not send
//! it again and then keep a SERVFAIL:
//!
//! * A name of the zone with another type, or a name of the zone that holds
//!   no query (the zone itself, or a part of a name that a resolver asks
//!   first, RFC 9156): NOERROR with no record, AA set.
//! * A query of a new session after the drop of the listener: NOERROR with
//!   no record, AA set.
//! * A name out of the zone: REFUSED, AA clear.
//! * An opcode other than QUERY: NOTIMP, AA clear.
//!
//! Each answer copies the bytes of the question of its query, so a resolver
//! that sends the name in mixed case (0x20) takes it, and has RA clear: the
//! listener does no recursion.
//!
//! A query of a new session at the limit of the handshakes gets no answer,
//! so the dialer sends it again. An answer gets no answer.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::Datagrams as AddressedDatagrams;
use pod_dns::txt;
use pod_identity::PublicKey;
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::task::{AbortHandle, JoinSet};
use tokio::time::Instant;
use tokio_rustls::TlsAcceptor;

use super::hello::{self, Hello, Parts};
use super::settings::DnsSettings;
use super::wire;
use super::{Dns, NAME, err};
use crate::arq::wire::Datagram;
use crate::carrier::{Accepted, BoxIo, CarrierError, CarrierErrorKind, Listener, Pending};
use crate::datagram::{DatagramFuture, Datagrams};
use crate::dgram_tls;

/// The largest query that the reader takes.
const MAX_QUERY: usize = 1 << 16;

/// The results of handshakes that wait for an accept.
const QUEUE: usize = 8;

/// The interval of the removal of the sessions that ended.
const PRUNE: Duration = Duration::from_secs(1);

/// The pause after a failed read of the socket, so the task does not spin.
const READ_PAUSE: Duration = Duration::from_millis(50);

/// The result of one handshake.
type Shaken = Result<(BoxIo, PublicKey), CarrierError>;

/// A listener of the DNS carrier. Its drop ends each handshake under way and
/// starts no new session; the task answers the sessions that it gave until
/// each of them ends.
pub struct DnsListener {
    local: SocketAddr,
    results: mpsc::Receiver<Shaken>,
    _stop: oneshot::Sender<()>,
}

impl fmt::Debug for DnsListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DnsListener").field("local", &self.local).finish_non_exhaustive()
    }
}

/// What each session of a listener needs.
#[derive(Clone)]
struct Shake {
    acceptor: TlsAcceptor,
    alpn: &'static [u8],
    arq: crate::arq::ArqSettings,
    within: Duration,
    quiet: Duration,
    mtu: usize,
}

/// The listener's half of one session: the queue up that the ARQ reads, and
/// the queue down that the ARQ fills, which the task drains into answers.
struct Entry {
    up: mpsc::Sender<Vec<u8>>,
    down: mpsc::Receiver<Vec<u8>>,
    held: VecDeque<Vec<u8>>,
    from: SocketAddr,
    /// The time of the last query of the session.
    seen: Instant,
    /// The session before its handshake: the parts of its first record.
    hello: Option<Box<Early>>,
    /// The task of the handshake, once it runs.
    shake: Option<AbortHandle>,
}

/// A session whose first record comes in: its parts, its slot, and the half
/// of the datagrams that its handshake takes.
struct Early {
    parts: Parts,
    permit: OwnedSemaphorePermit,
    server: ServerSession,
}

impl Entry {
    /// Whether the session is silent for `quiet` before the end of its
    /// handshake. A handshake under way then ends, and gives its error to
    /// `out` when the queue has room.
    fn quiet(&self, now: Instant, quiet: Duration, out: &mpsc::Sender<Shaken>) -> bool {
        if now.duration_since(self.seen) < quiet {
            return false;
        }
        if self.hello.is_some() {
            return true;
        }
        let Some(shake) = self.shake.as_ref().filter(|s| !s.is_finished()) else { return false };
        shake.abort();
        let why = format!("the handshake of {} sent nothing for {quiet:?}", self.from);
        let _ = out.try_send(Err(err(CarrierErrorKind::Io(io::ErrorKind::TimedOut), why)));
        true
    }
}

pub(super) fn bind(dns: &Dns, socket: Arc<dyn AddressedDatagrams>, zone: String) -> Result<DnsListener, CarrierError> {
    if dns.max_handshakes == 0 {
        return Err(err(CarrierErrorKind::Usage, "max_handshakes is 0: give 1 or more"));
    }
    if dns.handshake_quiet.is_zero() {
        return Err(err(CarrierErrorKind::Usage, "handshake_quiet is 0: give 1 ms or more"));
    }
    let mtu = wire::payload_mtu(&zone).map_err(|e| err(CarrierErrorKind::Usage, e))?;
    let shake = Shake {
        acceptor: dgram_tls::acceptor(NAME, &dns.provider, &dns.identity, &dns.clients, dns.alpn)?,
        alpn: dns.alpn,
        arq: dns.arq,
        within: dns.handshake_timeout,
        quiet: dns.handshake_quiet,
        mtu,
    };
    let local = socket.local_addr().map_err(|e| CarrierError::io(NAME, "the address of the socket", e))?;
    let (out, results) = mpsc::channel(QUEUE);
    let (stop, stopped) = oneshot::channel();
    let limit = Arc::new(Semaphore::new(dns.max_handshakes));
    tokio::spawn(serve(socket, zone, shake, dns.dns, limit, out, stopped));
    Ok(DnsListener { local, results, _stop: stop })
}

impl DnsListener {
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// The next client whose key is pinned, and that key.
    pub async fn accept_key(&mut self) -> Result<(BoxIo, PublicKey), CarrierError> {
        match self.results.recv().await {
            Some(result) => result,
            None => Err(err(CarrierErrorKind::Io(io::ErrorKind::BrokenPipe), "the task of the listener ended")),
        }
    }
}

impl Listener for DnsListener {
    fn address(&self) -> String {
        self.local.to_string()
    }

    fn accept(&mut self) -> Pending<'_, BoxIo> {
        Box::pin(async move { self.accept_key().await.map(|(io, _)| io) })
    }

    fn accept_peer(&mut self) -> Pending<'_, Accepted> {
        Box::pin(async move { self.accept_key().await.map(|(io, key)| Accepted { io, peer: Some(key) }) })
    }
}

/// The per-session datagrams of the listener: the ARQ sends down through
/// `down`, and reads up from `up`. When the task is gone, the peer is
/// silence.
struct ServerSession {
    down: mpsc::Sender<Vec<u8>>,
    up: AsyncMutex<mpsc::Receiver<Vec<u8>>>,
    mtu: usize,
}

impl Datagrams for ServerSession {
    fn mtu(&self) -> usize {
        self.mtu
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        Box::pin(async move {
            let _ = self.down.try_send(datagram.to_vec());
            Ok(())
        })
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        Box::pin(async move {
            match self.up.lock().await.recv().await {
                Some(datagram) => Ok(datagram),
                None => std::future::pending().await,
            }
        })
    }
}

impl std::fmt::Debug for ServerSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerSession").field("mtu", &self.mtu).finish_non_exhaustive()
    }
}

/// Read the socket, answer each query, and start a session at a new session
/// id.
async fn serve(
    socket: Arc<dyn AddressedDatagrams>,
    zone: String,
    shake: Shake,
    settings: DnsSettings,
    limit: Arc<Semaphore>,
    out: mpsc::Sender<Shaken>,
    mut stop: oneshot::Receiver<()>,
) {
    let mut sessions: HashMap<u32, Entry> = HashMap::new();
    let mut shakes = JoinSet::new();
    let mut stopped = false;
    let mut buf = vec![0u8; MAX_QUERY];
    let mut prune = tokio::time::interval(PRUNE);
    prune.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            got = pod_core::datagrams::recv_from(&*socket, &mut buf) => {
                let (n, from) = match got {
                    Ok(got) => got,
                    Err(e) => {
                        tracing::debug!(error = %e, "a read of the socket of a dns listener failed");
                        tokio::time::sleep(READ_PAUSE).await;
                        continue;
                    }
                };
                answer(&socket, &zone, &shake, &settings, &limit, &out, &mut sessions, &mut shakes, &buf[..n], from, stopped).await;
            }
            _ = &mut stop, if !stopped => {
                stopped = true;
                shakes.abort_all();
                // A session before its handshake starts none now.
                sessions.retain(|_, e| e.hello.is_none());
            }
            _ = prune.tick() => {
                let now = Instant::now();
                sessions.retain(|_, e| !e.up.is_closed() && !e.quiet(now, shake.quiet, &out));
                while shakes.try_join_next().is_some() {}
                if stopped && sessions.is_empty() {
                    return;
                }
            }
        }
    }
}

/// Answer one query: take its datagram up and start or feed its session,
/// then send the datagrams down of that session. The header of the module
/// gives the answer of each other query.
#[allow(clippy::too_many_arguments)]
async fn answer(
    socket: &Arc<dyn AddressedDatagrams>,
    zone: &str,
    shake: &Shake,
    settings: &DnsSettings,
    limit: &Arc<Semaphore>,
    out: &mpsc::Sender<Shaken>,
    sessions: &mut HashMap<u32, Entry>,
    shakes: &mut JoinSet<()>,
    query: &[u8],
    from: SocketAddr,
    stopped: bool,
) {
    // The name in lower case serves to decode; each answer copies the bytes
    // of the question, so a resolver that uses 0x20 takes it.
    let Ok((_, name, kind)) = txt::question(query) else { return };
    // The QR bit: a server that answers an answer can make a loop.
    if query[2] & 0x80 != 0 {
        return;
    }
    let asked = txt::asked_size(query);
    let edns = asked.map(|_| settings.answer);
    // The opcode: only QUERY reads a name.
    if query[2] & 0x78 != 0 {
        return empty(socket, query, txt::RCODE_NOTIMP, false, edns, from).await;
    }
    // REFUSED, as an authoritative server answers for a zone that it does not
    // hold: NXDOMAIN with AA would say that the name does not exist.
    if wire::under(zone, &name).is_none() {
        return empty(socket, query, txt::RCODE_REFUSED, false, edns, from).await;
    }
    // NOERROR with no record, not NXDOMAIN: NXDOMAIN says that no name exists
    // under this one (RFC 8020), so a resolver that asks for a part of the
    // name first (RFC 9156) stops. With no SOA record, a resolver keeps no
    // negative answer in its cache (RFC 2308, section 5).
    let decoded = if kind == txt::TXT { wire::decode_query(zone, &name).ok() } else { None };
    let Some((session, _counter, payload)) = decoded else {
        return empty(socket, query, txt::RCODE_NOERROR, true, edns, from).await;
    };
    use std::collections::hash_map::Entry as Slot;
    let entry = match sessions.entry(session) {
        Slot::Occupied(slot) => slot.into_mut(),
        Slot::Vacant(slot) => {
            // The listener starts no new session. An empty answer, not a
            // refusal: a resolver keeps no failure and marks the server as
            // bad for no time, and the dialer ends at its time limit.
            if stopped {
                return empty(socket, query, txt::RCODE_NOERROR, true, edns, from).await;
            }
            // Only a part of a ClientHello takes a slot.
            let Some(part) = hello::part(&payload, shake.arq.start) else {
                return empty(socket, query, txt::RCODE_NOERROR, true, edns, from).await;
            };
            // At the limit the query is lost: the dialer sends it again.
            let Ok(permit) = limit.clone().try_acquire_owned() else { return };
            let (up_tx, up_rx) = mpsc::channel(settings.depth);
            let (down_tx, down_rx) = mpsc::channel(settings.depth);
            let server = ServerSession { down: down_tx, up: AsyncMutex::new(up_rx), mtu: shake.mtu };
            let early = Early { parts: Parts::new(part.count), permit, server };
            let (up, down, held) = (up_tx, down_rx, VecDeque::new());
            let seen = Instant::now();
            slot.insert(Entry { up, down, held, from, seen, hello: Some(Box::new(early)), shake: None })
        }
    };
    entry.seen = Instant::now();
    if entry.hello.is_some() {
        if !first_record(entry, payload, shake, out, shakes) {
            sessions.remove(&session);
            return empty(socket, query, txt::RCODE_NOERROR, true, edns, from).await;
        }
    } else if !payload.is_empty() {
        // A full queue loses the datagram, as a lossy carrier does.
        let _ = entry.up.try_send(payload);
    }
    // The answer keeps to the size that the query takes, 512 bytes with no
    // OPT record, so a resolver on the path does not cut it (call C291).
    let size = asked.unwrap_or(txt::PLAIN_SIZE).min(settings.answer);
    let records = drain(entry, &name, size, asked.is_some());
    let refs: Vec<&[u8]> = records.iter().map(Vec::as_slice).collect();
    if let Ok(message) = txt::answer_to(query, &refs, edns) {
        send(socket, message, true, from).await;
    }
    if entry.up.is_closed() {
        sessions.remove(&session);
    }
}

/// Take the datagram `payload` of a session whose first record comes in. When
/// the record is whole and offers the ALPN of the link, or goes on past its
/// last part, give its parts to the session and start the handshake. False
/// when the session ends: a part 0 that opens no ClientHello, a part of
/// another count, or a record with no offer of the ALPN.
fn first_record(
    entry: &mut Entry,
    payload: Vec<u8>,
    shake: &Shake,
    out: &mpsc::Sender<Shaken>,
    shakes: &mut JoinSet<()>,
) -> bool {
    let Some(early) = entry.hello.as_mut() else { return true };
    // A poll or another datagram before the handshake carries nothing.
    if payload.is_empty() || Datagram::decode(&payload).is_ok_and(|d| !matches!(d, Datagram::Data { .. })) {
        return true;
    }
    let Some(part) = hello::part(&payload, shake.arq.start) else { return false };
    if !early.parts.add(part, payload) {
        return false;
    }
    if !early.parts.complete() {
        return true;
    }
    let Early { parts, permit, server } = *entry.hello.take().expect("a session before its handshake");
    let (datagrams, read) = parts.take(shake.alpn);
    if read == Hello::Refused {
        return false;
    }
    for datagram in datagrams {
        let _ = entry.up.try_send(datagram);
    }
    let (shake, out, from) = (shake.clone(), out.clone(), entry.from);
    entry.shake = Some(shakes.spawn(async move {
        let result = dgram_tls::accept(NAME, server, shake.acceptor, shake.alpn, shake.arq, &from, shake.within).await;
        // A failed handshake frees its slot before the caller takes the error.
        let _permit = result.is_ok().then_some(permit);
        let _ = out.send(result).await;
    }));
    true
}

/// Send the answer of `rcode` with no record to `query`, with the AA bit when
/// `authoritative` and an OPT record of `edns` bytes when given.
async fn empty(
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

/// The datagrams down that fit in an answer of `answer` bytes for `name`,
/// with an OPT record when `opt`. Leftover datagrams wait in the session
/// for the next query. The first datagram goes also over the size: a name
/// of 253 bytes, the OPT record and the largest datagram of the carrier
/// (147 bytes, [`wire::payload_mtu`]) make 442 bytes, so it fits in 512.
fn drain(entry: &mut Entry, name: &str, answer: u16, opt: bool) -> Vec<Vec<u8>> {
    while let Ok(datagram) = entry.down.try_recv() {
        entry.held.push_back(datagram);
    }
    // A pointer to the name costs two bytes; the header, the question and the
    // OPT record are the fixed part.
    let fixed = 12 + name.len() + 2 + 4 + if opt { 11 } else { 0 };
    let budget = usize::from(answer).saturating_sub(fixed);
    let mut used = 0;
    let mut records = Vec::new();
    while let Some(datagram) = entry.held.front() {
        let cost = 12 + datagram.len() + datagram.len().div_ceil(255).max(1);
        if used + cost > budget && !records.is_empty() {
            break;
        }
        used += cost;
        records.push(entry.held.pop_front().expect("a datagram at the front"));
    }
    records
}

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
//!
//! The listener keeps the id of a session that ended (its handshake failed,
//! or its link ended) for [`crate::dns::Dns::handshake_timeout`]: a late
//! query of that id (a retry of a resolver) gets NOERROR with no record and
//! starts no handshake. A session that ended at its quiet time leaves no id,
//! so its dialer can start it again.
//!
//! A poll of a session after its first record waits for data down for up
//! to [`DnsSettings::hold`] (`dns::reply`).
//!
//! A bare listener ([`crate::dns::Dns::bare`]) takes a bare hello as the
//! first record, not a ClientHello (`dns::bare`), and gives each client with
//! no key.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::Datagrams as AddressedDatagrams;
use pod_dns::txt;
use pod_identity::PublicKey;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::task::{AbortHandle, JoinSet};
use tokio::time::Instant;
use tokio_rustls::TlsAcceptor;

use super::bare;
use super::hello::{self, Hello, Parts};
use super::reply::{Down, Holds, Reply, ServerSession, empty};
use super::settings::DnsSettings;
use super::wire;
use super::{Dns, NAME, err};
use crate::arq::wire::Datagram;
use crate::carrier::{Accepted, BoxIo, CarrierError, CarrierErrorKind, Listener, Pending};
use crate::dgram_tls;

/// The largest query that the reader takes.
const MAX_QUERY: usize = 1 << 16;

/// The results of handshakes that wait for an accept.
const QUEUE: usize = 8;

/// The interval of the removal of the sessions that ended.
const PRUNE: Duration = Duration::from_secs(1);

/// The pause after a failed read of the socket, so the task does not spin.
const READ_PAUSE: Duration = Duration::from_millis(50);

/// The most ids of ended sessions that the listener keeps; past it, a late
/// query of an ended session can start a session again.
const MAX_ENDED: usize = 4096;

/// The result of one handshake, with the key of the client; no key at a
/// bare listener.
type Shaken = Result<(BoxIo, Option<PublicKey>), CarrierError>;

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

/// What each session of a listener needs. No acceptor: the bare form.
#[derive(Clone)]
struct Shake {
    acceptor: Option<TlsAcceptor>,
    alpn: &'static [u8],
    arq: crate::arq::ArqSettings,
    within: Duration,
    quiet: Duration,
    mtu: usize,
}

/// The listener's half of one session: the queue up that the ARQ reads, and
/// the half down that the ARQ fills, which the task drains into answers.
struct Entry {
    up: mpsc::Sender<Vec<u8>>,
    down: Down,
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
        acceptor: match dns.bare {
            true => None,
            false => Some(dgram_tls::acceptor(NAME, &dns.provider, &dns.identity, &dns.clients, dns.alpn)?),
        },
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

    /// The next client whose key is pinned, and that key. A bare listener
    /// proves no key: it gives a usage error, and [`Listener::accept_peer`]
    /// gives the client with no key.
    pub async fn accept_key(&mut self) -> Result<(BoxIo, PublicKey), CarrierError> {
        match self.next().await? {
            (io, Some(key)) => Ok((io, key)),
            (_, None) => Err(err(CarrierErrorKind::Usage, "a bare listener proves no key: use accept or accept_peer")),
        }
    }

    async fn next(&mut self) -> Shaken {
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
        Box::pin(async move { self.next().await.map(|(io, _)| io) })
    }

    fn accept_peer(&mut self) -> Pending<'_, Accepted> {
        Box::pin(async move { self.next().await.map(|(io, peer)| Accepted { io, peer }) })
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
    let mut ended: HashMap<u32, Instant> = HashMap::new();
    let mut holds = Holds::default();
    let (wake, mut woken) = mpsc::unbounded_channel();
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
                let table = Table { sessions: &mut sessions, ended: &mut ended, shakes: &mut shakes, holds: &mut holds, wake: &wake, stopped };
                answer(&socket, &zone, &shake, &settings, &limit, &out, table, &buf[..n], from).await;
            }
            Some(id) = woken.recv() => {
                if let Some(entry) = sessions.get_mut(&id) {
                    entry.down.release(&socket).await;
                }
            }
            () = tokio::time::sleep_until(holds.next().unwrap_or_else(Instant::now)), if holds.next().is_some() => {
                let now = Instant::now();
                for id in holds.due(now) {
                    if let Some(entry) = sessions.get_mut(&id) {
                        entry.down.expire(&socket, now).await;
                    }
                }
            }
            _ = &mut stop, if !stopped => {
                stopped = true;
                shakes.abort_all();
                // A session before its handshake starts none now.
                sessions.retain(|_, e| e.hello.is_none());
            }
            _ = prune.tick() => {
                let now = Instant::now();
                ended.retain(|_, at| now.duration_since(*at) < shake.within);
                let gone: Vec<Entry> = sessions
                    .extract_if(|id, e| {
                        let closed = e.up.is_closed();
                        if closed {
                            keep_ended(&mut ended, *id, now);
                        }
                        closed || e.quiet(now, shake.quiet, &out)
                    })
                    .map(|(_, e)| e)
                    .collect();
                for mut entry in gone {
                    entry.down.flush(&socket).await;
                }
                while shakes.try_join_next().is_some() {}
                if stopped && sessions.is_empty() {
                    return;
                }
            }
        }
    }
}

/// The sessions of the listener, the ids of the sessions that ended, the
/// tasks of the handshakes, the ends of the held polls, and the wake of the
/// sessions.
struct Table<'a> {
    sessions: &'a mut HashMap<u32, Entry>,
    ended: &'a mut HashMap<u32, Instant>,
    shakes: &'a mut JoinSet<()>,
    holds: &'a mut Holds,
    wake: &'a mpsc::UnboundedSender<u32>,
    stopped: bool,
}

/// Keep the id `id` of a session that ended at `now`, while the map has room.
fn keep_ended(ended: &mut HashMap<u32, Instant>, id: u32, now: Instant) {
    if ended.len() < MAX_ENDED {
        ended.insert(id, now);
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
    table: Table<'_>,
    query: &[u8],
    from: SocketAddr,
) {
    let Table { sessions, ended, shakes, holds, wake, stopped } = table;
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
            // A late query of an ended session starts no handshake either.
            if stopped || ended.contains_key(&session) {
                return empty(socket, query, txt::RCODE_NOERROR, true, edns, from).await;
            }
            // Only a part of a ClientHello takes a slot.
            let Some(part) = hello::part(&payload, shake.arq.start, shake.acceptor.is_none()) else {
                return empty(socket, query, txt::RCODE_NOERROR, true, edns, from).await;
            };
            // At the limit the query is lost: the dialer sends it again.
            let Ok(permit) = limit.clone().try_acquire_owned() else { return };
            let (up, up_rx) = mpsc::channel(settings.depth);
            let (down_tx, down_rx) = mpsc::channel(settings.depth);
            let (down, wake) = Down::new(down_rx, session, wake.clone());
            let server = ServerSession::new(down_tx, wake, up_rx, shake.mtu);
            let early = Early { parts: Parts::new(part.count), permit, server };
            let seen = Instant::now();
            slot.insert(Entry { up, down, from, seen, hello: Some(Box::new(early)), shake: None })
        }
    };
    entry.seen = Instant::now();
    let poll = payload.is_empty();
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
    // A poll of a session past its first record waits for data down.
    if poll && entry.hello.is_none() && !settings.hold.is_zero() && !entry.down.has_data() && !entry.up.is_closed() {
        let until = Instant::now() + settings.hold;
        entry.down.hold(socket, Reply::new(query, from, size, edns, name.len(), until)).await;
        holds.push(until, session);
        return;
    }
    entry.down.answer(socket, query, from, size, edns, name.len()).await;
    if entry.up.is_closed() {
        let mut gone = sessions.remove(&session).expect("the session of the query");
        gone.down.flush(socket).await;
        keep_ended(ended, session, Instant::now());
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
    let Some(part) = hello::part(&payload, shake.arq.start, shake.acceptor.is_none()) else { return false };
    if !early.parts.add(part, payload) {
        return false;
    }
    if !early.parts.complete() {
        return true;
    }
    let Early { parts, permit, server } = *entry.hello.take().expect("a session before its handshake");
    let (datagrams, read) = parts.take(shake.alpn, shake.acceptor.is_none());
    if read == Hello::Refused {
        return false;
    }
    for datagram in datagrams {
        let _ = entry.up.try_send(datagram);
    }
    let (shake, out, from) = (shake.clone(), out.clone(), entry.from);
    entry.shake = Some(shakes.spawn(async move {
        let (alpn, arq, within) = (shake.alpn, shake.arq, shake.within);
        let result = match shake.acceptor {
            Some(tls) => {
                dgram_tls::accept(NAME, server, tls, alpn, arq, &from, within).await.map(|(io, k)| (io, Some(k)))
            }
            None => bare::accept(server, alpn, arq, &from, within).await.map(|io| (io, None)),
        };
        // A failed handshake frees its slot before the caller takes the error.
        let _permit = result.is_ok().then_some(permit);
        let _ = out.send(result).await;
    }));
    true
}

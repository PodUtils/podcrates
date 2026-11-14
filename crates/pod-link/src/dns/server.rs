//! The listener's side of the DNS carrier: an authoritative server on a UDP
//! socket. One task reads the socket, reads the question of each query, and
//! takes the session, the counter and the datagram up from the labels of the
//! name under the zone. A new session id starts a session and its handshake,
//! at most [`crate::dns::Dns::max_handshakes`] at once. The answer of each
//! query holds the datagrams down of that session in TXT records. A wrong
//! name, or a query of a session that the server does not hold, gets an
//! empty answer.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::Datagrams as AddressedDatagrams;
use pod_dns::txt;
use pod_identity::PublicKey;
use tokio::sync::{Mutex as AsyncMutex, Semaphore, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;

use super::settings::DnsSettings;
use super::wire;
use super::{Dns, NAME, err};
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
    mtu: usize,
}

/// The listener's half of one session: the queue up that the ARQ reads, and
/// the queue down that the ARQ fills, which the task drains into answers.
struct Entry {
    up: mpsc::Sender<Vec<u8>>,
    down: mpsc::Receiver<Vec<u8>>,
    held: VecDeque<Vec<u8>>,
}

pub(super) fn bind(dns: &Dns, socket: Arc<dyn AddressedDatagrams>, zone: String) -> Result<DnsListener, CarrierError> {
    if dns.max_handshakes == 0 {
        return Err(err(CarrierErrorKind::Usage, "max_handshakes is 0: give 1 or more"));
    }
    let mtu = wire::payload_mtu(&zone).map_err(|e| err(CarrierErrorKind::Usage, e))?;
    let shake = Shake {
        acceptor: dgram_tls::acceptor(NAME, &dns.provider, &dns.identity, &dns.clients, dns.alpn)?,
        alpn: dns.alpn,
        arq: dns.arq,
        within: dns.handshake_timeout,
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
            }
            _ = prune.tick() => {
                sessions.retain(|_, e| !e.up.is_closed());
                while shakes.try_join_next().is_some() {}
                if stopped && sessions.is_empty() {
                    return;
                }
            }
        }
    }
}

/// Answer one query: take its datagram up and start or feed its session,
/// then send the datagrams down of that session.
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
    let Ok((id, name, kind)) = txt::question(query) else { return };
    if kind != txt::TXT {
        return;
    }
    let Ok((session, _counter, payload)) = wire::decode_query(zone, &name) else { return };
    use std::collections::hash_map::Entry as Slot;
    let entry = match sessions.entry(session) {
        Slot::Occupied(slot) => slot.into_mut(),
        Slot::Vacant(slot) => {
            if stopped {
                return;
            }
            // At the limit the query is lost: the dialer sends it again.
            let Ok(permit) = limit.clone().try_acquire_owned() else { return };
            let (up_tx, up_rx) = mpsc::channel(settings.depth);
            let (down_tx, down_rx) = mpsc::channel(settings.depth);
            let server = ServerSession { down: down_tx, up: AsyncMutex::new(up_rx), mtu: shake.mtu };
            let (shake, out) = (shake.clone(), out.clone());
            shakes.spawn(async move {
                let _permit = permit;
                let result =
                    dgram_tls::accept(NAME, server, shake.acceptor, shake.alpn, shake.arq, &from, shake.within).await;
                let _ = out.send(result).await;
            });
            slot.insert(Entry { up: up_tx, down: down_rx, held: VecDeque::new() })
        }
    };
    if !payload.is_empty() {
        // A full queue loses the datagram, as a lossy carrier does.
        let _ = entry.up.try_send(payload);
    }
    // The answer keeps to the size that the query takes, 512 bytes with no
    // OPT record, so a resolver on the path does not cut it (call C291).
    let asked = txt::asked_size(query);
    let size = asked.unwrap_or(txt::PLAIN_SIZE).min(settings.answer);
    let records = drain(entry, &name, size, asked.is_some());
    let refs: Vec<&[u8]> = records.iter().map(Vec::as_slice).collect();
    if let Ok(message) = txt::answer(id, &name, txt::TXT, &refs, asked.map(|_| settings.answer)) {
        let _ = pod_core::datagrams::send_to(&**socket, &message, from).await;
    }
    if entry.up.is_closed() {
        sessions.remove(&session);
    }
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

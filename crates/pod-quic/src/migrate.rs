//! The rotation of the connection IDs of a live QUIC connection and its
//! migration to a new local address (RFC 9000, sections 5.1 and 9).
//!
//! * [`Schedule`]: the data of a rotation schedule, the least interval, the
//!   most, and the jitter. A fixed interval is a signature, so each draw
//!   differs from the one before it.
//! * [`RotatingIds`]: noq's [`noq::ConnectionIdGenerator`] whose
//!   `cid_lifetime` is the next draw of the schedule. noq retires each
//!   batch of local connection IDs at that time and issues the next batch
//!   (`NEW_CONNECTION_ID` with a `Retire Prior To`), so the rotation runs
//!   without a timer of this crate.
//! * [`rebind`]: noq's `Endpoint::rebind` for a socket of the system or for
//!   any [`Datagrams`]. A live connection sends from the new address, and
//!   the peer validates the new path.
//! * [`Migrator`]: the driver. It rebinds, probes the session, and, when
//!   the path does not carry a packet again, opens a new connection and
//!   resumes the session where it stopped ([`Outcome::Reconnected`]). A
//!   server that refuses migration is not an error.
//!
//! noq 1.4 has the migration path: `Endpoint::rebind` and
//! `Endpoint::handle_network_change`. This crate takes the rebind, because
//! the caller gives a new local address. The connection IDs are noq's, and
//! its `ConnectionIdGenerator::cid_lifetime` is the rotation timer.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use noq::{Connection, ConnectionId, ConnectionIdGenerator, Endpoint, EndpointConfig, InvalidCid};
use pod_core::entropy::{Entropy as _, OsEntropy};
use pod_core::{BoxFuture, Class, Datagrams, Failure, Stage};

use crate::endpoint;
use crate::keys::ResetKey;
use crate::socket::DatagramSocket;

/// The bytes of a connection ID of [`RotatingIds`].
pub const CID_LEN: usize = 8;

fn config_error(text: impl Into<String>) -> Failure {
    Failure::new(Stage::Quic, Class::Config, text)
}

fn quic_error(text: impl Into<String>, e: impl std::error::Error + Send + Sync + 'static) -> Failure {
    Failure::caused(Stage::Quic, Class::Unavailable, text, e)
}

/// The data of a rotation schedule.
///
/// The interval of one rotation is a base drawn uniformly from
/// `[min, max]`, then moved by a part of `jitter`: with a jitter of 250, the
/// interval is between 75% and 125% of the base. `jitter` is in parts of
/// 1000, so 0 is no jitter and 1000 is a factor of two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    pub min: Duration,
    pub max: Duration,
    /// The jitter, in parts of 1000 of the base interval.
    pub jitter: u16,
}

impl Schedule {
    /// The schedule of `min`, `max` and `jitter`, checked by [`Schedule::check`].
    pub fn new(min: Duration, max: Duration, jitter: u16) -> Result<Schedule, Failure> {
        let schedule = Schedule { min, max, jitter };
        schedule.check()?;
        Ok(schedule)
    }

    /// The schedule that podnet keeps a live connection with: 8 to 13
    /// minutes, and a jitter of 20%. Two rotations fit in a transfer of
    /// half an hour, and no observer sees a period.
    pub fn common() -> Schedule {
        Schedule { min: Duration::from_secs(8 * 60), max: Duration::from_secs(13 * 60), jitter: 200 }
    }

    /// `min` is not zero, `max` is not less than `min`, `jitter` is at most
    /// 1000, and the interval is not fixed: a schedule of one interval and
    /// no jitter is a signature, and this crate does not take one.
    pub fn check(&self) -> Result<(), Failure> {
        if self.min.is_zero() {
            return Err(config_error("the least interval of a rotation is zero"));
        }
        if self.max < self.min {
            return Err(config_error(format!(
                "the most interval of a rotation ({} ms) is less than the least ({} ms)",
                self.max.as_millis(),
                self.min.as_millis()
            )));
        }
        if self.jitter > 1000 {
            return Err(config_error(format!("a jitter of {} is more than 1000", self.jitter)));
        }
        if self.max == self.min && self.jitter == 0 {
            let text =
                format!("a fixed interval of {} ms is a signature; give a range or a jitter", self.min.as_millis());
            return Err(config_error(text));
        }
        Ok(())
    }

    /// The next interval, drawn with the random source of this host.
    pub fn interval(&self) -> Result<Duration, Failure> {
        let mut rand = [0u8; 8];
        OsEntropy
            .fill(&mut rand)
            .map_err(|_| Failure::new(Stage::Quic, Class::Unavailable, "this host's random source gave no interval"))?;
        Ok(self.interval_of(u64::from_le_bytes(rand)))
    }

    /// [`Schedule::interval`] of the number `rand`: the same number gives
    /// the same interval, which a test reads.
    pub fn interval_of(&self, rand: u64) -> Duration {
        let a = splitmix64(rand);
        let b = splitmix64(a);
        let span = self.max.saturating_sub(self.min);
        let base = self.min + span.mul_f64(f64::from((a >> 32) as u32) / f64::from(u32::MAX));
        // The part of the base that the jitter moves: from -jitter to
        // +jitter, uniformly.
        let part = f64::from(self.jitter) / 1000.0;
        let unit = f64::from((b >> 32) as u32) / f64::from(u32::MAX);
        base.mul_f64(1.0 + part * (2.0 * unit - 1.0))
    }
}

/// A mixing of 64 bits (splitmix64): each call gives another value, and a
/// count of calls is not a period.
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The generator of the local connection IDs of an endpoint: each batch is
/// retired after the next interval of the schedule, so no two batches live
/// for the same time.
pub struct RotatingIds {
    schedule: Schedule,
    /// The state of the draws: seeded from the random source, advanced by
    /// each call of `cid_lifetime`.
    state: Mutex<u64>,
}

impl RotatingIds {
    /// A generator of the schedule, seeded from the random source of this
    /// host. A host with no random source still rotates, from the seed of
    /// the clock.
    pub fn new(schedule: Schedule) -> Result<RotatingIds, Failure> {
        schedule.check()?;
        let mut seed = [0u8; 8];
        let state = match OsEntropy.fill(&mut seed) {
            Ok(()) => u64::from_le_bytes(seed),
            Err(_) => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x5deece66d),
        };
        Ok(RotatingIds { schedule, state: Mutex::new(state | 1) })
    }

    /// The next interval of the schedule, and the advance of the state.
    fn next(&self) -> Duration {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        *state = splitmix64(*state);
        self.schedule.interval_of(*state)
    }
}

impl ConnectionIdGenerator for RotatingIds {
    fn generate_cid(&mut self) -> ConnectionId {
        let mut id = [0u8; CID_LEN];
        if OsEntropy.fill(&mut id).is_err() {
            // No random source: the state still gives each ID a value of
            // its own. This is a fallback of a broken host, not the rule.
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            *state = splitmix64(*state);
            id.copy_from_slice(&splitmix64(*state).to_le_bytes());
        }
        ConnectionId::new(&id)
    }

    fn validate(&self, _cid: ConnectionId) -> Result<(), InvalidCid> {
        Ok(())
    }

    fn cid_len(&self) -> usize {
        CID_LEN
    }

    /// The life of the batch that noq issues now: the next draw.
    fn cid_lifetime(&self) -> Option<Duration> {
        Some(self.next())
    }
}

/// The configuration of an endpoint whose local connection IDs rotate on
/// `schedule`, with a reset key of its own.
pub fn endpoint_config(schedule: Schedule) -> Result<EndpointConfig, Failure> {
    let ids = RotatingIds::new(schedule)?;
    let mut config = EndpointConfig::new(Arc::new(ResetKey::random()?));
    config.cid_generator(Arc::new(move || Box::new(ids.for_connection()) as Box<dyn ConnectionIdGenerator>));
    Ok(config)
}

impl RotatingIds {
    /// The generator of one connection: the same schedule, seeded again.
    fn for_connection(&self) -> RotatingIds {
        let state = *self.state.lock().unwrap_or_else(|e| e.into_inner());
        RotatingIds { schedule: self.schedule, state: Mutex::new(splitmix64(state)) }
    }
}

/// A new local address for a rotation: a socket of the system bound by this
/// call, or the datagrams of the caller.
pub enum Local {
    /// A UDP socket of the system, bound at this address (`[::]:0` or
    /// `0.0.0.0:0` for a port that the system picks).
    Socket(SocketAddr),
    /// The datagrams of a way of the caller (a userspace stack, a tunnel).
    Datagrams(Arc<dyn Datagrams>),
}

impl std::fmt::Debug for Local {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Local::Socket(addr) => f.debug_tuple("Local::Socket").field(addr).finish(),
            Local::Datagrams(d) => f.debug_tuple("Local::Datagrams").field(&d.local_addr()).finish(),
        }
    }
}

/// Where each rotation takes its local address from. A test gives the
/// addresses of its own way; a caller of the system gives
/// [`SystemLocals`].
pub trait Locals: Send + Sync {
    /// The local address of the next rotation.
    fn next(&self) -> BoxFuture<'_, Result<Local, Failure>>;
}

/// The local addresses of the system: a socket of the family of `any`,
/// with a port that the system picks.
#[derive(Clone, Copy, Debug)]
pub struct SystemLocals {
    pub any: SocketAddr,
}

impl SystemLocals {
    /// The addresses of the family of `any`.
    pub fn of(any: SocketAddr) -> SystemLocals {
        SystemLocals { any }
    }
}

impl Locals for SystemLocals {
    fn next(&self) -> BoxFuture<'_, Result<Local, Failure>> {
        Box::pin(async move { Ok(Local::Socket(self.any)) })
    }
}

/// Move the live connections of `endpoint` to `local`: noq's
/// `Endpoint::rebind`. Each connection sends from the new address, and its
/// peer validates the new path (RFC 9000, section 9.3). A socket that does
/// not bind leaves the old one in place.
pub fn rebind(endpoint: &Endpoint, local: Local) -> Result<(), Failure> {
    match local {
        Local::Socket(addr) => {
            let socket = endpoint::dual_stack(addr).map_err(|e| {
                Failure::caused(Stage::Connect, Class::of_io(e.kind()), format!("no UDP socket at {addr}: {e}"), e)
            })?;
            endpoint.rebind(socket).map_err(|e| quic_error(format!("the endpoint takes no socket at {addr}: {e}"), e))
        }
        Local::Datagrams(datagrams) => endpoint
            .rebind_abstract(Box::new(DatagramSocket::new(datagrams)))
            .map_err(|e| quic_error(format!("the endpoint takes no datagrams: {e}"), e)),
    }
}

/// A session that a rotation carries: the connection of now, a probe of the
/// path, and the open of a new connection at the offset of now. The caller
/// owns the bytes; this crate owns the transport.
pub trait Carried {
    /// The connection of now.
    fn connection(&self) -> Connection;

    /// One round trip on the session after a rotation: `Ok` when the path
    /// carries a packet again. The caller bounds it with its own time
    /// limit.
    fn probe(&mut self) -> BoxFuture<'_, Result<(), Failure>>;

    /// Open a new connection and continue the session at the offset of now,
    /// after the peer did not accept the new path. The caller replaces its
    /// connection with the one that this call gives.
    fn reopen(&mut self) -> BoxFuture<'_, Result<Connection, Failure>>;
}

/// Which path a rotation took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The live connection answered on its new local address.
    Migrated,
    /// The peer did not answer on the new path, or closed the connection:
    /// a new connection carries the session, from the offset of now.
    Reconnected,
}

/// The driver of the rotations: the schedule, and the time that a probe of
/// the new path may take.
pub struct Migrator {
    schedule: Schedule,
    /// The time that a probe of a new path may take before the driver
    /// opens a new connection.
    probe_wait: Duration,
}

impl Migrator {
    /// A driver of `schedule`, with [`PROBE_WAIT`].
    pub fn new(schedule: Schedule) -> Result<Migrator, Failure> {
        schedule.check()?;
        Ok(Migrator { schedule, probe_wait: PROBE_WAIT })
    }

    /// The time that a probe of a new path may take.
    pub fn with_probe_wait(mut self, wait: Duration) -> Migrator {
        self.probe_wait = wait;
        self
    }

    pub fn schedule(&self) -> Schedule {
        self.schedule
    }

    /// The interval until the next rotation.
    pub fn interval(&self) -> Result<Duration, Failure> {
        self.schedule.interval()
    }

    /// Rotate `endpoint` to the next local address of `locals`, and probe
    /// the session. When the probe fails, or the connection is closed, the
    /// caller's [`Carried::reopen`] gives a new connection and the session
    /// continues at its offset. A refusal of the new path is therefore not
    /// an error; a failure of the new connection is.
    pub async fn rotate(
        &self,
        endpoint: &Endpoint,
        locals: &dyn Locals,
        carried: &mut dyn Carried,
    ) -> Result<Outcome, Failure> {
        let local = locals.next().await?;
        let addr = match &local {
            Local::Socket(addr) => Some(*addr),
            Local::Datagrams(d) => d.local_addr().ok(),
        };
        rebind(endpoint, local)?;
        tracing::debug!(at = ?addr, "the QUIC connection rotates to a new local address");
        let probe = carried.probe();
        let probed = tokio::time::timeout(self.probe_wait, probe).await;
        let closed = carried.connection().close_reason();
        if closed.is_none() && matches!(probed, Ok(Ok(()))) {
            return Ok(Outcome::Migrated);
        }
        let why = match (&probed, &closed) {
            (_, Some(reason)) => format!("the peer closed the connection: {reason}"),
            (Ok(Err(e)), None) => e.chain(),
            (Err(_), None) => format!("no packet crossed the new path within {} ms", self.probe_wait.as_millis()),
            (Ok(Ok(())), None) => "the session did not answer after the rotation".to_string(),
        };
        tracing::info!(%why, "the peer did not accept the new path; a new connection carries the session");
        carried.connection().close(0u32.into(), b"migration refused");
        carried.reopen().await.map_err(|e| {
            let text =
                format!("the peer refused the migrated path ({why}), and the new connection failed: {}", e.chain());
            Failure::caused(e.stage, e.class, text, e)
        })?;
        Ok(Outcome::Reconnected)
    }
}

/// The time that a probe of a new path may take: beyond a round trip of a
/// slow path, and below the silence of a session.
pub const PROBE_WAIT: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule() -> Schedule {
        Schedule { min: Duration::from_secs(600), max: Duration::from_secs(900), jitter: 200 }
    }

    /// A fixed interval is a signature: the draws differ, each stays within
    /// the bounds of the schedule, and no two neighbours are equal.
    #[test]
    fn the_interval_jitters_and_is_never_constant() {
        let schedule = schedule();
        let draws: Vec<Duration> = (0..512u64).map(|i| schedule.interval_of(i.wrapping_mul(0x9e37_79b9))).collect();
        let least = Duration::from_millis(600 * 800);
        let most = Duration::from_millis(900 * 1200);
        for d in &draws {
            assert!(*d >= least && *d <= most, "{d:?} is outside [{least:?}, {most:?}]");
        }
        for pair in draws.windows(2) {
            assert_ne!(pair[0], pair[1], "two rotations of the same interval");
        }
        let mut distinct = draws.clone();
        distinct.sort();
        distinct.dedup();
        assert!(distinct.len() > 500, "{} distinct intervals of 512", distinct.len());
        // A jitter of 0 keeps the base, which still varies from min to max.
        let flat = Schedule { jitter: 0, ..schedule };
        let a = flat.interval_of(1);
        let b = flat.interval_of(2);
        assert_ne!(a, b, "the base interval is fixed too");
        assert!(a >= flat.min && a <= flat.max && b >= flat.min && b <= flat.max, "{a:?} {b:?}");
    }

    /// The same draw gives the same interval: a test reads the schedule.
    #[test]
    fn a_draw_is_reproducible() {
        let schedule = schedule();
        assert_eq!(schedule.interval_of(7), schedule.interval_of(7));
        assert_ne!(schedule.interval_of(7), schedule.interval_of(8));
    }

    #[test]
    fn a_schedule_is_checked() {
        assert!(Schedule::new(Duration::ZERO, Duration::from_secs(1), 0).is_err());
        assert!(Schedule::new(Duration::from_secs(2), Duration::from_secs(1), 0).is_err());
        assert!(Schedule::new(Duration::from_secs(1), Duration::from_secs(2), 1001).is_err());
        // A fixed interval is refused, with or without a jitter of 0.
        assert!(Schedule::new(Duration::from_secs(1), Duration::from_secs(1), 0).is_err());
        assert!(Schedule::new(Duration::from_secs(1), Duration::from_secs(1), 10).is_ok());
        Schedule::common().check().expect("the common schedule");
    }

    /// Each call of `cid_lifetime` draws again, so no two batches of local
    /// connection IDs live for the same time; each ID is 8 random bytes.
    #[test]
    fn the_generator_draws_a_life_for_each_batch() {
        let mut ids = RotatingIds::new(schedule()).unwrap();
        let lives: Vec<Duration> = (0..8).filter_map(|_| ids.cid_lifetime()).collect();
        assert_eq!(lives.len(), 8);
        let mut distinct = lives.clone();
        distinct.sort();
        distinct.dedup();
        assert!(distinct.len() >= 7, "{lives:?}");
        assert_eq!(ids.cid_len(), CID_LEN);
        let one = ids.generate_cid();
        assert_eq!(one.len(), CID_LEN);
        let two = ids.generate_cid();
        assert_ne!(&one[..], &two[..], "two batches of the same local connection ID");
    }
}

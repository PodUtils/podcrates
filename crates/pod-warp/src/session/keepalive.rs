//! The liveness check of a session (PodUtils/podnet issue 104). Over
//! HTTP/2: a PING when no data came in for an interval, and a dead session
//! after [`MAX_MISSED_PONGS`] intervals with no pong and no data. Over
//! HTTP/3, the keepalive and the idle timeout of QUIC do the same: the check
//! ends when the connection closes. Over WireGuard, the timers of the
//! tunnel: the keepalive, the rekey and a dead path.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use pod_quic::noq;

use crate::wg::WgKeepalive;

/// The intervals with no pong (and no data) that make a session dead.
pub const MAX_MISSED_PONGS: u32 = 3;

/// The liveness check of a session.
pub struct Keepalive {
    inner: KeepInner,
    progress: Arc<AtomicU64>,
}

enum KeepInner {
    H2(h2::PingPong),
    H3(noq::Connection),
    Wg(WgKeepalive),
}

impl Keepalive {
    /// The check of an HTTP/2 session, with the counter of its frames.
    pub fn h2(ping: h2::PingPong, progress: Arc<AtomicU64>) -> Keepalive {
        Keepalive { inner: KeepInner::H2(ping), progress }
    }

    /// The check of an HTTP/3 session: the close of its connection.
    pub fn h3(conn: noq::Connection, progress: Arc<AtomicU64>) -> Keepalive {
        Keepalive { inner: KeepInner::H3(conn), progress }
    }

    /// The check of a WireGuard session: the timers of its tunnel.
    pub fn wg(timers: WgKeepalive, progress: Arc<AtomicU64>) -> Keepalive {
        Keepalive { inner: KeepInner::Wg(timers), progress }
    }

    /// The counter of the frames that came in.
    pub fn progress(&self) -> Arc<AtomicU64> {
        self.progress.clone()
    }

    /// Ready with a failure when the session is dead. Over HTTP/2, one PING
    /// waits across the ticks until its pong comes: h2 takes one PING at a
    /// time. `every` is the interval of the HTTP/2 check.
    pub async fn run(self, every: Duration) -> Result<(), Failure> {
        let Keepalive { inner, progress } = self;
        let mut ping = match inner {
            KeepInner::H2(ping) => ping,
            KeepInner::H3(conn) => return crate::h3::closed(conn).await,
            KeepInner::Wg(timers) => return timers.run().await,
        };
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tick.tick().await;
        let mut seen = progress.load(Ordering::Relaxed);
        loop {
            tick.tick().await;
            let now = progress.load(Ordering::Relaxed);
            if now != seen {
                seen = now;
                continue;
            }
            let pong = ping.ping(h2::Ping::opaque());
            tokio::pin!(pong);
            let mut missed = 0;
            loop {
                tokio::select! {
                    result = &mut pong => {
                        result.map_err(|e| {
                            Failure::caused(Stage::Warp, Class::Unavailable, "the HTTP/2 PING of the MASQUE session", e)
                        })?;
                        break;
                    }
                    _ = tick.tick() => {
                        if progress.load(Ordering::Relaxed) != seen {
                            missed = 0;
                        } else {
                            missed += 1;
                        }
                        if missed >= MAX_MISSED_PONGS {
                            let secs = every.as_secs_f64() * f64::from(missed + 1);
                            let text = format!("the MASQUE session gave no pong and no data in {secs:.1} s");
                            return Err(Failure::new(Stage::Warp, Class::Timeout, text));
                        }
                    }
                }
            }
            seen = progress.load(Ordering::Relaxed);
        }
    }
}

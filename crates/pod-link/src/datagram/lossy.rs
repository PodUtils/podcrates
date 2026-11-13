//! A shim over a datagram carrier that loses, reorders and duplicates its
//! datagrams, from a seed: the same seed and the same sends give the same
//! faults. It is for tests, of this crate and of a consumer.

use std::sync::{Mutex, PoisonError};

use super::{DatagramFuture, Datagrams};

/// The faults of the shim, each a chance from 0 to 1 for each datagram
/// sent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Faults {
    /// The datagram is lost.
    pub loss: f64,
    /// The datagram goes after the next datagram.
    pub reorder: f64,
    /// The datagram goes two times.
    pub duplicate: f64,
}

impl Faults {
    /// No fault.
    pub const NONE: Faults = Faults { loss: 0.0, reorder: 0.0, duplicate: 0.0 };

    /// Each datagram is lost: a peer or a path that is silent.
    pub const SILENT: Faults = Faults { loss: 1.0, reorder: 0.0, duplicate: 0.0 };
}

/// What the shim did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LossyStats {
    /// The datagrams given to the carrier, with the duplicates.
    pub passed: u64,
    pub lost: u64,
    pub reordered: u64,
    pub duplicated: u64,
}

struct State {
    faults: Faults,
    rng: SplitMix64,
    held: Option<Vec<u8>>,
    stats: LossyStats,
}

/// A datagram carrier whose sends have [`Faults`].
pub struct Lossy<D> {
    inner: D,
    state: Mutex<State>,
}

impl<D: Datagrams> Lossy<D> {
    /// The shim over `inner` with `faults` from `seed`.
    pub fn new(inner: D, faults: Faults, seed: u64) -> Lossy<D> {
        let state = State { faults, rng: SplitMix64(seed), held: None, stats: LossyStats::default() };
        Lossy { inner, state: Mutex::new(state) }
    }

    /// Change the faults from now on.
    pub fn set_faults(&self, faults: Faults) {
        self.lock().faults = faults;
    }

    pub fn stats(&self) -> LossyStats {
        self.lock().stats
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The datagrams that go now for one send, in their order.
    fn plan(&self, datagram: &[u8]) -> Vec<Vec<u8>> {
        let mut s = self.lock();
        let faults = s.faults;
        if s.rng.chance(faults.loss) {
            s.stats.lost += 1;
            return Vec::new();
        }
        let mut out = vec![datagram.to_vec()];
        if s.rng.chance(faults.duplicate) {
            s.stats.duplicated += 1;
            out.push(datagram.to_vec());
        }
        if s.rng.chance(faults.reorder) {
            s.stats.reordered += 1;
            // This one waits for the next send; one that waited goes now.
            let late = s.held.replace(out.pop().expect("one datagram"));
            out.extend(late);
        } else if let Some(late) = s.held.take() {
            out.push(late);
        }
        s.stats.passed += out.len() as u64;
        out
    }
}

impl<D: Datagrams> Datagrams for Lossy<D> {
    fn mtu(&self) -> usize {
        self.inner.mtu()
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        Box::pin(async move {
            for d in self.plan(datagram) {
                self.inner.send(&d).await?;
            }
            Ok(())
        })
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        self.inner.recv()
    }
}

impl<D> std::fmt::Debug for Lossy<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("Lossy").field("faults", &s.faults).field("stats", &s.stats).finish_non_exhaustive()
    }
}

/// SplitMix64: a small generator with a seed, enough for faults of a test.
#[derive(Debug, Clone)]
pub struct SplitMix64(pub u64);

impl SplitMix64 {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// True with the chance `p` (0 to 1).
    pub fn chance(&mut self, p: f64) -> bool {
        if p <= 0.0 {
            return false;
        }
        // 53 bits give a uniform value in [0, 1).
        ((self.next_u64() >> 11) as f64 / (1u64 << 53) as f64) < p
    }
}

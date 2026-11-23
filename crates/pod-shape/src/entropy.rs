//! Entropy control: a payload inside the band of the profile, one printable
//! run per 64 KiB, and a nonce salted with a coarse timestamp and accepted
//! only inside its four-minute window.

use std::collections::VecDeque;

use crate::rng::Rng;
use crate::{Band, Error};

/// The seconds of one step of the coarse timestamp of a nonce.
pub const NONCE_STEP_SECS: u64 = 4;
/// The acceptance window of a nonce, in seconds.
pub const NONCE_WINDOW_SECS: u64 = 240;
/// The bytes of one measured entropy window.
const WINDOW: usize = 4096;
/// The bytes of one block that carries a printable run.
const BLOCK: usize = 65536;
/// The bytes of the alphabet that carries no printable value.
const ALPHABET: usize = 161;

/// A nonce: the coarse time of its making and a random tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nonce {
    /// The time of the nonce, in steps of [`NONCE_STEP_SECS`] seconds.
    pub salt: u64,
    pub tag: u32,
}

/// The entropy source of one session.
pub struct Entropy {
    band: Band,
    low: usize,
    high: usize,
    rng: Rng,
    seen: VecDeque<Nonce>,
}

impl Entropy {
    /// An entropy source for `band` with `seed`.
    pub fn new(band: Band, seed: u64) -> Result<Entropy, Error> {
        let (low, high) = symbol_range(band)?;
        Ok(Entropy { band, low, high, rng: Rng::new(seed), seen: VecDeque::new() })
    }

    /// The band of the source.
    pub fn band(&self) -> Band {
        self.band
    }

    fn symbol_count(&mut self) -> usize {
        self.low + self.rng.below((self.high - self.low + 1) as u64) as usize
    }

    fn symbols(&mut self, count: usize) -> Vec<u8> {
        let mut pool = alphabet();
        for at in 0..count {
            let rest = (ALPHABET - at) as u64;
            let pick = at + self.rng.below(rest) as usize;
            pool.swap(at, pick);
        }
        pool[..count].to_vec()
    }

    /// A payload of `len` bytes inside the band, with one printable run per
    /// 64 KiB.
    pub fn fill(&mut self, len: usize) -> Vec<u8> {
        let mut out = vec![0u8; len];
        let mut start = 0;
        while start < len {
            let end = (start + WINDOW).min(len);
            let count = self.symbol_count();
            let symbols = self.symbols(count);
            for byte in &mut out[start..end] {
                *byte = symbols[self.rng.below(count as u64) as usize];
            }
            start = end;
        }
        self.insert_runs(&mut out);
        out
    }

    fn insert_runs(&mut self, bytes: &mut [u8]) {
        let run = self.band.printable_run;
        if run == 0 {
            return;
        }
        for start in (0..bytes.len()).step_by(BLOCK) {
            let end = (start + BLOCK).min(bytes.len());
            let size = end - start;
            if size < run {
                continue;
            }
            let offset = self.rng.below((size - run + 1) as u64) as usize;
            for byte in &mut bytes[start + offset..start + offset + run] {
                *byte = 0x20 + self.rng.below(95) as u8;
            }
        }
    }

    /// Check that each measured window is inside the band and that each
    /// 64 KiB block carries the printable runs of the profile.
    pub fn check(&self, bytes: &[u8]) -> Result<(), Error> {
        for (at, window) in bytes.chunks(WINDOW).enumerate() {
            if window.len() < 512 {
                continue;
            }
            let bits = bits_per_byte(window);
            if bits < self.band.min_bits - 0.02 || bits > self.band.max_bits + 0.02 {
                return Err(Error::EntropyBand { at: at * WINDOW, bits });
            }
        }
        let run = self.band.printable_run;
        for (block, bytes) in bytes.chunks(BLOCK).enumerate() {
            let count = printable_runs(bytes, run);
            let full = bytes.len() == BLOCK;
            if (full && count != 1) || (!full && count > 1) {
                return Err(Error::PrintableRuns { block: block * BLOCK, count, want: usize::from(full) });
            }
        }
        Ok(())
    }

    /// A nonce for `now_unix_secs`, salted with its coarse time.
    pub fn nonce(&mut self, now_unix_secs: u64) -> Nonce {
        Nonce { salt: now_unix_secs / NONCE_STEP_SECS, tag: self.rng.next_u64() as u32 }
    }

    /// Accept `nonce` at `now_unix_secs`. A nonce outside the window, from
    /// the future or accepted before is refused.
    pub fn accept(&mut self, nonce: Nonce, now_unix_secs: u64) -> Result<(), Error> {
        let now = now_unix_secs / NONCE_STEP_SECS;
        if nonce.salt > now {
            return Err(Error::NonceFuture);
        }
        let age = now.saturating_sub(nonce.salt).saturating_mul(NONCE_STEP_SECS);
        if age > NONCE_WINDOW_SECS {
            return Err(Error::NonceExpired { age_secs: age });
        }
        while let Some(old) = self.seen.front() {
            if now.saturating_sub(old.salt).saturating_mul(NONCE_STEP_SECS) > NONCE_WINDOW_SECS {
                self.seen.pop_front();
            } else {
                break;
            }
        }
        if self.seen.contains(&nonce) {
            return Err(Error::Replay);
        }
        self.seen.push_back(nonce);
        Ok(())
    }
}

/// The count of the maximal printable runs of at least `minimum` bytes.
pub fn printable_runs(bytes: &[u8], minimum: usize) -> usize {
    let mut runs = 0;
    let mut len = 0;
    for &byte in bytes {
        if byte.is_ascii_graphic() || byte == b' ' {
            len += 1;
        } else {
            if len >= minimum && minimum > 0 {
                runs += 1;
            }
            len = 0;
        }
    }
    if len >= minimum && minimum > 0 {
        runs += 1;
    }
    runs
}

/// The Shannon entropy of `bytes`, in bits per byte.
pub fn bits_per_byte(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }
    let mut counts = [0u64; 256];
    for &byte in bytes {
        counts[byte as usize] += 1;
    }
    let total = bytes.len() as f64;
    let mut bits = 0.0;
    for count in counts {
        if count > 0 {
            let p = count as f64 / total;
            bits -= p * p.log2();
        }
    }
    bits
}

fn alphabet() -> [u8; ALPHABET] {
    let mut pool = [0u8; ALPHABET];
    let mut at = 0;
    for byte in 0u8..=0x1f {
        pool[at] = byte;
        at += 1;
    }
    for byte in 0x7fu8..=0xff {
        pool[at] = byte;
        at += 1;
    }
    pool
}

fn symbol_range(band: Band) -> Result<(usize, usize), Error> {
    let finite = band.min_bits.is_finite() && band.max_bits.is_finite();
    if !finite
        || band.min_bits <= 0.0
        || band.max_bits > 8.0
        || band.min_bits > band.max_bits
        || band.printable_run == 0
    {
        return Err(Error::BandTooNarrow { min: band.min_bits, max: band.max_bits });
    }
    if band.printable_run > BLOCK {
        return Err(Error::PrintableRunTooLong { run: band.printable_run, max: BLOCK });
    }
    let low = (2f64.powf(band.min_bits + 0.25).ceil() as usize).clamp(2, ALPHABET);
    let high = (2f64.powf(band.max_bits - 0.25).floor() as usize).clamp(1, ALPHABET);
    if low > high {
        return Err(Error::BandTooNarrow { min: band.min_bits, max: band.max_bits });
    }
    Ok((low, high))
}

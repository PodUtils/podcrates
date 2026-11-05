//! The waits between tries of a connection, the same for each crate: 1 s,
//! 2 s, 4 s and on to 30 s, each scaled by a random factor in [0.5, 1.5), so
//! that many clients that try again at once do not stay in step.

use std::time::Duration;

use rand::RngExt;

/// The time limit of one connection attempt.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// The first wait between rounds, before the random factor.
pub const BACKOFF_FIRST: Duration = Duration::from_secs(1);

/// The longest wait between rounds, before the random factor.
pub const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// The wait before try `retry` (1 is the first retry).
pub fn backoff(retry: u32) -> Duration {
    let first = BACKOFF_FIRST.as_millis() as u64;
    let max = BACKOFF_MAX.as_millis() as u64;
    let base = first.saturating_mul(1u64 << retry.saturating_sub(1).min(5)).min(max);
    let factor: f64 = rand::rng().random_range(0.5..1.5);
    Duration::from_millis((base as f64 * factor) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_wait_is_in_its_band_and_the_last_is_capped() {
        for (retry, base) in [(1u32, 1_000u64), (2, 2_000), (3, 4_000), (5, 16_000), (6, 30_000), (40, 30_000)] {
            for _ in 0..50 {
                let ms = backoff(retry).as_millis() as u64;
                assert!(ms >= base / 2 && ms < base * 3 / 2, "retry {retry}: {ms} ms");
            }
        }
    }
}

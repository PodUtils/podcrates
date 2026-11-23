//! The window of early-data nonces: each nonce is kept for 10 minutes and a
//! repeat is refused. Without the window a 0-RTT session is a replay
//! primitive, because an attacker who copies the first flight can send it
//! again and the server cannot tell the two apart.
//!
//! The window is a value of the caller: one per process, or one per session.
//! A refusal is not a failure of the session: the same session goes on at
//! 1-RTT, so the nonce's refusal costs a round trip and no more.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a nonce is kept: the 10 minutes of REPLAY-1.
pub const WINDOW: Duration = Duration::from_secs(600);

/// The most nonces one window keeps. A window past this drops the oldest:
/// a nonce that is no longer the one of a live session gives no replay.
pub const MOST: usize = 4_096;

/// What the window says of a nonce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admitted {
    /// Not seen within the window: kept, and this session may use it.
    Fresh,
    /// Seen within the window: a replay, refused.
    Repeat,
}

/// The nonces that early data used, and when.
#[derive(Debug)]
pub struct ReplayWindow {
    window: Duration,
    seen: Mutex<HashMap<Vec<u8>, Instant>>,
}

impl Default for ReplayWindow {
    fn default() -> Self {
        ReplayWindow::new()
    }
}

impl ReplayWindow {
    /// A window of [`WINDOW`].
    pub fn new() -> ReplayWindow {
        ReplayWindow::with_window(WINDOW)
    }

    /// A window whose nonces live `window`. A test uses a short one.
    pub fn with_window(window: Duration) -> ReplayWindow {
        ReplayWindow { window, seen: Mutex::new(HashMap::new()) }
    }

    /// How long a nonce lives here.
    pub fn window(&self) -> Duration {
        self.window
    }

    /// Keeps `nonce` for the window, or refuses it as a repeat. The same
    /// call twice within the window is `Fresh` then `Repeat`.
    pub fn admit(&self, nonce: &[u8]) -> Admitted {
        self.admit_at(nonce, Instant::now())
    }

    /// [`admit`](ReplayWindow::admit) at `now`: a test moves the clock.
    pub fn admit_at(&self, nonce: &[u8], now: Instant) -> Admitted {
        let mut seen = self.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        seen.retain(|_, at| now.saturating_duration_since(*at) < self.window);
        if seen.contains_key(nonce) {
            return Admitted::Repeat;
        }
        if seen.len() >= MOST {
            // The oldest goes: the entry whose instant is earliest.
            if let Some(oldest) = seen.iter().min_by_key(|(_, at)| **at).map(|(nonce, _)| nonce.clone()) {
                seen.remove(&oldest);
            }
        }
        seen.insert(nonce.to_vec(), now);
        Admitted::Fresh
    }

    /// The nonces kept now: a test reads it, and a caller may report it.
    pub fn len(&self) -> usize {
        self.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REPLAY-1: a repeat inside the window is refused; the same nonce after
    /// it is once again a first use.
    #[test]
    fn a_repeated_nonce_is_refused_inside_the_window_and_taken_once_after_it() {
        let window = ReplayWindow::new();
        let start = Instant::now();
        assert_eq!(window.admit_at(b"nonce-one", start), Admitted::Fresh);
        assert_eq!(window.admit_at(b"nonce-one", start + Duration::from_secs(599)), Admitted::Repeat);
        assert_eq!(window.admit_at(b"nonce-two", start + Duration::from_secs(599)), Admitted::Fresh);
        assert_eq!(window.len(), 2);
        // Ten minutes and one second after the first use, the nonce is gone.
        let after = start + WINDOW + Duration::from_secs(1);
        assert_eq!(window.admit_at(b"nonce-one", after), Admitted::Fresh);
        assert_eq!(window.admit_at(b"nonce-one", after), Admitted::Repeat);
        assert_eq!(window.admit_at(b"nonce-two", after), Admitted::Repeat, "two seconds of the window left");
        assert_eq!(window.len(), 2);
    }

    /// A window of another length keeps its own bound, and each nonce is its
    /// bytes: two nonces that differ in one byte are two uses.
    #[test]
    fn each_nonce_is_its_bytes_and_the_window_is_the_one_it_was_made_with() {
        let window = ReplayWindow::with_window(Duration::from_millis(50));
        let start = Instant::now();
        assert_eq!(window.window(), Duration::from_millis(50));
        assert_eq!(window.admit_at(b"a", start), Admitted::Fresh);
        assert_eq!(window.admit_at(b"b", start), Admitted::Fresh);
        assert_eq!(window.admit_at(b"a", start + Duration::from_millis(60)), Admitted::Fresh);
    }

    /// The window keeps no more than [`MOST`] nonces; the oldest goes first.
    #[test]
    fn the_oldest_nonce_goes_when_the_window_is_full() {
        let window = ReplayWindow::new();
        let start = Instant::now();
        for i in 0..MOST {
            let nonce = format!("nonce-{i}");
            let at = start + Duration::from_millis(i as u64);
            assert_eq!(window.admit_at(nonce.as_bytes(), at), Admitted::Fresh);
        }
        assert_eq!(window.len(), MOST);
        let newest = format!("nonce-{}", MOST);
        assert_eq!(window.admit_at(newest.as_bytes(), start + Duration::from_secs(1)), Admitted::Fresh);
        assert_eq!(window.len(), MOST);
        // The first nonce went, so it is a first use again.
        assert_eq!(window.admit_at(b"nonce-0", start + Duration::from_secs(1)), Admitted::Fresh);
        assert_eq!(window.admit_at(newest.as_bytes(), start + Duration::from_secs(2)), Admitted::Repeat);
    }
}

//! The bytes down that the sessions of a listener of the poll carriers keep
//! for their dialers, and their cap ([`super::Poll::max_bytes`]). A pump
//! takes room for the bytes that it read before it keeps them, and waits
//! while the room is not free; the bytes go back at each acknowledgement.
//! Each [`Charge`] gives its bytes back at its drop, so an abort or an end
//! of a session frees them.

use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::Notify;

/// The cap of a listener and the bytes taken.
pub(super) struct Budget {
    held: Mutex<usize>,
    max: usize,
    freed: Notify,
}

impl Budget {
    pub fn new(max: usize) -> Arc<Budget> {
        Arc::new(Budget { held: Mutex::new(0), max, freed: Notify::new() })
    }

    fn held_mut(&self) -> MutexGuard<'_, usize> {
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The bytes taken now.
    pub fn held(&self) -> usize {
        *self.held_mut()
    }

    /// At least `n` bytes are free now.
    pub fn has_room(&self, n: usize) -> bool {
        self.max.saturating_sub(self.held()) >= n
    }

    /// Take `n` bytes (at most the cap), and wait while fewer are free.
    pub async fn take(self: &Arc<Self>, n: usize) -> Charge {
        let n = n.min(self.max);
        loop {
            let notified = self.freed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut held = self.held_mut();
                if self.max.saturating_sub(*held) >= n {
                    *held += n;
                    return Charge { budget: self.clone(), n };
                }
            }
            notified.await;
        }
    }

    /// No bytes: the charge of the bytes that a session keeps, at its open.
    pub fn none(self: &Arc<Self>) -> Charge {
        Charge { budget: self.clone(), n: 0 }
    }

    fn give(&self, n: usize) {
        if n == 0 {
            return;
        }
        let mut held = self.held_mut();
        *held = held.saturating_sub(n);
        drop(held);
        self.freed.notify_waiters();
    }
}

/// Bytes taken from a [`Budget`]; its drop gives them back.
pub(super) struct Charge {
    budget: Arc<Budget>,
    n: usize,
}

impl Charge {
    #[cfg(test)]
    fn bytes(&self) -> usize {
        self.n
    }

    /// Move each byte of `from` into this charge.
    pub fn absorb(&mut self, mut from: Charge) {
        self.n += std::mem::take(&mut from.n);
    }

    /// Give `n` bytes of this charge back.
    pub fn release(&mut self, n: usize) {
        let n = n.min(self.n);
        self.n -= n;
        self.budget.give(n);
    }
}

impl Drop for Charge {
    fn drop(&mut self) {
        self.budget.give(self.n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn a_take_waits_for_its_room_and_each_charge_gives_its_bytes_back() {
        let budget = Budget::new(100);
        let mut kept = budget.none();
        kept.absorb(budget.take(70).await);
        assert!(budget.has_room(30) && !budget.has_room(31));
        let other = budget.take(30).await;
        assert_eq!((kept.bytes(), other.bytes(), budget.held()), (70, 30, 100));
        let waiting = tokio::spawn({
            let budget = budget.clone();
            async move { budget.take(20).await.bytes() }
        });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished(), "no byte is free");
        kept.release(10);
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished(), "10 bytes are free, and the take needs 20");
        kept.release(10);
        let got = tokio::time::timeout(Duration::from_secs(5), waiting).await.expect("the wait ended").unwrap();
        assert_eq!((got, kept.bytes(), budget.held()), (20, 50, 80), "the take had its bytes and gave them back");
        drop((kept, other));
        assert_eq!(budget.held(), 0, "each charge gave its bytes back");
        assert_eq!(budget.take(1000).await.bytes(), 100, "a take of more than the cap takes the cap");
    }
}

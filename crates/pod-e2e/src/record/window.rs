//! The replay window of one direction: the highest counter that opened, and
//! a bitmap of the [`WINDOW`] counters at and behind it. A record at or
//! below `highest - WINDOW` is stale; a record whose bit is set is a
//! repeat. A record marks the window only after its check, so that a forged
//! record cannot move it.

use super::Fault;

/// The counters that the window holds behind the highest.
pub const WINDOW: u64 = 1024;
const WORDS: usize = (WINDOW / 64) as usize;

#[derive(Debug, Clone, Default)]
pub struct Window {
    /// The highest counter that opened; none before the first record.
    highest: Option<u64>,
    /// Bit `c % WINDOW` is set when counter `c` of the window opened.
    bits: [u64; WORDS],
}

impl Window {
    /// The highest counter that opened.
    pub fn highest(&self) -> Option<u64> {
        self.highest
    }

    /// The counter after the highest that opened: what this end waits for
    /// next.
    pub fn next(&self) -> u64 {
        self.highest.map_or(0, |h| h.saturating_add(1))
    }

    /// Whether `counter` can open: above the highest, or in the window and
    /// not there yet.
    pub fn check(&self, counter: u64) -> Result<(), Fault> {
        let Some(highest) = self.highest else { return Ok(()) };
        if counter > highest {
            return Ok(());
        }
        if highest - counter >= WINDOW {
            return Err(Fault::Stale(counter));
        }
        match self.bit(counter) {
            true => Err(Fault::Replayed(counter)),
            false => Ok(()),
        }
    }

    /// Mark `counter`, which opened.
    pub fn mark(&mut self, counter: u64) {
        match self.highest {
            Some(highest) if counter <= highest => {}
            Some(highest) => {
                // The counters between the old highest and the new one did not
                // open: their bits are those of counters a window older.
                for c in (highest + 1..counter).take(WINDOW as usize) {
                    self.set(c, false);
                }
                self.highest = Some(counter);
            }
            None => {
                self.bits = [0; WORDS];
                self.highest = Some(counter);
            }
        }
        self.set(counter, true);
    }

    fn bit(&self, counter: u64) -> bool {
        let i = (counter % WINDOW) as usize;
        self.bits[i / 64] & (1 << (i % 64)) != 0
    }

    fn set(&mut self, counter: u64, on: bool) {
        let i = (counter % WINDOW) as usize;
        match on {
            true => self.bits[i / 64] |= 1 << (i % 64),
            false => self.bits[i / 64] &= !(1 << (i % 64)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeat_and_a_stale_counter_are_refused_and_a_late_one_opens() {
        let mut w = Window::default();
        assert_eq!(w.next(), 0);
        for c in [0, 1, 2, 5] {
            w.check(c).unwrap();
            w.mark(c);
        }
        assert_eq!((w.highest(), w.next()), (Some(5), 6));
        assert!(matches!(w.check(2), Err(Fault::Replayed(2))));
        assert!(matches!(w.check(5), Err(Fault::Replayed(5))));
        w.check(3).unwrap();
        w.mark(3);
        assert!(matches!(w.check(3), Err(Fault::Replayed(3))));
        w.check(4).unwrap();
        // A jump past the window: each counter below the edge is stale, and
        // a counter in the window that did not open can open.
        w.mark(5 + WINDOW);
        assert!(matches!(w.check(5), Err(Fault::Stale(5))));
        assert!(matches!(w.check(6), Ok(())), "highest - 1023 is in the window");
        assert!(matches!(w.check(5 + WINDOW), Err(Fault::Replayed(_))));
    }

    #[test]
    fn a_bit_of_a_counter_a_window_older_does_not_stay() {
        let mut w = Window::default();
        w.mark(10);
        // Counter 10 + WINDOW shares the bit of 10.
        w.mark(10 + WINDOW + 5);
        w.check(10 + WINDOW).unwrap();
        w.mark(10 + WINDOW);
        assert!(matches!(w.check(10 + WINDOW), Err(Fault::Replayed(_))));
    }
}

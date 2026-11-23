//! A small generator with a seed. The simulator and the search take the
//! same seed and give the same run.

/// xorshift64*: a generator that holds one word and repeats from a seed.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        // Zero is a fixed point of xorshift. A seed of zero starts at a
        // constant instead, so it still moves.
        Rng(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value below `n`. `n` must not be zero.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// True with the chance `num / den` (den must not be zero).
    pub fn chance(&mut self, num: u32, den: u32) -> bool {
        self.next_u64() % u64::from(den) < u64::from(num)
    }

    pub fn byte(&mut self) -> u8 {
        (self.next_u64() & 0xff) as u8
    }
}

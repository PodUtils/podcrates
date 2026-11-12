//! The budget of one stream through the relay (podnet's `RelayBudget`): the
//! stream counts its bytes, both ways, and the budget is spent at a number
//! of bytes or at an age. The relay ends a session at its caps
//! (`relay::session_byte_cap`, `relay::session_time_cap`), so a caller
//! replaces a spent stream first: it takes no new work, and its open work
//! goes on until it ends.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use pod_core::Metered;
use tokio::time::Instant;

use crate::context::Context;
use crate::relay::{byte_budget, time_budget};

/// Whether a stream through the relay spent its bytes or its time. Clones
/// share the count.
#[derive(Clone, Debug)]
pub struct RelayBudget {
    spent: Arc<AtomicBool>,
    opened: Instant,
    age: Duration,
}

impl RelayBudget {
    /// Count the bytes of `io` against a budget of `bytes` and `age`.
    pub fn meter<S>(io: S, bytes: u64, age: Duration) -> (Metered<S>, RelayBudget) {
        let spent = Arc::new(AtomicBool::new(false));
        let io = Metered::with_flag(io, bytes, spent.clone());
        (io, RelayBudget { spent, opened: Instant::now(), age })
    }

    /// [`RelayBudget::meter`] with the budgets of the run of `ctx`: under the
    /// limits of the relay that it uses, as a forward session's
    /// (`relay::byte_budget`, `relay::time_budget`).
    pub fn meter_for<S>(ctx: &Context, io: S) -> (Metered<S>, RelayBudget) {
        RelayBudget::meter(io, byte_budget(ctx, None), time_budget(&ctx.in_use()))
    }

    /// Whether the bytes or the time of the stream are spent.
    pub fn spent(&self) -> bool {
        self.spent.load(Ordering::Relaxed) || self.opened.elapsed() >= self.age
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::{SESSION_BYTE_BUDGET, SESSION_TIME_BUDGET};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    /// podnet's test: spent at its bytes (both directions) or at its age.
    #[tokio::test]
    async fn a_relay_budget_is_spent_at_its_bytes_or_its_age() {
        let (near, mut far) = tokio::io::duplex(1 << 16);
        let (mut io, budget) = RelayBudget::meter(near, 10, Duration::from_secs(3600));
        io.write_all(b"12345").await.unwrap();
        far.write_all(b"1234").await.unwrap();
        let mut got = [0u8; 4];
        io.read_exact(&mut got).await.unwrap();
        assert!(!budget.spent(), "9 of 10 bytes");
        io.write_all(b"x").await.unwrap();
        assert!(budget.spent());
        let (_io, aged) = RelayBudget::meter(tokio::io::duplex(8).0, 1 << 20, Duration::ZERO);
        assert!(aged.spent(), "the age is spent");
    }

    /// The budget of a run is the forward session's: under the limits of the
    /// relay in use, and spent at its age on tokio's clock.
    #[tokio::test(start_paused = true)]
    async fn the_budget_of_a_run_goes_under_the_limits_of_its_relay() {
        let ctx = crate::testing::context();
        let (_io, budget) = RelayBudget::meter_for(&ctx, tokio::io::duplex(8).0);
        assert_eq!(budget.age, SESSION_TIME_BUDGET);
        tokio::time::advance(SESSION_TIME_BUDGET - Duration::from_secs(1)).await;
        assert!(!budget.spent());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(budget.spent(), "spent at its age");
        // A relay with a smaller cap: the bytes go under it.
        ctx.set_in_use(crate::pool::Limits { max_session_bytes: Some(32 << 20), ..Default::default() });
        let (mut io, budget) = RelayBudget::meter_for(&ctx, tokio::io::sink());
        assert_eq!(byte_budget(&ctx, None), 28 << 20, "under {SESSION_BYTE_BUDGET}");
        let chunk = vec![0u8; 1 << 20];
        for _ in 0..27 {
            io.write_all(&chunk).await.unwrap();
        }
        io.write_all(&chunk[1..]).await.unwrap();
        assert!(!budget.spent());
        io.write_all(b"x").await.unwrap();
        assert!(budget.spent(), "spent at the run's byte budget");
    }
}

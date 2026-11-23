//! Pure, deterministic NAT classification and verdict engines.

pub mod alg;
pub mod alloc;
pub mod cgnat;
pub mod lifetime;
pub mod pooling;
pub mod verdict;

pub use alg::alg_from_responses;
pub use alloc::{AllocOutcome, AllocSample, classify_allocation, estimate_pool_band, is_time_correlated_counter};
pub use cgnat::{CgnatInput, cgnat_verdict};
pub use lifetime::{COARSE_RUNGS_SECS, RungOutcome, classify_lifetime, fine_rungs};
pub use pooling::{DestObs, MapView, PoolingAnalysis, classify_pooling};
pub use verdict::{
    Control, MIN_SENDS_FOR_TIMEOUT, Mapped, Probe, RING_LO, RING_SIZE, Verdict, filtering_verdict, mapping_verdict,
    mapping_verdict_multi, ring_delta, ring_wrap,
};

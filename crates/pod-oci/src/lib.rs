//! `TOOL.md` section 6.2: registry client, manifests, store, digests.
//! Milestone M1, [`TODO/milestones.md`](../../../TODO/milestones.md) T-1102 and
//! [`TODO/image.md`](../../../TODO/image.md) T-0201 to T-0204.
//!
//! ⭐ **Four rules decide whether this crate is right rather than merely
//! finished**, and each is one entry:
//!
//! 1. HTTPS only, with no plain-HTTP fallback at any layer ([`registry`],
//!    T-0201). tcp/80 egress is broken on the runtime podvm targets, so a
//!    fallback hangs instead of failing;
//! 2. every blob is verified **as it is written** and reaches `blobs/` only on
//!    success ([`digest::Verifier`], [`store`], T-0202);
//! 3. blocks **and inodes** are checked before any download, and the refusal
//!    names the destination, the free amount, the required amount and the unit
//!    ([`space`], T-0203);
//! 4. a GC cannot delete what a container is using, and says which it skipped
//!    ([`store::Lock`], T-0204).
//!
//! ⛔ **This crate is the first member to take a `[workspace.dependencies]`
//! pin.** Every one of them was ruled by a measured `cargo bloat` delta in
//! `TODO/deps.md` T-0901 to T-0910, and `experiments/110-bloat-delta.sh` is the
//! instrument. Adding one that was not measured is not a small change.
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod clock;
pub mod contain;
pub mod credentials;
pub mod digest;
pub mod error;
#[cfg(unix)]
pub mod exec_probe;
pub mod health;
pub mod layout;
pub mod net;
pub mod oci;
pub mod platform;
pub mod probe_cache;
pub mod pull;
pub mod reference;
pub mod registry;
pub mod space;
pub mod store;
pub mod tls;
pub mod transport;

pub use error::{Error, Result};
pub use reference::Reference;
pub use store::{Record, Store};

/// Open the store this machine should use, choosing a fallback root by free
/// space among the paths the probe obtained **by writing**.
///
/// ⛔ Deliberately not `$TMPDIR`. On the runtime podvm targets `/tmp` is
/// 64 MiB, which almost no image fits in, and taking the environment's
/// temporary directory with no space check is the shipped default this project
/// read in the corpus at
/// `references/VHSgunzo__memfd-exec/tree/src/executable.rs:580-584`.
///
/// ⚠ The probe is only run when the three configured locations are all unset,
/// which is the one case where podvm has nothing else to go on.
pub fn open_store() -> Result<Store> {
    let root = Store::default_root(|| {
        let (document, _) = probe_cache::measure("choosing a store root");
        let probe = probe_cache::Probe {
            document,
            source: probe_cache::Source::Measured("choosing a store root".into()),
            path: std::path::PathBuf::new(),
            not_written: None,
        };
        let writable = probe.writable();
        let mut dropped = Vec::new();
        let ranked = space::choose(
            writable.iter().map(|(p, s)| (p.as_str(), s.as_str())),
            &mut dropped,
        );
        ranked.into_iter().next().map(|c| c.path)
    })?;
    Store::open(root)
}

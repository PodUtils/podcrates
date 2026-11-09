//! `podvm pull`: resolve, precheck, fetch, verify, record.
//!
//! The order is the whole design and every step of it is an entry:
//!
//! 1. the reference is normalised, and `http://` is refused ([`crate::reference`], T-0201);
//! 2. the probe answer is taken from `$store/probe.json` where its key still
//!    holds ([`crate::probe_cache`], T-0111), because `pull` is one of the hot
//!    paths `TOOL.md` section 6.1 asks not to re-probe;
//! 3. the manifest is fetched and its digest **computed over the served bytes**
//!    (T-0202);
//! 4. blocks **and inodes** are checked at the destination before a single
//!    layer is fetched, and the refusal names the destination, the free amount,
//!    the required amount and the unit ([`crate::space`], T-0203);
//! 5. every blob is verified as it is written and is renamed into `blobs/` only
//!    on success (T-0202).
//!
//! ⛔ No extraction. `TODO/milestones.md` T-1102: M1 acquires, M2 extracts.

use std::io::Write;

use crate::digest::Digest;
use crate::error::{Error, Result};
use crate::oci::{self, Fetched};
use crate::platform::Platform;
use crate::reference::Reference;
use crate::registry::Client;
use crate::space;
use crate::store::{self, Record, StagedFile, Store};
use crate::transport::Policy;

/// What the store needs beyond the payload: the index, a staging copy of the
/// largest blob and the lock files.
///
/// ⚠ It is **not** an estimate of the extracted size. M2 checks that against
/// the layer contents it is about to write, and inventing a multiplier here
/// would be a fabricated number (`AGENTS.md` absolute 3).
pub const STORE_HEADROOM_BYTES: u64 = 16 * 1024 * 1024;

pub struct Pulled {
    pub record: Record,
    /// Blobs that were already in the store, by digest.
    pub reused: Vec<String>,
    pub fetched: Vec<String>,
    pub probe_source: crate::probe_cache::Source,
}

/// ⭐ `TODO/image.md` T-0207. How many blobs one pull fetches at once.
///
/// Fixed rather than scaled to the machine's cores: the constraint is the
/// registry's willingness to serve, not this machine's cores, and a
/// CPU-scaled bound on a many-core builder is how a client earns a rate
/// limit. Threads over an async runtime: `TODO/deps.md` T-0906 ruled a
/// blocking client on a measured size delta, and an async runtime for a
/// handful of downloads would reopen a decision that was closed against a
/// number.
///
/// ⛔ The arithmetic this has to satisfy: every worker holds exactly one
/// staging [`Lock`](crate::store::Store::stage) while it writes, and every
/// lock registers a fork-shed slot
/// (`podvm_probe::sys::FORK_CLOSE_SLOTS`). `FETCH_WORKERS` workers plus
/// [`FETCH_LOCK_HEADROOM`] transients stay under that ceiling, and a
/// seventeenth lock is refused by name rather than held unsafely. The `const`
/// assert below pins it at compile time.
pub const FETCH_WORKERS: usize = 4;
/// Transients any thread may hold while a fetch runs: the index lock at
/// record time, a sweep on another thread, and one spare. See
/// [`FETCH_WORKERS`].
pub const FETCH_LOCK_HEADROOM: usize = 3;
// ⛔ Pinned at compile time rather than in a test: raising `FETCH_WORKERS`
// past the headroom refuses the build instead of leaking lock fds into
// forked children in production. A `const` assert fires before any test
// runs, which is what makes it stronger than the runtime test it replaces.
const _: () = assert!(
    FETCH_WORKERS + FETCH_LOCK_HEADROOM <= podvm_probe::sys::FORK_CLOSE_SLOTS,
    "T-0207: workers plus transients must stay under the fork-shed slots"
);

/// One blob a worker fetches: a layer or the config, in manifest order.
struct FetchJob {
    digest: Digest,
    size: u64,
}

/// True where one blob's declared size alone passes the file-size
/// ceiling: the pre-flight half of the running cap `Client::drain`
/// enforces mid-stream. A missing ceiling (infinity, or unreadable)
/// binds nothing, because the pull worked without one before.
fn blob_over_ceiling(size: u64, ceiling: Option<u64>) -> bool {
    ceiling.is_some_and(|c| size > c)
}

/// What the fetch hands back per job: the worker wrote the bytes, this
/// value says what to do with the staged file.
///
/// ⛔ The worker owns the staged file's lifecycle in every arm, and the fetch
/// only writes: one staging discipline rather than one per fetch shape.
enum FetchStep {
    /// Written and verified; the worker flushes, syncs and commits it.
    Done(StagedFile),
    /// The rest was cancelled while this was staged; the worker drops the
    /// file and removes the staged path without committing.
    Skipped(StagedFile),
}

/// What one slot carries back to the thread that prints.
enum SlotOutcome {
    Done {
        notes: Vec<u8>,
    },
    /// A job no worker started: another one failed first.
    Skipped,
    Failed(Error),
}

/// Per job, in manifest order: a finished worker's buffered lines where one
/// finished, and the first failure in manifest order where one failed.
struct FetchReport {
    notes: Vec<Option<Vec<u8>>>,
    error: Option<Error>,
}

/// Remove one staged file after a failed or cancelled fetch.
///
/// ⛔ Committed blobs are NOT removed here and must not be: they are
/// content-addressed and verified, so keeping them is harmless and a later
/// pull reuses them. Only the unverified staging file goes, so no pull
/// leaves a partial store behind.
fn discard_staged(staged: &std::path::Path) {
    let _ = std::fs::remove_file(staged);
}

/// Fetch one job through the existing store functions: stage, let the fetch
/// write and verify, then flush, sync and commit.
///
/// ⛔ The flush, the sync and the commit live here rather than in each fetch,
/// so there is one write path and not two. A failure removes the staged file
/// here and sets the cancellation, so no worker leaves a `*.partial` behind
/// however the fetches interleave.
fn fetch_one<F>(
    store: &Store,
    job: &FetchJob,
    fetch: &F,
    cancel: &std::sync::atomic::AtomicBool,
) -> SlotOutcome
where
    F: Fn(&FetchJob, StagedFile, &mut Vec<u8>, &std::sync::atomic::AtomicBool) -> Result<FetchStep>
        + Sync,
{
    use std::sync::atomic::Ordering;
    let (staged, file) = match store.stage(job.digest.short()) {
        Ok(x) => x,
        Err(e) => {
            cancel.store(true, Ordering::Release);
            return SlotOutcome::Failed(e);
        }
    };
    // ⛔ Re-checked after staging: another worker may have failed while this
    // one staged, and a blob fetched past the cancellation is a download the
    // pull then throws away.
    if cancel.load(Ordering::Acquire) {
        drop(file);
        discard_staged(&staged);
        return SlotOutcome::Skipped;
    }
    let mut notes = Vec::new();
    match fetch(job, file, &mut notes, cancel) {
        Ok(FetchStep::Done(mut file)) => {
            let result = file
                .flush()
                .map_err(|e| Error::io(staged.display().to_string(), e))
                .and_then(|()| {
                    file.sync_all()
                        .map_err(|e| Error::io(staged.display().to_string(), e))
                })
                .and_then(|()| store.commit(&staged, &job.digest));
            match result {
                Ok(()) => SlotOutcome::Done { notes },
                Err(e) => {
                    drop(file);
                    discard_staged(&staged);
                    cancel.store(true, Ordering::Release);
                    SlotOutcome::Failed(e)
                }
            }
        }
        Ok(FetchStep::Skipped(file)) => {
            drop(file);
            discard_staged(&staged);
            SlotOutcome::Skipped
        }
        Err(e) => {
            discard_staged(&staged);
            cancel.store(true, Ordering::Release);
            SlotOutcome::Failed(e)
        }
    }
}

/// Fetch every job on at most [`FETCH_WORKERS`] threads, `std` only.
///
/// ⛔ Strided, not chunked: consecutive layers of one image are often one
/// size, and contiguous chunks would put every large layer on one worker.
/// ⛔ A failure in one worker cancels the rest: the flag stops any worker
/// starting another job, and every staged file is removed by its own worker.
/// What is already committed stays, content-addressed and harmless.
/// ⛔ This prints nothing. The caller assembles the transcript from `notes`
/// in manifest order, however the fetches interleaved.
fn fetch_parallel<F>(store: &Store, jobs: &[FetchJob], fetch: F) -> FetchReport
where
    F: Fn(&FetchJob, StagedFile, &mut Vec<u8>, &std::sync::atomic::AtomicBool) -> Result<FetchStep>
        + Sync,
{
    use std::sync::atomic::{AtomicBool, Ordering};
    if jobs.is_empty() {
        return FetchReport {
            notes: Vec::new(),
            error: None,
        };
    }
    let cancel = AtomicBool::new(false);
    // ⚠ Shared, not moved: each worker borrows the fetch and the flag through
    // these, so the bounds stay `Sync` rather than growing a `Send` no caller
    // needs. Both are `Copy` as shared references, so the `move` closures
    // below share them instead of taking them.
    let fetch = &fetch;
    let cancel = &cancel;
    // ⛔ A worker that ends without reporting is a loud refusal, never a
    // silent default: a missing blob read as fetched is the corruption
    // `docs/conventions/code.md` forbids.
    let per_worker = match std::thread::scope(|s| {
        let mut handles = Vec::with_capacity(FETCH_WORKERS);
        for w in 0..FETCH_WORKERS {
            handles.push(s.spawn(move || {
                let mut mine = Vec::new();
                let mut i = w;
                while i < jobs.len() {
                    if cancel.load(Ordering::Acquire) {
                        mine.push((i, SlotOutcome::Skipped));
                    } else {
                        mine.push((i, fetch_one(store, &jobs[i], fetch, cancel)));
                    }
                    i += FETCH_WORKERS;
                }
                mine
            }));
        }
        let mut out = Vec::with_capacity(FETCH_WORKERS);
        for h in handles {
            out.push(h.join().map_err(|_| {
                Error::Store(
                    "a fetch worker ended without reporting its jobs, so the \
                     pull is refused rather than reported partial"
                        .into(),
                )
            })?);
        }
        Ok::<_, Error>(out)
    }) {
        Ok(w) => w,
        Err(e) => {
            return FetchReport {
                notes: (0..jobs.len()).map(|_| None).collect(),
                error: Some(e),
            };
        }
    };
    // ⛔ Merged by INDEX, never in completion order: the transcript the
    // caller prints from `notes` stays in manifest order however the fetches
    // interleaved.
    let mut slots: Vec<Option<SlotOutcome>> = (0..jobs.len()).map(|_| None).collect();
    for (i, outcome) in per_worker.into_iter().flatten() {
        slots[i] = Some(outcome);
    }
    let mut notes = Vec::with_capacity(jobs.len());
    let mut error: Option<Error> = None;
    for (i, slot) in slots.into_iter().enumerate() {
        match slot {
            Some(SlotOutcome::Done { notes: n }) => notes.push(Some(n)),
            Some(SlotOutcome::Skipped) => notes.push(None),
            Some(SlotOutcome::Failed(e)) => {
                notes.push(None);
                if error.is_none() {
                    error = Some(e);
                }
            }
            // ⛔ Unreachable by construction: every index is dealt to exactly
            // one worker, and every worker reports every index it was dealt.
            // A silent default here would be a missing blob read as fetched.
            None => {
                notes.push(None);
                if error.is_none() {
                    error = Some(Error::Store(format!(
                        "worker assignment missed blob {i} of {}",
                        jobs.len()
                    )));
                }
            }
        }
    }
    FetchReport { notes, error }
}

impl Pulled {
    /// True when nothing had to be fetched, which is docker's "up to date".
    pub fn up_to_date(&self) -> bool {
        self.fetched.is_empty()
    }
}
mod run;
pub use run::*;

/// The digest a store already holds for a reference, for the "already present"
/// path that never touches the network.
pub fn local(store: &Store, want: &str) -> Result<Option<Digest>> {
    match store.find(want)?.first() {
        Some(r) => Ok(Some(Digest::parse(&r.digest)?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests;

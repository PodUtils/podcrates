//! The parts of a pull that do not know the store: the resolve of a
//! reference to one manifest of one platform, and the pool that fetches the
//! blobs of a manifest into the store of the caller ([`Blobs`]).
//!
//! The pool prints nothing. Each worker writes its lines to a buffer of
//! its job, and the caller prints the buffers in the order of the manifest,
//! however the fetches came in.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use pod_core::{Class, Failure, Stage};

use crate::digest::Digest;
use crate::error::{Result, registry};
use crate::platform::Platform;
use crate::reference::Reference;
use crate::registry::{Client, FetchedManifest, Restart};
use crate::spec::{self, Fetched, Manifest};

/// How many blobs one pull fetches at once, when the caller has no own
/// number. Fixed, not from the count of cores: the limit is what the
/// registry will serve, and a bound from the cores of a large builder earns
/// a limit of the rate.
pub const FETCH_WORKERS: usize = 4;

/// What a reference resolved to: the first manifest (an index or a
/// manifest), and the manifest of the platform with its bytes and digest.
pub struct Resolved {
    /// The document of the reference. Its digest is the digest of the
    /// reference: for a tag of more than one platform, the digest of the
    /// index, which `docker image inspect` gives in `RepoDigests`.
    pub top: FetchedManifest,
    pub manifest_digest: Digest,
    pub manifest_bytes: Vec<u8>,
    pub manifest: Manifest,
}

/// Fetch the manifest of `reference` and, for an index, the manifest of
/// `platform` in it. An index with no entry of the platform is
/// [`crate::Refusal::NoPlatform`]; an index in an index is
/// `Class::Unsupported`: an image is one level deep.
pub fn resolve(client: &mut Client, reference: &Reference, platform: &Platform) -> Result<Resolved> {
    let endpoint = reference.endpoint();
    let repository = &reference.repository;
    let top = client.manifest(endpoint, repository, &reference.manifest_selector())?;
    let (manifest_digest, manifest_bytes, manifest) = match Fetched::parse(&top.bytes, Some(&top.media_type))? {
        Fetched::Manifest(m) => (top.digest.clone(), top.bytes.clone(), m),
        Fetched::Index(index) => {
            let picked = spec::select_platform(&index, platform)?;
            let d = picked.parsed_digest()?;
            let inner = client.manifest(endpoint, repository, &d.to_string())?;
            match Fetched::parse(&inner.bytes, Some(&inner.media_type))? {
                Fetched::Manifest(m) => (inner.digest, inner.bytes, m),
                Fetched::Index(_) => {
                    return Err(registry(
                        Class::Unsupported,
                        format!(
                            "{endpoint}/{repository} served an index where the {platform} manifest should be. \
                             The client does not follow an index into another index: an image is one level deep"
                        ),
                    ));
                }
            }
        }
    };
    Ok(Resolved { top, manifest_digest, manifest_bytes, manifest })
}

/// The store of blobs of the caller, for the pool. A staged file is a file
/// at a path in the store: the commit of a checked blob is a rename, and the
/// pool removes the path of a blob that did not finish.
pub trait Blobs: Sync {
    /// The staged file of one blob. It can start again ([`Restart`]).
    type Staged: Write + Restart + Send;
    /// The error of the store and of a fetch. The pool makes its own
    /// failures (a worker that ended with no report) from a [`Failure`].
    type Error: From<Failure> + Send;
    /// A new staged file for `digest`, and its path. Each call gives a new
    /// path.
    fn stage(&self, digest: &Digest) -> std::result::Result<(std::path::PathBuf, Self::Staged), Self::Error>;
    /// Flush and sync `file`, then give the checked blob its name.
    fn commit(&self, path: &Path, file: Self::Staged, digest: &Digest) -> std::result::Result<(), Self::Error>;
    /// Remove the staged file at `path`, after a failed or a cancelled
    /// fetch. A committed blob is never removed: it is addressed by its
    /// content and checked, and a later pull uses it again.
    fn discard(&self, path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}

/// One blob that a worker fetches: a layer or the configuration, in the
/// order of the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub digest: Digest,
    pub size: u64,
}

/// True where the declared size of one blob alone is past the file-size
/// ceiling: the check before the fetch, beside the cap of
/// [`Client::blob`] in the stream. No ceiling (infinity, or no read)
/// limits nothing.
pub fn over_ceiling(size: u64, ceiling: Option<u64>) -> bool {
    ceiling.is_some_and(|c| size > c)
}

/// What a fetch gives back for its job. The worker owns the staged file in
/// each arm, and the fetch only writes: one way to stage, not one for each
/// kind of fetch.
pub enum Step<S> {
    /// Written and checked: the worker commits it.
    Done(S),
    /// The rest was cancelled while this was staged: the worker removes the
    /// staged file with no commit.
    Skipped(S),
}

/// What one slot gives back to the thread that prints.
enum Slot<E> {
    Done {
        notes: Vec<u8>,
    },
    /// A job that no worker started: another one failed first.
    Skipped,
    Failed(E),
}

/// For each job, in the order of the manifest: the lines of a finished
/// worker, and the first failure in the order of the manifest.
pub struct Report<E> {
    pub notes: Vec<Option<Vec<u8>>>,
    pub error: Option<E>,
}

/// Fetch one job: stage, let the fetch write and check, then commit. The
/// commit is here, not in each fetch: one way to write. A failure removes
/// the staged file here and sets the cancel flag, so no worker leaves a
/// staged file, however the fetches come in.
fn fetch_one<B, F>(blobs: &B, job: &Job, fetch: &F, cancel: &AtomicBool) -> Slot<B::Error>
where
    B: Blobs,
    F: Fn(&Job, B::Staged, &mut Vec<u8>, &AtomicBool) -> std::result::Result<Step<B::Staged>, B::Error> + Sync,
{
    let (staged, file) = match blobs.stage(&job.digest) {
        Ok(x) => x,
        Err(e) => {
            cancel.store(true, Ordering::Release);
            return Slot::Failed(e);
        }
    };
    // Checked again after the stage: another worker can fail meanwhile, and
    // a blob fetched after the cancel is a download that the pull discards.
    if cancel.load(Ordering::Acquire) {
        drop(file);
        blobs.discard(&staged);
        return Slot::Skipped;
    }
    let mut notes = Vec::new();
    match fetch(job, file, &mut notes, cancel) {
        Ok(Step::Done(file)) => match blobs.commit(&staged, file, &job.digest) {
            Ok(()) => Slot::Done { notes },
            Err(e) => {
                blobs.discard(&staged);
                cancel.store(true, Ordering::Release);
                Slot::Failed(e)
            }
        },
        Ok(Step::Skipped(file)) => {
            drop(file);
            blobs.discard(&staged);
            Slot::Skipped
        }
        Err(e) => {
            blobs.discard(&staged);
            cancel.store(true, Ordering::Release);
            Slot::Failed(e)
        }
    }
}

/// Fetch each job on at most `workers` threads (at least one), with `std`
/// only.
///
/// The jobs are dealt in strides, not in blocks: the layers of one image
/// often have one size, and blocks would put each large layer on one
/// worker. A failure of one worker cancels the rest: the flag stops each
/// worker before its next job, and each worker removes its own staged file.
/// What is committed stays. The report is in the order of the manifest.
pub fn fetch_parallel<B, F>(blobs: &B, jobs: &[Job], workers: usize, fetch: F) -> Report<B::Error>
where
    B: Blobs,
    F: Fn(&Job, B::Staged, &mut Vec<u8>, &AtomicBool) -> std::result::Result<Step<B::Staged>, B::Error> + Sync,
{
    if jobs.is_empty() {
        return Report { notes: Vec::new(), error: None };
    }
    let workers = workers.max(1);
    let cancel = AtomicBool::new(false);
    // Shared, not moved: each worker borrows the fetch and the flag.
    let fetch = &fetch;
    let cancel = &cancel;
    // A worker that ends with no report is a refusal, never a default: a
    // missing blob read as fetched is a corrupt store.
    let lost = |what: String| B::Error::from(Failure::new(Stage::Host, Class::Unavailable, what));
    let per_worker = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|w| {
                s.spawn(move || {
                    let mut mine = Vec::new();
                    let mut i = w;
                    while i < jobs.len() {
                        if cancel.load(Ordering::Acquire) {
                            mine.push((i, Slot::Skipped));
                        } else {
                            mine.push((i, fetch_one(blobs, &jobs[i], fetch, cancel)));
                        }
                        i += workers;
                    }
                    mine
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join()).collect::<std::result::Result<Vec<_>, _>>()
    });
    let per_worker = match per_worker {
        Ok(w) => w,
        Err(_) => {
            let text = "a fetch worker ended without reporting its jobs, so the pull is refused rather than \
                        reported partial";
            return Report { notes: (0..jobs.len()).map(|_| None).collect(), error: Some(lost(text.into())) };
        }
    };
    // Merged by index, never in the order of completion: the lines stay in
    // the order of the manifest.
    let mut slots: Vec<Option<Slot<B::Error>>> = (0..jobs.len()).map(|_| None).collect();
    for (i, outcome) in per_worker.into_iter().flatten() {
        slots[i] = Some(outcome);
    }
    let mut notes = Vec::with_capacity(jobs.len());
    let mut error = None;
    for (i, slot) in slots.into_iter().enumerate() {
        match slot {
            Some(Slot::Done { notes: n }) => notes.push(Some(n)),
            Some(Slot::Skipped) => notes.push(None),
            Some(Slot::Failed(e)) => {
                notes.push(None);
                error = error.or(Some(e));
            }
            // Each index goes to one worker, and each worker reports each
            // index that it got. A default here would be a missing blob
            // read as fetched.
            None => {
                notes.push(None);
                error = error.or_else(|| Some(lost(format!("worker assignment missed blob {i} of {}", jobs.len()))));
            }
        }
    }
    Report { notes, error }
}

#[cfg(test)]
mod tests;

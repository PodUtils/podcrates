use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

use super::*;

/// A store of blobs in a directory, for the pool: `staging/` and
/// `blobs/sha256/`.
struct Dir {
    root: PathBuf,
    next: AtomicU64,
}

impl Dir {
    fn new(name: &str) -> Dir {
        let root = std::env::temp_dir().join(format!("pod-oci-pull-par-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("staging")).unwrap();
        std::fs::create_dir_all(root.join("blobs/sha256")).unwrap();
        Dir { root, next: AtomicU64::new(0) }
    }

    fn has_blob(&self, d: &Digest) -> bool {
        self.root.join(d.blob_path()).exists()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Blobs for Dir {
    type Staged = std::fs::File;
    type Error = Failure;

    fn stage(&self, digest: &Digest) -> Result<(PathBuf, std::fs::File)> {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let path = self.root.join("staging").join(format!("{}.{n}.partial", digest.short()));
        let file = std::fs::File::create(&path).map_err(|e| crate::error::io(path.display(), e))?;
        Ok((path, file))
    }

    fn commit(&self, path: &Path, mut file: std::fs::File, digest: &Digest) -> Result<()> {
        file.flush().and_then(|()| file.sync_all()).map_err(|e| crate::error::io(path.display(), e))?;
        let dest = self.root.join(digest.blob_path());
        std::fs::rename(path, &dest).map_err(|e| crate::error::io(dest.display(), e))
    }
}

#[test]
fn only_a_blob_over_the_ceiling_is_over_it() {
    assert!(over_ceiling(101, Some(100)));
    assert!(!over_ceiling(100, Some(100)));
    assert!(!over_ceiling(99, Some(100)));
    assert!(!over_ceiling(u64::MAX, None), "no ceiling limits nothing, not everything");
}

fn par_jobs(n: usize) -> (Vec<String>, Vec<Job>) {
    let mut contents = Vec::new();
    let mut jobs = Vec::new();
    for i in 0..n {
        let content = format!("pool-blob-{i}");
        contents.push(content.clone());
        jobs.push(Job { digest: Digest::of(content.as_bytes()), size: content.len() as u64 });
    }
    (contents, jobs)
}

/// The merge is by index, never in the order of completion. Job `i` sleeps
/// `(n - i) * 25` ms, so the last job of the manifest ends first; a merge in
/// the order of completion would put the lines of job 5 at index 0.
#[test]
fn the_transcript_stays_in_manifest_order_however_fetches_interleave() {
    let store = Dir::new("order");
    let n = 6;
    let (contents, jobs) = par_jobs(n);
    let report = fetch_parallel(&store, &jobs, FETCH_WORKERS, |job, mut file, buf, _cancel| {
        let idx = jobs.iter().position(|j| j.digest == job.digest).expect("a job this call was not given");
        std::thread::sleep(std::time::Duration::from_millis(((n - idx) * 25) as u64));
        // The bytes match the digest, so the commit stores a blob that
        // agrees with its name.
        file.write_all(contents[idx].as_bytes()).map_err(|e| crate::error::io("order probe", e))?;
        buf.write_all(format!("job {idx}").as_bytes()).map_err(|e| crate::error::io("order probe", e))?;
        Ok(Step::Done(file))
    });
    assert!(report.error.is_none(), "no job fails here: {:?}", report.error.map(|e| e.to_string()));
    assert_eq!(report.notes.len(), n);
    for (i, note) in report.notes.iter().enumerate() {
        assert_eq!(note.as_deref(), Some(format!("job {i}").as_bytes()), "slot {i} carries another job's lines");
    }
    for job in &jobs {
        assert!(store.has_blob(&job.digest));
    }
}

/// The failure of one worker cancels the rest and no staged file stays:
/// the bad job fails with part of its bytes on disk, the jobs in flight
/// see the cancel and do not run out their wait, the job that no worker
/// started stays skipped, and `staging/` holds no `*.partial` after.
#[test]
fn a_failure_cancels_the_rest_and_leaves_no_staged_file() {
    use std::sync::atomic::AtomicUsize;
    const POISON: usize = 1;
    let store = Dir::new("cancel");
    let n = 6;
    let (_, jobs) = par_jobs(n);
    let jobs_ref = &jobs;
    let observed = AtomicUsize::new(0);
    let report = fetch_parallel(&store, &jobs, FETCH_WORKERS, |job, mut file, _buf, cancel| {
        let idx = jobs_ref.iter().position(|j| j.digest == job.digest).expect("a job this call was not given");
        if idx == POISON {
            // The others stage first, so that the cancel has jobs in flight
            // to reach; then a failure with bytes that the worker removes.
            std::thread::sleep(std::time::Duration::from_millis(100));
            file.write_all(b"partial").map_err(|e| crate::error::io("poison probe", e))?;
            drop(file);
            return Err(crate::Refusal::Digest {
                what: "poison probe".to_string(),
                want: job.digest.to_string(),
                got: "sha256:dead".to_string(),
            }
            .failure());
        }
        // In flight: wait for the cancel, with a limit, then yield with no
        // commit.
        let start = std::time::Instant::now();
        let bound = std::time::Duration::from_secs(30);
        while start.elapsed() < bound {
            if cancel.load(Ordering::Acquire) {
                observed.fetch_add(1, Ordering::Release);
                return Ok(Step::Skipped(file));
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        drop(file);
        Err(Failure::new(Stage::Host, Class::Timeout, "cancel probe ran out its wait without seeing the cancellation"))
    });
    // The failure of the bad job, not of the cancel, and each other slot
    // holds nothing.
    let Some(e) = report.error.as_ref() else {
        panic!("the poisoned job did not fail");
    };
    assert!(
        e.to_string().contains(&jobs[POISON].digest.to_string()),
        "the reported failure is not the poisoned blob: {e}"
    );
    assert!(report.notes.iter().all(|x| x.is_none()), "a job committed beside a failure");
    // Three in flight (0, 2, 3) saw the flag; jobs 4 and 5 never started,
    // as their workers were on job 0 and on the bad job when it failed.
    assert_eq!(observed.load(Ordering::Acquire), 3, "an in-flight worker missed the cancellation");
    // No staged file stays, and nothing was committed.
    let left: Vec<String> = std::fs::read_dir(store.root.join("staging"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|x| x.ends_with(".partial"))
        .collect();
    assert!(left.is_empty(), "staged files survived the failure: {left:?}");
    for job in &jobs {
        assert!(!store.has_blob(&job.digest), "a blob was committed beside a failure");
    }
}

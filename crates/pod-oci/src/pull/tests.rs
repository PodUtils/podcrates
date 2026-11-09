use super::*;

fn par_store(name: &str) -> Store {
    let d = std::env::temp_dir().join(format!("podvm-pull-par-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    Store::open(d).unwrap()
}

#[test]
fn only_a_blob_over_the_ceiling_is_over_it() {
    assert!(blob_over_ceiling(101, Some(100)));
    assert!(!blob_over_ceiling(100, Some(100)));
    assert!(!blob_over_ceiling(99, Some(100)));
    assert!(
        !blob_over_ceiling(u64::MAX, None),
        "no ceiling binds nothing, not everything"
    );
}

fn par_jobs(n: usize) -> (Vec<String>, Vec<FetchJob>) {
    let mut contents = Vec::new();
    let mut jobs = Vec::new();
    for i in 0..n {
        let content = format!("t0207-blob-{i}");
        contents.push(content.clone());
        jobs.push(FetchJob {
            digest: Digest::of(content.as_bytes()),
            size: content.len() as u64,
        });
    }
    (contents, jobs)
}

/// ⛔ TODO/image.md T-0207. The merge is by INDEX, never in completion
/// order. The forced interleave is job `i` sleeping `(n - i) * 25` ms, so
/// the last job in manifest order finishes first; a merge in completion
/// order would carry job 5's lines at index 0 and this would go red.
#[test]
fn the_transcript_stays_in_manifest_order_however_fetches_interleave() {
    let store = par_store("order");
    let n = 6;
    let (contents, jobs) = par_jobs(n);
    let report = fetch_parallel(&store, &jobs, |job, mut file, buf, _cancel| {
        use std::io::Write;
        let idx = jobs
            .iter()
            .position(|j| j.digest == job.digest)
            .expect("a job this call was not given");
        std::thread::sleep(std::time::Duration::from_millis(((n - idx) * 25) as u64));
        // ⚠ The bytes match the digest, so the commit below stores a
        // consistent blob rather than an unverified one.
        file.write_all(contents[idx].as_bytes())
            .map_err(|e| Error::io("t0207 order probe", e))?;
        buf.write_all(format!("job {idx}").as_bytes())
            .map_err(|e| Error::io("t0207 order probe", e))?;
        Ok(FetchStep::Done(file))
    });
    assert!(
        report.error.is_none(),
        "no job fails here: {:?}",
        report.error.map(|e| e.to_string())
    );
    assert_eq!(report.notes.len(), n);
    for (i, note) in report.notes.iter().enumerate() {
        assert_eq!(
            note.as_deref(),
            Some(format!("job {i}").as_bytes()),
            "slot {i} carries another job's lines"
        );
    }
    for job in &jobs {
        assert!(store.has_blob(&job.digest));
    }
    let _ = std::fs::remove_dir_all(store.root());
}

/// ⛔ TODO/image.md T-0207. One worker's failure cancels the rest and no
/// staged file survives: the poisoned job fails with partial bytes on
/// disk, the in-flight jobs observe the cancellation rather than running
/// out their wait, the job never started stays skipped, and the staging
/// directory holds zero `*.partial` afterwards.
#[test]
fn a_failure_cancels_the_rest_and_leaves_no_staged_file() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    const POISON: usize = 1;
    let store = par_store("cancel");
    let n = 6;
    let (_, jobs) = par_jobs(n);
    let jobs_ref = &jobs;
    let observed = AtomicUsize::new(0);
    let report = fetch_parallel(&store, &jobs, |job, mut file, _buf, cancel| {
        use std::io::Write;
        let idx = jobs_ref
            .iter()
            .position(|j| j.digest == job.digest)
            .expect("a job this call was not given");
        if idx == POISON {
            // ⛔ Let the others stage first, so the cancellation has
            // something in flight to reach, then fail with partial
            // bytes the worker must remove.
            std::thread::sleep(std::time::Duration::from_millis(100));
            file.write_all(b"partial")
                .map_err(|e| Error::io("t0207 poison probe", e))?;
            drop(file);
            return Err(Error::DigestMismatch {
                what: "t0207 poison probe".to_string(),
                want: job.digest.to_string(),
                got: "sha256:dead".to_string(),
            });
        }
        // In flight: wait for the cancellation, bounded, then yield
        // without committing.
        let start = std::time::Instant::now();
        let bound = std::time::Duration::from_secs(30);
        while start.elapsed() < bound {
            if cancel.load(Ordering::Acquire) {
                observed.fetch_add(1, Ordering::Release);
                return Ok(FetchStep::Skipped(file));
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        drop(file);
        Err(Error::Store(
            "t0207 cancel probe ran out its wait without seeing the cancellation".into(),
        ))
    });
    // The poison's error, not a cancellation artefact, and every other
    // slot holds nothing.
    let Some(e) = report.error.as_ref() else {
        panic!("the poisoned job did not fail");
    };
    assert!(
        e.to_string().contains(&jobs[POISON].digest.to_string()),
        "the reported failure is not the poisoned blob: {e}"
    );
    assert!(
        report.notes.iter().all(|x| x.is_none()),
        "a job committed beside a failure"
    );
    // Three in flight (0, 2, 3) observed the flag; jobs 4 and 5 never
    // started, because their workers were busy on job 0 and the poison
    // when it failed.
    assert_eq!(
        observed.load(Ordering::Acquire),
        3,
        "an in-flight worker missed the cancellation"
    );
    // ⛔ Zero staged files left, and nothing committed either.
    let staging = store.root().join("staging");
    let left: Vec<String> = std::fs::read_dir(&staging)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|x| x.ends_with(".partial"))
        .collect();
    assert!(
        left.is_empty(),
        "staged files survived the failure: {left:?}"
    );
    for job in &jobs {
        assert!(
            !store.has_blob(&job.digest),
            "a blob was committed beside a failure"
        );
    }
    let _ = std::fs::remove_dir_all(store.root());
}

/// ⛔ The refusal T-0201 won, kept, and now able to say what would permit
/// it. A message a caller cannot act on is a message that costs a session.
#[test]
fn an_http_reference_is_refused_and_the_refusal_names_the_flag() {
    let store =
        Store::open(std::env::temp_dir().join(format!("podvm-pull-http-{}", std::process::id())))
            .unwrap();
    let mut out = Vec::new();
    let e = pull(
        &store,
        "http://localhost:5000/x:latest",
        &Platform::host(),
        &Policy::default(),
        &mut out,
    );
    let Err(e) = e else {
        panic!("a pull succeeded with nothing listening")
    };
    let text = format!("{e}");
    assert!(
        text.contains("--insecure-registry localhost:5000"),
        "{text}"
    );
    assert!(text.contains("T-0201"), "{text}");
    // ⛔ And nothing was fetched: the refusal is before the network.
    assert!(out.is_empty(), "it printed a transcript before refusing");
    let _ = std::fs::remove_dir_all(store.root());
}

/// ⭐ The same reference, with the registry named insecure, gets past the
/// policy. ⚠ It then fails to CONNECT, because nothing is listening on
/// localhost:5000 in a test, and that is the right place to stop: this
/// asserts the policy decision, and `experiments/280-insecure-registry.sh`
/// drives the whole path against a registry that is really there.
#[test]
fn naming_the_registry_insecure_gets_past_the_policy() {
    let store =
        Store::open(std::env::temp_dir().join(format!("podvm-pull-ok-{}", std::process::id())))
            .unwrap();
    let mut out = Vec::new();
    let e = pull(
        &store,
        "http://localhost:5000/x:latest",
        &Platform::host(),
        &Policy::with_insecure(&["localhost:5000"]),
        &mut out,
    );
    let Err(e) = e else {
        panic!("a pull succeeded with nothing listening")
    };
    let text = format!("{e}");
    assert!(
        !text.contains("--insecure-registry"),
        "the policy still refused it: {text}"
    );
    let _ = std::fs::remove_dir_all(store.root());
}

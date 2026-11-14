//! The fuzz target of `pod-actions-results`: the input of the client of a
//! run, to the artifact store and to the cache store.

use std::io::Seek;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use pod_actions_results::cache::CacheStore;
use pod_actions_results::{ArtifactStore, Payload, backend_ids, fuzzing, results_scope};
use pod_core::entropy::{Entropy, NoRandom};

use crate::text;

/// The limit of each store: small, so that a run reaches it.
const LIMIT: u64 = 1 << 12;
/// The most requests of one input.
const REQUESTS: usize = 24;
/// The line that ends a request.
const END: &str = "\n%%\n";

/// Random bytes of a fixed value: each signature is `11` 16 times, so a
/// seed can hold a signed URL.
struct Fixed;

impl Entropy for Fixed {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), NoRandom> {
        out.fill(0x11);
        Ok(())
    }
}

/// A new scratch directory of one input.
fn scratch() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("podcrates-fuzz-results-{}-{n}", std::process::id()))
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

/// The input of the client of a run (`pod_actions_results`; new in
/// `pod-actions-results`, T-565, T-588): the parsers of a query, of a
/// block list, of the claims of a runtime token and of a `Range`, then a
/// script of requests to an `ArtifactStore` and a `CacheStore` in a
/// scratch directory. Each request is `METHOD PATH [RANGE]` on its first
/// line and its body after it, up to a line `%%`. No panic; each status is
/// an HTTP status; a file of an answer gives the bytes that it names; the
/// stores write only in their directories and keep their limits.
pub fn actions_results(data: &[u8]) {
    let input = String::from_utf8_lossy(data);
    let _ = fuzzing::query(&input, "sig");
    for id in fuzzing::block_list(&input) {
        assert!(!id.contains("</"), "{id:?}");
        assert_eq!(id, id.trim());
    }
    if let Some(ids) = backend_ids(&input) {
        assert!(!ids.run.contains(':') && !ids.job.contains(':'), "{ids:?}");
        assert!(!ids.run.contains(' ') && !ids.job.contains(' '), "{ids:?}");
        let _ = results_scope(&ids.run, &ids.job);
    }
    for len in [0, 1, 10, u64::MAX] {
        if let Some((a, b)) = fuzzing::range(&input, len) {
            assert!(a <= b && b < len, "{input:?}: {a}-{b} of {len}");
        }
    }
    let Some(script) = text(data) else { return };
    let root = scratch();
    run(script, &root);
    assert_eq!(names(&root), ["artifacts", "cache"], "a store wrote outside its directory");
    let _ = std::fs::remove_dir_all(&root);
}

fn run(script: &str, root: &Path) {
    let Ok(artifacts) = ArtifactStore::with_entropy(&root.join("artifacts"), LIMIT, Fixed) else { return };
    let Ok(cache) = CacheStore::with_entropy(&root.join("cache"), LIMIT, Fixed) else { return };
    let cache = cache.with_scopes(vec!["refs/heads/x".into(), "refs/heads/main".into()]);
    let base = "http://h/";
    for request in script.split(END).take(REQUESTS) {
        let (head, body) = request.split_once('\n').unwrap_or((request, ""));
        let mut words = head.split(' ');
        let (method, path, range) = (words.next().unwrap_or(""), words.next().unwrap_or(""), words.next());
        let answer = match artifacts.answer(method, path, body.as_bytes(), base) {
            Some(a) => Some(a),
            None => cache.answer(method, path, body.as_bytes(), range, base),
        };
        let Some(answer) = answer else { continue };
        assert!((100..600).contains(&answer.status), "{}", answer.status);
        if let Payload::File(f, len) = &answer.body {
            let mut at: &std::fs::File = f;
            let (pos, all) = (at.stream_position().unwrap_or(0), f.metadata().map_or(0, |m| m.len()));
            assert!(pos + len <= all, "{len} bytes at {pos} of {all}");
        }
        let (_, bytes) = answer.into_bytes();
        assert!(bytes.len() as u64 <= LIMIT.max(1 << 16), "{}", bytes.len());
        assert!(artifacts.used() <= LIMIT, "{}", artifacts.used());
        let kept: u64 = cache.entries().iter().filter_map(|e| e["size"].as_u64()).sum();
        assert!(kept <= LIMIT, "the cache keeps {kept} bytes");
    }
}

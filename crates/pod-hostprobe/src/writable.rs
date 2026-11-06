//! Probe the directories that a process can write by writing, for blocks
//! and inodes both.
//!
//! `{/tmp, /dev/shm, /workspace, /state}` is the allowlist of one confined
//! machine, and nobody verified it. The set below is a set of
//! **candidates**, and this module reports the set that a write obtained.
//! The caller gives the candidates that its environment names.
//!
//! A real write, never `access(W_OK)`. `access` consults the capability set,
//! which reports every bit on this class of runtime and therefore answers the
//! wrong question.

use crate::sys::{self, Errno};
use crate::verdict::{Outcome, Verdict};

pub struct Space {
    pub blocks_free: u64,
    pub block_size: i64,
    pub inodes_free: u64,
}

pub struct WriteProbe {
    pub path: String,
    /// Why this path is a candidate: the fixed list, the variable that
    /// named it, or the working directory. A reader has to be able to tell.
    pub source: String,
    pub outcome: Outcome,
    /// `None` where `statfs(2)` itself failed; the errno is then in
    /// [`WriteProbe::space_error`].
    pub space: Option<Space>,
    pub space_error: Option<Errno>,
}

impl WriteProbe {
    pub fn writable(&self) -> bool {
        self.outcome.verdict == Verdict::Ok
    }
}

/// The directories in which a workload can put things.
///
/// This is the candidate list, not a claim about any machine. The four of
/// the confined target come first; the rest are where an ordinary host puts
/// the same things.
pub const FIXED: &[&str] = &["/tmp", "/dev/shm", "/workspace", "/state", "/var/tmp", "/run"];

/// The variables of the environment that name a directory of a workload.
/// The crate does not read them: the caller reads them, puts its own first,
/// and gives each value with its name to [`candidates`].
pub const VARIABLES: &[&str] = &["XDG_RUNTIME_DIR", "XDG_DATA_HOME", "TMPDIR", "HOME"];

/// The candidates, in order: [`FIXED`], then each `(path, source)` of
/// `named` (the caller's, for example `("/data", "$TMPDIR")`), then the
/// working directory. An empty path and a path already in the list are
/// dropped.
pub fn candidates(named: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> =
        FIXED.iter().map(|p| (p.to_string(), "the candidate list".to_string())).collect();
    for (s, source) in named {
        if !s.is_empty() && !out.iter().any(|(p, _)| p == s) {
            out.push((s.clone(), source.clone()));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let s = cwd.to_string_lossy().to_string();
        if !out.iter().any(|(p, _)| *p == s) {
            out.push((s, "the working directory".to_string()));
        }
    }
    out
}

/// One candidate: a real file created, written, closed and removed, then
/// `statfs(2)` for blocks and inodes both.
fn probe_one(path: &str, source: &str, tag: &str) -> WriteProbe {
    let file = format!("{}/.pod-hostprobe-write-{tag}", path.trim_end_matches('/'));
    let outcome = match crate::sys::CBuf::new(&file) {
        None => Outcome::skip(None, "the path contains a NUL and cannot reach the kernel"),
        Some(p) => match sys::open(&p, sys::O_WRONLY | sys::O_CREAT | sys::O_EXCL, 0o600) {
            Ok(fd) => {
                // Write bytes, not just create. A filesystem that is full,
                // or read-only in a way `open` does not reach, answers here.
                let wrote = sys::write(fd, b"probe\n");
                let _ = sys::close(fd);
                let _ = sys::unlink(&p);
                match wrote {
                    Ok(_) => Outcome::ok(),
                    Err(e) => Outcome::denied(e),
                }
            }
            Err(sys::EEXIST) => {
                Outcome::skip(Some(sys::EEXIST), format!("{file} already exists and this probe does not overwrite"))
            }
            Err(sys::ENOENT) => Outcome::skip(
                Some(sys::ENOENT),
                format!("{path} does not exist on this machine, so no write was attempted"),
            ),
            Err(e) => Outcome::denied(e),
        },
    };

    let (space, space_error) = match crate::sys::CBuf::new(path).map(|p| sys::statfs(&p)) {
        Some(Ok(s)) => (Some(Space { blocks_free: s.f_bavail, block_size: s.f_bsize, inodes_free: s.f_ffree }), None),
        Some(Err(e)) => (None, Some(e)),
        None => (None, None),
    };

    WriteProbe { path: path.to_string(), source: source.to_string(), outcome, space, space_error }
}

/// Distinguishes concurrent probes **within one process**, which the pid
/// cannot.
static CALL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Probe each of `candidates`, as [`candidates`] gives them.
pub fn probe(candidates: &[(String, String)]) -> Vec<WriteProbe> {
    // The tag keeps two concurrent probes from colliding on one name, which
    // would make each report the other's EEXIST as its own answer, and a
    // candidate this machine CAN write to would be reported as a skip.
    //
    // THE PID ALONE IS NOT ENOUGH: it gave an intermittent failure.
    // `cargo test` runs tests in parallel THREADS of one process, so two
    // concurrent `probe()` calls shared a pid and therefore a filename: the
    // second saw `EEXIST` and skipped `/tmp`. It is not confined to tests: a
    // run can call the probe while another thread measures for a cache.
    //
    // The counter is per PROCESS and the pid is per machine, so the pair is
    // unique on both axes. A random number would need an entropy source in a
    // function that is also called between fork and exec elsewhere in this
    // crate; a counter needs nothing.
    let tag = format!("{}-{}", std::process::id(), CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    candidates.iter().map(|(p, src)| probe_one(p, src, &tag)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The regression that produced the counter, made deterministic.
    ///
    /// Two threads probe at once, which is exactly what `cargo test` does to
    /// this crate. With the tag as the pid alone they share a filename, the
    /// loser reads `EEXIST` and reports `/tmp` as a **skip**, and a caller sees
    /// a machine it can write to described as one it cannot.
    #[test]
    fn two_concurrent_probes_do_not_take_each_other_s_scratch_name() {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    probe(&candidates(&[]))
                        .into_iter()
                        .filter(|w| w.path == "/tmp")
                        .map(|w| w.outcome.verdict)
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for h in handles {
            let verdicts = h.join().expect("a probe thread panicked");
            for v in verdicts {
                assert!(
                    matches!(v, crate::verdict::Verdict::Ok),
                    "a concurrent probe reported /tmp as {v:?}: the scratch \
                     names collided"
                );
            }
        }
    }

    #[test]
    fn the_candidate_list_has_no_duplicates() {
        // A named path that the fixed list holds already is dropped.
        let named = [("/tmp".to_string(), "$TMPDIR".to_string()), ("/tmp".to_string(), "$HOME".to_string())];
        let c = candidates(&named);
        assert!(c.iter().any(|(p, s)| p == "/tmp" && s == "the candidate list"));
        let mut paths: Vec<&str> = c.iter().map(|(p, _)| p.as_str()).collect();
        paths.sort_unstable();
        let before = paths.len();
        paths.dedup();
        assert_eq!(before, paths.len());
    }

    #[test]
    fn tmp_is_writable_here_and_reports_blocks_and_inodes() {
        // The real path, on this host.
        let got = probe_one("/tmp", "test", "selftest");
        assert!(got.writable(), "{:?}", got.outcome);
        let space = got.space.expect("statfs on /tmp");
        assert!(space.block_size > 0);
    }

    #[test]
    fn a_directory_that_does_not_exist_is_a_skip_and_not_a_denial() {
        let got = probe_one("/proc/self/no-such-directory", "test", "selftest");
        assert_eq!(got.outcome.verdict, Verdict::Skip);
        assert_eq!(got.outcome.errno, Some(sys::ENOENT));
    }
}

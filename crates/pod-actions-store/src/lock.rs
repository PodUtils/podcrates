//! The lock file of the actions of a workflow (T-118):
//! `podrunner-actions.lock` beside the workflow. Each line pins one
//! reference `OWNER/REPO@REF` to a commit and to the SHA-256 of the
//! uncompressed tar of that commit:
//!
//! ```text
//! actions/github-script@v7 f28e40c7f34bde8b3046d885e986cb6290c5673b sha256:HEX
//! ```
//!
//! A line that starts with `#` is a comment. The lines are sorted, so a
//! change of the lock is a small diff.

use std::collections::BTreeMap;
use std::path::Path;

/// The name of the lock file, beside the workflow.
pub const LOCK_FILE: &str = "podrunner-actions.lock";

const HEADER: &str = "# The actions of the workflow beside this file, pinned by commit and by\n\
# the SHA-256 of the tar of the commit. `podrunner run` writes this file.\n";

/// The commit and the digest of one reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin {
    pub commit: String,
    /// `sha256:HEX` of the uncompressed tar.
    pub digest: String,
}

/// Each reference `OWNER/REPO@REF` and its pin.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lock {
    pub pins: BTreeMap<String, Pin>,
}

/// A full commit id: 40 hex digits.
pub fn is_commit(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_digest(s: &str) -> bool {
    s.strip_prefix("sha256:").is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Lock {
    pub fn parse(text: &str) -> Result<Lock, String> {
        let mut pins = BTreeMap::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let words: Vec<&str> = line.split_whitespace().collect();
            let [key, commit, digest] = words[..] else {
                return Err(format!("line {}: not `OWNER/REPO@REF COMMIT sha256:HEX`", n + 1));
            };
            let ok = key.split_once('@').is_some_and(|(r, v)| r.split('/').count() == 2 && !v.is_empty());
            if !ok || !is_commit(commit) || !is_digest(digest) {
                return Err(format!("line {}: not `OWNER/REPO@REF COMMIT sha256:HEX`", n + 1));
            }
            let pin = Pin { commit: commit.to_ascii_lowercase(), digest: digest.to_string() };
            if pins.insert(key.to_string(), pin).is_some() {
                return Err(format!("line {}: `{key}` again", n + 1));
            }
        }
        Ok(Lock { pins })
    }

    /// The lock in `path`; an empty lock when there is no file.
    pub fn read(path: &Path) -> Result<Lock, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Lock::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Lock::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn render(&self) -> String {
        let mut out = HEADER.to_string();
        for (key, pin) in &self.pins {
            out.push_str(&format!("{key} {} {}\n", pin.commit, pin.digest));
        }
        out
    }

    /// Write the lock through a temporary file, so a reader never sees a
    /// part of it.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let tmp = path.with_extension("lock.tmp");
        std::fs::write(&tmp, self.render()).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lock_reads_back_what_it_writes() {
        let mut lock = Lock::default();
        let pin = Pin { commit: "a".repeat(40), digest: format!("sha256:{}", "b".repeat(64)) };
        lock.pins.insert("o/r@v1".into(), pin.clone());
        lock.pins.insert("a/b@main".into(), pin);
        let text = lock.render();
        assert!(text.find("a/b@main").unwrap() < text.find("o/r@v1").unwrap(), "sorted: {text}");
        assert_eq!(Lock::parse(&text).unwrap(), lock);
    }

    #[test]
    fn a_bad_line_is_refused_with_its_number() {
        let c = "a".repeat(40);
        let d = format!("sha256:{}", "b".repeat(64));
        for bad in [
            format!("o/r@v1 {c}"),
            format!("o/r {c} {d}"),
            format!("o/r/p@v1 {c} {d}"),
            format!("o/r@v1 abc {d}"),
            format!("o/r@v1 {c} sha256:12"),
            format!("o/r@v1 {c} {d}\no/r@v1 {c} {d}"),
        ] {
            let e = Lock::parse(&format!("# c\n{bad}")).unwrap_err();
            assert!(e.starts_with("line 2") || e.starts_with("line 3"), "{bad}: {e}");
        }
    }
}

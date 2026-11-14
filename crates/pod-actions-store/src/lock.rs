//! The lock file of the actions of a workflow. Each line pins one reference
//! `OWNER/REPO@REF` to a commit and to the SHA-256 of the uncompressed tar
//! of that commit:
//!
//! ```text
//! actions/github-script@v7 f28e40c7f34bde8b3046d885e986cb6290c5673b sha256:HEX
//! ```
//!
//! An image of `container:`, `services:` or a `docker://` step that a tag
//! names has a line of two words: `docker://NAME:TAG sha256:HEX`, the digest
//! of the image in the store of the consumer ([`crate::images`]).
//!
//! A line that starts with `#` is a comment. The lines are sorted, so a
//! change of the lock is a small diff. The consumer names the file.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Error, LineFault};

/// The comment at the top of a lock that [`Lock::render`] writes.
pub const HEADER: &str = "# The actions of the workflow beside this file, pinned by commit and by\n\
# the SHA-256 of the uncompressed tar of the commit, and its images by tag,\n\
# pinned by digest.\n";

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
    /// Each image `NAME:TAG` and its digest `sha256:HEX`.
    pub images: BTreeMap<String, String>,
}

/// A full commit id: 40 hex digits.
pub fn is_commit(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A digest `sha256:HEX` of 64 hex digits.
pub(crate) fn is_digest(s: &str) -> bool {
    s.strip_prefix("sha256:").is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Lock {
    pub fn parse(text: &str) -> Result<Lock, Error> {
        let mut pins = BTreeMap::new();
        let mut images = BTreeMap::new();
        for (n, line) in text.lines().enumerate() {
            let bad = |fault| Error::Line { path: None, line: n + 1, fault };
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let words: Vec<&str> = line.split_whitespace().collect();
            if let Some(image) = words[0].strip_prefix("docker://") {
                let [_, digest] = words[..] else {
                    return Err(bad(LineFault::ImageForm));
                };
                // Only the form that `images::by_tag` gives: a lookup never
                // finds an image with no tag or with a digest.
                if crate::images::by_tag(image).as_deref() != Some(image) || !is_digest(digest) {
                    return Err(bad(LineFault::ImageForm));
                }
                if images.insert(image.to_string(), digest.to_string()).is_some() {
                    return Err(bad(LineFault::Again(format!("docker://{image}"))));
                }
                continue;
            }
            let [key, commit, digest] = words[..] else {
                return Err(bad(LineFault::Form));
            };
            let ok = key.split_once('@').is_some_and(|(r, v)| r.split('/').count() == 2 && !v.is_empty());
            if !ok || !is_commit(commit) || !is_digest(digest) {
                return Err(bad(LineFault::Form));
            }
            let pin = Pin { commit: commit.to_ascii_lowercase(), digest: digest.to_string() };
            if pins.insert(key.to_string(), pin).is_some() {
                return Err(bad(LineFault::Again(key.to_string())));
            }
        }
        Ok(Lock { pins, images })
    }

    /// The lock in `path`; an empty lock when there is no file.
    pub fn read(path: &Path) -> Result<Lock, Error> {
        match std::fs::read_to_string(path) {
            Ok(text) => Lock::parse(&text).map_err(|e| match e {
                Error::Line { line, fault, .. } => Error::Line { path: Some(path.to_path_buf()), line, fault },
                e => e,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Lock::default()),
            Err(e) => Err(Error::file(path, &e)),
        }
    }

    /// The lock with [`HEADER`] on top.
    pub fn render(&self) -> String {
        self.render_with(HEADER)
    }

    /// The lock with the consumer's comment on top: each line of `header`
    /// must start with `#`.
    pub fn render_with(&self, header: &str) -> String {
        let mut out = header.to_string();
        for (key, pin) in &self.pins {
            out.push_str(&format!("{key} {} {}\n", pin.commit, pin.digest));
        }
        for (image, digest) in &self.images {
            out.push_str(&format!("docker://{image} {digest}\n"));
        }
        out
    }

    /// Write the lock through a temporary file, so a reader never sees a
    /// part of it.
    pub fn write(&self, path: &Path) -> Result<(), Error> {
        self.write_with(path, HEADER)
    }

    /// [`Lock::write`] with the consumer's comment on top.
    pub fn write_with(&self, path: &Path, header: &str) -> Result<(), Error> {
        let tmp = path.with_extension("lock.tmp");
        std::fs::write(&tmp, self.render_with(header)).map_err(|e| Error::file(&tmp, &e))?;
        std::fs::rename(&tmp, path).map_err(|e| Error::file(path, &e))
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
        lock.images.insert("redis:7-alpine".into(), format!("sha256:{}", "c".repeat(64)));
        let text = lock.render();
        assert!(text.contains("docker://redis:7-alpine sha256:"), "an image line: {text}");
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
            format!("docker://redis:7 {c}"),
            format!("docker://redis@sha256:00 {d}"),
            format!("docker://redis {d}"),
            format!("docker://redis:7 {d}\ndocker://redis:7 {d}"),
        ] {
            let e = Lock::parse(&format!("# c\n{bad}")).unwrap_err().to_string();
            assert!(e.starts_with("line 2") || e.starts_with("line 3"), "{bad}: {e}");
        }
    }

    #[test]
    fn a_lock_file_is_written_whole_and_its_errors_name_the_file() {
        let dir = std::env::temp_dir().join(format!("pod-actions-store-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("actions.lock");
        assert_eq!(Lock::read(&path).unwrap(), Lock::default(), "no file is an empty lock");
        let mut lock = Lock::default();
        let pin = Pin { commit: "c".repeat(40), digest: format!("sha256:{}", "d".repeat(64)) };
        lock.pins.insert("z/z@v9".into(), pin.clone());
        lock.pins.insert("a/a@v1".into(), pin);
        lock.write_with(&path, "# mine\n").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "# mine");
        assert!(lines[1].starts_with("a/a@v1 ") && lines[2].starts_with("z/z@v9 "), "sorted: {text}");
        assert_eq!(Lock::read(&path).unwrap(), lock);
        assert!(!path.with_extension("lock.tmp").exists(), "no temporary file is left");
        std::fs::write(&path, "# c\nbad\n").unwrap();
        let e = Lock::read(&path).unwrap_err();
        assert!(matches!(&e, Error::Line { path: Some(p), line: 2, fault: LineFault::Form } if *p == path), "{e:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

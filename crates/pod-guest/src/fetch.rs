//! The store of a base image: bounded, with a digest, and all or nothing.
//!
//! Three bounds, and each is a refusal, never a truncation. Before the
//! transfer: a declared length over the ceiling. During the transfer: the
//! bytes past the ceiling (the limit of the caller and `RLIMIT_FSIZE`). At
//! the end: a digest that is not the pinned one. A refusal removes the
//! partial file, so that no later run trusts a truncated image under the
//! name of the destination.
//!
//! The ceiling applies to the declared length and to the bytes that come:
//! an origin can declare 10 MB and send 10 GB, or declare nothing.
//!
//! The transport is not in this module: [`store`] takes a `Read`, so the
//! policy (the ceilings, the digest, the cleanup) is tested with no
//! network, and the HTTP call is the caller's.

use std::io::Read;
use std::path::Path;

use pod_core::{Class, Failure, Stage};
use sha2::{Digest, Sha256};

use crate::{Result, io_at};

/// How much can be written, in bytes. `max_bytes` is the limit of the
/// caller and `fsize` is `RLIMIT_FSIZE`; both apply, and `u64::MAX` is no
/// limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ceiling {
    pub max_bytes: u64,
    pub fsize: u64,
}

impl Ceiling {
    /// The smaller of the two limits.
    pub fn effective(self) -> u64 {
        self.max_bytes.min(self.fsize)
    }
}

/// Why the store refused: the cause of the [`Failure`] of [`store`].
/// [`refusal`] finds it in a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The origin declared more than the ceiling, before the first byte was
    /// written.
    DeclaredTooLarge { declared: u64, ceiling: u64 },
    /// The bytes came past the ceiling: the origin lied, or declared no
    /// length.
    OverCeiling { ceiling: u64 },
    /// The digest is not the pinned one.
    Digest { want: String, got: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::DeclaredTooLarge { declared, ceiling } => write!(
                f,
                "the origin declares {declared} bytes, over the {ceiling}-byte ceiling, so nothing was downloaded"
            ),
            Refusal::OverCeiling { ceiling } => write!(
                f,
                "the transfer passed the {ceiling}-byte ceiling, so it was stopped and the partial file removed"
            ),
            Refusal::Digest { want, got } => write!(f, "sha256 mismatch: pinned {want}, downloaded {got}"),
        }
    }
}

impl std::error::Error for Refusal {}

impl Refusal {
    /// The failure of this refusal: [`Class::Policy`] for a ceiling,
    /// [`Class::Protocol`] for a digest.
    pub fn failure(self) -> Failure {
        let class = match self {
            Refusal::DeclaredTooLarge { .. } | Refusal::OverCeiling { .. } => Class::Policy,
            Refusal::Digest { .. } => Class::Protocol,
        };
        Failure::caused(Stage::Host, class, self.to_string(), self)
    }
}

/// The [`Refusal`] in the chain of `f`, when the store refused.
pub fn refusal(f: &Failure) -> Option<&Refusal> {
    let mut next = std::error::Error::source(f);
    while let Some(e) = next {
        if let Some(r) = e.downcast_ref::<Refusal>() {
            return Some(r);
        }
        next = e.source();
    }
    None
}

/// Refuse a declared length over the ceiling. `None` is an origin that
/// declared nothing: the check during the transfer bounds it.
pub fn check_declared(declared: Option<u64>, ceiling: Ceiling) -> Result<()> {
    let limit = ceiling.effective();
    match declared {
        Some(n) if n > limit => Err(Refusal::DeclaredTooLarge { declared: n, ceiling: limit }.failure()),
        _ => Ok(()),
    }
}

/// The name of the partial file of `dest`: the pid is in it, so that two
/// processes that store the same destination write two files. Two writers
/// in one file give a digest that matches nothing.
pub fn partial(dest: &Path) -> std::path::PathBuf {
    dest.with_file_name(format!(
        "{}.{}.part",
        dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "image".to_string()),
        std::process::id()
    ))
}

/// Write `src` to `dest` and return its sha256 in lower-case hex.
///
/// On each refusal the partial file is removed, so that no later run finds
/// a truncated image under the name of the destination. `want` is the
/// pinned digest, or `None` when the caller pinned none; the digest comes
/// back in both cases, so that the caller can record it.
pub fn store<R: Read>(
    mut src: R,
    declared: Option<u64>,
    dest: &Path,
    want: Option<&str>,
    ceiling: Ceiling,
) -> Result<String> {
    check_declared(declared, ceiling)?;
    let limit = ceiling.effective();
    let tmp = partial(dest);
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let mut out = std::fs::File::create(&tmp).map_err(|e| io_at(&tmp, e))?;
    let mut buf = vec![0u8; 1 << 20];
    let result = loop {
        let n = match src.read(&mut buf) {
            Ok(0) => break Ok(()),
            Ok(n) => n,
            Err(e) => break Err(Failure::caused(Stage::Host, Class::of_io(e.kind()), format!("read: {e}"), e)),
        };
        // `n` is at most the size of the buffer, so it fits in a u64 on
        // each target.
        written = written.saturating_add(n as u64);
        if written > limit {
            break Err(Refusal::OverCeiling { ceiling: limit }.failure());
        }
        hasher.update(&buf[..n]);
        if let Err(e) = std::io::Write::write_all(&mut out, &buf[..n]) {
            break Err(io_at(&tmp, e));
        }
    };
    // The output type of `sha2` 0.11 has no `LowerHex`, so the hex is
    // written here.
    let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    let refusal = match result {
        Err(e) => Some(e),
        Ok(()) => match want {
            Some(w) if !w.eq_ignore_ascii_case(&got) => {
                Some(Refusal::Digest { want: w.to_string(), got: got.clone() }.failure())
            }
            _ => None,
        },
    };
    drop(out);
    if let Some(e) = refusal {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, dest).map_err(|e| io_at(dest, e))?;
    Ok(got)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// The sha256 of the empty stream: the vector that matches.
    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    /// A sha256 that matches nothing: the vector of a mismatch.
    const WRONG_SHA256: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    fn ceiling(max: u64, fsize: u64) -> Ceiling {
        Ceiling { max_bytes: max, fsize }
    }

    #[test]
    fn the_effective_ceiling_is_the_smaller_of_the_two() {
        assert_eq!(ceiling(10, 4).effective(), 4);
        assert_eq!(ceiling(4, 10).effective(), 4);
        assert_eq!(ceiling(u64::MAX, u64::MAX).effective(), u64::MAX);
    }

    #[test]
    fn a_declared_length_over_the_ceiling_refuses_before_any_byte_is_written() {
        let e = check_declared(Some(1 << 30), ceiling(1 << 20, u64::MAX)).unwrap_err();
        assert!(matches!(refusal(&e), Some(Refusal::DeclaredTooLarge { .. })), "{e}");
        assert_eq!(e.class, Class::Policy);
        assert!(format!("{e}").contains("nothing was downloaded"), "{e}");
        // At the ceiling is allowed; over it is not.
        assert!(check_declared(Some(1024), ceiling(1024, u64::MAX)).is_ok());
        // The check during the transfer bounds a length that was not declared.
        assert!(check_declared(None, ceiling(1, 1)).is_ok());
    }

    #[test]
    fn an_undeclared_transfer_is_stopped_at_the_ceiling_and_the_partial_removed() {
        let dir = std::env::temp_dir().join(format!("pod-guest-fetch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("big.img");
        let body = vec![7u8; 4096];
        let e = store(Cursor::new(body), None, &dest, None, ceiling(1000, u64::MAX)).unwrap_err();
        assert_eq!(refusal(&e), Some(&Refusal::OverCeiling { ceiling: 1000 }), "{e}");
        assert!(!dest.exists(), "nothing is left at the destination");
        assert!(!partial(&dest).exists(), "and no partial either");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_size_bound_binds_even_when_the_caller_asked_for_more() {
        let dir = std::env::temp_dir().join(format!("pod-guest-fsize-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("fsize.img");
        let body = vec![0u8; 2048];
        let e = store(Cursor::new(body), None, &dest, None, ceiling(u64::MAX, 1024)).unwrap_err();
        assert_eq!(refusal(&e), Some(&Refusal::OverCeiling { ceiling: 1024 }), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_good_download_lands_and_reports_its_digest() {
        let dir = std::env::temp_dir().join(format!("pod-guest-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("ok.img");
        // The sha256 of an empty stream.
        let got = store(Cursor::new(Vec::new()), Some(0), &dest, Some(EMPTY_SHA256), ceiling(1024, 1024)).unwrap();
        assert!(got.starts_with("e3b0c442"), "{got}");
        assert_eq!(std::fs::read(&dest).unwrap().len(), 0);
        // The digest ignores the case: a pin can have either case.
        let d2 = dir.join("ok2.img");
        assert!(
            store(
                Cursor::new(b"guest".to_vec()),
                None,
                &d2,
                Some("2D7C5F0E1B1E4B1E9C0F1A2B3C4D5E6F708192A3B4C5D6E7F8091A2B3C4D5E6F"),
                ceiling(1024, 1024),
            )
            .is_err()
        );
        assert!(!d2.exists(), "a mismatched download leaves nothing behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_digest_mismatch_names_both_digests_and_removes_the_file() {
        let dir = std::env::temp_dir().join(format!("pod-guest-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("bad.img");
        let e = store(Cursor::new(b"payload".to_vec()), None, &dest, Some(WRONG_SHA256), ceiling(1 << 20, u64::MAX))
            .unwrap_err();
        assert!(matches!(refusal(&e), Some(Refusal::Digest { .. })), "{e}");
        assert_eq!(e.class, Class::Protocol);
        let msg = format!("{e}");
        assert!(msg.contains("0000") && msg.contains("sha256"), "{msg}");
        assert!(!dest.exists());
        assert!(!partial(&dest).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_fetches_of_one_destination_do_not_share_a_partial_file() {
        // The name has the pid, so the file of this process is not the
        // file of another process.
        let dest = Path::new("/tmp/x/base.vhdx");
        let p = partial(dest);
        assert!(p.to_string_lossy().ends_with(".part"));
        assert!(p.to_string_lossy().contains(&std::process::id().to_string()), "{p:?}");
        assert_ne!(p, dest.with_extension("part"), "not the shared fixed name");
    }
}

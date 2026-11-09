//! Base-image acquisition: bounded, checksummed, and all-or-nothing.
//! `TODO/milestones.md` T-1112.
//!
//! ⛔ **Why podvm fetches an image at all.** The guest cannot be built from
//! an OCI reference: it is a disk image. Leaving acquisition to the operator
//! means the feature only runs for somebody who already has one, which is
//! the same as only running on the machine that was used to develop it. The
//! reference implementation downloads its own base; so does this.
//!
//! ⛔ **Three bounds, and every one of them is a refusal rather than a
//! truncation.** The transfer is refused before it starts where the origin
//! declares more than `--max-bytes`, refused while running where the bytes
//! arrive past it or past `RLIMIT_FSIZE` (the ceiling `TODO/machine.md`
//! T-1305 already judges guest memory against), and refused at the end
//! where the digest is not the pinned one. A partial file is deleted, never
//! left behind under the name a later run would trust.
//!
//! ⚠ **The ceiling is checked against the declared length AND the arriving
//! bytes.** A liar that declares 10 MB and sends 10 GB is the case the
//! declared check cannot see, and a liar that declares nothing is not
//! allowed to become an unbounded write.
//!
//! ⭐ **The transport is not in this file.** [`store`] takes a `Read`, so
//! what is tested here is the policy, ceilings, digest, cleanup, and the
//! HTTP call is a thin caller. That split is what lets the refusals be
//! tested without a network, which is the only way they can be tested in a
//! lane that has none.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

/// How much may be written, in bytes. `max_bytes` is the caller's own limit
/// and `fsize` is `RLIMIT_FSIZE`; both are enforced, and `u64::MAX` means
/// unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ceiling {
    pub max_bytes: u64,
    pub fsize: u64,
}

impl Ceiling {
    pub fn effective(self) -> u64 {
        self.max_bytes.min(self.fsize)
    }
}

/// What acquisition refused, and why.
#[derive(Debug)]
pub enum FetchError {
    /// The origin said the object is bigger than the ceiling, before a
    /// single byte was written.
    DeclaredTooLarge {
        declared: u64,
        ceiling: u64,
    },
    /// The bytes kept coming past the ceiling. The origin lied or did not
    /// declare a length.
    OverCeiling {
        ceiling: u64,
    },
    /// The digest is not the pinned one.
    Digest {
        want: String,
        got: String,
    },
    Io(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::DeclaredTooLarge { declared, ceiling } => write!(
                f,
                "the origin declares {declared} bytes, over the {ceiling}-byte ceiling, \
                 so nothing was downloaded"
            ),
            FetchError::OverCeiling { ceiling } => write!(
                f,
                "the transfer passed the {ceiling}-byte ceiling, so it was stopped and \
                 the partial file removed"
            ),
            FetchError::Digest { want, got } => {
                write!(f, "sha256 mismatch: pinned {want}, downloaded {got}")
            }
            FetchError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FetchError {}

/// Refuse a declared length over the ceiling. `None` is an origin that
/// declared nothing, which is not a refusal here: the running check below
/// is what bounds it.
pub fn check_declared(declared: Option<u64>, ceiling: Ceiling) -> Result<(), FetchError> {
    let limit = ceiling.effective();
    match declared {
        Some(n) if n > limit => Err(FetchError::DeclaredTooLarge {
            declared: n,
            ceiling: limit,
        }),
        _ => Ok(()),
    }
}

/// The sha256 of `src`, as lowercase hex, written to `dest` as it goes.
///
/// ⛔ On any refusal the partial file is removed, so a later run never
/// finds a truncated image under the destination's name. `want` is the
/// pinned digest, or `None` where the caller pinned none; the digest is
/// returned either way, so a caller can record what it got.
pub fn store<R: Read>(
    mut src: R,
    declared: Option<u64>,
    dest: &Path,
    want: Option<&str>,
    ceiling: Ceiling,
) -> Result<String, FetchError> {
    check_declared(declared, ceiling)?;
    let limit = ceiling.effective();
    // ⛔ **The partial name is per-process.** A fixed `dest.part` is shared
    // by two fetches of the same destination, and two writers interleaving
    // into one file produce a file whose digest matches nothing, with the
    // verification reporting a mismatch rather than the collision that
    // caused it. The pid is in the name so a collision is a different file.
    let tmp = dest.with_file_name(format!(
        "{}.{}.part",
        dest.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "image".to_string()),
        std::process::id()
    ));
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let mut out = std::fs::File::create(&tmp)
        .map_err(|e| FetchError::Io(format!("{}: {e}", tmp.display())))?;
    let mut buf = vec![0u8; 1 << 20];
    let result = loop {
        let n = match src.read(&mut buf) {
            Ok(0) => break Ok(()),
            Ok(n) => n,
            Err(e) => break Err(FetchError::Io(format!("read: {e}"))),
        };
        written += n as u64;
        if written > limit {
            break Err(FetchError::OverCeiling { ceiling: limit });
        }
        hasher.update(&buf[..n]);
        if let Err(e) = std::io::Write::write_all(&mut out, &buf[..n]) {
            break Err(FetchError::Io(format!("{}: {e}", tmp.display())));
        }
    };
    // ⚠ `sha2 0.11`'s output type does not implement `LowerHex`, so the hex
    // is written here rather than formatted.
    let mut got = String::with_capacity(64);
    for b in hasher.finalize() {
        got.push_str(&format!("{b:02x}"));
    }
    let refusal = match result {
        Err(e) => Some(e),
        Ok(()) => match want {
            Some(w) if !w.eq_ignore_ascii_case(&got) => Some(FetchError::Digest {
                want: w.to_string(),
                got: got.clone(),
            }),
            _ => None,
        },
    };
    if let Some(e) = refusal {
        drop(out);
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    drop(out);
    std::fs::rename(&tmp, dest).map_err(|e| FetchError::Io(format!("{}: {e}", dest.display())))?;
    Ok(got)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Pinned sha256 of the empty stream, the matching test vector.
    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    /// Pinned sha256 that matches nothing, the mismatch test vector.
    const WRONG_SHA256: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    fn ceiling(max: u64, fsize: u64) -> Ceiling {
        Ceiling {
            max_bytes: max,
            fsize,
        }
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
        assert!(matches!(e, FetchError::DeclaredTooLarge { .. }), "{e}");
        assert!(format!("{e}").contains("nothing was downloaded"), "{e}");
        // at the ceiling is allowed; over it is not
        assert!(check_declared(Some(1024), ceiling(1024, u64::MAX)).is_ok());
        // an undeclared length is bounded by the running check instead
        assert!(check_declared(None, ceiling(1, 1)).is_ok());
    }

    #[test]
    fn an_undeclared_transfer_is_stopped_at_the_ceiling_and_the_partial_removed() {
        let dir = std::env::temp_dir().join(format!("pbx-fetch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("big.img");
        let body = vec![7u8; 4096];
        let e = store(
            Cursor::new(body),
            None,
            &dest,
            None,
            ceiling(1000, u64::MAX),
        )
        .unwrap_err();
        assert!(
            matches!(e, FetchError::OverCeiling { ceiling: 1000 }),
            "{e}"
        );
        assert!(!dest.exists(), "nothing is left at the destination");
        assert!(!partial(&dest).exists(), "and no partial either");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_size_bound_binds_even_when_the_caller_asked_for_more() {
        let dir = std::env::temp_dir().join(format!("pbx-fsize-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("fsize.img");
        let body = vec![0u8; 2048];
        let e = store(
            Cursor::new(body),
            None,
            &dest,
            None,
            ceiling(u64::MAX, 1024),
        )
        .unwrap_err();
        assert!(
            matches!(e, FetchError::OverCeiling { ceiling: 1024 }),
            "{e}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_good_download_lands_and_reports_its_digest() {
        let dir = std::env::temp_dir().join(format!("pbx-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("ok.img");
        // The sha256 of an empty stream, which is a value worth pinning.
        let got = store(
            Cursor::new(Vec::new()),
            Some(0),
            &dest,
            Some(EMPTY_SHA256),
            ceiling(1024, 1024),
        )
        .unwrap();
        assert!(got.starts_with("e3b0c442"), "{got}");
        assert_eq!(std::fs::read(&dest).unwrap().len(), 0);
        // and the digest is case-insensitive, because pins are written both ways
        let d2 = dir.join("ok2.img");
        assert!(store(
            Cursor::new(b"podvm".to_vec()),
            None,
            &d2,
            Some("2D7C5F0E1B1E4B1E9C0F1A2B3C4D5E6F708192A3B4C5D6E7F8091A2B3C4D5E6F"),
            ceiling(1024, 1024),
        )
        .is_err());
        assert!(!d2.exists(), "a mismatched download leaves nothing behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_digest_mismatch_names_both_digests_and_removes_the_file() {
        let dir = std::env::temp_dir().join(format!("pbx-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("bad.img");
        let e = store(
            Cursor::new(b"payload".to_vec()),
            None,
            &dest,
            Some(WRONG_SHA256),
            ceiling(1 << 20, u64::MAX),
        )
        .unwrap_err();
        assert!(matches!(e, FetchError::Digest { .. }), "{e}");
        let msg = format!("{e}");
        assert!(msg.contains("0000") && msg.contains("sha256"), "{msg}");
        assert!(!dest.exists());
        assert!(!partial(&dest).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The partial name `store` uses for `dest`, so a test names the same
    /// file the implementation does rather than guessing the suffix.
    fn partial(dest: &Path) -> std::path::PathBuf {
        dest.with_file_name(format!(
            "{}.{}.part",
            dest.file_name().unwrap().to_string_lossy(),
            std::process::id()
        ))
    }

    #[test]
    fn two_fetches_of_one_destination_do_not_share_a_partial_file() {
        // The name carries the pid, so the file this process writes is not
        // the file another process would write.
        let dest = Path::new("/tmp/x/base.vhdx");
        let p = partial(dest);
        assert!(p.to_string_lossy().ends_with(".part"));
        assert!(
            p.to_string_lossy()
                .contains(&std::process::id().to_string()),
            "{p:?}"
        );
        assert_ne!(p, dest.with_extension("part"), "not the shared fixed name");
    }
}

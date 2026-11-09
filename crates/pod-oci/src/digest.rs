//! `TODO/image.md` T-0202: content addressing, and verification **as bytes are
//! written** rather than afterwards.
//!
//! ⭐ The rule is read out of the corpus at
//! `references/qaidvoid__onelf/tree/crates/onelf-rt/src/main.rs:149-157`, which
//! checks a payload's content hash before an in-memory exec so that an
//! in-memory exec never runs unverified bytes. A layer about to be extracted as
//! root is the same case.
//!
//! ⛔ A digest is an opaque token, never a value re-parsed out of a mutable
//! name (`docs/conventions/code.md`). [`Digest`] owns the parsing, and every
//! path that turns one into a filename goes through [`Digest::blob_path`].

use std::fmt;
use std::io::Write;

use sha2::{Digest as _, Sha256};

use crate::error::{Error, Result};

/// The one algorithm podvm writes. ⚠ The type carries the algorithm anyway,
/// so a registry serving `sha512:` is refused by name rather than silently
/// hashed with the wrong function.
pub const SHA256: &str = "sha256";

/// Lower-case hex for a digest's own bytes.
///
/// ⛔ Written here rather than taken from the hash crate's `LowerHex`. That
/// trait is implemented on the crate's output type, and `sha2` 0.11 changed
/// that type from `generic-array` to `hybrid-array`, which does not carry it.
/// A formatting call against a dependency's type is a compile error on a
/// version bump; a function over `&[u8]` is not.
pub(crate) fn hex_of(bytes: &[u8]) -> String {
    use fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // ⚠ `write!` into a `String` cannot fail, and the result is read so
        // that a later refactor cannot turn this into an ignored one.
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// `<algorithm>:<hex>`, parsed once and carried whole.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Digest {
    algorithm: String,
    hex: String,
}

impl Digest {
    /// Parse `<algorithm>:<hex>`. ⛔ The shape is validated before it is
    /// trusted: an unvalidated digest reaches the filesystem as a path
    /// component, so `..` or a slash in it is a store escape.
    pub fn parse(s: &str) -> Result<Digest> {
        let Some((algorithm, hex)) = s.split_once(':') else {
            return Err(Error::Reference(format!(
                "{s:?} is not a digest: it has no <algorithm>:<hex> separator"
            )));
        };
        if algorithm != SHA256 {
            return Err(Error::Oci(format!(
                "digest algorithm {algorithm:?} is not supported; podvm writes \
                 {SHA256} and refuses to store bytes it cannot verify"
            )));
        }
        // ⚠ Lowercase hex, and the two halves of that are spelled out: an
        // `is_ascii_lowercase() && is_ascii_hexdigit()` reads correctly and
        // rejects every digit, because `0`-`9` are not lowercase LETTERS. It
        // was written that way here first and every real digest was refused.
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Reference(format!(
                "{s:?} is not a digest: {SHA256} needs 64 lowercase hex digits, \
                 and this has {} character(s)",
                hex.len()
            )));
        }
        Ok(Digest {
            algorithm: algorithm.to_string(),
            hex: hex.to_string(),
        })
    }

    /// The digest of a byte slice already in memory. Used for manifests, which
    /// a registry caps at a few megabytes; blobs go through [`Verifier`].
    pub fn of(bytes: &[u8]) -> Digest {
        let mut h = Sha256::new();
        h.update(bytes);
        Digest {
            algorithm: SHA256.to_string(),
            hex: hex_of(&h.finalize()),
        }
    }

    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }

    pub fn hex(&self) -> &str {
        &self.hex
    }

    /// docker's image ID display form: the first twelve hex digits.
    pub fn short(&self) -> &str {
        &self.hex[..12]
    }

    /// `blobs/<algorithm>/<hex>`, relative to the store root. ⚠ Both components
    /// were validated by [`Digest::parse`] or produced by [`Digest::of`], so
    /// neither can carry a separator.
    pub fn blob_path(&self) -> String {
        format!("blobs/{}/{}", self.algorithm, self.hex)
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm, self.hex)
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

/// A sink that hashes and counts everything written through it.
///
/// ⛔ Both, and the count is not the declared length. `docs/conventions/forbidden-patterns.md`:
/// trusting a declared length instead of counting what actually arrived
/// records a truncated object as complete.
pub struct Verifier<W: Write> {
    inner: W,
    hasher: Sha256,
    written: u64,
}

impl<W: Write> Verifier<W> {
    pub fn new(inner: W) -> Verifier<W> {
        Verifier {
            inner,
            hasher: Sha256::new(),
            written: 0,
        }
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    pub fn digest(&self) -> Digest {
        Digest {
            algorithm: SHA256.to_string(),
            hex: hex_of(&self.hasher.clone().finalize()),
        }
    }

    /// Consume the sink and assert both halves of the descriptor.
    ///
    /// ⛔ The size is checked as well as the digest. A registry that serves
    /// fewer bytes than the descriptor declares and happens to collide is not
    /// the threat; a descriptor whose size field is wrong is a store whose
    /// space precheck (T-0203) was computed against a lie.
    pub fn finish(self, what: &str, want: &Digest, want_size: Option<u64>) -> Result<W> {
        let got = Digest {
            algorithm: SHA256.to_string(),
            hex: hex_of(&self.hasher.finalize()),
        };
        if got != *want {
            return Err(Error::DigestMismatch {
                what: what.to_string(),
                want: want.to_string(),
                got: got.to_string(),
            });
        }
        if let Some(size) = want_size
            && size != self.written
        {
            return Err(Error::Http {
                what: what.to_string(),
                detail: format!(
                    "the descriptor declares {size} byte(s) and {} arrived",
                    self.written
                ),
            });
        }
        Ok(self.inner)
    }
}

impl<W: Write> Write for Verifier<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        // ⛔ Hash exactly what reached the sink, not what was offered. A short
        // write that hashed the whole buffer would verify bytes nobody stored.
        self.hasher.update(&buf[..n]);
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_sha256_is_the_known_value() {
        assert_eq!(
            Digest::of(b"").to_string(),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_digest_carrying_a_path_separator_is_refused() {
        // ⛔ The store-escape case. A digest reaches the filesystem as a path
        // component, so this is a containment check and not a tidiness one.
        for bad in [
            "sha256:../../etc/passwd",
            "sha256:a/b",
            "sha256:",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "sha256:E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
        ] {
            assert!(Digest::parse(bad).is_err(), "{bad} parsed");
        }
    }

    #[test]
    fn an_unsupported_algorithm_is_named_rather_than_hashed_anyway() {
        let e = Digest::parse(&format!("sha512:{}", "a".repeat(64))).unwrap_err();
        assert!(format!("{e}").contains("sha512"), "{e}");
    }

    #[test]
    fn a_verifier_rejects_bytes_that_do_not_match_and_names_both() {
        let mut v = Verifier::new(Vec::new());
        v.write_all(b"not alpine").unwrap();
        let want = Digest::of(b"alpine");
        let e = v.finish("layer", &want, None).unwrap_err();
        let text = format!("{e}");
        assert!(text.contains(&want.to_string()), "{text}");
        assert!(text.contains("computed sha256:"), "{text}");
    }

    #[test]
    fn a_verifier_rejects_a_short_body_whose_digest_would_never_be_reached() {
        let body = b"alpine";
        let mut v = Verifier::new(Vec::new());
        v.write_all(body).unwrap();
        let e = v.finish("layer", &Digest::of(body), Some(999)).unwrap_err();
        assert!(format!("{e}").contains("999"), "{e}");
    }

    #[test]
    fn a_verifier_accepts_the_bytes_it_was_given() {
        let body = b"alpine";
        let mut v = Verifier::new(Vec::new());
        v.write_all(body).unwrap();
        let out = v
            .finish("layer", &Digest::of(body), Some(body.len() as u64))
            .unwrap();
        assert_eq!(out, body);
    }

    #[test]
    fn the_short_form_is_dockers_twelve_digits() {
        let d = Digest::of(b"");
        assert_eq!(d.short(), "e3b0c44298fc");
        assert_eq!(d.blob_path(), format!("blobs/sha256/{}", d.hex()));
    }
}

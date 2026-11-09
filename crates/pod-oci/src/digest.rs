//! Content addressing, and the check of a digest while the bytes are
//! written, not after.
//!
//! A layer that is about to be extracted is checked before any use: the
//! bytes reach their final name only when the digest and the size agree.
//!
//! A digest is an opaque token. [`Digest`] owns its parse, and each path
//! that makes a file name from a digest goes through [`Digest::blob_path`].

use std::fmt;
use std::io::Write;

use sha2::{Digest as _, Sha256};

use crate::error::{Class, Refusal, Result, registry, usage};

/// The one algorithm of the crate. The type carries the algorithm all the
/// same, so a registry that serves `sha512:` gets a refusal by name and no
/// hash with the wrong function.
pub const SHA256: &str = "sha256";

/// Lower-case hex for the bytes of a digest.
///
/// The hash crate's `LowerHex` is not used: `sha2` 0.11 changed its output
/// type from `generic-array` to `hybrid-array`, which does not have it. A
/// function over `&[u8]` does not break on a version change.
pub(crate) fn hex_of(bytes: &[u8]) -> String {
    use fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // `write!` into a `String` cannot fail.
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
    /// Parse `<algorithm>:<hex>`. The shape is checked before any use: a
    /// digest becomes a path component, so `..` or a slash in it would
    /// escape the store. A bad shape is `Class::Usage`; an algorithm other
    /// than sha256 is `Class::Unsupported`.
    pub fn parse(s: &str) -> Result<Digest> {
        let Some((algorithm, hex)) = s.split_once(':') else {
            return Err(usage(format!(
                "invalid reference: {s:?} is not a digest: it has no <algorithm>:<hex> separator"
            )));
        };
        if algorithm != SHA256 {
            return Err(registry(
                Class::Unsupported,
                format!(
                    "digest algorithm {algorithm:?} is not supported; the client writes \
                     {SHA256} and refuses to store bytes it cannot verify"
                ),
            ));
        }
        // Lower-case hex: a digit or a letter from a to f. A test of
        // `is_ascii_lowercase() && is_ascii_hexdigit()` refuses each digit,
        // as a digit is not a lower-case letter.
        if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err(usage(format!(
                "invalid reference: {s:?} is not a digest: {SHA256} needs 64 lowercase hex digits, \
                 and this has {} character(s)",
                hex.len()
            )));
        }
        Ok(Digest { algorithm: algorithm.to_string(), hex: hex.to_string() })
    }

    /// The digest of a byte slice already in memory. Used for manifests, which
    /// a registry caps at a few megabytes; blobs go through [`Verifier`].
    pub fn of(bytes: &[u8]) -> Digest {
        let mut h = Sha256::new();
        h.update(bytes);
        Digest { algorithm: SHA256.to_string(), hex: hex_of(&h.finalize()) }
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

    /// `blobs/<algorithm>/<hex>`, relative to the root of a store or of an
    /// OCI layout. [`Digest::parse`] checked both parts, or [`Digest::of`]
    /// made them, so neither holds a separator.
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

/// A sink that hashes and counts each byte written through it.
///
/// The count is of the bytes that arrived, not the declared length: a
/// declared length would record a cut object as complete.
pub struct Verifier<W: Write> {
    inner: W,
    hasher: Sha256,
    written: u64,
}

impl<W: Write> Verifier<W> {
    pub fn new(inner: W) -> Verifier<W> {
        Verifier { inner, hasher: Sha256::new(), written: 0 }
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    pub fn digest(&self) -> Digest {
        Digest { algorithm: SHA256.to_string(), hex: hex_of(&self.hasher.clone().finalize()) }
    }

    /// Consume the sink and check both halves of the descriptor.
    ///
    /// The size is checked as well as the digest: a descriptor with a wrong
    /// size makes a wrong check of the free space before a pull. A wrong
    /// digest is [`Refusal::Digest`]; a wrong size is `Class::Protocol`.
    pub fn finish(self, what: &str, want: &Digest, want_size: Option<u64>) -> Result<W> {
        let got = Digest { algorithm: SHA256.to_string(), hex: hex_of(&self.hasher.finalize()) };
        if got != *want {
            return Err(
                Refusal::Digest { what: what.to_string(), want: want.to_string(), got: got.to_string() }.failure()
            );
        }
        if let Some(size) = want_size
            && size != self.written
        {
            return Err(registry(
                Class::Protocol,
                format!("{what}: the descriptor declares {size} byte(s) and {} arrived", self.written),
            ));
        }
        Ok(self.inner)
    }
}

impl<W: Write> Write for Verifier<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        // Hash only what reached the sink. A short write that hashed the
        // whole buffer would check bytes that nobody stored.
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
        // A digest becomes a path component: this is a check of the
        // containment.
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
        assert_eq!(e.class, Class::Unsupported);
        assert_eq!(Digest::parse("sha256:a/b").unwrap_err().class, Class::Usage);
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
        assert!(matches!(crate::refusal(&e), Some(Refusal::Digest { .. })), "{text}");
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
        let out = v.finish("layer", &Digest::of(body), Some(body.len() as u64)).unwrap();
        assert_eq!(out, body);
    }

    #[test]
    fn the_short_form_is_dockers_twelve_digits() {
        let d = Digest::of(b"");
        assert_eq!(d.short(), "e3b0c44298fc");
        assert_eq!(d.blob_path(), format!("blobs/sha256/{}", d.hex()));
    }
}

//! The verifier of a release: the Sigstore bundle of `SHA256SUMS` by the
//! caller's identity first, then each file against `SHA256SUMS`, then the
//! index against `SHA256SUMS`. Nothing of a release is trusted before its
//! bundle verifies: a file, a sum or an index that a mirror changed fails
//! one of the three.

use std::path::Path;

use crate::index::{self, Index};
use crate::sigstore::{self, Identity, Logged, Roots};
use crate::sums::Sums;
use crate::{Error, hex};

/// The name of the sums in a release.
pub const SUMS: &str = "SHA256SUMS";
/// The name of the bundle of the sums in a release.
pub const BUNDLE: &str = "SHA256SUMS.sigstore.json";

/// `SHA256SUMS` of a release, after its bundle verified.
#[derive(Debug, Clone)]
pub struct Verified {
    pub sums: Sums,
    pub logged: Logged,
    /// The SHA-256 of `SHA256SUMS`.
    pub digest: [u8; 32],
}

/// `sums` (the bytes of `SHA256SUMS`) with the bundle `bundle`, signed as
/// `who` by `roots`.
pub fn verify_release(sums: &[u8], bundle: &[u8], who: &Identity, roots: &Roots) -> Result<Verified, Error> {
    let logged = sigstore::verify_with(bundle, sums, who, roots)
        .map_err(|why| Error::Refused(format!("the signature of {SUMS}: {why}")))?;
    let parsed = Sums::parse(sums).map_err(Error::Refused)?;
    Ok(Verified { sums: parsed, logged, digest: crate::sha256(sums) })
}

impl Verified {
    /// `bytes` are the file `name` of the release.
    pub fn check(&self, name: &str, bytes: &[u8]) -> Result<(), Error> {
        self.sums.check(name, bytes).map_err(Error::Refused)
    }

    /// `text` is the index of the release: its line in `SHA256SUMS`, its
    /// rules, and each of its files with the sum of `SHA256SUMS`.
    pub fn index(&self, text: &[u8]) -> Result<Index, Error> {
        self.check(index::FILE, text)?;
        let text = std::str::from_utf8(text).map_err(|_| Error::Refused("the release index is not text".into()))?;
        let index = index::read(text).map_err(Error::Refused)?;
        index.agrees(&self.sums).map_err(Error::Refused)?;
        Ok(index)
    }
}

/// What [`verify_dir`] checked.
#[derive(Debug, Clone)]
pub struct Checked {
    pub verified: Verified,
    /// Each file of `SHA256SUMS` that is in the directory, checked.
    pub files: Vec<String>,
    /// Each file of `SHA256SUMS` that is not in the directory.
    pub missing: Vec<String>,
    /// The index, when `SHA256SUMS` names one.
    pub index: Option<Index>,
}

/// The release in the directory `dir`: `SHA256SUMS` and its bundle there,
/// signed as `who`; then each file that `SHA256SUMS` names and `dir` holds;
/// then the index, when `SHA256SUMS` names one. A name that is not a plain
/// file name is refused before a file is opened. The caller decides what a
/// missing file means: an installer downloads the files of its target only.
pub fn verify_dir(dir: &Path, who: &Identity, roots: &Roots) -> Result<Checked, Error> {
    let read = |name: &str| {
        std::fs::read(dir.join(name)).map_err(|e| Error::Unavailable(format!("{}: {e}", dir.join(name).display())))
    };
    let verified = verify_release(&read(SUMS)?, &read(BUNDLE)?, who, roots)?;
    let (mut files, mut missing, mut index) = (Vec::new(), Vec::new(), None);
    for name in verified.sums.names() {
        if !index::plain_name(name) {
            return Err(Error::Refused(format!("{SUMS} names {name:?}, which is not a file name")));
        }
        let path = dir.join(name);
        if !path.is_file() {
            missing.push(name.to_string());
            continue;
        }
        let bytes = read(name)?;
        if name == index::FILE {
            index = Some(verified.index(&bytes)?);
        } else {
            verified.check(name, &bytes)?;
        }
        files.push(name.to_string());
    }
    Ok(Checked { verified, files, missing, index })
}

impl std::fmt::Display for Checked {
    /// One line: the signer's log entry, and the counts.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{SUMS} {}: logged by Rekor at {} (entry {}); {} files checked, {} missing",
            hex(&self.verified.digest),
            self.verified.logged.time,
            self.verified.logged.index,
            self.files.len(),
            self.missing.len()
        )
    }
}

#[cfg(test)]
mod tests;

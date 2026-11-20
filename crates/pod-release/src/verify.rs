//! The verifier of a release: the Sigstore bundle of `SHA256SUMS` by the
//! caller's identity first, then each file against `SHA256SUMS`, then the
//! index against `SHA256SUMS` and the identity. Nothing of a release is
//! trusted before its bundle verifies: a file, a sum or an index that a
//! mirror changed fails one of the three.

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
        index_of(&self.sums, text)
    }
}

/// `text`, the index, against `sums`: its line, its rules, its files.
fn index_of(sums: &Sums, text: &[u8]) -> Result<Index, Error> {
    sums.check(index::FILE, text).map_err(Error::Refused)?;
    let text = std::str::from_utf8(text).map_err(|_| Error::Refused("the release index is not text".into()))?;
    let index = index::read(text).map_err(Error::Refused)?;
    index.agrees(sums).map_err(Error::Refused)?;
    Ok(index)
}

/// Which files of `SHA256SUMS` a check of a directory reads. Each choice
/// reads the index when the directory holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which<'a> {
    /// Each file that the directory holds; the others are missing. An
    /// installer downloads the files of its target only.
    Present,
    /// Each file: a missing one is [`Error::NotFound`].
    All,
    /// These names: each must be in `SHA256SUMS` and in the directory.
    Named(&'a [String]),
}

/// The files of a directory that a check read.
#[derive(Debug, Clone, Default)]
pub struct Found {
    /// Each file that the check read, in the order of `SHA256SUMS`.
    pub files: Vec<String>,
    /// Each file of `SHA256SUMS` that the directory does not hold, of the
    /// files that the check selects.
    pub missing: Vec<String>,
    /// The index, when the directory holds it.
    pub index: Option<Index>,
}

/// The bytes of the file `path`: a missing file is [`Error::NotFound`],
/// another failure [`Error::Unavailable`].
pub fn read_file(path: &Path) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|e| {
        let why = format!("{}: {e}", path.display());
        if e.kind() == std::io::ErrorKind::NotFound { Error::NotFound(why) } else { Error::Unavailable(why) }
    })
}

fn read(dir: &Path, name: &str) -> Result<Vec<u8>, Error> {
    read_file(&dir.join(name))
}

/// The files of `dir` that `which` selects, against `sums` (verified by the
/// caller, or taken as they are by a check before the signature). A name
/// that is not a plain file name is refused before a file is opened. An
/// index in `dir` that `sums` does not name is refused: nothing signs it.
pub fn check_files(dir: &Path, sums: &Sums, which: Which<'_>) -> Result<Found, Error> {
    let selects = |name: &str| match which {
        Which::Named(list) => list.iter().any(|n| n == name),
        Which::Present | Which::All => true,
    };
    if let Which::Named(list) = which {
        for name in list {
            if !index::plain_name(name) {
                return Err(Error::Refused(format!("{name:?} is not a file name")));
            }
            sums.get(name).map_err(Error::Refused)?;
        }
    }
    let names = sums.names();
    if !names.contains(&index::FILE) && dir.join(index::FILE).exists() {
        return Err(Error::Refused(format!("the directory holds {}, which {SUMS} does not name", index::FILE)));
    }
    let mut found = Found::default();
    for name in names {
        if !index::plain_name(name) {
            return Err(Error::Refused(format!("{SUMS} names {name:?}, which is not a file name")));
        }
        let is_index = name == index::FILE;
        if !is_index && !selects(name) {
            continue;
        }
        // A file that the caller asks for must be there; the others can be
        // missing.
        let asked = which == Which::All || matches!(which, Which::Named(_)) && selects(name);
        if !asked && !dir.join(name).is_file() {
            found.missing.push(name.to_string());
            continue;
        }
        let bytes = read(dir, name)?;
        if is_index {
            found.index = Some(index_of(sums, &bytes)?);
        } else {
            sums.check(name, &bytes).map_err(Error::Refused)?;
        }
        found.files.push(name.to_string());
    }
    Ok(found)
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
/// then the index, when `SHA256SUMS` names one. The caller decides what a
/// missing file means: an installer downloads the files of its target only.
pub fn verify_dir(dir: &Path, who: &Identity, roots: &Roots) -> Result<Checked, Error> {
    verify_files(dir, who, roots, Which::Present)
}

/// [`verify_dir`] of the files that `which` selects. The index must name
/// the signature of `who` ([`Index::signed_as`]).
pub fn verify_files(dir: &Path, who: &Identity, roots: &Roots, which: Which<'_>) -> Result<Checked, Error> {
    let verified = verify_release(&read(dir, SUMS)?, &read(dir, BUNDLE)?, who, roots)?;
    let found = check_files(dir, &verified.sums, which)?;
    if let Some(index) = &found.index {
        index.signed_as(who).map_err(Error::Refused)?;
    }
    Ok(Checked { verified, files: found.files, missing: found.missing, index: found.index })
}

impl std::fmt::Display for Checked {
    /// One line: the signer's log entry, and the counts.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{SUMS} {}: logged by Rekor at {} (entry {}); {} files checked, {} missing",
            hex(&self.verified.digest),
            crate::utc(self.verified.logged.time),
            self.verified.logged.index,
            self.files.len(),
            self.missing.len()
        )
    }
}

#[cfg(test)]
mod tests;

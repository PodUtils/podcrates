//! The release index `release-index.json`: one JSON document in each
//! release of each product, with the version, the date, the SHA-256 and the
//! size of each file and its target, where the signature is and whose it
//! is, the SBOM and the acceptance. `scripts/release-index.py` writes it in
//! CI before `SHA256SUMS`, and `SHA256SUMS` holds its line: the signature
//! of `SHA256SUMS` signs the index too. Its schema is
//! `schemas/release-index.v1.schema.json`, generated from these types.
//!
//! A reader refuses a field that it does not know, as each schema of
//! `schemas/` does (`additionalProperties: false`, the rule of T-177): a
//! reader of an index must read each field that it checks.

use serde::{Deserialize, Serialize};

use crate::sums::{Sums, digest_of};

/// The version of the form that [`Index::schema`] carries.
pub const SCHEMA: u32 = 1;

/// The name of the index among the files of a release.
pub const FILE: &str = "release-index.json";

/// The release index of one release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
pub struct Index {
    /// The version of this form: 1.
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "schema::version"))]
    pub schema: u32,
    /// The repository of the product, `OWNER/REPO`.
    #[cfg_attr(any(test, feature = "schema"), schemars(regex(pattern = r"^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$")))]
    pub project: String,
    /// The tag of the release, as `v0.77.0`.
    #[cfg_attr(any(test, feature = "schema"), schemars(regex(pattern = r"^[v0-9][0-9A-Za-z.+-]{0,63}$")))]
    pub version: String,
    /// The time of the release in UTC, `YYYY-MM-DDTHH:MM:SSZ`.
    #[cfg_attr(
        any(test, feature = "schema"),
        schemars(regex(pattern = r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"))
    )]
    pub date: String,
    /// The commit of the tag: 40 lowercase hex digits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(any(test, feature = "schema"), schemars(regex(pattern = r"^[0-9a-f]{40}$")))]
    pub commit: Option<String>,
    /// Where the signature is, and whose it must be.
    pub signature: Signature,
    /// Each file of the release, except `SHA256SUMS`, its bundle and the
    /// index, sorted by name.
    pub files: Vec<File>,
    /// The SBOM: the name of a file of `files`, or an `https://` URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sbom: Option<String>,
    /// The acceptance of the release (the asks that it passes): the name
    /// of a file of `files`, or an `https://` URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<String>,
}

/// The keyless signature of `SHA256SUMS`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
pub struct Signature {
    /// The file of the SHA-256 sums: `SHA256SUMS`.
    pub sums: String,
    /// The Sigstore bundle of `sums`: `SHA256SUMS.sigstore.json`.
    pub bundle: String,
    /// The URI that the leaf certificate must name: the release workflow
    /// at the tag.
    pub identity: String,
    /// The issuer of the OIDC token of the signer.
    pub issuer: String,
}

/// One file of the release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
pub struct File {
    /// The name of the file: no `/`, no `\`, not `.` or `..`.
    pub name: String,
    /// Its SHA-256: 64 lowercase hex digits.
    #[cfg_attr(any(test, feature = "schema"), schemars(regex(pattern = r"^[0-9a-f]{64}$")))]
    pub sha256: String,
    /// Its size in bytes.
    pub size: u64,
    /// The target triple of a binary, as `x86_64-unknown-linux-musl`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// A name of one file in a directory: not empty, no separator, not `.` or
/// `..`, no control character.
pub fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
}

/// The text of an index, read and checked ([`Index::check`]).
pub fn read(text: &str) -> Result<Index, String> {
    let index: Index = serde_json::from_str(text).map_err(|e| format!("the release index: {e}"))?;
    index.check()?;
    Ok(index)
}

impl Index {
    /// The rules that no type holds: the schema version, the forms of the
    /// tag, the date, the commit and each SHA-256, unique plain names, and
    /// an SBOM or an acceptance that is a file of the release or an
    /// `https://` URL.
    pub fn check(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!("the release index has the schema {}, not {SCHEMA}", self.schema));
        }
        let project = self.project.split('/').collect::<Vec<_>>();
        let part =
            |p: &&str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if project.len() != 2 || !project.iter().all(part) {
            return Err(format!("the project {:?} is not OWNER/REPO", self.project));
        }
        if !crate::version::valid_tag(&self.version) {
            return Err(format!("the version {:?} is not a tag", self.version));
        }
        if !utc_date(&self.date) {
            return Err(format!("the date {:?} is not YYYY-MM-DDTHH:MM:SSZ", self.date));
        }
        if let Some(commit) = &self.commit
            && (commit.len() != 40 || !commit.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        {
            return Err(format!("the commit {commit:?} is not 40 lowercase hex digits"));
        }
        let own = [self.signature.sums.as_str(), self.signature.bundle.as_str(), FILE];
        for name in [&self.signature.sums, &self.signature.bundle] {
            if !plain_name(name) {
                return Err(format!("the signature names {name:?}, not a file name"));
            }
        }
        let mut seen: Vec<&str> = Vec::new();
        for file in &self.files {
            if !plain_name(&file.name) || own.contains(&file.name.as_str()) || seen.contains(&file.name.as_str()) {
                return Err(format!("the file name {:?} is not a plain name, or comes twice", file.name));
            }
            if file.sha256.bytes().any(|b| b.is_ascii_uppercase()) || digest_of(&file.sha256).is_none() {
                return Err(format!("the SHA-256 of {} is not 64 lowercase hex digits", file.name));
            }
            seen.push(&file.name);
        }
        for (what, value) in [("sbom", &self.sbom), ("acceptance", &self.acceptance)] {
            if let Some(value) = value
                && !seen.contains(&value.as_str())
                && !value.starts_with("https://")
            {
                return Err(format!("the {what} {value:?} is no file of the release and no https:// URL"));
            }
        }
        Ok(())
    }

    /// The index and `sums` give each file the same SHA-256, and `sums`
    /// names no file that the index leaves out (except the index itself).
    pub fn agrees(&self, sums: &Sums) -> Result<(), String> {
        for file in &self.files {
            let want = sums.get(&file.name)?;
            if Some(want) != digest_of(&file.sha256) {
                return Err(format!("the index and SHA256SUMS give {} two SHA-256 sums", file.name));
            }
        }
        for name in sums.names() {
            if name != FILE && !self.files.iter().any(|f| f.name == name) {
                return Err(format!("SHA256SUMS names {name}, which the index does not"));
            }
        }
        Ok(())
    }

    /// The text of the index: JSON with two spaces, and a line end.
    pub fn to_text(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` with each field in its range.
fn utc_date(text: &str) -> bool {
    let b = text.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r.clone()).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    let num = |r: std::ops::Range<usize>| text.get(r).and_then(|t| t.parse::<u32>().ok()).unwrap_or(99);
    b.len() == 20
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19)
        && b[19] == b'Z'
        && (1..=12).contains(&num(5..7))
        && (1..=31).contains(&num(8..10))
        && num(11..13) < 24
        && num(14..16) < 60
        && num(17..19) < 61
}

#[cfg(any(test, feature = "schema"))]
pub mod schema;

#[cfg(test)]
mod tests;

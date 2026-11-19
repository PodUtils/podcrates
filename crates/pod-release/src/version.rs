//! Which release: the versions of the tags, in the order of semver, and the
//! tags of a list of releases: GitHub's API, or, where that fails (it
//! allows 60 calls an hour to an address with no token), the releases'
//! Atom feed. A tag with `-` in it is a prerelease, as a release workflow
//! marks one; only a prerelease follows another.
//!
//! The base is podssh's `update/release.rs`; the check of a tag's form
//! (a tag goes into URLs and into the identity of the signature) is
//! podnet's `valid_tag`.

use std::cmp::Ordering;

/// A version of semver's form, ordered as semver orders them: `1.0.0-beta.2`
/// before `1.0.0-beta.10`, and each prerelease before its release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<Ident>,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ident {
    Number(u64),
    Text(String),
}

impl Version {
    /// `MAJOR.MINOR.PATCH[-PRE][+BUILD]`, the build ignored as semver
    /// ignores it.
    pub fn parse(text: &str) -> Option<Version> {
        let plain = text.split('+').next()?;
        let (core, pre) = match plain.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (plain, None),
        };
        let numbers: Vec<u64> = core.split('.').map(number).collect::<Option<_>>()?;
        let core: [u64; 3] = numbers.try_into().ok()?;
        let pre = match pre {
            None => Vec::new(),
            Some(pre) => pre.split('.').map(ident).collect::<Option<_>>()?,
        };
        Some(Version { core, pre, text: plain.to_string() })
    }

    /// The version of a tag `vVERSION`.
    pub fn of_tag(tag: &str) -> Option<Version> {
        Version::parse(tag.strip_prefix('v')?)
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }

    /// The tag of this version.
    pub fn tag(&self) -> String {
        format!("v{}", self.text)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

/// A number with no leading zero, as semver writes one.
fn number(text: &str) -> Option<u64> {
    let ok = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) && (text == "0" || !text.starts_with('0'));
    ok.then(|| text.parse().ok()).flatten()
}

fn ident(text: &str) -> Option<Ident> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return None;
    }
    if text.bytes().all(|b| b.is_ascii_digit()) {
        return number(text).map(Ident::Number);
    }
    Some(Ident::Text(text.to_string()))
}

impl Ord for Ident {
    fn cmp(&self, other: &Ident) -> Ordering {
        match (self, other) {
            (Ident::Number(a), Ident::Number(b)) => a.cmp(b),
            (Ident::Number(_), Ident::Text(_)) => Ordering::Less,
            (Ident::Text(_), Ident::Number(_)) => Ordering::Greater,
            (Ident::Text(a), Ident::Text(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for Ident {
    fn partial_cmp(&self, other: &Ident) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> Ordering {
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => self.pre.cmp(&other.pre),
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// True when `tag` can be a release tag: 1 to 64 characters of
/// `0-9 A-Z a-z . + -`, the first `v` or a digit, and no `..`. A tag goes
/// into the URLs of the files and into the identity of the signature, so a
/// `/`, a `%` or a `..` must not pass.
pub fn valid_tag(tag: &str) -> bool {
    (1..=64).contains(&tag.len())
        && tag.starts_with(|c: char| c == 'v' || c.is_ascii_digit())
        && tag.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
        && !tag.contains("..")
}

/// The tag of each release in the answer of GitHub's
/// `GET /repos/OWNER/REPO/releases`, drafts and tags that are not
/// [`valid_tag`] left out.
pub fn tags_of_api(json: &[u8]) -> Result<Vec<String>, String> {
    let list: serde_json::Value = serde_json::from_slice(json).map_err(|e| format!("not JSON: {e}"))?;
    let list = list.as_array().ok_or("not a list of releases")?;
    Ok(list
        .iter()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .filter_map(|r| r["tag_name"].as_str().filter(|t| valid_tag(t)).map(str::to_string))
        .collect())
}

/// The tag of each entry of the feed `https://github.com/OWNER/REPO/releases.atom`:
/// the text after each `/releases/tag/` up to a quote, a `<` or a space,
/// once, when it is a [`valid_tag`]. A page that is not a feed (a login
/// page) gives none.
pub fn tags_of_atom(xml: &[u8]) -> Result<Vec<String>, String> {
    let text = std::str::from_utf8(xml).map_err(|_| "not UTF-8")?;
    let mut tags: Vec<String> = Vec::new();
    for part in text.split("/releases/tag/").skip(1) {
        let tag: String = part.chars().take_while(|c| !matches!(c, '"' | '<' | '\'' | ' ')).collect();
        if valid_tag(&tag) && !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    Ok(tags)
}

/// The newest of `tags` that is a version, a prerelease only when
/// `prerelease` allows one.
pub fn newest(tags: &[String], prerelease: bool) -> Option<Version> {
    tags.iter().filter_map(|t| Version::of_tag(t)).filter(|v| prerelease || !v.is_prerelease()).max()
}

/// The release of a target: the static musl binary of the same
/// architecture for a build of glibc (a release ships no glibc binary, and
/// a static binary runs on a glibc host too).
pub fn release_target(target: &str) -> String {
    target.replace("-gnueabihf", "-musleabihf").replace("-gnueabi", "-musleabi").replace("-gnu", "-musl")
}

/// The file of `program`'s binary for `target` in a release:
/// `PROGRAM-TARGET`, and `.exe` after it on Windows.
pub fn asset(program: &str, target: &str) -> String {
    if target.contains("-windows-") { format!("{program}-{target}.exe") } else { format!("{program}-{target}") }
}

#[cfg(test)]
mod tests;

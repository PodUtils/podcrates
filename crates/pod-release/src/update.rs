//! The steps of the self-update of a running binary (feature `fetch`):
//! which release ([`newest`], [`plan`]); its `SHA256SUMS` and the bundle,
//! checked by the identity of the product's release workflow ([`signed`]);
//! the binary of this target, checked against `SHA256SUMS` ([`binary`]);
//! then run once here and in place of the running one, the one before it
//! kept beside it ([`install`]). A consumer runs the steps in this order
//! and says each one in its own words: the crate prints nothing.
//!
//! The base is podssh's `update/mod.rs`; the plan of a pinned tag and of
//! `--force` is podnet's `plan`.

use std::cmp::Ordering;
use std::path::Path;
use std::time::Duration;

use crate::fetch::Fetcher;
use crate::replace::{self, Replaced, Stopped};
use crate::sigstore::{GITHUB_ISSUER, Identity, Roots};
use crate::trial::{self, Expect};
use crate::verify::{BUNDLE, SUMS, Verified, verify_release};
use crate::version::{self, Version};
use crate::{Error, hex};

/// The largest list of releases, from the API or the feed: each release's
/// notes are in it, up to 120 kB each.
pub const MAX_LIST: usize = 8 << 20;
/// The largest `SHA256SUMS` and bundle: a few kB each.
pub const MAX_SMALL: usize = 256 << 10;
/// The largest binary: a release's is about a tenth of this.
pub const MAX_BINARY: usize = 256 << 20;
/// Each small file, the connection and its answer.
pub const SMALL_LIMIT: Duration = Duration::from_secs(60);
/// The binary, on a slow link.
pub const BINARY_LIMIT: Duration = Duration::from_secs(900);

/// Where the releases of a product are, and who signs them: the settings
/// of the consumer (the ruling "Services: what stays").
#[derive(Debug, Clone)]
pub struct Source {
    /// `OWNER/REPO` on GitHub.
    pub repo: String,
    /// `https://api.github.com`, or a stand-in's.
    pub api: String,
    /// `https://github.com`, or a stand-in's.
    pub web: String,
    /// The release workflow, a file name in `.github/workflows/`.
    pub workflow: String,
    /// The issuer of the OIDC token of the release workflow.
    pub issuer: String,
    /// The roots of Sigstore that check the bundle.
    pub roots: Roots,
}

impl Source {
    /// The releases of `repo` on GitHub, signed by its workflow `workflow`
    /// with the token of GitHub Actions, checked by Sigstore's roots.
    pub fn github(repo: &str, workflow: &str) -> Source {
        Source {
            repo: repo.to_string(),
            api: "https://api.github.com".into(),
            web: "https://github.com".into(),
            workflow: workflow.to_string(),
            issuer: GITHUB_ISSUER.into(),
            roots: Roots::sigstore(),
        }
    }

    /// The identity that signs the release `tag`.
    pub fn identity(&self, tag: &str) -> Identity {
        Identity { issuer: self.issuer.clone(), ..Identity::github(&self.repo, &self.workflow, tag) }
    }

    /// The URL of the file `name` of the release `tag`.
    pub fn file(&self, tag: &str, name: &str) -> String {
        format!("{}/{}/releases/download/{tag}/{name}", self.web, self.repo)
    }
}

/// A reader of the tags of a list of releases.
type ReadTags = fn(&[u8]) -> Result<Vec<String>, String>;

/// The newest release that a build of `current` follows (a prerelease only
/// when `current` is one), and where its tag was read: GitHub's API, else
/// the releases' feed. `None` when a list was read and holds none.
pub async fn newest(
    fetcher: &Fetcher,
    source: &Source,
    current: &Version,
) -> Result<Option<(Version, &'static str)>, Error> {
    let pre = current.is_prerelease();
    let mut why = Vec::new();
    let mut read_one = false;
    let api = format!("{}/repos/{}/releases?per_page=30", source.api, source.repo);
    let feed = format!("{}/{}/releases.atom", source.web, source.repo);
    let sources: [(&str, &str, &'static str, ReadTags); 2] = [
        (api.as_str(), "application/vnd.github+json", "GitHub's API", version::tags_of_api),
        (feed.as_str(), "application/atom+xml", "the releases' feed", version::tags_of_atom),
    ];
    for (url, accept, name, read) in sources {
        match fetcher.get(url, accept, MAX_LIST, SMALL_LIMIT).await {
            Ok(body) => match read(&body).map(|tags| version::newest(&tags, pre)) {
                Ok(Some(version)) => return Ok(Some((version, name))),
                Ok(None) => {
                    why.push(format!("{name} lists no release of a version"));
                    read_one = true;
                }
                Err(e) => why.push(format!("{name}: {e}")),
            },
            Err(e) => why.push(e.why().to_string()),
        }
    }
    if read_one {
        return Ok(None);
    }
    Err(Error::Unavailable(format!("no list of the releases: {}", why.join("; "))))
}

/// The step before a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// The release is the version of this build: nothing to do.
    UpToDate,
    /// This build is newer than the release, which is not pinned: nothing
    /// to do. A caller says it apart from [`Plan::UpToDate`] (T-971).
    Newer,
    /// The pinned tag is the version of this build: nothing to do.
    Same,
    /// Only say that the release is there (a check).
    Available,
    /// Download, check and install.
    Install,
}

/// What to do with the release `tag` for a build of `current`. `pinned`:
/// the user named the tag (it may be older, on purpose); `check`: only say;
/// `force`: install the version of this build again. With no pinned tag,
/// an older release is never an update, also with `force`.
pub fn plan(tag: &Version, current: &Version, pinned: bool, check: bool, force: bool) -> Plan {
    let same = tag.cmp(current) == Ordering::Equal;
    if same && !force {
        return if pinned { Plan::Same } else { Plan::UpToDate };
    }
    if !pinned && !(force && same) && tag.cmp(current) == Ordering::Less {
        return Plan::Newer;
    }
    if check { Plan::Available } else { Plan::Install }
}

/// `SHA256SUMS` of a release, with its signature checked.
#[derive(Debug, Clone)]
pub struct Signed {
    pub version: Version,
    pub identity: Identity,
    pub verified: Verified,
}

/// `SHA256SUMS` of the release `version` and its bundle, checked by the
/// identity of `source` at the tag. A release with no `SHA256SUMS` is
/// [`Error::NotFound`].
pub async fn signed(fetcher: &Fetcher, source: &Source, version: &Version) -> Result<Signed, Error> {
    let tag = version.tag();
    if !version::valid_tag(&tag) {
        return Err(Error::Config(format!("not a release tag: {tag:?}")));
    }
    let small = |name: &'static str| {
        let url = source.file(&tag, name);
        async move { fetcher.get(&url, "application/octet-stream", MAX_SMALL, SMALL_LIMIT).await }
    };
    let sums = small(SUMS).await.map_err(|e| match e {
        Error::NotFound(why) => Error::NotFound(format!("no release has the tag {tag} ({why})")),
        other => other,
    })?;
    let bundle = small(BUNDLE).await?;
    let identity = source.identity(&tag);
    let verified = verify_release(&sums, &bundle, &identity, &source.roots).map_err(|e| {
        Error::Refused(format!(
            "{}; nothing was replaced. If Sigstore changed its roots, download the release by hand: {}/{}/releases/tag/{tag}",
            e.why(),
            source.web,
            source.repo
        ))
    })?;
    Ok(Signed { version: version.clone(), identity, verified })
}

/// The file `asset` of a signed release, checked against its
/// `SHA256SUMS`. A release whose `SHA256SUMS` names no `asset` is
/// [`Error::NotFound`], with no download.
pub async fn binary(fetcher: &Fetcher, source: &Source, signed: &Signed, asset: &str) -> Result<Vec<u8>, Error> {
    let tag = signed.version.tag();
    let want =
        signed.verified.sums.get(asset).map_err(|why| Error::NotFound(format!("{tag} has no {asset}: {why}")))?;
    let bytes = fetcher.get(&source.file(&tag, asset), "application/octet-stream", MAX_BINARY, BINARY_LIMIT).await?;
    let got = crate::sha256(&bytes);
    if got != want {
        return Err(Error::Refused(format!(
            "{asset} of {tag}: its SHA-256 is {}, and SHA256SUMS gives {}; nothing was replaced",
            hex(&got),
            hex(&want)
        )));
    }
    Ok(bytes)
}

/// `bytes`, the binary of `program` at `version`, in place of `exe`, once
/// it ran here and said `PROGRAM VERSION` within `limit`
/// ([`trial::LIMIT`] for a consumer).
pub fn install(exe: &Path, bytes: &[u8], program: &str, version: &Version, limit: Duration) -> Result<Replaced, Error> {
    let expect = Expect { program, version: Some(version), limit };
    let tried = |file: &Path| trial::check(file, &expect);
    let exe_shown = replace::shown(exe);
    replace::replace(exe, bytes, &tried).map_err(|stopped| match stopped {
        Stopped::Trial(why) => {
            Error::Refused(format!("{program} {version} did not run here: {why}; nothing was replaced"))
        }
        Stopped::Disk(why) => Error::CantWrite(format!("{why}; {exe_shown} is as it was")),
    })
}

#[cfg(test)]
mod tests;

//! The actions of a run by tag. Before the first job, the consumer collects
//! each `uses: OWNER/REPO[/PATH]@REF` of the workflow ([`workflow_uses`]);
//! [`resolve`] follows each composite action that it gets, resolves each
//! reference to a commit, and keeps the tarball of each commit in a store:
//! `DIR/OWNER_REPO/SHA.tar.gz`, the layout of the archive cache of the
//! runner. The lock pins each reference by commit and by the SHA-256 of the
//! tar; a tag that moved, or a tarball that differs from the lock, fails the
//! run by name. The host does each fetch; a unit never fetches an action.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{Read, Write};
use std::path::PathBuf;

use pod_actions_workflow::{Kind, Limits, Node, Workflow};
use sha2::{Digest, Sha256};

use crate::error::{CheckoutFault, Error};
pub use crate::git::Git;
use crate::lock::{Lock, Pin, is_commit};

/// A forge that resolves a reference and gives the tar of a commit. An
/// error is text for the user; [`resolve`] gives it as [`Error::Forge`].
pub trait Forge {
    /// The commit of `REF` (a tag or a branch) of `OWNER/REPO`; `HEAD` is
    /// the default branch (a checkout with no ref).
    fn resolve(&self, repo: &str, r: &str) -> Result<String, String>;
    /// The uncompressed tar of `commit`, with one top directory.
    fn tar(&self, repo: &str, commit: &str) -> Result<Vec<u8>, String>;
}

/// How a run may change the lock and the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Resolve and fetch what is missing, and check each tag of the lock.
    Online,
    /// Refuse a change of the lock; fetch only a tarball that the lock pins.
    Frozen,
    /// Use the lock and the store only: no fetch.
    Offline,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Mode::Online => "online",
            Mode::Frozen => "frozen",
            Mode::Offline => "offline",
        })
    }
}

/// The result: each `OWNER/REPO@REF` of the run and its commit, and the
/// lock after the run.
#[derive(Debug, Default)]
pub struct Resolved {
    pub commits: BTreeMap<String, String>,
    pub lock: Lock,
    pub changed: bool,
}

/// A reference of `uses:` split into `OWNER/REPO`, the path in it and the ref.
fn split(uses: &str) -> Option<(String, String, String)> {
    let (target, r) = uses.rsplit_once('@')?;
    let mut parts = target.splitn(3, '/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    if owner.is_empty() || repo.is_empty() || r.is_empty() {
        return None;
    }
    Some((format!("{owner}/{repo}"), parts.next().unwrap_or("").trim_matches('/').to_string(), r.to_string()))
}

/// Each `uses:` of a repository in the steps of the workflow, but a
/// repository `OWNER/REPO` of `except` (an action that the consumer gives
/// in its own way, as a stand-in of `actions/checkout`).
pub fn workflow_uses(w: &Workflow, except: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for job in &w.jobs {
        for s in &job.steps {
            match s.uses.as_deref() {
                Some(u) if except.iter().any(|r| u.split_once('@').is_some_and(|(t, _)| t == *r)) => {}
                Some(u) if remote(u) => out.push(u.to_string()),
                _ => {}
            }
        }
    }
    out
}

/// The repository of a checkout of another repository: `OWNER/REPO@REF` of
/// `with: repository` and `with: ref` (`HEAD` when no ref), or `None` for
/// the repository of the run (`self_repo`, by any case), which is the
/// consumer's own tree. An expression is refused: the host resolves each
/// tree before the first job.
pub fn checkout_target(with: Option<&Node>, self_repo: &str) -> Result<Option<String>, Error> {
    let value = |field: &'static str| -> Result<Option<String>, Error> {
        let fail = |value: &str, fault| Error::Checkout { field, value: value.to_string(), fault };
        match with.and_then(|w| w.get(field)).map(|n| &n.kind) {
            None => Ok(None),
            Some(Kind::String(v)) if v.trim() == "${{ github.repository }}" && field == "repository" => Ok(None),
            Some(Kind::String(v)) if v.contains("${{") => Err(fail(v, CheckoutFault::Expression)),
            Some(Kind::String(v)) => Ok(Some(v.trim().to_string())),
            Some(_) => Err(fail("", CheckoutFault::NotString)),
        }
    };
    let Some(repo) = value("repository")? else { return Ok(None) };
    if repo.eq_ignore_ascii_case(self_repo) {
        return Ok(None);
    }
    if repo.split('/').count() != 2 || repo.split('/').any(str::is_empty) {
        return Err(Error::Checkout { field: "repository", value: repo, fault: CheckoutFault::NotRepository });
    }
    Ok(Some(format!("{repo}@{}", value("ref")?.unwrap_or_else(|| "HEAD".into()))))
}

/// Each checkout of another repository in the steps of the workflow
/// (`uses: actions/checkout@REF` with `with: repository`).
pub fn workflow_trees(w: &Workflow, self_repo: &str) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    for job in &w.jobs {
        for s in &job.steps {
            if s.uses.as_deref().is_some_and(|u| u.starts_with("actions/checkout@"))
                && let Some(t) = checkout_target(s.with.as_ref(), self_repo)?
            {
                out.push(t);
            }
        }
    }
    Ok(out)
}

/// Resolve and keep each tree of `trees` (`OWNER/REPO@REF`) after the
/// actions of `out`: pinned and kept as an action is, with no action file
/// to read.
pub fn add_trees(
    trees: &[String],
    mut out: Resolved,
    store: &Store,
    forge: &dyn Forge,
    mode: Mode,
    lock_name: &str,
) -> Result<Resolved, Error> {
    for t in trees {
        let (repo, _, r) = split(t).ok_or_else(|| Error::Tree { tree: t.clone() })?;
        let key = format!("{repo}@{r}");
        if !out.commits.contains_key(&key) {
            let at = At { repo: &repo, r: &r, key: &key, lock_name };
            let (commit, _) = fetch(&at, &mut out, store, forge, mode)?;
            out.commits.insert(key, commit);
        }
    }
    Ok(out)
}

fn remote(uses: &str) -> bool {
    !uses.starts_with("./") && !uses.starts_with("docker://")
}

/// Each `uses:` of a repository in the steps of a composite `action.yml`.
fn composite_uses(text: &str) -> Result<Vec<String>, String> {
    let root = pod_actions_workflow::yaml::load(text, Limits::default()).map_err(|e| e.to_string())?;
    let Some(runs) = root.get("runs") else { return Ok(Vec::new()) };
    if !matches!(runs.get("using").map(|n| &n.kind), Some(Kind::String(u)) if u == "composite") {
        return Ok(Vec::new());
    }
    let Some(Kind::Seq(steps)) = runs.get("steps").map(|n| &n.kind) else { return Ok(Vec::new()) };
    Ok(steps
        .iter()
        .filter_map(|s| match s.get("uses").map(|n| &n.kind) {
            Some(Kind::String(u)) if remote(u) => Some(u.clone()),
            _ => None,
        })
        .collect())
}

/// The `action.yml` (or `action.yaml`) at `path` under the top directory of
/// a tar.
fn action_file(tar: &[u8], path: &str) -> Result<Option<String>, String> {
    let mut archive = tar::Archive::new(tar);
    let want: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let name = entry.path().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
        let parts: Vec<&str> = name.split('/').filter(|c| !c.is_empty()).collect();
        let Some((file, dirs)) = parts.split_last() else { continue };
        if dirs.len() == want.len() + 1 && dirs[1..] == want[..] && (*file == "action.yml" || *file == "action.yaml") {
            let mut text = String::new();
            entry.read_to_string(&mut text).map_err(|e| format!("{name}: {e}"))?;
            return Ok(Some(text));
        }
    }
    Ok(None)
}

/// `sha256:HEX` of `bytes`: the digest of a lock covers the uncompressed
/// tar, so the version of git or of gzip does not change it.
pub fn digest(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    format!("sha256:{}", d.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// The name of a tarball in the store: `OWNER_REPO/SHA.tar.gz`; `/` becomes
/// `_`, as in the archive cache of the runner.
pub fn archive_name(repo: &str, commit: &str) -> String {
    format!("{}/{commit}.tar.gz", repo.replace('/', "_"))
}

/// The store of tarballs: `DIR/OWNER_REPO/SHA.tar.gz`.
pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    pub fn path(&self, repo: &str, commit: &str) -> PathBuf {
        self.dir.join(archive_name(repo, commit))
    }

    /// The uncompressed tar of a commit in the store, if there is one.
    fn get(&self, repo: &str, commit: &str) -> Result<Option<Vec<u8>>, Error> {
        let path = self.path(repo, commit);
        let Ok(file) = std::fs::File::open(&path) else { return Ok(None) };
        let mut tar = Vec::new();
        flate2::read::GzDecoder::new(file).read_to_end(&mut tar).map_err(|e| Error::file(&path, &e))?;
        Ok(Some(tar))
    }

    /// Keep a tar, compressed, through a temporary file.
    fn put(&self, repo: &str, commit: &str, tar: &[u8]) -> Result<(), Error> {
        let path = self.path(repo, commit);
        let fail = |e: std::io::Error| Error::file(&path, &e);
        std::fs::create_dir_all(path.parent().expect("a directory of the repository")).map_err(fail)?;
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(tar).map_err(fail)?;
        std::fs::write(&tmp, gz.finish().map_err(fail)?).map_err(fail)?;
        std::fs::rename(&tmp, &path).map_err(fail)
    }
}

/// Resolve each reference of `uses`, and of each composite action that they
/// name, against the lock, the store and the forge. `lock_name` names the
/// lock file in each error.
pub fn resolve(
    uses: &[String],
    lock: &Lock,
    store: &Store,
    forge: &dyn Forge,
    mode: Mode,
    lock_name: &str,
) -> Result<Resolved, Error> {
    let mut out = Resolved { lock: lock.clone(), ..Resolved::default() };
    let mut queue: VecDeque<String> = uses.iter().cloned().collect();
    let mut seen = BTreeSet::new();
    let mut tars: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    while let Some(u) = queue.pop_front() {
        if !seen.insert(u.clone()) {
            continue;
        }
        let (repo, path, r) = split(&u).ok_or_else(|| Error::Uses { uses: u.clone() })?;
        let key = format!("{repo}@{r}");
        if !tars.contains_key(&key) {
            let at = At { repo: &repo, r: &r, key: &key, lock_name };
            let (commit, tar) = fetch(&at, &mut out, store, forge, mode)?;
            out.commits.insert(key.clone(), commit);
            tars.insert(key.clone(), tar);
        }
        let text = action_file(&tars[&key], &path).map_err(|reason| Error::Tarball { key: key.clone(), reason })?;
        let Some(text) = text else {
            return Err(Error::NoAction { uses: u, path });
        };
        queue.extend(composite_uses(&text).map_err(|reason| Error::Action { uses: u.clone(), reason })?);
    }
    Ok(out)
}

/// One reference: `OWNER/REPO`, its ref, its key in the lock, and the name
/// of the lock for the errors.
struct At<'a> {
    repo: &'a str,
    r: &'a str,
    key: &'a str,
    lock_name: &'a str,
}

/// The commit and the tar of one reference.
fn fetch(
    at: &At<'_>,
    out: &mut Resolved,
    store: &Store,
    forge: &dyn Forge,
    mode: Mode,
) -> Result<(String, Vec<u8>), Error> {
    let (repo, key, lock) = (at.repo, at.key.to_string(), at.lock_name.to_string());
    let forged = |reason| Error::Forge { repo: repo.to_string(), reason };
    let not_locked = || Error::NotLocked { key: key.clone(), lock: lock.clone(), mode };
    let pin = out.lock.pins.get(at.key).cloned();
    let commit = match (&pin, mode) {
        _ if is_commit(at.r) => at.r.to_ascii_lowercase(),
        (Some(p), Mode::Offline) => p.commit.clone(),
        (Some(p), _) => {
            let now = forge.resolve(repo, at.r).map_err(forged)?;
            if now != p.commit {
                return Err(Error::Moved { key, lock, pinned: p.commit.clone(), now });
            }
            now
        }
        // A reference that the lock does not have: refused before the forge.
        (None, Mode::Offline | Mode::Frozen) => return Err(not_locked()),
        (None, Mode::Online) => forge.resolve(repo, at.r).map_err(forged)?,
    };
    // The commit names a file of the store: take only a full commit id.
    if !is_commit(&commit) {
        return Err(forged(format!("`{key}`: the forge gave `{commit}`, not a commit id")));
    }
    if let Some(p) = &pin
        && p.commit != commit
    {
        return Err(Error::Pinned { key, lock, commit, pinned: p.commit.clone() });
    }
    let (tar, fresh) = match store.get(repo, &commit)? {
        Some(tar) => (tar, false),
        None if mode == Mode::Offline => return Err(Error::NotStored { key, commit }),
        None if mode == Mode::Frozen && pin.is_none() => return Err(not_locked()),
        None => (forge.tar(repo, &commit).map_err(forged)?, true),
    };
    let d = digest(&tar);
    match &pin {
        Some(p) if p.digest != d => {
            return Err(Error::Digest { key, commit, lock, digest: d, pinned: p.digest.clone() });
        }
        Some(_) => {}
        None if mode == Mode::Frozen => return Err(not_locked()),
        None => {
            out.lock.pins.insert(key, Pin { commit: commit.clone(), digest: d });
            out.changed = true;
        }
    }
    if fresh {
        store.put(repo, &commit, &tar)?;
    }
    Ok((commit, tar))
}

#[cfg(test)]
mod tests;

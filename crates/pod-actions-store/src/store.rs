//! The actions of a run by tag (T-118). Before the first job, `podrunner
//! run` collects each `uses: OWNER/REPO[/PATH]@REF` of the workflow and of
//! each composite action that it gets, resolves each one to a commit, and
//! keeps the tarball of each commit in a store: `DIR/OWNER_REPO/SHA.tar.gz`,
//! the layout that the fake broker serves (T-091). The lock file pins each
//! reference by commit and by the SHA-256 of the tar; a tag that moved, or a
//! tarball that differs from the lock, fails the run by name. The host does
//! each fetch; a unit never fetches an action.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use pod_actions_workflow::{Kind, Limits, Node, Workflow};

use crate::lock::{Lock, Pin, is_commit};

/// A forge that resolves a reference and gives the tar of a commit.
pub trait Forge {
    /// The commit of `REF` (a tag or a branch) of `OWNER/REPO`.
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

/// The result: each `OWNER/REPO@REF` of the run and its commit, for the
/// launch route of the fake broker, and the lock after the run.
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

/// Each `uses:` of a repository in the steps of the workflow. The stand-in
/// of `actions/checkout` (T-092) takes no tarball.
pub fn workflow_uses(w: &Workflow) -> Vec<String> {
    let mut out = Vec::new();
    for job in &w.jobs {
        for s in &job.steps {
            match s.uses.as_deref() {
                Some(u) if u.starts_with("actions/checkout@") => {}
                Some(u) if remote(u) => out.push(u.to_string()),
                _ => {}
            }
        }
    }
    out
}

fn remote(uses: &str) -> bool {
    !uses.starts_with("./") && !uses.starts_with("docker://")
}

fn get<'a>(n: &'a Node, key: &str) -> Option<&'a Node> {
    match &n.kind {
        Kind::Map(e) => e.iter().find(|(k, _)| k.name == key).map(|(_, v)| v),
        _ => None,
    }
}

/// Each `uses:` of a repository in the steps of a composite `action.yml`.
fn composite_uses(text: &str) -> Result<Vec<String>, String> {
    let root = pod_actions_workflow::yaml::load(text, Limits::default()).map_err(|e| e.to_string())?;
    let Some(runs) = get(&root, "runs") else { return Ok(Vec::new()) };
    if !matches!(get(runs, "using").map(|n| &n.kind), Some(Kind::String(u)) if u == "composite") {
        return Ok(Vec::new());
    }
    let Some(Kind::Seq(steps)) = get(runs, "steps").map(|n| &n.kind) else { return Ok(Vec::new()) };
    Ok(steps
        .iter()
        .filter_map(|s| match get(s, "uses").map(|n| &n.kind) {
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

pub fn digest(bytes: &[u8]) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, bytes);
    format!("sha256:{}", d.as_ref().iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// The store of tarballs: `DIR/OWNER_REPO/SHA.tar.gz`.
pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    pub fn path(&self, repo: &str, commit: &str) -> PathBuf {
        self.dir.join(podrunner_broker_fake::actions::archive_name(repo, commit))
    }

    /// The uncompressed tar of a commit in the store, if there is one.
    fn get(&self, repo: &str, commit: &str) -> Result<Option<Vec<u8>>, String> {
        let path = self.path(repo, commit);
        let Ok(file) = std::fs::File::open(&path) else { return Ok(None) };
        let mut tar = Vec::new();
        flate2::read::GzDecoder::new(file).read_to_end(&mut tar).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Some(tar))
    }

    /// Keep a tar, compressed, through a temporary file.
    fn put(&self, repo: &str, commit: &str, tar: &[u8]) -> Result<(), String> {
        let path = self.path(repo, commit);
        let fail = |e: std::io::Error| format!("{}: {e}", path.display());
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
) -> Result<Resolved, String> {
    let mut out = Resolved { lock: lock.clone(), ..Resolved::default() };
    let mut queue: VecDeque<String> = uses.iter().cloned().collect();
    let mut seen = BTreeSet::new();
    let mut tars: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    while let Some(u) = queue.pop_front() {
        if !seen.insert(u.clone()) {
            continue;
        }
        let (repo, path, r) = split(&u).ok_or_else(|| format!("`uses: {u}` is not OWNER/REPO[/PATH]@REF"))?;
        let key = format!("{repo}@{r}");
        if !tars.contains_key(&key) {
            let (commit, tar) = fetch(&repo, &r, &key, &mut out, store, forge, mode, lock_name)?;
            out.commits.insert(key.clone(), commit);
            tars.insert(key.clone(), tar);
        }
        let text = action_file(&tars[&key], &path).map_err(|e| format!("the tarball of `{key}`: {e}"))?;
        let Some(text) = text else {
            return Err(format!("`uses: {u}`: the tarball has no action.yml at `{path}`"));
        };
        queue.extend(composite_uses(&text).map_err(|e| format!("the action.yml of `{u}`: {e}"))?);
    }
    Ok(out)
}

/// The commit and the tar of one reference.
#[allow(clippy::too_many_arguments)]
fn fetch(
    repo: &str,
    r: &str,
    key: &str,
    out: &mut Resolved,
    store: &Store,
    forge: &dyn Forge,
    mode: Mode,
    lock_name: &str,
) -> Result<(String, Vec<u8>), String> {
    let pin = out.lock.pins.get(key).cloned();
    let commit = match (&pin, mode) {
        _ if is_commit(r) => r.to_ascii_lowercase(),
        (Some(p), Mode::Offline) => p.commit.clone(),
        (Some(p), _) => {
            let now = forge.resolve(repo, r)?;
            if now != p.commit {
                return Err(format!(
                    "`{key}` moved: {lock_name} has {}, the forge has {now}; remove its line from {lock_name} to take {now}",
                    p.commit
                ));
            }
            now
        }
        (None, Mode::Offline) => return Err(format!("`{key}` is not in {lock_name}; run once with no --offline")),
        (None, Mode::Frozen) => return Err(format!("`{key}` is not in {lock_name}, and --frozen refuses a change")),
        (None, Mode::Online) => forge.resolve(repo, r)?,
    };
    if let Some(p) = &pin
        && p.commit != commit
    {
        return Err(format!("`{key}` names {commit}, but {lock_name} has {}", p.commit));
    }
    let (tar, fresh) = match store.get(repo, &commit)? {
        Some(tar) => (tar, false),
        None if mode == Mode::Offline => {
            return Err(format!("`{key}`: {} is not in the store, and --offline fetches nothing", commit));
        }
        None if mode == Mode::Frozen && pin.is_none() => {
            return Err(format!("`{key}` is not in {lock_name}, and --frozen refuses a change"));
        }
        None => (forge.tar(repo, &commit)?, true),
    };
    let d = digest(&tar);
    match &pin {
        Some(p) if p.digest != d => {
            return Err(format!("the tarball of `{key}` ({commit}) has {d}, but {lock_name} has {}", p.digest));
        }
        Some(_) => {}
        None if mode == Mode::Frozen => {
            return Err(format!("`{key}` is not in {lock_name}, and --frozen refuses a change"));
        }
        None => {
            out.lock.pins.insert(key.to_string(), Pin { commit: commit.clone(), digest: d });
            out.changed = true;
        }
    }
    if fresh {
        store.put(repo, &commit, &tar)?;
    }
    Ok((commit, tar))
}

/// The forge of git over HTTPS: `git ls-remote` resolves a ref, and a
/// shallow fetch of the commit gives `git archive`. git takes the proxy of
/// the host from its environment.
pub struct Git {
    /// The base of the repositories, as `https://github.com/`.
    pub base: String,
}

fn git(args: &[&str], dir: Option<&Path>) -> Result<Vec<u8>, String> {
    let mut c = std::process::Command::new("git");
    if let Some(d) = dir {
        c.arg("-C").arg(d);
    }
    let out = c
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git {}: {}", args[0], err.trim()));
    }
    Ok(out.stdout)
}

impl Forge for Git {
    fn resolve(&self, repo: &str, r: &str) -> Result<String, String> {
        let url = format!("{}{repo}", self.base);
        let tag = format!("refs/tags/{r}");
        let head = format!("refs/heads/{r}");
        let text = String::from_utf8(git(&["ls-remote", &url, &tag, &format!("{tag}^{{}}"), &head], None)?)
            .map_err(|e| e.to_string())?;
        let find = |name: &str| text.lines().find_map(|l| l.split_once('\t').filter(|(_, n)| *n == name).map(|x| x.0));
        // An annotated tag names a tag object: its peeled line names the commit.
        find(&format!("{tag}^{{}}"))
            .or_else(|| find(&tag))
            .or_else(|| find(&head))
            .map(str::to_string)
            .ok_or_else(|| format!("`{repo}@{r}`: {url} has no tag or branch `{r}`"))
    }

    fn tar(&self, repo: &str, commit: &str) -> Result<Vec<u8>, String> {
        let dir = std::env::temp_dir().join(format!("podrunner-fetch-{}-{commit}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let url = format!("{}{repo}", self.base);
        let prefix = format!("--prefix={}-{}/", repo.replace('/', "-"), &commit[..7]);
        let r = git(&["init", "-q"], Some(&dir))
            .and_then(|_| git(&["fetch", "-q", "--depth", "1", &url, commit], Some(&dir)))
            .and_then(|_| git(&["archive", "--format=tar", &prefix, "FETCH_HEAD"], Some(&dir)));
        let _ = std::fs::remove_dir_all(&dir);
        r.map_err(|e| format!("`{repo}` at {commit}: {e}"))
    }
}

#[cfg(test)]
mod tests;

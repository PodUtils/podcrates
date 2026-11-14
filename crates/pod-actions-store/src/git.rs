//! The forge of git as a program: `git ls-remote` resolves a ref, and a
//! shallow fetch of the commit gives `git archive`. git takes the proxy of
//! the host from its environment; the crate sets nothing of it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::store::Forge;

/// The forge of git over any URL that git reads (`https://`, `file://`).
#[derive(Clone, Debug)]
pub struct Git {
    /// The base of the repositories, as `https://github.com/`: the URL of
    /// `OWNER/REPO` is the base and the name.
    pub base: String,
    /// The directory of the fetches: each fetch makes a directory of its
    /// own in it, and removes it.
    pub scratch: PathBuf,
    /// The git program: `git` in `PATH` by default.
    pub program: PathBuf,
}

impl Git {
    pub fn new(base: impl Into<String>, scratch: impl Into<PathBuf>) -> Git {
        Git { base: base.into(), scratch: scratch.into(), program: PathBuf::from("git") }
    }

    fn run(&self, args: &[&str], dir: Option<&Path>) -> Result<Vec<u8>, String> {
        let mut c = Command::new(&self.program);
        if let Some(d) = dir {
            c.arg("-C").arg(d);
        }
        let out = c
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("{}: {e}", self.program.display()))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("git {}: {}", args[0], err.trim()));
        }
        Ok(out.stdout)
    }

    /// A new directory under `scratch`. `create_dir` fails on a name that
    /// exists, so two fetches of one commit never share a directory.
    fn fresh_dir(&self, commit: &str) -> Result<PathBuf, String> {
        std::fs::create_dir_all(&self.scratch).map_err(|e| format!("{}: {e}", self.scratch.display()))?;
        for n in 0..u32::MAX {
            let dir = self.scratch.join(format!("fetch-{}-{commit}-{n}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(dir),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("{}: {e}", dir.display())),
            }
        }
        Err(format!("{}: no free name", self.scratch.display()))
    }
}

impl Forge for Git {
    fn resolve(&self, repo: &str, r: &str) -> Result<String, String> {
        let url = format!("{}{repo}", self.base);
        let tag = format!("refs/tags/{r}");
        let head = format!("refs/heads/{r}");
        let text = String::from_utf8(self.run(&["ls-remote", &url, &tag, &format!("{tag}^{{}}"), &head], None)?)
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
        let dir = self.fresh_dir(commit)?;
        let url = format!("{}{repo}", self.base);
        let short = commit.get(..7).unwrap_or(commit);
        let prefix = format!("--prefix={}-{short}/", repo.replace('/', "-"));
        let r = self
            .run(&["init", "-q"], Some(&dir))
            .and_then(|_| self.run(&["fetch", "-q", "--depth", "1", &url, commit], Some(&dir)))
            .and_then(|_| self.run(&["archive", "--format=tar", &prefix, "FETCH_HEAD"], Some(&dir)));
        let _ = std::fs::remove_dir_all(&dir);
        r.map_err(|e| format!("`{repo}` at {commit}: {e}"))
    }
}

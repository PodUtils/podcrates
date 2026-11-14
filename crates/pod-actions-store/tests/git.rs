//! The git forge against a bare repository that each test makes in its own
//! temporary directory: a lightweight tag, an annotated tag and a branch,
//! each on a commit of its own, served by `file://`. The tests skip with
//! the reason when `git` is not in `PATH`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use pod_actions_store::{Forge, Git, Lock, Mode, Store, digest, is_commit, resolve};

/// A temporary directory of one test.
fn scratch(name: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("pod-actions-store-git-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// git with a fixed identity and no signing, whatever the host's config.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(["-c", "commit.gpgSign=false", "-c", "tag.gpgSign=false", "-c", "init.defaultBranch=main"])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// The repositories: `o/r` with three commits, the tag `v1` on the first,
/// the annotated tag `v2` on the second and the branch `main` on the third;
/// `o/comp` with a composite action on its tag `v1` that names `o/r@v1`.
struct Repos {
    root: PathBuf,
    first: String,
    second: String,
    third: String,
}

fn commit(work: &Path, file: &str, text: &str, message: &str) -> String {
    let path = work.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    git(work, &["add", "-A"]);
    git(work, &["commit", "-q", "-m", message]);
    git(work, &["rev-parse", "HEAD"])
}

fn repos(name: &str) -> Repos {
    let root = scratch(name);
    let work = root.join("work-r");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q"]);
    let node = "runs:\n  using: node20\n  main: index.js\n";
    let first = commit(&work, "action.yml", node, "first");
    git(&work, &["tag", "v1"]);
    let second = commit(&work, "index.js", "1\n", "second");
    git(&work, &["tag", "-a", "v2", "-m", "an annotated tag"]);
    let third = commit(&work, "index.js", "2\n", "third");
    std::fs::create_dir_all(root.join("o")).unwrap();
    git(&root, &["clone", "-q", "--bare", "work-r", "o/r"]);
    let comp = root.join("work-comp");
    std::fs::create_dir_all(&comp).unwrap();
    git(&comp, &["init", "-q"]);
    let composite = "runs:\n  using: composite\n  steps:\n    - uses: o/r@v1\n";
    commit(&comp, "sub/action.yml", composite, "composite");
    git(&comp, &["tag", "v1"]);
    git(&root, &["clone", "-q", "--bare", "work-comp", "o/comp"]);
    Repos { root, first, second, third }
}

impl Repos {
    fn forge(&self) -> Git {
        let base = format!("file://{}/", self.root.display());
        Git::new(base, self.root.join("scratch"))
    }
}

#[test]
fn git_resolves_a_tag_an_annotated_tag_and_a_branch() {
    pod_testkit::need_tool!("git");
    let r = repos("resolve");
    let g = r.forge();
    assert_eq!(g.resolve("o/r", "v1").unwrap(), r.first, "a lightweight tag");
    let tag_object = git(&r.root.join("o/r"), &["rev-parse", "refs/tags/v2"]);
    assert_ne!(tag_object, r.second, "the test needs an annotated tag");
    assert_eq!(g.resolve("o/r", "v2").unwrap(), r.second, "an annotated tag gives its commit");
    assert_eq!(g.resolve("o/r", "main").unwrap(), r.third, "a branch");
    let e = g.resolve("o/r", "v9").unwrap_err();
    assert!(e.contains("has no tag or branch `v9`"), "{e}");
    let e = g.resolve("o/none", "v1").unwrap_err();
    assert!(e.starts_with("git ls-remote:"), "{e}");
    let _ = std::fs::remove_dir_all(&r.root);
}

#[test]
fn git_gives_the_tar_of_a_commit_with_one_top_directory() {
    pod_testkit::need_tool!("git");
    let r = repos("tar");
    let g = r.forge();
    let tar = g.tar("o/r", &r.second).unwrap();
    let mut names = Vec::new();
    for entry in tar::Archive::new(&tar[..]).entries().unwrap() {
        let entry = entry.unwrap();
        // git archive writes the commit id in a pax global header first.
        if entry.header().entry_type() != tar::EntryType::XGlobalHeader {
            names.push(entry.path().unwrap().to_string_lossy().into_owned());
        }
    }
    let top = format!("o-r-{}/", &r.second[..7]);
    assert!(names.iter().all(|n| n.starts_with(&top)), "{names:?}");
    assert!(names.contains(&format!("{top}action.yml")) && names.contains(&format!("{top}index.js")), "{names:?}");
    assert_eq!(digest(&g.tar("o/r", &r.second).unwrap()), digest(&tar), "the same commit gives the same tar");
    let left: Vec<_> = std::fs::read_dir(&g.scratch).unwrap().collect();
    assert!(left.is_empty(), "each fetch removes its directory: {left:?}");
    let e = g.tar("o/r", &"0".repeat(40)).unwrap_err();
    assert!(e.contains("`o/r` at 0000000"), "{e}");
    let _ = std::fs::remove_dir_all(&r.root);
}

#[test]
fn git_feeds_a_resolve_through_a_composite_action_and_back_offline() {
    pod_testkit::need_tool!("git");
    let r = repos("resolve-all");
    let g = r.forge();
    let store = Store { dir: r.root.join("store") };
    let uses = vec!["o/comp/sub@v1".to_string()];
    let got = resolve(&uses, &Lock::default(), &store, &g, Mode::Online, "L").unwrap();
    assert_eq!(got.commits["o/r@v1"], r.first, "the composite action names o/r@v1");
    assert!(is_commit(&got.commits["o/comp@v1"]));
    assert!(got.changed && got.lock.pins.len() == 2);
    assert!(store.path("o/r", &r.first).is_file());
    let frozen = resolve(&uses, &got.lock, &store, &g, Mode::Frozen, "L").unwrap();
    assert!(!frozen.changed);
    let gone = Git::new("file:///nowhere/", r.root.join("scratch"));
    let off = resolve(&uses, &got.lock, &store, &gone, Mode::Offline, "L").unwrap();
    assert_eq!(off.commits, got.commits);
    let _ = std::fs::remove_dir_all(&r.root);
}

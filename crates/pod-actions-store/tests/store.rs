//! The actions by tag against github.com (T-118): a composite action by tag
//! that names a second action by tag gives both commits, both tarballs and
//! a lock; a second run takes the lock and the store, and `--offline` asks
//! the forge nothing. Ignored by default: it needs git and github.com.
//!
//!   cargo test -p podrunner-local --test store -- --ignored --nocapture

use podrunner_local::lock::{Lock, is_commit};
use podrunner_local::store::{Forge, Git, Mode, Store, resolve};

/// A forge that may not be asked.
struct Refuse;

impl Forge for Refuse {
    fn resolve(&self, repo: &str, r: &str) -> Result<String, String> {
        Err(format!("asked to resolve {repo}@{r}"))
    }
    fn tar(&self, repo: &str, commit: &str) -> Result<Vec<u8>, String> {
        Err(format!("asked for {repo} at {commit}"))
    }
}

#[test]
#[ignore]
fn a_composite_action_by_tag_and_the_action_that_it_names_by_tag() {
    let dir = std::env::temp_dir().join(format!("podrunner-store-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store { dir: dir.clone() };
    let git = Git { base: "https://github.com/".into() };
    let uses = vec!["actions/upload-pages-artifact@v3".to_string()];
    let r = resolve(&uses, &Lock::default(), &store, &git, Mode::Online, "L").unwrap();
    println!("{}", r.lock.render());
    for key in ["actions/upload-pages-artifact@v3", "actions/upload-artifact@v4"] {
        let commit = &r.commits[key];
        assert!(is_commit(commit), "{key}: {commit}");
        let repo = key.split('@').next().unwrap();
        assert!(store.path(repo, commit).is_file(), "{key} is in the store");
        assert_eq!(&r.lock.pins[key].commit, commit);
    }
    let again = resolve(&uses, &r.lock, &store, &git, Mode::Frozen, "L").unwrap();
    assert!(!again.changed);
    let off = resolve(&uses, &r.lock, &store, &Refuse, Mode::Offline, "L").unwrap();
    assert_eq!(off.commits, r.commits);
    let _ = std::fs::remove_dir_all(&dir);
}

//! Where the files of an [`App`] live: the directory that its variable
//! names, the user's cache directory, the user's runtime directory, the
//! temporary directory, and the working directory, each from the
//! environment that the caller gives; `none` gives no directory. Only the
//! fixed places of the temporary directory of Unix are written here, and
//! tried last.
//!
//! On Unix the user's cache and runtime directories count only in a tree of
//! the effective uid or of root: one in another user's tree is passed over,
//! and still read for what the app kept there before, and the home of the
//! uid's entry in the user database gives the user's cache in its place.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The program whose files these are: its name (the directories are
/// `DIR/NAME`, `TMP/NAME-UID` and `.NAME`), and the variable that names its
/// directory, or `none` for no file (podssh: `PODSSH_CACHE_DIR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct App<'a> {
    pub name: &'a str,
    pub dir_env: &'a str,
}

/// The temporary directory's fixed places on Unix, tried only when no
/// variable names it, and last.
#[cfg(unix)]
const FIXED_TEMPORARY: &[&str] = &["/tmp", "/dev/shm"];

/// What the variable of the app says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chosen {
    /// Not set, or empty: the usual places.
    Unset,
    /// `none`: the app keeps no file between runs.
    Nothing,
    /// An absolute directory, tried first.
    Dir(PathBuf),
    /// A relative path, refused: its place would follow the working
    /// directory of each run.
    Relative(String),
}

/// What the variable of `app` says in the environment that `var` reads.
pub fn chosen_from(app: &App, var: &dyn Fn(&str) -> Option<OsString>) -> Chosen {
    match var(app.dir_env) {
        None => Chosen::Unset,
        Some(value) if value.is_empty() => Chosen::Unset,
        Some(value) if value.eq_ignore_ascii_case("none") => Chosen::Nothing,
        Some(value) if Path::new(&value).is_absolute() => Chosen::Dir(PathBuf::from(value)),
        Some(value) => Chosen::Relative(value.to_string_lossy().into_owned()),
    }
}

/// The places of the files: the directories that the app writes in, in
/// order, and each usual place that it passed over, with why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Places {
    /// The directories tried for a new file, in order.
    pub dirs: Vec<PathBuf>,
    /// The user's cache directory among them, where there is one.
    pub user: Option<PathBuf>,
    /// Each usual place in another user's tree.
    pub passed: Vec<Passed>,
    /// The name and the variable of the app, for the notes.
    pub app: String,
    pub dir_env: String,
}

/// A usual place of the cache that is in another user's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passed {
    /// The directory of the app that it would be, `DIR/NAME`.
    pub dir: PathBuf,
    /// Why: the directory on the way and its owner.
    pub why: String,
}

impl Places {
    /// Where a kept file is looked for: the directories that the app writes
    /// in, then each place passed over, so that a key, a pin, a pair or a
    /// token kept there before is still found, and a newer copy in this
    /// user's own cache wins.
    pub fn read(&self) -> Vec<PathBuf> {
        self.dirs.iter().chain(self.passed.iter().map(|p| &p.dir)).cloned().collect()
    }

    /// The place passed over that holds the file `path`, if any.
    pub fn passed_for(&self, path: &Path) -> Option<&Passed> {
        self.passed.iter().find(|p| path.parent() == Some(p.dir.as_path()))
    }

    /// What a run says once of a file at `path` that is in a place passed
    /// over: where it is, why, and where to move it to keep it; nothing is
    /// moved.
    pub fn kept_note(&self, path: &Path) -> Option<String> {
        let passed = self.passed_for(path)?;
        let to = match (&self.user, path.file_name()) {
            (Some(dir), Some(name)) => {
                format!("move it to {} to keep it in this user's own cache", dir.join(name).display())
            }
            _ => {
                format!("move it to a directory of this user's that lasts, and name that directory in {}", self.dir_env)
            }
        };
        Some(format!(
            "{} is in another user's tree ({}): {} reads it there and writes nothing more there; {to}",
            path.display(),
            passed.why,
            self.app
        ))
    }
}

/// The places of the files of `app` in the environment that `var` reads,
/// with the working directory `cwd`; the owners and the user database are
/// this host's. A consumer passes `&|name| std::env::var_os(name)`.
pub fn places_from(app: &App, var: &dyn Fn(&str) -> Option<OsString>, cwd: Option<PathBuf>) -> Places {
    #[cfg(unix)]
    {
        places_with(app, var, cwd, &super::owner::Host::this())
    }
    #[cfg(not(unix))]
    {
        // No owner is read: a profile of another user is no report there.
        collect(app, var, cwd, &|_, _| None, &|| None)
    }
}

/// [`places_from`] with the owners and the user database given (for tests,
/// which then run as any user).
#[cfg(unix)]
pub fn places_with(
    app: &App,
    var: &dyn Fn(&str) -> Option<OsString>,
    cwd: Option<PathBuf>,
    host: &super::owner::Host,
) -> Places {
    let foreign = |dir: &Path, top: Option<&Path>| super::owner::foreign(dir, top, host);
    collect(app, var, cwd, &foreign, &|| (host.home_of)(host.uid))
}

/// The directories tried, in order: the one of the variable of `app`, the
/// user's cache directory, the user's runtime directory, the temporary
/// directory, the working directory. `none` gives no directory.
pub fn candidate_dirs_from(app: &App, var: &dyn Fn(&str) -> Option<OsString>, cwd: Option<PathBuf>) -> Vec<PathBuf> {
    places_from(app, var, cwd).dirs
}

/// The list, with `foreign` saying why a directory is in another user's tree
/// (`top` is its home), and `entry` giving the home of the uid's entry in the
/// user database.
fn collect(
    app: &App,
    var: &dyn Fn(&str) -> Option<OsString>,
    cwd: Option<PathBuf>,
    foreign: &dyn Fn(&Path, Option<&Path>) -> Option<String>,
    entry: &dyn Fn() -> Option<PathBuf>,
) -> Places {
    let absolute = |name: &str| var(name).map(PathBuf::from).filter(|p| p.is_absolute());
    let mut places = Places { app: app.name.to_string(), dir_env: app.dir_env.to_string(), ..Places::default() };
    match chosen_from(app, var) {
        Chosen::Nothing => return places,
        Chosen::Dir(dir) => places.dirs.push(dir),
        Chosen::Unset | Chosen::Relative(_) => {}
    }
    if let Some(dir) = user_cache(app.name, &absolute, foreign, entry, &mut places.passed) {
        places.user = Some(dir.clone());
        places.dirs.push(dir);
    }
    // Made for each user by the login session, where there is one, and the
    // user's own by the XDG spec.
    if let Some(dir) = absolute("XDG_RUNTIME_DIR") {
        match foreign(&dir, None) {
            None => places.dirs.push(dir.join(app.name)),
            Some(why) => places.passed.push(Passed { dir: dir.join(app.name), why }),
        }
    }
    let tag = user_tag_from(var);
    for dir in temporary_dirs(var) {
        places.dirs.push(dir.join(format!("{}-{tag}", app.name)));
    }
    if let Some(cwd) = cwd {
        places.dirs.push(cwd.join(format!(".{}", app.name)));
    }
    places
}

/// The user's cache directory, `DIR/NAME`: on Windows `LOCALAPPDATA`; else
/// the first of `XDG_CACHE_HOME` and `HOME/.cache` that is in this user's
/// tree, else, once one was passed over, the `.cache` of the home of the
/// uid's entry in the user database. Each place passed over goes to `passed`.
fn user_cache(
    name: &str,
    absolute: &dyn Fn(&str) -> Option<PathBuf>,
    foreign: &dyn Fn(&Path, Option<&Path>) -> Option<String>,
    entry: &dyn Fn() -> Option<PathBuf>,
    passed: &mut Vec<Passed>,
) -> Option<PathBuf> {
    if cfg!(windows) {
        return absolute("LOCALAPPDATA").map(|dir| dir.join(name));
    }
    let home = absolute("HOME");
    let offered = absolute("XDG_CACHE_HOME")
        .map(|dir| (dir, home.clone()))
        .into_iter()
        .chain(home.clone().map(|home| (home.join(".cache"), Some(home))));
    for (dir, top) in offered {
        if passed.iter().any(|p| p.dir == dir.join(name)) {
            continue;
        }
        match foreign(&dir, top.as_deref()) {
            None => return Some(dir.join(name)),
            Some(why) => passed.push(Passed { dir: dir.join(name), why }),
        }
    }
    // With no variable that names a place, none is guessed.
    if passed.is_empty() {
        return None;
    }
    let home = entry()?;
    let dir = home.join(".cache");
    if passed.iter().any(|p| p.dir == dir.join(name)) {
        return None;
    }
    match foreign(&dir, Some(&home)) {
        None => Some(dir.join(name)),
        Some(why) => {
            passed.push(Passed { dir: dir.join(name), why });
            None
        }
    }
}

/// The temporary directory: the first absolute one of `TMPDIR`, `TMP` and
/// `TEMP` (Windows reads `TMP` and `TEMP`, then its own, at run time), and on
/// Unix the fixed places only when no variable names one.
fn temporary_dirs(var: &dyn Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let named =
        ["TMPDIR", "TMP", "TEMP"].iter().find_map(|name| var(name).map(PathBuf::from).filter(|p| p.is_absolute()));
    if let Some(dir) = named {
        return vec![dir];
    }
    #[cfg(unix)]
    {
        FIXED_TEMPORARY.iter().map(PathBuf::from).collect()
    }
    #[cfg(not(unix))]
    {
        vec![std::env::temp_dir()]
    }
}

/// A per-user tag for shared directories such as the temporary one: the
/// effective uid on Unix, the user's name on Windows.
fn user_tag_from(var: &dyn Fn(&str) -> Option<OsString>) -> String {
    #[cfg(unix)]
    {
        let _ = var;
        super::euid().to_string()
    }
    #[cfg(not(unix))]
    {
        var("USERNAME")
            .map(|u| u.to_string_lossy().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| "user".to_string())
    }
}

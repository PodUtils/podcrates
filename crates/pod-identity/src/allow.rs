//! Who may reach a node: the client keys in an allowlist
//! file, as sshd's `authorized_keys` gives access by key, on each road. The
//! file holds one key on each line, in one of the [`KEY_FORMS`]: OpenSSH's
//! public line (`ssh-ed25519 BASE64`, as `podssh keygen -y` prints it), the
//! fingerprint (`SHA256:...`), the key in iroh's hex or base32, or the
//! display fingerprint; then an optional comment. A line that starts with
//! `#` is a comment. A line that is not a key lets nobody in, and is named
//! ([`bad_lines_said`]). A word `routes=A,B` right after the key lets it
//! use those routes of the node, `routes=*` each one. A key reaches the
//! node's target that has no route only by a line with no `routes=` word
//! or with `routes=*` ([`Allowlist::reaches_target`]).

use std::path::Path;

use super::{KEY_FORMS, KeyName, OPENSSH_TYPE, PublicKey};
use pod_core::files::{self as cache, Others};

/// The keys of an allowlist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Allowlist {
    keys: Vec<Entry>,
    /// The numbers of the lines that hold no key, from 1.
    bad: Vec<usize>,
}

/// A key, and the routes that it may use.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name: KeyName,
    routes: Vec<String>,
    /// `routes=*`: each route.
    all: bool,
    /// A line of the key with no `routes=` word: the target with no route.
    bare: bool,
}

impl Allowlist {
    /// The keys of `text`, with no limit of entries.
    pub fn parse(text: &str) -> Allowlist {
        Allowlist::parse_with(text, usize::MAX)
    }

    /// The keys of `text`, at most `limit` of them (PodUtils/podnet#512,
    /// A11): a line of a new key past the limit is a bad line, so a node
    /// that reads a file of another hand does not grow without a bound. A
    /// key on a later line that is already in the list still adds its
    /// routes.
    pub fn parse_with(text: &str, limit: usize) -> Allowlist {
        let mut list = Allowlist::default();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut words = line.split_whitespace();
            let first = words.next().unwrap_or_default();
            let name = match first {
                OPENSSH_TYPE => words.next().and_then(|blob| super::openssh::from_words(first, blob)).map(KeyName::Key),
                _ => KeyName::parse(first),
            };
            let routes = match words.next().and_then(|w| w.strip_prefix("routes=")) {
                None => Some((Vec::new(), false)),
                Some("*") => Some((Vec::new(), true)),
                Some(list) => {
                    let routes: Vec<String> = list.split(',').map(str::to_string).collect();
                    routes.iter().all(|r| pod_core::names::check_node_name(r).is_ok()).then_some((routes, false))
                }
            };
            match (name, routes) {
                (Some(name), Some((routes, all))) => {
                    let full = list.keys.len() >= limit;
                    let bare = routes.is_empty() && !all;
                    match list.keys.iter_mut().find(|e| e.name == name) {
                        // A key on two lines has the routes of both, and
                        // keeps the mark of a line with no routes.
                        Some(entry) => {
                            entry.routes.extend(routes);
                            entry.all |= all;
                            entry.bare |= bare;
                        }
                        None if full => list.bad.push(n + 1),
                        None => list.keys.push(Entry { name, routes, all, bare }),
                    }
                }
                _ => list.bad.push(n + 1),
            }
        }
        list
    }

    /// The allowlist in the file at `path`: a file of this user that others
    /// cannot change, as sshd reads `authorized_keys`; others may read it, as
    /// it holds public keys only. A missing file is an error, so the node
    /// says that nobody can get in.
    pub fn read(path: &Path) -> Result<Allowlist, String> {
        Allowlist::read_with(path, usize::MAX)
    }

    /// [`Allowlist::read`] with at most `limit` keys ([`Allowlist::parse_with`]).
    pub fn read_with(path: &Path, limit: usize) -> Result<Allowlist, String> {
        match cache::read_own(path, Others::Read)? {
            Some(text) => Ok(Allowlist::parse_with(&text, limit)),
            None => Err(format!("{}: no such file", path.display())),
        }
    }

    /// Whether `key` may get in.
    pub fn admits(&self, key: &PublicKey) -> bool {
        self.keys.iter().any(|e| e.name.matches(key))
    }

    /// Whether `key` may use `route` of the node.
    pub fn grants(&self, key: &PublicKey, route: &str) -> bool {
        self.keys.iter().any(|e| e.name.matches(key) && (e.all || e.routes.iter().any(|r| r == route)))
    }

    /// Whether `key` may reach the node's target that has no route: by a
    /// line of the key with no `routes=` word, or with `routes=*`. A key
    /// whose lines all say `routes=A,B` reaches those routes alone.
    pub fn reaches_target(&self, key: &PublicKey) -> bool {
        self.keys.iter().any(|e| e.name.matches(key) && (e.bare || e.all))
    }

    /// The words of a refusal of the list's bad lines ([`bad_lines_said`]);
    /// `None` when each line is a key or a comment.
    pub fn bad_lines_said(&self) -> Option<String> {
        (!self.bad.is_empty()).then(|| bad_lines_said(&self.bad))
    }

    /// How many keys the list holds.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The numbers of the lines that are not a key, from 1.
    pub fn bad_lines(&self) -> &[usize] {
        &self.bad
    }
}

/// The words of a refusal of the bad lines `lines` of a list: their
/// numbers, and the forms of a line.
pub fn bad_lines_said(lines: &[usize]) -> String {
    let (lines, are) = match lines {
        [one] => (format!("line {one}"), "is not a key"),
        _ => (format!("lines {}", lines.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")), "are not keys"),
    };
    format!("{lines} {are}: a line holds {KEY_FORMS}, then routes=A,B and a comment, both optional")
}

/// Why the key of the list at `path` is refused: it is not in the list, and
/// the list's bad lines, which can be the line meant for the key.
fn not_in(path: &Path, list: &Allowlist) -> String {
    match list.bad_lines_said() {
        Some(bad) => format!("it is not in {}; {bad}", path.display()),
        None => format!("it is not in {}", path.display()),
    }
}

/// Whether the client `key` may connect, by the allowlist at `path`, read
/// again for each connection so that a key added counts at once, and a key
/// taken away at the next session. `Err` is why not, for the node's log;
/// the client learns only that its key is not let in.
pub fn admits(path: &Path, key: &PublicKey) -> Result<(), String> {
    match Allowlist::read(path) {
        Ok(list) if list.admits(key) => Ok(()),
        Ok(list) => Err(not_in(path, &list)),
        Err(why) => Err(why),
    }
}

/// Whether the client `key` may use `route` of the node, by the allowlist at
/// `path`, read again for each session.
pub fn grants(path: &Path, key: &PublicKey, route: &str) -> Result<(), String> {
    match Allowlist::read(path) {
        Ok(list) if list.grants(key, route) => Ok(()),
        Ok(list) if !list.admits(key) => Err(not_in(path, &list)),
        Ok(_) => Err(format!("{} does not give it the route {route}", path.display())),
        Err(why) => Err(why),
    }
}

/// Whether the client `key` may reach the node's target that has no route,
/// by the allowlist at `path`, read again for each session
/// ([`Allowlist::reaches_target`]).
pub fn reaches_target(path: &Path, key: &PublicKey) -> Result<(), String> {
    match Allowlist::read(path) {
        Ok(list) if list.reaches_target(key) => Ok(()),
        Ok(list) if !list.admits(key) => Err(not_in(path, &list)),
        Ok(_) => Err(format!(
            "its lines in {} give it routes alone, not the target with no route: a line with no routes= word or with \
             routes=* gives that",
            path.display()
        )),
        Err(why) => Err(why),
    }
}

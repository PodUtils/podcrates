//! The exec table of a spec: the paths of its rules with
//! [`Access::EXECUTE`], and the table of any access in the same shape
//! ([`Spec::table`]).
//!
//! Landlock checks `EXECUTE` only on the file that the kernel executes: the
//! program, its `PT_INTERP` loader, and the script and the interpreter of a
//! `#!` line. A loader that the caller runs with the program as its argument
//! (the entry with no chroot of `pod-enter`, and each exec that its
//! interposer routes through the loader of the image) only reads and maps
//! the program, so the kernel checks `EXECUTE` on the loader alone, and a
//! program outside each `exec=` path runs. The caller that starts such a
//! program asks the table first: [`PathTable::allows`], or [`allows_in`]
//! on the text of [`PathTable::encode`] in an object that takes no
//! dependency (an interposer); for the program of a loader, with
//! [`crate::loader`]. The table cannot stop a payload that runs the loader
//! itself with a program in a directory that it can write: the loader must
//! be executable, so the check holds the programs that the caller sees.
//!
//! Landlock governs no change of mode, owner or times (`chmod`, `chown`,
//! `utimensat`). The write table (`spec.table(Access::WRITE)`) gives an
//! interposer the paths where the spec lets the payload write, so it can
//! refuse such a change of a file outside them.

use pod_core::{Class, Failure};

use crate::{Access, Spec, failure};

/// The separator of the entries of [`PathTable::encode`]. A path that
/// holds it is a refusal of the encode.
pub const SEP: u8 = b':';

/// The paths of the rules of a spec with one access: each path once, with
/// no trailing `/` and no empty component. A rule on a directory covers the
/// tree under it, as in Landlock.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathTable {
    paths: Vec<String>,
}

/// The table of the rules with [`Access::EXECUTE`]: the paths that a
/// program may run from ([`Spec::exec_table`]).
pub type ExecTable = PathTable;

impl Spec {
    /// The exec table of the spec, with the paths as the rules name them:
    /// [`Spec::table`] of [`Access::EXECUTE`].
    pub fn exec_table(&self) -> PathTable {
        self.table(Access::EXECUTE)
    }

    /// The table of the rules that hold each bit of `access`, with the
    /// paths as the rules name them, in the order of the rules.
    /// `table(Access::WRITE)` is the write table. [`Access::NONE`] gives
    /// each rule.
    pub fn table(&self, access: Access) -> PathTable {
        let mut paths: Vec<String> = Vec::new();
        for r in self.paths.iter().filter(|r| r.access.contains(access)) {
            let p = lexical(&r.path);
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
        PathTable { paths }
    }
}

impl PathTable {
    /// The paths of the table.
    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    /// The table with each path that exists on this host resolved
    /// (`realpath`): a rule holds the inode that its path opens, so a link
    /// in a rule path counts as its target. A path that does not resolve
    /// stays as written. For host paths only: a guest path of
    /// [`Spec::compile_in`] resolves under its root, not here.
    pub fn resolved(&self) -> PathTable {
        let mut paths: Vec<String> = Vec::new();
        for p in &self.paths {
            let r =
                std::fs::canonicalize(p).ok().and_then(|r| r.to_str().map(str::to_string)).unwrap_or_else(|| p.clone());
            if !paths.contains(&r) {
                paths.push(r);
            }
        }
        PathTable { paths }
    }

    /// Whether the table covers the resolved, absolute path `program`: it
    /// is a path of the table or is under one. For the exec table, the
    /// program may run; for the write table, the file may change. A path
    /// with a `.` or `..` component, or that is not absolute, is not
    /// resolved, and gets `false`. An empty table allows nothing, as a spec
    /// with no rule of the access does.
    pub fn allows(&self, program: &str) -> bool {
        self.paths.iter().any(|p| covers(p.as_bytes(), program.as_bytes()))
    }

    /// The table as one text: the paths joined by [`SEP`]. A path with
    /// [`SEP`] or a NUL byte is a refusal by name (`Class::Usage`): the
    /// text could not spell it.
    pub fn encode(&self) -> Result<String, Failure> {
        if let Some(p) = self.paths.iter().find(|p| p.bytes().any(|b| b == SEP || b == 0)) {
            return Err(failure(
                Class::Usage,
                format!(
                    "the path {p:?} of a rule holds a {:?} or a NUL byte: the table of the paths cannot spell it",
                    char::from(SEP)
                ),
            ));
        }
        Ok(self.paths.join(&char::from(SEP).to_string()))
    }

    /// The table of a text of [`PathTable::encode`].
    pub fn decode(text: &str) -> PathTable {
        PathTable { paths: text.split(char::from(SEP)).filter(|p| !p.is_empty()).map(str::to_string).collect() }
    }
}

/// [`PathTable::allows`] on the text of [`PathTable::encode`], with no
/// allocation and no other item of the crate: an interposer that takes no
/// dependency keeps a copy of this function.
pub fn allows_in(table: &[u8], program: &[u8]) -> bool {
    table.split(|b| *b == SEP).any(|p| covers(p, program))
}

/// Whether the rule path `rule` covers the resolved path `program`.
fn covers(rule: &[u8], program: &[u8]) -> bool {
    if rule.first() != Some(&b'/') || program.first() != Some(&b'/') {
        return false;
    }
    if program.split(|b| *b == b'/').any(|c| c == b"." || c == b"..") {
        return false;
    }
    let mut rule = rule;
    while rule.len() > 1 && rule.ends_with(b"/") {
        rule = &rule[..rule.len() - 1];
    }
    if rule == b"/" {
        return true;
    }
    program.starts_with(rule) && matches!(program.get(rule.len()), None | Some(b'/'))
}

/// `path` with no empty component and no trailing `/`; `/` stays `/`.
fn lexical(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    if path.starts_with('/') { format!("/{}", parts.join("/")) } else { parts.join("/") }
}

#[cfg(test)]
mod tests;

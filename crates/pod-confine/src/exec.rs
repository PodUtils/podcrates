//! The exec table of a spec: the paths of its rules with
//! [`Access::EXECUTE`].
//!
//! Landlock checks `EXECUTE` only on the file that the kernel executes: the
//! program, its `PT_INTERP` loader, and the script and the interpreter of a
//! `#!` line. A loader that the caller runs with the program as its argument
//! (the entry with no chroot of `pod-enter`, and each exec that its
//! interposer routes through the loader of the image) only reads and maps
//! the program, so the kernel checks `EXECUTE` on the loader alone, and a
//! program outside each `exec=` path runs. The caller that starts such a
//! program asks the table first: [`ExecTable::allows`], or [`allows_in`]
//! on the text of [`ExecTable::encode`] in an object that takes no
//! dependency (an interposer). The table cannot stop a payload that runs
//! the loader itself: the loader must be executable, so the check holds
//! the programs that the caller starts, not each mapping of the payload.

use pod_core::{Class, Failure};

use crate::{Access, Spec, failure};

/// The separator of the entries of [`ExecTable::encode`]. A path that
/// holds it is a refusal of the encode.
pub const SEP: u8 = b':';

/// The paths that a program may run from: each path of a rule with
/// [`Access::EXECUTE`], with no trailing `/` and no empty component. A rule
/// on a directory covers the tree under it, as in Landlock.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecTable {
    paths: Vec<String>,
}

impl Spec {
    /// The exec table of the spec, with the paths as the rules name them.
    pub fn exec_table(&self) -> ExecTable {
        let mut paths: Vec<String> = Vec::new();
        for r in self.paths.iter().filter(|r| r.access.contains(Access::EXECUTE)) {
            let p = lexical(&r.path);
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
        ExecTable { paths }
    }
}

impl ExecTable {
    /// The paths of the table.
    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    /// The table with each path that exists on this host resolved
    /// (`realpath`): a rule holds the inode that its path opens, so a link
    /// in a rule path counts as its target. A path that does not resolve
    /// stays as written. For host paths only: a guest path of
    /// [`Spec::compile_in`] resolves under its root, not here.
    pub fn resolved(&self) -> ExecTable {
        let mut paths: Vec<String> = Vec::new();
        for p in &self.paths {
            let r =
                std::fs::canonicalize(p).ok().and_then(|r| r.to_str().map(str::to_string)).unwrap_or_else(|| p.clone());
            if !paths.contains(&r) {
                paths.push(r);
            }
        }
        ExecTable { paths }
    }

    /// Whether the resolved, absolute path `program` may run: it is a path
    /// of the table or is under one. A path with a `.` or `..` component,
    /// or that is not absolute, is not resolved, and gets `false`. An empty
    /// table allows nothing, as a spec with no execute rule does.
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
                    "the execute path {p:?} holds a {:?} or a NUL byte: the exec table cannot spell it",
                    char::from(SEP)
                ),
            ));
        }
        Ok(self.paths.join(&char::from(SEP).to_string()))
    }

    /// The table of a text of [`ExecTable::encode`].
    pub fn decode(text: &str) -> ExecTable {
        ExecTable { paths: text.split(char::from(SEP)).filter(|p| !p.is_empty()).map(str::to_string).collect() }
    }
}

/// [`ExecTable::allows`] on the text of [`ExecTable::encode`], with no
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

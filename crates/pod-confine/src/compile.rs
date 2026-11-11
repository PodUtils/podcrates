//! The compile of a [`Spec`] before the fork: the masks of the ABI, each
//! path open as `O_PATH`, the port rules and the BPF program. The child
//! then only reads this data ([`crate::apply`]).

use pod_core::{Class, Failure};
use pod_hostprobe::sys::{self as k, CBuf, Errno, SockFilter};

use crate::spec::{BIND_TCP, CONNECT_TCP, FILE_RIGHTS, handled_fs};
use crate::{Access, MIN_ABI, NET_ABI, Spec, errno_class, errno_text, failure};

/// The most path rules, and the most port rules, of one spec: a refusal of
/// [`crate::apply`] names a rule by its index in 16 bits.
pub const MOST_RULES: usize = u16::MAX as usize;

/// One path rule, opened: the descriptor, the rights that it gives, and the
/// path as the spec named it.
#[derive(Debug)]
pub struct Rule {
    pub(crate) fd: i64,
    pub(crate) allowed: u64,
    path: String,
}

impl Rule {
    /// The `O_PATH` descriptor of the rule.
    pub fn fd(&self) -> i64 {
        self.fd
    }

    /// The Landlock rights of the rule.
    pub fn allowed(&self) -> u64 {
        self.allowed
    }

    /// The path, as the spec named it.
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// A spec made ready for [`crate::apply`]: the ruleset masks, each path
/// open, the port rules and the BPF program. It owns the descriptors and
/// closes them when it drops; each one is close-on-exec, so a payload
/// never holds one.
#[derive(Debug)]
pub struct Compiled {
    pub(crate) abi: u32,
    pub(crate) handled_fs: u64,
    pub(crate) handled_net: u64,
    pub(crate) rules: Vec<Rule>,
    pub(crate) ports: Vec<(u16, u64)>,
    pub(crate) filter: Vec<SockFilter>,
}

impl Drop for Compiled {
    fn drop(&mut self) {
        for r in &self.rules {
            let _ = k::close(r.fd);
        }
    }
}

impl Compiled {
    /// The ABI that the masks follow.
    pub fn abi(&self) -> u32 {
        self.abi
    }

    /// The file rights that the ruleset handles: each one that no rule
    /// gives is denied.
    pub fn handled_fs(&self) -> u64 {
        self.handled_fs
    }

    /// The TCP rights that the ruleset handles.
    pub fn handled_net(&self) -> u64 {
        self.handled_net
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Each port rule: the port and its rights.
    pub fn ports(&self) -> &[(u16, u64)] {
        &self.ports
    }

    /// The BPF program of the deny list.
    pub fn filter(&self) -> &[SockFilter] {
        &self.filter
    }

    /// Move each descriptor whose number is in `taken` to a number over
    /// each of them. A caller that duplicates descriptors onto fixed
    /// numbers in the child (`dup2` onto 0, 1, 2 or 3) calls this first,
    /// so that no rule loses its descriptor to the duplicate.
    pub fn keep_clear_of(&mut self, taken: &[i64]) -> Result<(), Failure> {
        let above = taken.iter().copied().max().unwrap_or(-1) + 1;
        for r in &mut self.rules {
            if taken.contains(&r.fd) {
                let moved = k::fcntl(r.fd, k::F_DUPFD_CLOEXEC, above.max(0) as u64).map_err(|e| {
                    failure(
                        errno_class(e),
                        format!("fcntl(F_DUPFD_CLOEXEC) of the rule {:?}: {}", r.path, errno_text(e)),
                    )
                })?;
                let _ = k::close(r.fd);
                r.fd = moved;
            }
        }
        Ok(())
    }
}

impl Spec {
    /// Compile with host paths: each path opens from the root of this
    /// process. `abi` is the answer of [`crate::check`].
    pub fn compile(&self, abi: u32) -> Result<Compiled, Failure> {
        self.compile_with(abi, |c| k::open(c, k::O_PATH | k::O_CLOEXEC, 0))
    }

    /// Compile with the paths of a guest root: each path resolves under the
    /// directory `root` (a descriptor) as the kernel resolves it after a
    /// `chroot` there (`openat2` with `RESOLVE_IN_ROOT`: an absolute link
    /// and `..` stay in the root). The rule holds the inode, so it stays
    /// true after the root change of the child.
    pub fn compile_in(&self, abi: u32, root: i64) -> Result<Compiled, Failure> {
        let how = k::OpenHow { flags: k::O_PATH | k::O_CLOEXEC, mode: 0, resolve: k::RESOLVE_IN_ROOT };
        self.compile_with(abi, |c| k::openat2(root, c, &how))
    }

    fn compile_with(&self, abi: u32, open: impl Fn(&CBuf) -> Result<i64, Errno>) -> Result<Compiled, Failure> {
        if abi == 0 {
            return Err(failure(Class::Unsupported, "Landlock is absent: the payload does not run unconfined"));
        }
        if abi < MIN_ABI {
            return Err(failure(
                Class::Unsupported,
                format!(
                    "Landlock ABI {abi} is under {MIN_ABI}: truncate(2) stays unconfined there, so the payload \
                     does not run"
                ),
            ));
        }
        let tcp = self.connect.is_some() || self.bind.is_some();
        if tcp && abi < NET_ABI {
            return Err(failure(
                Class::Unsupported,
                format!("the spec has TCP port rules, and Landlock ABI {abi} has none (ABI {NET_ABI} has them)"),
            ));
        }
        let ports = port_rules(self)?;
        if self.paths.len() > MOST_RULES {
            return Err(failure(
                Class::Usage,
                format!("the spec has {} path rules; a spec takes at most {MOST_RULES}", self.paths.len()),
            ));
        }
        let handled = handled_fs(abi);
        let mut handled_net = 0;
        if self.connect.is_some() {
            handled_net |= CONNECT_TCP;
        }
        if self.bind.is_some() {
            handled_net |= BIND_TCP;
        }
        let filter = crate::deny::program(&self.deny)?;
        let mut out = Compiled {
            abi,
            handled_fs: handled,
            handled_net,
            rules: Vec::with_capacity(self.paths.len()),
            ports,
            filter,
        };
        for rule in &self.paths {
            let fd = open_rule(&rule.path, rule.access, &open)?;
            // Pushed before the stat, so that a refusal below closes it.
            out.rules.push(Rule { fd, allowed: 0, path: rule.path.clone() });
            let st = k::fstatat(fd, &k::cempty(), k::AT_EMPTY_PATH).map_err(|e| {
                failure(errno_class(e), format!("fstat of the path {:?} of the spec: {}", rule.path, errno_text(e)))
            })?;
            let mut allowed = rule.access.rights() & handled;
            if st.st_mode & k::S_IFMT != k::S_IFDIR {
                allowed &= FILE_RIGHTS;
            }
            if allowed == 0 {
                return Err(failure(
                    Class::Usage,
                    format!(
                        "the path {:?} of the spec gives {} on a file: no right of a file",
                        rule.path,
                        rule.access.words()
                    ),
                ));
            }
            if let Some(last) = out.rules.last_mut() {
                last.allowed = allowed;
            }
        }
        Ok(out)
    }
}

/// Open the path of one rule, or refuse it by name.
fn open_rule(path: &str, access: Access, open: &impl Fn(&CBuf) -> Result<i64, Errno>) -> Result<i64, Failure> {
    if access.is_empty() {
        return Err(failure(Class::Usage, format!("the path {path:?} of the spec has no access")));
    }
    if !path.starts_with('/') {
        return Err(failure(Class::Usage, format!("the path {path:?} of the spec is not absolute")));
    }
    let c = CBuf::new(path).ok_or_else(|| failure(Class::Usage, format!("the path {path:?} contains a NUL byte")))?;
    open(&c).map_err(|e| {
        let what = if e == k::ENOENT { "does not exist" } else { "cannot be opened" };
        failure(errno_class(e), format!("the path {path:?} ({}) of the spec {what}: {}", access.words(), errno_text(e)))
    })
}

/// One rule for each port and right: the kernel adds the rights of two
/// rules of one port.
fn port_rules(spec: &Spec) -> Result<Vec<(u16, u64)>, Failure> {
    let mut out = Vec::new();
    for (list, right) in [(&spec.connect, CONNECT_TCP), (&spec.bind, BIND_TCP)] {
        for &port in list.iter().flatten() {
            out.push((port, right));
        }
    }
    if out.len() > MOST_RULES {
        return Err(failure(
            Class::Usage,
            format!("the spec has {} port rules; a spec takes at most {MOST_RULES}", out.len()),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;

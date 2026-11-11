//! The compile of a [`Spec`] before the fork: the masks and the scopes of
//! the ABI, each path open as `O_PATH`, the port rules and the BPF program
//! of the deny list with the rules of the spec. The child then only reads
//! this data ([`crate::apply`]), or opens the prepared paths again in its
//! own view ([`Compiled::open_paths`]).

use std::fmt;

use pod_core::{Class, Failure};
use pod_hostprobe::sys::{self as k, CBuf, Errno, SockFilter};

use crate::deny::Deny;
use crate::spec::{BIND_TCP, CONNECT_TCP, FILE_RIGHTS, SCOPES, handled_fs};
use crate::{Access, Call, MIN_ABI, NET_ABI, Refusal, SCOPE_ABI, Spec, errno_class, errno_text, failure};

/// The most path rules, and the most port rules, of one spec: a refusal of
/// [`crate::apply`] names a rule by its index in 16 bits.
pub const MOST_RULES: usize = u16::MAX as usize;

/// One path rule, opened: the descriptor, the rights that it gives, and the
/// path as the spec named it.
pub struct Rule {
    pub(crate) fd: i64,
    pub(crate) allowed: u64,
    path: String,
    /// The path for an open in the child, made before the fork.
    cpath: CBuf,
}

impl fmt::Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rule").field("fd", &self.fd).field("allowed", &self.allowed).field("path", &self.path).finish()
    }
}

impl Rule {
    /// The `O_PATH` descriptor of the rule, or -1 between
    /// [`Compiled::close_paths`] and [`Compiled::open_paths`].
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

/// True where the kernel of the build target has `socketcall(2)`: a socket
/// made through it hides its family and type from a filter.
const SOCKETCALL: bool = cfg!(any(target_arch = "x86", target_arch = "s390x", target_arch = "powerpc64"));
#[cfg(any(target_arch = "x86", target_arch = "s390x", target_arch = "powerpc64"))]
const _: i64 = k::SYS_SOCKETCALL;

/// A spec made ready for [`crate::apply`]: the ruleset masks and scopes,
/// each path open, the port rules and the BPF program. It owns the
/// descriptors and closes them when it drops; each one is close-on-exec,
/// so a payload never holds one.
#[derive(Debug)]
pub struct Compiled {
    pub(crate) abi: u32,
    pub(crate) handled_fs: u64,
    pub(crate) handled_net: u64,
    pub(crate) scoped: u64,
    pub(crate) keep_caps: u64,
    pub(crate) rules: Vec<Rule>,
    pub(crate) ports: Vec<(u16, u64)>,
    pub(crate) filter: Vec<SockFilter>,
}

impl Drop for Compiled {
    fn drop(&mut self) {
        for r in self.rules.iter().filter(|r| r.fd >= 0) {
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

    /// The scopes of the ruleset: [`crate::spec::SCOPES`] where the spec
    /// asks for them and the ABI has them, else 0.
    pub fn scoped(&self) -> u64 {
        self.scoped
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
            if r.fd >= 0 && taken.contains(&r.fd) {
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

    /// Close the descriptor of each rule, and keep its path. Call it after
    /// the compile, before the fork, where the child changes its view of
    /// the paths (a mount, a root change): the child then opens each path
    /// in its own view with [`Compiled::open_paths`]. Landlock does not
    /// read a rule on a directory under a mount point, so a descriptor of
    /// the compile does not cover a mount that the child makes on its path.
    /// No descriptor of a rule crosses the fork, so a `dup2` of the child
    /// takes the number of no rule.
    pub fn close_paths(&mut self) {
        for r in self.rules.iter_mut().filter(|r| r.fd >= 0) {
            let _ = k::close(r.fd);
            r.fd = -1;
        }
    }

    /// Open the path of each rule with no descriptor as `O_PATH |
    /// O_CLOEXEC`, in the view of this process: its mounts and its root.
    /// A rule with a descriptor keeps it. The first failed open is a
    /// [`Refusal`] of [`Call::OpenPath`] with the index of its rule, and the
    /// caller must not exec.
    ///
    /// Async-signal-safe: no allocation, no lock. Call it in the child
    /// after its mounts, its root change and its `dup2` calls, before
    /// [`crate::apply`]. A guest path of [`Spec::compile_in`] then resolves
    /// as the compile resolved it, under the new root.
    pub fn open_paths(&mut self) -> Result<(), Refusal> {
        for (i, r) in self.rules.iter_mut().enumerate().filter(|(_, r)| r.fd < 0) {
            r.fd = k::open(&r.cpath, k::O_PATH | k::O_CLOEXEC, 0).map_err(|errno| Refusal {
                call: Call::OpenPath,
                index: i as u16,
                errno,
            })?;
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
    /// true after a root change of the child, but not under a mount that
    /// the child makes on its path: there, use [`Compiled::close_paths`]
    /// and [`Compiled::open_paths`].
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
        let scoped = if self.scope && abi >= SCOPE_ABI { SCOPES } else { 0 };
        let filter = crate::deny::program(&self.deny_rules()?)?;
        let mut out = Compiled {
            abi,
            handled_fs: handled,
            handled_net,
            scoped,
            keep_caps: self.keep_caps,
            rules: Vec::with_capacity(self.paths.len()),
            ports,
            filter,
        };
        for rule in &self.paths {
            let (fd, cpath) = open_rule(&rule.path, rule.access, &open)?;
            // Pushed before the stat, so that a refusal below closes it.
            out.rules.push(Rule { fd, allowed: 0, path: rule.path.clone(), cpath });
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

impl Spec {
    /// The rules of the filter: [`Spec::deny`], then the rules of a closed
    /// UDP ([`Deny::udp`]), of a TCP list ([`Deny::mptcp`]), and the
    /// io_uring calls where the spec closes a socket ([`Deny::io_uring`]).
    ///
    /// A closed UDP is refused on a target with `socketcall(2)` (i686,
    /// s390x, powerpc64): a payload makes a UDP socket through it, and a
    /// deny of each `socketcall` stops each socket of a payload that makes
    /// its sockets only through it (Go on 386 and s390x; glibc on i386 for
    /// a kernel under 4.3).
    pub fn deny_rules(&self) -> Result<Vec<Deny>, Failure> {
        self.deny_rules_on(SOCKETCALL)
    }

    pub(crate) fn deny_rules_on(&self, socketcall: bool) -> Result<Vec<Deny>, Failure> {
        let mut rules = self.deny.clone();
        let tcp = self.connect.is_some() || self.bind.is_some();
        if !self.udp {
            if socketcall {
                return Err(failure(
                    Class::Unsupported,
                    "the spec closes UDP, and the kernel of this target has socketcall(2): a filter cannot read \
                     the family and the type of a socket made through it, so UDP would stay open",
                ));
            }
            rules.extend(Deny::udp());
        }
        if tcp {
            rules.extend(Deny::mptcp());
        }
        if !self.udp || tcp {
            rules.extend(Deny::io_uring());
        }
        Ok(rules)
    }
}

/// Open the path of one rule, or refuse it by name. The path comes back as
/// a C string for an open in the child.
fn open_rule(path: &str, access: Access, open: &impl Fn(&CBuf) -> Result<i64, Errno>) -> Result<(i64, CBuf), Failure> {
    if access.is_empty() {
        return Err(failure(Class::Usage, format!("the path {path:?} of the spec has no access")));
    }
    if !path.starts_with('/') {
        return Err(failure(Class::Usage, format!("the path {path:?} of the spec is not absolute")));
    }
    let c = CBuf::new(path).ok_or_else(|| failure(Class::Usage, format!("the path {path:?} contains a NUL byte")))?;
    let fd = open(&c).map_err(|e| {
        let what = if e == k::ENOENT { "does not exist" } else { "cannot be opened" };
        failure(errno_class(e), format!("the path {path:?} ({}) of the spec {what}: {}", access.words(), errno_text(e)))
    })?;
    Ok((fd, c))
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

//! What a payload may do: the access set of each path, the TCP ports of
//! `connect` and `bind`, the Landlock scopes, UDP, and the calls that fail.
//! A profile of a consumer maps to a [`Spec`]; [`Spec::compile`] turns it
//! into the data of [`crate::apply`].

use pod_hostprobe::sys as k;

use crate::deny::Deny;

/// The access set of one path: read, write, execute, or a union of them
/// (`Access::READ | Access::EXECUTE`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Access(u8);

impl Access {
    /// No access. A rule with no access is refused at the compile.
    pub const NONE: Access = Access(0);
    /// Read a file, list a directory.
    pub const READ: Access = Access(1);
    /// Write, truncate, make, remove, rename and link; an `ioctl` on a
    /// device.
    pub const WRITE: Access = Access(2);
    /// Execute a file.
    pub const EXECUTE: Access = Access(4);

    /// True when each access of `other` is in `self`.
    pub fn contains(self, other: Access) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The words of the set, for a refusal: `read+execute`.
    pub fn words(self) -> String {
        let words: Vec<&str> = [(Access::READ, "read"), (Access::WRITE, "write"), (Access::EXECUTE, "execute")]
            .into_iter()
            .filter(|(a, _)| self.contains(*a))
            .map(|(_, w)| w)
            .collect();
        if words.is_empty() { "none".to_string() } else { words.join("+") }
    }

    /// The Landlock rights of the set for a directory, at each ABI. The
    /// compile masks them with the rights that the ABI handles.
    pub fn rights(self) -> u64 {
        let mut r = 0;
        if self.contains(Access::READ) {
            r |= READ_RIGHTS;
        }
        if self.contains(Access::WRITE) {
            r |= WRITE_RIGHTS;
        }
        if self.contains(Access::EXECUTE) {
            r |= u64::from(k::LANDLOCK_ACCESS_FS_EXECUTE);
        }
        r
    }
}

impl std::ops::BitOr for Access {
    type Output = Access;
    fn bitor(self, other: Access) -> Access {
        Access(self.0 | other.0)
    }
}

const READ_RIGHTS: u64 = (k::LANDLOCK_ACCESS_FS_READ_FILE | k::LANDLOCK_ACCESS_FS_READ_DIR) as u64;

const WRITE_RIGHTS: u64 = (k::LANDLOCK_ACCESS_FS_WRITE_FILE
    | k::LANDLOCK_ACCESS_FS_REMOVE_DIR
    | k::LANDLOCK_ACCESS_FS_REMOVE_FILE
    | k::LANDLOCK_ACCESS_FS_MAKE_CHAR
    | k::LANDLOCK_ACCESS_FS_MAKE_DIR
    | k::LANDLOCK_ACCESS_FS_MAKE_REG
    | k::LANDLOCK_ACCESS_FS_MAKE_SOCK
    | k::LANDLOCK_ACCESS_FS_MAKE_FIFO
    | k::LANDLOCK_ACCESS_FS_MAKE_BLOCK
    | k::LANDLOCK_ACCESS_FS_MAKE_SYM
    | k::LANDLOCK_ACCESS_FS_REFER
    | k::LANDLOCK_ACCESS_FS_TRUNCATE
    | k::LANDLOCK_ACCESS_FS_IOCTL_DEV) as u64;

/// The rights that a rule on a file (not a directory) can hold; the kernel
/// refuses each other right on a file with `EINVAL`.
pub const FILE_RIGHTS: u64 = (k::LANDLOCK_ACCESS_FS_EXECUTE
    | k::LANDLOCK_ACCESS_FS_WRITE_FILE
    | k::LANDLOCK_ACCESS_FS_READ_FILE
    | k::LANDLOCK_ACCESS_FS_TRUNCATE
    | k::LANDLOCK_ACCESS_FS_IOCTL_DEV) as u64;

/// The file rights that the kernel of `abi` knows: the ruleset handles each
/// of them, so a right that no rule gives is denied. ABI 1 has the first
/// thirteen, ABI 2 adds `REFER`, ABI 3 `TRUNCATE`, ABI 5 `IOCTL_DEV`.
pub fn handled_fs(abi: u32) -> u64 {
    if abi == 0 {
        return 0;
    }
    let mut r = (k::LANDLOCK_ACCESS_FS_MAKE_SYM << 1) as u64 - 1;
    if abi >= 2 {
        r |= u64::from(k::LANDLOCK_ACCESS_FS_REFER);
    }
    if abi >= 3 {
        r |= u64::from(k::LANDLOCK_ACCESS_FS_TRUNCATE);
    }
    if abi >= 5 {
        r |= u64::from(k::LANDLOCK_ACCESS_FS_IOCTL_DEV);
    }
    r
}

/// The right of a TCP `connect` and of a TCP `bind` (ABI 4).
pub const CONNECT_TCP: u64 = k::LANDLOCK_ACCESS_NET_CONNECT_TCP as u64;
pub const BIND_TCP: u64 = k::LANDLOCK_ACCESS_NET_BIND_TCP as u64;

/// The scopes of [`Spec::scope`] (ABI 6): a signal to a process outside
/// the domain, and a connect or a send to an abstract Unix socket made
/// outside it.
pub const SCOPES: u64 = (k::LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET | k::LANDLOCK_SCOPE_SIGNAL) as u64;

/// One path and its access set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathRule {
    /// An absolute path: of the host for [`Spec::compile`], of the guest
    /// root for [`Spec::compile_in`].
    pub path: String,
    pub access: Access,
}

/// The capabilities that docker keeps for a container by default:
/// `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `FSETID`, `KILL`, `SETGID`,
/// `SETUID`, `SETPCAP`, `NET_BIND_SERVICE`, `NET_RAW`, `SYS_CHROOT`,
/// `MKNOD`, `AUDIT_WRITE` and `SETFCAP`. A root payload that installs
/// packages needs some of them; give it to [`Spec::keep_caps`].
pub const DOCKER_CAPS: u64 = (1 << 0)
    | (1 << 1)
    | (1 << 3)
    | (1 << 4)
    | (1 << 5)
    | (1 << 6)
    | (1 << 7)
    | (1 << 8)
    | (1 << 10)
    | (1 << 13)
    | (1 << 18)
    | (1 << 27)
    | (1 << 29)
    | (1 << 31);

/// The confinement of a payload.
///
/// Each path that no rule names, and each path under it, is denied for
/// each access (the ruleset handles each file right of the ABI). A rule on
/// a directory covers the tree under it. A TCP list that is `None` leaves
/// that call free; `Some` allows only its ports (an empty list denies each
/// port), and an MPTCP socket fails ([`Deny::mptcp`]). Landlock does not
/// restrict UDP or a Unix socket of a path: `udp` closes UDP.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub paths: Vec<PathRule>,
    /// The ports of a TCP `connect`.
    pub connect: Option<Vec<u16>>,
    /// The ports of a TCP `bind`.
    pub bind: Option<Vec<u16>>,
    /// The Landlock scopes ([`SCOPES`]): the payload cannot signal a
    /// process outside its domain, and cannot connect to an abstract Unix
    /// socket made outside it (`EPERM`). It needs ABI 6 (Linux 6.12);
    /// under ABI 6 the field gives nothing ([`crate::Compiled::scoped`]).
    pub scope: bool,
    /// True: UDP is free. False: a UDP socket fails ([`Deny::udp`], with
    /// `EACCES`) and the io_uring calls fail ([`Deny::io_uring`], with
    /// `EPERM`). The compile refuses it on a target with `socketcall(2)`.
    pub udp: bool,
    /// The calls that fail, with their errno ([`Deny::defaults`]).
    pub deny: Vec<Deny>,
    /// The capabilities that the payload keeps, as a mask (bit `n` is
    /// capability `n`). Each other one goes out of the bounding set (where
    /// the thread holds `CAP_SETPCAP`) and out of the effective, permitted,
    /// inheritable and ambient sets. With `no_new_privs`, an `execve` as
    /// root gives no more than the kept set. The default is
    /// [`DOCKER_CAPS`] (the owner's ruling "Root caps": the most
    /// convenience): a root payload that installs packages works, and the
    /// set has neither `CAP_SYS_ADMIN` nor `CAP_PERFMON`, with which a root
    /// reads the environment of its unconfined ancestors past the Landlock
    /// domain. [`Spec::no_caps`] keeps none.
    pub keep_caps: u64,
}

impl Default for Spec {
    fn default() -> Spec {
        Spec::new()
    }
}

impl Spec {
    /// No path, TCP and UDP free, the scopes on, the default deny list,
    /// and docker's capabilities ([`DOCKER_CAPS`]).
    pub fn new() -> Spec {
        Spec {
            paths: Vec::new(),
            connect: None,
            bind: None,
            scope: true,
            udp: true,
            deny: Deny::defaults(),
            keep_caps: DOCKER_CAPS,
        }
    }

    /// Add a path with its access set.
    pub fn allow(mut self, path: impl Into<String>, access: Access) -> Spec {
        self.paths.push(PathRule { path: path.into(), access });
        self
    }

    /// Allow a TCP `connect` to `port` (and restrict each other port).
    pub fn connect_to(mut self, port: u16) -> Spec {
        self.connect.get_or_insert_with(Vec::new).push(port);
        self
    }

    /// Allow a TCP `bind` to `port` (and restrict each other port).
    pub fn bind_to(mut self, port: u16) -> Spec {
        self.bind.get_or_insert_with(Vec::new).push(port);
        self
    }

    /// Close UDP ([`Spec::udp`]).
    pub fn close_udp(mut self) -> Spec {
        self.udp = false;
        self
    }

    /// Leave the scopes off ([`Spec::scope`]): the payload can signal each
    /// process of its user and connect to each abstract Unix socket.
    pub fn no_scope(mut self) -> Spec {
        self.scope = false;
        self
    }

    /// Keep no capability ([`Spec::keep_caps`] 0); [`Spec::keep_cap`] then
    /// adds one at a time.
    pub fn no_caps(mut self) -> Spec {
        self.keep_caps = 0;
        self
    }

    /// Keep the capability `cap` too (0 to 63; [`Spec::keep_caps`]).
    pub fn keep_cap(mut self, cap: u32) -> Spec {
        self.keep_caps |= 1u64.checked_shl(cap).unwrap_or(0);
        self
    }
}

#[cfg(test)]
mod tests;

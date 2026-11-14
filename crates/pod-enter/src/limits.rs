//! The resource limits and `no_new_privs` of a payload, set in its child
//! after the root change and the `chdir`, before the exec: they reach the
//! payload and no other process. A limit such as `fsize` set in the caller
//! would limit the caller's own writes, and `no_new_privs` cannot be undone.

use pod_hostprobe::sys::rlimit;

/// A resource of `setrlimit(2)`, by the name of docker's `--ulimit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Cpu,
    Fsize,
    Data,
    Stack,
    Core,
    Nofile,
    Nproc,
    As,
    Memlock,
    Msgqueue,
    Nice,
    Rtprio,
    Locks,
    Sigpending,
    Rttime,
}

const ALL: [(Resource, &str, u64, &str); 15] = [
    (Resource::Cpu, "cpu", rlimit::CPU, "RLIMIT_CPU"),
    (Resource::Fsize, "fsize", rlimit::FSIZE, "RLIMIT_FSIZE"),
    (Resource::Data, "data", rlimit::DATA, "RLIMIT_DATA"),
    (Resource::Stack, "stack", rlimit::STACK, "RLIMIT_STACK"),
    (Resource::Core, "core", rlimit::CORE, "RLIMIT_CORE"),
    (Resource::Nofile, "nofile", rlimit::NOFILE, "RLIMIT_NOFILE"),
    (Resource::Nproc, "nproc", rlimit::NPROC, "RLIMIT_NPROC"),
    (Resource::As, "as", rlimit::AS, "RLIMIT_AS"),
    (Resource::Memlock, "memlock", rlimit::MEMLOCK, "RLIMIT_MEMLOCK"),
    (Resource::Msgqueue, "msgqueue", rlimit::MSGQUEUE, "RLIMIT_MSGQUEUE"),
    (Resource::Nice, "nice", rlimit::NICE, "RLIMIT_NICE"),
    (Resource::Rtprio, "rtprio", rlimit::RTPRIO, "RLIMIT_RTPRIO"),
    (Resource::Locks, "locks", rlimit::LOCKS, "RLIMIT_LOCKS"),
    (Resource::Sigpending, "sigpending", rlimit::SIGPENDING, "RLIMIT_SIGPENDING"),
    (Resource::Rttime, "rttime", rlimit::RTTIME, "RLIMIT_RTTIME"),
];

impl Resource {
    fn row(self) -> &'static (Resource, &'static str, u64, &'static str) {
        ALL.iter().find(|r| r.0 == self).expect("each resource has a row")
    }

    /// The resource of docker's `--ulimit NAME`, for example `nofile`.
    pub fn from_word(word: &str) -> Option<Resource> {
        ALL.iter().find(|r| r.1 == word).map(|r| r.0)
    }

    /// The word of docker's `--ulimit`.
    pub fn word(self) -> &'static str {
        self.row().1
    }

    /// The number of the kernel of this architecture.
    pub fn number(self) -> u64 {
        self.row().2
    }

    /// The name of the kernel, for a refusal: `RLIMIT_NOFILE`.
    pub fn name(self) -> &'static str {
        self.row().3
    }
}

/// No limit: `RLIM64_INFINITY`.
pub const INFINITY: u64 = rlimit::INFINITY;

/// The most limits of one plan: the child names a refused one by its index
/// in one byte.
pub const MOST: usize = 64;

/// One limit of the payload: the soft and the hard value ([`INFINITY`]:
/// none). A hard value over the hard value of the caller needs a
/// privilege, and the child refuses it by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub resource: Resource,
    pub soft: u64,
    pub hard: u64,
}

/// The resource of the first limit that `limits` gives two times, or
/// `None`. The entry refuses such a plan with [`crate::Error::LimitTwice`]:
/// the child applies the limits in order, and a first entry that lowers the
/// hard value makes the second fail with `EPERM`. The caller coalesces the
/// entries first (docker keeps the last one).
pub fn repeated(limits: &[Limit]) -> Option<Resource> {
    limits
        .iter()
        .enumerate()
        .find(|(at, l)| limits[..*at].iter().any(|e| e.resource == l.resource))
        .map(|(_, l)| l.resource)
}

#[cfg(test)]
mod tests;

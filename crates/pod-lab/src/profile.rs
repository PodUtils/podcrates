//! The types of a profile: its parts, what observes each part, and what
//! holds each part in a lab.

pub use crate::net::{Ca, Net, NetFact, Proxy};

/// One class of hostile host.
#[derive(Debug)]
pub struct Profile {
    /// The name on the command line: `bind-denied`.
    pub name: &'static str,
    /// One line for `podlab list`.
    pub about: &'static str,
    /// The consumer that asked for the profile: `podrunner`.
    pub source: &'static str,
    pub parts: &'static [Part],
}

/// One fact of a profile.
#[derive(Debug)]
pub struct Part {
    /// The name of the part in a verdict: `tcp-bind-refused`.
    pub name: &'static str,
    pub about: &'static str,
    /// Each observation must hold for the part to hold.
    pub observe: &'static [Observe],
    /// What makes the part hold in a lab ([`crate::run`]).
    pub hold: Hold,
}

/// One observation of a part.
#[derive(Debug)]
pub enum Observe {
    /// A row of `pod-hostprobe` (`pod_hostprobe::probes::PROBES`, by name).
    Row { row: &'static str, expect: Expect },
    /// A fact of the identity of the report (`pod_hostprobe::identity`).
    Identity(IdFact),
    /// A fact that no row of `pod-hostprobe` measures. A lab measures it
    /// inside itself before the command; a check of a report cannot
    /// observe it. [`LabFact::entry`] names the entry that adds the row.
    Lab(LabFact),
    /// A fact that a lab measures inside itself beside the rows of the
    /// part, to check its own stand-ins where a row reads less than the
    /// fact (the routes of the main table, not a connect out). A check of
    /// a report leaves it out: the rows of the part see the class on a
    /// host.
    LabCheck(LabFact),
    /// A fact that nothing in this crate measures: `needs` names the
    /// probe, `entry` the number of the entry of the record that adds it
    /// (692: T-692). The number is not printed.
    Nothing { needs: &'static str, entry: u32 },
}

/// What a row must give.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// The kernel refuses the operation with one of these errno numbers.
    Refused(&'static [i32]),
    /// The operation succeeds.
    Allowed,
    /// The row `landlock_create_ruleset(VERSION)`: no Landlock (`ENOSYS`,
    /// `EOPNOTSUPP`), or an ABI below this one.
    LandlockBelow(u32),
    /// A row of a fact: `ok`, and the item `key` of its reason is `value`
    /// (`pod_hostprobe::verdict::Outcome::value`). Another value, or a
    /// refusal, contradicts the part.
    Value { key: &'static str, value: &'static str },
    /// A row of a fact that shows the class where the item `key` is
    /// `value`. Another answer does not contradict the part, as the host
    /// can hold the class in a way that the row does not read (a filter in
    /// place of no route): the observation is then unseen.
    Shows { key: &'static str, value: &'static str },
}

/// A fact of the identity of a process (`/proc/self/status` and the ids).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdFact {
    /// The uid is not 0.
    NotRoot,
    /// `CapEff` is 0.
    NoCaps,
    /// `NoNewPrivs` is 1.
    NoNewPrivs,
    /// `Seccomp` is 2 (a filter).
    SeccompFilter,
    /// `CapEff` has no `CAP_NET_ADMIN` (bit 12).
    NoNetAdmin,
}

impl IdFact {
    pub fn name(self) -> &'static str {
        match self {
            IdFact::NotRoot => "uid is not 0",
            IdFact::NoCaps => "CapEff is 0",
            IdFact::NoNewPrivs => "NoNewPrivs is 1",
            IdFact::SeccompFilter => "Seccomp is 2",
            IdFact::NoNetAdmin => "CapEff has no CAP_NET_ADMIN",
        }
    }
}

/// A fact that a lab measures inside itself: no row of `pod-hostprobe`
/// measures it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabFact {
    /// `TMPDIR` is a file system of at most this many bytes.
    TmpdirAtMost(u64),
    /// The descriptors 3 to `last` are open, and none is close-on-exec.
    FdsOpen { last: i32 },
    /// A TCP bind of `[::1]:0` fails with `EACCES`.
    Inet6BindRefused,
    /// An AF_UNIX `socket`, a `bind` of an abstract name and a `connect`
    /// to it succeed.
    UnixAllowed,
    /// `getsockname(2)` of a TCP socket fails with `EOPNOTSUPP`.
    GetsocknameRefused,
    /// A fact of a network class, through the stand-ins of the lab.
    Net(NetFact),
}

impl LabFact {
    /// The fact in words, for a verdict.
    pub fn name(self) -> String {
        match self {
            LabFact::TmpdirAtMost(n) => format!("TMPDIR holds at most {n} bytes"),
            LabFact::FdsOpen { last } => format!("descriptors 3 to {last} open, not close-on-exec"),
            LabFact::Inet6BindRefused => "bind([::1]:0) gives EACCES".to_string(),
            LabFact::UnixAllowed => "socket, bind and connect of AF_UNIX succeed".to_string(),
            LabFact::GetsocknameRefused => "getsockname(TCP socket) gives EOPNOTSUPP".to_string(),
            LabFact::Net(f) => f.name(),
        }
    }

    /// The number of the entry of the record that adds a row of
    /// `pod-hostprobe` for the fact (693: T-693), so that a check of a
    /// report observes it. The number is not printed.
    pub fn entry(self) -> u32 {
        match self {
            LabFact::TmpdirAtMost(_) | LabFact::FdsOpen { .. } => 694,
            LabFact::Inet6BindRefused | LabFact::UnixAllowed | LabFact::GetsocknameRefused => 693,
            LabFact::Net(_) => 751,
        }
    }
}

/// One call that a seccomp filter of a lab refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Call {
    pub name: &'static str,
    /// The number of the call on the build target (`pod_hostprobe::sys`).
    pub nr: i64,
    /// The errno of the refusal.
    pub errno: i32,
    /// Refuse only where each condition holds; empty: each call.
    pub when: &'static [When],
}

/// A condition of a [`Call`] on the low 32 bits of one argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    /// Argument `arg` has a bit of `mask`.
    Bits { arg: u8, mask: u32 },
    /// Argument `arg`, masked with `mask`, is `value`.
    Equal { arg: u8, mask: u32, value: u32 },
}

/// What makes a part hold in a lab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    /// A seccomp filter refuses these calls (through `pod-confine`).
    Seccomp(&'static [Call]),
    /// The default deny list of `pod-confine` (`pod_confine::Deny::defaults`).
    ConfineDefaults,
    /// A Landlock ruleset with no port rule refuses each TCP bind and each
    /// TCP connect (ABI 4).
    LandlockTcp,
    /// A Landlock ruleset with no port rule refuses each TCP bind; a TCP
    /// connect passes (ABI 4).
    LandlockBind,
    /// A Landlock ruleset handles `rights` and gives them to each path
    /// except `path` and the directories above it.
    Hide { path: &'static str, rights: u64 },
    /// A tmpfs on `/tmp` in a mount namespace of the lab, and `TMPDIR` on
    /// it.
    Tmpfs { size: Option<u64>, noexec: bool },
    /// The descriptors 3 to `last` open (`/dev/null`) for the command.
    Fds { last: i32 },
    /// The caller is not root: the lab cannot drop root.
    NonRoot,
    /// `PR_SET_NO_NEW_PRIVS`.
    NoNewPrivs,
    /// No capability: the bounding set, the ambient set and the three sets
    /// of the lab are empty, and `no_new_privs` is set.
    DropCaps,
    /// A new session: the lab has no controlling terminal.
    NoCtty,
    /// A tmpfs on `/dev` with `null`, `zero`, `full`, `random` and
    /// `urandom` of the host, and a FIFO at `/dev/tty`: no `/dev/ptmx`, no
    /// `/dev/pts`.
    DevNoPty,
    /// `/etc/passwd` and `/etc/group` with no line of the ids of the caller,
    /// bound over the files of the host.
    NoPasswdEntry,
    /// A bind of `HOME` over itself with `noexec`.
    NoexecHome,
    /// A read-only empty tmpfs on `/proc`.
    NoProc,
    /// A bind of `path` over itself, read-only.
    ReadOnly { path: &'static str },
    /// A new network namespace: its loopback is down and has no address.
    NetNs,
    /// A new network namespace with its loopback up, and the stand-ins of
    /// the network lab on it.
    LabNet(&'static Net),
    /// The host gives it; the lab changes nothing.
    Host,
    /// No lab of this crate holds it: the text names who holds it.
    Elsewhere(&'static str),
}

impl Hold {
    /// The mechanism in words, for a verdict and for `podlab list`.
    pub fn words(self) -> String {
        match self {
            Hold::Seccomp(calls) => {
                let names: Vec<&str> = calls.iter().map(|c| c.name).collect();
                format!("seccomp: {}", names.join(", "))
            }
            Hold::ConfineDefaults => "seccomp: the default deny list of pod-confine".to_string(),
            Hold::LandlockTcp => "Landlock: no TCP port".to_string(),
            Hold::LandlockBind => "Landlock: no TCP bind".to_string(),
            Hold::Hide { path, .. } => format!("Landlock: {path} hidden"),
            Hold::Tmpfs { size: Some(n), noexec } => format!("tmpfs of {n} bytes on /tmp{}", noexec_word(noexec)),
            Hold::Tmpfs { size: None, noexec } => format!("tmpfs on /tmp{}", noexec_word(noexec)),
            Hold::Fds { last } => format!("descriptors 3 to {last}"),
            Hold::NonRoot => "the caller is not root".to_string(),
            Hold::NoNewPrivs => "no_new_privs".to_string(),
            Hold::DropCaps => "no capability, no_new_privs".to_string(),
            Hold::NoCtty => "a new session with no controlling terminal".to_string(),
            Hold::DevNoPty => "a tmpfs on /dev with no ptmx and no pts, a FIFO at /dev/tty".to_string(),
            Hold::NoPasswdEntry => "/etc/passwd and /etc/group with no line of the caller".to_string(),
            Hold::NoexecHome => "HOME bound noexec".to_string(),
            Hold::NoProc => "an empty tmpfs on /proc".to_string(),
            Hold::ReadOnly { path } => format!("{path} bound read-only"),
            Hold::NetNs => "a network namespace with no loopback".to_string(),
            Hold::LabNet(net) => net.words(),
            Hold::Host => "the host".to_string(),
            Hold::Elsewhere(who) => format!("not here: {who}"),
        }
    }

    /// True where a lab of this crate can hold the part.
    pub fn here(self) -> bool {
        !matches!(self, Hold::Elsewhere(_))
    }
}

fn noexec_word(noexec: bool) -> &'static str {
    if noexec { ", noexec" } else { "" }
}

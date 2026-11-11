//! The apply in the child: `no_new_privs`, the Landlock ruleset and its
//! rules, the restriction, then the seccomp filter. No allocation and no
//! lock: each step is one system call on the data of a [`Compiled`].

use std::fmt;

use pod_core::Failure;
use pod_hostprobe::sys::{self as k, Errno};

use crate::{Compiled, errno_class, errno_text, failure};

/// The call of [`apply`] that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    NoNewPrivs,
    CreateRuleset,
    AddPathRule,
    AddPortRule,
    RestrictSelf,
    Seccomp,
}

impl Call {
    const ALL: [Call; 6] = [
        Call::NoNewPrivs,
        Call::CreateRuleset,
        Call::AddPathRule,
        Call::AddPortRule,
        Call::RestrictSelf,
        Call::Seccomp,
    ];

    /// The call as a reader knows it.
    pub fn name(self) -> &'static str {
        match self {
            Call::NoNewPrivs => "prctl(PR_SET_NO_NEW_PRIVS)",
            Call::CreateRuleset => "landlock_create_ruleset",
            Call::AddPathRule => "landlock_add_rule(LANDLOCK_RULE_PATH_BENEATH)",
            Call::AddPortRule => "landlock_add_rule(LANDLOCK_RULE_NET_PORT)",
            Call::RestrictSelf => "landlock_restrict_self",
            Call::Seccomp => "seccomp(SECCOMP_SET_MODE_FILTER)",
        }
    }

    /// One byte for a pipe from a child: 1 to 6.
    pub fn code(self) -> u8 {
        Call::ALL.iter().position(|c| *c == self).map_or(0, |i| i as u8 + 1)
    }

    /// The call of a byte of [`Call::code`].
    pub fn from_code(code: u8) -> Option<Call> {
        Call::ALL.get(usize::from(code).checked_sub(1)?).copied()
    }
}

/// A refusal of [`apply`]: the call, the index of its rule (a path rule or
/// a port rule; 0 for the other calls) and the errno. It holds no heap
/// data, so a child after a fork can return it and write it to a pipe
/// ([`Refusal::to_bytes`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub call: Call,
    pub index: u16,
    pub errno: Errno,
}

impl Refusal {
    /// Five bytes: the call, the errno (16 bits) and the index (16 bits),
    /// little-endian.
    pub fn to_bytes(self) -> [u8; 5] {
        let e = (self.errno.0 as u16).to_le_bytes();
        let i = self.index.to_le_bytes();
        [self.call.code(), e[0], e[1], i[0], i[1]]
    }

    /// The refusal of [`Refusal::to_bytes`], or `None` for a call byte that
    /// names no call.
    pub fn from_bytes(b: [u8; 5]) -> Option<Refusal> {
        let call = Call::from_code(b[0])?;
        Some(Refusal {
            call,
            errno: Errno(i32::from(u16::from_le_bytes([b[1], b[2]]))),
            index: u16::from_le_bytes([b[3], b[4]]),
        })
    }

    /// The refusal with the path or the port of its rule, from the
    /// compiled spec that the child applied.
    pub fn describe(&self, compiled: &Compiled) -> String {
        let at = usize::from(self.index);
        let rule = match self.call {
            Call::AddPathRule => compiled.rules.get(at).map(|r| format!(" for the path {:?}", r.path())),
            Call::AddPortRule => compiled.ports.get(at).map(|(p, _)| format!(" for the TCP port {p}")),
            _ => None,
        };
        format!("{}{}: {}", self.call.name(), rule.unwrap_or_default(), errno_text(self.errno))
    }

    /// The failure of the refusal, at the stage `host`, with the class of
    /// its errno.
    pub fn failure(&self, compiled: &Compiled) -> Failure {
        failure(errno_class(self.errno), format!("the confinement failed at {}", self.describe(compiled)))
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (rule {}): {} ({})", self.call.name(), self.index, self.errno.name(), self.errno.0)
    }
}

/// Confine this thread and each process that it makes later, for good.
///
/// In order: `PR_SET_NO_NEW_PRIVS` (each `execve` after it gives no
/// privilege; Landlock and seccomp need it in a task with no
/// `CAP_SYS_ADMIN`), the ruleset with the handled rights and the scopes,
/// each path rule, each port rule, `landlock_restrict_self`, then the
/// seccomp filter in mode 2. The first failure returns, and the caller must not exec: a
/// kernel with no Landlock fails at `landlock_create_ruleset`.
///
/// Async-signal-safe: no allocation, no lock, no `std` I/O. Call it in the
/// child between its fork and its exec, as the last step before the exec.
pub fn apply(c: &Compiled) -> Result<(), Refusal> {
    let refuse = |call: Call, index: usize, errno: Errno| Refusal { call, index: index as u16, errno };
    k::set_no_new_privs().map_err(|e| refuse(Call::NoNewPrivs, 0, e))?;
    let attr =
        k::LandlockRulesetAttr { handled_access_fs: c.handled_fs, handled_access_net: c.handled_net, scoped: c.scoped };
    let ruleset = k::landlock_create_ruleset(&attr).map_err(|e| refuse(Call::CreateRuleset, 0, e))?;
    let restricted = add_rules(c, ruleset)
        .and_then(|()| k::landlock_restrict_self(ruleset).map(drop).map_err(|e| refuse(Call::RestrictSelf, 0, e)));
    let _ = k::close(ruleset);
    restricted?;
    k::seccomp_set_filter(&c.filter, 0).map_err(|e| refuse(Call::Seccomp, 0, e))?;
    Ok(())
}

fn add_rules(c: &Compiled, ruleset: i64) -> Result<(), Refusal> {
    for (i, r) in c.rules.iter().enumerate() {
        let rule = k::LandlockPathBeneathAttr { allowed_access: r.allowed, parent_fd: r.fd as i32 };
        if let Err(errno) = k::landlock_add_path_rule(ruleset, &rule) {
            return Err(Refusal { call: Call::AddPathRule, index: i as u16, errno });
        }
    }
    for (i, &(port, allowed)) in c.ports.iter().enumerate() {
        let rule = k::LandlockNetPortAttr { allowed_access: allowed, port: u64::from(port) };
        if let Err(errno) = k::landlock_add_port_rule(ruleset, &rule) {
            return Err(Refusal { call: Call::AddPortRule, index: i as u16, errno });
        }
    }
    Ok(())
}

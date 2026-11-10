//! The level of a hold, and its proof.

use std::fmt;

/// How sure the hold is that each connect of the payload goes through it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    /// The kernel enforces the hold: the payload has no other way out (a
    /// network namespace whose only route is the TUN of the supervisor).
    Enforced,
    /// The supervisor answers the calls that a filter stops; a payload can
    /// pass around it (an inherited descriptor, a call or a family that
    /// the filter does not stop).
    Advisory,
    /// No hold.
    None,
}

impl Level {
    /// The word of the level: `enforced`, `advisory` or `none`.
    pub fn word(self) -> &'static str {
        match self {
            Level::Enforced => "enforced",
            Level::Advisory => "advisory",
            Level::None => "none",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// What a hold gives a consumer: its level, the proof (what the supervisor
/// did, and the kernel accepted), and what the hold does not hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hold {
    pub level: Level,
    pub proof: String,
    pub not_held: Vec<&'static str>,
}

/// What a seccomp hold does not hold.
pub const SECCOMP_NOT_HELD: [&str; 3] = [
    "descriptors that the payload inherits at its start",
    "the system calls that the filter does not stop",
    "a socket family that the handler gives back to the kernel",
];

/// What a netns hold does not hold.
pub const NETNS_NOT_HELD: [&str; 1] = ["descriptors that the payload inherits at its start"];

impl Hold {
    /// No hold, for the reason `why`.
    pub fn none(why: impl Into<String>) -> Hold {
        Hold { level: Level::None, proof: why.into(), not_held: vec!["each connect"] }
    }

    /// The seccomp hold of the task `pid` with the filter of `calls`
    /// stopped calls.
    pub fn seccomp(pid: i32, calls: usize) -> Hold {
        Hold {
            level: Level::Advisory,
            proof: format!(
                "seccomp user notification: the kernel accepted a filter with no_new_privs that stops \
                 {calls} system calls of pid {pid} and of each of its children"
            ),
            not_held: SECCOMP_NOT_HELD.to_vec(),
        }
    }

    /// The netns hold of the task `pid`: a new network namespace (in a new
    /// user namespace when `userns`) whose only route is the TUN `tun`.
    pub fn netns(pid: i32, userns: bool, tun: &str) -> Hold {
        let flags = if userns { "CLONE_NEWUSER|CLONE_NEWNET" } else { "CLONE_NEWNET" };
        Hold {
            level: Level::Enforced,
            proof: format!(
                "network namespace: unshare({flags}) in pid {pid}; the only route is the TUN {tun} of the supervisor"
            ),
            not_held: NETNS_NOT_HELD.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_hold_names_its_level_and_its_proof() {
        assert_eq!(Level::Enforced.to_string(), "enforced");
        assert_eq!(Level::Advisory.word(), "advisory");
        assert_eq!(Level::None.word(), "none");
        let hold = Hold::seccomp(42, 21);
        assert_eq!(hold.level, Level::Advisory);
        assert!(hold.proof.contains("21 system calls of pid 42"), "{}", hold.proof);
        assert!(hold.not_held.iter().any(|n| n.contains("inherits")));
        let hold = Hold::netns(7, true, "eth0");
        assert_eq!(hold.level, Level::Enforced);
        assert!(hold.proof.contains("CLONE_NEWUSER|CLONE_NEWNET") && hold.proof.contains("eth0"));
        assert_eq!(Hold::none("no seccomp").level, Level::None);
    }
}

//! The mechanisms that hold the parts of a lab, in the lab process before
//! the command: the descriptors, the session, the Landlock rulesets,
//! `no_new_privs`, the drop of the capabilities and the seccomp filter.
//! [`crate::mounts`] holds the mount namespace.
//!
//! The lab process has one thread (a fresh exec of the binary), so it can
//! enter a user namespace. Each function gives the reason of a refusal in
//! words; the part then does not hold.

use pod_confine::deny::{Deny, program};
use pod_confine::spec::FILE_RIGHTS;
use pod_hostprobe::sys::{self as k, CBuf, Errno};

use crate::profile::{Call, Hold, When};

pub(crate) fn c(s: &str) -> Result<CBuf, String> {
    CBuf::new(s).ok_or_else(|| format!("the path {s:?} holds a NUL"))
}

pub(crate) fn words(call: &str, e: Errno) -> String {
    format!("{call}: {} ({})", e.name(), e.0)
}

pub(crate) fn euid() -> u32 {
    // SAFETY: no pointer; geteuid always succeeds.
    unsafe { k::sys(k::SYS_GETEUID, [0; 6]) }.map_or(0, |n| n as u32)
}

pub(crate) fn egid() -> u32 {
    // SAFETY: as above.
    unsafe { k::sys(k::SYS_GETEGID, [0; 6]) }.map_or(0, |n| n as u32)
}

/// Write `bytes` into `path` under the directory `dir` in one `write(2)`,
/// as the kernel takes a map.
pub(crate) fn write_once(dir: i64, path: &str, bytes: &[u8]) -> Result<(), String> {
    let fd = k::openat(dir, &c(path)?, k::O_WRONLY | k::O_CLOEXEC, 0).map_err(|e| words(path, e))?;
    let n = k::write(fd, bytes);
    let _ = k::close(fd);
    match n {
        Ok(n) if n as usize == bytes.len() => Ok(()),
        Ok(_) => Err(format!("{path}: a short write")),
        Err(e) => Err(words(path, e)),
    }
}

/// Close each descriptor from 3 up except `keep`: the lab starts from a
/// known set, whatever its parent left open.
pub fn close_from_3(keep: i64) {
    let open: Vec<i64> = match std::fs::read_dir("/proc/self/fd") {
        Ok(dir) => dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok()).collect(),
        Err(_) => (3..1024).collect(),
    };
    for fd in open.into_iter().filter(|&fd| fd >= 3 && fd != keep) {
        let _ = k::close(fd);
    }
}

/// Open `/dev/null` at each descriptor from 3 to `last`, not close-on-exec.
pub fn fds(last: i32) -> Result<(), String> {
    let null = c("/dev/null")?;
    for n in 3..=i64::from(last) {
        let fd = k::open(&null, k::O_RDWR, 0).map_err(|e| words("open(/dev/null)", e))?;
        if fd != n {
            k::dup2(fd, n).map_err(|e| words("dup2", e))?;
            let _ = k::close(fd);
        }
    }
    Ok(())
}

/// A ruleset of `rights` (file) and `net`, restricted on this process; a
/// TCP connect to each port of `connect` stays allowed.
fn ruleset(fs: u64, net: u64, rules: &[(i64, u64)], connect: &[u16]) -> Result<(), String> {
    let attr = k::LandlockRulesetAttr { handled_access_fs: fs, handled_access_net: net, scoped: 0 };
    let set = k::landlock_create_ruleset(&attr).map_err(|e| words("landlock_create_ruleset", e))?;
    let out = (|| {
        for &(fd, allowed) in rules {
            let rule = k::LandlockPathBeneathAttr { allowed_access: allowed, parent_fd: fd as i32 };
            match k::landlock_add_path_rule(set, &rule) {
                // A file of a pseudo file system (`/dev/stdin` is a pipe):
                // no rule can name it, so it stays refused.
                Err(e) if e.0 == linux_raw_sys::errno::EBADFD as i32 => {}
                r => {
                    r.map_err(|e| words("landlock_add_rule", e))?;
                }
            }
        }
        for &port in connect {
            let rule = k::LandlockNetPortAttr {
                allowed_access: k::LANDLOCK_ACCESS_NET_CONNECT_TCP as u64,
                port: u64::from(port),
            };
            k::landlock_add_port_rule(set, &rule).map_err(|e| words("landlock_add_rule(port)", e))?;
        }
        k::set_no_new_privs().map_err(|e| words("prctl(PR_SET_NO_NEW_PRIVS)", e))?;
        k::landlock_restrict_self(set).map_err(|e| words("landlock_restrict_self", e))
    })();
    let _ = k::close(set);
    out.map(|_| ())
}

/// Each TCP bind and each TCP connect fails with `EACCES` (Landlock ABI 4,
/// a ruleset that handles both rights), but a connect to a port of
/// `connect`: the way out of a host that refuses each bind and lets a
/// connect reach its proxy (PodUtils/podssh#278).
pub fn landlock_tcp(connect: &[u16]) -> Result<(), String> {
    let abi = k::landlock_abi().map_err(|e| words("Landlock", e))?;
    if abi < 4 {
        return Err(format!("the Landlock ABI is {abi}; a TCP rule needs ABI 4"));
    }
    let net = (k::LANDLOCK_ACCESS_NET_BIND_TCP | k::LANDLOCK_ACCESS_NET_CONNECT_TCP) as u64;
    ruleset(0, net, &[], connect)
}

/// A Landlock ruleset that handles the TCP bind alone, with no port: each
/// TCP bind gives `EACCES`, and each TCP connect passes.
pub fn landlock_bind() -> Result<(), String> {
    let abi = k::landlock_abi().map_err(|e| words("Landlock", e))?;
    if abi < 4 {
        return Err(format!("the Landlock ABI is {abi}; a TCP rule needs ABI 4"));
    }
    ruleset(0, k::LANDLOCK_ACCESS_NET_BIND_TCP as u64, &[], &[])
}

/// `rights` on each path except `path` and the directories above it: for
/// each directory from `/` down to `path`, a rule on each entry that is not
/// the next step. The listing is the one of the start of the lab.
pub fn hide(path: &str, rights: u64) -> Result<(), String> {
    k::landlock_abi().map_err(|e| words("Landlock", e))?;
    let steps: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut rules: Vec<(i64, u64)> = Vec::new();
    let mut dir = String::from("/");
    let out = (|| {
        for i in 0..=steps.len() {
            if i == steps.len() && !steps.is_empty() {
                break;
            }
            let next = steps.get(i).copied();
            let list = match std::fs::read_dir(&dir) {
                // The path is absent from here down: the rules so far hide
                // nothing more, and the path stays absent.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                r => r.map_err(|e| format!("a listing of {dir}: {e}"))?,
            };
            for entry in list.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if Some(name) == next {
                    continue;
                }
                let full = format!("{}/{name}", dir.trim_end_matches('/'));
                // A path that does not open (a dead link, a race) gets no
                // rule: it stays refused, which hides no more than it must.
                let Ok(fd) = k::open(&c(&full)?, k::O_PATH | k::O_CLOEXEC, 0) else { continue };
                let is_dir = std::fs::metadata(&full).is_ok_and(|m| m.is_dir());
                let allowed = if is_dir { rights } else { rights & FILE_RIGHTS };
                if allowed == 0 {
                    let _ = k::close(fd);
                } else {
                    rules.push((fd, allowed));
                }
            }
            if let Some(next) = next {
                dir = format!("{}/{next}", dir.trim_end_matches('/'));
            }
        }
        ruleset(rights, 0, &rules, &[])
    })();
    for (fd, _) in rules {
        let _ = k::close(fd);
    }
    out
}

/// The seccomp filter of `calls`, through the BPF program of `pod-confine`,
/// after `no_new_privs`.
pub fn seccomp(calls: &[Deny]) -> Result<(), String> {
    let filter = program(calls).map_err(|f| format!("the filter: {f}"))?;
    k::set_no_new_privs().map_err(|e| words("prctl(PR_SET_NO_NEW_PRIVS)", e))?;
    k::seccomp_set_filter(&filter, 0).map_err(|e| words("seccomp(SET_MODE_FILTER)", e)).map(|_| ())
}

/// The deny rules of a hold, for the one filter of a lab.
pub fn deny_rules(hold: Hold) -> Vec<Deny> {
    match hold {
        Hold::Seccomp(calls) => calls.iter().map(deny).collect(),
        Hold::ConfineDefaults => Deny::defaults(),
        _ => Vec::new(),
    }
}

fn deny(call: &Call) -> Deny {
    call.when.iter().fold(Deny::call(call.name, call.nr, call.errno as u16), |d, w| match *w {
        When::Bits { arg, mask } => d.when_flags(arg, mask),
        When::Equal { arg, mask, value } => d.when_equal(arg, mask, value),
    })
}

/// A target where `socket(2)` and its siblings can go through
/// `socketcall(2)`: a filter of `socket`, `listen` or `getsockname` does
/// not see that path.
pub const SOCKETCALL: bool = cfg!(any(target_arch = "x86", target_arch = "s390x", target_arch = "powerpc64"));

/// The socket calls that `socketcall(2)` also reaches.
const SOCKET_CALLS: [i64; 3] = [k::SYS_SOCKET, k::SYS_LISTEN, k::SYS_GETSOCKNAME];

/// No capability: the ambient set, the bounding set (where the lab holds
/// `CAP_SETPCAP`), the three sets of the lab, then `no_new_privs`. An exec
/// of root then gets no capability back, as the bounding set is empty and
/// `no_new_privs` keeps the permitted set of the lab.
pub fn drop_caps() -> Result<(), String> {
    let _ = k::ambient_clear();
    for cap in 0..64 {
        match k::capbset_drop(cap) {
            Ok(_) => {}
            // Past the last capability of the kernel.
            Err(e) if e == k::EINVAL => break,
            // No CAP_SETPCAP: a caller that is not root. Its exec gets no
            // capability from the bounding set with no_new_privs.
            Err(e) if e == k::EPERM => break,
            Err(e) => return Err(words("prctl(PR_CAPBSET_DROP)", e)),
        }
    }
    k::capset(k::CapSets::default()).map_err(|e| words("capset", e))?;
    k::set_no_new_privs().map_err(|e| words("prctl(PR_SET_NO_NEW_PRIVS)", e)).map(|_| ())
}

/// A new session, so that the lab has no controlling terminal.
pub fn no_ctty() -> Result<(), String> {
    k::setsid().map(|_| ()).map_err(|e| words("setsid", e))
}

/// The reason that a hold cannot run on this target or for this caller,
/// before any change; `None` where it can try.
pub fn cannot(hold: Hold) -> Option<String> {
    match hold {
        Hold::Elsewhere(who) => Some(format!("no lab of pod-lab holds it: {who}")),
        Hold::NonRoot if euid() == 0 => {
            Some("the caller is root; a lab cannot drop root, so run the lab as a user that is not root".to_string())
        }
        Hold::Seccomp(calls) if SOCKETCALL && calls.iter().any(|c| SOCKET_CALLS.contains(&c.nr)) => {
            Some("on this target a socket call can go through socketcall(2), which the filter does not see".to_string())
        }
        _ => None,
    }
}

//! Read the ID maps directly; do not infer them.
//!
//! A dozen probes infer what one read answers, and the inference is wrong in
//! the case that matters: `groups=0,65534` looks like a supplementary group
//! and is `overflowgid`, which is what `getgroups(2)` returns for a group with
//! no mapping.
//!
//! A full `CapEff` proves nothing on its own. A process that is root in a new
//! user namespace receives the complete set by construction, whatever its
//! parent held.

use crate::sys;

#[derive(Default)]
pub struct Identity {
    pub uid: i64,
    pub gid: i64,
    pub groups: Vec<i32>,
    /// The three files, verbatim and whitespace-collapsed. `None` where the
    /// file could not be read, with the reason in [`Identity::unreadable`].
    pub uid_map: Option<String>,
    pub gid_map: Option<String>,
    pub setgroups: Option<String>,
    /// From `/proc/self/status`, by name.
    pub cap_eff: Option<String>,
    pub cap_prm: Option<String>,
    pub cap_bnd: Option<String>,
    pub seccomp: Option<String>,
    pub seccomp_filters: Option<String>,
    pub no_new_privs: Option<String>,
    pub threads: Option<String>,
    /// Every read that did not happen, and why. An absent value is reported
    /// as absent, never as a zero or an empty string that reads like one.
    pub unreadable: Vec<String>,
}

/// True when the map is a single range covering exactly one id, which is the
/// fact that explains every `EINVAL` from `chown` and `setuid` on this class of
/// runtime. A diagnostic quotes it for that reason.
pub fn is_single_id_map(map: &str) -> bool {
    let mut ranges = 0;
    for line in map.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() != 3 {
            continue;
        }
        ranges += 1;
        if f[2] != "1" {
            return false;
        }
    }
    ranges == 1
}

fn read_trimmed(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map(|s| {
            s.lines()
                .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join("; ")
        })
        .map_err(|e| format!("{path}: {e}"))
}

pub fn read() -> Identity {
    // getuid(2) and getgid(2) are documented always to succeed, so their raw
    // returns are the values and there is no error branch to write.
    let mut id = Identity {
        uid: unsafe { sys::syscall6(sys::SYS_GETUID, 0, 0, 0, 0, 0, 0) },
        gid: unsafe { sys::syscall6(sys::SYS_GETGID, 0, 0, 0, 0, 0, 0) },
        ..Default::default()
    };
    match sys::getgroups() {
        Ok(mut g) => {
            // getgroups(2) makes no ordering guarantee, and two runs that
            // differ only in order are two runs somebody has to read twice.
            g.sort_unstable();
            id.groups = g;
        }
        Err(e) => id.unreadable.push(format!("getgroups(2): {} ({})", e.name(), e.0)),
    }

    for (path, slot) in [
        ("/proc/self/uid_map", &mut id.uid_map),
        ("/proc/self/gid_map", &mut id.gid_map),
        ("/proc/self/setgroups", &mut id.setgroups),
    ] {
        match read_trimmed(path) {
            Ok(v) => *slot = Some(v),
            Err(why) => id.unreadable.push(why),
        }
    }

    match std::fs::read_to_string("/proc/self/status") {
        Ok(text) => {
            for line in text.lines() {
                let Some((key, value)) = line.split_once(':') else {
                    continue;
                };
                let value = value.trim().to_string();
                // Selected by NAME: a reader of structured output selects
                // by name, never by position.
                let slot = match key {
                    "CapEff" => &mut id.cap_eff,
                    "CapPrm" => &mut id.cap_prm,
                    "CapBnd" => &mut id.cap_bnd,
                    "Seccomp" => &mut id.seccomp,
                    "Seccomp_filters" => &mut id.seccomp_filters,
                    "NoNewPrivs" => &mut id.no_new_privs,
                    "Threads" => &mut id.threads,
                    _ => continue,
                };
                *slot = Some(value);
            }
        }
        Err(e) => id.unreadable.push(format!("/proc/self/status: {e}")),
    }
    id
}

/// `/proc/sys/kernel/random/boot_id`. It is the **kernel's**, so each
/// namespace on one kernel reads the same value: alone it is not a good key
/// for a cache of verdicts.
pub const BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
/// `readlink` of this is `mnt:[<inode>]`, and it is the one component that
/// differs between a host, a confined container and a plain docker
/// container.
pub const MNT_NS_PATH: &str = "/proc/self/ns/mnt";

/// All that a probe verdict depends on and that a caller can read without
/// a new run of the probe.
///
/// **A cache with a wrong key is worse than no cache.** A host, a confined
/// container and a plain docker container read one boot id and give two
/// different rungs. A cache with the boot id as its only key serves the
/// `namespace` verdict of the host to a confined process.
///
/// A component that could not be read is `None`, and
/// [`ConfinementKey::is_complete`] is then false. A key with a hole in it never
/// matches, because two `None`s comparing equal is a cache hit established by
/// the absence of evidence.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfinementKey {
    pub boot_id: Option<String>,
    pub mnt_ns: Option<String>,
    pub uid_map: Option<String>,
    pub gid_map: Option<String>,
    pub setgroups: Option<String>,
    pub seccomp: Option<String>,
    pub seccomp_filters: Option<String>,
    /// The capability sets that the verdicts turn on. The
    /// chroot verdict is a live `chroot(2)` turning on `CAP_SYS_CHROOT`, so a
    /// process that drops capabilities under the same mount namespace and
    /// seccomp filter would otherwise be served the stale verdict. Read from
    /// `id`, which already carries them; never re-read here (one reader).
    pub cap_eff: Option<String>,
    pub cap_bnd: Option<String>,
    /// WHICH INSTRUMENT ANSWERED. A probe run under `qemu-user` measures the
    /// emulator, so an answer taken there must never go to a process that
    /// was not. `none` where no interpreter was found, which is the absence
    /// of evidence and not a claim of nativeness; [`crate::interp`] says
    /// what it checked.
    pub interpreter: Option<String>,
}

impl ConfinementKey {
    /// The ten components, as `(name, value)`, in a fixed order so two renderings of one
    /// key are byte-identical.
    pub fn components(&self) -> [(&'static str, Option<&str>); 10] {
        [
            ("boot_id", self.boot_id.as_deref()),
            ("mnt_ns", self.mnt_ns.as_deref()),
            ("uid_map", self.uid_map.as_deref()),
            ("gid_map", self.gid_map.as_deref()),
            ("setgroups", self.setgroups.as_deref()),
            ("seccomp", self.seccomp.as_deref()),
            ("seccomp_filters", self.seccomp_filters.as_deref()),
            ("cap_eff", self.cap_eff.as_deref()),
            ("cap_bnd", self.cap_bnd.as_deref()),
            ("interpreter", self.interpreter.as_deref()),
        ]
    }

    pub fn is_complete(&self) -> bool {
        self.components().iter().all(|(_, v)| v.is_some())
    }

    /// Which components a reader could not establish, for the message that
    /// says why the cache was not used.
    pub fn missing(&self) -> Vec<&'static str> {
        self.components().iter().filter(|(_, v)| v.is_none()).map(|(k, _)| *k).collect()
    }

    /// The names whose values differ. An incomplete key on either side
    /// differs in every component it is missing, so a hole never reads as a
    /// match.
    pub fn differences(&self, other: &ConfinementKey) -> Vec<&'static str> {
        self.components()
            .iter()
            .zip(other.components().iter())
            .filter(|((_, a), (_, b))| a.is_none() || b.is_none() || a != b)
            .map(|((k, _), _)| *k)
            .collect()
    }
}

/// Read the key. The components that [`read`] reads come from `id`; the
/// boot id and the inode of the mount namespace are read here. `interpreter`
/// is the answer of [`crate::interp::detect`].
///
/// One reader, called by the writer of the cache and by the reader that
/// checks it. Two readers of one file drift, and the copy the check trusts is
/// the wrong one.
pub fn confinement_key(id: &Identity, interpreter: &crate::interp::Interpreter) -> ConfinementKey {
    ConfinementKey {
        boot_id: read_trimmed(BOOT_ID_PATH).ok(),
        mnt_ns: crate::sys::CBuf::new(MNT_NS_PATH).and_then(|p| crate::sys::readlink(&p).ok()),
        uid_map: id.uid_map.clone(),
        gid_map: id.gid_map.clone(),
        setgroups: id.setgroups.clone(),
        seccomp: id.seccomp.clone(),
        seccomp_filters: id.seccomp_filters.clone(),
        cap_eff: id.cap_eff.clone(),
        cap_bnd: id.cap_bnd.clone(),
        // Always `Some`: the key must name the instrument, and an unset
        // component would let an emulated answer match a native one when both
        // are unreadable. `interp::detect` has no failure state, only two
        // answers, and one of them is "no evidence" spelled `none`.
        interpreter: Some(interpreter.key()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_range_of_one_id_is_recognised() {
        // The map of the confined target, as the Go reference instrument read it.
        assert!(is_single_id_map("0 1000 1"));
        assert!(is_single_id_map("         0       1000          1"));
    }

    #[test]
    fn a_full_map_and_a_multi_range_map_are_not() {
        assert!(!is_single_id_map("0 0 4294967295"));
        assert!(!is_single_id_map("0 1000 1\n1 100000 65536"));
        assert!(!is_single_id_map(""));
    }

    #[test]
    fn this_process_can_read_its_own_maps() {
        // The real path, not an injected double.
        let id = read();
        assert!(id.uid >= 0);
        assert!(id.cap_eff.is_some(), "unreadable: {:?}", id.unreadable);
    }

    #[test]
    fn the_confinement_key_reads_the_mount_namespace_this_process_is_in() {
        let key = confinement_key(&read(), &crate::interp::detect(&|_| false));
        let mnt = key.mnt_ns.as_deref().expect("readlink /proc/self/ns/mnt");
        // The shape, not a particular inode: the number is this machine's.
        assert!(mnt.starts_with("mnt:["), "{mnt}");
        assert!(mnt.ends_with(']'), "{mnt}");
        assert!(key.is_complete(), "missing: {:?}", key.missing());
    }

    #[test]
    fn a_key_with_a_hole_in_it_never_matches_another_with_the_same_hole() {
        // Two `None`s comparing equal would be a cache hit established by
        // the absence of evidence.
        let hole = ConfinementKey { boot_id: Some("b".into()), ..Default::default() };
        assert!(!hole.is_complete());
        assert_eq!(hole.missing().len(), 9);
        assert_eq!(hole.differences(&hole).len(), 9);
    }

    #[test]
    fn two_keys_differing_only_in_the_mount_namespace_report_that_component() {
        // The measured case: a host and a confined container read ONE boot
        // id and differ here. Keying on the boot id alone would call these
        // equal and serve the host's rung to a confined process.
        let host = ConfinementKey {
            boot_id: Some("01703f0e".into()),
            mnt_ns: Some("mnt:[4026531832]".into()),
            uid_map: Some("0 0 4294967295".into()),
            gid_map: Some("0 0 4294967295".into()),
            setgroups: Some("allow".into()),
            seccomp: Some("0".into()),
            seccomp_filters: Some("0".into()),
            cap_eff: Some("000001ffffffffff".into()),
            cap_bnd: Some("000001ffffffffff".into()),
            interpreter: Some("none".into()),
        };
        let confined = ConfinementKey { mnt_ns: Some("mnt:[4026532567]".into()), ..host.clone() };
        assert_eq!(host.differences(&confined), vec!["mnt_ns"]);
        assert!(host.differences(&host).is_empty());
    }

    /// The capability set is part of the key: two
    /// identities differing only in `cap_eff` are different keys, so a
    /// process that drops capabilities under the same mount namespace and
    /// seccomp filter is never served the stale verdict.
    #[test]
    fn two_identities_differing_only_in_capabilities_are_different_keys() {
        let a = read();
        let mut b = read();
        // A mask guaranteed to differ from this process's own, whichever
        // way it reads: the values are the machine's, the difference is
        // the test's.
        let flip = |v: &Option<String>| {
            if v.as_deref() == Some("0000000000000000") {
                Some("ffffffffffffffff".to_string())
            } else {
                Some("0000000000000000".to_string())
            }
        };
        b.cap_eff = flip(&a.cap_eff);
        b.cap_bnd = flip(&a.cap_bnd);
        let native = crate::interp::detect(&|_| false);
        let ka = confinement_key(&a, &native);
        let kb = confinement_key(&b, &native);
        assert!(!ka.differences(&kb).is_empty(), "a dropped capability set reads as the same key");
        assert!(kb.differences(&ka).iter().any(|k| *k == "cap_eff" || *k == "cap_bnd"));
    }
}

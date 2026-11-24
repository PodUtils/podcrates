use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use super::*;
use crate::cgroup::{CGROUP_SUPER_MAGIC, CGROUP2_SUPER_MAGIC};
use crate::usage::{CPU_STAT, MEMORY_EVENTS, MEMORY_PEAK, Version};

const TMPFS_MAGIC: u64 = linux_raw_sys::general::TMPFS_MAGIC as u64;

/// The mounts of a hosted ubuntu-24.04 runner in an incus instance: cgroup2
/// at the root of the cgroup namespace, with the optional field `shared:9`.
const HOSTED: &str = "\
22 30 0:21 / /proc rw,nosuid,nodev,noexec,relatime shared:12 - proc proc rw
23 30 0:22 / /sys rw,nosuid,nodev,noexec,relatime shared:7 - sysfs sysfs rw
35 23 0:30 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime shared:9 - cgroup2 cgroup2 rw,nsdelegate,memory_recursiveprot
";

/// The mounts of a host with both versions: a tmpfs at /sys/fs/cgroup,
/// cgroup2 at `unified` and the v1 hierarchies below it.
const HYBRID: &str = "\
25 18 0:22 / /sys/fs/cgroup ro,nosuid,nodev,noexec shared:9 - tmpfs tmpfs ro,mode=755
26 25 0:23 / /sys/fs/cgroup/unified rw,nosuid,nodev,noexec,relatime shared:10 - cgroup2 cgroup2 rw,nsdelegate
27 25 0:24 / /sys/fs/cgroup/systemd rw,nosuid,nodev,noexec,relatime shared:11 - cgroup cgroup rw,xattr,name=systemd
30 25 0:27 / /sys/fs/cgroup/cpu,cpuacct rw,nosuid,nodev,noexec,relatime shared:14 - cgroup cgroup rw,cpu,cpuacct
31 25 0:28 / /sys/fs/cgroup/pids rw,nosuid,nodev,noexec,relatime shared:15 - cgroup cgroup rw,pids
33 25 0:30 / /sys/fs/cgroup/memory rw,nosuid,nodev,noexec,relatime shared:17 - cgroup cgroup rw,memory
";

/// One line of mountinfo with one optional field.
fn line(root: &str, point: &str, fstype: &str, superopts: &str) -> String {
    format!("40 23 0:40 {root} {point} rw,nosuid,nodev,noexec,relatime shared:5 - {fstype} {fstype} {superopts}\n")
}

fn some(p: &str) -> Option<PathBuf> {
    Some(PathBuf::from(p))
}

fn v2(p: &str) -> Mounts {
    Mounts { v2: some(p), ..Mounts::default() }
}

/// The table of PodUtils/podvm#659: each text and the directories that it
/// gives with a check that always agrees.
#[test]
fn the_cgroup_directories_come_from_mountinfo() {
    let long = format!("40 23 0:40 / /a {} - cgroup2 cgroup2 rw\n", "o".repeat(MAX_LINE));
    let table: &[(&str, String, Mounts)] = &[
        ("hosted runner", HOSTED.to_string(), v2("/sys/fs/cgroup")),
        (
            "hybrid",
            HYBRID.to_string(),
            Mounts {
                v2: some("/sys/fs/cgroup/unified"),
                v1_memory: some("/sys/fs/cgroup/memory"),
                v1_cpuacct: some("/sys/fs/cgroup/cpu,cpuacct"),
            },
        ),
        (
            "v1 alone, memory beside hugetlb",
            line("/", "/c/mem", "cgroup", "rw,memory,hugetlb") + &line("/", "/c/cpu", "cgroup", "rw,cpuacct"),
            Mounts { v1_memory: some("/c/mem"), v1_cpuacct: some("/c/cpu"), ..Mounts::default() },
        ),
        (
            "one v1 mount of both",
            line("/", "/c/both", "cgroup", "rw,memory,cpuacct"),
            Mounts { v1_memory: some("/c/both"), v1_cpuacct: some("/c/both"), ..Mounts::default() },
        ),
        ("sub-tree", line("/lxc/x", "/sys/fs/cgroup", "cgroup2", "rw"), Mounts::default()),
        ("escaped root", line("\\057", "/sys/fs/cgroup", "cgroup2", "rw"), Mounts::default()),
        (
            "an option that only starts with memory",
            line("/", "/c/m", "cgroup", "rw,memoryx,xmemory,cpuacctx,memory_recursiveprot"),
            Mounts::default(),
        ),
        ("escaped space", line("/", "/a\\040b", "cgroup2", "rw"), v2("/a b")),
        ("escaped tab, newline, backslash", line("/", "/a\\011\\012\\134", "cgroup2", "rw"), v2("/a\t\n\\")),
        ("bad escape 9", line("/", "/a\\9bc", "cgroup2", "rw"), Mounts::default()),
        ("bad escape x", line("/", "/a\\1x1", "cgroup2", "rw"), Mounts::default()),
        ("escape over 255", line("/", "/a\\400", "cgroup2", "rw"), Mounts::default()),
        ("escape over 255 that wraps to a letter", line("/", "/a\\501", "cgroup2", "rw"), Mounts::default()),
        ("cut escape", line("/", "/a\\04", "cgroup2", "rw"), Mounts::default()),
        ("cut backslash", line("/", "/a\\", "cgroup2", "rw"), Mounts::default()),
        ("escaped NUL", line("/", "/a\\000b", "cgroup2", "rw"), Mounts::default()),
        ("relative path", line("/", "sys/fs/cgroup", "cgroup2", "rw"), Mounts::default()),
        ("escaped relative path", line("/", "\\057a", "cgroup2", "rw"), v2("/a")),
        ("no separator", "40 23 0:40 / /a rw shared:5 cgroup2 cgroup2 rw\n".to_string(), Mounts::default()),
        ("too few fields before", "40 23 / /a rw - cgroup2 cgroup2 rw\n".to_string(), Mounts::default()),
        ("too few fields after", "40 23 0:40 / /a rw - cgroup2 cgroup2\n".to_string(), Mounts::default()),
        ("no optional field", "40 23 0:40 / /a rw - cgroup2 cgroup2 rw\n".to_string(), v2("/a")),
        ("two optional fields", "40 23 0:40 / /a rw shared:5 master:1 - cgroup2 cgroup2 rw\n".to_string(), v2("/a")),
        ("options of one dash", "40 23 0:40 / /a - - cgroup2 cgroup2 rw\n".to_string(), v2("/a")),
        ("another file system", line("/", "/a", "tmpfs", "rw,memory,cpuacct"), Mounts::default()),
        ("a type that starts with cgroup", line("/", "/a", "cgroup2x", "rw"), Mounts::default()),
        (
            "the last line wins",
            line("/", "/a", "cgroup2", "rw") + &line("/", "/b", "cgroup2", "rw") + &line("/x", "/c", "cgroup2", "rw"),
            v2("/b"),
        ),
        ("empty", String::new(), Mounts::default()),
        ("no newline at the end", line("/", "/a", "cgroup2", "rw").trim_end().to_string(), v2("/a")),
        ("a line over the bound", long.clone(), Mounts::default()),
        ("a long line before a short one", long + &line("/", "/b", "cgroup2", "rw"), v2("/b")),
    ];
    for (name, text, want) in table {
        assert_eq!(Mounts::of(text), *want, "{name}: {text:?}");
    }
}

#[test]
fn a_line_of_the_bound_counts() {
    let head = "40 23 0:40 / /a ";
    let tail = " - cgroup2 cgroup2 rw";
    let text = format!("{head}{}{tail}", "o".repeat(MAX_LINE - head.len() - tail.len()));
    assert_eq!(text.len(), MAX_LINE);
    assert_eq!(Mounts::of(&text), v2("/a"));
    assert_eq!(Mounts::of(&format!("{text}o")), Mounts::default());
}

#[test]
fn a_line_past_the_line_bound_of_the_table_is_not_read() {
    let filler = "1 1 0:1 / /x rw - tmpfs tmpfs rw\n".repeat(MAX_LINES - 1);
    let at = filler.clone() + &line("/", "/a", "cgroup2", "rw");
    assert_eq!(Mounts::of(&at), v2("/a"));
    let past = filler + "1 1 0:1 / /x rw - tmpfs tmpfs rw\n" + &line("/", "/a", "cgroup2", "rw");
    assert_eq!(Mounts::of(&past), Mounts::default());
}

#[test]
fn an_escape_keeps_the_bytes_of_the_mount_point() {
    let m = Mounts::of(&line("/", "/a\\377", "cgroup2", "rw"));
    assert_eq!(m.v2, Some(PathBuf::from(OsString::from_vec(b"/a\xff".to_vec()))));
}

/// A check from a table of mount points and magics; a point that is not in
/// it fails `statfs`.
fn check(table: &[(&str, u64)]) -> impl Fn(&Path) -> Option<u64> {
    let table: Vec<(PathBuf, u64)> = table.iter().map(|(p, m)| (PathBuf::from(p), *m)).collect();
    move |p| table.iter().find(|(q, _)| q == p).map(|(_, m)| *m)
}

#[test]
fn the_check_takes_the_magic_of_each_kind() {
    let magic = check(&[
        ("/sys/fs/cgroup", TMPFS_MAGIC),
        ("/sys/fs/cgroup/unified", CGROUP2_SUPER_MAGIC),
        ("/sys/fs/cgroup/systemd", CGROUP_SUPER_MAGIC),
        ("/sys/fs/cgroup/cpu,cpuacct", CGROUP_SUPER_MAGIC),
        ("/sys/fs/cgroup/pids", CGROUP_SUPER_MAGIC),
        ("/sys/fs/cgroup/memory", CGROUP_SUPER_MAGIC),
    ]);
    assert_eq!(Mounts::of_with(HYBRID, &magic), Mounts::of(HYBRID));
    let hosted = check(&[("/sys/fs/cgroup", CGROUP2_SUPER_MAGIC)]);
    assert_eq!(Mounts::of_with(HOSTED, &hosted), v2("/sys/fs/cgroup"));
    // The check gets the mount point with its escapes undone.
    let spaced = check(&[("/a b", CGROUP2_SUPER_MAGIC)]);
    assert_eq!(Mounts::of_with(&line("/", "/a\\040b", "cgroup2", "rw"), &spaced), v2("/a b"));
}

#[test]
fn a_mount_point_that_fails_the_check_does_not_count() {
    let two = line("/", "/a", "cgroup2", "rw") + &line("/", "/b", "cgroup2", "rw");
    // A tmpfs over the later mount point: the line before stays.
    let over_b = check(&[("/a", CGROUP2_SUPER_MAGIC), ("/b", TMPFS_MAGIC)]);
    assert_eq!(Mounts::of_with(&two, &over_b), v2("/a"));
    // A tmpfs over the one mount point: no directory.
    let over_a = check(&[("/a", TMPFS_MAGIC)]);
    assert_eq!(Mounts::of_with(&line("/", "/a", "cgroup2", "rw"), &over_a), Mounts::default());
    // A failed statfs does not count.
    assert_eq!(Mounts::of_with(&two, &check(&[])), Mounts::default());
    // A v1 line whose point answers cgroup2 is not a v1 hierarchy.
    let v1 = line("/", "/m", "cgroup", "rw,memory") + &line("/", "/c", "cgroup", "rw,cpuacct");
    let wrong = check(&[("/m", CGROUP2_SUPER_MAGIC), ("/c", CGROUP2_SUPER_MAGIC)]);
    assert_eq!(Mounts::of_with(&v1, &wrong), Mounts::default());
    // A cgroup2 line whose point answers cgroup v1 is not cgroup2.
    let wrong = check(&[("/a", CGROUP_SUPER_MAGIC), ("/b", CGROUP_SUPER_MAGIC)]);
    assert_eq!(Mounts::of_with(&two, &wrong), Mounts::default());
}

#[test]
fn the_meter_opens_the_directories_of_the_mounts() {
    let root = std::env::temp_dir().join(format!("pod-hostprobe-mounts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (f, text) in [(CPU_STAT, "usage_usec 7\n"), (MEMORY_PEAK, "4096\n"), (MEMORY_EVENTS, "oom_kill 1\n")] {
        std::fs::write(root.join(f), text).unwrap();
    }
    let m = Mounts { v2: Some(root.clone()), ..Mounts::default() };
    let mut meter = m.meter();
    assert_eq!(meter.version(), Some(Version::V2));
    let u = meter.read();
    assert_eq!((u.cpu_usec, u.peak_bytes, u.oom_kills), (Some(7), Some(4096), Some(1)), "{:?}", u.why);
    assert_eq!(Mounts::default().meter().version(), None);
    let _ = std::fs::remove_dir_all(&root);
}

/// The mounts of this host: each directory that counts is a mount of its
/// kind now.
#[test]
fn the_mounts_of_the_host_pass_their_check() {
    if !Path::new(MOUNTINFO).exists() {
        eprintln!("skip: no {MOUNTINFO} on this host");
        return;
    }
    let m = Mounts::of_host().expect("the mount table of this process");
    let magic = |p: &Path| {
        crate::sys::CBuf::from_bytes(p.as_os_str().as_encoded_bytes())
            .and_then(|c| crate::sys::statfs(&c).ok())
            .map(|s| s.f_type)
    };
    for (dir, want) in
        [(&m.v2, CGROUP2_SUPER_MAGIC), (&m.v1_memory, CGROUP_SUPER_MAGIC), (&m.v1_cpuacct, CGROUP_SUPER_MAGIC)]
    {
        if let Some(dir) = dir {
            assert!(dir.is_dir(), "{}", dir.display());
            assert_eq!(magic(dir), Some(want), "{}", dir.display());
        }
    }
    eprintln!("the mounts of this host: {m:?}");
}

use std::path::{Path, PathBuf};

use super::*;

const CPU_STAT_TEXT: &str = "usage_usec 1234567\nuser_usec 1000000\nsystem_usec 234567\n\
                             nr_periods 0\nnr_throttled 0\nthrottled_usec 0\n";
const EVENTS_TEXT: &str = "low 0\nhigh 0\nmax 4\noom 2\noom_kill 2\noom_group_kill 0\n";
const OOM_CONTROL_TEXT: &str = "oom_kill_disable 0\nunder_oom 0\noom_kill 3\n";

#[test]
fn the_cpu_time_of_cpu_stat_is_the_line_usage_usec() {
    let table: &[(&str, Option<u64>)] = &[
        (CPU_STAT_TEXT, Some(1_234_567)),
        ("usage_usec 0\n", Some(0)),
        ("user_usec 5\nsystem_usec 6\n", None),
        ("usage_usec\n", None),
        ("usage_usec -1\n", None),
        ("usage_usec 12x\n", None),
        ("usage_usec_extra 9\n", None),
        ("", None),
    ];
    for (text, want) in table {
        assert_eq!(cpu_usec_v2(text), *want, "{text:?}");
    }
}

#[test]
fn the_nanoseconds_of_cpuacct_usage_round_down_to_microseconds() {
    let table: &[(&str, Option<u64>)] = &[
        ("1234567890\n", Some(1_234_567)),
        ("999\n", Some(0)),
        ("1000\n", Some(1)),
        ("1999\n", Some(1)),
        ("0\n", Some(0)),
        ("18446744073709551615\n", Some(18_446_744_073_709_551)),
        ("\n", None),
        ("-5\n", None),
        ("12.5\n", None),
    ];
    for (text, want) in table {
        assert_eq!(cpu_usec_v1(text), *want, "{text:?}");
    }
}

#[test]
fn the_bytes_of_a_peak_are_one_count() {
    let table: &[(&str, Option<u64>)] =
        &[("52428800\n", Some(52_428_800)), ("0", Some(0)), ("max\n", None), ("\n", None), ("1 2\n", None)];
    for (text, want) in table {
        assert_eq!(bytes(text), *want, "{text:?}");
    }
}

#[test]
fn the_oom_kills_are_the_line_oom_kill_of_both_versions() {
    let table: &[(&str, Option<u64>)] = &[
        (EVENTS_TEXT, Some(2)),
        (OOM_CONTROL_TEXT, Some(3)),
        ("oom_kill_disable 0\nunder_oom 0\n", None),
        ("low 0\n", None),
        ("oom_kill many\n", None),
        ("", None),
    ];
    for (text, want) in table {
        assert_eq!(oom_kills_in(text), *want, "{text:?}");
    }
}

/// A made-up root of cgroup directories in the temporary directory.
fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pod-hostprobe-usage-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn dir(root: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = root.join(name);
    std::fs::create_dir_all(&d).unwrap();
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

/// A host with both versions: the v1 hierarchies with the values of the
/// run, beside the given cgroup2 files.
fn hybrid(name: &str, v2_files: &[(&str, &str)]) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let r = root(name);
    let v2 = dir(&r, "unified", v2_files);
    let mem = dir(&r, "memory", &[(MEMORY_MAX_USAGE, "8388608\n"), (MEMORY_OOM_CONTROL, OOM_CONTROL_TEXT)]);
    let cpu = dir(&r, "cpuacct", &[(CPUACCT_USAGE, "1500000000\n")]);
    (r, v2, mem, cpu)
}

#[test]
fn cgroup2_with_its_memory_controller_serves() {
    let (r, v2, mem, cpu) =
        hybrid("v2", &[(CPU_STAT, CPU_STAT_TEXT), (MEMORY_PEAK, "52428800\n"), (MEMORY_EVENTS, EVENTS_TEXT)]);
    let mut m = Meter::open(Some(&v2), Some(&mem), Some(&cpu));
    assert_eq!(m.version(), Some(Version::V2));
    let u = m.read();
    let want = Usage { cpu_usec: Some(1_234_567), peak_bytes: Some(52_428_800), oom_kills: Some(2), why: vec![] };
    assert_eq!(u, want);
    let _ = std::fs::remove_dir_all(&r);
}

/// The measured fault of a host with both versions: the cgroup2 with no
/// controllers has a `cpu.stat` of the host. The meter takes v1.
#[test]
fn cgroup2_with_no_memory_controller_gives_way_to_v1() {
    let (r, v2, mem, cpu) = hybrid("v1", &[(CPU_STAT, "usage_usec 43000000\n")]);
    let mut m = Meter::open(Some(&v2), Some(&mem), Some(&cpu));
    assert_eq!(m.version(), Some(Version::V1));
    let u = m.read();
    assert_eq!((u.cpu_usec, u.peak_bytes, u.oom_kills), (Some(1_500_000), Some(8_388_608), Some(3)));
    assert_eq!(u.why.len(), 1, "{:?}", u.why);
    assert!(u.why[0].starts_with("cgroup2 ") && u.why[0].contains("no memory controller"), "{:?}", u.why);
    let _ = std::fs::remove_dir_all(&r);
}

/// Linux 5.8 to 5.18: the memory controller with no `memory.peak`. The
/// meter stays on cgroup2 and gives the reason of the absent peak.
#[test]
fn cgroup2_with_no_memory_peak_stays_on_cgroup2() {
    let (r, v2, mem, cpu) = hybrid("v2-no-peak", &[(CPU_STAT, CPU_STAT_TEXT), (MEMORY_EVENTS, EVENTS_TEXT)]);
    let u = read(Some(&v2), Some(&mem), Some(&cpu));
    assert_eq!((u.cpu_usec, u.peak_bytes, u.oom_kills), (Some(1_234_567), None, Some(2)));
    assert_eq!(u.why.len(), 1, "{:?}", u.why);
    assert!(u.why[0].starts_with("peak_bytes: ") && u.why[0].contains(MEMORY_PEAK), "{:?}", u.why);
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn each_missing_value_gives_its_reason() {
    let r = root("missing");
    let v2 = dir(&r, "unified", &[]);
    let mem = dir(&r, "memory", &[(MEMORY_OOM_CONTROL, "oom_kill_disable 0\nunder_oom 0\n")]);
    let u = read(Some(&v2), Some(&mem), None);
    assert_eq!((u.cpu_usec, u.peak_bytes, u.oom_kills), (None, None, None));
    assert_eq!(u.why.len(), 4, "{:?}", u.why);
    assert!(u.why[0].contains("no memory controller"), "{:?}", u.why);
    assert_eq!(u.why[1], "cpu_usec: no cgroup v1 hierarchy cpuacct");
    assert!(u.why[2].starts_with("peak_bytes: ") && u.why[2].contains(MEMORY_MAX_USAGE), "{:?}", u.why);
    assert!(u.why[3].starts_with("oom_kills: ") && u.why[3].ends_with("no count"), "{:?}", u.why);

    let u = read(None, None, None);
    assert_eq!((u.cpu_usec, u.peak_bytes, u.oom_kills), (None, None, None));
    assert_eq!(u.why.len(), 3, "{:?}", u.why);
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn a_meter_with_no_file_gives_a_reason_for_each_value() {
    let u = Meter::default().read();
    assert_eq!(u.why.len(), 3, "{:?}", u.why);
    assert!(u.why.iter().all(|w| w.ends_with("no cgroup file is open")), "{:?}", u.why);
}

/// The files stay open: a read after a change of the text gives the new
/// text from offset 0, and a read after the remove of the directory still
/// reads the open files.
#[test]
fn a_meter_reads_its_open_files_again_from_the_start() {
    let r = root("reread");
    let v2 = dir(&r, "leaf", &[(CPU_STAT, "usage_usec 5\n"), (MEMORY_PEAK, "7\n"), (MEMORY_EVENTS, "oom_kill 0\n")]);
    let mut m = Meter::v2(&v2).unwrap();
    assert_eq!(m.read().cpu_usec, Some(5));
    std::fs::write(v2.join(CPU_STAT), "usage_usec 99\n").unwrap();
    assert_eq!(m.read().cpu_usec, Some(99));
    std::fs::remove_dir_all(&r).unwrap();
    let u = m.read();
    assert_eq!((u.cpu_usec, u.peak_bytes, u.oom_kills), (Some(99), Some(7), Some(0)));
}

/// A caller that mounts each hierarchy in turn at one place.
#[test]
fn the_parts_of_v1_open_one_at_a_time() {
    let r = root("parts");
    let at = r.join("mount");
    dir(&r, "mount", &[]);
    let refused = Meter::v2(&at).unwrap_err();
    assert_eq!((refused.stage, refused.class), (Stage::Host, Class::Unsupported));
    assert!(refused.io_cause().is_some());
    let mut m = Meter::default();
    m.note(refused.chain());
    dir(&r, "mount", &[(MEMORY_MAX_USAGE, "4096\n"), (MEMORY_OOM_CONTROL, OOM_CONTROL_TEXT)]);
    m.v1_memory(&at);
    std::fs::remove_dir_all(&at).unwrap();
    dir(&r, "mount", &[(CPUACCT_USAGE, "2000\n")]);
    m.v1_cpu(&at);
    let u = m.read();
    assert_eq!((u.cpu_usec, u.peak_bytes, u.oom_kills), (Some(2), Some(4096), Some(3)));
    assert_eq!(u.why.len(), 1, "{:?}", u.why);
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn the_oom_kills_of_a_directory_come_from_memory_events() {
    let r = root("oom-of");
    assert_eq!(oom_kills_of(&r), None);
    std::fs::write(r.join(MEMORY_EVENTS), EVENTS_TEXT).unwrap();
    assert_eq!(oom_kills_of(&r), Some(2));
    std::fs::write(r.join(MEMORY_EVENTS), "low 0\n").unwrap();
    assert_eq!(oom_kills_of(&r), None);
    let _ = std::fs::remove_dir_all(&r);
}

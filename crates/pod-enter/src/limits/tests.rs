use std::path::{Path, PathBuf};

use super::*;
use crate::userland::Maps;
use crate::{Fds, Names, Plan, RootDir, spawn_userland};

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-enter-limits-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plan(limits: Vec<Limit>, no_new_privs: bool) -> Plan {
    plan_with(limits, no_new_privs, false)
}

fn plan_with(limits: Vec<Limit>, no_new_privs: bool, die_with_parent: bool) -> Plan {
    Plan {
        argv: vec!["sh".to_string()],
        env: vec!["PATH=/bin:/usr/bin".to_string()],
        working_dir: "/".to_string(),
        fds: Fds::default(),
        banner: String::new(),
        path_dirs: vec!["/bin".to_string()],
        names: Names::DEFAULT,
        limits,
        no_new_privs,
        die_with_parent,
    }
}

/// Run `script` with `/bin/sh` through the entry with no chroot, under
/// `plan`: its exit code, or the refusal of the entry.
fn run(d: &Path, plan: &Plan, script: &str) -> Result<i32, String> {
    let m = Maps::of_root(d.to_str().unwrap());
    let root = RootDir::open(m.rootfs()).unwrap();
    let argv = vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()];
    let child =
        spawn_userland(&root, plan, &m, argv, "userland", None, &mut std::io::sink()).map_err(|e| e.to_string())?;
    child.wait().map_err(|e| e.to_string())
}

/// The line of `text` that starts with `head`, with single spaces.
fn line_of(text: &str, head: &str) -> String {
    text.lines().find(|l| l.starts_with(head)).unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn each_word_of_docker_names_one_resource_of_the_kernel() {
    for word in ["cpu", "fsize", "nofile", "nproc", "as", "core"] {
        let r = Resource::from_word(word).unwrap_or_else(|| panic!("{word}"));
        assert_eq!(r.word(), word);
        assert!(r.name().starts_with("RLIMIT_"));
    }
    assert_eq!(Resource::Nofile.name(), "RLIMIT_NOFILE");
    assert_eq!(Resource::Fsize.number(), pod_hostprobe::sys::RLIMIT_FSIZE);
    assert_eq!(Resource::from_word("files"), None);
}

#[test]
fn a_limit_reaches_the_payload_and_not_the_caller() {
    pod_testkit::need_tool!("/bin/sh");
    let d = dir("nofile");
    let out = d.join("limits.txt");
    let p = plan(vec![Limit { resource: Resource::Nofile, soft: 64, hard: 64 }], false);
    assert_eq!(run(&d, &p, &format!("cat /proc/self/limits > {}", out.display())), Ok(0));
    let child = std::fs::read_to_string(&out).unwrap();
    assert_eq!(line_of(&child, "Max open files"), "Max open files 64 64 files", "{child}");
    let mine = std::fs::read_to_string("/proc/self/limits").unwrap();
    assert_ne!(line_of(&mine, "Max open files"), "Max open files 64 64 files", "the caller took the limit");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_file_size_limit_stops_a_write_of_the_payload_only() {
    pod_testkit::need_tool!("/bin/sh");
    pod_testkit::need_tool!("dd");
    let d = dir("fsize");
    let big = d.join("big");
    let p = plan(vec![Limit { resource: Resource::Fsize, soft: 1 << 20, hard: 1 << 20 }], false);
    // The shell ignores SIGXFSZ, so that the write past the limit fails
    // with EFBIG in place of a kill.
    let script = format!("trap '' XFSZ; dd if=/dev/zero of={} bs=1024 count=2048 2>/dev/null; exit 0", big.display());
    assert_eq!(run(&d, &p, &script), Ok(0));
    assert_eq!(std::fs::metadata(&big).unwrap().len(), 1 << 20, "the payload wrote past its limit");
    // The caller writes more than the limit of the payload.
    std::fs::write(d.join("mine"), vec![0u8; 2 << 20]).unwrap();
    assert_eq!(std::fs::metadata(d.join("mine")).unwrap().len(), 2 << 20);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn no_new_privs_is_set_in_the_payload_only() {
    pod_testkit::need_tool!("/bin/sh");
    let d = dir("nnp");
    let out = d.join("status.txt");
    let script = format!("grep NoNewPrivs /proc/self/status > {}", out.display());
    assert_eq!(run(&d, &plan(Vec::new(), true), &script), Ok(0));
    let child = std::fs::read_to_string(&out).unwrap();
    assert_eq!(line_of(&child, "NoNewPrivs"), "NoNewPrivs: 1", "{child}");
    assert_eq!(run(&d, &plan(Vec::new(), false), &script), Ok(0));
    let without = std::fs::read_to_string(&out).unwrap();
    assert_eq!(line_of(&without, "NoNewPrivs"), "NoNewPrivs: 0", "{without}");
    let mine = std::fs::read_to_string("/proc/self/status").unwrap();
    assert_eq!(line_of(&mine, "NoNewPrivs"), "NoNewPrivs: 0", "the caller took no_new_privs");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_hard_limit_over_the_callers_is_refused_by_name() {
    pod_testkit::need_tool!("/bin/sh");
    let (_, hard) = pod_hostprobe::sys::prlimit(Resource::Nofile.number()).unwrap();
    // Root can raise a hard limit, but not past `fs.nr_open`; another user
    // cannot raise it past its own.
    let root = std::fs::read_to_string("/proc/self/status").unwrap().lines().any(|l| l.starts_with("Uid:\t0\t"));
    let nr_open = std::fs::read_to_string("/proc/sys/fs/nr_open").ok().and_then(|t| t.trim().parse::<u64>().ok());
    let over = match (root, nr_open) {
        (true, Some(n)) => n + 1,
        (false, _) if hard != INFINITY => hard + 1,
        _ => return,
    };
    let d = dir("over");
    let p = plan(vec![Limit { resource: Resource::Nofile, soft: over, hard: over }], false);
    let e = run(&d, &p, "true").unwrap_err();
    assert!(e.contains("setrlimit(RLIMIT_NOFILE): EPERM"), "{e}");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn more_limits_than_the_most_are_refused_before_the_fork() {
    let d = dir("most");
    let p = plan(vec![Limit { resource: Resource::Core, soft: 0, hard: 0 }; MOST + 1], false);
    let e = run(&d, &p, "true").unwrap_err();
    assert!(e.contains("at most 64"), "{e}");
    let _ = std::fs::remove_dir_all(d);
}

/// With `die_with_parent`, the payload ends when the thread that forked it
/// exits (T-205): a supervisor that is killed leaves no payload. The
/// payload writes its pid and sleeps; the thread that spawned it returns.
#[test]
fn a_payload_ends_with_the_thread_that_forked_it() {
    let d = dir("pdeathsig");
    let pidfile = d.join("payload.pid");
    let p = pidfile.display().to_string();
    let plan = plan_with(Vec::new(), false, true);
    let script = format!("echo $$ > {p}.tmp; mv {p}.tmp {p}; exec sleep 300");
    let d2 = d.clone();
    let spawner = std::thread::spawn(move || {
        let m = Maps::of_root(d2.to_str().unwrap());
        let root = RootDir::open(m.rootfs()).unwrap();
        let argv = vec!["/bin/sh".to_string(), "-c".to_string(), script];
        // Kept and never waited for: the thread ends while the child runs.
        let _child = spawn_userland(&root, &plan, &m, argv, "userland", None, &mut std::io::sink()).unwrap();
        while !std::path::Path::new(&p).exists() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });
    spawner.join().unwrap();
    let pid: i64 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
    let alive = || std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| !stat.contains(") Z "));
    let at = std::time::Instant::now();
    while alive() && at.elapsed() < std::time::Duration::from_secs(5) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let lived = alive();
    let _ = pod_hostprobe::sys::kill(pid, 9);
    let _ = std::fs::remove_dir_all(&d);
    assert!(!lived, "the payload {pid} outlived the thread that forked it");
}

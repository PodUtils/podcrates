//! The tests of the confinement through the entry. A host with no
//! Landlock skips the runs of a payload with a line that says so; the run
//! as a user with no `chroot(2)` skips the rungs with a chroot.

#[path = "image.rs"]
mod image;
#[path = "mounts.rs"]
mod mounts;
#[path = "payload.rs"]
mod payload;

use std::path::{Path, PathBuf};

use pod_confine::{Access, Deny, Spec};
use pod_enter::userland::Maps;
use pod_enter::{Fds, Limit, Names, Plan, Resource, RootDir, spawn, spawn_namespace, spawn_userland};
use pod_hostprobe::sys as k;
use pod_testkit::host::skip_line;

use payload::{PAYLOAD, Run, WRITER, run};

pub fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some(PAYLOAD) => std::process::exit(payload::payload(&args[2..])),
        Some(WRITER) => std::process::exit(payload::writer(&args[2..])),
        _ => {}
    }
    let tests: [(&str, fn()); 10] = [
        ("the_entry_with_no_chroot_confines_with_host_paths", the_entry_with_no_chroot_confines_with_host_paths),
        ("a_passed_descriptor_takes_the_place_of_no_rule", a_passed_descriptor_takes_the_place_of_no_rule),
        ("a_passed_descriptor_takes_the_place_of_no_pipe", a_passed_descriptor_takes_the_place_of_no_pipe),
        ("the_rungs_with_a_chroot_confine_with_guest_paths", the_rungs_with_a_chroot_confine_with_guest_paths),
        (
            "a_path_that_does_not_exist_is_refused_before_the_fork",
            a_path_that_does_not_exist_is_refused_before_the_fork,
        ),
        ("a_payload_never_runs_where_landlock_is_absent", a_payload_never_runs_where_landlock_is_absent),
        ("a_plan_with_no_confinement_runs_free", a_plan_with_no_confinement_runs_free),
        ("a_rule_covers_the_tmpfs_of_the_namespace_rung", mounts::a_rule_covers_the_tmpfs_of_the_namespace_rung),
        ("a_rule_on_a_path_that_the_tmpfs_hides_is_refused", mounts::a_rule_on_a_path_that_the_tmpfs_hides_is_refused),
        ("a_write_rule_holds_on_the_entry_with_no_chroot", mounts::a_write_rule_holds_on_the_entry_with_no_chroot),
    ];
    let mut failed = 0;
    for (name, test) in tests {
        let ok = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        failed += usize::from(!ok);
    }
    println!("test result: {} passed; {failed} failed", tests.len() - failed);
    if failed > 0 {
        std::process::exit(101);
    }
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-enter-confine-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn text(p: &Path) -> String {
    p.to_str().unwrap().to_string()
}

/// True where a confinement loads here; else a skip line.
fn landlock() -> bool {
    match pod_confine::check() {
        Ok(abi) if abi >= pod_confine::NET_ABI => true,
        Ok(abi) => {
            skip_line(&format!("Landlock ABI {abi} has no TCP rules (ABI {})", pod_confine::NET_ABI));
            false
        }
        Err(e) => {
            skip_line(&e.to_string());
            false
        }
    }
}

fn plan(argv: Vec<String>, confine: Option<Spec>) -> Plan {
    Plan {
        argv,
        env: Vec::new(),
        working_dir: "/".to_string(),
        fds: Fds::default(),
        banner: String::new(),
        path_dirs: Vec::new(),
        names: Names::DEFAULT,
        limits: Vec::new(),
        no_new_privs: false,
        die_with_parent: false,
        confine,
    }
}

/// Two loopback listeners: the port that the spec allows, and another.
fn ports() -> (std::net::TcpListener, std::net::TcpListener) {
    let l = || std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    (l(), l())
}

fn port(l: &std::net::TcpListener) -> u16 {
    l.local_addr().unwrap().port()
}

/// The checks of experiment 414 on one run.
fn assert_confined(rung: &str, r: &Run) {
    assert_eq!(r.line.trim_end(), "secret=-13 allowed=0 connect=0 other=-13 unshare=-1 setns=-1", "{rung}: {r:?}");
    assert_eq!(r.status, vec!["NoNewPrivs:\t1", "Seccomp:\t2"], "{rung}");
    assert_eq!(r.code, 0, "{rung}");
}

/// The entry with no chroot takes host paths: the binary, its libraries
/// and `allowed` can be read; `secret` beside it cannot.
fn the_entry_with_no_chroot_confines_with_host_paths() {
    pod_testkit::need_loopback!();
    if !landlock() {
        return;
    }
    let d = dir("userland");
    std::fs::write(d.join("allowed"), b"allowed").unwrap();
    std::fs::write(d.join("secret"), b"secret").unwrap();
    let exe = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let (ok, other) = ports();
    let mut spec = Spec::new().allow(text(&d.join("allowed")), Access::READ).connect_to(port(&ok));
    for dir in image::dirs_of(&image::mapped_files()) {
        spec = spec.allow(dir, Access::READ | Access::EXECUTE);
    }
    let argv: Vec<String> = [text(&exe), PAYLOAD.into(), text(&d.join("secret")), text(&d.join("allowed"))]
        .into_iter()
        .chain([port(&ok).to_string(), port(&other).to_string()])
        .collect();
    let mut p = plan(argv.clone(), Some(spec));
    let m = Maps::of_root(&text(&d));
    let root = RootDir::open(m.rootfs()).unwrap();
    let r = run(&mut p, 0, |p| spawn_userland(&root, p, &m, argv, "userland", None, &mut std::io::sink())).unwrap();
    assert_confined("userland", &r);
    let _ = std::fs::remove_dir_all(d);
}

/// A descriptor that the child puts onto the number of a rule (the lowest
/// free numbers) does not take the place of the rule: the compile moves
/// each rule clear of the passed numbers.
fn a_passed_descriptor_takes_the_place_of_no_rule() {
    pod_testkit::need_loopback!();
    if !landlock() {
        return;
    }
    let d = dir("cover");
    std::fs::write(d.join("allowed"), b"allowed").unwrap();
    std::fs::write(d.join("secret"), b"secret").unwrap();
    let exe = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let (ok, other) = ports();
    let mut spec = Spec::new().allow(text(&d.join("allowed")), Access::READ).connect_to(port(&ok));
    for dir in image::dirs_of(&image::mapped_files()) {
        spec = spec.allow(dir, Access::READ | Access::EXECUTE);
    }
    let rules = spec.paths.len();
    let argv: Vec<String> = [text(&exe), PAYLOAD.into(), text(&d.join("secret")), text(&d.join("allowed"))]
        .into_iter()
        .chain([port(&ok).to_string(), port(&other).to_string()])
        .collect();
    let mut p = plan(argv.clone(), Some(spec));
    let m = Maps::of_root(&text(&d));
    let root = RootDir::open(m.rootfs()).unwrap();
    let r = run(&mut p, rules, |p| spawn_userland(&root, p, &m, argv, "userland", None, &mut std::io::sink()));
    assert_confined("userland, each rule number passed", &r.unwrap());
    let _ = std::fs::remove_dir_all(d);
}

/// A descriptor that the child puts onto the number of the readiness pipe
/// does not take its place: a step that fails in the child is still a
/// refusal, not a payload that seems to start. A hard limit over the
/// limit of the caller is the step that fails.
fn a_passed_descriptor_takes_the_place_of_no_pipe() {
    let (_, hard) = k::prlimit(Resource::Nofile.number()).unwrap();
    let root_user = pod_testkit::host::root().is_ok();
    let nr_open = std::fs::read_to_string("/proc/sys/fs/nr_open").ok().and_then(|t| t.trim().parse::<u64>().ok());
    let over = match (root_user, nr_open) {
        (true, Some(n)) => n + 1,
        (false, _) if hard != pod_enter::limits::INFINITY => hard + 1,
        _ => {
            skip_line("no limit of RLIMIT_NOFILE is over the limit of this process");
            return;
        }
    };
    let d = dir("pipe");
    let mut p = plan(vec!["/bin/true".into()], None);
    p.limits = vec![Limit { resource: Resource::Nofile, soft: over, hard: over }];
    let m = Maps::of_root(&text(&d));
    // The root descriptor above 3 and 4, the numbers of the pipes of `run`.
    let null = k::CBuf::new("/dev/null").unwrap();
    let held: Vec<i64> = (0..2).map(|_| k::open(&null, k::O_RDONLY | k::O_CLOEXEC, 0).unwrap()).collect();
    let root = RootDir::open(m.rootfs()).unwrap();
    for fd in held {
        let _ = k::close(fd);
    }
    let argv = vec!["/bin/true".to_string()];
    let e = run(&mut p, 2, |p| spawn_userland(&root, p, &m, argv, "userland", None, &mut std::io::sink()));
    let e = e.expect_err("the payload seemed to start");
    assert!(e.contains("setrlimit(RLIMIT_NOFILE): EPERM"), "{e}");
    // A descriptor onto the number of the root is refused before the fork.
    p.fds.pass = vec![(root.fd(), 0)];
    let e = spawn_userland(&root, &p, &m, vec!["/bin/true".into()], "userland", None, &mut std::io::sink());
    let e = e.expect_err("a passed descriptor took the number of the root").to_string();
    assert!(e.contains("the number of the root file system"), "{e}");
    let _ = std::fs::remove_dir_all(d);
}

/// The chroot sequence and the namespace rung take guest paths: `/allowed`
/// and `/secret` exist in the image only, and a host path that the image
/// does not hold is refused by name.
fn the_rungs_with_a_chroot_confine_with_guest_paths() {
    pod_testkit::need_loopback!();
    if !landlock() {
        return;
    }
    if let Err(why) = pod_testkit::host::root() {
        skip_line(&format!("the rungs with a chroot need chroot(2): {why}"));
        return;
    }
    let d = dir("image");
    let dirs = image::build(&d);
    let (ok, other) = ports();
    let mut spec = Spec::new().allow("/allowed", Access::READ).connect_to(port(&ok));
    for dir in dirs {
        spec = spec.allow(dir, Access::READ | Access::EXECUTE);
    }
    let argv: Vec<String> = [image::GUEST_EXE, PAYLOAD, "/secret", "/allowed"]
        .into_iter()
        .map(String::from)
        .chain([port(&ok).to_string(), port(&other).to_string()])
        .collect();
    let root = RootDir::open(&text(&d)).unwrap();
    let mut p = plan(argv, Some(spec.clone()));
    let r = run(&mut p, 0, |p| spawn(&root, p, &mut std::io::sink())).unwrap();
    assert_confined("chroot", &r);
    let mut banner = Vec::new();
    let r = run(&mut p, 0, |p| spawn_namespace(&root, p, &mut banner)).unwrap();
    assert_confined("namespace", &r);
    if !banner.is_empty() {
        skip_line(&format!("the namespace rung fell back to chroot: {}", String::from_utf8_lossy(&banner).trim_end()));
    }
    // A host path is no path of the guest: the image is not in itself.
    let host = text(&d);
    let mut p = plan(vec![image::GUEST_EXE.into()], Some(spec.allow(host.clone(), Access::READ)));
    let e = run(&mut p, 0, |p| spawn(&root, p, &mut std::io::sink())).unwrap_err();
    assert!(e.contains(&host) && e.contains("does not exist"), "{e}");
    let _ = std::fs::remove_dir_all(d);
}

/// A path of the spec that does not exist stops the entry before the fork,
/// with its name.
fn a_path_that_does_not_exist_is_refused_before_the_fork() {
    if !landlock() {
        return;
    }
    let d = dir("missing");
    let gone = text(&d.join("gone"));
    let mut p = plan(vec!["/bin/true".into()], Some(Spec::new().allow(gone.clone(), Access::READ)));
    let m = Maps::of_root(&text(&d));
    let root = RootDir::open(m.rootfs()).unwrap();
    let argv = vec!["/bin/true".to_string()];
    let e = run(&mut p, 0, |p| spawn_userland(&root, p, &m, argv, "userland", None, &mut std::io::sink())).unwrap_err();
    assert!(e.contains(&gone) && e.contains("does not exist"), "{e}");
    let _ = std::fs::remove_dir_all(d);
}

/// Where Landlock is absent, the entry refuses and no payload runs. A
/// child of this process (one thread) stands in for such a host: a
/// filter answers ENOSYS to the Landlock calls, then the child asks for
/// a confined payload that would write a file.
fn a_payload_never_runs_where_landlock_is_absent() {
    let d = dir("absent");
    let mark = d.join("ran");
    let absent = pod_confine::deny::program(&[
        Deny::call("landlock_create_ruleset", k::SYS_LANDLOCK_CREATE_RULESET, 38),
        Deny::call("landlock_add_rule", k::SYS_LANDLOCK_ADD_RULE, 38),
        Deny::call("landlock_restrict_self", k::SYS_LANDLOCK_RESTRICT_SELF, 38),
    ])
    .unwrap();
    let argv = vec!["/bin/sh".to_string(), "-c".to_string(), format!("echo ran > {}", text(&mark))];
    let p = plan(argv.clone(), Some(Spec::new().allow("/", Access::READ | Access::WRITE | Access::EXECUTE)));
    let m = Maps::of_root(&text(&d));
    let root = RootDir::open(m.rootfs()).unwrap();
    // SAFETY: this process has one thread, so the child may allocate.
    let pid = match unsafe { k::clone_fork(k::SIGCHLD) }.unwrap() {
        0 => {
            if k::set_no_new_privs().is_err() || k::seccomp_set_filter(&absent, 0).is_err() {
                k::exit_group(3);
            }
            let code = match spawn_userland(&root, &p, &m, argv, "userland", None, &mut std::io::sink()) {
                Err(e) if e.to_string().contains("Landlock is absent") && e.to_string().contains("ENOSYS") => 0,
                Err(e) => {
                    eprintln!("another refusal: {e}");
                    1
                }
                Ok(c) => {
                    let _ = c.wait();
                    2
                }
            };
            k::exit_group(code)
        }
        pid => pid,
    };
    let mut status = 0;
    k::wait4(pid, &mut status).unwrap();
    if status == 3 << 8 {
        skip_line("this host refuses a seccomp filter");
        return;
    }
    assert_eq!(status, 0, "the wait status of the child: 1 another refusal, 2 the payload ran");
    assert!(!mark.exists(), "the payload ran with no Landlock");
    let _ = std::fs::remove_dir_all(d);
}

/// The control: the same payload with no confinement reads `secret`,
/// makes a user namespace or not as the host allows, and has no filter.
fn a_plan_with_no_confinement_runs_free() {
    pod_testkit::need_loopback!();
    let d = dir("free");
    std::fs::write(d.join("allowed"), b"allowed").unwrap();
    std::fs::write(d.join("secret"), b"secret").unwrap();
    let exe = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let (ok, other) = ports();
    let argv: Vec<String> = [text(&exe), PAYLOAD.into(), text(&d.join("secret")), text(&d.join("allowed"))]
        .into_iter()
        .chain([port(&ok).to_string(), port(&other).to_string()])
        .collect();
    let mut p = plan(argv.clone(), None);
    let m = Maps::of_root(&text(&d));
    let root = RootDir::open(m.rootfs()).unwrap();
    let r = run(&mut p, 0, |p| spawn_userland(&root, p, &m, argv, "userland", None, &mut std::io::sink())).unwrap();
    assert!(r.line.starts_with("secret=0 allowed=0 connect=0 other=0 "), "{r:?}");
    assert!(r.line.contains(" setns=-9"), "setns(-1) with no filter is EBADF: {r:?}");
    let mine = std::fs::read_to_string("/proc/self/status").unwrap();
    let mine = mine.lines().find(|l| l.starts_with("Seccomp:")).unwrap_or_default().to_string();
    assert!(r.status.contains(&mine), "the payload has a filter that this process has not: {r:?}");
    assert_eq!(r.code, 0);
    let _ = std::fs::remove_dir_all(d);
}

//! The mounts of the namespace rung are locked against its payload, and the
//! payload cannot leave its root (PodUtils/podvm#695, T-1560 and T-1561).
//!
//! The payload is this binary (first argument `probe`), in an image of it
//! (`confine/image.rs`): it calls `mount(2)`, `umount2(2)` and `chroot(2)`
//! directly, as busybox `mount` does not send a bind remount. It gets
//! `EPERM` on a remount that clears `ro` of a volume or `nosuid` of `/proc`,
//! and on a remount of a tmpfs of the plan with another size; `EINVAL` on
//! an unmount of a volume; and the way out of a `chroot(2)` reaches no host
//! path. A join lands in the namespaces of the payload and gets the same.
//! A read-only root refuses a write with `EROFS`, and `/tmp`, `/dev`, a
//! volume and a tmpfs stay writable.
//!
//! Each case runs with and with no PID namespace, on each path: as root
//! (host root), as a user with no capabilities (uid 65534 where the test
//! runs as root: the user namespace of the rung), and as uid 0 of a user
//! namespace of one id (podvm's foreground run). A path that this host
//! does not give says `skipping`; `scripts/ci/userns-tests.sh` fails on
//! that line. The target has its own `main` (`harness = false`): a child
//! then forks from a process with one thread.

#[cfg(target_os = "linux")]
#[path = "locked/cases.rs"]
mod cases;
#[cfg(target_os = "linux")]
#[path = "confine/image.rs"]
mod image;
#[cfg(target_os = "linux")]
#[path = "locked/probe.rs"]
mod probe;

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some(probe::PROBE) => std::process::exit(probe::main(&args[2..])),
        Some(cases::CASES) => std::process::exit(cases::run_all(args.get(2).map_or("", String::as_str))),
        _ => std::process::exit(cases::each_path()),
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("skip: the namespace rung is Linux only");
}

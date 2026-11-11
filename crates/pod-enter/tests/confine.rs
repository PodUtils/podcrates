//! The confinement of a real payload (`Plan.confine`) on each rung that
//! the crate can test: the entry with no chroot (host paths), the chroot
//! sequence and the namespace rung (guest paths, in an image of this
//! binary). The target has its own `main` (`harness = false`): a child
//! then forks from a process with one thread, and the binary is the
//! payload too (first argument `payload`). A host other than Linux has no
//! confinement to test.

#[cfg(target_os = "linux")]
#[path = "confine/linux.rs"]
mod linux;

#[cfg(target_os = "linux")]
fn main() {
    linux::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("skip: the confinement is Linux only");
}

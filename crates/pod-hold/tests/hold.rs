//! The holds of a real payload (`hold/linux.rs`). The target has its own
//! `main` (`harness = false`), so it needs one on each host; a host other
//! than Linux has no hold to test.

#[cfg(target_os = "linux")]
#[path = "hold/linux.rs"]
mod linux;

#[cfg(target_os = "linux")]
fn main() {
    linux::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("skip: the holds are Linux only");
}

//! The veth wire of the netns hold, end to end (`veth/linux.rs`). The
//! target has its own `main` (`harness = false`), so it needs one on each
//! host; a host other than Linux has no wire to test.

#[cfg(target_os = "linux")]
#[path = "veth/linux.rs"]
mod linux;

#[cfg(target_os = "linux")]
fn main() {
    linux::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("skip: the veth wire is Linux only");
}

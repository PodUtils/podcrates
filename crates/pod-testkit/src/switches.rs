//! A gate of no blocking call in a hot path (T-838, VF-3 of the ingest
//! study). A call that blocks (a read of a file, a wait on a lock, a sleep,
//! a write to a full pipe) gives up the processor: the kernel counts a
//! voluntary switch of context for the thread. [`count`] gives the
//! voluntary switches of this thread while a closure runs, so that a test
//! holds a seal, an open or one turn of a state machine to none. A switch
//! that the scheduler forces (the time slice ended) is involuntary and does
//! not count.
//!
//! Linux only (`getrusage(RUSAGE_THREAD)`); elsewhere the count is `None`.
//! Under a user-mode emulator (qemu-user) the count is `None` too: the
//! emulator blocks for its own work (a translation of code, a lock of its
//! own), and the thread counts those switches.
//!
//! A major page fault (a page of the binary that waits for the disk) also
//! gives up the processor, but it is not a call of the code: [`count`]
//! subtracts the major faults of the thread from its voluntary switches.
//!
//! Put each gate in a test binary of its own, with no other test. The
//! threads of one process share the lock of the address space and the lock
//! of the allocator: musl's allocator unmaps and maps its groups while the
//! closure runs, and when another test thread holds the lock of the address
//! space, the call sleeps and counts as a voluntary switch (T-1082).

/// The value of `f`, and the voluntary switches of context of this thread
/// while it ran, less its major page faults; `None` where the system does
/// not count them or an emulator runs the test.
pub fn count<R>(f: impl FnOnce() -> R) -> (R, Option<u64>) {
    let before = usage();
    let value = f();
    let after = usage();
    if crate::host::emulated() {
        return (value, None);
    }
    let switches = before.zip(after).map(|((sb, fb), (sa, fa))| {
        let faults = fa.saturating_sub(fb);
        sa.saturating_sub(sb).saturating_sub(faults)
    });
    (value, switches)
}

/// The voluntary switches and the major page faults of this thread.
#[cfg(target_os = "linux")]
fn usage() -> Option<(u64, u64)> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: `usage` is a valid place for one `rusage`, which the call
    // fills; RUSAGE_THREAD reads the counts of the calling thread.
    let rc = unsafe { libc::getrusage(libc::RUSAGE_THREAD, usage.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    // SAFETY: the call returned 0, so it filled `usage`.
    let usage = unsafe { usage.assume_init() };
    Some((u64::try_from(usage.ru_nvcsw).ok()?, u64::try_from(usage.ru_majflt).ok()?))
}

#[cfg(not(target_os = "linux"))]
fn usage() -> Option<(u64, u64)> {
    None
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn a_sleep_counts_and_a_computation_does_not() {
        if crate::host::emulated() {
            eprintln!("skipped: an emulator blocks for its own work, so the count is None");
            assert_eq!(count(|| ()).1, None);
            return;
        }
        let ((), slept) = count(|| std::thread::sleep(std::time::Duration::from_millis(2)));
        assert!(slept.unwrap() >= 1, "a sleep gives up the processor");
        let (sum, computed) = count(|| (0..200_000u64).map(std::hint::black_box).sum::<u64>());
        assert_eq!(sum, 199_999 * 200_000 / 2);
        assert_eq!(computed, Some(0), "a computation blocks on nothing");
    }
}

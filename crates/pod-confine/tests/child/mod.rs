//! A child that runs a body with no allocation and gives its results to
//! the test through a pipe. The test process has threads (libtest), so the
//! child of a fork must not take a lock: the body makes system calls on
//! data made before the fork, and the results are integers.

use pod_hostprobe::sys::{self as k, Sysres};

/// A result as one integer: the value, or the negative errno.
pub fn code(r: Sysres) -> i64 {
    match r {
        Ok(v) => v,
        Err(e) => -i64::from(e.0),
    }
}

/// Run `body` in a forked child and give its `N` results. The test fails
/// when the child does not give each result or does not exit with 0.
pub fn in_child<const N: usize>(body: impl FnOnce() -> [i64; N]) -> [i64; N] {
    let mut pipe = [0i32; 2];
    k::pipe2(&mut pipe, k::O_CLOEXEC).expect("a pipe");
    let (read_end, write_end) = (i64::from(pipe[0]), i64::from(pipe[1]));
    // SAFETY: the child runs `body` (system calls on data that exists),
    // `write` and `exit_group`; nothing in it allocates or locks.
    match unsafe { k::clone_fork(k::SIGCHLD) }.expect("a fork") {
        0 => {
            let _ = k::close(read_end);
            for v in body() {
                let _ = k::write(write_end, &v.to_ne_bytes());
            }
            k::exit_group(0)
        }
        pid => {
            let _ = k::close(write_end);
            let mut bytes = Vec::new();
            let mut buf = [0u8; 512];
            loop {
                match k::read(read_end, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => bytes.extend_from_slice(&buf[..n as usize]),
                    Err(e) if e == k::EINTR => continue,
                    Err(e) => panic!("the read of the results of the child: {}", e.name()),
                }
            }
            let _ = k::close(read_end);
            let mut status = 0;
            k::wait4(pid, &mut status).expect("the wait of the child");
            assert_eq!(status, 0, "the child ended with the wait status {status}; results {bytes:?}");
            assert_eq!(bytes.len(), N * 8, "the child gave {} bytes of {} results", bytes.len(), N);
            let mut out = [0i64; N];
            for (v, b) in out.iter_mut().zip(bytes.as_chunks::<8>().0) {
                *v = i64::from_ne_bytes(*b);
            }
            out
        }
    }
}

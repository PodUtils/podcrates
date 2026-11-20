//! The forward of the waiter (`Child::wait_forwarding`): a real payload, and
//! a waiter thread that blocks the signals of the test before it sends them
//! to itself. A signal that the waiter does not read then stays pending on
//! that thread, and never ends the test process.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use pod_hostprobe::sys;

use super::waiter_forwards;
use crate::{Child, WAIT_FORWARDED};

/// `si_code` of a signal of the kernel, as a terminal sends SIGINT.
const SI_KERNEL: i32 = 0x80;
/// `si_code` of `sigqueue`.
const SI_QUEUE: i32 = -1;

/// A payload that ends with its own code on each trapped signal: 3 on
/// SIGINT, 5 on SIGQUIT, 7 on SIGUSR1. It writes "ready" after its traps,
/// so no signal comes before them. `None` where the host has no `sh`.
fn trapping_payload() -> Option<std::process::Child> {
    let script = "trap 'kill $! 2>/dev/null; exit 3' INT; trap 'kill $! 2>/dev/null; exit 5' QUIT; \
                  trap 'kill $! 2>/dev/null; exit 7' USR1; echo ready; sleep 20 & wait $!";
    let mut c = Command::new("sh").args(["-c", script]).stdout(Stdio::piped()).spawn().ok()?;
    let mut line = String::new();
    BufReader::new(c.stdout.take()?).read_line(&mut line).ok()?;
    (line == "ready\n").then_some(c)
}

/// Wait on `pid` with the forward, in a thread that blocks `mask` and then
/// calls `send` with its thread id. Gives the code and the text on `err`.
fn wait_on(pid: i64, mask: u64, send: impl FnOnce(i64) + Send + 'static) -> (Result<i32, String>, String) {
    std::thread::spawn(move || {
        let mut old = 0u64;
        sys::sigblock(mask, &mut old).expect("the mask of the test signals");
        let tid = unsafe { sys::sys(sys::SYS_GETTID, [0; 6]) }.expect("gettid");
        send(tid);
        let mut err = Vec::new();
        let code = Child::of(pid).wait_forwarding(&mut err).map_err(|e| e.to_string());
        (code, String::from_utf8_lossy(&err).into_owned())
    })
    .join()
    .unwrap()
}

/// `tgkill` to the thread `tid` of this process: `si_code` is `SI_TKILL`.
fn tgkill(tid: i64, sig: u64) {
    let me = i64::from(std::process::id());
    unsafe { sys::sys(sys::SYS_TGKILL, [me as u64, tid as u64, sig, 0, 0, 0]) }.expect("tgkill");
}

/// `rt_tgsigqueueinfo` to the calling thread `tid` with `code`. The kernel
/// takes a code of the kernel only from the thread to itself.
fn queue(tid: i64, sig: u64, code: i32) {
    let me = i64::from(std::process::id());
    // `siginfo_t`: `si_signo`, then `si_errno` and `si_code`, which mips
    // keeps in the other order.
    let code_at = if cfg!(any(target_arch = "mips", target_arch = "mips64")) { 4 } else { 8 };
    let mut info = [0u8; 128];
    info[0..4].copy_from_slice(&(sig as i32).to_ne_bytes());
    info[code_at..code_at + 4].copy_from_slice(&code.to_ne_bytes());
    let at = info.as_ptr() as u64;
    unsafe { sys::sys(sys::SYS_RT_TGSIGQUEUEINFO, [me as u64, tid as u64, sig, at, 0, 0]) }.expect("rt_tgsigqueueinfo");
}

/// A SIGHUP to the waiter reaches the payload, as SIGINT and SIGTERM do: the
/// payload ends with it and nothing stays (PodUtils/podvm#569).
#[test]
fn a_sighup_of_the_waiter_reaches_the_payload() {
    let Ok(payload) = Command::new("sleep").arg("30").spawn() else { return };
    let (code, err) = wait_on(i64::from(payload.id()), sys::sigbit(sys::SIGHUP), |tid| tgkill(tid, sys::SIGHUP));
    assert_eq!(code, Ok(129), "{err}");
    assert!(err.contains("forwarded SIGHUP"), "{err}");
}

/// A SIGUSR1 to the waiter reaches the payload, and the waiter gives the
/// code of the payload: the waiter does not die and leave the payload
/// (PodUtils/podvm#608).
#[test]
fn a_sigusr1_of_the_waiter_reaches_the_payload() {
    let Some(payload) = trapping_payload() else { return };
    let (code, err) = wait_on(i64::from(payload.id()), sys::sigbit(sys::SIGUSR1), |tid| tgkill(tid, sys::SIGUSR1));
    assert_eq!(code, Ok(7), "{err}");
    assert!(err.contains("forwarded SIGUSR1"), "{err}");
}

/// A SIGINT of the terminal (`SI_KERNEL`) reached the process group of the
/// payload already, so the waiter does not give it again; a SIGQUIT that a
/// process queued (`SI_QUEUE`) goes to the payload.
#[test]
fn a_sigint_of_the_terminal_is_not_forwarded_by_the_waiter() {
    let Some(payload) = trapping_payload() else { return };
    let mask = sys::sigbit(sys::SIGINT) | sys::sigbit(sys::SIGQUIT);
    let (code, err) = wait_on(i64::from(payload.id()), mask, |tid| {
        queue(tid, sys::SIGINT, SI_KERNEL);
        queue(tid, sys::SIGQUIT, SI_QUEUE);
    });
    assert_eq!(code, Ok(5), "{err}");
    assert!(err.contains("forwarded SIGQUIT"), "{err}");
    assert!(!err.contains("SIGINT"), "{err}");
}

/// The rule of the waiter: each signal of the list from a process; SIGINT
/// and SIGQUIT not from the kernel; no signal out of the list.
#[test]
fn the_waiter_forwards_its_list_and_not_a_signal_of_the_terminal() {
    for s in 1..=64u64 {
        let listed = WAIT_FORWARDED & sys::sigbit(s) != 0;
        assert_eq!(waiter_forwards(s as u32, 0), listed, "signal {s} from kill");
        assert_eq!(waiter_forwards(s as u32, -6), listed, "signal {s} from tgkill");
    }
    assert!(!waiter_forwards(sys::SIGINT as u32, SI_KERNEL));
    assert!(!waiter_forwards(sys::SIGQUIT as u32, SI_KERNEL));
    assert!(waiter_forwards(sys::SIGQUIT as u32, SI_QUEUE));
    assert!(waiter_forwards(sys::SIGHUP as u32, SI_KERNEL), "the hangup of a terminal can reach the waiter alone");
    assert!(!waiter_forwards(sys::SIGWINCH as u32, 0));
    assert!(!waiter_forwards(sys::SIGCHLD as u32, 0));
    assert!(!waiter_forwards(0, 0));
    assert!(!waiter_forwards(65, 0));
}

//! **`termios::Tty` against a pty of the kernel.**
//!
//! Each case of a table gives the same state and the same steps to a pty of
//! the kernel and to a `Tty`: the input from the user (a write to the
//! master), a new state (`tcsetattr`), or the output of the child (a write
//! to the slave). After each step the bytes for the child (each read of the
//! slave) and the bytes for the user (the reads of the master: the echo and
//! the output) must be equal. The table holds the cases of `termios.rs` and
//! `termios_modes.rs`, except the flow control (`^S` holds the output of
//! the master, where `Tty` gives an event to the caller), the signals
//! themselves (the test process has no terminal, so the kernel sends none),
//! the size of the window, and the values that are data only. A host where
//! `/dev/ptmx` does not open skips the test with the reason.
//!
//! The kernel reads the input on a work queue, so the test waits, for each
//! step, until the reads give the bytes of `Tty` or 2 s pass, then 30 ms
//! more for bytes past them.

#![cfg(target_os = "linux")]

mod common;

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::{Duration, Instant};

use common::tty::got;
use pod_term::termios::flags::*;
use pod_term::termios::{LINE_MAX, Termios, Tty, When};

/// One step of a case.
enum Step {
    /// Bytes from the user.
    In(Vec<u8>),
    /// A new state, by a change of the current one.
    Set(fn(&mut Termios), When),
    /// Bytes of the child.
    Out(Vec<u8>),
}

struct Case {
    name: &'static str,
    state: fn(&mut Termios),
    steps: Vec<Step>,
}

fn case(name: &'static str, state: fn(&mut Termios), steps: Vec<Step>) -> Case {
    Case { name, state, steps }
}

fn input(bytes: &[u8]) -> Step {
    Step::In(bytes.to_vec())
}

fn sane(_: &mut Termios) {}

fn raw(s: &mut Termios) {
    s.make_raw();
}

/// The cases of `termios.rs` and `termios_modes.rs`, in their order.
fn cases() -> Vec<Case> {
    vec![
        case("a line at its end", sane, vec![input(b"ab"), input(b"\r")]),
        case("VERASE", sane, vec![input(b"abc\x7f\r")]),
        case("an erase on an empty line", sane, vec![input(b"\x7f\x15\x17")]),
        case("VKILL with ECHOKE", sane, vec![input(b"ab\x15"), input(b"\r")]),
        case("VKILL with no ECHOKE", |s| s.lflag &= !ECHOKE, vec![input(b"ab\x15"), input(b"c\r")]),
        case("VKILL with no ECHOK", |s| s.lflag &= !(ECHOKE | ECHOK), vec![input(b"ab\x15")]),
        case("VWERASE", sane, vec![input(b"foo bar  \x17"), input(b"\r")]),
        case("VWERASE at punctuation", sane, vec![input(b"a.b\x17\r")]),
        case("VEOF", sane, vec![input(b"ab\x04"), input(b"\x04")]),
        case(
            "VEOL and VEOL2",
            |s| {
                s.cc[VEOL] = b';';
                s.cc[VEOL2] = b'|';
            },
            vec![input(b"a;b|")],
        ),
        case(
            "VEOL2 with no IEXTEN",
            |s| {
                s.cc[VEOL2] = b'|';
                s.lflag &= !IEXTEN;
            },
            vec![input(b"c|\r")],
        ),
        case("a disabled character", sane, vec![input(b"a\0b\r")]),
        case("VLNEXT", sane, vec![input(b"\x16\x03\r")]),
        case("VREPRINT", sane, vec![input(b"ab\x12"), input(b"\r")]),
        case("no IEXTEN", |s| s.lflag &= !IEXTEN, vec![input(b"a\x17\x16\x12\r")]),
        case("LINE_MAX", sane, vec![input(&[b'a'; LINE_MAX + 2]), input(b"\r")]),
        case("IUTF8", |s| s.iflag |= IUTF8, vec![input("aé\x7f\r".as_bytes())]),
        case("no IUTF8", sane, vec![input("aé\x7f\r".as_bytes())]),
        case("the erase of a tab", sane, vec![input(b"ab\t\x7f")]),
        case("the erase of a tab after a prompt", sane, vec![Step::Out(b"$ ".to_vec()), input(b"ab\t\x7f")]),
        case("the erase of a control character", sane, vec![input(b"\x16\x01\x7f")]),
        case("no ECHO", |s| s.lflag &= !ECHO, vec![input(b"ab\x7fc\x12\x15d\r")]),
        case(
            "ECHONL",
            |s| {
                s.lflag &= !ECHO;
                s.lflag |= ECHONL;
            },
            vec![input(b"ab\r")],
        ),
        case("ECHOCTL", sane, vec![input(b"\x16\x7f\x16\x1b\r")]),
        case("no ECHOCTL", |s| s.lflag &= !ECHOCTL, vec![input(b"\x16\x01\r")]),
        case("no ECHOE", |s| s.lflag &= !ECHOE, vec![input(b"ab\x7f\r")]),
        case("the echo with no ONLCR", |s| s.oflag &= !ONLCR, vec![input(b"a\r")]),
        case("the echo with no OPOST", |s| s.oflag &= !OPOST, vec![input(b"a\r")]),
        case("a tab", sane, vec![input(b"\t")]),
        // The modes. A signal with a flush comes in its own write: the kernel
        // drops an echo that it did not write yet.
        case("VINTR", sane, vec![input(b"ab"), input(b"\x03"), input(b"\r")]),
        case("NOFLSH", |s| s.lflag |= NOFLSH, vec![input(b"ab\x03\r")]),
        case("VQUIT and VSUSP", sane, vec![input(b"\x1c"), input(b"\x1a")]),
        case("no ISIG", |s| s.lflag &= !ISIG, vec![input(b"\x03\x1c\r")]),
        case("a signal with no ECHO", |s| s.lflag &= !ECHO, vec![input(b"\x03")]),
        case("ICRNL", sane, vec![input(b"a\r")]),
        case("IGNCR", |s| s.iflag |= IGNCR, vec![input(b"a\r\n")]),
        case("no ICRNL", |s| s.iflag &= !ICRNL, vec![input(b"a\r\n")]),
        case(
            "INLCR, raw",
            |s| {
                raw(s);
                s.iflag |= INLCR;
            },
            vec![input(b"\n")],
        ),
        case("INLCR", |s| s.iflag |= INLCR, vec![input(b"a\n\r")]),
        case("ISTRIP", |s| s.iflag |= ISTRIP, vec![input(&[0xe1]), input(&[0x83])]),
        case("VLNEXT of a flow character", sane, vec![input(b"\x16\x13\r")]),
        case("ONLCR", sane, vec![Step::Out(b"a\nb\n".to_vec())]),
        case("no OPOST", |s| s.oflag &= !OPOST, vec![Step::Out(b"a\nb\r".to_vec())]),
        case("OCRNL", |s| s.oflag |= OCRNL, vec![Step::Out(b"a\rb".to_vec())]),
        case("ONOCR", |s| s.oflag |= ONOCR, vec![Step::Out(b"\rab\r".to_vec())]),
        case("ONLRET", |s| s.oflag = OPOST | ONOCR | ONLRET, vec![Step::Out(b"ab\n\r".to_vec())]),
        case("raw", raw, vec![input(b"ab\x03\x7f\r"), Step::Out(b"x\n".to_vec())]),
        case("no ICANON, an erase", |s| s.lflag &= !ICANON, vec![input(b"a\x7f")]),
        case("no ICANON, a newline", |s| s.lflag &= !ICANON, vec![input(b"\r")]),
        case("TCSETS to raw", sane, vec![input(b"ab"), Step::Set(raw, When::Now), input(b"c")]),
        case("TCSETSF to raw", sane, vec![input(b"ab"), Step::Set(raw, When::Flush), input(b"c")]),
        case("TCSETSW, no ECHO", sane, vec![input(b"ab"), Step::Set(|s| s.lflag &= !ECHO, When::Drain), input(b"c\r")]),
        case(
            "a switch of ICANON after VLNEXT",
            sane,
            vec![input(b"\x16"), Step::Set(|s| s.lflag &= !ICANON, When::Now), input(b"\x03")],
        ),
    ]
}

/// What one step gave: the reads of the slave, and the bytes of the master.
#[derive(Debug, Default, PartialEq)]
struct Seen {
    child: Vec<Vec<u8>>,
    user: Vec<u8>,
}

impl Seen {
    /// The reads of the child as `Tty` gives them: one line each in the
    /// canonical mode, joined with no `ICANON`, where a read takes what came.
    fn shaped(mut self, canonical: bool) -> Seen {
        if !canonical && !self.child.is_empty() {
            self.child = vec![self.child.concat()];
        }
        self
    }

    fn show(&self) -> String {
        let child: Vec<String> = self.child.iter().map(|c| pod_term::bytes::quoted(c)).collect();
        format!("child={child:?} user={}", pod_term::bytes::quoted(&self.user))
    }
}

/// A pty of the kernel: the master, and the slave with no controlling
/// terminal, both with no wait.
struct Pty {
    master: OwnedFd,
    slave: OwnedFd,
}

fn errno(what: &str) -> String {
    format!("{what}: {}", std::io::Error::last_os_error())
}

impl Pty {
    fn open() -> Result<Pty, String> {
        // SAFETY: each call gets a descriptor that this function owns, or -1.
        unsafe {
            let m = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC);
            if m < 0 {
                return Err(errno("posix_openpt(/dev/ptmx)"));
            }
            let master = OwnedFd::from_raw_fd(m);
            if libc::grantpt(m) != 0 || libc::unlockpt(m) != 0 {
                return Err(errno("grantpt or unlockpt"));
            }
            let mut name = [0 as libc::c_char; 128];
            if libc::ptsname_r(m, name.as_mut_ptr(), name.len()) != 0 {
                return Err(errno("ptsname_r"));
            }
            let s = libc::open(name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC);
            if s < 0 {
                return Err(errno("open of the slave"));
            }
            Ok(Pty { master, slave: OwnedFd::from_raw_fd(s) })
        }
    }

    fn set(&self, state: &Termios, when: When) {
        let fd = self.slave.as_raw_fd();
        // SAFETY: `t` is a whole `termios` that `tcgetattr` fills.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(fd, &mut t), 0, "{}", errno("tcgetattr"));
            t.c_iflag = state.iflag;
            t.c_oflag = state.oflag;
            t.c_cflag = state.cflag;
            t.c_lflag = state.lflag;
            t.c_cc[..NCCS].copy_from_slice(&state.cc);
            let action = match when {
                When::Now => libc::TCSANOW,
                When::Drain => libc::TCSADRAIN,
                When::Flush => libc::TCSAFLUSH,
            };
            assert_eq!(libc::tcsetattr(fd, action, &t), 0, "{}", errno("tcsetattr"));
        }
    }

    fn write(fd: &OwnedFd, mut bytes: &[u8]) {
        let limit = Instant::now() + Duration::from_secs(5);
        while !bytes.is_empty() {
            // SAFETY: `bytes` is a live slice of its length.
            let n = unsafe { libc::write(fd.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
            if n > 0 {
                bytes = &bytes[n as usize..];
            } else {
                assert!(Instant::now() < limit, "{}", errno("a write to the pty"));
                wait(fd, libc::POLLOUT);
            }
        }
    }

    /// Each read that gives bytes now, or the end of a line (a read of 0
    /// in the canonical mode, after `VEOF` on an empty line).
    fn reads(fd: &OwnedFd, canonical: bool, into: &mut Vec<Vec<u8>>) {
        let mut buf = vec![0u8; 8192];
        for _ in 0..10_000 {
            // SAFETY: `buf` is a live buffer of its length.
            let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            match n {
                n if n > 0 => into.push(buf[..n as usize].to_vec()),
                0 if canonical => into.push(Vec::new()),
                _ => return,
            }
        }
    }
}

/// Until `fd` is ready for `events`, 10 ms at most.
fn wait(fd: &OwnedFd, events: libc::c_short) {
    let mut p = libc::pollfd { fd: fd.as_raw_fd(), events, revents: 0 };
    // SAFETY: one live `pollfd`.
    unsafe { libc::poll(&mut p, 1, 10) };
}

/// What the kernel gives after one step, read until it equals `want` or 2 s
/// pass, then 30 ms more.
fn kernel(pty: &Pty, canonical: bool, want: &Seen) -> Seen {
    let (mut child, mut user) = (Vec::new(), Vec::new());
    let collect = |child: &mut Vec<Vec<u8>>, user: &mut Vec<Vec<u8>>| {
        Pty::reads(&pty.slave, canonical, child);
        Pty::reads(&pty.master, false, user);
        Seen { child: child.clone(), user: user.concat() }.shaped(canonical)
    };
    let limit = Instant::now() + Duration::from_secs(2);
    while collect(&mut child, &mut user) != *want && Instant::now() < limit {
        wait(&pty.master, libc::POLLIN);
    }
    let quiet = Instant::now() + Duration::from_millis(30);
    while Instant::now() < quiet {
        wait(&pty.slave, libc::POLLIN);
        collect(&mut child, &mut user);
    }
    collect(&mut child, &mut user)
}

/// The differences of one case, each with its step.
fn run(case: &Case, pty: &Pty) -> Vec<String> {
    let mut state = Termios::sane();
    (case.state)(&mut state);
    pty.set(&state, When::Now);
    let mut tty = Tty::new(state);
    let mut faults = Vec::new();
    for (i, step) in case.steps.iter().enumerate() {
        let events = match step {
            Step::In(bytes) => {
                Pty::write(&pty.master, bytes);
                tty.input(bytes)
            }
            Step::Set(change, when) => {
                change(&mut state);
                pty.set(&state, *when);
                tty.set(state, *when)
            }
            Step::Out(bytes) => {
                Pty::write(&pty.slave, bytes);
                vec![pod_term::termios::Event::ToUser(tty.output(bytes))]
            }
        };
        let g = got(events);
        let canonical = tty.state().canonical();
        let want = Seen { child: g.child, user: g.user }.shaped(canonical);
        let seen = kernel(pty, canonical, &want);
        if seen != want {
            faults.push(format!("{} step {i}: the kernel {} | Tty {}", case.name, seen.show(), want.show()));
        }
    }
    faults
}

#[test]
fn tty_gives_the_bytes_of_a_kernel_pty_for_each_case() {
    let mut faults = Vec::new();
    let all = cases();
    for case in &all {
        // A new pty for each case: no column, line or state stays.
        let pty = match Pty::open() {
            Ok(pty) => pty,
            Err(why) => return pod_testkit::host::skip_line(&format!("no pty of the kernel here: {why}")),
        };
        faults.extend(run(case, &pty));
    }
    assert!(faults.is_empty(), "{} of {} cases differ:\n{}", faults.len(), all.len(), faults.join("\n"));
}

//! The payload and the run of one payload under a plan.
//!
//! The payload makes the checks of experiment 414 with raw system calls,
//! so each result is the errno of the kernel; it writes one line of
//! results to descriptor 4 and then waits for the end of descriptor 3, so
//! that the test reads its `/proc/PID/status` while it runs.

use pod_enter::{Child, Plan, Result};
use pod_hostprobe::sys::{self as k, CBuf, PollFd};

/// The first argument of the binary as a payload.
pub const PAYLOAD: &str = "payload";

fn code(r: k::Sysres) -> i64 {
    match r {
        Ok(v) => v.min(0),
        Err(e) => -i64::from(e.0),
    }
}

/// The payload: `secret`, `allowed`, the allowed port and another port.
/// Exit 0 after the line, 2 on a usage fault.
pub fn payload(args: &[String]) -> i32 {
    let [secret, allowed, ok_port, other_port] = args else { return 2 };
    let (Ok(ok_port), Ok(other_port)) = (ok_port.parse::<u16>(), other_port.parse::<u16>()) else { return 2 };
    let open = |p: &str| code(k::open(&CBuf::new(p).expect("no NUL"), k::O_RDONLY | k::O_CLOEXEC, 0));
    let connect = |port: u16| {
        let fd = k::socket(k::AF_INET, k::SOCK_STREAM | k::SOCK_CLOEXEC, 0).unwrap_or(-1);
        code(k::connect(fd, &k::sockaddr_in([127, 0, 0, 1], port)))
    };
    // SAFETY: integer arguments only.
    let setns = code(unsafe { k::sys(k::SYS_SETNS, [u64::MAX, 0, 0, 0, 0, 0]) });
    let line = format!(
        "secret={} allowed={} connect={} other={} unshare={} setns={}\n",
        open(secret),
        open(allowed),
        connect(ok_port),
        connect(other_port),
        code(k::unshare(0x1000_0000)),
        setns,
    );
    let _ = k::write(4, line.as_bytes());
    let _ = k::close(4);
    let mut b = [0u8; 1];
    while let Ok(1..) = k::read(3, &mut b) {}
    0
}

/// The first argument of the binary as the payload that writes.
pub const WRITER: &str = "writer";

/// The payload that writes: it makes each file of `paths` and writes
/// `PATH=RESULT` for each (0, or the negative errno) in one line.
pub fn writer(paths: &[String]) -> i32 {
    let mut line = String::new();
    for p in paths {
        let made = k::open(&CBuf::new(p).expect("no NUL"), k::O_CREAT | k::O_WRONLY | k::O_CLOEXEC, 0o644);
        line.push_str(&format!("{p}={} ", code(made)));
    }
    let _ = k::write(4, format!("{}\n", line.trim_end()).as_bytes());
    let _ = k::close(4);
    let mut b = [0u8; 1];
    while let Ok(1..) = k::read(3, &mut b) {}
    0
}

/// What one run gave: the line of the payload, its `NoNewPrivs` and
/// `Seccomp` lines, and its exit code.
#[derive(Debug)]
pub struct Run {
    pub line: String,
    pub status: Vec<String>,
    pub code: i32,
}

fn pipe() -> (i64, i64) {
    let mut p = [0i32; 2];
    k::pipe2(&mut p, k::O_CLOEXEC).expect("a pipe");
    (i64::from(p[0]), i64::from(p[1]))
}

/// Start the payload with `spawn` under `plan` (the two pipes become its
/// descriptors 3 and 4), read its line and its status, then let it end.
/// `Err` is the refusal of the entry.
///
/// `cover` more descriptors (of `/dev/null`) go onto the `cover` lowest
/// free numbers: the numbers that the entry takes next for its own
/// descriptors (the process has one thread, so the numbers are known).
pub fn run(
    plan: &mut Plan,
    cover: usize,
    spawn: impl FnOnce(&Plan) -> Result<Child>,
) -> std::result::Result<Run, String> {
    let (wait_r, wait_w) = pipe();
    let (line_r, line_w) = pipe();
    plan.fds.pass = vec![(3, wait_r), (4, line_w)];
    let null = CBuf::new("/dev/null").expect("no NUL");
    let open = || k::open(&null, k::O_RDONLY | k::O_CLOEXEC, 0).expect("/dev/null");
    let sources: Vec<i64> = (0..cover).map(|_| open()).collect();
    let free: Vec<i64> = (0..cover).map(|_| open()).collect();
    for (&number, &source) in free.iter().zip(&sources) {
        let _ = k::close(number);
        plan.fds.pass.push((number, source));
    }
    let spawned = spawn(plan);
    let _ = (k::close(wait_r), k::close(line_w));
    for source in sources {
        let _ = k::close(source);
    }
    let child = match spawned {
        Ok(c) => c,
        Err(e) => {
            let _ = (k::close(wait_w), k::close(line_r));
            return Err(e.to_string());
        }
    };
    let mut line = Vec::new();
    let mut buf = [0u8; 256];
    while !line.ends_with(b"\n") {
        let mut fds = [PollFd { fd: line_r as i32, events: k::POLLIN, revents: 0 }];
        if k::ppoll(&mut fds, 60_000) != Ok(1) {
            break;
        }
        match k::read(line_r, &mut buf) {
            Ok(n @ 1..) => line.extend_from_slice(&buf[..n as usize]),
            _ => break,
        }
    }
    let status = std::fs::read_to_string(format!("/proc/{}/status", child.pid)).unwrap_or_default();
    let status = status.lines().filter(|l| l.starts_with("NoNewPrivs:") || l.starts_with("Seccomp:")).map(String::from);
    let status: Vec<String> = status.collect();
    let _ = (k::close(wait_w), k::close(line_r));
    let code = child.wait().map_err(|e| e.to_string())?;
    Ok(Run { line: String::from_utf8_lossy(&line).into_owned(), status, code })
}

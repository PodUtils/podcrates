//! A server child of a test on a port of the loopback. A harness that
//! chooses a port with a bind to port 0, lets the port go and gives it to
//! its child can lose the port to another test's child before its child
//! binds it. A connection to the port is then no proof that the child is
//! up: the listener can be the other test's server, which that test stops
//! at any time. [`start`] takes a child only when the child itself holds
//! each listener of its port ([`own_listener`]), and else tries again with
//! a new port.

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::process::Child;
use std::time::{Duration, Instant};

/// How many ports [`start`] tries before it gives up.
pub const TRIES: u32 = 5;

/// A port of the loopback that nobody holds now. Another process can take
/// it before the caller binds it: a server child goes through [`start`],
/// and an address that must refuse each connect is `refused::refused`
/// (feature `refused`).
pub fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port of the loopback");
    listener.local_addr().expect("its address").port()
}

/// Start a server child with `spawn(port)` on a free port of 127.0.0.1,
/// and give the child and its port when the child itself listens there
/// within `limit` ([`own_listener`]). A child that ends, or that does not
/// listen in time, is killed, and a new port is tried, [`TRIES`] times at
/// most. `spawn` can choose more ports with [`free_port`]: a child that
/// cannot bind one of them ends, and the next try takes new ones. `Err`:
/// why the last try failed.
pub fn start(limit: Duration, mut spawn: impl FnMut(u16) -> Child) -> Result<(Child, u16), String> {
    let mut why = String::new();
    for _ in 0..TRIES {
        let port = free_port();
        let mut child = spawn(port);
        match own_listener(&mut child, SocketAddr::from((Ipv4Addr::LOCALHOST, port)), limit) {
            Ok(()) => return Ok((child, port)),
            Err(not) => {
                let _ = child.kill();
                let _ = child.wait();
                eprintln!("the server child did not take the port {port}: {not}; a new port");
                why = not;
            }
        }
    }
    Err(format!("the server child did not listen on a port of its own in {TRIES} tries: {why}"))
}

/// Wait until `child` itself holds each listener at `at`; else why not: it
/// ended, it did not listen within `limit`, or another process also
/// listens there (`SO_REUSEPORT` lets two servers of one user share a
/// port). A connection to `at` is no proof: the listener can be another
/// process's.
pub fn own_listener(child: &mut Child, at: SocketAddr, limit: Duration) -> Result<(), String> {
    let deadline = Instant::now() + limit;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("it ended ({status}) before it listened at {at}"));
        }
        match holds_listener(child.id(), at) {
            Some(true) => return Ok(()),
            Some(false) => return Err(format!("another process listens at {at} with it")),
            None if Instant::now() >= deadline => {
                return Err(format!("it did not listen at {at} within {} s", limit.as_secs()));
            }
            None => {}
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Whether the process `pid` holds each socket that listens at `at`: the
/// inodes of the listeners at `at` in `/proc/net/tcp` against the files of
/// the process. `None`: the process holds none of them; `Some(false)`:
/// another process holds one of them too.
#[cfg(target_os = "linux")]
fn holds_listener(pid: u32, at: SocketAddr) -> Option<bool> {
    let SocketAddr::V4(v4) = at else { return None };
    // The kernel writes the address as the u32 of its bytes in the host's
    // order, and the port in the host's order, both in hex.
    let local = format!("{:08X}:{:04X}", u32::from_ne_bytes(v4.ip().octets()), v4.port());
    let table = std::fs::read_to_string("/proc/net/tcp").ok()?;
    // The columns: sl, local, remote, state (0A: listen), queues, timer,
    // retransmits, uid, timeout, inode.
    let sockets: Vec<String> = table
        .lines()
        .skip(1)
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .filter(|cols| cols.len() > 9 && cols[1] == local && cols[3] == "0A")
        .map(|cols| format!("socket:[{}]", cols[9]))
        .collect();
    let fds = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
    let own: Vec<String> = fds
        .flatten()
        .filter_map(|fd| std::fs::read_link(fd.path()).ok()?.into_os_string().into_string().ok())
        .collect();
    let held = sockets.iter().filter(|s| own.contains(s)).count();
    (held > 0).then_some(held == sockets.len())
}

/// No table of the sockets on this system: a listener at `at` while the
/// child lives.
#[cfg(not(target_os = "linux"))]
fn holds_listener(_pid: u32, at: SocketAddr) -> Option<bool> {
    std::net::TcpStream::connect_timeout(&at, Duration::from_millis(200)).is_ok().then_some(true)
}

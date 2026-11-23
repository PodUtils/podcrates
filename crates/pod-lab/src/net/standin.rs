//! The stand-in process of a network lab. The lab makes the two CAs and
//! writes the certificate file of the command, then forks twice: the
//! grandchild serves the listening sockets of [`super::Ready`], one thread
//! for each connection, and is no child of the command, so a command that
//! waits for each of its children does not wait for it. It holds a pidfd
//! of the lab process, which is the command after the exec: when the
//! command ends, the stand-in removes the certificate file and exits. With a
//! way out (`out`), it keeps the end of the socket pair of the way out. It
//! dials the served ports of the command (`local`) on the loopback.

use std::net::TcpListener;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::Arc;

use pod_hostprobe::sys as k;

use super::local::Local;
use super::out::WayOut;
use super::proxy::Ends;
use super::tls::Cas;
use super::{Ca, Net, Ready};
use crate::hold::words;

/// A network lab that runs: what the check from inside and the command
/// need.
pub struct Started {
    pub net: &'static Net,
    /// The certificates of the two CAs.
    pub cas: Arc<Cas>,
    /// The certificate file of the command (`SSL_CERT_FILE`).
    pub cert_file: PathBuf,
}

/// The certificate file: the CA of the server, the CA of the proxy where
/// the proxy terminates TLS, and with a way out the roots of the host
/// (`host`), for a server whose real chain the way out passes. A new file
/// of the temporary directory; the stand-in removes it.
fn write_cert_file(cas: &Cas, net: &Net, host: Option<&str>) -> Result<PathBuf, String> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut text = cas.of(Ca::Server).pem();
    if net.proxy_ca() {
        text.push_str(&cas.of(Ca::Proxy).pem());
    }
    // With a way out: the roots of the host after the CAs of the lab.
    if let Some(host) = host {
        text.push_str(host);
    }
    let path = std::env::temp_dir().join(format!("podlab-ca-{}.pem", k::getpid()));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Close each descriptor from 3 up but `keep`.
fn close_but(keep: &[i64]) {
    let open: Vec<i64> = match std::fs::read_dir("/proc/self/fd") {
        Ok(dir) => dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok()).collect(),
        Err(_) => (3..1024).collect(),
    };
    for fd in open.into_iter().filter(|fd| *fd >= 3 && !keep.contains(fd)) {
        let _ = k::close(fd);
    }
}

/// The start of a process of the lab that serves and never runs the
/// command (a stand-in, the way out): each descriptor but `keep` closed,
/// `/dev/null` on the standard streams, no capability, not dumpable, and
/// SIGPIPE ignored.
/// A reader of the output of the command would wait for a process that
/// holds it. A process that is not dumpable keeps its memory and its
/// `/proc/PID/environ` (the environment of the caller, the proxy of the
/// host) from a command of the same user with no capability.
pub fn shed(keep: &[i64]) {
    close_but(keep);
    if let Ok(null) =
        crate::hold::c("/dev/null").and_then(|p| k::open(&p, k::O_RDWR, 0).map_err(|e| e.name().to_string()))
    {
        for n in 0..3 {
            let _ = k::dup2(null, n);
        }
        let _ = k::close(null);
    }
    let _ = crate::hold::drop_caps();
    not_dumpable();
    ignore_sigpipe();
}

/// SIGPIPE to `SIG_IGN`: a peer that closes before an answer gives the
/// write `EPIPE`, not the end of the process. A consumer can start the lab
/// with SIGPIPE at its default, and rustls writes with `writev`, which
/// raises it (PodUtils/podvm#455). No process that calls it runs an exec,
/// so the command of the lab keeps its own disposition.
fn ignore_sigpipe() {
    use linux_raw_sys::general::{
        __NR_rt_sigaction, __kernel_sighandler_t, SIGPIPE, kernel_sigaction, kernel_sigset_t,
    };
    // SAFETY: all zero is a valid `kernel_sigaction`: no handler, no flag,
    // no mask.
    let mut act: kernel_sigaction = unsafe { std::mem::zeroed() };
    // SAFETY: `SIG_IGN` is the handler 1; the kernel reads the value, and
    // no code calls it.
    act.sa_handler_kernel = unsafe { std::mem::transmute::<usize, __kernel_sighandler_t>(1) };
    let at = std::ptr::from_ref(&act) as usize as u64;
    let args = [u64::from(SIGPIPE), at, 0, std::mem::size_of::<kernel_sigset_t>() as u64, 0, 0];
    // SAFETY: the kernel reads `act` and writes nothing (no old action).
    let _ = unsafe { k::sys(i64::from(__NR_rt_sigaction), args) };
}

/// `prctl(PR_SET_DUMPABLE, 0)`: a process of the same user with no
/// capability cannot read the memory or `/proc/PID/environ` of this
/// process. `podlab run --way-out` sets it on itself, as its environment
/// holds the proxy of the host.
pub fn not_dumpable() {
    let _ = k::set_dumpable(false);
}

/// The work of the stand-in process. It does not return.
fn serve(
    ready: Ready,
    net: &'static Net,
    cas: Arc<Cas>,
    lab: i64,
    cert_file: PathBuf,
    out: Option<WayOut>,
    served: Local,
) -> ! {
    let mut keep: Vec<i64> = ready.servers.iter().map(|l| i64::from(l.as_raw_fd())).collect();
    keep.extend(ready.proxy.iter().map(|l| i64::from(l.as_raw_fd())));
    keep.push(lab);
    keep.extend(out.as_ref().map(WayOut::fd));
    shed(&keep);
    // Each thread of the stand-in reads the way out and the served ports
    // until the exit.
    let out: Option<&'static WayOut> = out.map(|w| &*Box::leak(Box::new(w)));
    let ends = Ends { out, served: Box::leak(Box::new(served)) };
    let accept = |l: TcpListener, is_proxy: bool, cas: Arc<Cas>| {
        let _ = std::thread::Builder::new().spawn(move || {
            for tcp in l.incoming().flatten() {
                super::proxy::spawn(&cas, net, tcp, is_proxy, ends);
            }
        });
    };
    for l in ready.servers {
        accept(l, false, Arc::clone(&cas));
    }
    if let Some(l) = ready.proxy {
        accept(l, true, Arc::clone(&cas));
    }
    let mut fds = [k::PollFd { fd: lab as i32, events: k::POLLIN, revents: 0 }];
    loop {
        match k::ppoll(&mut fds, -1) {
            Ok(n) if n > 0 => break,
            Err(e) if e == k::EINTR => continue,
            _ => break,
        }
    }
    let _ = std::fs::remove_file(&cert_file);
    k::exit_group(0)
}

/// Make the CAs and the certificate file, and start the stand-in process
/// on the sockets of `ready`, with the way out `out` and the served ports
/// of the command. The lab process has one thread, so the children of the
/// fork may allocate.
pub fn start(ready: Ready, net: &'static Net, out: Option<WayOut>, served: Local) -> Result<Started, String> {
    let cas = Arc::new(Cas::new()?);
    let cert_file = write_cert_file(&cas, net, out.as_ref().map(|w| w.pem.as_str()))?;
    let fail = |why: String| {
        let _ = std::fs::remove_file(&cert_file);
        Err(why)
    };
    // The pidfd of the lab, which the stand-in inherits: the lab is the
    // command after the exec.
    let lab = match k::pidfd_open(k::getpid()) {
        Ok(fd) => fd,
        Err(e) => return fail(words("pidfd_open", e)),
    };
    // SAFETY: the lab process has one thread; each child allocates only
    // its own memory.
    let first = unsafe { k::clone_fork(k::SIGCHLD) };
    match first {
        Err(e) => {
            let _ = k::close(lab);
            fail(words("fork of the stand-in", e))
        }
        Ok(0) => {
            // SAFETY: as above.
            match unsafe { k::clone_fork(k::SIGCHLD) } {
                Ok(0) => serve(ready, net, Arc::clone(&cas), lab, cert_file.clone(), out, served),
                Ok(_) => k::exit_group(0),
                Err(_) => k::exit_group(1),
            }
        }
        Ok(pid) => {
            let _ = k::close(lab);
            let mut status = 0;
            let _ = k::wait4(pid, &mut status);
            drop(ready);
            drop(out);
            drop(served);
            if status != 0 {
                return fail(format!("the stand-in did not start: status {status}"));
            }
            Ok(Started { net, cas, cert_file })
        }
    }
}

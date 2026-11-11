//! podnet's `veth_wire_carries_a_tcp_flow`, with a stack of pod-netstack in
//! the place of the pod. A child enters a new network namespace, makes the
//! veth wire also where `/dev/net/tun` opens (`WireChoice::Veth`), and
//! gives the packet socket to the parent; a stack on the socket answers.
//! The child sends a datagram to the gateway (UDP on the wire, after ARP
//! for the gateway), connects to an address past the gateway (TCP on the
//! default route), reads a greeting, and sends 256 KiB that the stack
//! echoes. A host that refuses a network namespace, `lo`, or a veth pair
//! and a packet socket skips the test with a line `SKIP:`; another failure
//! of the wire fails it.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pod_hold::netns::{self, Frame, Tun, Wire, WireChoice};
use pod_hold::{Hold, Level, reply};
use pod_netstack::smoltcp::phy::{Checksum, ChecksumCapabilities};
use pod_netstack::{NetStack, PacketQueue, StackConfig, Subnet};
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncReadExt, AsyncWriteExt, Interest};

const NAME: &str = "veth_wire_carries_a_tcp_flow";
/// An address past the gateway (TEST-NET-1): the child reaches it on the
/// default route only.
const TARGET: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 80);
const GREETING: &[u8] = b"HELLO-FROM-TARGET\n";
const BYTES: usize = 256 * 1024;
const LIMIT: Duration = Duration::from_secs(20);

// The exit codes of the child.
const NO_NAMESPACE: i32 = 10;
const NO_HANDSHAKE: i32 = 11;
const NO_LOOPBACK: i32 = 12;
/// The host refuses the veth pair or the packet socket.
const NO_VETH: i32 = 13;
/// The wire failed for another reason: a defect.
const VETH_FAILED: i32 = 14;
const NOT_SENT: i32 = 15;
const NO_GO: i32 = 16;
const NO_ANSWER: i32 = 20;
const FLOW_FAILED: i32 = 21;

fn data() -> Vec<u8> {
    (0..BYTES).map(|i| i as u8).collect()
}

/// True when the host refuses the call (no capability in the namespace, no
/// veth module, no packet sockets), not when the wire has a defect.
fn host_refuses(e: &pod_core::Failure) -> bool {
    let errno = e.io_cause().and_then(io::Error::raw_os_error);
    [libc::EPERM, libc::EACCES, libc::EOPNOTSUPP, libc::EAFNOSUPPORT, libc::EPROTONOSUPPORT]
        .iter()
        .any(|refused| errno == Some(*refused))
}

fn wait(pid: i32) -> i32 {
    let mut status = 0;
    // SAFETY: waitpid of a child of this process.
    unsafe { libc::waitpid(pid, &mut status, 0) };
    if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { 128 + libc::WTERMSIG(status) }
}

/// The child: the namespace, `lo`, the veth wire to the parent, then the
/// datagram and the flow when the parent says go.
fn child(sock: RawFd) -> i32 {
    let tun = Tun::default();
    let Ok(userns) = netns::enter() else { return NO_NAMESPACE };
    if netns::announce(sock, userns).is_err() {
        return NO_HANDSHAKE;
    }
    if let Err(e) = netns::bringup_loopback() {
        eprintln!("{NAME}: child: {}", e.chain());
        return NO_LOOPBACK;
    }
    let wire = match netns::open_wire(&tun, WireChoice::Veth) {
        Ok((fd, Wire::Veth)) => fd,
        Ok((_, wire)) => {
            eprintln!("{NAME}: child: the wire is {}", wire.word());
            return VETH_FAILED;
        }
        Err(e) => {
            eprintln!("{NAME}: child: {}", e.chain());
            return if host_refuses(&e) { NO_VETH } else { VETH_FAILED };
        }
    };
    if netns::send_fd(sock, wire.as_raw_fd()).is_err() {
        return NOT_SENT;
    }
    // Each open packet socket of the wire end gets a copy of each frame.
    drop(wire);
    let mut go = [0u8; 1];
    if !matches!(netns::read_exact_fd(sock, &mut go), Ok(1)) {
        return NO_GO;
    }
    if let Err(e) = datagram(tun.gateway) {
        eprintln!("{NAME}: child: the datagram: {e}");
        return NO_ANSWER;
    }
    match flow() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{NAME}: child: the flow: {e}");
            FLOW_FAILED
        }
    }
}

/// A datagram to port 53 of the gateway and its answer. The kernel holds
/// the first datagram until the wire answers ARP; a lost one is sent again.
fn datagram(gateway: Ipv4Addr) -> io::Result<()> {
    let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    s.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut buf = [0u8; 64];
    for _ in 0..5 {
        s.send_to(b"which?", (gateway, 53))?;
        match s.recv_from(&mut buf) {
            Ok((n, from)) if &buf[..n] == b"answer:which?" && from == SocketAddr::from((gateway, 53)) => return Ok(()),
            Ok((n, from)) => return Err(io::Error::other(format!("{n} other bytes from {from}"))),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "no answer of the gateway"))
}

/// A connect past the gateway, the greeting, and 256 KiB out and back.
fn flow() -> io::Result<()> {
    let mut s = TcpStream::connect_timeout(&SocketAddr::V4(TARGET), LIMIT)?;
    s.set_read_timeout(Some(LIMIT))?;
    s.set_write_timeout(Some(LIMIT))?;
    let mut writer = s.try_clone()?;
    let sender = std::thread::spawn(move || writer.write_all(&data()));
    let mut got = vec![0u8; GREETING.len() + BYTES];
    s.read_exact(&mut got)?;
    sender.join().map_err(|_| io::Error::other("the sender panicked"))??;
    if got[..GREETING.len()] != *GREETING || got[GREETING.len()..] != data()[..] {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "the echo is not the data"));
    }
    Ok(())
}

fn read_frame(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
    // SAFETY: the buffer is valid for its length.
    let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
    if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
}

/// Write one frame. A frame that the kernel drops (`ENOBUFS`) is lost as
/// on a wire: TCP sends it again.
async fn send(wire: &AsyncFd<OwnedFd>, frame: &[u8]) {
    loop {
        let mut guard = wire.writable().await.expect("the readiness of the wire");
        // SAFETY: the frame is valid for its length.
        let wrote =
            guard.try_io(|fd| match unsafe { libc::write(fd.as_raw_fd(), frame.as_ptr().cast(), frame.len()) } {
                n if n < 0 => Err(io::Error::last_os_error()),
                n => Ok(n),
            });
        match wrote {
            Ok(Ok(_)) => return,
            Ok(Err(e)) if e.raw_os_error() == Some(libc::ENOBUFS) => return,
            Ok(Err(e)) => panic!("the write of a frame: {e}"),
            Err(_would_block) => continue,
        }
    }
}

/// The device of the stack on the veth wire: each frame of the program end
/// through `take_frame` (an IP packet to the stack, an ARP reply back to
/// the wire), and each packet of the stack through `frame_for`. `large`
/// counts the frames larger than the MTU: a segment of an offload that
/// stayed on, which `take_frame` drops and TCP sends again in parts.
async fn pump(wire: AsyncFd<OwnedFd>, link_in: PacketQueue, link_out: PacketQueue, tun: Tun, large: Arc<AtomicUsize>) {
    let mut buf = vec![0u8; Wire::Veth.read_buffer(tun.mtu)];
    loop {
        tokio::select! {
            ready = wire.readable() => {
                let mut guard = ready.expect("the readiness of the wire");
                loop {
                    let read = guard.try_io(|fd| read_frame(fd.as_raw_fd(), &mut buf));
                    if let Ok(Ok(n)) = read
                        && n > tun.mtu + netns::ETH_HEADER
                    {
                        large.fetch_add(1, Ordering::Relaxed);
                    }
                    match read {
                        Ok(Ok(n)) => match netns::take_frame(&buf[..n], tun.gateway, tun.mtu) {
                            Frame::Ip(packet) => {
                                link_in.push(packet);
                            }
                            Frame::Reply(frame) => send(&wire, &frame).await,
                            Frame::Drop => {}
                        },
                        Ok(Err(e)) => panic!("the read of a frame: {e}"),
                        Err(_would_block) => break,
                    }
                }
            }
            packet = link_out.recv() => {
                if let Some(frame) = netns::frame_for(&packet) {
                    send(&wire, &frame).await;
                }
            }
        }
    }
}

/// The end of the child: after the go, the child writes nothing on the
/// socket pair, so its end is readable only when it closes.
async fn child_gone(sock: RawFd) {
    // SAFETY: dup of an open descriptor.
    let copy = unsafe { libc::dup(sock) };
    assert!(copy >= 0, "dup of the socket pair");
    // SAFETY: a new descriptor that nothing else owns.
    let copy = unsafe { OwnedFd::from_raw_fd(copy) };
    // SAFETY: the OwnedFd holds `copy` open and returns it from as_raw_fd
    // until the AsyncFd drops it.
    let end = unsafe { AsyncFd::register_with_interest(copy, Interest::READABLE) }
        .map_err(|e| e.to_string())
        .expect("the socket pair in the reactor");
    let _ = end.readable().await;
}

/// The stack of the parent on the packet socket: it owns the gateway,
/// answers each datagram to port 53 of it, and takes the connection to
/// [`TARGET`] (any IP), which it greets and echoes.
async fn serve(wire: OwnedFd, sock: RawFd) {
    let tun = Tun::default();
    reply::set_nonblocking(wire.as_raw_fd()).expect("a wire with no block");
    let mut checksum = ChecksumCapabilities::default();
    if !Wire::Veth.checks_checksums() {
        checksum.ipv4 = Checksum::Tx;
        checksum.tcp = Checksum::Tx;
        checksum.udp = Checksum::Tx;
        checksum.icmpv4 = Checksum::Tx;
    }
    let config = StackConfig {
        subnet: Subnet { v4: tun.gateway, v4_prefix: tun.prefix, ..Subnet::default() },
        local_v4: tun.gateway,
        mtu: tun.mtu,
        any_ip: true,
        checksum,
        ..StackConfig::default()
    };
    let mut stack = NetStack::over_link(config);
    let mut listener = stack.listen_any(TARGET.port()).await.expect("a listener");
    let mut udp = stack.bind_udp((tun.gateway, 53)).await.expect("a UDP socket");
    // SAFETY: the OwnedFd holds the socket open and returns it from
    // as_raw_fd until the AsyncFd drops it.
    let wire = unsafe { AsyncFd::register(wire) }.map_err(|e| e.to_string()).expect("the wire in the reactor");
    let large = Arc::new(AtomicUsize::new(0));
    let pump = tokio::spawn(pump(wire, stack.link_in(), stack.link_out(), tun, large.clone()));
    let names = tokio::spawn(async move {
        let mut buf = [0u8; 512];
        while let Ok((n, from)) = udp.recv_from(&mut buf).await {
            let mut answer = b"answer:".to_vec();
            answer.extend_from_slice(&buf[..n]);
            if udp.send_to(&answer, from).await.is_err() {
                break;
            }
        }
    });
    netns::write_all_fd(sock, b"g").expect("the go of the child");
    let mut stream = tokio::select! {
        accepted = listener.accept() => accepted.expect("the connection of the child"),
        // The caller reads the exit code of the child.
        () = child_gone(sock) => {
            pump.abort();
            names.abort();
            return;
        }
    };
    assert_eq!(stream.local_addr(), SocketAddr::V4(TARGET), "the address that the child dialed");
    stream.write_all(GREETING).await.expect("the greeting");
    let mut buf = vec![0u8; 16384];
    let mut echoed = 0;
    loop {
        let n = stream.read(&mut buf).await.expect("a read of the flow");
        if n == 0 {
            break;
        }
        stream.write_all(&buf[..n]).await.expect("the echo");
        echoed += n;
    }
    assert_eq!(echoed, BYTES, "the bytes of the child");
    // create_veth turns off the offloads of the program end: no frame is
    // larger than the MTU.
    assert_eq!(large.load(Ordering::Relaxed), 0, "frames larger than the MTU on the wire");
    pump.abort();
    names.abort();
    stack.shutdown().await;
}

fn veth_wire_carries_a_tcp_flow() {
    let (parent, child_end) = netns::socketpair().expect("a pair");
    // SAFETY: this process has one thread here; the child exits with _exit.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork");
    if pid == 0 {
        drop(parent);
        let code = child(child_end.as_raw_fd());
        // SAFETY: the end of the child.
        unsafe { libc::_exit(code) };
    }
    drop(child_end);
    // SAFETY: getuid and getgid do not fail.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    let userns = match netns::answer(parent.as_raw_fd(), pid as u32, uid, gid) {
        Ok(userns) => userns,
        Err(e) => {
            let code = wait(pid);
            println!("SKIP: {NAME}: the host refuses a network namespace (child exit {code}): {}", e.chain());
            return;
        }
    };
    let Ok(wire) = netns::recv_fd(parent.as_raw_fd()) else {
        match wait(pid) {
            NO_LOOPBACK => println!("SKIP: {NAME}: the namespace cannot set up lo (no capability)"),
            NO_VETH => println!("SKIP: {NAME}: the host refuses a veth pair or a packet socket"),
            code => panic!("the child made no veth wire: exit {code}"),
        }
        return;
    };
    assert_eq!(Wire::of(wire.as_raw_fd()).expect("fstat"), Wire::Veth);
    let hold = Hold::netns(pid, userns, Wire::Veth, "eth0");
    assert_eq!(hold.level, Level::Enforced);
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
    // The child gives up in its own limits (10 s for the datagram, 20 s for
    // each step of the flow); the parent waits a little longer.
    let served = runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(45), serve(wire, parent.as_raw_fd())).await });
    let code = wait(pid);
    assert!(served.is_ok(), "no flow in 45 s; the exit of the child: {code}");
    assert_eq!(code, 0, "the exit of the child: the datagram and the flow on the veth wire");
    println!("ok: {NAME}");
}

pub fn main() {
    veth_wire_carries_a_tcp_flow();
}

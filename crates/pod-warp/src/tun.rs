//! A TUN interface whose packets go through a WARP session (podnet's `warp
//! tun`, Linux only).
//!
//! This mode needs `CAP_NET_ADMIN` and `/dev/net/tun`. [`Tun::create`]
//! makes the interface, sets the address of the tunnel and the MTU, and
//! brings it up; [`run`] moves the packets between the interface and a
//! MASQUE session. Neither changes a route: [`crate::route::Routes`] does,
//! for the caller that asks for it. Do not send the front door through the
//! interface, or the session goes into its own tunnel.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::time::Duration;

use pod_core::{Class, Failure, Report, Stage};
use tokio::io::unix::AsyncFd;

use crate::live::Probe;
use crate::session::MasqueSession;

pub mod probe;
pub mod rp_filter;

use rp_filter::RpFilter;

/// `TUNSETIFF`, `_IOW('T', 202, int)`: powerpc, mips and sparc put three
/// bits of direction at bit 29, so the x86_64 number 0x4004_54CA is wrong
/// there.
const TUNSETIFF: libc::c_ulong = {
    let three_bit = cfg!(any(
        target_arch = "powerpc",
        target_arch = "powerpc64",
        target_arch = "mips",
        target_arch = "mips64",
        target_arch = "sparc64"
    ));
    let (write, shift) = if three_bit { (4u32, 29) } else { (1u32, 30) };
    ((write << shift) | ((size_of::<libc::c_int>() as u32) << 16) | ((b'T' as u32) << 8) | 202) as libc::c_ulong
};
const IFF_TUN: libc::c_short = 0x0001;
const IFF_NO_PI: libc::c_short = 0x1000;
/// The interval of the liveness check of an idle session.
pub const KEEPALIVE: Duration = Duration::from_secs(15);

/// An open TUN interface.
pub struct Tun {
    fd: AsyncFd<OwnedFd>,
    /// The interface name that the kernel gave.
    pub name: String,
    /// The IPv4 address of the interface, when this value set it: the
    /// source of the TCP probe.
    pub address: Option<Ipv4Addr>,
}

fn host(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Host, class, text)
}

fn last_os(what: &str) -> Failure {
    let e = std::io::Error::last_os_error();
    Failure::caused(Stage::Host, Class::of_io(e.kind()), what.to_string(), e)
}

fn ifreq(name: &str) -> Result<libc::ifreq, Failure> {
    if name.len() >= libc::IFNAMSIZ || name.contains('\0') {
        return Err(host(Class::Usage, format!("the interface name {name:?} is too long or has a NUL byte")));
    }
    // SAFETY: a zeroed ifreq is valid.
    let mut req: libc::ifreq = unsafe { std::mem::zeroed() };
    for (slot, byte) in req.ifr_name.iter_mut().zip(name.bytes()) {
        *slot = byte as libc::c_char;
    }
    Ok(req)
}

fn ioctl(fd: RawFd, request: libc::c_ulong, req: &mut libc::ifreq, what: &str) -> Result<(), Failure> {
    // SAFETY: an ioctl of the interfaces with a live ifreq.
    if unsafe { libc::ioctl(fd, request as _, req as *mut libc::ifreq) } < 0 {
        return Err(last_os(what));
    }
    Ok(())
}

/// A datagram socket of `family` for the ioctls of the interfaces.
fn control(family: libc::c_int, what: &str) -> Result<OwnedFd, Failure> {
    // SAFETY: a socket call with constant arguments.
    let fd = unsafe { libc::socket(family, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(last_os(what));
    }
    // SAFETY: a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Write the IPv4 address `addr` into the `sockaddr` of an ifreq.
fn set_sin(slot: &mut libc::sockaddr, addr: Ipv4Addr) {
    let sin = libc::sockaddr_in {
        sin_family: libc::AF_INET as libc::sa_family_t,
        sin_port: 0,
        sin_addr: libc::in_addr { s_addr: u32::from(addr).to_be() },
        sin_zero: [0; 8],
    };
    // SAFETY: a sockaddr has the room of a sockaddr_in.
    unsafe { std::ptr::write(slot as *mut libc::sockaddr as *mut libc::sockaddr_in, sin) };
}

/// `struct in6_ifreq` of linux/ipv6.h (SIOCSIFADDR on an AF_INET6 socket).
#[repr(C)]
struct In6Ifreq {
    addr: libc::in6_addr,
    prefixlen: u32,
    ifindex: libc::c_int,
}

impl Tun {
    /// Make the interface `name` (for example `podnet0`), set `address/32`
    /// and `mtu`, and bring it up.
    pub fn create(name: &str, address: Ipv4Addr, mtu: usize) -> Result<Tun, Failure> {
        let mut req = ifreq(name)?;
        // SAFETY: a valid path; the flags are constants.
        let raw = unsafe { libc::open(c"/dev/net/tun".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC | libc::O_NONBLOCK) };
        if raw < 0 {
            return Err(last_os("opening /dev/net/tun"));
        }
        // SAFETY: a new descriptor that nothing else owns.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        req.ifr_ifru.ifru_flags = IFF_TUN | IFF_NO_PI;
        ioctl(fd.as_raw_fd(), TUNSETIFF, &mut req, "TUNSETIFF (needs CAP_NET_ADMIN)")?;
        // SAFETY: the kernel wrote a name with its NUL byte.
        let name = unsafe { std::ffi::CStr::from_ptr(req.ifr_name.as_ptr()) }.to_string_lossy().into_owned();
        // The address, the MTU and the flags are set on an ordinary socket.
        let control = control(libc::AF_INET, "a socket for the ioctls of the interface")?;
        let ctl = control.as_raw_fd();
        let mut addr = ifreq(&name)?;
        // SAFETY: the address member of the union of a zeroed ifreq.
        set_sin(unsafe { &mut addr.ifr_ifru.ifru_addr }, address);
        ioctl(ctl, libc::SIOCSIFADDR, &mut addr, "SIOCSIFADDR")?;
        let mut mask = ifreq(&name)?;
        // SAFETY: the netmask member of the union of a zeroed ifreq.
        set_sin(unsafe { &mut mask.ifr_ifru.ifru_netmask }, Ipv4Addr::BROADCAST);
        ioctl(ctl, libc::SIOCSIFNETMASK, &mut mask, "SIOCSIFNETMASK")?;
        let mut size = ifreq(&name)?;
        size.ifr_ifru.ifru_mtu = mtu as libc::c_int;
        ioctl(ctl, libc::SIOCSIFMTU, &mut size, "SIOCSIFMTU")?;
        let mut flags = ifreq(&name)?;
        ioctl(ctl, libc::SIOCGIFFLAGS, &mut flags, "SIOCGIFFLAGS")?;
        // SAFETY: SIOCGIFFLAGS wrote the flags member of the union.
        unsafe { flags.ifr_ifru.ifru_flags |= (libc::IFF_UP | libc::IFF_RUNNING) as libc::c_short };
        ioctl(ctl, libc::SIOCSIFFLAGS, &mut flags, "SIOCSIFFLAGS")?;
        Ok(Tun { fd: register(fd)?, name, address: Some(address) })
    }

    /// A TUN that another part made (podnet: the raw pod of `warp run
    /// --backend netns-raw`). The descriptor becomes non-blocking.
    pub fn from_fd(fd: OwnedFd, name: &str) -> Result<Tun, Failure> {
        // SAFETY: fcntl on a descriptor that this value owns.
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        // SAFETY: as above.
        if flags < 0 || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(last_os("making the TUN non-blocking"));
        }
        Ok(Tun { fd: register(fd)?, name: name.to_string(), address: None })
    }

    /// Add the IPv6 address of the tunnel (/128) to the interface
    /// (PodUtils/podnet issue 81). A host with no IPv6 refuses the AF_INET6
    /// socket: the failure says so, and IPv4 still works.
    pub fn add_ipv6(&self, address: Ipv6Addr) -> Result<(), Failure> {
        let control = control(libc::AF_INET6, "an AF_INET6 socket (no IPv6 on this host?)")?;
        let mut index = ifreq(&self.name)?;
        ioctl(control.as_raw_fd(), libc::SIOCGIFINDEX, &mut index, "SIOCGIFINDEX")?;
        // SAFETY: SIOCGIFINDEX wrote the index in the union.
        let ifindex = unsafe { index.ifr_ifru.ifru_ifindex };
        let mut req = In6Ifreq { addr: libc::in6_addr { s6_addr: address.octets() }, prefixlen: 128, ifindex };
        // SAFETY: an ioctl with a live in6_ifreq of the size that the kernel
        // reads.
        if unsafe { libc::ioctl(control.as_raw_fd(), libc::SIOCSIFADDR as _, &mut req as *mut In6Ifreq) } < 0 {
            return Err(last_os("SIOCSIFADDR (IPv6)"));
        }
        Ok(())
    }

    /// Read one packet.
    pub async fn read(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let mut guard = self.fd.readable().await?;
            match guard.try_io(|fd| {
                // SAFETY: a read into a live buffer of its length.
                let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n < 0 { Err(std::io::Error::last_os_error()) } else { Ok(n as usize) }
            }) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }

    /// Write one packet.
    pub async fn write(&self, packet: &[u8]) -> std::io::Result<usize> {
        loop {
            let mut guard = self.fd.writable().await?;
            match guard.try_io(|fd| {
                // SAFETY: a write of a live buffer of its length.
                let n = unsafe { libc::write(fd.as_raw_fd(), packet.as_ptr().cast(), packet.len()) };
                if n < 0 { Err(std::io::Error::last_os_error()) } else { Ok(n as usize) }
            }) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }
}

fn register(fd: OwnedFd) -> Result<AsyncFd<OwnedFd>, Failure> {
    // SAFETY: the AsyncFd owns the descriptor: it stays open while it is
    // registered.
    let registered = unsafe { AsyncFd::register(fd) };
    registered.map_err(|e| {
        let e = std::io::Error::from(e);
        Failure::caused(Stage::Host, Class::of_io(e.kind()), "registering the TUN", e)
    })
}

/// The settings of [`run`] (podnet's `--probe`, `--session-max-age` and
/// `--keep-rp-filter` of `warp tun`).
#[derive(Clone, Debug)]
pub struct TunSettings {
    /// The TCP probe of the data path: a SYN from the address of the TUN
    /// ([`probe::TunProbe`]). Only an IPv4 target counts. `None`: no probe.
    pub probe: Option<Probe>,
    /// The session ends on purpose at this age. `None`: never.
    pub max_age: Option<Duration>,
    /// The reverse path filter of the interface. `None`: no read and no
    /// write of it.
    pub rp_filter: Option<RpFilter>,
    /// The interval of the liveness check ([`KEEPALIVE`]).
    pub keepalive: Duration,
    /// A larger packet of the interface is dropped ([`crate::MTU`]).
    pub mtu: usize,
    /// The events of the caller.
    pub report: Report,
}

impl Default for TunSettings {
    fn default() -> TunSettings {
        TunSettings {
            probe: Some(crate::live::DEFAULT_PROBE),
            max_age: None,
            rp_filter: None,
            keepalive: KEEPALIVE,
            mtu: crate::MTU,
            report: Report::default(),
        }
    }
}

fn ended(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Warp, class, text)
}

/// Move packets between `tun` and `session` until one of them ends: the
/// session closes, the TCP probe gets no answer, or the session reaches
/// its age. The failure says which; the caller opens a new session.
pub async fn run(tun: &Tun, session: MasqueSession, settings: &TunSettings) -> Result<(), Failure> {
    let (mut tx, mut rx) = session.split();
    let keepalive = tx.keepalive(&rx);
    tracing::info!(interface = %tun.name, "the TUN interface is connected to WARP");
    // PodUtils/podnet@T-072: rp_filter 0 at each session, and again 2 s
    // later (udev can run after the create).
    let _recheck = settings.rp_filter.clone().map(|filter| {
        filter.apply(&tun.name, &settings.report);
        let (name, report) = (tun.name.clone(), settings.report.clone());
        Abort(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            filter.apply(&name, &report);
        }))
    });
    let state = match (tun.address, settings.probe) {
        (Some(source), Some(Probe { target: SocketAddr::V4(target), misses, .. })) => {
            Some(std::sync::Mutex::new(probe::TunProbe::new(source, target, misses)))
        }
        _ => None,
    };
    let every = settings.probe.map_or(Duration::from_secs(30), |p| p.every);
    let (rst_tx, mut rst_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(4);
    let outbound = async {
        let mut buf = vec![0u8; 65_536];
        let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
        loop {
            tokio::select! {
                n = tun.read(&mut buf) => {
                    let n = n.map_err(|e| Failure::caused(Stage::Host, Class::of_io(e.kind()), "reading the TUN interface", e))?;
                    if n == 0 {
                        continue;
                    }
                    if n > settings.mtu {
                        tracing::debug!(n, "a packet larger than the MTU of the tunnel is dropped");
                        continue;
                    }
                    tx.send_packet(&buf[..n]).await?;
                }
                _ = ticks.tick(), if state.is_some() => {
                    let tick = state.as_ref().map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).tick());
                    match tick {
                        Some(Ok(syn)) => tx.send_packet(&syn).await?,
                        Some(Err(misses)) => {
                            let text = format!("the TCP probe got no answer {misses} times: the session carries no TCP");
                            return Err::<(), _>(ended(Class::Unavailable, text));
                        }
                        None => {}
                    }
                }
                Some(rst) = rst_rx.recv() => tx.send_packet(&rst).await?,
            }
        }
    };
    let inbound = async {
        while let Some(packet) = rx.recv_packet().await? {
            // The answer to the probe does not go to the TUN.
            if let Some(state) = &state {
                let taken = state.lock().unwrap_or_else(|e| e.into_inner()).take(&packet);
                if let Some(rst) = taken {
                    if let Some(rst) = rst {
                        let _ = rst_tx.try_send(rst);
                    }
                    continue;
                }
            }
            if let Err(err) = tun.write(&packet).await {
                tracing::debug!(error = %err, "a write to the TUN interface failed");
            }
        }
        Ok::<(), Failure>(())
    };
    // PodUtils/podnet issue 104: the liveness check is its own task, not a
    // tick of the send loop (a wait for send capacity starved it).
    let alive = async {
        match keepalive {
            Some(keepalive) => keepalive.run(settings.keepalive).await,
            None => std::future::pending().await,
        }
    };
    let aged = async {
        match settings.max_age {
            Some(age) => tokio::time::sleep(age).await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        result = outbound => result,
        result = inbound => {
            result?;
            Err(ended(Class::Unavailable, "the MASQUE session ended"))
        }
        result = alive => {
            result?;
            Err(ended(Class::Unavailable, "the MASQUE session ended"))
        }
        _ = aged => Err(ended(Class::TempFail, "the session ends on purpose at its most age")),
    }
}

/// Abort the task when the value drops.
struct Abort(tokio::task::JoinHandle<()>);

impl Drop for Abort {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The number of `TUNSETIFF` of each target: `_IOW('T', 202, int)`.
    #[test]
    fn the_tunsetiff_number_of_the_target() {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "x86", target_arch = "arm"))]
        assert_eq!(TUNSETIFF, 0x4004_54CA);
        #[cfg(any(target_arch = "powerpc", target_arch = "powerpc64", target_arch = "mips", target_arch = "mips64"))]
        assert_eq!(TUNSETIFF, 0x8004_54CA);
    }

    /// Make an interface where the host permits it, and check the address.
    #[tokio::test]
    async fn creates_an_interface_with_the_address() {
        let name = format!("podwarpt{}", std::process::id() % 10_000);
        let tun = match Tun::create(&name, Ipv4Addr::new(172, 16, 0, 2), 1280) {
            Ok(tun) => tun,
            Err(err) => return pod_testkit::host::skip_line(&err.chain()),
        };
        assert_eq!(tun.name, name);
        // PodUtils/podnet issue 217: in a new network namespace (unshare
        // -n), /sys can be the sysfs of the namespace before, with no entry
        // for the new interface.
        if !std::path::Path::new(&format!("/sys/class/net/{name}")).exists() {
            return pod_testkit::host::skip_line(&format!(
                "/sys/class/net has no {name} (the sysfs of another namespace)"
            ));
        }
        let mtu = std::fs::read_to_string(format!("/sys/class/net/{name}/mtu")).unwrap();
        assert_eq!(mtu.trim(), "1280");
        let flags = std::fs::read_to_string(format!("/sys/class/net/{name}/flags")).unwrap();
        let flags = u32::from_str_radix(flags.trim().trim_start_matches("0x"), 16).unwrap();
        assert_ne!(flags & libc::IFF_UP as u32, 0, "the interface is up");
    }

    /// A name that the kernel would cut is refused before a system call.
    #[test]
    fn a_long_interface_name_is_refused() {
        let failure = Tun::create("a-name-of-twenty-bytes", Ipv4Addr::new(172, 16, 0, 2), 1280).err().unwrap();
        assert_eq!(failure.class, Class::Usage);
    }
}

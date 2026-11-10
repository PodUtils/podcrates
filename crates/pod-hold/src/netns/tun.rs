//! The TUN device of the netns hold: its setup inside the namespace (the
//! address, the MTU, the default route) with ioctls, not with `ip`:
//! RTNETLINK can fail inside a user namespace where the ioctls work.

use std::ffi::CString;
use std::net::Ipv4Addr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use pod_core::{Class, Failure};

use crate::abi::TUNSETIFF;
use crate::failure;

pub const IFF_TUN: libc::c_int = 0x0001;
pub const IFF_NO_PI: libc::c_int = 0x1000;

/// The TUN of a namespace: its name, the address of the namespace side,
/// the gateway that the supervisor's stack answers for, the prefix and the
/// MTU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tun {
    pub name: String,
    pub addr: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub prefix: u8,
    pub mtu: usize,
}

impl Default for Tun {
    /// podnet's TUN: `eth0`, the /31 of 10.66.255.254 (the gateway) and
    /// 10.66.255.255, the MTU 1500.
    fn default() -> Self {
        Tun {
            name: "eth0".into(),
            addr: Ipv4Addr::new(10, 66, 255, 255),
            gateway: Ipv4Addr::new(10, 66, 255, 254),
            prefix: 31,
            mtu: 1500,
        }
    }
}

impl Tun {
    /// The netmask of the prefix.
    pub fn netmask(&self) -> Ipv4Addr {
        let bits = u32::from(self.prefix.min(32));
        Ipv4Addr::from(u32::MAX.checked_shl(32 - bits).unwrap_or(0))
    }
}

/// An `ifreq` with the name `name` (cut at `IF_NAMESIZE - 1` bytes).
pub fn ifreq_for(name: &str) -> libc::ifreq {
    // SAFETY: a zeroed ifreq is valid.
    let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
    for (i, b) in name.bytes().enumerate().take(libc::IF_NAMESIZE - 1) {
        ifr.ifr_name[i] = b as libc::c_char;
    }
    ifr
}

/// A datagram socket for the ioctls of the interfaces.
fn control() -> Result<OwnedFd, Failure> {
    // SAFETY: socket with constant arguments.
    let sock = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if sock < 0 {
        return Err(failure::last_os("socket() for the interfaces"));
    }
    // SAFETY: a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(sock) })
}

fn set_flags(sock: &OwnedFd, name: &str, what: &str) -> Result<(), Failure> {
    let mut ifr = ifreq_for(name);
    ifr.ifr_ifru.ifru_flags = (libc::IFF_UP | libc::IFF_RUNNING) as libc::c_short;
    // SAFETY: a valid ifreq for SIOCSIFFLAGS.
    if unsafe { libc::ioctl(sock.as_raw_fd(), libc::SIOCSIFFLAGS as _, &ifr as *const _) } < 0 {
        return Err(failure::last_os(what));
    }
    Ok(())
}

/// Bring up `lo` of the namespace.
pub fn bringup_loopback() -> Result<(), Failure> {
    set_flags(&control()?, "lo", "ioctl SIOCSIFFLAGS lo")
}

/// Create and configure the TUN `tun` inside the namespace: its address,
/// its MTU, up, and the default route through its gateway. Return its fd.
pub fn create_tun(tun: &Tun) -> Result<OwnedFd, Failure> {
    let name = tun.name.as_str();
    if name.is_empty() || name.len() >= libc::IF_NAMESIZE {
        return Err(failure::host(Class::Usage, format!("the TUN name {name:?} is empty or too long")));
    }
    // SAFETY: a valid path; the flags are constants.
    let fd = unsafe { libc::open(c"/dev/net/tun".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(failure::last_os("open /dev/net/tun"));
    }
    // SAFETY: a new descriptor that nothing else owns.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut ifr = ifreq_for(name);
    ifr.ifr_ifru.ifru_flags = (IFF_TUN | IFF_NO_PI) as libc::c_short;
    // SAFETY: a valid ifreq for TUNSETIFF.
    if unsafe { libc::ioctl(owned.as_raw_fd(), TUNSETIFF as _, &mut ifr as *mut libc::ifreq) } < 0 {
        return Err(failure::last_os("ioctl TUNSETIFF"));
    }
    let sock = control()?;
    let set_addr = |cmd: libc::c_ulong, addr: Ipv4Addr, what: &str| -> Result<(), Failure> {
        let mut ifr = ifreq_for(name);
        // SAFETY: ifru_addr has the room of a sockaddr_in.
        let r = unsafe {
            let sin = &mut ifr.ifr_ifru.ifru_addr as *mut libc::sockaddr as *mut libc::sockaddr_in;
            (*sin).sin_family = libc::AF_INET as libc::sa_family_t;
            (*sin).sin_addr.s_addr = u32::from_ne_bytes(addr.octets());
            libc::ioctl(sock.as_raw_fd(), cmd as _, &ifr as *const _)
        };
        if r < 0 {
            return Err(failure::last_os(what));
        }
        Ok(())
    };
    let mut mtu = ifreq_for(name);
    mtu.ifr_ifru.ifru_mtu = tun.mtu as libc::c_int;
    // SAFETY: a valid ifreq for SIOCSIFMTU.
    if unsafe { libc::ioctl(sock.as_raw_fd(), libc::SIOCSIFMTU as _, &mtu as *const _) } < 0 {
        return Err(failure::last_os("ioctl SIOCSIFMTU"));
    }
    set_addr(libc::SIOCSIFADDR as _, tun.addr, "ioctl SIOCSIFADDR")?;
    set_addr(libc::SIOCSIFNETMASK as _, tun.netmask(), "ioctl SIOCSIFNETMASK")?;
    set_flags(&sock, name, "ioctl SIOCSIFFLAGS up")?;
    // The default route through the gateway that the supervisor's stack
    // answers for.
    // SAFETY: a zeroed rtentry is valid.
    let mut route: libc::rtentry = unsafe { std::mem::zeroed() };
    let dst = &mut route.rt_dst as *mut libc::sockaddr as *mut libc::sockaddr_in;
    let gw = &mut route.rt_gateway as *mut libc::sockaddr as *mut libc::sockaddr_in;
    let mask = &mut route.rt_genmask as *mut libc::sockaddr as *mut libc::sockaddr_in;
    // SAFETY: each field has the room of a sockaddr_in.
    unsafe {
        (*dst).sin_family = libc::AF_INET as libc::sa_family_t;
        (*gw).sin_family = libc::AF_INET as libc::sa_family_t;
        (*gw).sin_addr.s_addr = u32::from_ne_bytes(tun.gateway.octets());
        (*mask).sin_family = libc::AF_INET as libc::sa_family_t;
    }
    route.rt_flags = libc::RTF_UP | libc::RTF_GATEWAY;
    let name_c = CString::new(name).map_err(|_| failure::host(Class::Usage, "the TUN name has a NUL byte"))?;
    route.rt_dev = name_c.as_ptr() as *mut libc::c_char;
    // SAFETY: a valid rtentry; `name_c` lives for the call.
    if unsafe { libc::ioctl(sock.as_raw_fd(), libc::SIOCADDRT as _, &route as *const _) } < 0 {
        return Err(failure::last_os("ioctl SIOCADDRT"));
    }
    Ok(owned)
}

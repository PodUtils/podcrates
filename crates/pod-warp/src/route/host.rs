//! The ioctls of the routes, and [`Routes`]: the changes of a TUN that
//! takes the host, put back when the value drops.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};

use pod_core::{Class, Failure, Stage};

use super::resolv::{self, Dns, Paths};
use super::{Entry, Entry6, Exempt, Exempt6, install_all, mask, parse_entries, parse_entries6};

/// The route tables of the host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tables {
    pub v4: Vec<Entry>,
    pub v6: Vec<Entry6>,
}

impl Tables {
    /// The tables of this host: `/proc/net/route` and
    /// `/proc/net/ipv6_route` (a missing file is an empty table).
    pub fn system() -> Tables {
        let read = |path: &str| std::fs::read_to_string(path).unwrap_or_default();
        Tables { v4: parse_entries(&read("/proc/net/route")), v6: parse_entries6(&read("/proc/net/ipv6_route")) }
    }
}

fn host_io(text: String, e: std::io::Error) -> Failure {
    Failure::caused(Stage::Host, Class::of_io(e.kind()), text, e)
}

fn socket(family: libc::c_int) -> Result<OwnedFd, Failure> {
    // SAFETY: a socket call; the descriptor is owned below.
    let fd = unsafe { libc::socket(family, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(host_io("a socket for the route ioctls".into(), std::io::Error::last_os_error()));
    }
    // SAFETY: a descriptor that this function opened.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn sin(addr: Ipv4Addr) -> libc::sockaddr {
    let sin = libc::sockaddr_in {
        sin_family: libc::AF_INET as libc::sa_family_t,
        sin_port: 0,
        sin_addr: libc::in_addr { s_addr: u32::from_ne_bytes(addr.octets()) },
        sin_zero: [0; 8],
    };
    // SAFETY: sockaddr_in and sockaddr have the same size.
    unsafe { std::mem::transmute::<libc::sockaddr_in, libc::sockaddr>(sin) }
}

fn device(dev: &str) -> Result<std::ffi::CString, Failure> {
    std::ffi::CString::new(dev)
        .map_err(|_| Failure::new(Stage::Host, Class::Usage, format!("the device name {dev:?} has a NUL byte")))
}

/// A route that is there (add) or not there (delete) is no fault.
fn fine(add: bool, err: &std::io::Error) -> bool {
    (add && err.raw_os_error() == Some(libc::EEXIST)) || (!add && err.raw_os_error() == Some(libc::ESRCH))
}

/// Add (`SIOCADDRT`) or delete (`SIOCDELRT`) an IPv4 route.
fn route4(add: bool, dst: Ipv4Addr, prefix: u8, via: Option<Ipv4Addr>, dev: &str) -> Result<(), Failure> {
    let ctl = socket(libc::AF_INET)?;
    let dev_c = device(dev)?;
    // SAFETY: a zeroed rtentry is valid; its fields are set below.
    let mut rt: libc::rtentry = unsafe { std::mem::zeroed() };
    rt.rt_dst = sin(dst);
    rt.rt_genmask = sin(mask(prefix));
    rt.rt_flags = libc::RTF_UP;
    if prefix == 32 {
        rt.rt_flags |= libc::RTF_HOST;
    }
    if let Some(gw) = via {
        rt.rt_gateway = sin(gw);
        rt.rt_flags |= libc::RTF_GATEWAY;
    }
    rt.rt_dev = dev_c.as_ptr() as *mut libc::c_char;
    let cmd = if add { libc::SIOCADDRT } else { libc::SIOCDELRT };
    // SAFETY: an ioctl with a live rtentry; `dev_c` lives for the call.
    if unsafe { libc::ioctl(ctl.as_raw_fd(), cmd as _, &rt as *const libc::rtentry) } < 0 {
        let err = std::io::Error::last_os_error();
        if !fine(add, &err) {
            let verb = if add { "adding" } else { "deleting" };
            return Err(host_io(format!("{verb} the route {dst}/{prefix} on {dev}"), err));
        }
    }
    Ok(())
}

/// `struct in6_rtmsg` of linux/ipv6_route.h.
#[repr(C)]
struct In6Rtmsg {
    dst: libc::in6_addr,
    src: libc::in6_addr,
    gateway: libc::in6_addr,
    kind: u32,
    dst_len: u16,
    src_len: u16,
    metric: u32,
    info: libc::c_ulong,
    flags: u32,
    ifindex: libc::c_int,
}

/// Add or delete an IPv6 route on `dev`, through `via` when it is given.
fn route6(add: bool, dst: Ipv6Addr, prefix: u16, via: Option<Ipv6Addr>, dev: &str) -> Result<(), Failure> {
    let ctl = socket(libc::AF_INET6)?;
    let dev_c = device(dev)?;
    // SAFETY: a C string; 0 when the device does not exist.
    let ifindex = unsafe { libc::if_nametoindex(dev_c.as_ptr()) };
    if ifindex == 0 {
        return Err(Failure::new(Stage::Host, Class::NotFound, format!("no interface {dev}")));
    }
    // SAFETY: a zeroed in6_rtmsg is valid; its fields are set below.
    let mut rt: In6Rtmsg = unsafe { std::mem::zeroed() };
    rt.dst = libc::in6_addr { s6_addr: dst.octets() };
    rt.dst_len = prefix;
    rt.metric = 1;
    rt.flags = libc::RTF_UP as u32;
    if prefix == 128 {
        rt.flags |= libc::RTF_HOST as u32;
    }
    if let Some(gw) = via {
        rt.gateway = libc::in6_addr { s6_addr: gw.octets() };
        rt.flags |= libc::RTF_GATEWAY as u32;
    }
    rt.ifindex = ifindex as libc::c_int;
    let cmd = if add { libc::SIOCADDRT } else { libc::SIOCDELRT };
    // SAFETY: an ioctl with a live in6_rtmsg.
    if unsafe { libc::ioctl(ctl.as_raw_fd(), cmd as _, &mut rt as *mut In6Rtmsg) } < 0 {
        let err = std::io::Error::last_os_error();
        if !fine(add, &err) {
            return Err(host_io(format!("the IPv6 route {dst}/{prefix} on {dev}"), err));
        }
    }
    Ok(())
}

/// The halves of the IPv4 and the IPv6 address space.
const HALVES4: [(Ipv4Addr, u8); 2] = [(Ipv4Addr::new(0, 0, 0, 0), 1), (Ipv4Addr::new(128, 0, 0, 0), 1)];
const HALVES6: [(Ipv6Addr, u16); 2] = [(Ipv6Addr::UNSPECIFIED, 1), (Ipv6Addr::new(0x8000, 0, 0, 0, 0, 0, 0, 0), 1)];

/// The routes and the resolver of a TUN that takes the host; dropped, it
/// puts each change back.
#[derive(Debug)]
pub struct Routes {
    tun: String,
    /// The host routes that keep the MASQUE connection outside the tunnel.
    exempt: Vec<(Ipv4Addr, Option<Ipv4Addr>, String)>,
    /// The IPv6 host routes of the same.
    exempt6: Vec<(Ipv6Addr, Option<Ipv6Addr>, String)>,
    ipv6: bool,
    dns: Option<Dns>,
    up: bool,
}

impl Routes {
    /// Add a host route for each address of `required` and `optional` that
    /// the default routes of the TUN `tun` would capture, by the routes of
    /// `tables`. The default routes come with [`Routes::up`], the resolver
    /// with [`Routes::set_dns`]. A host with no default route works when the
    /// MASQUE connection goes through a proxy on the loopback or on a
    /// connected subnet (PodUtils/podnet issue 105).
    ///
    /// An address of `required` (the front door or the proxy of the
    /// session) with no route is `Unavailable`. An address of `optional`
    /// (the relay of a later session, of both families) with no route is
    /// skipped.
    pub fn setup(
        tun: &str,
        required: Vec<IpAddr>,
        optional: Vec<IpAddr>,
        ipv6: bool,
        tables: &Tables,
    ) -> Result<Routes, Failure> {
        let mut routes =
            Routes { tun: tun.to_string(), exempt: Vec::new(), exempt6: Vec::new(), ipv6, dns: None, up: false };
        let all = required.into_iter().map(|ip| (ip, true)).chain(optional.into_iter().map(|ip| (ip, false)));
        for (ip, needed) in all {
            let unreachable = match ip {
                IpAddr::V4(ip) => match super::plan_exempt(ip, &tables.v4, tun) {
                    Exempt::No => false,
                    Exempt::Via { dev, gateway } => {
                        route4(true, ip, 32, gateway, &dev)?;
                        routes.exempt.push((ip, gateway, dev));
                        false
                    }
                    Exempt::Unreachable => true,
                },
                IpAddr::V6(ip) => match super::plan_exempt6(ip, &tables.v6, tun) {
                    Exempt6::No => false,
                    Exempt6::Via { dev, gateway } => {
                        route6(true, ip, 128, gateway, &dev)?;
                        routes.exempt6.push((ip, gateway, dev));
                        false
                    }
                    Exempt6::Unreachable => true,
                },
            };
            if unreachable && needed {
                let text = format!(
                    "no route to {ip}, an address of the MASQUE connection: it cannot stay outside the tunnel (use a proxy on a reachable address)"
                );
                return Err(Failure::new(Stage::Host, Class::Unavailable, text));
            }
            if unreachable {
                tracing::debug!(%ip, "no route: this address of the relay is not kept outside the tunnel");
            }
        }
        Ok(routes)
    }

    /// Set the resolver of the host to `servers` ([`set_dns`]); the drop
    /// puts it back.
    pub fn set_dns(&mut self, servers: &[IpAddr], paths: &Paths) -> Result<(), Failure> {
        self.dns = Some(set_dns(&self.tun, servers, paths)?);
        Ok(())
    }

    /// The addresses that have a host route outside the tunnel.
    pub fn exempted(&self) -> Vec<IpAddr> {
        let v4 = self.exempt.iter().map(|(ip, _, _)| IpAddr::V4(*ip));
        v4.chain(self.exempt6.iter().map(|(ip, _, _)| IpAddr::V6(*ip))).collect()
    }

    /// The default routes on the TUN (the session is up). A failure after
    /// one half removes the half that is in (PodUtils/podnet issue 278).
    pub fn up(&mut self) -> Result<(), Failure> {
        let tun = self.tun.clone();
        install_all(&HALVES4, |(dst, prefix), add| route4(add, *dst, *prefix, None, &tun))?;
        if self.ipv6 {
            for (dst, prefix) in HALVES6 {
                if let Err(err) = route6(true, dst, prefix, None, &self.tun) {
                    tracing::warn!(error = %err.chain(), "no IPv6 default route on the TUN");
                    self.ipv6 = false;
                    break;
                }
            }
        }
        self.up = true;
        Ok(())
    }

    /// No default routes on the TUN (the session is down: traffic fails
    /// fast).
    pub fn down(&mut self) {
        if !self.up {
            return;
        }
        for (dst, prefix) in HALVES4 {
            let _ = route4(false, dst, prefix, None, &self.tun);
        }
        if self.ipv6 {
            for (dst, prefix) in HALVES6 {
                let _ = route6(false, dst, prefix, None, &self.tun);
            }
        }
        self.up = false;
    }
}

impl Drop for Routes {
    fn drop(&mut self) {
        self.down();
        for (ip, gateway, dev) in &self.exempt {
            let _ = route4(false, *ip, 32, *gateway, dev);
        }
        for (ip, gateway, dev) in &self.exempt6 {
            let _ = route6(false, *ip, 128, *gateway, dev);
        }
        match self.dns.take() {
            Some(Dns::Resolved) => {
                let _ = std::process::Command::new("resolvectl").args(["revert", &self.tun]).status();
            }
            Some(Dns::File { paths, .. }) => resolv::undo(&paths),
            None => {}
        }
        tracing::info!(tun = %self.tun, "the routes and the resolver are as before");
    }
}

/// Set the resolver of the host to `servers` for the TUN `tun`:
/// `resolvectl` on the TUN when systemd-resolved runs, else the resolver
/// file of `paths` ([`resolv::set_file`]).
pub fn set_dns(tun: &str, servers: &[IpAddr], paths: &Paths) -> Result<Dns, Failure> {
    let names: Vec<String> = servers.iter().map(|s| s.to_string()).collect();
    let resolved = std::path::Path::new("/run/systemd/resolve").is_dir()
        && std::process::Command::new("resolvectl")
            .arg("dns")
            .arg(tun)
            .args(&names)
            .status()
            .is_ok_and(|s| s.success());
    if resolved {
        let _ = std::process::Command::new("resolvectl").args(["domain", tun, "~."]).status();
        let _ = std::process::Command::new("resolvectl").args(["default-route", tun, "yes"]).status();
        return Ok(Dns::Resolved);
    }
    resolv::set_file(paths, &super::resolv_text(&paths.marker, servers))
}

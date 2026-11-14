//! The options of a plan for a full guest: a NIC, virtio devices, media, a
//! VNC socket, the serial line on a socket, virtio-serial channels and the
//! Hyper-V flags. Each one maps to its arguments of the emulator.
//!
//! The default is the mailbox run: no network (`-nic none`), no medium, no
//! display, and the serial line to the file of the plan.

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use pod_core::Class;

use super::{Accel, base_format};
use crate::{Result, failure};

/// The name of the virtio-serial channel of the QEMU Guest Agent.
pub const QGA_CHANNEL: &str = "org.qemu.guest_agent.0";
/// The longest path of a Unix socket: `sun_path` holds 108 bytes with its
/// NUL.
pub const MAX_SOCKET_PATH: usize = 107;
/// The most CD-ROM drives: q35 has six AHCI ports (`ide.0` to `ide.5`).
pub const MAX_CDROMS: usize = 6;
/// The Hyper-V enlightenments of a Windows guest under KVM.
pub const HYPERV_FLAGS: &[&str] = &[
    "hv_relaxed",
    "hv_vapic",
    "hv_spinlocks=0x1fff",
    "hv_vpindex",
    "hv_runtime",
    "hv_synic",
    "hv_stimer",
    "hv_reset",
    "hv_frequencies",
    "hv_time",
    "hv_tlbflush",
    "hv_ipi",
];

/// The options of a [`super::Plan`]. [`Options::check`] refuses a value
/// that the emulator cannot take.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub nic: Nic,
    /// Each virtio device one time.
    pub virtio: Vec<Virtio>,
    /// The CD-ROM and USB media, in order.
    pub media: Vec<Medium>,
    /// A VNC server on this Unix socket. The display stays `none`.
    pub vnc: Option<PathBuf>,
    /// An absolute pointer on the USB bus, for a VNC client: the inbox
    /// drivers of Windows know it.
    pub usb_tablet: bool,
    /// The serial line on this Unix socket. The file of the plan keeps a
    /// copy of each byte (`logfile`).
    pub serial_socket: Option<PathBuf>,
    /// Virtio-serial channels, each on a Unix socket.
    pub channels: Vec<Channel>,
    /// The Hyper-V enlightenments ([`HYPERV_FLAGS`]) on the CPU line. They
    /// need KVM: under TCG the CPU line stays `max`.
    pub hyperv: bool,
}

/// The network of the guest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Nic {
    /// No network: `-nic none`.
    #[default]
    None,
    /// User-mode networking of the emulator.
    User(UserNet),
}

/// `-nic user`: a NAT in the emulator, with no privilege on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserNet {
    pub model: NicModel,
    /// The ports of the host that go to the guest.
    pub forwards: Vec<Forward>,
    /// `restrict=on`: the guest reaches no address outside, and the
    /// forwards still work.
    pub restrict: bool,
}

/// The device of the NIC.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NicModel {
    /// Intel 82574: Windows has its driver in the box.
    #[default]
    E1000e,
    /// virtio-net: faster, with a driver from virtio-win.
    VirtioNet,
}

impl NicModel {
    pub fn word(self) -> &'static str {
        match self {
            NicModel::E1000e => "e1000e",
            NicModel::VirtioNet => "virtio-net-pci",
        }
    }
}

/// The protocol of a forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Tcp,
    Udp,
}

impl Proto {
    pub fn word(self) -> &'static str {
        match self {
            Proto::Tcp => "tcp",
            Proto::Udp => "udp",
        }
    }
}

/// One `hostfwd`: a port of the host to a port of the guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Forward {
    pub proto: Proto,
    /// The address of the host to listen on.
    pub host: Ipv4Addr,
    pub host_port: u16,
    pub guest_port: u16,
}

impl Forward {
    /// A forward on the loopback of the host: no other host reaches the
    /// guest.
    pub fn local(proto: Proto, host_port: u16, guest_port: u16) -> Forward {
        Forward { proto, host: Ipv4Addr::LOCALHOST, host_port, guest_port }
    }

    /// The value of `hostfwd=`.
    pub fn spec(&self) -> String {
        format!("{}:{}:{}-:{}", self.proto.word(), self.host, self.host_port, self.guest_port)
    }
}

/// A virtio device with no backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Virtio {
    Rng,
    Balloon,
    Keyboard,
    Tablet,
}

impl Virtio {
    pub fn device(self) -> &'static str {
        match self {
            Virtio::Rng => "virtio-rng-pci",
            Virtio::Balloon => "virtio-balloon-pci",
            Virtio::Keyboard => "virtio-keyboard-pci",
            Virtio::Tablet => "virtio-tablet-pci",
        }
    }
}

/// The bus of a medium.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediumKind {
    /// A CD-ROM on the AHCI bus, read-only, raw (an ISO).
    Cdrom,
    /// A USB stick on the xHCI bus, in the format of its name
    /// ([`base_format`]).
    Usb,
}

/// One medium.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Medium {
    pub kind: MediumKind,
    pub path: PathBuf,
    /// Read-only. A CD-ROM is always read-only.
    pub readonly: bool,
    /// The firmware boots from this medium first (`bootindex=0`).
    pub boot: bool,
}

/// One virtio-serial channel on a Unix socket of the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    /// The name that the guest sees, for example [`QGA_CHANNEL`].
    pub name: String,
    pub socket: PathBuf,
}

impl Options {
    /// Refuse a value that the emulator cannot take, with [`Class::Usage`]:
    /// a port 0, a forward of one port of the host two times, a socket
    /// path that is empty, longer than [`MAX_SOCKET_PATH`] or has a NUL, a
    /// medium with no path, more than [`MAX_CDROMS`] CD-ROMs, two media to
    /// boot first, a virtio device two times, and a channel name that is
    /// empty, has a byte other than ASCII letters, digits, `.`, `_` and
    /// `-`, or comes two times.
    pub fn check(&self) -> Result<()> {
        if let Nic::User(u) = &self.nic {
            for (i, f) in u.forwards.iter().enumerate() {
                if f.host_port == 0 || f.guest_port == 0 {
                    return Err(usage(format!("the forward {} has a port 0", f.spec())));
                }
                let twice =
                    u.forwards[..i].iter().any(|g| (g.proto, g.host, g.host_port) == (f.proto, f.host, f.host_port));
                if twice {
                    return Err(usage(format!("the port {} of the host is forwarded two times", f.host_port)));
                }
            }
        }
        for (i, v) in self.virtio.iter().enumerate() {
            if self.virtio[..i].contains(v) {
                return Err(usage(format!("the device {} is named two times", v.device())));
            }
        }
        for m in &self.media {
            if m.path.as_os_str().is_empty() {
                return Err(usage("a medium has no path"));
            }
        }
        if self.media.iter().filter(|m| m.kind == MediumKind::Cdrom).count() > MAX_CDROMS {
            return Err(usage(format!("more than {MAX_CDROMS} CD-ROM drives")));
        }
        if self.media.iter().filter(|m| m.boot).count() > 1 {
            return Err(usage("two media boot first"));
        }
        for path in self.vnc.iter().chain(&self.serial_socket).chain(self.channels.iter().map(|c| &c.socket)) {
            check_socket(path)?;
        }
        for (i, c) in self.channels.iter().enumerate() {
            let good = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-');
            if c.name.is_empty() || !c.name.bytes().all(good) {
                return Err(usage(format!("{:?} is not a name of a channel", c.name)));
            }
            if self.channels[..i].iter().any(|d| d.name == c.name) {
                return Err(usage(format!("the channel {} is named two times", c.name)));
            }
        }
        Ok(())
    }

    /// The CPU line: the model of the accelerator, with the Hyper-V flags
    /// under KVM.
    pub(crate) fn cpu(&self, accel: Accel) -> String {
        match accel {
            Accel::Kvm if self.hyperv => format!("{},{}", accel.cpu(), HYPERV_FLAGS.join(",")),
            _ => accel.cpu().to_string(),
        }
    }

    /// The value of `-nic`.
    pub(crate) fn nic(&self) -> String {
        match &self.nic {
            Nic::None => "none".to_string(),
            Nic::User(u) => {
                let mut s = format!("user,model={}", u.model.word());
                if u.restrict {
                    s.push_str(",restrict=on");
                }
                for f in &u.forwards {
                    s.push_str(",hostfwd=");
                    s.push_str(&f.spec());
                }
                s
            }
        }
    }

    /// The devices: virtio, the media, the USB bus and its tablet, the
    /// virtio-serial channels and VNC.
    pub(crate) fn devices(&self, a: &mut Vec<String>) {
        let mut pair = |flag: &str, value: String| {
            a.push(flag.to_string());
            a.push(value);
        };
        for v in &self.virtio {
            pair("-device", v.device().to_string());
        }
        let usb = self.usb_tablet || self.media.iter().any(|m| m.kind == MediumKind::Usb);
        if usb {
            pair("-device", "qemu-xhci,id=xhci".to_string());
        }
        let boot = |m: &Medium| if m.boot { ",bootindex=0" } else { "" };
        let (mut cd, mut stick) = (0usize, 0usize);
        for m in &self.media {
            let file = escape(&m.path);
            match m.kind {
                MediumKind::Cdrom => {
                    pair("-drive", format!("if=none,id=cd{cd},media=cdrom,readonly=on,format=raw,file={file}"));
                    pair("-device", format!("ide-cd,drive=cd{cd},bus=ide.{cd}{}", boot(m)));
                    cd += 1;
                }
                MediumKind::Usb => {
                    let ro = if m.readonly { ",readonly=on" } else { "" };
                    let format = base_format(&m.path);
                    pair("-drive", format!("if=none,id=usb{stick},format={format}{ro},file={file}"));
                    pair("-device", format!("usb-storage,bus=xhci.0,drive=usb{stick},removable=on{}", boot(m)));
                    stick += 1;
                }
            }
        }
        if self.usb_tablet {
            pair("-device", "usb-tablet,bus=xhci.0".to_string());
        }
        if !self.channels.is_empty() {
            pair("-device", "virtio-serial-pci,id=vser".to_string());
        }
        for (i, c) in self.channels.iter().enumerate() {
            pair("-chardev", format!("socket,id=chan{i},path={},server=on,wait=off", escape(&c.socket)));
            pair("-device", format!("virtserialport,bus=vser.0,chardev=chan{i},name={}", c.name));
        }
        if let Some(vnc) = &self.vnc {
            pair("-vnc", format!("unix:{}", escape(vnc)));
        }
    }

    /// The serial line: the file of the plan, or a socket with the file as
    /// its log.
    pub(crate) fn serial(&self, file: &Path, a: &mut Vec<String>) {
        match &self.serial_socket {
            None => a.extend(["-serial".to_string(), format!("file:{}", file.display())]),
            Some(socket) => a.extend([
                "-chardev".to_string(),
                format!(
                    "socket,id=serial0,path={},server=on,wait=off,logfile={},logappend=on",
                    escape(socket),
                    escape(file)
                ),
                "-serial".to_string(),
                "chardev:serial0".to_string(),
            ]),
        }
    }
}

/// A path as a value of an option list of the emulator: a comma ends a
/// value there, and two commas are one comma.
pub fn escape(path: &Path) -> String {
    path.display().to_string().replace(',', ",,")
}

fn check_socket(path: &Path) -> Result<()> {
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.is_empty() || bytes.contains(&0) || bytes.len() > MAX_SOCKET_PATH {
        return Err(usage(format!(
            "{} is not a path of a Unix socket: 1 to {MAX_SOCKET_PATH} bytes with no NUL",
            path.display()
        )));
    }
    Ok(())
}

fn usage(text: impl Into<String>) -> pod_core::Failure {
    failure(Class::Usage, text)
}

#[cfg(test)]
mod tests;

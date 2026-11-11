//! The wire of the netns hold: the device of the namespace whose descriptor
//! the stack of the parent reads and writes. The child makes the TUN, else
//! a veth pair (a namespace that cannot open `/dev/net/tun`), and sends the
//! descriptor; the parent reads the wire from the descriptor. The device of
//! the stack (its queues, its medium) is the consumer's.

use std::os::fd::{OwnedFd, RawFd};

use pod_core::{Failure, Stage};

use super::tun::{Tun, create_tun};
use super::veth::{ETH_HEADER, WIRE_NAME, create_veth};
use crate::failure;

/// The wire of the namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Wire {
    /// The TUN: each read and each write is one IP packet.
    Tun,
    /// The veth pair: each read and each write is one Ethernet frame of a
    /// packet socket on the wire end ([`super::take_frame`],
    /// [`super::frame_for`]).
    Veth,
}

impl Wire {
    /// The wire of the descriptor `fd` that the child sent: a socket is the
    /// veth wire, else the TUN.
    pub fn of(fd: RawFd) -> Result<Wire, Failure> {
        // SAFETY: a zeroed stat is valid; fstat fills it.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `st` lives for the call.
        if unsafe { libc::fstat(fd, &mut st) } != 0 {
            return Err(failure::last_os("fstat of the wire"));
        }
        Ok(if st.st_mode & libc::S_IFMT == libc::S_IFSOCK { Wire::Veth } else { Wire::Tun })
    }

    /// The word of the wire: `tun` or `veth`.
    pub fn word(self) -> &'static str {
        match self {
            Wire::Tun => "tun",
            Wire::Veth => "veth",
        }
    }

    /// True when the stack can check the checksums of the packets of the
    /// program. The program end of a veth pair can send a partial checksum
    /// (an offload that stays on): on that wire the stack computes each sum
    /// and checks none.
    pub fn checks_checksums(self) -> bool {
        self == Wire::Tun
    }

    /// The bytes of a buffer for one read of the wire with the MTU `mtu`.
    /// A read of the TUN is one packet. A read of the veth wire is one frame
    /// with its header, and a segment of an offload can be 64 KiB: it is
    /// read whole, so that the next read starts at a frame, and
    /// [`super::take_frame`] drops it.
    pub fn read_buffer(self, mtu: usize) -> usize {
        match self {
            Wire::Tun => mtu,
            Wire::Veth => 65536 + ETH_HEADER,
        }
    }
}

/// The wire that the child makes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WireChoice {
    /// The TUN, else the veth pair (podnet's default).
    #[default]
    Auto,
    /// The TUN only.
    Tun,
    /// The veth pair, also where `/dev/net/tun` opens (podnet's test
    /// variable `PODNET_NETNS_TEST_VETH`).
    Veth,
}

/// Make the wire of `tun` in the namespace, after
/// [`super::bringup_loopback`]: the TUN, or the veth pair of `tun.name` and
/// [`WIRE_NAME`], as `choice` says. Return the descriptor for the parent
/// and its wire. With [`WireChoice::Auto`], a failure of both names both.
///
/// A child keeps its copy of a TUN open, and closes its copy of the packet
/// socket after it sends it ([`create_veth`]).
pub fn open_wire(tun: &Tun, choice: WireChoice) -> Result<(OwnedFd, Wire), Failure> {
    match choice {
        WireChoice::Tun => Ok((create_tun(tun)?, Wire::Tun)),
        WireChoice::Veth => Ok((create_veth(tun, WIRE_NAME)?, Wire::Veth)),
        WireChoice::Auto => {
            let no_tun = match create_tun(tun) {
                Ok(fd) => return Ok((fd, Wire::Tun)),
                Err(e) => e,
            };
            match create_veth(tun, WIRE_NAME) {
                Ok(fd) => Ok((fd, Wire::Veth)),
                Err(no_veth) => Err(Failure::caused(
                    Stage::Host,
                    no_veth.class,
                    format!("no TUN ({}) and no veth wire", no_tun.chain()),
                    no_veth,
                )),
            }
        }
    }
}

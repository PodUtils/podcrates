//! The codec of an `AF_INET` and an `AF_INET6` socket address, and the
//! read of the address of a call from the memory of the task.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use pod_core::Failure;

use crate::mem::Memory;

/// The most bytes of an address that the supervisor reads: the size of
/// `struct sockaddr_storage`.
pub const MOST: usize = 128;

/// The length of a `struct sockaddr_in`.
pub const IN_LEN: usize = 16;
/// The length of a `struct sockaddr_in6`.
pub const IN6_LEN: usize = 28;

/// The address of a call: its family, its IP address and its port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address {
    pub family: u16,
    pub ip: IpAddr,
    pub port: u16,
}

/// The bytes of a `struct sockaddr_in` or `struct sockaddr_in6`, with
/// `family` in its first two bytes.
pub fn encode(family: u16, ip: IpAddr, port: u16) -> Vec<u8> {
    match ip {
        IpAddr::V4(v4) => {
            let mut b = vec![0u8; IN_LEN];
            b[0..2].copy_from_slice(&family.to_ne_bytes());
            b[2..4].copy_from_slice(&port.to_be_bytes());
            b[4..8].copy_from_slice(&v4.octets());
            b
        }
        IpAddr::V6(v6) => {
            let mut b = vec![0u8; IN6_LEN];
            b[0..2].copy_from_slice(&family.to_ne_bytes());
            b[2..4].copy_from_slice(&port.to_be_bytes());
            b[8..24].copy_from_slice(&v6.octets());
            b
        }
    }
}

/// The address in `bytes` (the bytes that the task gave, at most its
/// length). `None` for another family, or for an address shorter than its
/// family needs: the supervisor reads no byte past the length of the task.
pub fn parse(bytes: &[u8]) -> Option<Address> {
    let family = u16::from_ne_bytes([*bytes.first()?, *bytes.get(1)?]);
    let port = u16::from_be_bytes([*bytes.get(2)?, *bytes.get(3)?]);
    let ip = match i32::from(family) {
        libc::AF_INET => {
            let o: [u8; 4] = bytes.get(4..8)?.try_into().ok()?;
            IpAddr::V4(Ipv4Addr::from(o))
        }
        libc::AF_INET6 => {
            let o: [u8; 16] = bytes.get(8..24)?.try_into().ok()?;
            IpAddr::V6(Ipv6Addr::from(o))
        }
        _ => return None,
    };
    Some(Address { family, ip, port })
}

/// The family in the first two bytes; `None` for fewer bytes.
pub fn family(bytes: &[u8]) -> Option<u16> {
    Some(u16::from_ne_bytes([*bytes.first()?, *bytes.get(1)?]))
}

/// Read the address of a call (`connect`, `sendto`, `bind`): `len` bytes
/// at `ptr` of the task, at most [`MOST`]. Return the bytes that were read;
/// [`parse`] gives the address.
pub fn read(memory: &Memory, ptr: u64, len: u64) -> Result<Vec<u8>, Failure> {
    let len = usize::try_from(len).unwrap_or(MOST).min(MOST);
    memory.read(ptr, len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::memfd;

    #[test]
    fn an_address_comes_back_from_its_bytes() {
        for (ip, port) in [(IpAddr::from([10, 66, 255, 254]), 53), (IpAddr::from([0xfd63, 0, 0, 0, 0, 0, 0, 1]), 443)] {
            let family = if ip.is_ipv4() { libc::AF_INET } else { libc::AF_INET6 } as u16;
            let bytes = encode(family, ip, port);
            assert_eq!(parse(&bytes), Some(Address { family, ip, port }));
            assert_eq!(super::family(&bytes), Some(family));
        }
    }

    // An address shorter than its family needs is refused: no byte past
    // the length that the task gave is read.
    #[test]
    fn a_short_address_is_refused() {
        let v4 = encode(libc::AF_INET as u16, IpAddr::from([127, 0, 0, 1]), 80);
        assert!(parse(&v4[..7]).is_none());
        assert!(parse(&v4[..8]).is_some());
        let v6 = encode(libc::AF_INET6 as u16, IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1]), 80);
        assert!(parse(&v6[..23]).is_none());
        assert!(parse(&v6[..24]).is_some());
        // A v6 family on the bytes of a v4 address.
        let mut mixed = v4.clone();
        mixed[0..2].copy_from_slice(&(libc::AF_INET6 as u16).to_ne_bytes());
        assert!(parse(&mixed).is_none());
        assert!(parse(&[]).is_none());
        assert!(parse(&[2]).is_none());
        let unix = (libc::AF_UNIX as u16).to_ne_bytes();
        assert!(parse(&[unix[0], unix[1], 0, 0, 0, 0, 0, 0]).is_none());
    }

    // The read takes the length of the task, bounded by the size of
    // sockaddr_storage, from the memory of the task.
    #[test]
    fn the_read_of_an_address_takes_the_length_of_the_task() {
        let mut image = vec![0u8; 512];
        let v4 = encode(libc::AF_INET as u16, IpAddr::from([192, 0, 2, 1]), 8080);
        image[100..100 + v4.len()].copy_from_slice(&v4);
        let memory = Memory::of_fd(memfd(&image).unwrap(), false);
        let bytes = read(&memory, 100, v4.len() as u64).unwrap();
        assert_eq!(parse(&bytes).map(|a| (a.ip, a.port)), Some((IpAddr::from([192, 0, 2, 1]), 8080)));
        assert_eq!(read(&memory, 100, 7).map(|b| parse(&b)).unwrap(), None);
        assert_eq!(read(&memory, 0, u64::MAX).unwrap().len(), MOST);
        assert!(read(&memory, 500, 64).is_err(), "a read past the memory fails");
    }
}

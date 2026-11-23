//! One TCP segment with a real 20-byte header and a real checksum.
//!
//! The simulator uses a fixed IPv4 pair (10.0.0.1 to 10.0.0.2) for the
//! pseudo-header, so two runs of the same seed give the same bytes. The
//! segment is a value: a primitive returns a changed list of segments, and
//! the simulator serialises that list.

use std::fmt;

pub const FIN: u8 = 0x01;
pub const SYN: u8 = 0x02;
pub const RST: u8 = 0x04;
pub const PSH: u8 = 0x08;
pub const ACK: u8 = 0x10;

/// The length of a TCP header with no option.
pub const HEADER: usize = 20;
/// The largest payload of one segment of the simulator.
pub const MSS: usize = 8;
/// The receiving window of the simulator, in bytes.
pub const WINDOW: u16 = 4096;

pub const CLIENT_PORT: u16 = 40000;
pub const SERVER_PORT: u16 = 80;

/// The addresses of the pseudo-header. They are fixed, so the checksum of a
/// segment is a function of its fields alone.
pub const CLIENT_ADDR: [u8; 4] = [10, 0, 0, 1];
pub const SERVER_ADDR: [u8; 4] = [10, 0, 0, 2];

/// A TCP segment.
#[derive(Clone, PartialEq, Eq)]
pub struct Segment {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq: u32,
    pub ack: u32,
    pub flags: u8,
    pub window: u16,
    pub payload: Vec<u8>,
    /// The checksum as it travels. [`Segment::to_bytes`] rewrites it from the
    /// fields, so a caller that wants an invalid checksum changes it after
    /// the call or uses [`Segment::tamper_checksum`].
    pub checksum: u16,
}

impl Segment {
    pub fn new(seq: u32, ack: u32, flags: u8, payload: impl Into<Vec<u8>>) -> Segment {
        Segment {
            src_port: CLIENT_PORT,
            dst_port: SERVER_PORT,
            seq,
            ack,
            flags,
            window: WINDOW,
            payload: payload.into(),
            checksum: 0,
        }
    }

    pub fn syn(seq: u32) -> Segment {
        Segment::new(seq, 0, SYN, Vec::new())
    }

    pub fn syn_ack(seq: u32, ack: u32) -> Segment {
        Segment::new(seq, ack, SYN | ACK, Vec::new())
    }

    pub fn ack(seq: u32, ack: u32) -> Segment {
        Segment::new(seq, ack, ACK, Vec::new())
    }

    pub fn data(seq: u32, ack: u32, payload: impl Into<Vec<u8>>) -> Segment {
        Segment::new(seq, ack, ACK | PSH, payload)
    }

    pub fn fin(seq: u32, ack: u32) -> Segment {
        Segment::new(seq, ack, FIN | ACK, Vec::new())
    }

    pub fn rst(seq: u32, ack: u32) -> Segment {
        Segment::new(seq, ack, RST | ACK, Vec::new())
    }

    /// The ports swap, so the segment goes the other way.
    pub fn reverse(&mut self) {
        std::mem::swap(&mut self.src_port, &mut self.dst_port);
    }

    pub fn is_syn(&self) -> bool {
        self.flags & SYN != 0
    }

    pub fn is_ack(&self) -> bool {
        self.flags & ACK != 0
    }

    pub fn is_fin(&self) -> bool {
        self.flags & FIN != 0
    }

    pub fn is_rst(&self) -> bool {
        self.flags & RST != 0
    }

    pub fn has_payload(&self) -> bool {
        !self.payload.is_empty()
    }

    /// The last sequence number that this segment occupies, plus one.
    pub fn end_seq(&self) -> u32 {
        let mut n = self.payload.len() as u32;
        if self.is_syn() || self.is_fin() {
            n += 1;
        }
        self.seq.wrapping_add(n)
    }

    /// The 16-bit one's-complement checksum over the pseudo-header, the
    /// header and the payload, with the fixed addresses of the simulator.
    pub fn compute_checksum(&self) -> u16 {
        self.checksum_over(CLIENT_ADDR, SERVER_ADDR)
    }

    /// The checksum over a pseudo-header with these addresses. A raw socket
    /// uses the addresses of its own path.
    pub fn checksum_over(&self, src: [u8; 4], dst: [u8; 4]) -> u16 {
        checksum_with(&src, &dst, &self.header(), &self.payload)
    }

    fn header(&self) -> [u8; HEADER] {
        let mut header = [0u8; HEADER];
        header[0..2].copy_from_slice(&self.src_port.to_be_bytes());
        header[2..4].copy_from_slice(&self.dst_port.to_be_bytes());
        header[4..8].copy_from_slice(&self.seq.to_be_bytes());
        header[8..12].copy_from_slice(&self.ack.to_be_bytes());
        header[12] = 5 << 4;
        header[13] = self.flags;
        header[14..16].copy_from_slice(&self.window.to_be_bytes());
        header
    }

    pub fn checksum_valid(&self) -> bool {
        self.checksum == self.compute_checksum()
    }

    /// Set the checksum from the fields.
    pub fn seal(&mut self) {
        self.checksum = self.compute_checksum();
    }

    /// Flip one bit of the checksum, so the segment fails a check.
    pub fn tamper_checksum(&mut self, mask: u16) {
        self.checksum ^= mask;
    }

    /// Change one payload byte. `None` when the payload is empty.
    pub fn tamper_payload(&mut self, offset: usize, mask: u8) -> Option<usize> {
        if self.payload.is_empty() {
            return None;
        }
        let at = offset % self.payload.len();
        self.payload[at] ^= mask;
        self.seal();
        Some(at)
    }

    /// The bytes of the segment with a fresh checksum.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER + self.payload.len()];
        bytes[0..2].copy_from_slice(&self.src_port.to_be_bytes());
        bytes[2..4].copy_from_slice(&self.dst_port.to_be_bytes());
        bytes[4..8].copy_from_slice(&self.seq.to_be_bytes());
        bytes[8..12].copy_from_slice(&self.ack.to_be_bytes());
        bytes[12] = 5 << 4;
        bytes[13] = self.flags;
        bytes[14..16].copy_from_slice(&self.window.to_be_bytes());
        let sum = checksum(&bytes[..HEADER], &self.payload);
        bytes[16..18].copy_from_slice(&sum.to_be_bytes());
        bytes[HEADER..].copy_from_slice(&self.payload);
        bytes
    }

    /// Read one segment. The checksum is read as it is; the caller decides
    /// whether the path requires a valid one.
    pub fn from_bytes(bytes: &[u8]) -> Result<Segment, PacketError> {
        if bytes.len() < HEADER {
            return Err(PacketError::Short(bytes.len()));
        }
        let offset = usize::from(bytes[12] >> 4) * 4;
        if offset < HEADER || offset > bytes.len() {
            return Err(PacketError::Header(offset));
        }
        Ok(Segment {
            src_port: u16::from_be_bytes([bytes[0], bytes[1]]),
            dst_port: u16::from_be_bytes([bytes[2], bytes[3]]),
            seq: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            ack: u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            flags: bytes[13],
            window: u16::from_be_bytes([bytes[14], bytes[15]]),
            checksum: u16::from_be_bytes([bytes[16], bytes[17]]),
            payload: bytes[offset..].to_vec(),
        })
    }
}

impl fmt::Debug for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Segment({}->{}, seq {}, ack {}, flags {:?}, len {}, csum {:04x})",
            self.src_port,
            self.dst_port,
            self.seq,
            self.ack,
            Flags(self.flags),
            self.payload.len(),
            self.checksum
        )
    }
}

/// The flags of a segment, for a readable debug line.
struct Flags(u8);

impl fmt::Debug for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut names = Vec::new();
        for (bit, name) in [(SYN, "SYN"), (ACK, "ACK"), (FIN, "FIN"), (RST, "RST"), (PSH, "PSH")] {
            if self.0 & bit != 0 {
                names.push(name);
            }
        }
        if names.is_empty() { f.write_str("-") } else { f.write_str(&names.join("|")) }
    }
}

/// The one's-complement sum of a buffer, the odd byte padded with zero.
fn sum(bytes: &[u8]) -> u32 {
    let mut total = 0u32;
    for pair in bytes.chunks(2) {
        let word = if pair.len() == 2 { u16::from_be_bytes([pair[0], pair[1]]) } else { u16::from(pair[0]) << 8 };
        total += u32::from(word);
    }
    total
}

/// The checksum over the IPv4 pseudo-header, the header and the payload.
pub fn checksum(header: &[u8], payload: &[u8]) -> u16 {
    checksum_with(&CLIENT_ADDR, &SERVER_ADDR, header, payload)
}

/// The checksum over a pseudo-header with these addresses.
pub fn checksum_with(src: &[u8; 4], dst: &[u8; 4], header: &[u8], payload: &[u8]) -> u16 {
    let length = (HEADER + payload.len()) as u16;
    let mut total = 0u32;
    total += sum(src);
    total += sum(dst);
    total += u32::from(6u16); // TCP
    total += u32::from(length);
    total += sum(header);
    total += sum(payload);
    while total >> 16 != 0 {
        total = (total & 0xffff) + (total >> 16);
    }
    !(total as u16)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketError {
    /// Fewer bytes than a header.
    Short(usize),
    /// The data offset is outside the segment.
    Header(usize),
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PacketError::Short(n) => write!(f, "segment of {n} bytes is shorter than a header"),
            PacketError::Header(n) => write!(f, "segment data offset {n} is outside the segment"),
        }
    }
}

impl std::error::Error for PacketError {}

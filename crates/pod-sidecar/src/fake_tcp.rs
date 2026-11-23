//! Message framing over a datagram channel: a length prefix, a sequence, a
//! checksum and a reassembly window (rule FAKE-1). An application that needs
//! datagram boundaries below our own framing uses it when a real TCP carrier
//! is not available.
//!
//! The costs, stated (rule FAKE-2):
//!
//! * one extra round trip for the first frame, because the far end answers
//!   only after a whole message has arrived;
//! * the TTL of the carrier, which a path sees and can act on;
//! * a checksum that a middlebox may repair, so a corrupted frame can arrive
//!   with a valid checksum and the framing cannot tell.
//!
//! The framing does not hide the datagrams: a path that refuses UDP by
//! protocol number refuses these frames too. [`crate::limits`] names the
//! condition and the check.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::rc::Rc;

use crate::error::Refusal;

/// The first two bytes of every frame.
pub const MAGIC: &[u8] = &[0xf7, 0x54];
/// The version of the frame.
pub const VERSION: u8 = 1;
/// The length of a frame header.
pub const HEADER: usize = 20;
/// The most bytes of one message.
pub const MAX_MESSAGE: usize = 1 << 20;
/// The default largest frame.
pub const MTU: usize = 1200;
/// The default count of fragments that may wait out of order.
pub const WINDOW: usize = 16;

/// A datagram channel. [`Channel`] is a pair in one process; a consumer can
/// give a UDP socket.
pub trait Datagram {
    fn send(&mut self, bytes: &[u8]) -> Result<(), Refusal>;
    fn recv(&mut self) -> Option<Vec<u8>>;
}

/// One end of an in-process datagram pair.
pub struct Channel {
    tx: Rc<RefCell<VecDeque<Vec<u8>>>>,
    rx: Rc<RefCell<VecDeque<Vec<u8>>>>,
    allow: fn(&[u8]) -> bool,
}

impl Channel {
    /// Put a datagram directly into the queue of this end, for a test of a
    /// reordered or a corrupted frame.
    pub fn inject(&self, bytes: Vec<u8>) {
        self.rx.borrow_mut().push_back(bytes);
    }

    pub fn pending(&self) -> usize {
        self.rx.borrow().len()
    }
}

impl Datagram for Channel {
    fn send(&mut self, bytes: &[u8]) -> Result<(), Refusal> {
        if !(self.allow)(bytes) {
            return Err(Refusal::new("the path refuses a plain udp datagram"));
        }
        self.tx.borrow_mut().push_back(bytes.to_vec());
        Ok(())
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        self.rx.borrow_mut().pop_front()
    }
}

/// A pair of channels in one process.
pub fn pair() -> (Channel, Channel) {
    filtered_pair(|_| true)
}

/// A pair whose path takes a datagram only when `allow` accepts it. A path
/// that refuses a plain datagram gives the simulator's refusal.
pub fn filtered_pair(allow: fn(&[u8]) -> bool) -> (Channel, Channel) {
    let one = Rc::new(RefCell::new(VecDeque::new()));
    let two = Rc::new(RefCell::new(VecDeque::new()));
    (Channel { tx: one.clone(), rx: two.clone(), allow }, Channel { tx: two, rx: one, allow })
}

/// A failure of the framing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FakeTcpError {
    /// The frame carries no magic, or a version that this crate does not read.
    BadFrame(String),
    /// The checksum of the frame does not match.
    BadChecksum,
    /// More fragments wait than the window allows.
    OutOfWindow { window: usize },
    /// The path refused the datagram.
    Path(String),
}

impl fmt::Display for FakeTcpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FakeTcpError::BadFrame(m) => write!(f, "fake tcp frame: {m}"),
            FakeTcpError::BadChecksum => f.write_str("fake tcp frame: checksum does not match"),
            FakeTcpError::OutOfWindow { window } => write!(f, "fake tcp: more than {window} fragments wait"),
            FakeTcpError::Path(m) => write!(f, "fake tcp path: {m}"),
        }
    }
}

impl std::error::Error for FakeTcpError {}

/// One frame on the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub message: u32,
    pub index: u16,
    pub count: u16,
    pub total: u32,
    pub flags: u8,
    pub payload: Vec<u8>,
}

/// A data frame.
pub const DATA: u8 = 0;
/// A frame that carries an empty message and closes the message stream.
pub const FIN: u8 = 1;

impl Frame {
    /// The bytes of the frame, with a valid checksum.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER + self.payload.len()];
        bytes[0..2].copy_from_slice(MAGIC);
        bytes[2] = VERSION;
        bytes[3] = self.flags;
        bytes[4..8].copy_from_slice(&self.message.to_be_bytes());
        bytes[8..10].copy_from_slice(&self.index.to_be_bytes());
        bytes[10..12].copy_from_slice(&self.count.to_be_bytes());
        bytes[12..16].copy_from_slice(&self.total.to_be_bytes());
        bytes[HEADER..].copy_from_slice(&self.payload);
        let sum = checksum(&bytes);
        bytes[16..20].copy_from_slice(&sum.to_be_bytes());
        bytes
    }

    /// Read one frame and check its checksum.
    pub fn decode(bytes: &[u8]) -> Result<Frame, FakeTcpError> {
        if bytes.len() < HEADER {
            return Err(FakeTcpError::BadFrame(format!("{} bytes is less than a header", bytes.len())));
        }
        if &bytes[0..2] != MAGIC {
            return Err(FakeTcpError::BadFrame("no magic".to_string()));
        }
        if bytes[2] != VERSION {
            return Err(FakeTcpError::BadFrame(format!("version {} is not {VERSION}", bytes[2])));
        }
        let total = u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        if total as usize > MAX_MESSAGE {
            return Err(FakeTcpError::BadFrame(format!("message of {total} bytes is too long")));
        }
        let count = u16::from_be_bytes([bytes[10], bytes[11]]);
        let index = u16::from_be_bytes([bytes[8], bytes[9]]);
        if count == 0 || index >= count {
            return Err(FakeTcpError::BadFrame(format!("fragment {index} of {count}")));
        }
        let mut check = bytes.to_vec();
        check[16..20].fill(0);
        let got = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        if checksum(&check) != got {
            return Err(FakeTcpError::BadChecksum);
        }
        Ok(Frame {
            message: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            index,
            count,
            total,
            flags: bytes[3],
            payload: bytes[HEADER..].to_vec(),
        })
    }
}

/// The frames of one message that have arrived.
#[derive(Clone, Debug)]
struct Partial {
    count: u16,
    total: u32,
    parts: BTreeMap<u16, Vec<u8>>,
}

impl Partial {
    fn new(count: u16, total: u32) -> Partial {
        Partial { count, total, parts: BTreeMap::new() }
    }

    fn complete(&self) -> Option<Vec<u8>> {
        if self.parts.len() != usize::from(self.count) {
            return None;
        }
        let mut out = Vec::with_capacity(self.total as usize);
        for index in 0..self.count {
            out.extend_from_slice(self.parts.get(&index)?);
        }
        (out.len() == self.total as usize).then_some(out)
    }
}

/// Message framing over a datagram channel.
pub struct FakeTcp<C: Datagram> {
    channel: C,
    mtu: usize,
    window: usize,
    next_id: u32,
    partials: BTreeMap<u32, Partial>,
    buffered: usize,
}

impl<C: Datagram> FakeTcp<C> {
    pub fn new(channel: C) -> FakeTcp<C> {
        FakeTcp { channel, mtu: MTU, window: WINDOW, next_id: 1, partials: BTreeMap::new(), buffered: 0 }
    }

    pub fn with_mtu(mut self, mtu: usize) -> FakeTcp<C> {
        self.mtu = mtu.max(HEADER + 1);
        self
    }

    pub fn with_window(mut self, window: usize) -> FakeTcp<C> {
        self.window = window.max(1);
        self
    }

    pub fn channel(&self) -> &C {
        &self.channel
    }

    /// Send one message as frames of at most `mtu` bytes.
    pub fn send(&mut self, message: &[u8]) -> Result<(), FakeTcpError> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let chunk = self.mtu - HEADER;
        let count = message.len().div_ceil(chunk).max(1) as u16;
        if count as usize > MAX_MESSAGE / chunk {
            return Err(FakeTcpError::BadFrame("message has too many fragments".to_string()));
        }
        for index in 0..count {
            let start = usize::from(index) * chunk;
            let end = (start + chunk).min(message.len());
            let frame = Frame {
                message: id,
                index,
                count,
                total: message.len() as u32,
                flags: DATA,
                payload: message[start..end].to_vec(),
            };
            self.channel.send(&frame.encode()).map_err(|r| FakeTcpError::Path(r.reason().to_string()))?;
        }
        Ok(())
    }

    /// Take the next whole message. `None` when more frames are needed.
    pub fn recv(&mut self) -> Result<Option<Vec<u8>>, FakeTcpError> {
        while let Some(bytes) = self.channel.recv() {
            let frame = Frame::decode(&bytes)?;
            if frame.flags == FIN {
                continue;
            }
            if let Some(message) = self.accept(frame)? {
                return Ok(Some(message));
            }
        }
        Ok(None)
    }

    fn accept(&mut self, frame: Frame) -> Result<Option<Vec<u8>>, FakeTcpError> {
        let (count, total) = (frame.count, frame.total);
        let partial = self.partials.entry(frame.message).or_insert_with(|| Partial::new(count, total));
        if partial.count != frame.count || partial.total != frame.total {
            self.partials.remove(&frame.message);
            return Err(FakeTcpError::BadFrame("fragments of one message disagree".to_string()));
        }
        if partial.parts.insert(frame.index, frame.payload).is_none() {
            self.buffered += 1;
        }
        let complete = partial.complete();
        if complete.is_none() && self.buffered > self.window {
            let removed = self.partials.remove(&frame.message).map(|p| p.parts.len()).unwrap_or(0);
            self.buffered -= removed;
            return Err(FakeTcpError::OutOfWindow { window: self.window });
        }
        if let Some(message) = complete {
            let removed = self.partials.remove(&frame.message).map(|p| p.parts.len()).unwrap_or(0);
            self.buffered -= removed;
            return Ok(Some(message));
        }
        Ok(None)
    }
}

fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5u32, |acc, b| acc.wrapping_mul(0x0100_0193).wrapping_add(u32::from(*b)))
}

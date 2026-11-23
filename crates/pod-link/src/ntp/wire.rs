//! The NTPv4 packet of the carrier (RFC 5905, RFC 7822): the 48-byte base
//! and the extension fields of a 16-bit type, a 16-bit length and a value.
//! The codec reads and writes the packet alone; it does no I/O. The
//! session of the carrier is [`super::session`].
//!
//! An extension field's length counts its 4-byte header and its value, so a
//! length under 4 is malformed, and so is one that runs past the datagram.
//! [`parse`] refuses both. [`parse_lenient`] takes the same packet, but a
//! field that a server cut short is a truncation for the caller to report as
//! a carrier limit (PodUtils/podnet#484, NTP-4), not a malformed length: it
//! gives the bytes that arrived.

use std::time::{SystemTime, UNIX_EPOCH};

/// The bytes of the base packet: the header and the four timestamps.
pub const HEADER: usize = 48;

/// The bytes of the header of one extension field.
pub const FIELD_HEADER: usize = 4;

/// NTP version 4.
pub const VERSION: u8 = 4;

/// The mode of a client's request.
pub const MODE_CLIENT: u8 = 3;

/// The mode of a server's reply.
pub const MODE_SERVER: u8 = 4;

/// The seconds from the NTP epoch (1900-01-01) to the Unix epoch.
pub const EPOCH: i128 = 2_208_988_800;

/// The first extension field type of this carrier. The registry of NTP
/// extension field types assigns none of the range from 0x1000 to 0xEFFF at
/// the time of this work, and 0xF000 to 0xFFFF is private use (NTP-1).
pub const FIRST_TYPE: u16 = 0x1000;

/// The last extension field type of this carrier.
pub const LAST_TYPE: u16 = 0xefff;

/// Whether `kind` is in the range that this carrier uses: unassigned, and
/// not the private-use block.
pub fn unassigned(kind: u16) -> bool {
    (FIRST_TYPE..=LAST_TYPE).contains(&kind)
}

/// A 64-bit NTP timestamp: seconds since 1900 in the first 32 bits, and the
/// fraction of a second in the last 32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Timestamp(pub u64);

impl Timestamp {
    pub const ZERO: Timestamp = Timestamp(0);

    /// The time of `now`.
    pub fn now() -> Timestamp {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => {
                Timestamp::from_nanos((EPOCH + i128::from(d.as_secs())) * 1_000_000_000 + i128::from(d.subsec_nanos()))
            }
            Err(_) => Timestamp::ZERO,
        }
    }

    /// A timestamp of `nanos` since 1900. A value before 1900 is zero.
    pub fn from_nanos(nanos: i128) -> Timestamp {
        if nanos <= 0 {
            return Timestamp::ZERO;
        }
        let value = ((nanos as u128) << 32) / 1_000_000_000;
        Timestamp(value.min(u128::from(u64::MAX)) as u64)
    }

    /// The nanoseconds since 1900 that this timestamp names.
    pub fn nanos(self) -> i128 {
        (i128::from(self.0) * 1_000_000_000) >> 32
    }

    /// This timestamp moved by `by` nanoseconds.
    pub fn shifted(self, by: i128) -> Timestamp {
        Timestamp::from_nanos(self.nanos().saturating_add(by))
    }
}

/// One extension field: its type and its value, without padding.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct Field {
    pub kind: u16,
    pub value: Vec<u8>,
}

/// An NTPv4 packet: the header, the four timestamps, and the extension
/// fields in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Packet {
    pub leap: u8,
    pub version: u8,
    pub mode: u8,
    pub stratum: u8,
    pub poll: i8,
    pub precision: i8,
    pub root_delay: u32,
    pub root_dispersion: u32,
    pub reference_id: [u8; 4],
    pub reference: Timestamp,
    pub origin: Timestamp,
    pub receive: Timestamp,
    pub transmit: Timestamp,
    pub fields: Vec<Field>,
}

/// Why bytes are no NTP packet of this codec: the offset of the byte where
/// the reading stopped, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    pub offset: usize,
    pub why: String,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at byte {})", self.why, self.offset)
    }
}

impl std::error::Error for DecodeError {}

/// Why a packet cannot be written: an extension field with more bytes than
/// its 16-bit length takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeError {
    pub why: String,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.why)
    }
}

impl std::error::Error for EncodeError {}

/// The packet in `bytes`, a whole datagram. A field with a length under 4,
/// or one that runs past the datagram, is refused.
pub fn parse(bytes: &[u8]) -> Result<Packet, DecodeError> {
    parse_in(bytes, false).map(|(packet, _)| packet)
}

/// [`parse`], but a field that a server cut short is taken with the bytes
/// that arrived, and its length is `Some`: a truncation for the caller, not
/// a malformed length (NTP-4).
pub fn parse_lenient(bytes: &[u8]) -> Result<(Packet, Option<usize>), DecodeError> {
    parse_in(bytes, true)
}

fn parse_in(bytes: &[u8], lenient: bool) -> Result<(Packet, Option<usize>), DecodeError> {
    let fail = |offset: usize, why: String| DecodeError { offset, why };
    if bytes.len() < HEADER {
        return Err(fail(bytes.len(), format!("{} bytes, fewer than the header's 48", bytes.len())));
    }
    let first = bytes[0];
    let mut packet = Packet {
        leap: first >> 6,
        version: (first >> 3) & 0x07,
        mode: first & 0x07,
        stratum: bytes[1],
        poll: bytes[2] as i8,
        precision: bytes[3] as i8,
        root_delay: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        root_dispersion: u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        reference_id: [bytes[12], bytes[13], bytes[14], bytes[15]],
        reference: Timestamp(u64::from_be_bytes(bytes[16..24].try_into().expect("eight bytes"))),
        origin: Timestamp(u64::from_be_bytes(bytes[24..32].try_into().expect("eight bytes"))),
        receive: Timestamp(u64::from_be_bytes(bytes[32..40].try_into().expect("eight bytes"))),
        transmit: Timestamp(u64::from_be_bytes(bytes[40..48].try_into().expect("eight bytes"))),
        fields: Vec::new(),
    };
    let mut rest = &bytes[HEADER..];
    while !rest.is_empty() {
        let at = bytes.len() - rest.len();
        if rest.len() < FIELD_HEADER {
            if lenient {
                return Ok((packet, Some(0)));
            }
            return Err(fail(at, format!("an extension field header of {} bytes", rest.len())));
        }
        let kind = u16::from_be_bytes([rest[0], rest[1]]);
        let length = usize::from(u16::from_be_bytes([rest[2], rest[3]]));
        if length < FIELD_HEADER {
            return Err(fail(at, format!("an extension field of type 0x{kind:04x} with the length {length}")));
        }
        let value = length - FIELD_HEADER;
        if rest.len() - FIELD_HEADER < value {
            if lenient {
                packet.fields.push(Field { kind, value: rest[FIELD_HEADER..].to_vec() });
                return Ok((packet, Some(rest.len() - FIELD_HEADER)));
            }
            return Err(fail(at, format!("the extension field 0x{kind:04x} says {value} bytes, past the datagram")));
        }
        packet.fields.push(Field { kind, value: rest[FIELD_HEADER..FIELD_HEADER + value].to_vec() });
        rest = &rest[length..];
    }
    Ok((packet, None))
}

impl Packet {
    /// The packet as a datagram.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::with_capacity(HEADER);
        out.push((self.leap & 0x03) << 6 | (self.version & 0x07) << 3 | (self.mode & 0x07));
        out.push(self.stratum);
        out.push(self.poll as u8);
        out.push(self.precision as u8);
        out.extend_from_slice(&self.root_delay.to_be_bytes());
        out.extend_from_slice(&self.root_dispersion.to_be_bytes());
        out.extend_from_slice(&self.reference_id);
        out.extend_from_slice(&self.reference.0.to_be_bytes());
        out.extend_from_slice(&self.origin.0.to_be_bytes());
        out.extend_from_slice(&self.receive.0.to_be_bytes());
        out.extend_from_slice(&self.transmit.0.to_be_bytes());
        for field in &self.fields {
            if field.value.len() > usize::from(u16::MAX) - FIELD_HEADER {
                return Err(EncodeError { why: format!("an extension field of {} bytes", field.value.len()) });
            }
            out.extend_from_slice(&field.kind.to_be_bytes());
            out.extend_from_slice(&(field.value.len() as u16 + FIELD_HEADER as u16).to_be_bytes());
            out.extend_from_slice(&field.value);
        }
        Ok(out)
    }

    /// The values of the extension fields of this carrier's range, in
    /// order.
    pub fn payload(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for field in &self.fields {
            if unassigned(field.kind) {
                out.extend_from_slice(&field.value);
            }
        }
        out
    }
}

/// A request of a client: `transmit`, and one field of `value`.
pub fn request(
    transmit: Timestamp,
    poll: i8,
    origin: Timestamp,
    reference: Timestamp,
    kind: u16,
    value: Vec<u8>,
) -> Packet {
    Packet {
        leap: 0,
        version: VERSION,
        mode: MODE_CLIENT,
        stratum: 0,
        poll,
        // The clock of the client is at 2^-20 s.
        precision: -20,
        root_delay: 0,
        root_dispersion: 0,
        reference_id: [0; 4],
        reference,
        origin,
        receive: Timestamp::ZERO,
        transmit,
        fields: vec![Field { kind, value }],
    }
}

/// A reply of a server: the timestamps of the exchange, and one field of
/// `value`.
pub fn reply(
    request: &Packet,
    receive: Timestamp,
    transmit: Timestamp,
    reference: Timestamp,
    stratum: u8,
    kind: u16,
    value: Vec<u8>,
) -> Packet {
    Packet {
        leap: 0,
        version: VERSION,
        mode: MODE_SERVER,
        stratum,
        poll: request.poll,
        precision: -20,
        // A small root delay and dispersion, as a server near its source.
        root_delay: 1,
        root_dispersion: 1,
        reference_id: [127, 0, 0, 1],
        reference,
        origin: request.transmit,
        receive,
        transmit,
        fields: vec![Field { kind, value }],
    }
}

/// The allocator of extension field types: one unassigned type for each
/// packet, never the same twice in a row, so that a constant type is not a
/// signature (NTP-1).
#[derive(Debug, Clone)]
pub struct Types {
    next: u16,
    step: u16,
}

impl Types {
    /// An allocator that starts in the range of this carrier at a place of
    /// `seed`, and steps by a value coprime with the range (odd, and not a
    /// multiple of 7).
    pub fn new(seed: u16) -> Types {
        let span = u32::from(LAST_TYPE - FIRST_TYPE) + 1;
        let start = u32::from(seed) % span;
        let mut step = u32::from(seed) % (span - 1) + 1;
        if step % 2 == 0 {
            step += 1;
        }
        while step % 7 == 0 {
            step += 2;
        }
        if step >= span {
            step = 3;
        }
        Types { next: FIRST_TYPE + start as u16, step: step as u16 }
    }

    /// The type of the next packet.
    pub fn take(&mut self) -> u16 {
        let value = self.next;
        let span = u32::from(LAST_TYPE - FIRST_TYPE) + 1;
        let offset = (u32::from(value - FIRST_TYPE) + u32::from(self.step)) % span;
        self.next = FIRST_TYPE + offset as u16;
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packet_round_trips_with_its_fields_and_a_malformed_length_is_refused() {
        let when = Timestamp::from_nanos(EPOCH * 1_000_000_000);
        let packet = request(when, -5, Timestamp::ZERO, Timestamp::ZERO, 0x1234, b"hello".to_vec());
        let bytes = packet.encode().expect("an encoded packet");
        assert_eq!(parse(&bytes), Ok(packet));
        assert_eq!(parse_lenient(&bytes).expect("a whole packet").1, None);
        // A field of length 3 is malformed: its header alone is 4.
        let mut short = bytes.clone();
        let at = HEADER + 2;
        short[at..at + 2].copy_from_slice(&3u16.to_be_bytes());
        let refused = parse(&short).expect_err("a length under 4");
        assert!(refused.why.contains("length 3"), "{refused}");
        // A field that runs past the datagram is malformed, and a cut one
        // is a truncation for the lenient reader.
        let mut long = bytes.clone();
        long[at..at + 2].copy_from_slice(&99u16.to_be_bytes());
        assert!(parse(&long).is_err(), "a field past the datagram");
        let cut = parse_lenient(&long).expect("a cut field");
        assert_eq!(cut.1, Some(5), "the bytes of the value that arrived");
        assert_eq!(cut.0.fields[0].value, b"hello");
        assert!(parse(&bytes[..40]).is_err(), "a packet under 48 bytes");
    }

    #[test]
    fn the_types_are_unassigned_and_each_packet_gets_another() {
        let mut types = Types::new(0x4242);
        let mut seen = Vec::new();
        let mut last = None;
        for _ in 0..100 {
            let kind = types.take();
            assert!(unassigned(kind), "0x{kind:04x} is unassigned");
            assert_ne!(Some(kind), last, "a constant type is a signature");
            last = Some(kind);
            seen.push(kind);
        }
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 100, "each packet got its own type");
    }

    #[test]
    fn a_reply_names_the_request_and_a_plausible_clock() {
        let t1 = Timestamp::from_nanos(EPOCH * 1_000_000_000 + 1_000_000_000);
        let asked = request(t1, -5, Timestamp::ZERO, Timestamp::ZERO, 0x2000, vec![1, 2, 3]);
        let t2 = t1.shifted(1_000_000);
        let t3 = t2.shifted(1_000);
        let answered = reply(&asked, t2, t3, t1, 2, 0x3000, vec![4, 5]);
        let bytes = answered.encode().expect("a reply");
        let read = parse(&bytes).expect("a read reply");
        assert_eq!((read.version, read.mode, read.stratum), (VERSION, MODE_SERVER, 2));
        assert_eq!(read.origin, asked.transmit, "the origin is the transmit of the client");
        assert_eq!((read.receive, read.transmit), (t2, t3));
        assert_eq!(read.payload(), vec![4, 5]);
        assert_eq!(asked.payload(), vec![1, 2, 3]);
    }

    #[test]
    fn a_timestamp_of_nanos_keeps_the_time_of_day() {
        let when = Timestamp::from_nanos(EPOCH * 1_000_000_000 + 123_456_789);
        let back = when.nanos() - EPOCH * 1_000_000_000;
        assert!((back - 123_456_789).abs() < 1_000, "the fraction of a second: {back}");
    }
}

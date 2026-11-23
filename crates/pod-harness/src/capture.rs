//! A capture: the bytes and the time of each packet of one harness run.
//!
//! # Format
//!
//! A capture is UTF-8 text. The first line that is not a comment is the
//! header `pod-capture/1`. Each later line is one packet:
//!
//! ```text
//! c2s 0 16030100f4010000f00303
//! s2c 1250 1603030040
//! ```
//!
//! The fields are the direction (`c2s` or `s2c`), the timestamp in
//! microseconds from the start of the run, and the bytes as lower or upper
//! case hexadecimal with no spaces. A line that starts with `#` is a
//! comment. An empty line is ignored.
//!
//! The middlebox simulator writes this same format, so a capture of its
//! output loads as any other capture.

use crate::error::{Error, Result};
use std::path::Path;

/// The header of the format.
pub const MAGIC: &str = "pod-capture/1";

/// The largest packet that a capture holds.
pub const MAX_PACKET_BYTES: usize = 1 << 20;

/// The end of a packet that sends the bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    ClientToServer,
    ServerToClient,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::ClientToServer => "c2s",
            Direction::ServerToClient => "s2c",
        }
    }

    pub fn parse(text: &str) -> Option<Direction> {
        match text {
            "c2s" => Some(Direction::ClientToServer),
            "s2c" => Some(Direction::ServerToClient),
            _ => None,
        }
    }
}

/// One packet: its direction, its time and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub direction: Direction,
    pub timestamp_micros: u64,
    pub bytes: Vec<u8>,
}

impl Packet {
    pub fn new(direction: Direction, timestamp_micros: u64, bytes: impl Into<Vec<u8>>) -> Packet {
        Packet { direction, timestamp_micros, bytes: bytes.into() }
    }
}

/// The packets of one run, in the order that they went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capture {
    pub packets: Vec<Packet>,
}

impl Capture {
    pub fn new() -> Capture {
        Capture { packets: Vec::new() }
    }

    pub fn push(&mut self, packet: Packet) {
        self.packets.push(packet);
    }

    /// The first packet from the client to the server.
    pub fn first_client_packet(&self) -> Option<&Packet> {
        self.packets.iter().find(|p| p.direction == Direction::ClientToServer)
    }

    /// The payload length of each TLS record in the client's packets. A
    /// packet that is not a TLS record adds no size.
    pub fn tls_record_sizes(&self) -> Vec<usize> {
        let mut sizes = Vec::new();
        for packet in &self.packets {
            sizes.extend(tls_record_sizes(&packet.bytes));
        }
        sizes
    }

    pub fn parse(text: &str) -> Result<Capture> {
        let mut capture = Capture::new();
        let mut header = false;
        for (number, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !header {
                if line != MAGIC {
                    return Err(Error::new(format!(
                        "line {}: the first line of a capture is {MAGIC}, not {line:?}",
                        number + 1
                    )));
                }
                header = true;
                continue;
            }
            let mut fields = line.split_whitespace();
            let direction = fields
                .next()
                .and_then(Direction::parse)
                .ok_or_else(|| Error::new(format!("line {}: the direction is c2s or s2c", number + 1)))?;
            let timestamp = fields
                .next()
                .ok_or_else(|| Error::new(format!("line {}: no timestamp", number + 1)))?
                .parse::<u64>()
                .map_err(|_| Error::new(format!("line {}: the timestamp is microseconds", number + 1)))?;
            let hex = fields.next().ok_or_else(|| Error::new(format!("line {}: no bytes", number + 1)))?;
            if fields.next().is_some() {
                return Err(Error::new(format!("line {}: more than three fields", number + 1)));
            }
            let bytes = decode_hex(hex)?;
            if bytes.len() > MAX_PACKET_BYTES {
                return Err(Error::new(format!(
                    "line {}: {} bytes, over the limit of {MAX_PACKET_BYTES}",
                    number + 1,
                    bytes.len()
                )));
            }
            capture.push(Packet::new(direction, timestamp, bytes));
        }
        if !header {
            return Err(Error::new(format!("the capture has no {MAGIC} header")));
        }
        Ok(capture)
    }

    pub fn load(path: &Path) -> Result<Capture> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::io("read the capture", path, e))?;
        Capture::parse(&text)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from(MAGIC);
        out.push('\n');
        for packet in &self.packets {
            out.push_str(packet.direction.as_str());
            out.push(' ');
            out.push_str(&packet.timestamp_micros.to_string());
            out.push(' ');
            out.push_str(&encode_hex(&packet.bytes));
            out.push('\n');
        }
        out
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        std::fs::write(path, self.to_text()).map_err(|e| Error::io("write the capture", path, e))
    }
}

/// The TLS record payload lengths of one buffer. A length that runs past
/// the end of the buffer ends the scan.
pub fn tls_record_sizes(mut bytes: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    while bytes.len() >= 5 {
        let kind = bytes[0];
        if !(0x14..=0x17).contains(&kind) {
            break;
        }
        let len = u16::from_be_bytes([bytes[3], bytes[4]]) as usize;
        if bytes.len() < 5 + len {
            break;
        }
        out.push(len);
        bytes = &bytes[5 + len..];
    }
    out
}

pub fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    out
}

pub fn decode_hex(text: &str) -> Result<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return Err(Error::new("a hexadecimal string has an even number of digits"));
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    for pair in bytes.as_chunks::<2>().0 {
        let high = hex_digit(pair[0])?;
        let low = hex_digit(pair[1])?;
        out.push(high << 4 | low);
    }
    Ok(out)
}

fn hex_digit(b: u8) -> Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(Error::new(format!("{:?} is not a hexadecimal digit", b as char))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_round_trips() {
        let mut capture = Capture::new();
        capture.push(Packet::new(Direction::ClientToServer, 0, [0x16, 0x03, 0x01, 0x00, 0x01, 0x00]));
        capture.push(Packet::new(Direction::ServerToClient, 1250, [0x16, 0x03, 0x03, 0x00, 0x00]));
        let text = capture.to_text();
        assert!(text.starts_with("pod-capture/1\n"));
        assert_eq!(Capture::parse(&text).unwrap(), capture);
        assert_eq!(capture.tls_record_sizes(), vec![1, 0]);
    }

    #[test]
    fn comments_and_empty_lines_are_ignored() {
        let text = "# a run\n\npod-capture/1\n# client\nc2s 7 00ff\n";
        let capture = Capture::parse(text).unwrap();
        assert_eq!(capture.packets.len(), 1);
        assert_eq!(capture.packets[0].bytes, vec![0x00, 0xff]);
        assert_eq!(capture.packets[0].timestamp_micros, 7);
    }

    #[test]
    fn a_bad_line_names_itself() {
        assert!(Capture::parse("pod-capture/1\nup 0 00\n").is_err());
        assert!(Capture::parse("pod-capture/1\nc2s x 00\n").is_err());
        assert!(Capture::parse("pod-capture/1\nc2s 0 0\n").is_err());
        assert!(Capture::parse("pod-capture/1\nc2s 0 zz\n").is_err());
        assert!(Capture::parse("c2s 0 00\n").is_err());
        assert!(Capture::parse("").is_err());
    }

    #[test]
    fn the_hex_reader_accepts_both_cases() {
        assert_eq!(decode_hex("00aF").unwrap(), vec![0x00, 0xaf]);
        assert!(decode_hex("0").is_err());
        assert!(decode_hex("gg").is_err());
    }
}

//! AX.25 frames in KISS framing, as a TNC carries them on a serial line.
//!
//! A frame starts and ends with `FEND` (0xC0). Inside a frame, a byte
//! `FEND` is sent as `FESC TFEND`, and a byte `FESC` (0xDB) as
//! `FESC TFESC`. The first byte of the frame is the KISS port.
//!
//! The maximum frame is 256 bytes of payload, which is the AX.25 frame
//! with its control byte. A larger frame is an error of the name
//! `frame-too-large`; the crate never splits it in silence.

use crate::Error;

/// The end of a frame.
pub const FEND: u8 = 0xC0;
/// The escape of a byte inside a frame.
pub const FESC: u8 = 0xDB;
/// The escaped `FEND`.
pub const TFEND: u8 = 0xDC;
/// The escaped `FESC`.
pub const TFESC: u8 = 0xDD;

/// The largest payload of one frame, and so the largest AX.25 frame.
pub const MAX_FRAME: usize = 256;

/// The KISS port of an AX.25 frame of the link.
pub const PORT_AX25: u8 = 0x00;
/// The port of an announcement of local discovery.
pub const PORT_DISCOVERY: u8 = 0x01;
/// The port of the frames of the modem driver.
pub const PORT_MODEM: u8 = 0x02;

/// One frame: its port and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub port: u8,
    pub payload: Vec<u8>,
}

/// The bytes of `payload` on `port`, escaped and enclosed by `FEND`.
pub fn encode(port: u8, payload: &[u8]) -> Result<Vec<u8>, Error> {
    if payload.len() > MAX_FRAME {
        return Err(Error::FrameTooLarge { size: payload.len(), max: MAX_FRAME });
    }
    let mut out = Vec::with_capacity(payload.len() + 4);
    out.push(FEND);
    out.push(port);
    for &byte in payload {
        match byte {
            FEND => out.extend_from_slice(&[FESC, TFEND]),
            FESC => out.extend_from_slice(&[FESC, TFESC]),
            other => out.push(other),
        }
    }
    out.push(FEND);
    Ok(out)
}

/// A reader of frames: the caller gives it each byte that came, and it
/// gives each frame that is complete.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
    escape: bool,
    active: bool,
    count: usize,
    oversize: bool,
}

impl Decoder {
    pub fn new() -> Decoder {
        Decoder::default()
    }

    /// Take `bytes` and give each frame that they complete. A bad byte and
    /// an oversize frame are errors of their own; the decoder goes on.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Result<Frame, Error>> {
        let mut out = Vec::new();
        for &byte in bytes {
            match byte {
                FEND => {
                    if self.escape {
                        out.push(Err(Error::KissEscape { byte: FEND }));
                    } else if self.active && self.oversize {
                        out.push(Err(Error::FrameTooLarge { size: self.count.saturating_sub(1), max: MAX_FRAME }));
                    } else if self.active && self.count > 0 {
                        let port = self.buf[0];
                        out.push(Ok(Frame { port, payload: self.buf[1..].to_vec() }));
                    }
                    self.reset();
                }
                FESC => {
                    self.escape = true;
                    self.active = true;
                }
                other => {
                    let byte = if self.escape {
                        self.escape = false;
                        match other {
                            TFEND => FEND,
                            TFESC => FESC,
                            bad => {
                                out.push(Err(Error::KissEscape { byte: bad }));
                                continue;
                            }
                        }
                    } else {
                        other
                    };
                    self.active = true;
                    self.count += 1;
                    if self.count > MAX_FRAME + 1 {
                        self.oversize = true;
                        self.buf.clear();
                    } else if !self.oversize {
                        self.buf.push(byte);
                    }
                }
            }
        }
        out
    }

    /// The frame in progress, when the line gave no end yet.
    pub fn pending(&self) -> usize {
        if self.active { self.count } else { 0 }
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.escape = false;
        self.active = false;
        self.count = 0;
        self.oversize = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_escape_bytes_survive_a_round_trip() {
        let payload = [0x01, FEND, 0x02, FESC, TFEND, TFESC, 0x03];
        let wire = encode(PORT_AX25, &payload).unwrap();
        assert!(wire.windows(2).any(|w| w == [FESC, TFEND]));
        assert!(wire.windows(2).any(|w| w == [FESC, TFESC]));
        let mut decoder = Decoder::new();
        let frames = decoder.push(&wire);
        assert_eq!(frames.len(), 1);
        let frame = frames[0].as_ref().unwrap();
        assert_eq!(frame.port, PORT_AX25);
        assert_eq!(frame.payload, payload);
    }

    #[test]
    fn the_maximum_frame_is_taken_and_the_next_byte_is_refused_by_name() {
        let full = vec![0x5a; MAX_FRAME];
        let wire = encode(PORT_AX25, &full).unwrap();
        let frame = Decoder::new().push(&wire).remove(0).unwrap();
        assert_eq!(frame.payload.len(), MAX_FRAME);

        let above = vec![0x5a; MAX_FRAME + 1];
        let refused = encode(PORT_AX25, &above).unwrap_err();
        assert_eq!(refused.name(), "frame-too-large");
        assert!(refused.to_string().contains("257 bytes"));
    }

    #[test]
    fn a_decoder_refuses_an_oversize_frame_rather_than_cutting_it() {
        let mut wire = vec![FEND, PORT_AX25];
        wire.extend(std::iter::repeat_n(0x41, MAX_FRAME + 1));
        wire.push(FEND);
        let failed = Decoder::new().push(&wire).remove(0).unwrap_err();
        assert_eq!(failed.name(), "frame-too-large");
        assert!(matches!(failed, Error::FrameTooLarge { size, max } if size == MAX_FRAME + 1 && max == MAX_FRAME));
    }

    #[test]
    fn two_frames_in_one_read_come_apart() {
        let mut wire = encode(PORT_AX25, b"one").unwrap();
        wire.extend(encode(PORT_MODEM, b"two").unwrap());
        let frames = Decoder::new().push(&wire);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].as_ref().unwrap().payload, b"one");
        assert_eq!(frames[1].as_ref().unwrap().payload, b"two");
        assert_eq!(frames[1].as_ref().unwrap().port, PORT_MODEM);
    }

    #[test]
    fn an_unknown_escape_is_a_named_error() {
        let wire = [FEND, PORT_AX25, FESC, 0x00, FEND];
        let failed = Decoder::new().push(&wire).remove(0).unwrap_err();
        assert_eq!(failed.name(), "kiss-escape");
    }

    #[test]
    fn a_frame_that_did_not_end_is_counted() {
        let mut decoder = Decoder::new();
        assert_eq!(decoder.pending(), 0);
        decoder.push(&[FEND, PORT_AX25, 0x01]);
        assert_eq!(decoder.pending(), 2);
    }
}

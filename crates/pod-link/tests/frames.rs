//! The frame codec: each frame round trips, in any split of its bytes, and
//! each broken header fails by name.

use pod_link::frame::{HEADER, PREFACE, kind};
use pod_link::{Decoder, Frame, FrameError, MAX_RECORD};

fn all() -> Vec<Frame> {
    vec![
        Frame::Open(1),
        Frame::Data(3, b"hello".to_vec()),
        Frame::Data(u32::MAX, vec![7; MAX_RECORD]),
        Frame::Window(0, 1),
        Frame::Window(2, u32::MAX),
        Frame::Close(4),
        Frame::Reset(5, 1),
        Frame::Ping(u64::MAX),
        Frame::Pong(0),
        Frame::GoAway(1),
    ]
}

fn encoded(frames: &[Frame]) -> Vec<u8> {
    let mut out = PREFACE.to_vec();
    for f in frames {
        f.encode(&mut out).unwrap();
    }
    out
}

#[test]
fn each_frame_round_trips_in_any_split() {
    let frames = all();
    let bytes = encoded(&frames);
    for step in [1, 2, 7, 8, 9, 4096, bytes.len()] {
        let mut decoder = Decoder::with_preface();
        let mut got = Vec::new();
        for piece in bytes.chunks(step) {
            decoder.push(piece);
            while let Some(f) = decoder.next().unwrap() {
                got.push(f);
            }
        }
        assert_eq!(got, frames, "pieces of {step} bytes");
        assert_eq!(decoder.buffered(), 0);
    }
}

#[test]
fn the_header_is_eight_bytes_with_a_24_bit_length() {
    let mut out = Vec::new();
    Frame::Data(0x0102_0304, vec![0xaa; 3]).encode(&mut out).unwrap();
    assert_eq!(out, [kind::DATA, 1, 2, 3, 4, 0, 0, 3, 0xaa, 0xaa, 0xaa]);
    assert_eq!(Frame::Data(1, vec![0; 3]).wire_len(), HEADER + 3);
}

fn first_error(bytes: &[u8]) -> FrameError {
    let mut decoder = Decoder::new();
    decoder.push(bytes);
    loop {
        match decoder.next() {
            Ok(Some(_)) => continue,
            Ok(None) => panic!("no error in {bytes:02x?}"),
            Err(e) => {
                // The same error again, and nothing more is read.
                decoder.push(&[kind::OPEN, 0, 0, 0, 1, 0, 0, 0]);
                assert_eq!(decoder.next().unwrap_err(), e);
                return e;
            }
        }
    }
}

#[test]
fn a_body_over_one_mebibyte_fails_at_its_header() {
    let len = (MAX_RECORD + 1) as u32;
    let header = [kind::DATA, 0, 0, 0, 1, (len >> 16) as u8, (len >> 8) as u8, len as u8];
    assert_eq!(first_error(&header), FrameError::TooLong(MAX_RECORD + 1));
    assert_eq!(
        Frame::Data(1, vec![0; MAX_RECORD + 1]).encode(&mut Vec::new()),
        Err(FrameError::TooLong(MAX_RECORD + 1))
    );
}

#[test]
fn broken_headers_fail_by_name() {
    assert_eq!(first_error(&[0x09]), FrameError::UnknownKind(0x09));
    assert_eq!(first_error(&[0x00, 0, 0, 0, 0, 0, 0, 0]), FrameError::UnknownKind(0));
    assert_eq!(first_error(&[kind::OPEN, 0, 0, 0, 0, 0, 0, 0]), FrameError::LinkId(kind::OPEN));
    assert_eq!(first_error(&[kind::DATA, 0, 0, 0, 1, 0, 0, 0]), FrameError::EmptyData);
    assert_eq!(first_error(&[kind::OPEN, 0, 0, 0, 1, 0, 0, 1, 0]), FrameError::BadLength { kind: kind::OPEN, len: 1 });
    assert_eq!(
        first_error(&[kind::PING, 0, 0, 0, 3, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]),
        FrameError::StreamId { kind: kind::PING, id: 3 }
    );
    assert_eq!(first_error(&[kind::WINDOW, 0, 0, 0, 1, 0, 0, 4, 0, 0, 0, 0]), FrameError::ZeroCredit);
    assert_eq!(
        first_error(&[kind::RESET, 0, 0, 0, 1, 0, 0, 2, 0, 0]),
        FrameError::BadLength { kind: kind::RESET, len: 2 }
    );
}

#[test]
fn a_wrong_preface_fails_at_its_first_byte() {
    let mut decoder = Decoder::with_preface();
    decoder.push(b"PODLINK");
    assert_eq!(decoder.next(), Ok(None));
    decoder.push(b"\x02");
    assert_eq!(decoder.next(), Err(FrameError::Preface));
    let mut http = Decoder::with_preface();
    http.push(b"G");
    assert_eq!(http.next(), Err(FrameError::Preface));
}

#[test]
fn the_encoder_refuses_what_the_decoder_refuses() {
    for frame in [Frame::Open(0), Frame::Data(1, Vec::new()), Frame::Window(1, 0), Frame::Reset(0, 0)] {
        let mut out = vec![1, 2];
        assert!(frame.encode(&mut out).is_err(), "{frame:?}");
        assert_eq!(out, [1, 2], "the output stays as it was");
    }
}

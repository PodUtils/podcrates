//! The frames of `pod-link` (T-300).

/// The bytes of one direction of a link (`pod_link::Decoder`; new in
/// `pod-link`, T-300), pushed in pieces whose sizes the first byte sets;
/// its low bit asks for the preface first. Each frame that comes out keeps
/// the rules of the protocol (a stream frame has an id, a body has at most
/// 1 MiB) and encodes to bytes that decode to the same frame; after an
/// error, each call gives the same error, and nothing after it is read.
pub fn link_frame(data: &[u8]) {
    use pod_link::{Decoder, Frame, MAX_RECORD};
    let Some((&first, bytes)) = data.split_first() else { return };
    let step = usize::from(first >> 1) % 64 + 1;
    let mut decoder = if first & 1 == 1 { Decoder::with_preface() } else { Decoder::new() };
    for piece in bytes.chunks(step) {
        decoder.push(piece);
        loop {
            match decoder.next() {
                Ok(Some(frame)) => {
                    match &frame {
                        Frame::Open(id) | Frame::Close(id) | Frame::Reset(id, _) => assert_ne!(*id, 0),
                        Frame::Data(id, body) => {
                            assert_ne!(*id, 0);
                            assert!(!body.is_empty() && body.len() <= MAX_RECORD, "{} bytes", body.len());
                        }
                        Frame::Window(_, credit) => assert_ne!(*credit, 0),
                        Frame::Ping(_) | Frame::Pong(_) | Frame::GoAway(_) => {}
                    }
                    let mut again = Vec::new();
                    frame.encode(&mut again).expect("a decoded frame encodes");
                    assert_eq!(again.len(), frame.wire_len());
                    let mut other = Decoder::new();
                    other.push(&again);
                    assert_eq!(other.next().expect("its bytes decode"), Some(frame));
                    assert_eq!(other.buffered(), 0);
                }
                Ok(None) => break,
                Err(e) => {
                    decoder.push(b"\x01\x00\x00\x00\x01\x00\x00\x00");
                    assert_eq!(decoder.next().unwrap_err(), e);
                    return;
                }
            }
        }
    }
}

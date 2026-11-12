//! The records, the handed-over state and the UDP frames of `pod-session`
//! (T-020).

/// The bytes of one link of the resumable layer
/// (`pod_session::decode::Decoder`; new in `pod-session`, T-020), pushed in
/// pieces whose sizes the first byte sets. Each record that comes out
/// encodes again, to the same record; after an error, each call gives the
/// same error, and nothing after it is read.
pub fn session_record(data: &[u8]) {
    use pod_session::decode::Decoder;
    let Some((&step, bytes)) = data.split_first() else { return };
    let step = usize::from(step % 64) + 1;
    let mut decoder = Decoder::new();
    for piece in bytes.chunks(step) {
        decoder.push(piece);
        loop {
            match decoder.next() {
                Ok(Some(record)) => {
                    let mut again = Vec::new();
                    record.encode(&mut again).expect("a decoded record encodes");
                    let mut other = Decoder::new();
                    other.push(&again);
                    assert!(other.next().expect("its bytes decode") == Some(record), "{again:02x?}");
                    assert_eq!(other.buffered(), 0);
                }
                Ok(None) => break,
                Err(e) => {
                    decoder.push(b"\x01");
                    assert_eq!(decoder.next().unwrap_err(), e);
                    return;
                }
            }
        }
    }
}

/// The state of a far end's session that the binary before this one handed
/// over (`pod_session::handover::LayerState::decode`; new in `pod-session`,
/// T-020). A state that reads encodes to bytes that read to the same
/// bytes again.
pub fn session_handover(data: &[u8]) {
    use pod_session::handover::LayerState;
    if let Ok(state) = LayerState::decode(data) {
        let bytes = state.encode();
        let again = LayerState::decode(&bytes).expect("an encoded state reads");
        assert_eq!(*again.encode(), *bytes);
    }
}

/// UDP in a stream (`pod_session::udp::Decoder`, `framed`; new in
/// `pod-session`, T-020), pushed in pieces whose sizes the first byte sets:
/// each datagram is at most `MAX_DATAGRAM` bytes and frames back to the
/// bytes that it came from; a length over the limit is refused.
pub fn udp_frame(data: &[u8]) {
    use pod_session::udp::{Decoder, MAX_DATAGRAM, framed};
    let Some((&step, bytes)) = data.split_first() else { return };
    let step = usize::from(step % 64) + 1;
    let (mut decoder, mut used) = (Decoder::new(), 0);
    for piece in bytes.chunks(step) {
        decoder.push(piece);
        loop {
            match decoder.next_datagram() {
                Ok(Some(datagram)) => {
                    assert!(datagram.len() <= MAX_DATAGRAM);
                    let frame = framed(&datagram).expect("a decoded datagram frames");
                    assert_eq!(frame[..], bytes[used..used + frame.len()]);
                    used += frame.len();
                }
                Ok(None) => break,
                Err(_) => {
                    let len = usize::from(u16::from_be_bytes([bytes[used], bytes[used + 1]]));
                    assert!(len > MAX_DATAGRAM);
                    return;
                }
            }
        }
    }
    assert_eq!(used + decoder.pending(), bytes.len());
}

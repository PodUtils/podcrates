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

/// The unit of the carrier of `link_arq`.
pub const ARQ_MTU: usize = 1200;

/// `pod_link::arq::Arq` takes the datagrams of a stranger: the input is a
/// byte of setup (the bytes that this end writes: `b * 64`), then datagrams,
/// each a step of the clock in tens of milliseconds, a length (u16, big
/// endian) and its bytes. After each datagram the ARQ sends what it has and
/// fires its timer when its time passed. No panic; each datagram that it
/// sends decodes and fits the unit; a `DATA` frame of one sequence always
/// carries the same fragment, also when it goes again; an error stays.
pub fn link_arq(data: &[u8]) {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    use pod_link::arq::wire::Datagram;
    use pod_link::arq::{Arq, ArqSettings};

    let Some((&setup, mut rest)) = data.split_first() else { return };
    let Ok(mut arq) = Arq::new(ArqSettings::default(), ARQ_MTU) else { return };
    let _ = arq.write(&vec![0x5a; usize::from(setup) * 64]);
    let mut now = Instant::now();
    let mut sent: HashMap<u32, (u16, u16, Vec<u8>)> = HashMap::new();
    let mut drain = |arq: &mut Arq, now: Instant| {
        while let Some(out) = arq.poll_transmit(now) {
            assert!(out.len() <= ARQ_MTU, "{} bytes over the unit", out.len());
            match Datagram::decode(&out).expect("a datagram of the ARQ decodes") {
                Datagram::Data { seq, index, count, payload } => {
                    let fragment = (index, count, payload);
                    let first = sent.entry(seq).or_insert_with(|| fragment.clone());
                    assert!(*first == fragment, "the sequence {seq} carries two fragments");
                }
                Datagram::Fin { .. } | Datagram::Ack { .. } | Datagram::Probe => {}
            }
        }
    };
    drain(&mut arq, now);
    while rest.len() >= 3 {
        let step = Duration::from_millis(u64::from(rest[0]) * 10);
        let len = usize::from(u16::from_be_bytes([rest[1], rest[2]])).min(rest.len() - 3);
        let (datagram, after) = rest[3..].split_at(len);
        rest = after;
        now += step;
        let failed = arq.error().cloned();
        arq.on_datagram(now, datagram);
        if let Some(due) = arq.poll_timeout()
            && due <= now
        {
            arq.on_timeout(now);
        }
        drain(&mut arq, now);
        if let Some(e) = failed {
            assert_eq!(arq.error(), Some(&e), "an error of the ARQ stays");
        }
    }
}

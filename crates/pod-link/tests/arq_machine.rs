//! The ARQ as a state machine, with a clock of the test: the fragments of
//! a record, the window, the acknowledgement of each lost frame, the
//! retransmission timer and its end, the probe of a closed window, the
//! wrap of the sequence, and datagrams that the ARQ does not take.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use pod_link::arq::wire::{Bitmap, Datagram};
use pod_link::arq::{Arq, ArqError, ArqSettings};
use pod_link::datagram::lossy::SplitMix64;

const MTU: usize = 1200;

fn arq(settings: ArqSettings) -> Arq {
    Arq::new(settings, MTU).unwrap()
}

/// Each datagram that `from` gives now.
fn drain(from: &mut Arq, now: Instant) -> Vec<Datagram> {
    std::iter::from_fn(|| from.poll_transmit(now)).map(|d| Datagram::decode(&d).unwrap()).collect()
}

fn give(to: &mut Arq, now: Instant, datagrams: &[Datagram]) {
    for d in datagrams {
        to.on_datagram(now, &d.to_vec());
    }
}

fn seq(d: &Datagram) -> Option<u32> {
    match d {
        Datagram::Data { seq, .. } | Datagram::Fin { seq } => Some(*seq),
        _ => None,
    }
}

/// Bytes that differ at each place, so that a reorder shows.
fn pattern(n: usize, salt: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(salt).wrapping_add((i >> 8) as u8)).collect()
}

/// A record of 5000 bytes is 5 fragments of at most 1 KiB, with the index
/// 0 to 4 and the count 5; over a unit of 200 bytes it is 27 fragments.
/// The receiver gives the record whole.
#[test]
fn a_record_is_cut_into_fragments_with_their_count() {
    let now = Instant::now();
    for (mtu, size, count) in [(1200, 1024, 5u16), (200, 191, 27)] {
        let mut a = Arq::new(ArqSettings::default(), mtu).unwrap();
        let mut b = Arq::new(ArqSettings::default(), mtu).unwrap();
        assert_eq!(a.fragment(), size);
        let record = pattern(5000, 7);
        assert!(a.send_record(&record).unwrap());
        let out = drain(&mut a, now);
        assert_eq!(out.len(), usize::from(count));
        for (i, d) in out.iter().enumerate() {
            let Datagram::Data { seq, index, count: c, payload } = d else { panic!("{d:?}") };
            assert_eq!((*seq, usize::from(*index), *c), (i as u32, i, count));
            assert!(payload.len() <= size && d.wire_len() <= mtu);
        }
        give(&mut b, now, &out);
        assert_eq!(b.recv_record().unwrap(), Some(record));
        assert_eq!(b.recv_record().unwrap(), None);
        assert_eq!(b.stats().records_received, 1);
    }
}

/// With no `ACK`, 256 frames go and no more; a window of 16 gives 16. An
/// `ACK` of 10 frames lets 10 more go.
#[test]
fn the_window_bounds_the_frames_in_flight() {
    let now = Instant::now();
    for window in [256u16, 16] {
        let settings = ArqSettings { window, send_buffer: 1 << 20, ..ArqSettings::default() };
        let mut a = arq(settings);
        for _ in 0..8 {
            assert!(a.send_record(&pattern(64 << 10, 1)).unwrap());
        }
        let out = drain(&mut a, now);
        assert_eq!(out.len(), usize::from(window));
        assert_eq!(a.in_flight(), usize::from(window));
        let ack = Datagram::Ack { next: 10, window: 256, bitmap: Bitmap::default() };
        a.on_datagram(now, &ack.to_vec());
        let more = drain(&mut a, now);
        assert_eq!(
            more.iter().map(|d| seq(d).unwrap()).collect::<Vec<_>>(),
            (0..10).map(|i| u32::from(window) + i).collect::<Vec<_>>()
        );
    }
}

/// The window of the peer bounds the frames too: an `ACK` with a window
/// of 4 lets 4 frames go after its `next`.
#[test]
fn the_window_of_the_peer_bounds_the_frames() {
    let now = Instant::now();
    let mut a = arq(ArqSettings::default());
    assert!(a.send_record(&pattern(64 << 10, 1)).unwrap());
    a.on_datagram(now, &Datagram::Ack { next: 0, window: 4, bitmap: Bitmap::default() }.to_vec());
    assert_eq!(drain(&mut a, now).len(), 4);
    a.on_datagram(now, &Datagram::Ack { next: 4, window: 4, bitmap: Bitmap::default() }.to_vec());
    assert_eq!(drain(&mut a, now).iter().map(|d| seq(d).unwrap()).collect::<Vec<_>>(), [4, 5, 6, 7]);
}

/// Of 10 frames, 3 and 7 are lost. The `ACK` has the next frame 3 and a
/// bit for 4, 5, 6, 8 and 9. Frame 3 has 3 or more acknowledged frames
/// above it: it goes again at once, alone. Frame 7 has 2: it goes again at
/// its RTO, alone. Then the receiver has each byte in order.
#[test]
fn each_lost_frame_goes_again_alone() {
    let t0 = Instant::now();
    let mut a = arq(ArqSettings::default());
    let mut b = arq(ArqSettings::default());
    let record = pattern(10 << 10, 3);
    assert!(a.send_record(&record).unwrap());
    let out = drain(&mut a, t0);
    assert_eq!(out.len(), 10);
    let kept: Vec<_> = out.iter().filter(|d| !matches!(seq(d), Some(3 | 7))).cloned().collect();
    give(&mut b, t0, &kept);
    let ack = drain(&mut b, t0);
    let [Datagram::Ack { next: 3, bitmap, .. }] = ack.as_slice() else { panic!("{ack:?}") };
    assert_eq!((0..256).filter(|i| bitmap.get(*i)).collect::<Vec<_>>(), [1, 2, 3, 5, 6]);
    let t1 = t0 + Duration::from_millis(5);
    give(&mut a, t1, &ack);
    let again = drain(&mut a, t1);
    assert_eq!(again, [out[3].clone()]);
    give(&mut b, t1, &again);
    let ack = drain(&mut b, t1);
    give(&mut a, t1, &ack);
    assert_eq!(a.in_flight(), 1);
    // The round trip of 5 ms gives the RTO floor.
    let at = a.poll_timeout().unwrap();
    assert_eq!(at, t0 + Duration::from_millis(200));
    a.on_timeout(at);
    let again = drain(&mut a, at);
    assert_eq!(again, [out[7].clone()]);
    give(&mut b, at, &again);
    give(&mut a, at, &drain(&mut b, at));
    assert_eq!(a.in_flight(), 0);
    assert_eq!(a.stats().retransmitted, 2);
    assert_eq!(b.recv_record().unwrap(), Some(record));
}

/// A frame with no `ACK` goes again after the RTO, doubled each time up to
/// the ceiling of 8 s: 1, 2, 4 and 8 s, then 8 s. After the 8th
/// retransmission and its RTO, the ARQ ends with `arq-timeout`.
#[test]
fn the_timer_doubles_and_ends_after_eight_retries() {
    let t0 = Instant::now();
    let mut a = arq(ArqSettings::default());
    assert!(a.send_record(b"one").unwrap());
    let first = drain(&mut a, t0);
    let mut sends = vec![Duration::ZERO];
    let mut now = t0;
    // A bound on the turns, so that a timer with no end fails the test.
    for _ in 0..20 {
        let Some(at) = a.poll_timeout() else { break };
        assert!(at > now);
        now = at;
        a.on_timeout(now);
        let out = drain(&mut a, now);
        if out.is_empty() {
            break;
        }
        assert_eq!(out, first);
        sends.push(now - t0);
    }
    let secs: Vec<u64> = sends.iter().map(Duration::as_secs).collect();
    assert_eq!(secs, [0, 1, 3, 7, 15, 23, 31, 39, 47]);
    let e = a.error().cloned().unwrap();
    assert_eq!(e, ArqError::Timeout { seq: 0, retries: 8, waited: Duration::from_secs(55) });
    assert_eq!(e.class(), pod_core::Class::Timeout);
    assert!(e.to_string().starts_with("arq-timeout"), "{e}");
    assert!(a.poll_transmit(now).is_none());
    assert_eq!(a.write(b"more").unwrap_err(), e);
}

/// After a round trip of 0, the RTO is the floor of 200 ms; with no more
/// `ACK` the next frame goes again at 0.2, 0.6, 1.4, 3.0, 6.2, 12.6, 20.6
/// and 28.6 s, and the ARQ ends at 36.6 s.
#[test]
fn the_rto_keeps_its_floor_and_its_ceiling() {
    let t0 = Instant::now();
    let mut a = arq(ArqSettings::default());
    let mut b = arq(ArqSettings::default());
    assert!(a.send_record(b"first").unwrap());
    give(&mut b, t0, &drain(&mut a, t0));
    give(&mut a, t0, &drain(&mut b, t0));
    assert_eq!(a.rto(), Duration::from_millis(200));
    assert!(a.send_record(b"second").unwrap());
    drain(&mut a, t0);
    let mut sends = Vec::new();
    for _ in 0..20 {
        let Some(at) = a.poll_timeout() else { break };
        a.on_timeout(at);
        if drain(&mut a, at).is_empty() {
            break;
        }
        sends.push((at - t0).as_millis());
    }
    assert_eq!(sends, [200, 600, 1400, 3000, 6200, 12600, 20600, 28600]);
    let Some(ArqError::Timeout { seq: 1, retries: 8, waited }) = a.error().cloned() else { panic!("{:?}", a.error()) };
    assert_eq!(waited, Duration::from_millis(36600));
}

/// A reader that does not read closes its window: the sender stops, and
/// probes the peer after each RTO. The peer answers, so the sender does
/// not end, also after an hour. A read opens the window, and the bytes go.
#[test]
fn a_closed_window_is_probed_and_opens() {
    let t0 = Instant::now();
    let mut a = arq(ArqSettings::default());
    let mut b = arq(ArqSettings { receive_buffer: 256 << 10, ..ArqSettings::default() });
    let data = pattern(512 << 10, 9);
    for chunk in data.chunks(64 << 10) {
        assert!(a.send_record(chunk).unwrap());
    }
    let mut now = t0;
    let mut probes = 0;
    while now < t0 + Duration::from_secs(3600) {
        let out = drain(&mut a, now);
        probes += out.iter().filter(|d| **d == Datagram::Probe).count();
        give(&mut b, now, &out);
        give(&mut a, now, &drain(&mut b, now));
        let Some(at) = a.poll_timeout() else { break };
        now = at.max(now);
        a.on_timeout(now);
    }
    assert!(a.error().is_none(), "{:?}", a.error());
    assert_eq!(b.readable(), 256 << 10);
    assert!(probes > 400, "{probes} probes in an hour");
    let mut got = Vec::new();
    while got.len() < data.len() {
        let mut buf = [0; 4096];
        let n = b.read(&mut buf).unwrap();
        got.extend_from_slice(&buf[..n]);
        give(&mut a, now, &drain(&mut b, now));
        give(&mut b, now, &drain(&mut a, now));
    }
    assert_eq!(got, data);
}

/// The first sequence is 10 before the wrap: the ARQ asks for a rekey at
/// once. Ten frames go, then no new frame until the rekey; no sequence
/// carries two payloads. After `rekeyed` the sequence wraps to 0, the
/// receiver has each byte in order, and the next rekey is far.
#[test]
fn the_sequence_does_not_wrap_without_a_rekey() {
    let t0 = Instant::now();
    let settings = ArqSettings { start: u32::MAX - 9, ..ArqSettings::default() };
    let mut a = arq(settings);
    let mut b = arq(settings);
    assert!(a.rekey_due());
    let data = pattern(20 << 10, 5);
    assert!(a.send_record(&data).unwrap());
    let mut payloads: BTreeMap<u32, Datagram> = BTreeMap::new();
    pump(&mut a, &mut b, &mut payloads, t0);
    assert_eq!(payloads.len(), 10);
    assert!(payloads.keys().all(|s| *s >= u32::MAX - 9));
    assert!(a.rekey_due());
    assert_eq!(a.stats().rekeys, 0);
    a.rekeyed();
    pump(&mut a, &mut b, &mut payloads, t0);
    assert_eq!(payloads.len(), 20);
    assert_eq!(payloads.keys().filter(|s| **s < 10).count(), 10);
    assert!(!a.rekey_due());
    assert_eq!(a.stats().rekeys, 1);
    assert_eq!(b.recv_record().unwrap(), Some(data));
}

/// Four turns of frames and `ACK`s at `now`; each sequence keeps one
/// payload.
fn pump(a: &mut Arq, b: &mut Arq, payloads: &mut BTreeMap<u32, Datagram>, now: Instant) {
    for _ in 0..4 {
        let out = drain(a, now);
        for d in &out {
            let s = seq(d).unwrap();
            assert_eq!(payloads.entry(s).or_insert_with(|| d.clone()), d, "the sequence {s} with two payloads");
        }
        give(b, now, &out);
        give(a, now, &drain(b, now));
    }
}

/// With `rekey_ahead` of 256, a start 1000 frames before the wrap asks for
/// no rekey; the ask comes when 256 frames are left.
#[test]
fn the_rekey_is_asked_ahead_of_the_wrap() {
    let t0 = Instant::now();
    let settings = ArqSettings { start: u32::MAX - 999, rekey_ahead: 256, ..ArqSettings::default() };
    let mut a = arq(settings);
    let mut b = arq(settings);
    assert!(!a.rekey_due());
    let mut sent = 0u32;
    while !a.rekey_due() {
        assert!(a.send_record(&[1; 1024]).unwrap());
        let out = drain(&mut a, t0);
        sent += out.len() as u32;
        give(&mut b, t0, &out);
        give(&mut a, t0, &drain(&mut b, t0));
    }
    assert_eq!(sent, 1000 - 256);
}

/// Datagrams that the ARQ does not take are dropped and counted: an
/// unknown type, a bad length, a frame out of the window, an `ACK` of
/// frames not sent. Random bytes never panic; a peer whose fragments
/// disagree ends the ARQ with a protocol error.
#[test]
fn datagrams_that_break_the_form_are_dropped() {
    let now = Instant::now();
    let mut b = arq(ArqSettings::default());
    let far = Datagram::Data { seq: 300, index: 0, count: 1, payload: vec![1] };
    let ack = Datagram::Ack { next: 50, window: 1, bitmap: Bitmap::default() };
    for d in [vec![], vec![9], vec![2, 0], far.to_vec(), ack.to_vec()] {
        b.on_datagram(now, &d);
    }
    assert_eq!(b.stats().dropped, 5);
    assert!(b.error().is_none());
    let mut rng = SplitMix64(7);
    for _ in 0..20_000 {
        let len = (rng.next_u64() % 64) as usize;
        let mut d: Vec<u8> = (0..len).map(|_| rng.next_u64() as u8).collect();
        if let Some(t) = d.first_mut() {
            *t %= 5;
        }
        b.on_datagram(now, &d);
        let _ = b.poll_transmit(now);
    }
    let mut c = arq(ArqSettings::default());
    c.on_datagram(now, &Datagram::Data { seq: 0, index: 0, count: 2, payload: vec![1] }.to_vec());
    c.on_datagram(now, &Datagram::Data { seq: 1, index: 0, count: 3, payload: vec![1] }.to_vec());
    assert!(matches!(c.error(), Some(ArqError::Protocol(_))), "{:?}", c.error());
}

/// A frame that comes two times counts as a duplicate, and its bytes go
/// to the reader once. The `FIN` ends the reader after the bytes.
#[test]
fn a_duplicate_goes_once_and_the_fin_ends_the_reader() {
    let now = Instant::now();
    let mut a = arq(ArqSettings::default());
    let mut b = arq(ArqSettings::default());
    assert_eq!(a.write(b"hello").unwrap(), 5);
    a.close();
    assert_eq!(a.write(b"late").unwrap_err(), ArqError::WriteClosed);
    let out = drain(&mut a, now);
    assert_eq!(out.len(), 2);
    give(&mut b, now, &out);
    give(&mut b, now, &out);
    assert_eq!(b.stats().duplicates, 2);
    let mut buf = [0; 16];
    assert_eq!(b.read(&mut buf).unwrap(), 5);
    assert_eq!(&buf[..5], b"hello");
    assert!(b.read_finished());
    give(&mut a, now, &drain(&mut b, now));
    assert!(a.closed());
}

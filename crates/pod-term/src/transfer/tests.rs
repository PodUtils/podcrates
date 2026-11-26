//! The test vectors of the design: the rows T (the near end's scan of the
//! output) and F (the frame reader). The exchanges and the faults are
//! `tests/transfer.rs`.

use std::time::Instant;

use super::reader::{Got, Reader};
use super::wire::{self, Msg};
use super::*;

const ID: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
const T1: &[u8] = b"#PODSSH-TX2:S:0123456789abcdef:f9541cc2\r\n";
const F1: &[u8] = b"#HELO:AgEjRWeJq83vAACAAA!9c5e7795\r\n";
const DATA: &[u8] = b"#DATA:AAAAAAAAAABoZWxsbwo!beee1f4d";

fn hello_data() -> Msg {
    Msg::Data { offset: 0, bytes: b"hello\n".to_vec() }
}

fn shown(events: &[Event]) -> Vec<u8> {
    events.iter().flat_map(|e| if let Event::Show(b) = e { b.clone() } else { Vec::new() }).collect()
}

fn starts(events: &[Event]) -> usize {
    events.iter().filter(|e| matches!(e, Event::Start { .. })).count()
}

/// What the reader finds in `reads`, each frame as its message.
fn frames(reads: &[&[u8]]) -> Vec<Result<Msg, Got>> {
    let mut reader = Reader::new();
    let mut found = Vec::new();
    for read in reads {
        for &b in *read {
            match reader.push(b) {
                Some(Got::Frame { kind, payload }) => found.push(Ok(Msg::decode(&kind, &payload).expect("a message"))),
                Some(other) => found.push(Err(other)),
                None => {}
            }
        }
    }
    found
}

#[test]
fn t1_a_trigger_after_output_starts_a_download_and_sends_nothing() {
    let mut near = Transfer::near();
    let now = Instant::now();
    let events = near.feed(&[b"ls\r\n".as_slice(), T1].concat(), now);
    assert_eq!(events, [Event::Show(b"ls\r\n".to_vec()), Event::Start { dir: Dir::Send, id: ID }]);
    assert_eq!(near.poll_output(now), b"");
}

#[test]
fn t2_a_trigger_of_an_upload() {
    let events = Transfer::near().feed(b"#PODSSH-TX2:R:0123456789abcdef:1606aa23\r\n", Instant::now());
    assert_eq!(events, [Event::Start { dir: Dir::Receive, id: ID }]);
}

#[test]
fn t3_t4_a_trigger_cut_by_a_read_waits_for_its_rest_100_ms() {
    let now = Instant::now();
    let mut near = Transfer::near();
    assert_eq!(near.feed(b"#PODSSH-TX2:S:0123", now), []);
    assert_eq!(near.feed(b"456789abcdef:f9541cc2\r\n", now), [Event::Start { dir: Dir::Send, id: ID }]);
    let mut near = Transfer::near();
    assert_eq!(near.feed(b"#PODSSH-TX2:S:0123", now), []);
    assert_eq!(near.deadline(), Some(now + TRIGGER_HOLD));
    assert_eq!(near.tick(now + TRIGGER_HOLD / 2), []);
    assert_eq!(near.tick(now + TRIGGER_HOLD), [Event::Show(b"#PODSSH-TX2:S:0123".to_vec())]);
    assert_eq!(near.deadline(), None);
}

#[test]
fn t5_t6_t9_a_trigger_that_does_not_hold_shows_each_byte() {
    for (bytes, unsupported) in [
        (&b"#PODSSH-TX2:S:0123456789abcdef:f9541cc3\r\n"[..], None),
        (b"#PODSSH-TX1:S:0123456789abcdef:56fd5108\r\n", Some(1)),
        (b"#PODSSH-TX3:S:0123456789abcdef:9c332784\r\n", Some(3)),
        (b"#PODSSH-TX2:S:0123\x1b[K456789abcdef:f9541cc2\r\n", None),
    ] {
        let events = Transfer::near().feed(bytes, Instant::now());
        assert_eq!(shown(&events), bytes, "{:?}", String::from_utf8_lossy(bytes));
        assert_eq!(starts(&events), 0);
        let versions: Vec<u32> = events
            .iter()
            .filter_map(|e| if let Event::Unsupported { version } = e { Some(*version) } else { None })
            .collect();
        assert_eq!(versions, unsupported.into_iter().collect::<Vec<_>>());
    }
}

#[test]
fn t7_t8_a_trigger_seen_before_starts_nothing_and_a_new_id_starts() {
    let now = Instant::now();
    let mut near = Transfer::near();
    assert_eq!(starts(&near.feed(T1, now)), 1);
    near.refuse("refused here");
    assert!(!near.active());
    let again = near.feed(T1, now);
    assert_eq!((shown(&again), starts(&again)), (T1.to_vec(), 0));
    let id = [0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10];
    assert_eq!(near.feed(b"#PODSSH-TX2:S:fedcba9876543210:756e6da1", now), [Event::Start { dir: Dir::Send, id }]);
}

#[test]
fn the_far_end_paints_the_trigger_of_the_vectors() {
    assert_eq!(trigger::line(Dir::Send, ID), [b"\r\x1b[K".as_slice(), T1].concat());
    let far = Transfer::far_receive(ID, Instant::now()).poll_output(Instant::now());
    assert_eq!(far, b"\r\x1b[K#PODSSH-TX2:R:0123456789abcdef:1606aa23\r\n");
}

#[test]
fn f1_f2_a_frame_ends_at_its_crc_whatever_follows() {
    let hello = Msg::Helo { version: 2, id: ID, chunk: 32768 };
    assert_eq!(wire::frame(&hello, b"\r\n", WRAP), F1);
    for end in [&b"\r\n"[..], b"\n", b"\r", b""] {
        let frame = [&F1[..F1.len() - 2], end].concat();
        assert_eq!(frames(&[&frame]), [Ok(hello.clone())], "{end:?}");
    }
}

#[test]
fn f3_f4_f5_f8_a_wrapped_repainted_or_cut_frame_is_one_frame() {
    for reads in [
        vec![&b"#DATA:AAAAAA\r\nAAAABoZWxsbwo!beee1f4d\r\n"[..]],
        vec![b"junk\x1b[K#DATA:AAAAAA\x1b[12;1HAAAABoZWxsbwo!beee1f4d"],
        vec![b"#DATA:AAAAAAAAAABo", b"ZWxsbwo!beee", b"1f4d"],
        vec![b"#DATA:AAAAAAAAAABoZWxsbwo=!beee1f4d"],
    ] {
        assert_eq!(frames(&reads), [Ok(hello_data())], "{reads:?}");
    }
    assert_eq!(wire::frame(&hello_data(), b"", WRAP), DATA);
}

#[test]
fn f6_f7_f9_a_frame_with_a_bad_byte_crc_or_length_is_damaged() {
    for bad in [
        &b"#DATA:AA*AAAAAAABoZWxsbwo!beee1f4d"[..],
        b"#DATA:AAAAAAAAAABoZWxsbwo!beee1f40",
        b"#DATA:AAAAAAAAAABoZWxsbwoA!beee1f4d",
    ] {
        assert_eq!(frames(&[bad]), [Err(Got::Damaged)], "{:?}", String::from_utf8_lossy(bad));
    }
}

#[test]
fn f14_hold_is_a_frame_with_an_empty_payload() {
    assert_eq!(wire::frame(&Msg::Hold, b"\r", WRAP), b"#HOLD:!464f8be9\r");
    assert_eq!(frames(&[b"#HOLD:!464f8be9\r\n"]), [Ok(Msg::Hold)]);
    assert_eq!(frames(&[b"#HOLD:AA!464f8be9"]), [Err(Got::Damaged)]);
}

#[test]
fn f10_a_damaged_frame_resyncs_at_the_next_hash() {
    let found = frames(&[b"#DATA:AAAA#ACKN:AAAAAAAAAAAAAAAAAAAABg!168c223e"]);
    assert_eq!(found, [Err(Got::Damaged), Ok(Msg::Ackn { of: 0, count: 6 })]);
}

/// A far receiver that waits for `HELO`, fed `bytes`: what it sends after
/// its trigger, and its events.
fn far_fed(bytes: &[u8]) -> (Vec<u8>, Vec<Event>) {
    let now = Instant::now();
    let mut far = Transfer::far_receive(ID, now);
    far.poll_output(now);
    let events = far.feed(bytes, now);
    (far.poll_output(now), events)
}

fn stopped(events: &[Event]) -> Option<u8> {
    events.iter().find_map(|e| match e {
        Event::End { ended: Ended::Stopped { code, .. }, .. } => Some(*code),
        _ => None,
    })
}

#[test]
fn f11_f12_f13_a_long_frame_junk_or_an_unknown_kind_stops_the_session() {
    let mut long = b"#DATA:".to_vec();
    long.extend(std::iter::repeat_n(b'A', 44_027));
    let junk = vec![b'x'; 65_537];
    for (what, bytes) in [("a long frame", long), ("junk", junk), ("an unknown kind", b"#ZZZZ:!f74208bb".to_vec())] {
        let (sent, events) = far_fed(&bytes);
        assert_eq!(stopped(&events), Some(stop::PROTOCOL), "{what}: {events:?}");
        assert!(sent.starts_with(b"#STOP:BA"), "{what}: {:?}", String::from_utf8_lossy(&sent));
    }
    // One byte less is no fault.
    let (sent, events) = far_fed(&vec![b'x'; 65_536]);
    assert_eq!((sent, events), (Vec::new(), Vec::new()));
}

#[test]
fn nine_damaged_frames_in_a_row_stop_the_session_and_eight_do_not() {
    let bad = b"#DATA:AA*\r\n".repeat(DAMAGED_MAX);
    let (sent, events) = far_fed(&bad);
    assert_eq!((sent.len(), events.len()), (0, DAMAGED_MAX));
    let (sent, events) = far_fed(&[bad.as_slice(), b"#DATA:AA*"].concat());
    assert_eq!(stopped(&events), Some(stop::PROTOCOL));
    assert!(sent.starts_with(b"#STOP:"));
}

#[test]
fn five_can_from_the_peer_end_the_session_with_no_stop_sent() {
    let (sent, events) = far_fed(b"\x18\x18x\x18\x18\x18\x18\x18");
    assert_eq!(sent, b"");
    assert!(matches!(&events[..], [Event::End { ended: Ended::Stopped { code: 1, by_peer: true, .. }, .. }]));
}

#[test]
fn a_long_frame_breaks_inside_its_base64_and_reads_as_one() {
    let data = Msg::Data { offset: 7, bytes: (0..=255).collect() };
    for width in [16, 40, 79] {
        let text = wire::frame(&data, b"\r\n", width);
        assert!(text.split(|&b| b == b'\n').all(|line| line.len() <= width + 1), "width {width}");
        assert_eq!(frames(&[&text]), [Ok(data.clone())], "width {width}");
    }
}

#[test]
fn a_name_from_the_peer_is_its_last_component_or_none() {
    assert_eq!(wire::file_name(b"dir/a.txt").as_deref(), Some("a.txt"));
    for bad in [&b".."[..], b"dir/", b"a/.", b"a\x01b", b"a\x7fb", "a\u{85}b".as_bytes(), b"\xff", &[b'a'; 256]] {
        assert_eq!(wire::file_name(bad), None, "{bad:?}");
    }
    let long = [b"d/".repeat(512).as_slice(), b"a"].concat();
    assert_eq!(wire::file_name(&long), None);
    assert_eq!(wire::file_name(&[b'a'; 255]).map(|n| n.len()), Some(255));
}

#[test]
fn a_reason_is_cut_to_200_bytes_at_a_character() {
    let long = "\u{e9}".repeat(150);
    let cut = wire::cut(long.as_bytes());
    assert_eq!(cut.len(), 200);
    assert!(std::str::from_utf8(cut).is_ok());
    let stop = Msg::Stop { code: 4, reason: format!("x{long}").into_bytes() };
    assert_eq!(stop.payload().len(), 1 + 2 + 199);
}

/// The lines of a frame: each ends in CR LF.
fn lines_of(frame: &[u8]) -> usize {
    frame.windows(2).filter(|w| w == b"\r\n").count()
}

#[test]
fn the_lines_of_a_frame_are_those_that_wire_breaks() {
    for width in [16, 39, 47, 79, 119] {
        assert_eq!(pane::lines(0, width), lines_of(&wire::frame(&Msg::Hold, b"\r\n", width)));
        for n in 0..3000 {
            let data = Msg::Data { offset: 9, bytes: vec![b'x'; n] };
            let frame = wire::frame(&data, b"\r\n", width);
            let len = data.payload().len();
            assert_eq!(pane::lines(len, width), lines_of(&frame), "width {width}, payload {len}");
        }
    }
}

#[test]
fn each_chunk_up_to_the_cap_of_a_pane_stays_within_its_rows() {
    for (cols, rows) in [(120, 23), (48, 23), (80, 24), (200, 60)] {
        let cap = pane::chunk(cols, rows);
        let data = |n: usize| Msg::Data { offset: 1 << 40, bytes: vec![0xa5; n] };
        let most = (1..=cap).map(|n| lines_of(&wire::frame(&data(n), b"\r\n", cols - 1))).max();
        assert!(most <= Some(rows - SPARE_ROWS), "{rows} by {cols}: {most:?} lines at the cap {cap}");
        // The cap is the largest that fits.
        let next = lines_of(&wire::frame(&data(cap + 1), b"\r\n", cols - 1));
        assert!(cap == CHUNK_MAX || next > rows - SPARE_ROWS, "{rows} by {cols}: {cap} + 1 fits");
    }
}

/// A far sender of `file` after `TAKE`, the time, and its `NeedData`.
fn far_sender(file: &[u8], now: Instant) -> (Transfer, Vec<Event>) {
    let info = FileInfo { name: "f".into(), size: file.len() as u64, mode: None, mtime: None };
    let mut far = Transfer::far_send(info, ID, now);
    far.poll_output(now);
    let hello = Msg::Helo { version: 2, id: ID, chunk: CHUNK_MAX as u32 };
    far.feed(&wire::frame(&hello, b"\r", WRAP), now);
    far.poll_output(now);
    let events = far.feed(&wire::frame(&Msg::Take { from: 0 }, b"\r", WRAP), now);
    (far, events)
}

fn ackn(of: u64, count: u64) -> Vec<u8> {
    wire::frame(&Msg::Ackn { of, count }, b"\r", WRAP)
}

#[test]
fn a_data_lost_goes_again_at_the_floor_and_a_late_ack_of_the_first_counts() {
    use sha2::{Digest, Sha256};
    let mut now = Instant::now();
    let file: Vec<u8> = (0..3000u32).map(|i| (i * 7) as u8).collect();
    let (mut far, events) = far_sender(&file, now);
    assert_eq!(events, [Event::NeedData { offset: 0, max: CHUNK_MIN }]);
    far.supply(&file[..CHUNK_MIN]);
    let first = far.poll_output(now);
    assert_eq!(frames(&[&first]), [Ok(Msg::Data { offset: 0, bytes: file[..CHUNK_MIN].to_vec() })]);
    now += RESEND_MIN;
    far.tick(now);
    let again = far.poll_output(now);
    assert_ne!(again, first, "the resend repeats the bytes that the path cut");
    assert_eq!(frames(&[&again]), [Ok(Msg::Data { offset: 0, bytes: file[..CHUNK_FLOOR].to_vec() })]);
    assert_eq!(far.chunk(), Some(CHUNK_FLOOR));
    // A second loss sends the frame at the floor again.
    now += RESEND_MIN;
    far.tick(now);
    assert_eq!(far.poll_output(now), again);
    // The first frame came late, and its ack counts past the frame in
    // flight: no fault, and the next DATA starts at the count.
    let mut events = far.feed(&ackn(0, CHUNK_MIN as u64), now);
    let next = Event::NeedData { offset: CHUNK_MIN as u64, max: CHUNK_FLOOR };
    assert_eq!(events, [Event::Progress { done: CHUNK_MIN as u64, size: 3000 }, next]);
    while let Some(Event::NeedData { offset, max }) = events.pop() {
        let (from, to) = (offset as usize, (offset as usize + max).min(file.len()));
        far.supply(&file[from..to]);
        far.poll_output(now);
        events = far.feed(&ackn(offset, to as u64), now);
    }
    let sha256: [u8; 32] = Sha256::digest(&file).into();
    assert_eq!(frames(&[&far.poll_output(now)]), [Ok(Msg::Done { size: 3000, sha256 })]);
}

#[test]
fn an_ack_past_the_bytes_supplied_stops_the_session() {
    let now = Instant::now();
    let file = vec![1u8; 3000];
    let (mut far, _) = far_sender(&file, now);
    far.supply(&file[..CHUNK_MIN]);
    far.poll_output(now);
    let events = far.feed(&ackn(0, CHUNK_MIN as u64 + 1), now);
    assert!(
        matches!(events.last(), Some(Event::End { ended: Ended::Stopped { code: stop::PROTOCOL, .. }, .. })),
        "{events:?}"
    );
}

#[test]
fn an_offer_names_the_last_component_only() {
    let now = Instant::now();
    let info = FileInfo { name: "a/b/c.txt".into(), size: 1, mode: None, mtime: None };
    let mut far = Transfer::far_send(info, ID, now);
    far.poll_output(now);
    far.feed(&wire::frame(&Msg::Helo { version: 2, id: ID, chunk: 300 }, b"\r", WRAP), now);
    let sent = frames(&[&far.poll_output(now)]);
    let offer = Msg::Offr { size: 1, mode: 0, mtime: 0, name: b"c.txt".to_vec() };
    // The far end answers with the smaller cap of the two.
    assert_eq!(sent, [Ok(Msg::Helo { version: 2, id: ID, chunk: 300 }), Ok(offer)]);
}

//! The exchanges and the faults of the design of the transfer codec
//! (podssh's `docs/terminal.md`, "The 7-bit envelope"), byte for byte: the
//! file `a.txt` of 6 bytes `hello\n`, mode 0644, the id `0123456789abcdef`.
//! The near end's frames end in CR, the far end's in CRLF.

#![cfg(feature = "transfer")]

use std::time::{Duration, Instant};

use pod_term::transfer::{
    CHUNK_MIN, Dir, Ended, Event, FileInfo, LINGER, RESEND_MAX, RESEND_MIN, Transfer, WAIT, stop,
};

const ID: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
const T1: &[u8] = b"#PODSSH-TX2:S:0123456789abcdef:f9541cc2\r\n";
const T2: &[u8] = b"#PODSSH-TX2:R:0123456789abcdef:1606aa23\r\n";
const HELO: &str = "#HELO:AgEjRWeJq83vAACAAA!9c5e7795";
const OFFR: &str = "#OFFR:AAAAAAAAAAYAAAGkAAAAAAAAAAAABWEudHh0!51054221";
const TAKE: &str = "#TAKE:AAAAAAAAAAA!5b64f8f9";
const DATA: &str = "#DATA:AAAAAAAAAABoZWxsbwo!beee1f4d";
const ACKN: &str = "#ACKN:AAAAAAAAAAAAAAAAAAAABg!168c223e";
const DONE: &str = "#DONE:AAAAAAAAAAZYkbW1ItXfCG0P8LEQ+9nSG7T8cWOvNNCChqLoRva+Aw!6ff6dfbc";
const KEPT: &str = "#KEPT:AAAAAAAAAAY!67cdbc05";
const HOLD: &str = "#HOLD:!464f8be9";
/// A `DONE` of the same size with a SHA-256 of zeros.
const DONE_ZEROS: &str = "#DONE:AAAAAAAAAAYAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA!b65d2465";
const SHA256: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

fn a_txt() -> FileInfo {
    FileInfo { name: "a.txt".into(), size: 6, mode: Some(0o644), mtime: None }
}

/// A frame of the near end, and of the far end.
fn near(frame: &str) -> Vec<u8> {
    format!("{frame}\r").into_bytes()
}

fn far(frame: &str) -> Vec<u8> {
    format!("{frame}\r\n").into_bytes()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn ended(events: &[Event]) -> Option<Ended> {
    events.iter().find_map(|e| if let Event::End { ended, .. } = e { Some(ended.clone()) } else { None })
}

fn code(events: &[Event]) -> Option<u8> {
    match ended(events)? {
        Ended::Stopped { code, .. } => Some(code),
        Ended::Done | Ended::Unconfirmed => None,
    }
}

/// The near end of the download, after the step `step` of its exchange,
/// and the time.
fn download(step: u32) -> (Transfer, Instant) {
    let now = Instant::now();
    let mut t = Transfer::near();
    let events = t.feed(T1, now);
    assert_eq!(events, [Event::Start { dir: Dir::Send, id: ID }]);
    if step < 2 {
        return (t, now);
    }
    t.accept(now);
    assert_eq!(text(&t.poll_output(now)), text(&near(HELO)), "2");
    assert_eq!(t.feed(&far(HELO), now), [], "3");
    let events = t.feed(&far(OFFR), now);
    assert_eq!(events, [Event::Offer { name: "a.txt".into(), size: 6, mode: Some(0o644), mtime: None }], "4");
    if step < 5 {
        return (t, now);
    }
    t.take();
    assert_eq!(text(&t.poll_output(now)), text(&near(TAKE)), "5");
    if step < 6 {
        return (t, now);
    }
    let events = t.feed(&far(DATA), now);
    assert_eq!(events[0], Event::Write { offset: 0, bytes: b"hello\n".to_vec() }, "6");
    assert_eq!(text(&t.poll_output(now)), text(&near(ACKN)), "7");
    (t, now)
}

#[test]
fn the_download_of_the_design_byte_for_byte() {
    let (mut t, now) = download(7);
    let events = t.feed(&far(DONE), now);
    let Some(Event::Verified { size: 6, sha256 }) = events.last() else { panic!("8: {events:?}") };
    assert_eq!(sha256.iter().map(|b| format!("{b:02x}")).collect::<String>(), SHA256);
    t.kept();
    assert_eq!(text(&t.poll_output(now)), text(&near(KEPT)), "9");
    assert_eq!(ended(&t.events()), Some(Ended::Done));
    assert!(!t.active());
}

/// The near end of the upload, after the step `step` of its exchange.
fn upload(step: u32) -> (Transfer, Instant) {
    let now = Instant::now();
    let mut t = Transfer::near();
    assert_eq!(t.feed(T2, now), [Event::Start { dir: Dir::Receive, id: ID }], "1");
    t.send_file(a_txt(), now);
    assert_eq!(text(&t.poll_output(now)), text(&near(HELO)), "2");
    assert_eq!(t.feed(&far(HELO), now), [], "3");
    assert_eq!(text(&t.poll_output(now)), text(&near(OFFR)), "4");
    if step < 5 {
        return (t, now);
    }
    assert_eq!(t.feed(&far(TAKE), now), [Event::NeedData { offset: 0, max: 1024 }], "5");
    t.supply(b"hello\n");
    assert_eq!(text(&t.poll_output(now)), text(&near(DATA)), "6");
    (t, now)
}

#[test]
fn the_upload_of_the_design_byte_for_byte() {
    let (mut t, now) = upload(6);
    let events = t.feed(&far(ACKN), now);
    assert_eq!(events, [Event::Progress { done: 6, size: 6 }], "7");
    assert_eq!(text(&t.poll_output(now)), text(&near(DONE)), "8");
    let events = t.feed(&far(KEPT), now);
    assert_eq!(ended(&events), Some(Ended::Done), "9");
}

#[test]
fn the_far_end_of_each_exchange_byte_for_byte() {
    let now = Instant::now();
    let mut send = Transfer::far_send(a_txt(), ID, now);
    assert_eq!(send.poll_output(now), [b"\r\x1b[K".as_slice(), T1].concat());
    assert_eq!(send.feed(&near(HELO), now), []);
    assert_eq!(text(&send.poll_output(now)), text(&[far(HELO), far(OFFR)].concat()));
    assert_eq!(send.feed(&near(TAKE), now), [Event::NeedData { offset: 0, max: 1024 }]);
    send.supply(b"hello\n");
    assert_eq!(text(&send.poll_output(now)), text(&far(DATA)));
    send.feed(&near(ACKN), now);
    assert_eq!(text(&send.poll_output(now)), text(&far(DONE)));
    assert_eq!(ended(&send.feed(&near(KEPT), now)), Some(Ended::Done));

    far_receiver(now, true);
}

/// The far end of the upload, byte for byte up to `Verified`, and up to
/// the hand-over of its `KEPT` when `keep` is set.
fn far_receiver(now: Instant, keep: bool) -> Transfer {
    let mut receive = Transfer::far_receive(ID, now);
    receive.poll_output(now);
    receive.feed(&near(HELO), now);
    assert_eq!(text(&receive.poll_output(now)), text(&far(HELO)));
    assert!(matches!(&receive.feed(&near(OFFR), now)[..], [Event::Offer { .. }]));
    receive.take();
    assert_eq!(text(&receive.poll_output(now)), text(&far(TAKE)));
    receive.feed(&near(DATA), now);
    assert_eq!(text(&receive.poll_output(now)), text(&far(ACKN)));
    assert!(matches!(receive.feed(&near(DONE), now).last(), Some(Event::Verified { .. })));
    if keep {
        receive.kept();
        assert_eq!(text(&receive.poll_output(now)), text(&far(KEPT)));
        assert_eq!(ended(&receive.events()), Some(Ended::Done));
    }
    receive
}

#[test]
fn x1_a_lost_ack_sends_the_same_data_again_and_the_chunk_falls() {
    let (mut t, now) = upload(6);
    assert_eq!(t.deadline(), Some(now + Duration::from_secs(1)));
    assert_eq!(t.tick(now + Duration::from_millis(999)), []);
    assert_eq!(t.poll_output(now), b"");
    t.tick(now + Duration::from_secs(1));
    let later = now + Duration::from_secs(1);
    assert_eq!(text(&t.poll_output(later)), text(&near(DATA)));
    t.feed(&far(ACKN), later);
    assert_eq!(text(&t.poll_output(later)), text(&near(DONE)));
    assert_eq!(t.chunk(), Some(1024));
}

#[test]
fn x2_an_ack_that_does_not_advance_sends_the_data_again() {
    let (mut t, now) = upload(6);
    assert_eq!(t.feed(&far("#ACKN:AAAAAAAAAAAAAAAAAAAAAA!ffef870b"), now), []);
    assert_eq!(text(&t.poll_output(now)), text(&near(DATA)));
}

#[test]
fn x3_a_duplicate_data_writes_nothing_and_is_acked_again() {
    let (mut t, now) = download(7);
    assert_eq!(t.feed(&far(DATA), now), []);
    assert_eq!(text(&t.poll_output(now)), text(&near(ACKN)));
}

#[test]
fn x4_a_hash_that_differs_keeps_nothing() {
    let (mut t, now) = download(7);
    let events = t.feed(&far(DONE_ZEROS), now);
    assert!(!events.iter().any(|e| matches!(e, Event::Verified { .. })), "{events:?}");
    assert_eq!(code(&events), Some(stop::DIFFERS));
    let stop = near("#STOP:AwAhU0hBLTI1NiBkaWZmZXJzOyBub3RoaW5nIHdhcyBrZXB0!59691c9e");
    assert_eq!(text(&t.poll_output(now)), text(&stop));
}

#[test]
fn x5_x6_ctrl_c_or_five_can_typed_stop_the_session() {
    for (keys, frame) in [
        (vec![&b"\x03"[..]], "#STOP:AQAXY2FuY2VsbGVkIGhlcmUgKEN0cmwtQyk!7306070e"),
        (vec![b"\x18\x18", b"\x18\x18\x18"], "#STOP:AQAUY2FuY2VsbGVkIGhlcmUgKENBTik!f4d1116f"),
    ] {
        let (mut t, now) = download(6);
        for k in keys {
            assert_eq!(t.keys(k, now), b"");
        }
        assert_eq!(text(&t.poll_output(now)), text(&near(frame)));
        assert_eq!(code(&t.events()), Some(stop::CANCELLED));
        // Outside a session, each key passes.
        assert_eq!(t.keys(b"\x03", now), b"\x03");
    }
}

#[test]
fn x7_a_refusal_at_the_start() {
    let (mut t, now) = download(1);
    assert_eq!(t.keys(b"y", now), b"y", "keys pass before the caller accepts");
    t.refuse("refused here");
    assert_eq!(text(&t.poll_output(now)), text(&near("#STOP:BgAMcmVmdXNlZCBoZXJl!cffba5b2")));
    assert_eq!(code(&t.events()), Some(stop::REFUSED));
}

#[test]
fn x8_a_skip_by_the_far_end_ends_the_upload() {
    let (mut t, now) = upload(4);
    let events = t.feed(&far("#SKIP:ABFhLnR4dCBleGlzdHMgaGVyZQ!409966d8"), now);
    let reason = "a.txt exists here".to_string();
    assert_eq!(ended(&events), Some(Ended::Stopped { code: stop::REFUSED, by_peer: true, reason }));
}

#[test]
fn x9_x10_a_name_with_a_path_is_its_last_component_and_dot_dot_is_refused() {
    let now = Instant::now();
    let offer = |frame: &str| {
        let mut t = Transfer::near();
        t.feed(T1, now);
        t.accept(now);
        t.feed(&far(HELO), now);
        let events = t.feed(&far(frame), now);
        (events, t.poll_output(now))
    };
    let (events, _) = offer("#OFFR:AAAAAAAAAAYAAAGkAAAAAAAAAAAACWRpci9hLnR4dA!21fcfe26");
    assert!(matches!(&events[..], [Event::Offer { name, .. }] if name == "a.txt"), "{events:?}");
    let (events, sent) = offer("#OFFR:AAAAAAAAAAYAAAGkAAAAAAAAAAAAAi4u!4533a1d9");
    assert!(!events.iter().any(|e| matches!(e, Event::Offer { .. })));
    assert_eq!(code(&events), Some(stop::REFUSED));
    assert!(text(&sent).ends_with(&format!("{}\r", "#STOP:BgAbdGhlIG5hbWUgbmFtZXMgbm8gZmlsZSBoZXJl!077e5dcc")));
}

#[test]
fn x11_silence_for_20_s_stops_the_session() {
    let (mut t, now) = download(5);
    assert_eq!(t.deadline(), Some(now + WAIT));
    assert_eq!(t.tick(now + WAIT - Duration::from_millis(1)), []);
    assert_eq!(code(&t.tick(now + WAIT)), Some(stop::OUT_OF_TIME));
    assert!(text(&t.poll_output(now + WAIT)).starts_with("#STOP:Ag"));
}

#[test]
fn x12_an_echo_of_its_own_frame_is_junk() {
    let (mut t, now) = download(5);
    assert_eq!(t.feed(&far(TAKE), now), []);
    assert_eq!(t.poll_output(now), b"");
}

#[test]
fn the_output_after_the_last_frame_shows_and_a_new_trigger_is_watched() {
    let (mut t, now) = upload(6);
    t.feed(&far(ACKN), now);
    let events = t.feed(&[far(KEPT).as_slice(), b"$ "].concat(), now);
    assert_eq!(events, [Event::End { ended: Ended::Done, rest: b"$ ".to_vec() }]);
    let events = t.feed(b"ls\r\n#PODSSH-TX2:S:fedcba9876543210:756e6da1", now);
    assert!(matches!(&events[..], [Event::Show(_), Event::Start { .. }]), "{events:?}");
}

#[test]
fn the_chunk_doubles_after_a_fast_ack_and_halves_after_a_slow_one() {
    let mut now = Instant::now();
    let size = 64 * 1024;
    let info = FileInfo { name: "f".into(), size, mode: None, mtime: None };
    let mut far = Transfer::far_receive(ID, now);
    let mut t = Transfer::near();
    t.feed(&far.poll_output(now), now);
    t.send_file(info, now);
    far.feed(&t.poll_output(now), now);
    t.feed(&far.poll_output(now), now);
    far.feed(&t.poll_output(now), now);
    far.take();
    let mut events = t.feed(&far.poll_output(now), now);
    let mut chunks = Vec::new();
    // The time of each ack: three fast, one between, two slow.
    for ms in [10, 10, 10, 600, 2200, 2200] {
        let Some(Event::NeedData { offset, max }) = events.pop() else { panic!("{events:?}") };
        t.supply(&vec![b'x'; max.min((size - offset) as usize)]);
        far.feed(&t.poll_output(now), now);
        now += Duration::from_millis(ms);
        events = t.feed(&far.poll_output(now), now);
        chunks.push(t.chunk().expect("a session"));
    }
    let k = CHUNK_MIN;
    assert_eq!(chunks, [2 * k, 4 * k, 8 * k, 8 * k, 4 * k, 2 * k]);
}

#[test]
fn a_far_end_answers_a_helo_that_comes_again() {
    let now = Instant::now();
    let mut send = Transfer::far_send(a_txt(), ID, now);
    send.poll_output(now);
    send.feed(&near(HELO), now);
    assert_eq!(text(&send.poll_output(now)), text(&[far(HELO), far(OFFR)].concat()));
    assert_eq!(send.feed(&near(HELO), now), []);
    assert_eq!(text(&send.poll_output(now)), text(&far(HELO)));
}

fn shown(events: &[Event]) -> Vec<u8> {
    events.iter().flat_map(|e| if let Event::Show(b) = e { b.clone() } else { Vec::new() }).collect()
}

/// The near end of the download after its `KEPT`, handed over at the time.
fn download_kept() -> (Transfer, Instant) {
    let (mut t, now) = download(7);
    assert!(matches!(t.feed(&far(DONE), now).last(), Some(Event::Verified { .. })));
    t.kept();
    assert_eq!(text(&t.poll_output(now)), text(&near(KEPT)));
    assert_eq!(ended(&t.events()), Some(Ended::Done));
    (t, now)
}

#[test]
fn x13_a_kept_lost_in_a_download_goes_again_and_the_done_does_not_show() {
    let (mut t, now) = download_kept();
    assert!(t.lingers() && !t.active());
    let later = now + Duration::from_secs(2);
    assert_eq!(t.feed(&far(DONE), later), []);
    assert_eq!(text(&t.poll_output(later)), text(&near(KEPT)));
    // A frame cut by a read, and one that the terminal repaints.
    let done = far(DONE);
    assert_eq!(t.feed(&done[..20], later), []);
    assert_eq!(t.feed(&done[20..], later), []);
    let repainted = format!("\r\x1b[K{}\x1b[2;1H{}\r\n", &DONE[..40], &DONE[40..]);
    assert_eq!(shown(&t.feed(repainted.as_bytes(), later)), b"\r\x1b[K");
    assert_eq!(text(&t.poll_output(later)), text(&[near(KEPT), near(KEPT)].concat()));
    // Other output shows, also a line that starts with `#`.
    assert_eq!(shown(&t.feed(b"$ #DONE: no frame\r\n", later)), b"$ #DONE: no frame\r\n");
    assert_eq!(t.poll_output(later), b"");
}

#[test]
fn x14_a_kept_lost_in_an_upload_the_far_end_lingers_and_sends_it_again() {
    let now = Instant::now();
    let mut r = far_receiver(now, true);
    assert!(r.lingers() && !r.active());
    assert_eq!(r.deadline(), Some(now + LINGER));
    let later = now + Duration::from_secs(5);
    assert_eq!(r.feed(&near(DONE), later), []);
    assert_eq!(text(&r.poll_output(later)), text(&far(KEPT)));
    // Each other frame and byte is dropped: the far program ended.
    assert_eq!(r.feed(&[b"ls\r".as_slice(), &near(DATA), &near(HELO)].concat(), later), []);
    assert_eq!(r.poll_output(later), b"");
    assert_eq!(r.tick(now + LINGER - Duration::from_millis(1)), []);
    assert!(r.lingers());
    assert_eq!(r.tick(now + LINGER), []);
    assert!(!r.lingers());
    assert_eq!(r.deadline(), None);
    assert_eq!(r.feed(&near(DONE), now + LINGER), []);
    assert_eq!(r.poll_output(now + LINGER), b"");

    // The near sender sends DONE again, and the second KEPT ends it.
    let (mut t, now) = upload(6);
    t.feed(&far(ACKN), now);
    assert_eq!(text(&t.poll_output(now)), text(&near(DONE)));
    t.tick(now + RESEND_MIN);
    assert_eq!(text(&t.poll_output(now + RESEND_MIN)), text(&near(DONE)));
    assert_eq!(ended(&t.feed(&far(KEPT), now + RESEND_MIN)), Some(Ended::Done));
}

#[test]
fn x15_a_done_after_the_linger_shows_as_text() {
    let (mut t, now) = download_kept();
    assert_eq!(t.deadline(), Some(now + LINGER));
    let late = now + LINGER;
    assert_eq!(shown(&t.feed(&far(DONE), late)), far(DONE));
    assert_eq!(t.poll_output(late), b"");
    assert!(!t.lingers());
}

#[test]
fn x16_a_done_of_another_sha256_gets_no_kept() {
    let (mut t, now) = download_kept();
    assert_eq!(shown(&t.feed(&far(DONE_ZEROS), now)), far(DONE_ZEROS));
    assert_eq!(t.poll_output(now), b"");
    let mut r = far_receiver(now, true);
    assert_eq!(r.feed(&near(DONE_ZEROS), now), []);
    assert_eq!(r.poll_output(now), b"");
    // A new trigger clears the verdict.
    let new = b"#PODSSH-TX2:S:fedcba9876543210:756e6da1\r\n";
    assert!(matches!(&t.feed(new, now)[..], [Event::Start { .. }]));
    t.refuse("refused here");
    t.poll_output(now);
    assert!(!t.lingers());
    assert_eq!(shown(&t.feed(&far(DONE), now)), far(DONE));
}

/// The sender at `at`: a `HOLD` comes, and its frame in flight goes again
/// `RESEND_MAX` after it, for 12 rounds (a minute).
fn hold_a_minute(t: &mut Transfer, frame: &str, mut at: Instant) -> Instant {
    for _ in 0..12 {
        at += Duration::from_millis(100);
        assert_eq!(t.feed(&far(HOLD), at), []);
        assert_eq!(t.deadline(), Some(at + RESEND_MAX));
        assert_eq!(t.tick(at + RESEND_MAX - Duration::from_millis(1)), []);
        at += RESEND_MAX;
        assert_eq!(t.tick(at), []);
        assert_eq!(text(&t.poll_output(at)), text(&near(frame)));
    }
    at
}

#[test]
fn x17_a_person_who_answers_an_offer_after_60_s_gets_the_file() {
    let (mut t, now) = download(4);
    for s in 1..=12 {
        let at = now + RESEND_MAX * s;
        assert_eq!(t.feed(&far(OFFR), at), [], "the OFFR {s} is not a second offer");
        assert_eq!(text(&t.poll_output(at)), text(&near(HOLD)));
        assert_eq!(t.tick(at), []);
    }
    t.take();
    assert_eq!(text(&t.poll_output(now + RESEND_MAX * 12)), text(&near(TAKE)));

    let (mut t, now) = upload(4);
    assert_eq!(t.deadline(), Some(now + RESEND_MIN));
    let at = hold_a_minute(&mut t, OFFR, now);
    assert_eq!(t.feed(&far(TAKE), at), [Event::NeedData { offset: 0, max: 1024 }]);
}

#[test]
fn x18_a_rename_that_takes_60_s_ends_with_done_at_the_sender() {
    let (mut t, now) = download(7);
    assert!(matches!(t.feed(&far(DONE), now).last(), Some(Event::Verified { .. })));
    for s in 1..=12 {
        let at = now + RESEND_MAX * s;
        assert_eq!(t.feed(&far(DONE), at), [], "the DONE {s} is not verified again");
        assert_eq!(text(&t.poll_output(at)), text(&near(HOLD)));
    }
    t.kept();
    assert_eq!(text(&t.poll_output(now + RESEND_MAX * 12)), text(&near(KEPT)));

    let (mut t, now) = upload(6);
    t.feed(&far(ACKN), now);
    assert_eq!(text(&t.poll_output(now)), text(&near(DONE)));
    let at = hold_a_minute(&mut t, DONE, now);
    assert_eq!(ended(&t.feed(&far(KEPT), at)), Some(Ended::Done));
}

#[test]
fn x19_no_frame_after_done_for_20_s_ends_unconfirmed_with_no_stop() {
    let (mut t, now) = upload(6);
    t.feed(&far(ACKN), now);
    let mut sent = t.poll_output(now);
    for s in 1..20 {
        let at = now + Duration::from_secs(s);
        assert_eq!(t.tick(at), []);
        sent.extend(t.poll_output(at));
    }
    assert_eq!(ended(&t.tick(now + WAIT)), Some(Ended::Unconfirmed));
    sent.extend(t.poll_output(now + WAIT));
    assert!(text(&sent).starts_with(DONE) && !text(&sent).contains("#STOP:"), "{}", text(&sent));
    assert!(!t.active() && !t.lingers());
}

#[test]
fn x20_a_peer_of_version_1_gets_unsupported() {
    let now = Instant::now();
    // The trigger of a far end of version 1 starts nothing here.
    let v1 = b"#PODSSH-TX1:S:0123456789abcdef:56fd5108\r\n";
    let mut t = Transfer::near();
    let events = t.feed(v1, now);
    assert_eq!((shown(&events), events.contains(&Event::Unsupported { version: 1 })), (v1.to_vec(), true));
    assert!(!t.active());
    assert_eq!(t.poll_output(now), b"");
    // The HELO of version 1 stops the far end with the code UNSUPPORTED.
    let mut send = Transfer::far_send(a_txt(), ID, now);
    send.poll_output(now);
    let events = send.feed(&near("#HELO:AQEjRWeJq83vAACAAA!21941b5b"), now);
    assert_eq!(code(&events), Some(stop::UNSUPPORTED));
    let stop = "#STOP:BwArSEVMTyBvZiB2ZXJzaW9uIDE7IHRoaXMgZW5kIHRha2VzIHZlcnNpb24gMg!cb0e9a21";
    assert_eq!(text(&send.poll_output(now)), text(&far(stop)));
}

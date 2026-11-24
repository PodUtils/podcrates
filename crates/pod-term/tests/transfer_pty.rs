//! Whole transfers of the codec through a stand-in for a pty and a
//! terminal that rewrite bytes: CR to LF, LF to CRLF, a dropped ZDLE (CAN),
//! a repaint with cursor moves at each wrap, each read painted twice, and
//! lost frames. The two ends run with a clock of the test, and a caller
//! that answers at once or after a time.

#![cfg(feature = "transfer")]

use std::time::{Duration, Instant};

use pod_term::transfer::{CHUNK_MAX, CHUNK_MIN, Dir, Ended, Event, FileInfo, Transfer};
use proptest::prelude::*;

const ID: [u8; 8] = [7, 6, 5, 4, 3, 2, 1, 0];
const STEP: Duration = Duration::from_millis(5);

/// What sits between the two ends in one direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pty {
    Clean,
    /// A far pty that maps CR to LF (ICRNL).
    CrToLf,
    /// A pty that maps LF to CRLF (ONLCR).
    LfToCrlf,
    /// tmux: it drops CAN, and paints a line of 80 columns with a cursor
    /// move at each wrap and an erase at each CR.
    Tmux,
    /// Each read painted again.
    Twice,
    /// Each third read that holds `DATA` lost.
    Lossy,
    /// The first read that holds `KEPT` lost.
    LoseKept,
    /// Each read lost that the mask of the path marks.
    Mask,
}

/// The state of a stand-in.
struct Path {
    pty: Pty,
    col: usize,
    row: usize,
    data_reads: usize,
    kept_lost: bool,
    mask: Vec<bool>,
    reads: usize,
}

impl Path {
    fn new(pty: Pty) -> Path {
        Path { pty, col: 0, row: 1, data_reads: 0, kept_lost: false, mask: Vec::new(), reads: 0 }
    }

    fn masked(mask: Vec<bool>) -> Path {
        Path { mask, ..Path::new(Pty::Mask) }
    }

    fn pass(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.reads += 1;
        match self.pty {
            Pty::Clean => bytes.to_vec(),
            Pty::CrToLf => bytes.iter().map(|&b| if b == b'\r' { b'\n' } else { b }).collect(),
            Pty::LfToCrlf => {
                bytes.iter().flat_map(|&b| if b == b'\n' { vec![b'\r', b'\n'] } else { vec![b] }).collect()
            }
            Pty::Twice => [bytes, bytes].concat(),
            Pty::Lossy => {
                let data = bytes.windows(6).any(|w| w == b"#DATA:");
                self.data_reads += usize::from(data);
                if data && self.data_reads.is_multiple_of(3) { Vec::new() } else { bytes.to_vec() }
            }
            Pty::LoseKept => {
                let kept = !self.kept_lost && holds(bytes, b"#KEPT:") > 0;
                self.kept_lost |= kept;
                if kept { Vec::new() } else { bytes.to_vec() }
            }
            Pty::Mask => {
                if self.mask.get(self.reads - 1) == Some(&true) {
                    Vec::new()
                } else {
                    bytes.to_vec()
                }
            }
            Pty::Tmux => {
                let mut out = Vec::new();
                for &b in bytes.iter().filter(|&&b| b != 0x18) {
                    match b {
                        b'\r' => {
                            self.col = 0;
                            out.extend_from_slice(b"\r\x1b[K");
                        }
                        b'\n' => {
                            self.row = self.row % 24 + 1;
                            out.push(b);
                        }
                        _ => {
                            if self.col == 80 {
                                self.col = 0;
                                self.row = self.row % 24 + 1;
                                out.extend_from_slice(format!("\x1b[{};1H", self.row).as_bytes());
                            }
                            self.col += 1;
                            out.push(b);
                        }
                    }
                }
                out
            }
        }
    }
}

/// The count of `frame` in `bytes`.
fn holds(bytes: &[u8], frame: &[u8]) -> usize {
    bytes.windows(frame.len()).filter(|w| *w == frame).count()
}

/// When the caller of the receiver takes the offered file, and renames the
/// copy, after the event.
#[derive(Debug, Clone, Copy, Default)]
struct Caller {
    answer: Duration,
    rename: Duration,
}

/// One end and its caller.
struct End {
    t: Transfer,
    ended: Option<Ended>,
    caller: Caller,
    take_at: Option<Instant>,
    kept_at: Option<Instant>,
    shown: Vec<u8>,
}

impl End {
    fn new(t: Transfer, caller: Caller) -> End {
        End { t, ended: None, caller, take_at: None, kept_at: None, shown: Vec::new() }
    }

    /// The acts of the caller that are due.
    fn due(&mut self, now: Instant) -> Vec<Event> {
        if self.take_at.take_if(|t| *t <= now).is_some() {
            self.t.take();
        }
        if self.kept_at.take_if(|t| *t <= now).is_some() {
            self.t.kept();
        }
        self.t.events()
    }
}

/// What a run gave.
struct Run {
    received: Vec<u8>,
    near: Option<Ended>,
    far: Option<Ended>,
    largest_chunk: usize,
    /// The `HOLD` frames of the receiver.
    holds: usize,
    /// What the near end showed.
    shown: Vec<u8>,
}

fn info(file: &[u8]) -> FileInfo {
    FileInfo { name: "dir/f.bin".into(), size: file.len() as u64, mode: Some(0o600), mtime: Some(1) }
}

/// The caller of an end acts on its events, and on the events of its acts.
fn act(end: &mut End, mut events: Vec<Event>, file: &[u8], received: &mut Vec<u8>, now: Instant) {
    while !events.is_empty() {
        for event in events {
            match event {
                Event::Start { dir: Dir::Send, .. } => end.t.accept(now),
                Event::Start { dir: Dir::Receive, .. } => end.t.send_file(info(file), now),
                Event::Offer { name, size, .. } => {
                    assert_eq!((name.as_str(), size), ("f.bin", file.len() as u64));
                    end.take_at = Some(now + end.caller.answer);
                }
                Event::NeedData { offset, max } => {
                    let from = offset as usize;
                    end.t.supply(&file[from..(from + max).min(file.len())]);
                }
                Event::Write { offset, bytes } => {
                    assert_eq!(offset, received.len() as u64, "a write out of order");
                    received.extend_from_slice(&bytes);
                }
                Event::Verified { .. } => end.kept_at = Some(now + end.caller.rename),
                Event::End { ended, .. } => end.ended = Some(ended),
                Event::Show(bytes) => end.shown.extend_from_slice(&bytes),
                _ => {}
            }
        }
        events = end.due(now);
    }
}

fn run(dir: Dir, file: &[u8], down: Pty, up: Pty) -> Run {
    run_with(dir, file, Path::new(down), Path::new(up), Caller::default(), STEP)
}

fn run_with(dir: Dir, file: &[u8], mut down: Path, mut up: Path, caller: Caller, step: Duration) -> Run {
    let mut now = Instant::now();
    let far = match dir {
        Dir::Send => Transfer::far_send(info(file), ID, now),
        Dir::Receive => Transfer::far_receive(ID, now),
    };
    let (mut near, mut far) = (End::new(Transfer::near(), caller), End::new(far, caller));
    let mut received = Vec::new();
    let mut largest_chunk = 0;
    let mut holds_sent = 0;
    for _ in 0..200_000 {
        now += step;
        for end in [&mut near, &mut far] {
            let events = end.due(now);
            act(end, events, file, &mut received, now);
        }
        let bytes = far.t.poll_output(now);
        holds_sent += holds(&bytes, b"#HOLD:");
        if !bytes.is_empty() {
            let events = near.t.feed(&down.pass(&bytes), now);
            act(&mut near, events, file, &mut received, now);
        }
        let bytes = near.t.poll_output(now);
        holds_sent += holds(&bytes, b"#HOLD:");
        if !bytes.is_empty() {
            let events = far.t.feed(&up.pass(&bytes), now);
            act(&mut far, events, file, &mut received, now);
        }
        for end in [&mut near, &mut far] {
            if end.t.deadline().is_some_and(|d| d <= now) {
                let events = end.t.tick(now);
                act(end, events, file, &mut received, now);
            }
            largest_chunk = largest_chunk.max(end.t.chunk().unwrap_or(0));
        }
        if near.ended.is_some() && far.ended.is_some() {
            break;
        }
    }
    Run { received, near: near.ended, far: far.ended, largest_chunk, holds: holds_sent, shown: near.shown }
}

/// Bytes of each value, in an order with no pattern.
fn file(len: usize) -> Vec<u8> {
    let mut x = 0x2545_f491_u32;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn check(dir: Dir, len: usize, down: Pty, up: Pty) -> Run {
    let file = file(len);
    let r = run(dir, &file, down, up);
    let what = format!("{dir:?} of {len} bytes, {down:?} down, {up:?} up");
    assert_eq!((&r.near, &r.far), (&Some(Ended::Done), &Some(Ended::Done)), "{what}");
    assert!(r.received == file, "{what}: {} bytes received", r.received.len());
    r
}

#[test]
fn a_transfer_through_a_clean_path_grows_its_chunk_to_the_largest() {
    for dir in [Dir::Send, Dir::Receive] {
        let r = check(dir, 300_000, Pty::Clean, Pty::Clean);
        assert_eq!(r.largest_chunk, CHUNK_MAX, "{dir:?}");
    }
}

#[test]
fn a_transfer_survives_a_pty_that_rewrites_bytes() {
    for dir in [Dir::Send, Dir::Receive] {
        for (down, up) in [(Pty::Tmux, Pty::CrToLf), (Pty::LfToCrlf, Pty::CrToLf), (Pty::Twice, Pty::Twice)] {
            check(dir, 70_000, down, up);
        }
    }
}

#[test]
fn a_transfer_survives_lost_frames() {
    for dir in [Dir::Send, Dir::Receive] {
        let (down, up) = if dir == Dir::Send { (Pty::Lossy, Pty::Clean) } else { (Pty::Clean, Pty::Lossy) };
        let r = check(dir, 40_000, down, up);
        assert!(r.largest_chunk >= CHUNK_MIN, "{dir:?}");
    }
}

#[test]
fn an_empty_file_goes_with_no_data() {
    for dir in [Dir::Send, Dir::Receive] {
        check(dir, 0, Pty::Clean, Pty::Clean);
    }
}

#[test]
fn a_transfer_whose_first_kept_is_lost_ends_with_done_at_both_ends() {
    for dir in [Dir::Send, Dir::Receive] {
        // The receiver's KEPT goes up in a download, and down in an upload.
        let (down, up) = if dir == Dir::Send { (Pty::Clean, Pty::LoseKept) } else { (Pty::LoseKept, Pty::Clean) };
        let r = check(dir, 5_000, down, up);
        assert_eq!(holds(&r.shown, b"#DONE:"), 0, "{dir:?}: {}", String::from_utf8_lossy(&r.shown));
    }
}

#[test]
fn x17_x18_a_person_who_takes_a_minute_gets_the_file() {
    let minute = Duration::from_secs(60);
    for caller in [Caller { answer: minute, rename: Duration::ZERO }, Caller { answer: Duration::ZERO, rename: minute }]
    {
        for dir in [Dir::Send, Dir::Receive] {
            let file = file(3_000);
            let r = run_with(dir, &file, Path::new(Pty::Clean), Path::new(Pty::Clean), caller, STEP);
            let what = format!("{dir:?}, {caller:?}");
            assert_eq!((&r.near, &r.far), (&Some(Ended::Done), &Some(Ended::Done)), "{what}");
            assert!(r.received == file, "{what}");
            // The first OFFR or DONE goes again after 1 s, then each 5 s.
            assert!((11..=13).contains(&r.holds), "{what}: {} HOLD frames", r.holds);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    /// For any loss of the reads of one session, the sender ends with
    /// `Done` or `Unconfirmed`, never with a stop, when the receiver kept
    /// the file; and `Done` only when the receiver kept it.
    #[test]
    fn a_sender_never_reports_a_stop_for_a_file_that_the_receiver_kept(
        send in any::<bool>(),
        down in proptest::collection::vec(prop::bool::weighted(0.3), 0..40),
        up in proptest::collection::vec(prop::bool::weighted(0.3), 0..40),
    ) {
        let dir = if send { Dir::Send } else { Dir::Receive };
        let file = file(2_500);
        let r = run_with(dir, &file, Path::masked(down), Path::masked(up), Caller::default(), STEP * 10);
        let (sender, receiver) = if send { (&r.far, &r.near) } else { (&r.near, &r.far) };
        let kept = receiver == &Some(Ended::Done);
        if kept {
            prop_assert!(r.received == file);
            prop_assert!(matches!(sender, Some(Ended::Done | Ended::Unconfirmed)), "{dir:?}: the sender {sender:?}");
        }
        if sender == &Some(Ended::Done) {
            prop_assert!(kept, "{dir:?}: the receiver {receiver:?}");
        }
    }
}

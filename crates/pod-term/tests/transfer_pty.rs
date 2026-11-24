//! Whole transfers of the codec through a stand-in for a pty and a
//! terminal that rewrite bytes: CR to LF, LF to CRLF, a dropped ZDLE (CAN),
//! a repaint with cursor moves at each wrap, each read painted twice, and
//! lost frames. The two ends run with a clock of the test.

#![cfg(feature = "transfer")]

use std::time::{Duration, Instant};

use pod_term::transfer::{CHUNK_MAX, CHUNK_MIN, Dir, Ended, Event, FileInfo, Transfer};

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
}

/// The state of a stand-in.
struct Path {
    pty: Pty,
    col: usize,
    row: usize,
    data_reads: usize,
}

impl Path {
    fn new(pty: Pty) -> Path {
        Path { pty, col: 0, row: 1, data_reads: 0 }
    }

    fn pass(&mut self, bytes: &[u8]) -> Vec<u8> {
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

/// One end and its caller.
struct End {
    t: Transfer,
    ended: Option<Ended>,
}

/// What a run gave.
struct Run {
    received: Vec<u8>,
    near: Option<Ended>,
    far: Option<Ended>,
    largest_chunk: usize,
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
                    end.t.take();
                }
                Event::NeedData { offset, max } => {
                    let from = offset as usize;
                    end.t.supply(&file[from..(from + max).min(file.len())]);
                }
                Event::Write { offset, bytes } => {
                    assert_eq!(offset, received.len() as u64, "a write out of order");
                    received.extend_from_slice(&bytes);
                }
                Event::Verified { .. } => end.t.kept(),
                Event::End { ended, .. } => end.ended = Some(ended),
                _ => {}
            }
        }
        events = end.t.events();
    }
}

fn run(dir: Dir, file: &[u8], down: Pty, up: Pty) -> Run {
    let mut now = Instant::now();
    let far = match dir {
        Dir::Send => Transfer::far_send(info(file), ID, now),
        Dir::Receive => Transfer::far_receive(ID, now),
    };
    let (mut near, mut far) = (End { t: Transfer::near(), ended: None }, End { t: far, ended: None });
    let (mut down, mut up) = (Path::new(down), Path::new(up));
    let mut received = Vec::new();
    let mut largest_chunk = 0;
    for _ in 0..200_000 {
        now += STEP;
        let bytes = far.t.poll_output(now);
        if !bytes.is_empty() {
            let events = near.t.feed(&down.pass(&bytes), now);
            act(&mut near, events, file, &mut received, now);
        }
        let bytes = near.t.poll_output(now);
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
    Run { received, near: near.ended, far: far.ended, largest_chunk }
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

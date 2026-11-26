//! Whole transfers of the codec through a model of a tmux pane that falls
//! behind: of each read, it paints only the last screen, so the rows that
//! scroll off before it are lost (PodUtils/podssh#358). The far end knows
//! its pane ([`Transfer::set_size`]) and keeps each frame within it; the
//! near end learns the cap from `HELO`.

#![cfg(feature = "transfer")]

use std::time::{Duration, Instant};

use pod_term::transfer::{CHUNK_FLOOR, CHUNK_MAX, Dir, Ended, Event, FileInfo, SPARE_ROWS, Transfer};

const ID: [u8; 8] = [9, 8, 7, 6, 5, 4, 3, 2];
const STEP: Duration = Duration::from_millis(5);
/// The panes of the ask: tmux at 120 columns, and podssh's narrowest.
const PANES: [(usize, usize); 2] = [(120, 23), (48, 23)];

/// A pane of tmux whose client falls behind at each read.
struct Pane {
    cols: usize,
    rows: usize,
    /// The most rows that one read painted.
    tallest: usize,
    /// The lines with a head `#DATA:` that scrolled off.
    cut: usize,
}

impl Pane {
    fn new((cols, rows): (usize, usize)) -> Pane {
        Pane { cols, rows, tallest: 0, cut: 0 }
    }

    /// The rows of one line: its bytes other than CR and LF, wrapped at the
    /// columns.
    fn rows_of(&self, line: &[u8]) -> usize {
        let shown = line.iter().filter(|&&b| b != b'\r' && b != b'\n').count();
        shown.div_ceil(self.cols).max(1)
    }

    /// The bytes of the last screen of `bytes`: the rows above the row of
    /// the cursor are `rows - 1`.
    fn paint(&mut self, bytes: &[u8]) -> Vec<u8> {
        let lines: Vec<&[u8]> = bytes.split_inclusive(|&b| b == b'\n').collect();
        self.tallest = self.tallest.max(lines.iter().map(|l| self.rows_of(l)).sum());
        let mut room = self.rows - 1;
        let mut kept: Vec<&[u8]> = Vec::new();
        for line in lines.into_iter().rev() {
            let rows = self.rows_of(line);
            if rows <= room {
                room -= rows;
                kept.push(line);
                continue;
            }
            // The rows of the line that are left on the screen: its tail.
            let tail = line.len().min(room * self.cols + 2);
            kept.push(&line[line.len() - tail..]);
            self.cut += usize::from(line.windows(6).any(|w| w == b"#DATA:"));
            room = 0;
        }
        kept.into_iter().rev().flatten().copied().collect()
    }
}

/// Bytes of each value, in an order with no pattern.
fn file(len: usize) -> Vec<u8> {
    let mut x = 0x9e37_79b9_u32;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn info(file: &[u8]) -> FileInfo {
    FileInfo { name: "f.bin".into(), size: file.len() as u64, mode: None, mtime: None }
}

/// One end of a run, and what its caller saw.
struct End {
    t: Transfer,
    ended: Option<Ended>,
    largest_max: usize,
}

/// The caller of an end acts on its events at once.
fn act(end: &mut End, mut events: Vec<Event>, file: &[u8], received: &mut Vec<u8>, now: Instant) {
    while !events.is_empty() {
        for event in events {
            match event {
                Event::Start { dir: Dir::Send, .. } => end.t.accept(now),
                Event::Start { dir: Dir::Receive, .. } => end.t.send_file(info(file), now),
                Event::Offer { .. } => end.t.take(),
                Event::NeedData { offset, max } => {
                    end.largest_max = end.largest_max.max(max);
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

/// What a run gave.
struct Run {
    near: End,
    far: End,
    pane: Pane,
    received: Vec<u8>,
    /// The time of the resize, when the run had one.
    resized: Option<Instant>,
}

/// A transfer of `file`; the far end's output goes through `pane`. With
/// `shrink`, the pane takes those columns after the first `DATA` at the
/// largest chunk, and the far end learns it 50 ms later.
fn run(dir: Dir, file: &[u8], size: (usize, usize), shrink: Option<usize>) -> Run {
    let mut now = Instant::now();
    let mut far = match dir {
        Dir::Send => Transfer::far_send(info(file), ID, now),
        Dir::Receive => Transfer::far_receive(ID, now),
    };
    far.set_size(size.0, size.1);
    let cap = Transfer::pane_chunk(size.0, size.1);
    let mut r = Run {
        near: End { t: Transfer::near(), ended: None, largest_max: 0 },
        far: End { t: far, ended: None, largest_max: 0 },
        pane: Pane::new(size),
        received: Vec::new(),
        resized: None,
    };
    for _ in 0..100_000 {
        now += STEP;
        let bytes = r.far.t.poll_output(now);
        if let Some(cols) = shrink.filter(|_| r.resized.is_none() && r.far.largest_max == cap) {
            r.pane.cols = cols;
            r.resized = Some(now);
        }
        if r.resized.is_some_and(|at| at + Duration::from_millis(50) == now) {
            r.far.t.set_size(r.pane.cols, r.pane.rows);
        }
        if !bytes.is_empty() {
            let events = r.near.t.feed(&r.pane.paint(&bytes), now);
            act(&mut r.near, events, file, &mut r.received, now);
        }
        let bytes = r.near.t.poll_output(now);
        if !bytes.is_empty() {
            let events = r.far.t.feed(&bytes, now);
            act(&mut r.far, events, file, &mut r.received, now);
        }
        for end in [&mut r.near, &mut r.far] {
            if end.t.deadline().is_some_and(|d| d <= now) {
                let events = end.t.tick(now);
                act(end, events, file, &mut r.received, now);
            }
        }
        if r.near.ended.is_some() && r.far.ended.is_some() {
            break;
        }
    }
    r
}

fn done(r: &Run, file: &[u8], what: &str) {
    assert_eq!((&r.near.ended, &r.far.ended), (&Some(Ended::Done), &Some(Ended::Done)), "{what}");
    assert!(r.received == file, "{what}: {} bytes received", r.received.len());
}

#[test]
fn the_cap_of_a_pane_keeps_a_data_frame_within_its_rows_with_a_floor() {
    assert_eq!(Transfer::pane_chunk(120, 23), 1855);
    assert_eq!(Transfer::pane_chunk(48, 23), 721);
    // A pane that is short or narrow: the floor, not a chunk of nothing.
    assert_eq!(Transfer::pane_chunk(40, 10), CHUNK_FLOOR);
    assert_eq!(Transfer::pane_chunk(48, 3), CHUNK_FLOOR);
    // A size that is not known, and a tall pane: no cap past CHUNK_MAX.
    assert_eq!(Transfer::pane_chunk(0, 0), CHUNK_MAX);
    assert_eq!(Transfer::pane_chunk(400, 300), CHUNK_MAX);
}

#[test]
fn a_download_through_a_pane_keeps_each_read_within_its_rows() {
    let file = file(40_000);
    for size in PANES {
        let r = run(Dir::Send, &file, size, None);
        let what = format!("a pane of {} by {}", size.1, size.0);
        done(&r, &file, &what);
        assert!(r.pane.tallest <= size.1 - SPARE_ROWS, "{what}: a read of {} rows", r.pane.tallest);
        assert_eq!(r.pane.cut, 0, "{what}");
        // The chunk grew to the cap of the pane and not past it.
        assert_eq!(r.far.largest_max, Transfer::pane_chunk(size.0, size.1), "{what}");
    }
}

#[test]
fn the_cap_of_the_far_end_in_helo_lowers_the_chunk_of_the_near_end() {
    let file = file(20_000);
    for size in PANES {
        let r = run(Dir::Receive, &file, size, None);
        let what = format!("an upload, the far pane {} by {}", size.1, size.0);
        done(&r, &file, &what);
        let cap = Transfer::pane_chunk(size.0, size.1);
        assert_eq!(r.near.largest_max, cap, "{what}");
        assert!(r.pane.tallest <= size.1 - SPARE_ROWS, "{what}: a read of {} rows", r.pane.tallest);
    }
}

#[test]
fn a_data_cut_by_a_narrower_pane_goes_again_at_the_floor_and_arrives() {
    let file = file(30_000);
    let r = run(Dir::Send, &file, PANES[0], Some(PANES[1].0));
    assert!(r.resized.is_some(), "the chunk did not reach the cap");
    assert!(r.pane.cut > 0, "the narrower pane cut no frame");
    done(&r, &file, "a pane that shrinks from 120 to 48 columns");
}

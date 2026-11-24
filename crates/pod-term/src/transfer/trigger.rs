//! The trigger line, `#PODSSH-TX<version>:<dir>:<id>:<crc>`: the far end
//! paints it, and the near end scans its output for it.

use std::collections::VecDeque;
use std::time::Instant;

use super::{Dir, Event, IDS_KEPT, TRIGGER_HOLD, VERSION};

const PREFIX: &[u8] = b"#PODSSH-TX";

/// The trigger line that the far end paints: a clean line first, then the
/// trigger and CRLF.
pub(super) fn line(dir: Dir, id: [u8; 8]) -> Vec<u8> {
    let mut t = format!("#PODSSH-TX{VERSION}:{}:", dir_char(dir) as char).into_bytes();
    t.extend_from_slice(hex(&id).as_bytes());
    let crc = crc32fast::hash(&t);
    let mut out = b"\r\x1b[K".to_vec();
    out.extend_from_slice(&t);
    out.extend_from_slice(format!(":{crc:08x}\r\n").as_bytes());
    out
}

fn dir_char(dir: Dir) -> u8 {
    match dir {
        Dir::Send => b'S',
        Dir::Receive => b'R',
    }
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What the bytes held so far are.
enum Held {
    /// The start of a trigger.
    Part,
    /// A whole trigger: its version, its direction, its id, and whether its
    /// CRC holds.
    Whole { version: u32, dir: Dir, id: [u8; 8], crc_holds: bool },
    /// No trigger.
    Not,
}

/// The bytes after the version: `:`, the direction, `:`, 16 lowercase hex
/// digits of the id, `:`, 8 hex digits of the CRC.
const SHAPE: usize = 28;

/// Whether `b` can be the byte `i` after the version.
fn fits(i: usize, b: u8) -> bool {
    match i {
        0 | 2 | 19 => b == b':',
        1 => b == b'S' || b == b'R',
        3..=18 => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        _ => b.is_ascii_hexdigit(),
    }
}

/// Reads `held` as a trigger, from its `#`.
fn read(held: &[u8]) -> Held {
    let n = held.len().min(PREFIX.len());
    if held[..n] != PREFIX[..n] {
        return Held::Not;
    }
    let rest = &held[n..];
    if held.len() <= PREFIX.len() {
        return Held::Part;
    }
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits > 3 || (digits == 0 && !rest.is_empty()) {
        return Held::Not;
    }
    let Some(after) = rest.get(digits..).filter(|a| !a.is_empty()) else { return Held::Part };
    let version: u32 = rest[..digits].iter().fold(0, |v, d| v * 10 + u32::from(d - b'0'));
    if after.len() > SHAPE || after.iter().enumerate().any(|(i, &b)| !fits(i, b)) {
        return Held::Not;
    }
    if after.len() < SHAPE {
        return Held::Part;
    }
    let dir = if after[1] == b'S' { Dir::Send } else { Dir::Receive };
    let mut id = [0u8; 8];
    for (i, pair) in after[3..19].chunks(2).enumerate() {
        id[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16).unwrap_or(0);
    }
    let crc_text = std::str::from_utf8(&after[20..]).unwrap_or("");
    let crc = u32::from_str_radix(crc_text, 16).ok();
    let signed = held.len() - 9;
    Held::Whole { version, dir, id, crc_holds: crc == Some(crc32fast::hash(&held[..signed])) }
}

/// A trigger that starts a session, and the bytes of its read that it and
/// its line end took.
pub(super) struct Start {
    pub dir: Dir,
    pub id: [u8; 8],
    pub used: usize,
}

/// The scanner of the near end's output while no session runs.
#[derive(Debug, Clone, Default)]
pub(super) struct Scanner {
    held: Vec<u8>,
    since: Option<Instant>,
    seen: VecDeque<[u8; 8]>,
}

impl Scanner {
    /// Reads `bytes`: each byte that cannot begin a trigger shows at once.
    pub(super) fn feed(&mut self, bytes: &[u8], now: Instant, events: &mut Vec<Event>) -> Option<Start> {
        for (i, &b) in bytes.iter().enumerate() {
            if self.held.is_empty() {
                if b == b'#' {
                    self.held.push(b);
                    self.since = Some(now);
                } else {
                    show(events, &[b]);
                }
                continue;
            }
            self.held.push(b);
            match read(&self.held) {
                Held::Part => {}
                Held::Not => {
                    let restart = b == b'#';
                    let keep = if restart { self.held.len() - 1 } else { self.held.len() };
                    show(events, &self.held[..keep]);
                    self.held.clear();
                    if restart {
                        self.held.push(b);
                        self.since = Some(now);
                    }
                }
                Held::Whole { version, dir, id, crc_holds } => {
                    let held = std::mem::take(&mut self.held);
                    if crc_holds && version == VERSION && !self.seen.contains(&id) {
                        if self.seen.len() == IDS_KEPT {
                            self.seen.pop_front();
                        }
                        self.seen.push_back(id);
                        events.push(Event::Start { dir, id });
                        return Some(Start { dir, id, used: i + 1 + line_end(&bytes[i + 1..]) });
                    }
                    show(events, &held);
                    if crc_holds && version != VERSION {
                        events.push(Event::Unsupported { version });
                    }
                }
            }
        }
        None
    }

    /// When the bytes of a trigger cut by a read show, if none comes.
    pub(super) fn deadline(&self) -> Option<Instant> {
        self.since.filter(|_| !self.held.is_empty()).map(|t| t + TRIGGER_HOLD)
    }

    /// Shows the held bytes at their deadline.
    pub(super) fn tick(&mut self, now: Instant, events: &mut Vec<Event>) {
        if self.deadline().is_some_and(|d| now >= d) {
            show(events, &std::mem::take(&mut self.held));
        }
    }
}

/// The length of the line end (CRLF, CR or LF) at the start of `bytes`.
pub(super) fn line_end(bytes: &[u8]) -> usize {
    match bytes {
        [b'\r', b'\n', ..] => 2,
        [b'\r' | b'\n', ..] => 1,
        _ => 0,
    }
}

/// Appends output for the terminal to the last `Show`, or adds one.
pub(super) fn show(events: &mut Vec<Event>, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    match events.last_mut() {
        Some(Event::Show(shown)) => shown.extend_from_slice(bytes),
        _ => events.push(Event::Show(bytes.to_vec())),
    }
}

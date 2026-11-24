//! A 7-bit transfer of one file through a terminal that repaints its
//! output, for a pty that damages ZMODEM (tmux drops ZDLE; a far pty can
//! map CR and LF). Sans-IO: the peer's bytes, the keys and the time come
//! in; the bytes for the peer and the terminal, and events, go out. It
//! reads no file and writes none: the caller does, from the events.
//!
//! Each byte that it sends is printable ASCII, CR or LF, so a terminal
//! between the two ends paints it, and the other end reads it back from
//! the paint. The design comes from podssh (`docs/terminal.md`, "The 7-bit
//! envelope (T-831, step 2): the design for pod-term"):
//!
//! - The far end paints a trigger line, `#PODSSH-TX1:<S|R>:<id>:<crc>`. The
//!   near end ([`Transfer::near`]) scans its output for it: a trigger whose
//!   CRC holds and whose id is new gives [`Event::Start`], and its caller
//!   calls [`Transfer::accept`] (a download), [`Transfer::send_file`] (an
//!   upload) or [`Transfer::refuse`]. Each other byte shows
//!   ([`Event::Show`]); a trigger cut by a read waits [`TRIGGER_HOLD`].
//! - A frame is `#KIND:<base64>!<crc>`, the CRC-32 of `KIND`, `:` and the
//!   payload. The reader drops escape sequences, control bytes, CR and LF
//!   inside a frame, so a wrapped or repainted frame reads as one. After a
//!   byte that does not belong, it resyncs at the last `#KIND:`.
//! - One frame waits for its answer at a time. The receiver echoes the
//!   count of bytes it received in order (`ACKN`). The chunk starts at
//!   [`CHUNK_MIN`], doubles after an ack in [`FAST_ACK`] and halves after
//!   one past [`SLOW_ACK`], up to [`CHUNK_MAX`]; a resend sets it to
//!   [`CHUNK_MIN`].
//! - The file comes as a copy under a temporary name: the receiver's
//!   caller writes each [`Event::Write`], and on [`Event::Verified`] (the
//!   size and the SHA-256 hold) it renames the copy and calls
//!   [`Transfer::kept`]. At each other [`Event::End`] it removes the copy.
//! - While the receiver's caller acts (it asks a person about an offer, or
//!   it renames the copy), the receiver answers each frame of the sender
//!   that comes again with `HOLD`. The sender counts its [`WAIT`] from the
//!   last frame of any kind, and sends its frame again after
//!   [`RESEND_MAX`]. The codec sets no limit on the time of the person.
//! - The receiver keeps the verdict of the file (its size and SHA-256) for
//!   [`LINGER`] after `KEPT` ([`Transfer::lingers`]), and answers a `DONE`
//!   that comes again with the same `KEPT`: the pty can lose `KEPT`. A
//!   sender that gets no frame in [`WAIT`] after `DONE` ends with
//!   [`Ended::Unconfirmed`], never with a stop.
//!
//! This codec is the source of the frame table and the waits; podssh's
//! document follows it. A peer of another [`VERSION`] does not start: the
//! near end shows its trigger and gives [`Event::Unsupported`], and a far
//! end stops at a `HELO` of another version with [`stop::UNSUPPORTED`].
//!
//! The reader of escape sequences is the state machine of
//! [`crate::grammar`]: a string (OSC, DCS, SOS, PM, APC) is dropped up to
//! its end.

use std::time::{Duration, Instant};

mod flow;
mod linger;
mod reader;
mod trigger;
mod wire;

use flow::Session;
use linger::Verdict;
use trigger::Scanner;

/// The version of the trigger and of `HELO`. Version 2 adds `HOLD`, which
/// a peer of version 1 reads as a kind that it does not know.
pub const VERSION: u32 = 2;
/// A trigger cut by the end of a read waits this long for its rest.
pub const TRIGGER_HOLD: Duration = Duration::from_millis(100);
/// The ids of the triggers that started a session, kept so that a repaint
/// starts nothing.
pub const IDS_KEPT: usize = 32;
/// The longest frame, from `#` to its CRC, with no CR, LF or escape.
pub const FRAME_MAX: usize = 44_032;
/// The most bytes in a row with no whole frame.
pub const JUNK_MAX: usize = 64 * 1024;
/// The most damaged frames in a row.
pub const DAMAGED_MAX: usize = 8;
/// The least and the largest chunk of `DATA`.
pub const CHUNK_MIN: usize = 1024;
pub const CHUNK_MAX: usize = 32 * 1024;
/// The longest name of a file on the wire, and its last component.
pub const NAME_MAX: usize = 1024;
pub const COMPONENT_MAX: usize = 255;
/// The longest reason in `SKIP` or `STOP`.
pub const REASON_MAX: usize = 200;
/// Each wait for a valid frame from the peer; the far end's first wait.
pub const WAIT: Duration = Duration::from_secs(20);
pub const FIRST_WAIT: Duration = Duration::from_secs(120);
/// The frame in flight goes again after four times the last ack, in these
/// bounds.
pub const RESEND_MIN: Duration = Duration::from_secs(1);
pub const RESEND_MAX: Duration = Duration::from_secs(5);
/// The receiver keeps its verdict this long after the hand-over of `KEPT`:
/// the sender's last frame from the receiver came before `KEPT`, so it sends
/// `DONE` again for at most [`WAIT`] after it, and the last `DONE` needs one
/// more [`RESEND_MAX`] on the path.
pub const LINGER: Duration = WAIT.saturating_add(RESEND_MAX);
/// An ack in this time doubles the chunk; one after the slow time halves it.
pub const FAST_ACK: Duration = Duration::from_millis(500);
pub const SLOW_ACK: Duration = Duration::from_secs(2);
/// A frame breaks into lines of this many bytes at most.
pub const WRAP: usize = 1024;

/// The codes of `STOP`.
pub mod stop {
    pub const CANCELLED: u8 = 1;
    pub const OUT_OF_TIME: u8 = 2;
    /// The size or the SHA-256 differs.
    pub const DIFFERS: u8 = 3;
    pub const PROTOCOL: u8 = 4;
    /// A local error of the caller: a write, a rename.
    pub const LOCAL: u8 = 5;
    pub const REFUSED: u8 = 6;
    pub const UNSUPPORTED: u8 = 7;
}

/// The direction of a trigger, as the far end says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// `S`: the far end sends (a download).
    Send,
    /// `R`: the far end receives (an upload).
    Receive,
}

/// One file that a sender offers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    /// The low 12 bits of the mode.
    pub mode: Option<u32>,
    /// Seconds since 1970.
    pub mtime: Option<u64>,
}

/// How a session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// The receiver kept the file under its name.
    Done,
    /// A `STOP` or `SKIP`, sent or read, or five CAN from the peer.
    Stopped { code: u8, by_peer: bool, reason: String },
    /// The sender: the file and its SHA-256 went, and no frame came in
    /// [`WAIT`] after `DONE`, so the receiver's verdict is not known. No
    /// `STOP` went. Say "sent; the receiver did not confirm", not a failure.
    Unconfirmed,
}

/// What the caller acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The near end: output for the terminal, in order.
    Show(Vec<u8>),
    /// The near end: a trigger. Call `accept`, `send_file` or `refuse`.
    Start {
        dir: Dir,
        id: [u8; 8],
    },
    /// The near end: a trigger of another version; its line was shown. Say
    /// that the far end takes another version of the codec.
    Unsupported {
        version: u32,
    },
    /// The receiver: a file. Call `take` or `skip`; while the caller asks
    /// a person, the codec answers the sender with `HOLD`.
    Offer {
        name: String,
        size: u64,
        mode: Option<u32>,
        mtime: Option<u64>,
    },
    /// The sender: call `supply` with at most `max` bytes from `offset`.
    NeedData {
        offset: u64,
        max: usize,
    },
    /// The receiver: bytes of the file, in order, never twice.
    Write {
        offset: u64,
        bytes: Vec<u8>,
    },
    Progress {
        done: u64,
        size: u64,
    },
    /// A frame was dropped.
    Damaged,
    /// The receiver: the size and the SHA-256 hold. Rename the copy, then
    /// call `kept`, or `stop` with [`stop::LOCAL`]. The codec answers the
    /// sender with `HOLD` while the rename runs.
    Verified {
        size: u64,
        sha256: [u8; 32],
    },
    /// The session ended; `rest` is the output after its last frame and its
    /// line end in the same read, for the terminal.
    End {
        ended: Ended,
        rest: Vec<u8>,
    },
}

/// One end of the codec.
#[derive(Debug, Clone)]
pub struct Transfer {
    near: bool,
    scanner: Scanner,
    session: Option<Session>,
    /// Bytes of a session that ended before `poll_output`.
    out: Vec<u8>,
    events: Vec<Event>,
    width: usize,
    /// The receiver's verdict after `KEPT`.
    verdict: Option<Verdict>,
}

impl Transfer {
    fn new(near: bool, session: Option<Session>) -> Transfer {
        let (scanner, out, events) = (Scanner::default(), Vec::new(), Vec::new());
        Transfer { near, scanner, session, out, events, width: WRAP, verdict: None }
    }

    /// The near end: it scans the output of the far end for a trigger.
    pub fn near() -> Transfer {
        Transfer::new(true, None)
    }

    /// The far end that sends `offer`, with a random `id`. It paints the
    /// trigger first.
    pub fn far_send(offer: FileInfo, id: [u8; 8], now: Instant) -> Transfer {
        Transfer::far(Dir::Send, Some(offer), id, now)
    }

    /// The far end that receives a file.
    pub fn far_receive(id: [u8; 8], now: Instant) -> Transfer {
        Transfer::far(Dir::Receive, None, id, now)
    }

    fn far(dir: Dir, offer: Option<FileInfo>, id: [u8; 8], now: Instant) -> Transfer {
        let mut t = Transfer::new(false, Some(Session::far(id, offer, WRAP, now)));
        t.out = trigger::line(dir, id);
        t
    }

    /// The far end whose output is a terminal of `cols` columns breaks a
    /// frame at `cols - 1` bytes.
    pub fn set_width(&mut self, cols: usize) {
        self.width = cols.saturating_sub(1);
        if let Some(s) = &mut self.session {
            s.set_width(self.width);
        }
    }

    /// Whether a session runs.
    pub fn active(&self) -> bool {
        self.session.is_some()
    }

    /// Whether this end keeps the verdict of the file that it received: for
    /// [`LINGER`] after the hand-over of `KEPT`, it answers a `DONE` that
    /// comes again with the same `KEPT`. The far end that received a file
    /// reads its input, and calls [`Transfer::tick`] at
    /// [`Transfer::deadline`], until this is false; then its program exits.
    /// The near end does the same with its scan of the output.
    pub fn lingers(&self) -> bool {
        self.verdict.is_some()
    }

    /// The chunk of the session, for a test or a display.
    pub fn chunk(&self) -> Option<usize> {
        self.session.as_ref().map(Session::chunk)
    }

    /// The peer's output.
    pub fn feed(&mut self, bytes: &[u8], now: Instant) -> Vec<Event> {
        self.expire(now);
        let mut at = 0;
        while at < bytes.len() {
            if let Some(s) = &mut self.session {
                if let Some(used) = s.feed(&bytes[at..], now, &mut self.events) {
                    let rest = &bytes[at + used..];
                    // The line end of the last frame is no output.
                    let rest = rest[trigger::line_end(rest)..].to_vec();
                    if let Some(Event::End { rest: r, .. }) = self.events.last_mut() {
                        *r = rest;
                    }
                }
                self.finish();
                break;
            }
            if !self.near {
                if let Some(v) = &mut self.verdict {
                    v.feed(&bytes[at..], &mut self.out);
                }
                break;
            }
            let verdict = self.verdict.as_ref();
            let Some(start) = self.scanner.feed(&bytes[at..], now, &mut self.events, verdict, &mut self.out) else {
                break;
            };
            // A new session clears the verdict.
            self.verdict = None;
            let mut s = Session::near(start.dir, start.id);
            s.set_width(self.width);
            self.session = Some(s);
            at += start.used;
        }
        self.events()
    }

    /// The keys typed at the near end: what passes to the peer. While a
    /// session runs, Ctrl-C or five CAN in a row stop it, and no key passes;
    /// before the caller accepts a trigger, and outside a session, each key
    /// passes.
    pub fn keys(&mut self, keys: &[u8], _now: Instant) -> Vec<u8> {
        match &mut self.session {
            Some(s) if s.started() => {
                s.keys(keys, &mut self.events);
                self.finish();
                Vec::new()
            }
            _ => keys.to_vec(),
        }
    }

    /// The bytes for the peer.
    pub fn poll_output(&mut self, now: Instant) -> Vec<u8> {
        let mut out = std::mem::take(&mut self.out);
        if let Some(s) = &mut self.session {
            out.extend(s.poll(now));
        }
        if let Some(v) = self.verdict.as_mut().filter(|_| !out.is_empty()) {
            v.handed(now);
        }
        out
    }

    /// When to call [`Transfer::tick`].
    pub fn deadline(&self) -> Option<Instant> {
        match &self.session {
            Some(s) => s.deadline(),
            None => {
                [self.scanner.deadline(), self.verdict.as_ref().and_then(Verdict::until)].into_iter().flatten().min()
            }
        }
    }

    /// The time passed: a held trigger shows, a frame goes again, a wait
    /// ends the session, or the verdict goes at the end of [`LINGER`].
    pub fn tick(&mut self, now: Instant) -> Vec<Event> {
        self.expire(now);
        match &mut self.session {
            Some(s) => s.tick(now, &mut self.events),
            None => self.scanner.tick(now, &mut self.events),
        }
        self.finish();
        self.events()
    }

    /// The events of the calls since the last `feed`, `tick` or `events`.
    pub fn events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Accepts the trigger of a download.
    pub fn accept(&mut self, now: Instant) {
        self.act(|s, _| s.accept(now));
    }

    /// Accepts the trigger of an upload with the file to send.
    pub fn send_file(&mut self, info: FileInfo, now: Instant) {
        self.act(|s, _| s.send_file(info, now));
    }

    /// Refuses a trigger: `STOP` with [`stop::REFUSED`].
    pub fn refuse(&mut self, reason: &str) {
        self.act(|s, e| {
            if !s.started() {
                s.stop(stop::REFUSED, reason, e)
            }
        });
    }

    /// Takes the offered file.
    pub fn take(&mut self) {
        self.act(|s, _| s.take());
    }

    /// Refuses the offered file: `SKIP`.
    pub fn skip(&mut self, reason: &str) {
        self.act(|s, e| s.skip(reason, e));
    }

    /// The bytes at the offset of [`Event::NeedData`], at most its `max`.
    /// No bytes before the size is reached ends the session.
    pub fn supply(&mut self, bytes: &[u8]) {
        self.act(|s, e| s.supply(bytes, e));
    }

    /// The receiver renamed the copy: `KEPT`, and the verdict for
    /// [`LINGER`] ([`Transfer::lingers`]).
    pub fn kept(&mut self) {
        self.act(|s, e| s.kept(e));
    }

    /// Ends the session with `STOP`.
    pub fn stop(&mut self, code: u8, reason: &str) {
        self.act(|s, e| s.stop(code, reason, e));
    }

    /// The local terminal went (SIGHUP, SIGTERM, the end of stdin).
    pub fn hangup(&mut self) {
        self.stop(stop::CANCELLED, "the local terminal went");
    }

    fn act(&mut self, f: impl FnOnce(&mut Session, &mut Vec<Event>)) {
        if let Some(s) = &mut self.session {
            f(s, &mut self.events);
            self.finish();
        }
    }

    /// Drops a session that ended, and keeps its last bytes for the peer
    /// and its verdict.
    fn finish(&mut self) {
        if let Some(s) = self.session.take_if(|s| s.ended) {
            self.out.extend_from_slice(&s.out);
            self.verdict = s.verdict;
        }
    }

    /// The verdict goes at the end of [`LINGER`].
    fn expire(&mut self, now: Instant) {
        self.verdict.take_if(|v| v.over(now));
    }
}

#[cfg(test)]
mod tests;

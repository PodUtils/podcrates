//! One session of the codec: the exchange of the frames, the frame in
//! flight with its resend, the chunk, the waits and the hash.

use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

mod receiver;
mod sender;

use super::linger::Verdict;
use super::reader::{Got, Reader};
use super::wire::{self, Bad, Msg, file_name, reason_text};
use super::{
    CHUNK_MAX, CHUNK_MIN, Dir, Ended, Event, FIRST_WAIT, FileInfo, RESEND_MAX, RESEND_MIN, VERSION, WAIT, WRAP, stop,
};

const KINDS: [&[u8; 4]; 10] =
    [b"HELO", b"OFFR", b"TAKE", b"SKIP", b"DATA", b"ACKN", b"DONE", b"KEPT", b"HOLD", b"STOP"];

/// Where a session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The near end gave `Start` and waits for its caller.
    Pending,
    /// The near end sent `HELO`, or the far end waits for it.
    Hello,
    /// The receiver waits for `OFFR`.
    AwaitOffer,
    /// The receiver gave `Offer` and waits for its caller.
    Offered,
    /// The sender sent `OFFR`.
    AwaitTake,
    /// The sender gave `NeedData` and waits for its caller.
    Supply { offset: u64, max: usize },
    /// The sender sent `DATA`.
    Data,
    /// The receiver sent `TAKE` and takes `DATA` and `DONE`.
    Receiving,
    /// The receiver gave `Verified` and waits for its caller.
    Verify,
    /// The sender sent `DONE`.
    AwaitKept,
}

/// The frame that waits for its answer, sent again with the same bytes.
#[derive(Debug, Clone)]
struct Flight {
    bytes: Vec<u8>,
    /// When `poll_output` handed it over.
    handed: Option<Instant>,
    /// It went again: its ack does not time the path (Karn's rule).
    resent: bool,
    /// A `HOLD` came: the receiver is there and waits for its caller, so
    /// the frame goes again after `RESEND_MAX`.
    held: bool,
    /// The offset and the file's bytes of a `DATA`.
    offset: u64,
    data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(super) struct Session {
    id: [u8; 8],
    sender: bool,
    far: bool,
    phase: Phase,
    reader: Reader,
    file: Option<FileInfo>,
    chunk: usize,
    cap: usize,
    flight: Option<Flight>,
    last_ack: Option<Duration>,
    wait_limit: Option<Duration>,
    wait_from: Option<Instant>,
    /// The receiver: the bytes received in order. The sender: the bytes
    /// acknowledged.
    count: u64,
    hash: Sha256,
    width: usize,
    /// The receiver's `TAKE`, sent again when `OFFR` comes again.
    answer: Option<Vec<u8>>,
    typed_cans: u8,
    pub(super) out: Vec<u8>,
    pub(super) ended: bool,
    /// The receiver after `kept`: the verdict that the end keeps.
    pub(super) verdict: Option<Verdict>,
}

impl Session {
    fn new(id: [u8; 8], sender: bool, far: bool, phase: Phase) -> Session {
        Session {
            id,
            sender,
            far,
            phase,
            reader: Reader::new(),
            file: None,
            chunk: CHUNK_MIN,
            cap: CHUNK_MAX,
            flight: None,
            last_ack: None,
            wait_limit: None,
            wait_from: None,
            count: 0,
            hash: Sha256::new(),
            width: WRAP,
            answer: None,
            typed_cans: 0,
            out: Vec::new(),
            ended: false,
            verdict: None,
        }
    }

    /// The near end after a trigger: a download when the far end sends.
    pub(super) fn near(dir: Dir, id: [u8; 8]) -> Session {
        Session::new(id, dir == Dir::Receive, false, Phase::Pending)
    }

    /// The far end, which waits for the near end's `HELO`.
    pub(super) fn far(id: [u8; 8], offer: Option<FileInfo>, width: usize, now: Instant) -> Session {
        let mut s = Session::new(id, offer.is_some(), true, Phase::Hello);
        s.file = offer;
        s.width = width;
        s.wait_limit = Some(FIRST_WAIT);
        s.wait_from = Some(now);
        s
    }

    pub(super) fn set_width(&mut self, width: usize) {
        self.width = width;
    }

    pub(super) fn chunk(&self) -> usize {
        self.chunk
    }

    /// Whether the caller accepted the trigger.
    pub(super) fn started(&self) -> bool {
        self.phase != Phase::Pending
    }

    fn send(&mut self, msg: &Msg) -> Vec<u8> {
        let end: &[u8] = if self.far { b"\r\n" } else { b"\r" };
        let bytes = wire::frame(msg, end, self.width);
        self.out.extend_from_slice(&bytes);
        bytes
    }

    fn fly(&mut self, msg: &Msg) {
        let bytes = self.send(msg);
        let (offset, data) = match msg {
            Msg::Data { offset, bytes } => (*offset, bytes.clone()),
            _ => (0, Vec::new()),
        };
        self.flight = Some(Flight { bytes, handed: None, resent: false, held: false, offset, data });
    }

    /// The session waits for the peer; with no `now`, from the next
    /// hand-over of its bytes.
    fn wait_peer(&mut self, now: Option<Instant>) {
        self.wait_limit = Some(WAIT);
        self.wait_from = now;
    }

    fn wait_caller(&mut self) {
        self.wait_limit = None;
        self.wait_from = None;
    }

    fn end(&mut self, ended: Ended, events: &mut Vec<Event>) {
        self.ended = true;
        events.push(Event::End { ended, rest: Vec::new() });
    }

    pub(super) fn stop(&mut self, code: u8, reason: &str, events: &mut Vec<Event>) {
        self.send(&Msg::Stop { code, reason: reason.as_bytes().to_vec() });
        let reason = reason_text(reason.as_bytes());
        self.end(Ended::Stopped { code, by_peer: false, reason }, events);
    }

    fn protocol(&mut self, why: &str, events: &mut Vec<Event>) {
        self.stop(stop::PROTOCOL, why, events);
    }

    fn hello(&self) -> Msg {
        Msg::Helo { version: VERSION as u8, id: self.id, chunk: CHUNK_MAX as u32 }
    }

    /// The near end of a download accepts the trigger.
    pub(super) fn accept(&mut self, now: Instant) {
        if self.phase == Phase::Pending && !self.sender {
            self.fly(&self.hello());
            self.phase = Phase::Hello;
            self.wait_peer(Some(now));
        }
    }

    /// The near end of an upload accepts the trigger with its file.
    pub(super) fn send_file(&mut self, info: FileInfo, now: Instant) {
        if self.phase == Phase::Pending && self.sender {
            self.file = Some(info);
            self.fly(&self.hello());
            self.phase = Phase::Hello;
            self.wait_peer(Some(now));
        }
    }

    /// Keys typed while the session runs: Ctrl-C or five CAN stop it, and
    /// no key reaches the peer.
    pub(super) fn keys(&mut self, keys: &[u8], events: &mut Vec<Event>) {
        for &k in keys {
            match k {
                0x03 => return self.stop(stop::CANCELLED, "cancelled here (Ctrl-C)", events),
                0x18 => {
                    self.typed_cans += 1;
                    if self.typed_cans == 5 {
                        return self.stop(stop::CANCELLED, "cancelled here (CAN)", events);
                    }
                }
                _ => self.typed_cans = 0,
            }
        }
    }

    /// The peer's bytes; `Some(n)` when the session ended at the byte `n - 1`.
    pub(super) fn feed(&mut self, bytes: &[u8], now: Instant, events: &mut Vec<Event>) -> Option<usize> {
        for (i, &b) in bytes.iter().enumerate() {
            let Some(got) = self.reader.push(b) else { continue };
            match got {
                Got::Frame { kind, payload } => self.frame(&kind, &payload, now, events),
                Got::Damaged => events.push(Event::Damaged),
                Got::TooDamaged => {
                    events.push(Event::Damaged);
                    self.protocol("too many damaged frames", events);
                }
                Got::TooLong => self.protocol("a frame is too long", events),
                Got::Junk(last) => {
                    let why = format!("no frame in 64 KiB of output: {}", String::from_utf8_lossy(&last));
                    self.protocol(&why, events);
                }
                Got::Cancelled => {
                    let reason = "cancelled by the peer (CAN)".to_string();
                    self.end(Ended::Stopped { code: stop::CANCELLED, by_peer: true, reason }, events);
                }
            }
            if self.ended {
                return Some(i + 1);
            }
        }
        None
    }

    /// Whether this end takes a frame of `kind` where it is.
    fn takes(&self, kind: &[u8; 4]) -> bool {
        match (kind, self.phase) {
            (b"STOP", _) => true,
            (b"HELO", Phase::Hello) => true,
            (b"HELO", phase) => self.far && phase != Phase::Pending,
            (b"OFFR", Phase::AwaitOffer | Phase::Offered | Phase::Receiving) => !self.sender,
            (b"DATA", Phase::Receiving) => !self.sender,
            (b"DONE", Phase::Receiving | Phase::Verify) => !self.sender,
            (b"TAKE" | b"SKIP", Phase::AwaitTake) => self.sender,
            (b"HOLD", Phase::AwaitTake | Phase::AwaitKept) => self.sender,
            (b"ACKN", Phase::Data) => self.sender,
            (b"KEPT", Phase::AwaitKept) => self.sender,
            _ => false,
        }
    }

    fn frame(&mut self, kind: &[u8; 4], payload: &[u8], now: Instant, events: &mut Vec<Event>) {
        if !KINDS.contains(&kind) {
            return self.protocol("a frame of an unknown kind", events);
        }
        if !self.takes(kind) {
            // An echo, a repaint, or a frame for another state: junk.
            return;
        }
        let msg = match Msg::decode(kind, payload) {
            Ok(msg) => msg,
            Err(Bad::Kind | Bad::Field) => return self.protocol("a field of a frame is out of range", events),
        };
        if self.wait_limit.is_some() {
            self.wait_from = Some(now);
        }
        match msg {
            Msg::Stop { code, reason } => {
                let reason = reason_text(&reason);
                self.end(Ended::Stopped { code, by_peer: true, reason }, events);
            }
            Msg::Helo { version, id, chunk } => self.on_hello(version, id, chunk, now, events),
            Msg::Offr { size, mode, mtime, name } if self.phase == Phase::AwaitOffer => {
                let Some(name) = file_name(&name) else {
                    return self.stop(stop::REFUSED, "the name names no file here", events);
                };
                let info =
                    FileInfo { name, size, mode: (mode != 0).then_some(mode), mtime: (mtime != 0).then_some(mtime) };
                events.push(Event::Offer { name: info.name.clone(), size, mode: info.mode, mtime: info.mtime });
                self.file = Some(info);
                self.phase = Phase::Offered;
                self.wait_caller();
            }
            Msg::Offr { .. } => self.answer_again(),
            Msg::Take { from } => self.on_take(from, events),
            Msg::Skip { reason } => {
                let reason = reason_text(&reason);
                self.end(Ended::Stopped { code: stop::REFUSED, by_peer: true, reason }, events);
            }
            Msg::Data { offset, bytes } => self.on_data(offset, &bytes, events),
            Msg::Ackn { of, count } => self.on_ack(of, count, now, events),
            Msg::Done { size, sha256 } => self.on_done(size, sha256, events),
            Msg::Hold => self.on_hold(now),
            Msg::Kept { size } => {
                if Some(size) == self.file.as_ref().map(|f| f.size) {
                    self.end(Ended::Done, events);
                } else {
                    self.protocol("KEPT names another size", events);
                }
            }
        }
    }

    fn on_hello(&mut self, version: u8, id: [u8; 8], chunk: u32, now: Instant, events: &mut Vec<Event>) {
        if u32::from(version) != VERSION {
            let why = format!("HELO of version {version}; this end takes version {VERSION}");
            return self.stop(stop::UNSUPPORTED, &why, events);
        }
        if id != self.id || chunk == 0 {
            return self.protocol("HELO does not match the trigger", events);
        }
        if self.phase != Phase::Hello {
            // The far end's answer was lost: the near end sent HELO again.
            self.send(&self.hello());
            return;
        }
        self.cap = CHUNK_MAX.min(chunk as usize);
        self.chunk = self.floor();
        self.flight = None;
        if self.far {
            self.send(&self.hello());
        }
        self.wait_peer(Some(now));
        if self.sender {
            let f = self.file.clone().unwrap_or_default();
            let offer = Msg::Offr {
                size: f.size,
                mode: f.mode.unwrap_or(0) & 0o7777,
                mtime: f.mtime.unwrap_or(0),
                name: f.name.into_bytes(),
            };
            self.fly(&offer);
            self.phase = Phase::AwaitTake;
        } else {
            self.phase = Phase::AwaitOffer;
        }
    }

    fn floor(&self) -> usize {
        CHUNK_MIN.min(self.cap)
    }

    fn size(&self) -> u64 {
        self.file.as_ref().map_or(0, |f| f.size)
    }

    /// The frame in flight again, with the same bytes, and the chunk at its
    /// floor.
    fn resend(&mut self) {
        if let Some(flight) = &mut self.flight {
            self.out.extend_from_slice(&flight.bytes);
            flight.handed = None;
            flight.resent = true;
            self.chunk = CHUNK_MIN.min(self.cap);
        }
    }

    /// A `HOLD`: it is no ack and does not time the path. The frame in
    /// flight goes again `RESEND_MAX` after it, so a person who takes a
    /// minute costs about 12 frames each way.
    fn on_hold(&mut self, now: Instant) {
        if let Some(flight) = &mut self.flight {
            flight.handed = Some(now);
            flight.held = true;
        }
    }

    fn resend_at(&self) -> Option<Instant> {
        let flight = self.flight.as_ref()?;
        let rto = match self.last_ack {
            _ if flight.held => RESEND_MAX,
            Some(a) => (a * 4).clamp(RESEND_MIN, RESEND_MAX),
            None => RESEND_MIN,
        };
        flight.handed.map(|h| h + rto)
    }

    fn wait_until(&self) -> Option<Instant> {
        Some(self.wait_from? + self.wait_limit?)
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        [self.resend_at(), self.wait_until()].into_iter().flatten().min()
    }

    pub(super) fn tick(&mut self, now: Instant, events: &mut Vec<Event>) {
        if self.wait_until().is_some_and(|t| now >= t) {
            if self.phase == Phase::AwaitKept {
                // The file and its hash went; a lost KEPT is no stop. No
                // STOP goes: the receiver can have kept the file.
                return self.end(Ended::Unconfirmed, events);
            }
            return self.stop(stop::OUT_OF_TIME, "no answer from the peer in time", events);
        }
        if self.resend_at().is_some_and(|t| now >= t) {
            self.resend();
        }
    }

    /// The bytes for the peer. The time of an ack and of a wait runs from
    /// here.
    pub(super) fn poll(&mut self, now: Instant) -> Vec<u8> {
        if self.out.is_empty() {
            return Vec::new();
        }
        if let Some(flight) = &mut self.flight {
            flight.handed.get_or_insert(now);
        }
        if self.wait_limit.is_some() {
            self.wait_from.get_or_insert(now);
        }
        std::mem::take(&mut self.out)
    }
}

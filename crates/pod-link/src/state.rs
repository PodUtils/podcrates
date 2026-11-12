//! The state of one link: the table of streams, the credits, the queues of
//! frames for the carrier and the keepalive's times. One mutex holds it;
//! no lock is held across an await. [`Shared::with`] runs an operation,
//! then wakes the tasks that it woke, out of the lock.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};
use std::task::Waker;
use std::time::Duration;

use tokio::sync::Notify;
use tokio::time::Instant;

use crate::error::{Error, GoAwayCode};
use crate::frame::{Frame, PREFACE};
use crate::settings::{INITIAL_WINDOW, Settings};

/// The most frames other than `DATA` that wait for the carrier. A peer
/// that sends `PING` or `OPEN` frames and does not read the answers ends
/// the link at this count, so its answers cannot fill the memory.
pub(crate) const CONTROL_LIMIT: usize = 8192;

/// The most bytes that one write to the carrier takes from the queue, so
/// that a `WINDOW` or a `PONG` does not wait behind a long run of `DATA`.
const BATCH: usize = 64 << 10;

/// Which end of the link this is. The dialer opens streams with odd ids,
/// the listener with even ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Dialer,
    Listener,
}

impl Role {
    fn first_id(self) -> u64 {
        match self {
            Role::Dialer => 1,
            Role::Listener => 2,
        }
    }

    /// Whether `id` is an id that this end opens.
    pub(crate) fn owns(self, id: u32) -> bool {
        (id % 2 == 1) == (self == Role::Dialer)
    }
}

/// One stream of the link.
#[derive(Debug)]
pub(crate) struct Slot {
    /// The bytes that came and that the reader did not take.
    pub buf: VecDeque<u8>,
    /// The receive window of the stream now: it grows to the setting.
    pub window: u32,
    /// The bytes that the peer can still send before more credit.
    pub recv_left: u64,
    /// The bytes that the reader took and whose credit did not go back.
    pub to_return: u64,
    /// The bytes that this end can still send.
    pub send_credit: u64,
    pub local_fin: bool,
    pub remote_fin: bool,
    /// The reset of the stream (an [`Error::Reset`]), by either end.
    pub reset: Option<Error>,
    pub read_waker: Option<Waker>,
    pub write_waker: Option<Waker>,
    /// The writer waits for credit of the link or for room in the queue.
    pub blocked: bool,
}

impl Slot {
    fn new() -> Slot {
        Slot {
            buf: VecDeque::new(),
            window: INITIAL_WINDOW,
            recv_left: u64::from(INITIAL_WINDOW),
            to_return: 0,
            send_credit: u64::from(INITIAL_WINDOW),
            local_fin: false,
            remote_fin: false,
            reset: None,
            read_waker: None,
            write_waker: None,
            blocked: false,
        }
    }
}

/// What the writer task does after a batch.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Batch {
    /// Write the bytes; take the next batch.
    Write,
    /// Nothing to write: wait.
    Idle,
    /// Write the bytes, shut the carrier down, and stop.
    Stop,
}

pub(crate) struct State {
    pub role: Role,
    pub settings: Settings,
    next_id: u64,
    /// The largest id that the peer opened.
    pub last_peer: u32,
    pub streams: HashMap<u32, Slot>,
    pub accept: VecDeque<u32>,
    /// `WINDOW`, `PING` and `PONG`: before the other frames.
    urgent: VecDeque<Frame>,
    /// `OPEN`, `DATA`, `CLOSE`, `RESET` and `GOAWAY`, in the order of each
    /// stream.
    out: VecDeque<Frame>,
    /// The `DATA` bytes in `out`.
    pub queued: usize,
    /// The frames other than `DATA` in both queues.
    control: usize,
    preface_sent: bool,
    /// The streams whose writer waits for the link's credit or for room.
    blocked: Vec<u32>,
    /// The bytes that this end can still send on the link.
    pub send_credit: u64,
    /// The bytes that the peer can still send on the link.
    pub recv_left: u64,
    /// The bytes of the link that left the streams (read or dropped) and
    /// whose credit did not go back.
    pub to_return: u64,
    pub last_heard: Instant,
    pub last_ping: Instant,
    pub ping: Option<(u64, Instant)>,
    pub ping_seq: u64,
    pub rtt: Option<Duration>,
    /// This end queued its `GOAWAY`.
    pub closing: bool,
    /// The carrier took this end's `GOAWAY`: the end of the carrier is the
    /// close of the link.
    pub goaway_sent: bool,
    pub ended: Option<Error>,
    /// The frame that the writer sends after the end: a `GOAWAY` for a
    /// peer that broke the protocol.
    final_frame: Option<Frame>,
    kick: bool,
    kick_accept: bool,
    kick_end: bool,
    woken: Vec<Waker>,
}

impl State {
    pub fn new(role: Role, settings: Settings) -> State {
        let now = Instant::now();
        let mut state = State {
            role,
            settings,
            next_id: role.first_id(),
            last_peer: 0,
            streams: HashMap::new(),
            accept: VecDeque::new(),
            urgent: VecDeque::new(),
            out: VecDeque::new(),
            queued: 0,
            control: 0,
            preface_sent: false,
            blocked: Vec::new(),
            send_credit: u64::from(INITIAL_WINDOW),
            recv_left: u64::from(settings.link_window),
            to_return: 0,
            last_heard: now,
            last_ping: now,
            ping: None,
            ping_seq: 0,
            rtt: None,
            closing: false,
            goaway_sent: false,
            ended: None,
            final_frame: None,
            kick: false,
            kick_accept: false,
            kick_end: false,
            woken: Vec::new(),
        };
        // The link's window past the initial credit goes at once.
        let more = settings.link_window - INITIAL_WINDOW;
        if more > 0 {
            state.push_urgent(Frame::Window(0, more));
        }
        state
    }

    /// The most `DATA` bytes that wait for the carrier.
    pub fn queue_limit(&self) -> usize {
        (4 * self.settings.frame_size).max(BATCH)
    }

    pub fn push_urgent(&mut self, frame: Frame) {
        self.count_control();
        self.urgent.push_back(frame);
        self.kick = true;
    }

    pub fn push_out(&mut self, frame: Frame) {
        match &frame {
            Frame::Data(_, body) => self.queued += body.len(),
            _ => self.count_control(),
        }
        self.out.push_back(frame);
        self.kick = true;
    }

    fn count_control(&mut self) {
        self.control += 1;
        if self.control > CONTROL_LIMIT {
            self.fail(Error::Protocol(format!(
                "more than {CONTROL_LIMIT} frames wait for the peer: it sends and does not read"
            )));
        }
    }

    pub fn wake(&mut self, waker: Option<Waker>) {
        if let Some(w) = waker {
            self.woken.push(w);
        }
    }

    /// Mark a stream's writer as one that waits for the link.
    pub fn block(&mut self, id: u32) {
        if let Some(slot) = self.streams.get_mut(&id)
            && !slot.blocked
        {
            slot.blocked = true;
            self.blocked.push(id);
        }
    }

    /// Wake each writer that waits for the link's credit or for room.
    pub fn unblock(&mut self) {
        for id in std::mem::take(&mut self.blocked) {
            if let Some(slot) = self.streams.get_mut(&id) {
                slot.blocked = false;
                let w = slot.write_waker.take();
                self.wake(w);
            }
        }
    }

    pub fn accepted(&mut self, id: u32) {
        self.accept.push_back(id);
        self.kick_accept = true;
    }

    /// Open a stream of this end.
    pub fn open(&mut self) -> Result<u32, Error> {
        if let Some(e) = &self.ended {
            return Err(e.clone());
        }
        if self.closing {
            return Err(Error::Closed { by_peer: false });
        }
        if self.streams.len() >= self.settings.max_streams {
            return Err(Error::TooManyStreams(self.settings.max_streams));
        }
        let Ok(id) = u32::try_from(self.next_id) else {
            return Err(Error::IdsExhausted);
        };
        self.next_id += 2;
        self.streams.insert(id, Slot::new());
        self.push_out(Frame::Open(id));
        Ok(id)
    }

    /// Insert a stream that the peer opened.
    pub fn insert_peer(&mut self, id: u32) {
        self.streams.insert(id, Slot::new());
    }

    /// Whether `id` names a stream that existed and is gone: its frames
    /// that were on the way are dropped.
    pub fn is_old(&self, id: u32) -> bool {
        if self.role.owns(id) { u64::from(id) < self.next_id } else { id <= self.last_peer }
    }

    /// Give credit back after a read or a drop: the stream's when the
    /// reader took half its window, the link's with it or when 16 KiB
    /// wait. A reader that empties its buffer doubles its window.
    pub fn return_credit(&mut self, id: Option<u32>) {
        let max = self.settings.stream_window;
        let mut sent = false;
        if let Some(slot) = id.and_then(|id| self.streams.get_mut(&id).map(|s| (id, s))) {
            let (id, slot) = slot;
            let open = !slot.remote_fin && slot.reset.is_none();
            if open && slot.to_return >= u64::from(slot.window / 2) {
                let grow = if slot.buf.is_empty() { slot.window.min(max.saturating_sub(slot.window)) } else { 0 };
                slot.window += grow;
                let more = slot.to_return + u64::from(grow);
                slot.recv_left += more;
                slot.to_return = 0;
                sent = true;
                self.push_urgent(Frame::Window(id, more as u32));
            }
        }
        if self.to_return > 0 && (sent || self.to_return >= u64::from(INITIAL_WINDOW)) {
            let more = std::mem::take(&mut self.to_return);
            self.recv_left += more;
            self.push_urgent(Frame::Window(0, more as u32));
        }
    }

    /// Bytes of the link that no stream keeps: their credit goes back.
    pub fn discard(&mut self, n: usize) {
        self.to_return += n as u64;
        self.return_credit(None);
    }

    /// End the link with `e`. Each stream and each waiter learns it. A peer
    /// that broke the protocol gets a `GOAWAY` with the code `PROTOCOL`.
    pub fn fail(&mut self, e: Error) {
        if self.ended.is_some() {
            return;
        }
        if matches!(e, Error::Frame(_) | Error::Protocol(_)) {
            self.final_frame = Some(Frame::GoAway(GoAwayCode::PROTOCOL.0));
        }
        tracing::debug!(error = %e, "pod-link: the link ends");
        self.ended = Some(e);
        let wakers: Vec<Waker> =
            self.streams.values_mut().flat_map(|s| [s.read_waker.take(), s.write_waker.take()]).flatten().collect();
        self.woken.extend(wakers);
        self.kick_end = true;
    }

    /// The reason of the end of the carrier after this end's `GOAWAY`.
    pub fn after_goaway(&self) -> Option<Error> {
        self.goaway_sent.then_some(Error::Closed { by_peer: false })
    }

    /// Take the next frames for the carrier, encoded into `out`.
    pub fn take_batch(&mut self, out: &mut Vec<u8>) -> Batch {
        if !self.preface_sent {
            self.preface_sent = true;
            out.extend_from_slice(PREFACE);
        }
        if self.ended.is_some() {
            if let Some(f) = self.final_frame.take() {
                encode(&f, out);
            }
            return Batch::Stop;
        }
        let full = self.queued >= self.queue_limit();
        while let Some(f) = self.urgent.pop_front() {
            self.control -= 1;
            encode(&f, out);
        }
        let mut stop = false;
        while out.len() < BATCH
            && let Some(f) = self.out.pop_front()
        {
            match &f {
                Frame::Data(_, body) => self.queued -= body.len(),
                _ => self.control -= 1,
            }
            encode(&f, out);
            if let Frame::GoAway(_) = f {
                stop = true;
                break;
            }
        }
        if full && self.queued < self.queue_limit() {
            self.unblock();
        }
        match (stop, out.is_empty()) {
            (true, _) => Batch::Stop,
            (false, true) => Batch::Idle,
            (false, false) => Batch::Write,
        }
    }
}

fn encode(frame: &Frame, out: &mut Vec<u8>) {
    // The link makes each frame itself, within the rules.
    if let Err(e) = frame.encode(out) {
        debug_assert!(false, "a frame of the link breaks a rule: {e}");
    }
}

/// The state of a link and the notices of its tasks.
pub(crate) struct Shared {
    state: Mutex<State>,
    /// Frames wait for the writer task.
    pub writer: Notify,
    /// A stream waits to be accepted, or the link ended.
    pub accepts: Notify,
    /// The link ended.
    pub ended: Notify,
}

impl Shared {
    pub fn new(state: State) -> Shared {
        Shared { state: Mutex::new(state), writer: Notify::new(), accepts: Notify::new(), ended: Notify::new() }
    }

    /// Run `f` on the state, then tell the tasks that it woke.
    pub fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let r = f(&mut state);
        let kick = std::mem::take(&mut state.kick);
        let accept = std::mem::take(&mut state.kick_accept);
        let end = std::mem::take(&mut state.kick_end);
        let woken = std::mem::take(&mut state.woken);
        drop(state);
        for w in woken {
            w.wake();
        }
        if kick || end {
            self.writer.notify_one();
        }
        if accept || end {
            self.accepts.notify_waiters();
        }
        if end {
            self.ended.notify_waiters();
        }
        r
    }

    pub fn fail(&self, e: Error) {
        self.with(|s| s.fail(e));
    }

    /// Why the link ended, when it did.
    pub fn ended(&self) -> Option<Error> {
        self.with(|s| s.ended.clone())
    }
}

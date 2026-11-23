//! The window of 1 to 7 frames with acknowledgements, and a retransmission
//! timeout that comes from the baud rate.
//!
//! One frame carries a control byte and the bytes of the caller: an
//! information frame has control `0x00 | (sequence << 1)`, and a
//! receiver-ready frame (the acknowledgement) `0x01 | (sequence << 1)`, as
//! AX.25 numbers them modulo 8. An acknowledgement is cumulative: it takes
//! the named frame and each frame before it. [`Receiver`] takes the frames in
//! order and gives the acknowledgement of the last one.
//!
//! A 256-byte frame spends 2560 bits on an 8N1 line, so at 1200 baud it
//! takes about 2.1 s. [`retransmit_after`] is two frame times, the send and
//! the return: about 4.3 s at 1200 baud. There is no fixed floor: a floor of
//! 200 ms sits below one frame time and retransmits into the frame that is
//! still arriving.
//!
//! The engine is sans I/O: the caller puts each frame in KISS framing
//! ([`super::kiss::encode`]), writes it to its line, and each call gives
//! the engine the clock.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::kiss::MAX_FRAME;
use crate::Error;

/// The least window, in frames.
pub const WINDOW_LEAST: u8 = 1;
/// The most window, in frames: AX.25 numbers a frame modulo 8.
pub const WINDOW_MOST: u8 = 7;
/// The bytes of the control field of one frame.
pub const HEADER: usize = 1;
/// The largest caller payload of one frame, with the control byte inside
/// the 256-byte maximum.
pub const MAX_PAYLOAD: usize = MAX_FRAME - HEADER;
/// The bits of one byte on an 8N1 line: 8 data bits, no parity, 1 stop bit.
pub const BITS_PER_BYTE: u32 = 10;

/// The time that a full 256-byte frame spends on a line at `baud`.
pub fn frame_time(baud: u32) -> Duration {
    Duration::from_secs_f64(f64::from(MAX_FRAME as u32 * BITS_PER_BYTE) / f64::from(baud))
}

/// The time after which the sender sends an unacknowledged frame again:
/// the frame out and the acknowledgement back.
pub fn retransmit_after(baud: u32) -> Duration {
    frame_time(baud) * 2
}

/// The control byte of an information frame of `seq`.
pub fn i_control(seq: u8) -> u8 {
    (seq & 0x07) << 1
}

/// The control byte of an acknowledgement of `seq`.
pub fn rr_control(seq: u8) -> u8 {
    0x01 | ((seq & 0x07) << 1)
}

/// The sequence of a control byte.
pub fn seq_of(control: u8) -> u8 {
    (control >> 1) & 0x07
}

/// Whether the control byte is an acknowledgement.
pub fn is_ack(control: u8) -> bool {
    control & 0x01 == 1
}

/// What one frame of the link carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Received {
    /// An information frame with its sequence and its bytes.
    Data { seq: u8, payload: Vec<u8> },
    /// An acknowledgement of a sequence.
    Ack { seq: u8 },
}

/// Read the payload of one KISS frame of the link.
pub fn parse(payload: &[u8]) -> Result<Received, Error> {
    let (control, rest) = payload.split_first().ok_or_else(|| Error::Frame { why: "no control byte".into() })?;
    if is_ack(*control) {
        Ok(Received::Ack { seq: seq_of(*control) })
    } else {
        Ok(Received::Data { seq: seq_of(*control), payload: rest.to_vec() })
    }
}

/// The receiver of a window: the sequence that it expects, and the
/// acknowledgement that it sends back.
#[derive(Debug, Clone)]
pub struct Receiver {
    expected: u8,
}

impl Default for Receiver {
    fn default() -> Receiver {
        Receiver::new()
    }
}

impl Receiver {
    /// A receiver that expects the frame 0.
    pub fn new() -> Receiver {
        Receiver { expected: 0 }
    }

    /// Take one frame. It gives `true` for the frame that the receiver
    /// expects, and the next frame is then expected. A frame out of order
    /// gives `false`, and the caller sends the last good acknowledgement.
    pub fn accept(&mut self, seq: u8) -> bool {
        if seq == self.expected {
            self.expected = (self.expected + 1) & 0x07;
            true
        } else {
            false
        }
    }

    /// The control byte that acknowledges the last frame taken in order.
    pub fn ack(&self) -> u8 {
        rr_control((self.expected + 0x07) & 0x07)
    }

    /// The sequence that this receiver expects next.
    pub fn expected(&self) -> u8 {
        self.expected
    }
}

#[derive(Debug)]
struct Sent {
    seq: u8,
    frame: Vec<u8>,
    sent_at: Instant,
}

/// The sender of a window of frames.
#[derive(Debug)]
pub struct Arq {
    window: u8,
    baud: u32,
    next_seq: u8,
    inflight: VecDeque<Sent>,
    queue: VecDeque<Vec<u8>>,
    defer: bool,
    retransmissions: u32,
}

impl Arq {
    /// A sender of `window` frames at `baud`. The window is 1 to 7.
    pub fn new(window: u8, baud: u32) -> Result<Arq, Error> {
        if !(WINDOW_LEAST..=WINDOW_MOST).contains(&window) {
            return Err(Error::Window { window });
        }
        if baud == 0 {
            return Err(Error::Baud { baud });
        }
        Ok(Arq {
            window,
            baud,
            next_seq: 0,
            inflight: VecDeque::new(),
            queue: VecDeque::new(),
            defer: false,
            retransmissions: 0,
        })
    }

    /// A sender that never retransmits: the ARQ of the modem owns the
    /// retransmission, and a second chain would stack on the first.
    pub fn deferring(baud: u32) -> Result<Arq, Error> {
        let mut arq = Arq::new(1, baud)?;
        arq.defer = true;
        Ok(arq)
    }

    pub fn window(&self) -> u8 {
        self.window
    }

    pub fn baud(&self) -> u32 {
        self.baud
    }

    /// Whether this sender leaves its retransmissions to the modem.
    pub fn deferring_arq(&self) -> bool {
        self.defer
    }

    /// The timeout of one frame of this line.
    pub fn timeout(&self) -> Duration {
        retransmit_after(self.baud)
    }

    /// The frames that were sent and not acknowledged yet.
    pub fn inflight(&self) -> usize {
        self.inflight.len()
    }

    /// The payloads that wait for a place in the window.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// The retransmissions of this sender.
    pub fn retransmissions(&self) -> u32 {
        self.retransmissions
    }

    /// Offer `payload` at `now`. It gives the frame to write when a place
    /// in the window is free, and `None` when it waits in the queue.
    pub fn push_at(&mut self, now: Instant, payload: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        if payload.len() > MAX_PAYLOAD {
            let size = payload.len() + HEADER;
            return Err(Error::FrameTooLarge { size, max: MAX_FRAME });
        }
        if self.queue.is_empty() && self.inflight.len() < usize::from(self.window) {
            return self.send(now, payload).map(Some);
        }
        self.queue.push_back(payload.to_vec());
        Ok(None)
    }

    /// The next frame to write at `now`: a queued frame that has a place,
    /// or the oldest frame that has waited for its timeout. `None` when the
    /// line has nothing to send.
    pub fn poll_at(&mut self, now: Instant) -> Result<Option<Vec<u8>>, Error> {
        if let Some(payload) = self.queue.pop_front() {
            if self.inflight.len() < usize::from(self.window) {
                return self.send(now, &payload).map(Some);
            }
            self.queue.push_front(payload);
        }
        if self.defer {
            return Ok(None);
        }
        let timeout = self.timeout();
        if let Some(front) = self.inflight.front_mut()
            && front.sent_at + timeout <= now
        {
            front.sent_at = now;
            self.retransmissions += 1;
            return Ok(Some(front.frame.clone()));
        }
        Ok(None)
    }

    /// Take `seq` and each frame that the receiver acknowledges before it.
    /// It gives the number of frames taken.
    pub fn ack(&mut self, seq: u8) -> Result<usize, Error> {
        let at = self.inflight.iter().position(|sent| sent.seq == seq).ok_or(Error::Ack { seq })?;
        let taken = at + 1;
        self.inflight.drain(..taken);
        Ok(taken)
    }

    /// The control byte that acknowledges the last frame in flight, for a
    /// receiver that has all of them.
    pub fn ack_control(&self) -> Option<u8> {
        self.inflight.back().map(|sent| rr_control(sent.seq))
    }

    fn send(&mut self, now: Instant, payload: &[u8]) -> Result<Vec<u8>, Error> {
        let seq = self.next_seq & 0x07;
        self.next_seq = (self.next_seq + 1) & 0x07;
        let mut frame = Vec::with_capacity(payload.len() + HEADER);
        frame.push(i_control(seq));
        frame.extend_from_slice(payload);
        self.inflight.push_back(Sent { seq, frame: frame.clone(), sent_at: now });
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timeout_comes_from_the_baud_rate_and_not_from_a_floor() {
        let at_1200 = retransmit_after(1200);
        assert_eq!(frame_time(1200), Duration::from_secs_f64(2560.0 / 1200.0));
        assert_eq!(at_1200, frame_time(1200) * 2);
        assert!(at_1200 > Duration::from_secs(4));
        assert!(at_1200 > Duration::from_millis(200));
        assert!(frame_time(1200) > Duration::from_millis(200));
        // A faster line waits less, in the same proportion.
        assert!(retransmit_after(9600) < at_1200);
    }

    #[test]
    fn the_window_is_one_to_seven() {
        assert!(Arq::new(0, 1200).is_err());
        assert!(Arq::new(8, 1200).is_err());
        for window in 1..=7 {
            assert_eq!(Arq::new(window, 1200).unwrap().window(), window);
        }
    }

    #[test]
    fn a_full_window_queues_the_next_frame() {
        let now = Instant::now();
        let mut arq = Arq::new(2, 1200).unwrap();
        assert!(arq.push_at(now, b"one").unwrap().is_some());
        assert!(arq.push_at(now, b"two").unwrap().is_some());
        assert!(arq.push_at(now, b"three").unwrap().is_none());
        assert_eq!((arq.inflight(), arq.queued()), (2, 1));
        assert_eq!(arq.ack(0).unwrap(), 1);
        assert!(arq.poll_at(now).unwrap().is_some());
        assert_eq!((arq.inflight(), arq.queued()), (2, 0));
    }

    #[test]
    fn a_lost_frame_is_sent_again_after_the_timeout() {
        let start = Instant::now();
        let mut arq = Arq::new(1, 1200).unwrap();
        let first = arq.push_at(start, b"lost").unwrap().unwrap();
        assert!(arq.poll_at(start + Duration::from_millis(200)).unwrap().is_none());
        assert!(arq.poll_at(start + arq.timeout() - Duration::from_millis(1)).unwrap().is_none());
        let again = arq.poll_at(start + arq.timeout()).unwrap().unwrap();
        assert_eq!(again, first);
        assert_eq!(arq.retransmissions(), 1);
    }

    #[test]
    fn a_deferring_sender_leaves_each_retransmission_to_the_modem() {
        let start = Instant::now();
        let mut arq = Arq::deferring(1200).unwrap();
        arq.push_at(start, b"lost").unwrap().unwrap();
        assert!(arq.poll_at(start + Duration::from_secs(3600)).unwrap().is_none());
        assert_eq!(arq.retransmissions(), 0);
        assert!(arq.deferring_arq());
    }

    #[test]
    fn a_payload_above_the_frame_is_refused_by_name() {
        let mut arq = Arq::new(1, 1200).unwrap();
        let failed = arq.push_at(Instant::now(), &vec![0u8; MAX_PAYLOAD + 1]).unwrap_err();
        assert_eq!(failed.name(), "frame-too-large");
        assert!(matches!(failed, Error::FrameTooLarge { size, max } if size == MAX_FRAME + 1 && max == MAX_FRAME));
    }

    #[test]
    fn the_sequence_and_the_acknowledgement_of_a_frame_are_read_back() {
        assert_eq!(i_control(3), 0x06);
        assert_eq!(rr_control(3), 0x07);
        assert_eq!(seq_of(rr_control(5)), 5);
        assert!(is_ack(rr_control(0)));
        assert!(!is_ack(i_control(0)));
        assert_eq!(parse(&[i_control(2), 9, 8]).unwrap(), Received::Data { seq: 2, payload: vec![9, 8] });
        assert_eq!(parse(&[rr_control(4)]).unwrap(), Received::Ack { seq: 4 });
        assert_eq!(parse(&[]).unwrap_err().name(), "frame");
    }

    #[test]
    fn an_acknowledgement_of_a_frame_that_is_not_in_flight_is_refused() {
        let mut arq = Arq::new(3, 1200).unwrap();
        arq.push_at(Instant::now(), b"one").unwrap().unwrap();
        assert_eq!(arq.ack(5).unwrap_err().name(), "ack");
    }

    #[test]
    fn the_receiver_takes_the_frames_in_order_and_acknowledges_the_last_one() {
        let mut receiver = Receiver::new();
        assert_eq!(receiver.expected(), 0);
        assert!(receiver.accept(0));
        assert_eq!(receiver.ack(), rr_control(0));
        assert!(receiver.accept(1));
        assert_eq!(receiver.ack(), rr_control(1));
        assert!(!receiver.accept(1), "a repeated frame was taken twice");
        assert!(!receiver.accept(3), "a frame out of order was taken");
        assert_eq!(receiver.ack(), rr_control(1));
        assert_eq!(receiver.expected(), 2);
    }
}

//! An ARDOP or VARA modem as a serial device with an ASCII command set.
//!
//! The modem takes text commands, each ended by a carriage return, and it
//! answers in text. This driver uses:
//!
//! * `SPEED <bps>`: the speed of the carrier, from 300 to 3000 bit/s. The
//!   modem answers `OK`.
//! * `TX <n>`: then `n` bytes of one KISS frame of the port
//!   [`kiss::PORT_MODEM`]. The modem answers `OK`, or `BUSY` when the
//!   channel is not free.
//! * `BUSY` and `IDLE`: the modem says when it receives and when the
//!   channel is free again.
//! * `DATA <n>`: then `n` bytes, which hold the KISS frames that the modem
//!   received.
//!
//! A half-duplex link never transmits while it receives. [`Modem::transmit_at`]
//! refuses when the modem said `BUSY` or `DATA`, and it refuses during the
//! turnaround after a reception. The modem's ARQ is the only ARQ:
//! [`DeferArq`] gives a session ARQ with no retransmission of its own, so
//! two chains do not stack.
//!
//! The driver blocks on the line while it waits for the answer of the
//! modem. The README names this limit.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use crate::Error;
use crate::serial::arq::{Arq, seq_of};
use crate::serial::kiss::{self, PORT_MODEM};

/// The least speed of the carrier.
pub const SPEED_LEAST: u32 = 300;
/// The most speed of the carrier.
pub const SPEED_MOST: u32 = 3_000;
/// The speed when the caller names none.
pub const DEFAULT_SPEED: u32 = 1_200;
/// The quiet time between a reception and the next transmission.
pub const TURNAROUND: Duration = Duration::from_millis(500);
/// The longest line of the modem, in bytes.
const LINE_MOST: usize = 128;

/// What the modem does now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// The channel is free for this end.
    Idle,
    /// The modem receives. The link is half-duplex.
    Receiving,
}

/// The ARQ of the session when the modem owns the retransmission. The
/// session sends a frame once, and the modem sends it again: one chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeferArq {
    modem_retransmits: u8,
}

impl DeferArq {
    /// A policy for a modem that retries a frame `modem_retransmits` times.
    pub const fn new(modem_retransmits: u8) -> DeferArq {
        DeferArq { modem_retransmits }
    }

    /// The degraded-link profile, which this carrier always uses: the
    /// window of one frame, and each retransmission left to the modem.
    pub const DEGRADED: DeferArq = DeferArq::new(5);

    /// The retransmissions of the session: none, by construction.
    pub const fn session_retransmits(&self) -> u8 {
        0
    }

    /// The retransmissions of the modem.
    pub const fn modem_retransmits(&self) -> u8 {
        self.modem_retransmits
    }

    /// The ARQ of the session for this policy: a deferring one.
    pub fn arq(&self, baud: u32) -> Result<Arq, Error> {
        Arq::deferring(baud)
    }
}

/// A modem on a serial line.
pub struct Modem<L: Read + Write> {
    link: L,
    speed: u32,
    activity: Activity,
    ready_at: Option<Instant>,
    turnaround: Duration,
    writes: u32,
    frames: VecDeque<Vec<u8>>,
}

impl<L: Read + Write> Modem<L> {
    /// A modem at [`DEFAULT_SPEED`].
    pub fn new(link: L) -> Modem<L> {
        Modem {
            link,
            speed: DEFAULT_SPEED,
            activity: Activity::Idle,
            ready_at: None,
            turnaround: TURNAROUND,
            writes: 0,
            frames: VecDeque::new(),
        }
    }

    /// The link, for a caller that reads a line of its own.
    pub fn link(&self) -> &L {
        &self.link
    }

    pub fn link_mut(&mut self) -> &mut L {
        &mut self.link
    }

    pub fn speed(&self) -> u32 {
        self.speed
    }

    pub fn activity(&self) -> Activity {
        self.activity
    }

    /// The time when the turnaround ends, after a reception.
    pub fn ready_at(&self) -> Option<Instant> {
        self.ready_at
    }

    /// The frames that the driver wrote, a retransmission included.
    pub fn writes(&self) -> u32 {
        self.writes
    }

    /// The retransmissions of the modem: each write after the first.
    pub fn retransmissions(&self) -> u32 {
        self.writes.saturating_sub(1)
    }

    /// Tell the modem the speed of the carrier.
    pub fn set_speed(&mut self, bps: u32) -> Result<(), Error> {
        if !(SPEED_LEAST..=SPEED_MOST).contains(&bps) {
            return Err(Error::Speed { bps });
        }
        let answer = self.command(&format!("SPEED {bps}"))?;
        if answer != "OK" {
            return Err(Error::Modem { why: format!("the modem answered {answer:?} to SPEED") });
        }
        self.speed = bps;
        Ok(())
    }

    /// Tell the driver that a reception ended at `now`: the next
    /// transmission waits for the turnaround.
    pub fn note_receive_at(&mut self, now: Instant) {
        self.activity = Activity::Receiving;
        self.ready_at = Some(now + self.turnaround);
    }

    /// Send one frame of the session at `now`. The modem puts it in a KISS
    /// frame of its own port. A busy channel and a reception are errors;
    /// no byte goes to the line then.
    pub fn transmit_at(&mut self, now: Instant, data: &[u8]) -> Result<(), Error> {
        if self.activity == Activity::Receiving {
            return Err(Error::Receiving);
        }
        if let Some(ready) = self.ready_at
            && now < ready
        {
            return Err(Error::Busy { retry_after: ready - now });
        }
        let wire = kiss::encode(PORT_MODEM, data)?;
        self.write_all(format!("TX {}\r", wire.len()).as_bytes())?;
        self.write_all(&wire)?;
        self.flush()?;
        self.writes += 1;
        let answer = self.read_line()?;
        match answer.as_str() {
            "OK" => {
                self.activity = Activity::Idle;
                self.ready_at = None;
                Ok(())
            }
            "BUSY" => {
                self.activity = Activity::Receiving;
                self.ready_at = Some(now + self.turnaround);
                Err(Error::Busy { retry_after: self.turnaround })
            }
            other => Err(Error::Modem { why: format!("the modem answered {other:?} to TX") }),
        }
    }

    /// Read the next word of the modem at `now`. `DATA` gives a frame of
    /// the session.
    pub fn receive_at(&mut self, now: Instant) -> Result<Option<Vec<u8>>, Error> {
        if let Some(frame) = self.frames.pop_front() {
            return Ok(Some(frame));
        }
        let line = self.read_line()?;
        match line.as_str() {
            "IDLE" => {
                self.activity = Activity::Idle;
                self.ready_at = None;
                Ok(None)
            }
            "BUSY" => {
                self.activity = Activity::Receiving;
                self.ready_at = Some(now + self.turnaround);
                Ok(None)
            }
            word if word.starts_with("DATA ") => {
                let count = parse_count(word, "DATA")?;
                let mut bytes = vec![0u8; count];
                self.link.read_exact(&mut bytes).map_err(|e| Error::Modem { why: e.to_string() })?;
                let mut decoder = kiss::Decoder::new();
                for frame in decoder.push(&bytes) {
                    let frame = frame?;
                    self.frames.push_back(frame.payload);
                }
                self.note_receive_at(now);
                Ok(self.frames.pop_front())
            }
            other => Err(Error::Modem { why: format!("the modem said {other:?}") }),
        }
    }

    fn command(&mut self, text: &str) -> Result<String, Error> {
        self.write_all(format!("{text}\r").as_bytes())?;
        self.flush()?;
        self.read_line()
    }

    fn read_line(&mut self) -> Result<String, Error> {
        let mut line = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            match self.link.read(&mut byte) {
                Ok(0) => return Err(Error::Modem { why: "the line ended".into() }),
                Ok(_) => {}
                Err(e) => return Err(Error::Modem { why: e.to_string() }),
            }
            match byte[0] {
                b'\r' | b'\n' => {
                    if line.is_empty() {
                        continue;
                    }
                    break;
                }
                other => line.push(other),
            }
            if line.len() > LINE_MOST {
                return Err(Error::Modem { why: format!("a line of the modem is above {LINE_MOST} bytes") });
            }
        }
        String::from_utf8(line).map_err(|_| Error::Modem { why: "a line of the modem is not UTF-8".into() })
    }

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.link.write_all(bytes).map_err(|e| Error::Modem { why: e.to_string() })
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.link.flush().map_err(|e| Error::Modem { why: e.to_string() })
    }
}

fn parse_count(word: &str, kind: &str) -> Result<usize, Error> {
    let text = word.strip_prefix(kind).map(str::trim).unwrap_or_default();
    text.parse::<usize>().map_err(|_| Error::Modem { why: format!("{kind} without a count of bytes") })
}

/// A session of the link: the ARQ of the session and the modem.
pub struct Session<L: Read + Write> {
    modem: Modem<L>,
    arq: Arq,
    policy: DeferArq,
    pending: Option<Vec<u8>>,
    attempts: u32,
}

impl<L: Read + Write> Session<L> {
    /// A session that defers each retransmission to the modem.
    pub fn open(link: L, baud: u32, policy: DeferArq) -> Result<Session<L>, Error> {
        if !(SPEED_LEAST..=SPEED_MOST).contains(&baud) {
            return Err(Error::Speed { bps: baud });
        }
        let arq = policy.arq(baud)?;
        Ok(Session { modem: Modem::new(link), arq, policy, pending: None, attempts: 0 })
    }

    pub fn modem(&self) -> &Modem<L> {
        &self.modem
    }

    pub fn modem_mut(&mut self) -> &mut Modem<L> {
        &mut self.modem
    }

    pub fn arq(&self) -> &Arq {
        &self.arq
    }

    /// The ARQ of the session, for the caller's check that it never
    /// retransmits under a [`DeferArq`] policy.
    pub fn arq_mut(&mut self) -> &mut Arq {
        &mut self.arq
    }

    pub fn policy(&self) -> DeferArq {
        self.policy
    }

    /// Send `payload` at `now`. It gives the attempts of this frame, so 2
    /// is one retransmission. A busy channel is an error, and the frame
    /// waits for the next call: the modem owns the retransmission. The
    /// modem's `OK` takes the frame out of the window of the session,
    /// because the modem now owns its delivery.
    pub fn send_at(&mut self, now: Instant, payload: &[u8]) -> Result<usize, Error> {
        if self.pending.is_none() {
            match self.arq.push_at(now, payload)? {
                Some(frame) => {
                    self.pending = Some(frame);
                    self.attempts = 0;
                }
                None => return Ok(0),
            }
        }
        let frame = self.pending.clone().ok_or(Error::Modem { why: "no frame waits".into() })?;
        let before = self.modem.writes();
        let sent = self.modem.transmit_at(now, &frame);
        if self.modem.writes() > before {
            self.attempts += 1;
        }
        sent?;
        let seq = seq_of(frame[0]);
        self.arq.ack(seq)?;
        self.pending = None;
        Ok(self.attempts as usize)
    }

    /// Read the next word or frame of the modem.
    pub fn receive_at(&mut self, now: Instant) -> Result<Option<Vec<u8>>, Error> {
        self.modem.receive_at(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sink(Vec<u8>);

    impl Read for Sink {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Ok(0)
        }
    }

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_speed_is_three_hundred_to_three_thousand() {
        for bps in [SPEED_LEAST, 1_200, SPEED_MOST] {
            assert!(DeferArq::new(3).arq(bps).is_ok());
        }
        assert!(DeferArq::new(3).arq(SPEED_LEAST).is_ok());
        assert_eq!(DeferArq::new(3).session_retransmits(), 0);
        assert_eq!(DeferArq::new(3).modem_retransmits(), 3);
    }

    #[test]
    fn a_speed_outside_the_range_is_refused() {
        let modem = Modem::new(Sink(Vec::new()));
        assert_eq!(modem.speed(), DEFAULT_SPEED);
        assert_eq!(modem.retransmissions(), 0);
        let mut modem = modem;
        assert_eq!(modem.set_speed(299).unwrap_err().name(), "speed");
        assert_eq!(modem.set_speed(3_001).unwrap_err().name(), "speed");
        assert_eq!(DeferArq::DEGRADED.session_retransmits(), 0);
    }

    #[test]
    fn the_policy_gives_a_deferring_arq() {
        let arq = DeferArq::new(2).arq(1_200).unwrap();
        assert!(arq.deferring_arq());
        assert_eq!(arq.window(), 1);
    }
}

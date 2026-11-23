//! The tests of the modem driver against a simulator that says `BUSY` and
//! `IDLE` over a memory link.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use pod_offline::Error;
use pod_offline::hf::{Activity, DeferArq, Modem, Session, TURNAROUND};
use pod_offline::serial::kiss;

enum Phase {
    Line,
    Data,
}

/// A modem that answers to the commands of the driver, and counts the
/// frames that came.
struct Sim {
    to_host: VecDeque<u8>,
    from_host: Vec<u8>,
    replies: VecDeque<&'static str>,
    phase: Phase,
    line: Vec<u8>,
    data: Vec<u8>,
    left: usize,
    tx: u32,
    frames: Vec<Vec<u8>>,
}

impl Sim {
    fn new(replies: &[&'static str]) -> Sim {
        Sim {
            to_host: VecDeque::new(),
            from_host: Vec::new(),
            replies: replies.iter().copied().collect(),
            phase: Phase::Line,
            line: Vec::new(),
            data: Vec::new(),
            left: 0,
            tx: 0,
            frames: Vec::new(),
        }
    }

    fn say(&mut self, text: &str) {
        self.to_host.extend(text.bytes());
    }

    fn command(&mut self, text: &str) {
        if let Some(rest) = text.strip_prefix("TX ") {
            let count: usize = rest.parse().expect("a count");
            self.tx += 1;
            self.phase = Phase::Data;
            self.left = count;
            self.data.clear();
        } else if text.starts_with("SPEED ") {
            let reply = self.replies.pop_front().unwrap_or("OK");
            self.say(&format!("{reply}\r"));
        }
    }

    fn data_end(&mut self) {
        self.frames.push(self.data.clone());
        let reply = self.replies.pop_front().unwrap_or("OK");
        self.say(&format!("{reply}\r"));
        self.phase = Phase::Line;
    }
}

impl Read for Sim {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.to_host.pop_front() {
            Some(byte) => {
                buf[0] = byte;
                Ok(1)
            }
            None => Err(io::Error::new(io::ErrorKind::WouldBlock, "the simulator has no answer")),
        }
    }
}

impl Write for Sim {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for &byte in buf {
            self.from_host.push(byte);
            match self.phase {
                Phase::Line => {
                    if byte == b'\r' {
                        let text = String::from_utf8_lossy(&self.line).into_owned();
                        self.line.clear();
                        self.command(&text);
                    } else {
                        self.line.push(byte);
                    }
                }
                Phase::Data => {
                    self.data.push(byte);
                    self.left -= 1;
                    if self.left == 0 {
                        self.data_end();
                    }
                }
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn no_transmit_while_the_modem_receives() {
    let mut sim = Sim::new(&[]);
    sim.say("BUSY\r");
    let mut modem = Modem::new(sim);
    let now = Instant::now();
    assert!(modem.receive_at(now).expect("a word").is_none());
    assert_eq!(modem.activity(), Activity::Receiving);
    let before = modem.link().from_host.len();
    let failed = modem.transmit_at(now + Duration::from_secs(30), b"frame").expect_err("a receiving modem");
    assert_eq!(failed.name(), "receiving");
    assert_eq!(modem.link().from_host.len(), before, "a byte went to the line while the modem received");
    assert_eq!(modem.link().tx, 0);
}

#[test]
fn the_modem_retries_once_and_the_session_does_not() {
    let sim = Sim::new(&["BUSY", "OK"]);
    let mut session = Session::open(sim, 1200, DeferArq::DEGRADED).expect("a session");
    let started = Instant::now();
    let failed = session.send_at(started, b"hello").expect_err("a busy first attempt");
    match failed {
        Error::Busy { retry_after } => assert_eq!(retry_after, TURNAROUND),
        other => panic!("the first attempt failed with {other}"),
    }
    assert_eq!(session.modem().link().tx, 1);

    session.modem_mut().link_mut().say("IDLE\r");
    assert!(session.receive_at(started + TURNAROUND).expect("the idle word").is_none());
    assert_eq!(session.modem().activity(), Activity::Idle);
    let writes = session.send_at(started + TURNAROUND, b"hello").expect("the second attempt");
    assert_eq!(writes, 2);
    assert_eq!(session.modem().link().tx, 2, "the modem wrote the frame twice");
    assert_eq!(session.modem().retransmissions(), 1);
    assert_eq!(session.arq().retransmissions(), 0);
    assert_eq!(session.policy().session_retransmits(), 0);
    assert_eq!(
        session.modem().retransmissions() + u32::from(session.policy().session_retransmits()),
        1,
        "one retransmission chain, not two"
    );
    assert!(
        session.arq_mut().poll_at(started + Duration::from_secs(3600)).expect("a poll").is_none(),
        "the session ARQ sent the frame again"
    );

    let frames = &session.modem().link().frames;
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0], frames[1], "the two attempts carried different frames");
    let decoded = kiss::Decoder::new().push(&frames[0]);
    let payload = &decoded[0].as_ref().expect("a frame").payload;
    assert_eq!(payload[0], 0, "the control byte of the first information frame");
    assert_eq!(&payload[1..], b"hello");
}

#[test]
fn the_modem_takes_the_frame_out_of_the_session_window() {
    let sim = Sim::new(&["OK", "OK"]);
    let mut session = Session::open(sim, 1200, DeferArq::DEGRADED).expect("a session");
    let now = Instant::now();
    assert_eq!(session.send_at(now, b"one").expect("the first frame"), 1);
    assert_eq!(session.arq().inflight(), 0);
    assert_eq!(session.send_at(now, b"two").expect("the second frame"), 1);
    assert_eq!(session.arq().inflight(), 0);
    let frames = &session.modem().link().frames;
    assert_eq!(frames.len(), 2);
    let first = kiss::Decoder::new().push(&frames[0]).remove(0).expect("a frame");
    let second = kiss::Decoder::new().push(&frames[1]).remove(0).expect("a frame");
    assert_eq!(first.payload[0], 0, "the sequence of the first frame");
    assert_eq!(second.payload[0], 2, "the sequence of the second frame");
    assert_eq!(&first.payload[1..], b"one");
    assert_eq!(&second.payload[1..], b"two");
}

#[test]
fn the_speed_command_is_ascii_and_its_answer_ok() {
    let mut modem = Modem::new(Sim::new(&["OK"]));
    modem.set_speed(500).expect("a speed");
    assert_eq!(modem.speed(), 500);
    assert_eq!(modem.link().from_host, b"SPEED 500\r");
}

#[test]
fn a_speed_outside_300_to_3000_is_refused_before_a_byte_goes_out() {
    let sim = Sim::new(&[]);
    assert_eq!(Session::open(sim, 299, DeferArq::new(1)).err().expect("a speed").name(), "speed");
    let mut modem = Modem::new(Sim::new(&[]));
    assert_eq!(modem.set_speed(3_001).expect_err("a speed").name(), "speed");
    assert_eq!(modem.set_speed(0).expect_err("a speed").name(), "speed");
    assert!(modem.link().from_host.is_empty());
}

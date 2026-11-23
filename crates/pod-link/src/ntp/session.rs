//! The session of the NTP carrier, without I/O: the bytes up that wait for
//! the next request, the bytes down that arrived, the extension field types
//! of each packet, and the clock discipline. The carrier's poll loop
//! (`client`) and the tests drive it the same way.

use std::collections::VecDeque;
use std::time::Duration;

use crate::carrier::Limit;

use super::wire::{self, Timestamp, Types};

/// The largest payload of one extension field: the 16-bit length of a field
/// less its 4-byte header.
pub const MAX_PAYLOAD: usize = u16::MAX as usize - wire::FIELD_HEADER;

/// The bytes of the two queues of a session: a full queue makes the writer
/// wait.
pub const MAX_QUEUE: usize = 1 << 20;

/// The clock discipline of the carrier (NTP-2): the offset that the local
/// clock takes from the server, and the uncertainty of the sample that set
/// it. The carrier steers its own timestamps, not the system clock.
#[derive(Debug, Clone)]
pub struct Discipline {
    /// The offset that the timestamps of this carrier take, and that the
    /// carrier calls disciplined inside.
    pub tolerance: Duration,
    offset: i128,
    best_delay: Option<i128>,
    samples: u64,
}

impl Discipline {
    /// A discipline that holds the sample of the smallest round trip, and
    /// whose uncertainty must be at most `tolerance`.
    pub fn new(tolerance: Duration) -> Discipline {
        Discipline { tolerance, offset: 0, best_delay: None, samples: 0 }
    }

    /// Take one exchange: `t1` and `t4` are the clock of this carrier,
    /// which the offset already steers, and `t2` and `t3` the server's,
    /// each in nanoseconds since 1900. The exchange gives a correction of
    /// the offset, and the smallest round trip gives the uncertainty.
    pub fn observe(&mut self, t1: i128, t2: i128, t3: i128, t4: i128) {
        let delay = (t4 - t1) - (t3 - t2);
        if delay < 0 {
            // A clock that went back in the exchange is no sample.
            return;
        }
        self.samples += 1;
        let correction = ((t2 - t1) + (t3 - t4)) / 2;
        self.offset = self.offset.saturating_add(correction);
        if self.best_delay.is_none_or(|best| delay < best) {
            self.best_delay = Some(delay);
        }
    }

    /// The offset that the local clock takes, in nanoseconds.
    pub fn offset(&self) -> i128 {
        self.offset
    }

    /// The smallest round trip that a sample saw, or `None` before one.
    pub fn delay(&self) -> Option<Duration> {
        self.best_delay.map(|n| Duration::from_nanos(n.unsigned_abs().min(u64::MAX as u128) as u64))
    }

    /// The uncertainty of the offset: half of the smallest round trip.
    pub fn uncertainty(&self) -> Option<Duration> {
        self.best_delay.map(|n| Duration::from_nanos((n / 2).unsigned_abs().min(u64::MAX as u128) as u64))
    }

    /// Whether the offset is known inside the tolerance (NTP-2).
    pub fn disciplined(&self) -> bool {
        self.uncertainty().is_some_and(|error| error <= self.tolerance)
    }

    /// The exchanges that the discipline took.
    pub fn samples(&self) -> u64 {
        self.samples
    }
}

/// The session: the bytes up of at most `payload` for each request, the
/// bytes down in order, and one request in flight.
#[derive(Debug)]
pub struct Session {
    payload: usize,
    poll: i8,
    types: Types,
    discipline: Discipline,
    up: VecDeque<u8>,
    down: VecDeque<u8>,
    flight: Option<(Timestamp, Vec<u8>)>,
    origin: Timestamp,
    reference: Timestamp,
    limit: Option<Limit>,
}

impl Session {
    /// A session of at most `payload` bytes for each request, which asks
    /// its server every `poll` seconds and holds its clock inside
    /// `tolerance`. `seed` picks the first extension field type.
    pub fn new(payload: usize, poll: Duration, tolerance: Duration, seed: u16) -> Session {
        let payload = payload.clamp(1, MAX_PAYLOAD);
        Session {
            payload,
            poll: poll_log(poll),
            types: Types::new(seed),
            discipline: Discipline::new(tolerance),
            up: VecDeque::new(),
            down: VecDeque::new(),
            flight: None,
            origin: Timestamp::ZERO,
            reference: Timestamp::ZERO,
            limit: None,
        }
    }

    /// Queue `bytes` for the next request; it takes what the queue holds.
    pub fn write(&mut self, bytes: &[u8]) -> usize {
        let room = MAX_QUEUE.saturating_sub(self.up.len());
        let taken = bytes.len().min(room);
        self.up.extend(&bytes[..taken]);
        taken
    }

    /// The bytes of the next request: at most the payload, in order, in one
    /// extension field of a new type. `now` is the clock of the host.
    pub fn request(&mut self, now: Timestamp) -> Result<Vec<u8>, wire::EncodeError> {
        let take = self.up.len().min(self.payload);
        let mut value = Vec::with_capacity(take);
        for _ in 0..take {
            if let Some(byte) = self.up.pop_front() {
                value.push(byte);
            }
        }
        let transmit = self.clock(now);
        let kind = self.types.take();
        let packet = wire::request(transmit, self.poll, self.origin, self.reference, kind, value.clone());
        let bytes = packet.encode()?;
        self.flight = Some((transmit, value));
        Ok(bytes)
    }

    /// Take the reply in `bytes`, at the clock `now` of the host: `Some` of
    /// the bytes that it delivered, or `None` for a reply to another
    /// request. A cut field is a carrier limit, and it reduces the payload
    /// (NTP-4).
    pub fn reply(&mut self, bytes: &[u8], now: Timestamp) -> Result<Option<usize>, wire::DecodeError> {
        let (packet, cut) = wire::parse_lenient(bytes)?;
        if packet.mode != wire::MODE_SERVER || packet.version != wire::VERSION {
            return Ok(None);
        }
        let Some((t1, _)) = self.flight.as_ref() else {
            return Ok(None);
        };
        let t1 = *t1;
        if packet.origin != t1 {
            return Ok(None);
        }
        let t4 = self.clock(now).nanos();
        self.discipline.observe(t1.nanos(), packet.receive.nanos(), packet.transmit.nanos(), t4);
        // The server took the bytes of this request: the client does not
        // send them again.
        self.flight = None;
        self.origin = packet.transmit;
        self.reference = packet.receive;
        let mut payload = packet.payload();
        if let Some(arrived) = cut {
            self.limit = Some(Limit::truncated(arrived as u64));
            self.payload = self.payload.min(arrived.max(1));
            payload.truncate(arrived);
        }
        let delivered = payload.len();
        self.down.extend(payload);
        Ok(Some(delivered))
    }

    /// Put the bytes of a request that got no reply back, for the next
    /// poll.
    pub fn retry(&mut self) {
        if let Some((_, sent)) = self.flight.take() {
            for byte in sent.into_iter().rev() {
                self.up.push_front(byte);
            }
        }
    }

    /// Take the bytes down, in order.
    pub fn read(&mut self, out: &mut [u8]) -> usize {
        let take = out.len().min(self.down.len());
        for slot in out.iter_mut().take(take) {
            if let Some(byte) = self.down.pop_front() {
                *slot = byte;
            }
        }
        take
    }

    /// The clock that the timestamps of this session name: the host's, with
    /// the offset of the discipline.
    pub fn clock(&self, now: Timestamp) -> Timestamp {
        now.shifted(self.discipline.offset())
    }

    pub fn payload(&self) -> usize {
        self.payload
    }

    /// The largest payload of the next requests.
    pub fn set_payload(&mut self, payload: usize) {
        self.payload = payload.clamp(1, MAX_PAYLOAD);
    }

    pub fn pending_up(&self) -> usize {
        self.up.len()
    }

    pub fn pending_down(&self) -> usize {
        self.down.len()
    }

    /// The last limit that the server of this session set.
    pub fn limit(&self) -> Option<Limit> {
        self.limit
    }

    pub fn discipline(&self) -> &Discipline {
        &self.discipline
    }
}

/// The `poll` field of the header: the log2 of the seconds of the poll.
fn poll_log(poll: Duration) -> i8 {
    let secs = poll.as_secs().clamp(16, 1 << 17);
    secs.ilog2() as i8
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn now_nanos(offset: i128) -> i128 {
        let duration = std::time::SystemTime::now().duration_since(UNIX_EPOCH).expect("a clock");
        (wire::EPOCH + i128::from(duration.as_secs())) * 1_000_000_000 + i128::from(duration.subsec_nanos()) + offset
    }

    #[test]
    fn the_discipline_takes_the_sample_of_the_smallest_round_trip() {
        let mut discipline = Discipline::new(Duration::from_millis(1));
        assert!(!discipline.disciplined());
        // A server 2 s ahead, with a slow sample first and a fast one after.
        // The clock of the carrier already takes the offset of the first
        // sample, as `Session::clock` gives it.
        let local = now_nanos(0);
        discipline.observe(
            local,
            local + 2_000_000_000 + 900_000,
            local + 2_000_000_000 + 1_000_000,
            local + 3_000_000,
        );
        assert!(!discipline.disciplined(), "a round trip of 2.9 ms is not inside 1 ms");
        let steered = local + discipline.offset();
        discipline.observe(
            steered,
            local + 2_000_000_000 + 300_000,
            local + 2_000_000_000 + 320_000,
            steered + 200_000,
        );
        assert!(discipline.disciplined(), "a round trip of 180 us is inside 1 ms");
        assert!((discipline.offset() - 2_000_000_000).abs() < 1_000_000, "{}", discipline.offset());
        assert_eq!(discipline.samples(), 2);
    }

    #[test]
    fn a_request_carries_the_payload_and_a_reply_delivers_another() {
        let mut session = Session::new(3, Duration::from_secs(32), Duration::from_millis(1), 7);
        assert_eq!(session.write(b"abcdefg"), 7);
        let now = Timestamp::now();
        let request = wire::parse(&session.request(now).expect("a request")).expect("a request packet");
        assert_eq!(request.payload(), b"abc");
        let answered = wire::reply(&request, now, now, now, 2, 0x2000, b"xyz".to_vec()).encode().expect("a reply");
        assert_eq!(session.reply(&answered, now).expect("a reply"), Some(3));
        let mut out = [0u8; 8];
        assert_eq!(session.read(&mut out), 3);
        assert_eq!(&out[..3], b"xyz");
        assert_eq!(session.pending_up(), 4, "the rest waits for the next request");
        // A reply to another request changes nothing.
        let stale = wire::reply(&request, now, now, now, 2, 0x2000, b"zzz".to_vec()).encode().expect("a reply");
        assert_eq!(session.reply(&stale, now).expect("a stale reply"), None);
    }

    #[test]
    fn a_cut_reply_is_a_limit_and_it_reduces_the_payload() {
        let mut session = Session::new(8, Duration::from_secs(16), Duration::from_millis(1), 3);
        session.write(b"12345678");
        let now = Timestamp::now();
        let request = wire::parse(&session.request(now).expect("a request")).expect("a request packet");
        let mut answered = wire::reply(&request, now, now, now, 2, 0x2000, vec![0; 8]).encode().expect("a reply");
        answered.truncate(answered.len() - 3);
        assert_eq!(session.reply(&answered, now).expect("a cut reply"), Some(5));
        assert_eq!(session.limit(), Some(Limit::truncated(5)));
        assert_eq!(session.payload(), 5);
    }
}

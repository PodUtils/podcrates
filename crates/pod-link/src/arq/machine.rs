//! The ARQ as a state machine with no I/O: the caller gives it the
//! datagrams of the carrier and the time, and takes the datagrams to send
//! and the bytes in order.

use std::time::{Duration, Instant};

use super::error::ArqError;
use super::recv::Receiver;
use super::send::{Body, Sender};
use super::settings::{ArqSettings, ArqSettingsError};
use super::stats::Stats;
use super::wire::{ACK_LEN, DATA_HEADER, Datagram, MIN_FRAGMENT};

/// One end of the selective-repeat ARQ. [`crate::arq::ArqStream`] drives
/// it over a [`crate::datagram::Datagrams`]; a caller with its own loop
/// drives it with these calls:
///
/// - [`Arq::on_datagram`] for each datagram of the carrier;
/// - [`Arq::poll_transmit`] until it gives `None`, and send each datagram;
/// - [`Arq::poll_timeout`], and [`Arq::on_timeout`] at that time;
/// - [`Arq::write`] and [`Arq::read`] (or the records) for the bytes.
///
/// After an error each call does nothing, and [`Arq::error`] gives it.
pub struct Arq {
    send: Sender,
    recv: Receiver,
    error: Option<ArqError>,
    stats: Stats,
    fragment: usize,
}

impl Arq {
    /// An end with `settings` over a carrier whose largest datagram has
    /// `mtu` bytes. A fragment is at most `mtu` less the header.
    pub fn new(settings: ArqSettings, mtu: usize) -> Result<Arq, ArqError> {
        settings.check()?;
        if mtu < ACK_LEN || mtu < DATA_HEADER + MIN_FRAGMENT {
            return Err(ArqSettingsError::Mtu(mtu).into());
        }
        let fragment = settings.fragment.min(mtu - DATA_HEADER);
        if settings.record.div_ceil(fragment) > usize::from(u16::MAX) {
            return Err(ArqSettingsError::Record(settings.record).into());
        }
        Ok(Arq {
            send: Sender::new(&settings, fragment),
            recv: Receiver::new(settings.start, settings.receive_buffer),
            error: None,
            stats: Stats::default(),
            fragment,
        })
    }

    /// Take one datagram of the carrier. A datagram that the ARQ does not
    /// take is dropped and counted ([`Stats::dropped`]).
    pub fn on_datagram(&mut self, now: Instant, bytes: &[u8]) {
        if self.error.is_some() {
            return;
        }
        let Ok(datagram) = Datagram::decode(bytes) else {
            self.stats.dropped += 1;
            return;
        };
        let result = match datagram {
            Datagram::Data { seq, index, count, payload } => {
                self.recv.on_frame(seq, Body::Data { index, count, payload }, &mut self.stats)
            }
            Datagram::Fin { seq } => self.recv.on_frame(seq, Body::Fin, &mut self.stats),
            Datagram::Ack { next, window, bitmap } => {
                if self.send.on_ack(now, next, window, &bitmap) {
                    self.stats.acks_taken += 1;
                } else {
                    self.stats.dropped += 1;
                }
                Ok(())
            }
            Datagram::Probe => {
                self.recv.ack_due = true;
                Ok(())
            }
        };
        if let Err(e) = result {
            self.error = Some(e);
        }
    }

    /// The next datagram to send: an `ACK` first, then the frames to send
    /// again, the new frames, and a probe. `None` when nothing goes now.
    pub fn poll_transmit(&mut self, now: Instant) -> Option<Vec<u8>> {
        if self.error.is_some() {
            return None;
        }
        if self.recv.ack_due {
            self.stats.acks_sent += 1;
            return Some(self.recv.ack().to_vec());
        }
        self.send.poll_transmit(now, &mut self.stats).map(|d| d.to_vec())
    }

    /// The time of the next timer, when one runs.
    pub fn poll_timeout(&self) -> Option<Instant> {
        if self.error.is_some() { None } else { self.send.poll_timeout() }
    }

    /// Run the timers at `now`. A frame with no retry left ends the ARQ
    /// with [`ArqError::Timeout`].
    pub fn on_timeout(&mut self, now: Instant) {
        if self.error.is_some() {
            return;
        }
        if let Err(e) = self.send.on_timeout(now, &mut self.stats) {
            self.error = Some(e);
        }
    }

    /// Queue bytes: one record of at most `record` bytes. It gives the
    /// bytes taken, 0 when the send buffer is full.
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize, ArqError> {
        self.check()?;
        let n = bytes.len().min(self.send.room());
        if n > 0 && self.send.push_record(&bytes[..n])? {
            self.stats.records_sent += 1;
            return Ok(n);
        }
        if self.send.write_closed() { Err(ArqError::WriteClosed) } else { Ok(0) }
    }

    /// Queue `record` whole. False when the send buffer has no room for it
    /// now. A record over [`ArqSettings::record`] never fits.
    pub fn send_record(&mut self, record: &[u8]) -> Result<bool, ArqError> {
        self.check()?;
        let queued = self.send.push_record(record)?;
        if queued && !record.is_empty() {
            self.stats.records_sent += 1;
        }
        Ok(queued)
    }

    /// Copy the bytes in order into `buf`. 0 when none waits (see
    /// [`Arq::read_finished`] for the end). The bytes that came go to the
    /// reader before an error.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, ArqError> {
        let n = self.recv.read(buf);
        if n == 0 && !buf.is_empty() && !self.recv.finished() {
            self.check()?;
        }
        Ok(n)
    }

    /// The next whole record of the peer, in order.
    pub fn recv_record(&mut self) -> Result<Option<Vec<u8>>, ArqError> {
        self.check()?;
        Ok(self.recv.record())
    }

    /// The bytes that a read gives now.
    pub fn readable(&self) -> usize {
        self.recv.readable()
    }

    /// The bytes that a write takes now.
    pub fn writable(&self) -> usize {
        self.send.room()
    }

    /// The peer ended its bytes, and the reader took each one.
    pub fn read_finished(&self) -> bool {
        self.recv.finished()
    }

    /// End the bytes of this end: a `FIN` after the queued records.
    pub fn close(&mut self) {
        self.send.close();
    }

    /// This end queued its `FIN`: no more writes.
    pub fn write_closed(&self) -> bool {
        self.send.write_closed()
    }

    /// The `FIN` of this end was acknowledged.
    pub fn closed(&self) -> bool {
        self.send.closed()
    }

    /// Nothing of this end waits for an acknowledgement.
    pub fn idle(&self) -> bool {
        self.send.idle()
    }

    /// Both ends sent their `FIN`, and each was acknowledged.
    pub fn done(&self) -> bool {
        self.send.closed() && self.recv.fin()
    }

    /// The sequence nears its wrap to 0: the caller must rekey, then call
    /// [`Arq::rekeyed`]. New frames stop at the wrap until then.
    pub fn rekey_due(&self) -> bool {
        self.send.rekey_due()
    }

    /// The caller rekeyed: the sequence can wrap to 0 one time.
    pub fn rekeyed(&mut self) {
        self.send.rekeyed();
    }

    /// The frames in flight.
    pub fn in_flight(&self) -> usize {
        self.send.in_flight()
    }

    /// The bytes of records that are not acknowledged.
    pub fn unacknowledged(&self) -> usize {
        self.send.queued_bytes()
    }

    /// The RTO of a frame sent now.
    pub fn rto(&self) -> Duration {
        self.send.rto()
    }

    /// The smoothed round trip, after the first sample.
    pub fn srtt(&self) -> Option<Duration> {
        self.send.rto.srtt()
    }

    /// The payload of one fragment.
    pub fn fragment(&self) -> usize {
        self.fragment
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Why the ARQ ended.
    pub fn error(&self) -> Option<&ArqError> {
        self.error.as_ref()
    }

    /// End the ARQ with `e`: a failure of the carrier.
    pub(crate) fn fail(&mut self, e: ArqError) {
        self.error.get_or_insert(e);
    }

    fn check(&self) -> Result<(), ArqError> {
        self.error.clone().map_or(Ok(()), Err)
    }
}

impl std::fmt::Debug for Arq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Arq")
            .field("fragment", &self.fragment)
            .field("in_flight", &self.in_flight())
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

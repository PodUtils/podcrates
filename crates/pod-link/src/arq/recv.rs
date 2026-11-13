//! The receive side of the ARQ: the frames out of order within the window,
//! the records put back together in order, the `ACK` with its bitmap, and
//! the window that this end gives the peer.

use std::collections::VecDeque;

use super::error::ArqError;
use super::send::Body;
use super::stats::Stats;
use super::wire::{Bitmap, Datagram, MAX_FRAGMENT, SACK_BITS, after, before};

/// The growth of the window, in frames, that sends an `ACK` at once after
/// a read.
const WINDOW_STEP: u16 = 64;

pub(crate) struct Receiver {
    /// The next frame in order: each frame before it came.
    next: u32,
    /// The frames from `next` on that came, at `seq % SACK_BITS`.
    slots: Vec<Option<Body>>,
    /// The record that is not whole: its bytes, the next index, the count.
    partial: Vec<u8>,
    index: u16,
    count: u16,
    /// The whole records, and the bytes of the first one already read.
    ready: VecDeque<Vec<u8>>,
    offset: usize,
    /// The bytes in order that the reader did not take.
    held: usize,
    receive_buffer: usize,
    /// The end came in order.
    fin: bool,
    pub(crate) ack_due: bool,
    /// The window of the last `ACK`.
    advertised: u16,
}

impl Receiver {
    pub(crate) fn new(start: u32, receive_buffer: usize) -> Receiver {
        let mut r = Receiver {
            next: start,
            slots: (0..SACK_BITS).map(|_| None).collect(),
            partial: Vec::new(),
            index: 0,
            count: 0,
            ready: VecDeque::new(),
            offset: 0,
            held: 0,
            receive_buffer,
            fin: false,
            ack_due: false,
            advertised: 0,
        };
        r.advertised = r.window();
        r
    }

    /// The frames that this end takes from `next` on.
    pub(crate) fn window(&self) -> u16 {
        let free = self.receive_buffer.saturating_sub(self.held) / MAX_FRAGMENT;
        free.min(SACK_BITS) as u16
    }

    /// Take a frame of the peer.
    pub(crate) fn on_frame(&mut self, seq: u32, body: Body, stats: &mut Stats) -> Result<(), ArqError> {
        self.ack_due = true;
        if before(seq, self.next) {
            stats.duplicates += 1;
            return Ok(());
        }
        let ahead = after(self.next, seq) as usize;
        if ahead >= SACK_BITS || self.fin {
            stats.dropped += 1;
            return Ok(());
        }
        let slot = &mut self.slots[seq as usize % SACK_BITS];
        if slot.is_some() {
            stats.duplicates += 1;
            return Ok(());
        }
        *slot = Some(body);
        stats.received += 1;
        while let Some(body) = self.slots[self.next as usize % SACK_BITS].take() {
            self.next = self.next.wrapping_add(1);
            self.deliver(body, stats)?;
            if self.fin {
                break;
            }
        }
        Ok(())
    }

    fn deliver(&mut self, body: Body, stats: &mut Stats) -> Result<(), ArqError> {
        let (index, count, payload) = match body {
            Body::Fin if self.count == 0 => {
                self.fin = true;
                self.slots.iter_mut().for_each(|s| *s = None);
                return Ok(());
            }
            Body::Fin => {
                return Err(ArqError::Protocol(format!(
                    "the end came after the fragment {} of a record of {}",
                    self.index, self.count
                )));
            }
            Body::Data { index, count, payload } => (index, count, payload),
        };
        if self.count == 0 && index == 0 {
            self.count = count;
        } else if index != self.index || count != self.count {
            let due = match self.count {
                0 => "the first fragment of a record".to_string(),
                n => format!("the fragment {} of {n}", self.index),
            };
            return Err(ArqError::Protocol(format!("the fragment {index} of {count} came where {due} was due")));
        }
        self.held += payload.len();
        if self.count == 1 {
            self.ready.push_back(payload);
        } else {
            self.partial.extend_from_slice(&payload);
        }
        self.index += 1;
        if self.index == self.count {
            if self.count > 1 {
                self.ready.push_back(std::mem::take(&mut self.partial));
            }
            self.index = 0;
            self.count = 0;
            stats.records_received += 1;
        }
        Ok(())
    }

    /// The `ACK` of the frames that came.
    pub(crate) fn ack(&mut self) -> Datagram {
        let mut bitmap = Bitmap::default();
        for i in 0..SACK_BITS {
            if self.slots[self.next.wrapping_add(i as u32) as usize % SACK_BITS].is_some() {
                bitmap.set(i);
            }
        }
        self.ack_due = false;
        self.advertised = self.window();
        Datagram::Ack { next: self.next, window: self.advertised, bitmap }
    }

    /// Copy bytes in order into `buf`, across records.
    pub(crate) fn read(&mut self, buf: &mut [u8]) -> usize {
        let mut n = 0;
        while n < buf.len() {
            let Some(front) = self.ready.front() else { break };
            let take = (front.len() - self.offset).min(buf.len() - n);
            buf[n..n + take].copy_from_slice(&front[self.offset..self.offset + take]);
            n += take;
            self.offset += take;
            if self.offset == front.len() {
                self.ready.pop_front();
                self.offset = 0;
            }
        }
        self.taken(n);
        n
    }

    /// The next whole record, or the rest of it after a [`Receiver::read`].
    pub(crate) fn record(&mut self) -> Option<Vec<u8>> {
        let mut record = self.ready.pop_front()?;
        record.drain(..std::mem::take(&mut self.offset));
        self.taken(record.len());
        Some(record)
    }

    fn taken(&mut self, n: usize) {
        self.held -= n;
        let now = self.window();
        if n > 0 && ((self.advertised == 0 && now > 0) || now >= self.advertised.saturating_add(WINDOW_STEP)) {
            self.ack_due = true;
        }
    }

    /// The bytes that a read gives now.
    pub(crate) fn readable(&self) -> usize {
        self.ready.iter().map(Vec::len).sum::<usize>() - self.offset
    }

    /// The end came, and the reader took each byte.
    pub(crate) fn finished(&self) -> bool {
        self.fin && self.ready.is_empty()
    }

    /// The end came in order.
    pub(crate) fn fin(&self) -> bool {
        self.fin
    }
}

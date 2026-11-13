//! The sealing side of the records: a record of the caller, with its
//! padding and its key's epoch, and the RESUME record.

use std::time::Instant;

use pod_core::entropy::Entropy;

use super::header::{HEADER, Header, MAX_LENGTH, PHASE, VERSION, bucket};
use super::keys::nonce;
use super::{Fault, RESUME, RESUME_BODY, Records};
use crate::TAG;

impl Records {
    /// Seal `payload` as a record of `kind` (any but [`RESUME`]) on `stream`,
    /// with the caller's `flags` (any but [`PHASE`]), at `now`: the bytes of
    /// the whole record. A key at its limit of records, of bytes or of age
    /// gives way to the next one first.
    pub fn seal(&mut self, now: Instant, kind: u8, flags: u16, stream: u32, payload: &[u8]) -> Result<Vec<u8>, Fault> {
        if kind == RESUME {
            return Err(Fault::Malformed("the type of a RESUME record is the layer's".into()));
        }
        if flags & PHASE != 0 {
            return Err(Fault::Malformed("the bit of the key's phase is the layer's".into()));
        }
        if payload.len() > MAX_LENGTH {
            return Err(Fault::Malformed(format!("a payload of {} bytes, above {MAX_LENGTH}", payload.len())));
        }
        let length = if self.settings.pad { bucket(payload.len()) } else { payload.len() };
        if self.out.counter == u64::MAX {
            return Err(Fault::Exhausted);
        }
        let out = &mut self.out;
        let s = &self.settings;
        let old = now.saturating_duration_since(out.since) >= s.rekey_age;
        if out.records >= s.rekey_records || out.bytes >= s.rekey_bytes || old {
            out.epoch = out.epoch.next(self.suite);
            (out.records, out.bytes, out.since) = (0, 0, now);
        }
        let header = Header {
            version: VERSION,
            kind,
            flags: flags | if out.epoch.phase() { PHASE } else { 0 },
            stream,
            counter: out.counter,
            // MAX_LENGTH fits u32, and a padding is below the last bucket.
            length: length as u32,
            pad: (length - payload.len()) as u16,
            reserved: 0,
        };
        let record = sealed(&header, payload, |n, aad, body| out.epoch.cipher.seal(&nonce(out.prefix, n), aad, body));
        out.counter += 1;
        out.records += 1;
        out.bytes += length as u64;
        Ok(record)
    }

    /// The RESUME record of this end for a new carrier: its token (used
    /// once), the counter that this end waits for next, a fresh prefix from
    /// `entropy`, which this end's records after it take, and the epoch of
    /// its key, which a peer that lost records of an epoch moves to. The
    /// peer opens it with its resume key, whatever prefix it holds: a RESUME
    /// that the lost carrier took is followed by the next one.
    pub fn resume(&mut self, entropy: &mut dyn Entropy) -> Result<Vec<u8>, Fault> {
        let mut fresh = [0u8; 4];
        entropy.fill(&mut fresh).map_err(|e| Fault::Malformed(e.to_string()))?;
        let out = &mut self.out;
        let index = out.resume;
        if index == u64::MAX {
            return Err(Fault::Exhausted);
        }
        let mut body = [0u8; RESUME_BODY];
        body[..32].copy_from_slice(&out.start.token(index)[..]);
        body[32..40].copy_from_slice(&self.inn.window.next().to_be_bytes());
        body[40..44].copy_from_slice(&fresh);
        body[44..].copy_from_slice(&out.epoch.number.to_be_bytes());
        let header = Header {
            version: VERSION,
            kind: RESUME,
            flags: 0,
            stream: 0,
            counter: index,
            length: RESUME_BODY as u32,
            pad: 0,
            reserved: 0,
        };
        // The resume key seals only RESUME records, each with its own number.
        let record = sealed(&header, &body, |n, aad, b| out.start.resume.seal(&nonce([0; 4], n), aad, b));
        out.resume += 1;
        out.prefix = fresh;
        Ok(record)
    }
}

/// The bytes of a record of `header` with `payload` and its zero padding,
/// sealed by `seal` (of the counter, the associated data and the body).
fn sealed(header: &Header, payload: &[u8], seal: impl FnOnce(u64, &[u8], &mut [u8]) -> [u8; TAG]) -> Vec<u8> {
    let length = header.length as usize;
    let mut record = Vec::with_capacity(HEADER + length + TAG);
    record.extend_from_slice(&header.encode());
    record.extend_from_slice(payload);
    record.resize(HEADER + length, 0);
    let (aad, body) = record.split_at_mut(HEADER);
    let tag = seal(header.counter, aad, body);
    record.extend_from_slice(&tag);
    record
}

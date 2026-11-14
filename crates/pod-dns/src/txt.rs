//! The TXT and CNAME answers of RFC 1035 for a carrier of bytes over DNS
//! (PodUtils/podnet#479): a query with an ID of the caller and an EDNS0
//! OPT record with a size (RFC 6891), and the reader of its answer: the
//! strings of each TXT record, the target of each CNAME record, the TC bit,
//! the full RCODE and the size of the server. By hand, as [`crate::wire`]:
//! a parser of untrusted bytes that the fuzzer reaches
//! (`fuzz/fuzz_targets/dns_wire.rs`).
//!
//! The transport (UDP, TCP with its length prefix, the retry of a cut
//! answer) is the caller's.

use crate::wire::{self, read_name};

/// The TXT record (RFC 1035, section 3.3.14).
pub const TXT: u16 = 16;
/// The CNAME record (RFC 1035, section 3.3.1).
pub const CNAME: u16 = 5;
/// The OPT pseudo-record of EDNS0 (RFC 6891, section 6.1.1).
pub const OPT: u16 = 41;
/// The largest answer over UDP with no EDNS0 (RFC 1035, section 4.2.1).
pub const PLAIN_SIZE: u16 = 512;
/// The size of EDNS0 that DNS Flag Day 2020 gives: no IP fragment on most
/// paths.
pub const EDNS_SIZE: u16 = 1232;

const HEADER: usize = 12;
const RESPONSE: u16 = 0x8000;
/// The TC bit: the server cut the answer to the size of the transport.
const TRUNCATED: u16 = 0x0200;
const CUT: &str = "not a DNS message: it ends before its last record";

/// The query of `kind` for `name` with the ID `id`, recursion desired, and
/// with `edns` an OPT record that gives the size of the largest answer that
/// the caller takes (512 or more, RFC 6891 section 6.2.5). A name that a
/// resolver should not be asked for is refused.
pub fn query(id: u16, name: &str, kind: u16, edns: Option<u16>) -> Result<Vec<u8>, String> {
    let mut out = wire::query(name, kind)?;
    out[..2].copy_from_slice(&id.to_be_bytes());
    if let Some(size) = edns {
        if size < PLAIN_SIZE {
            return Err(format!("an EDNS0 size of {size}: give {PLAIN_SIZE} or more"));
        }
        // ARCOUNT 1: the OPT record.
        out[11] = 1;
        // The root name, the type, the size in the class, the extended
        // RCODE, the version and the flags 0, and no options.
        out.push(0);
        out.extend_from_slice(&OPT.to_be_bytes());
        out.extend_from_slice(&size.to_be_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    }
    Ok(out)
}

/// The ID, the name and the type of the first question of a query, for a
/// server that answers [`query`]. The name is in lower case with no final
/// dot. A message with no question, or one that ends in its question, is
/// refused.
pub fn question(msg: &[u8]) -> Result<(u16, String, u16), String> {
    let header = msg.get(..HEADER).ok_or("not a DNS message: shorter than its header")?;
    let word = |at: usize| u16::from_be_bytes([header[at], header[at + 1]]);
    if word(4) == 0 {
        return Err("the query has no question".into());
    }
    let (name, end) = read_name(msg, HEADER)?;
    let fixed = msg.get(end..end + 4).ok_or(CUT)?;
    let kind = u16::from_be_bytes([fixed[0], fixed[1]]);
    Ok((word(0), name.to_ascii_lowercase(), kind))
}

/// The EDNS0 size of the query `msg`: the size of its OPT record, at least
/// 512 (RFC 6891, section 6.2.5). `None` when it has no OPT record: the
/// answer then takes [`PLAIN_SIZE`] bytes and no OPT record. A message that
/// ends in its question or its records gives `None`. For a server that
/// answers [`query`].
pub fn asked_size(msg: &[u8]) -> Option<u16> {
    let header = msg.get(..HEADER)?;
    let word = |at: usize| u16::from_be_bytes([header[at], header[at + 1]]);
    let mut at = HEADER;
    for _ in 0..word(4) {
        let (_, end) = read_name(msg, at).ok()?;
        at = end + 4;
    }
    let records = u32::from(word(6)) + u32::from(word(8)) + u32::from(word(10));
    for _ in 0..records {
        let Raw { rtype, class, start, len, .. } = record_at(msg, at).ok()??;
        if rtype == OPT {
            return Some(class.max(PLAIN_SIZE));
        }
        at = start + len;
    }
    None
}

/// An answer to the query `id` for `name` of type `kind`, with one record
/// for each item of `records` (each cut into strings of 255 bytes for TXT),
/// recursion available, no error, and an OPT record of `edns` bytes when
/// given. The owner of each record is a pointer to the question. For a
/// server that answers [`query`].
pub fn answer(id: u16, name: &str, kind: u16, records: &[&[u8]], edns: Option<u16>) -> Result<Vec<u8>, String> {
    let mut out = wire::query(name, kind)?;
    out[..2].copy_from_slice(&id.to_be_bytes());
    // QR and RA set, RD kept, RCODE 0.
    out[2] |= 0x80;
    out[3] |= 0x80;
    out[6..8].copy_from_slice(&(records.len() as u16).to_be_bytes());
    if edns.is_some() {
        out[11] = 1;
    }
    for record in records {
        // The owner points to the question name at offset 12.
        out.extend_from_slice(&[0xc0, 0x0c]);
        out.extend_from_slice(&kind.to_be_bytes());
        // Class IN, TTL 0.
        out.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
        let data = if kind == TXT { txt_strings(record) } else { record.to_vec() };
        if data.len() > u16::MAX as usize {
            return Err(format!("a record of {} bytes: give {} or less", data.len(), u16::MAX));
        }
        out.extend_from_slice(&(data.len() as u16).to_be_bytes());
        out.extend_from_slice(&data);
    }
    if let Some(size) = edns {
        if size < PLAIN_SIZE {
            return Err(format!("an EDNS0 size of {size}: give {PLAIN_SIZE} or more"));
        }
        out.push(0);
        out.extend_from_slice(&OPT.to_be_bytes());
        out.extend_from_slice(&size.to_be_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    }
    Ok(out)
}

/// The data of a TXT record of `bytes`: each 255 bytes as one character
/// string, with a last string for the rest (and one empty string for no
/// byte, so an empty record has a length of 1).
fn txt_strings(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 255 + 1);
    let mut chunks = bytes.chunks(255);
    loop {
        let chunk = chunks.next().unwrap_or(&[]);
        out.push(chunk.len() as u8);
        out.extend_from_slice(chunk);
        if chunk.len() < 255 {
            return out;
        }
    }
}

/// One record of the answer section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Record {
    /// The strings of a TXT record, each 255 bytes at most, in order.
    Txt(Vec<Vec<u8>>),
    /// The target of a CNAME record, in lower case with no final dot.
    Cname(String),
}

/// The answer to a query of [`query`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub id: u16,
    /// The TC bit: the server cut the answer. `records` holds the records
    /// before the cut.
    pub truncated: bool,
    /// The RCODE with the 8 bits of the OPT record above the 4 bits of the
    /// header (RFC 6891, section 6.1.3): 0 is no error, 2 SERVFAIL, 3
    /// NXDOMAIN.
    pub rcode: u16,
    /// The size of the largest answer that the server takes (its OPT
    /// record); `None` when the answer has no OPT record.
    pub edns: Option<u16>,
    /// The TXT and CNAME records of class IN of the answer section, in
    /// order. A CNAME before a TXT is the chain to its name.
    pub records: Vec<Record>,
}

impl Answer {
    /// The bytes of each TXT record: its strings joined.
    pub fn txt(&self) -> Vec<Vec<u8>> {
        let joined = |strings: &Vec<Vec<u8>>| strings.concat();
        self.records.iter().filter_map(|r| if let Record::Txt(s) = r { Some(joined(s)) } else { None }).collect()
    }

    /// The target of each CNAME record.
    pub fn cnames(&self) -> Vec<&str> {
        self.records.iter().filter_map(|r| if let Record::Cname(n) = r { Some(n.as_str()) } else { None }).collect()
    }
}

/// The answer in `msg` to the query of [`query`] with `id`, `name` and
/// `kind`. A query, another ID, and an answer to another question are
/// refused. A message cut in its records is refused unless the TC bit is
/// set: then the records before the cut are the answer. A TXT record whose
/// strings do not fill its data, or a CNAME whose target is not a name, is
/// refused.
pub fn read(msg: &[u8], id: u16, name: &str, kind: u16) -> Result<Answer, String> {
    let header = msg.get(..HEADER).ok_or("not a DNS message: shorter than its header")?;
    let word = |at: usize| u16::from_be_bytes([header[at], header[at + 1]]);
    if (word(2) & RESPONSE) == 0 {
        return Err("not a DNS answer: the message is a query".into());
    }
    if word(0) != id {
        return Err(format!("the answer has the ID {}, not {id}", word(0)));
    }
    let truncated = (word(2) & TRUNCATED) != 0;
    let mut answer = Answer { id, truncated, rcode: word(2) & 0x000F, edns: None, records: Vec::new() };
    let mut at = HEADER;
    for i in 0..word(4) {
        let (asked, end) = read_name(msg, at)?;
        let fixed = msg.get(end..end + 4).ok_or(CUT)?;
        let asked_kind = u16::from_be_bytes([fixed[0], fixed[1]]);
        if i == 0 && (!asked.eq_ignore_ascii_case(name.trim_end_matches('.')) || asked_kind != kind) {
            return Err(format!("the answer is for another question ({asked} type {asked_kind})"));
        }
        at = end + 4;
    }
    let sections = [word(6), word(8), word(10)];
    for (section, count) in sections.into_iter().enumerate() {
        for _ in 0..count {
            let Some(Raw { rtype, class, ttl, start, len }) = record_at(msg, at)? else {
                // A cut answer ends where the server cut it.
                return if truncated { Ok(answer) } else { Err(CUT.into()) };
            };
            let data = &msg[start..start + len];
            match (section, rtype) {
                (0, TXT) if class == 1 => answer.records.push(Record::Txt(strings(data)?)),
                (0, CNAME) if class == 1 => {
                    let (target, end) = read_name(msg, start)?;
                    if end != start + len || target.is_empty() {
                        return Err("a CNAME record holds no name of its own length".into());
                    }
                    answer.records.push(Record::Cname(target));
                }
                (2, OPT) => {
                    answer.edns = Some(class);
                    answer.rcode |= u16::from((ttl >> 24) as u8) << 4;
                }
                _ => {}
            }
            at = start + len;
        }
    }
    Ok(answer)
}

/// The fixed fields of one record, and where its data is.
struct Raw {
    rtype: u16,
    class: u16,
    ttl: u32,
    start: usize,
    len: usize,
}

/// The record at `at`; `None` when the message ends before the end of the
/// record.
fn record_at(msg: &[u8], at: usize) -> Result<Option<Raw>, String> {
    let end = match skip(msg, at) {
        Some(end) => end?,
        None => return Ok(None),
    };
    let Some(fixed) = msg.get(end..end + 10) else { return Ok(None) };
    let number = |i: usize| u16::from_be_bytes([fixed[i], fixed[i + 1]]);
    let ttl = u32::from_be_bytes([fixed[4], fixed[5], fixed[6], fixed[7]]);
    let (start, len) = (end + 10, usize::from(number(8)));
    if msg.len() - start < len {
        return Ok(None);
    }
    Ok(Some(Raw { rtype: number(0), class: number(2), ttl, start, len }))
}

/// The offset after the owner name at `at`; `None` when the message ends in
/// it.
fn skip(msg: &[u8], mut at: usize) -> Option<Result<usize, String>> {
    loop {
        let len = *msg.get(at)?;
        match len & 0xC0 {
            0x00 if len == 0 => return Some(Ok(at + 1)),
            0x00 => at += 1 + usize::from(len),
            0xC0 => return msg.get(at + 1).map(|_| Ok(at + 2)),
            _ => return Some(Err("a name holds a label of a type that RFC 1035 does not define".into())),
        }
    }
}

/// The character strings of the data of a TXT record: each a length byte
/// and its bytes, which fill the data exactly.
fn strings(mut data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let mut out = Vec::new();
    while let Some((&len, rest)) = data.split_first() {
        let len = usize::from(len);
        let text = rest.get(..len).ok_or("a TXT string ends past its record")?;
        out.push(text.to_vec());
        data = &rest[len..];
    }
    Ok(out)
}

#[cfg(test)]
mod tests;

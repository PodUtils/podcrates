//! The first record of a new session of the DNS listener: the ClientHello of
//! TLS, in `DATA` frames of the ARQ at the first sequences. A query of a new
//! session id takes a handshake slot only when it holds a part of that
//! record, and the listener reads the whole record before it starts the
//! handshake, so a query of junk holds no slot (T-1165). The parts can come
//! in any order: the dialer sends its queries at once, and a resolver can
//! reorder them.

use crate::arq::wire::{DATA_HEADER, Datagram};

/// The most parts of a first record that a session holds before its
/// handshake: a ClientHello of 4 KiB in the smallest fragments of the ARQ
/// (32 bytes).
pub(super) const MAX_PARTS: u16 = 128;

/// The content type of a TLS handshake record (RFC 8446, section 5.1).
const HANDSHAKE: u8 = 0x16;

/// The type of a ClientHello message (RFC 8446, section 4).
const CLIENT_HELLO: u8 = 0x01;

/// One part of the first record: the fragment `index` of `count`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Part {
    pub(super) index: u16,
    pub(super) count: u16,
}

/// The part of the first record that the datagram `datagram` of the ARQ
/// holds: a `DATA` frame at the sequence `start` plus its index, of at most
/// [`MAX_PARTS`] parts. The part 0 must open a TLS handshake record of a
/// ClientHello. `None` for each other datagram (a poll, an `ACK`, junk).
pub(super) fn part(datagram: &[u8], start: u32) -> Option<Part> {
    let Ok(Datagram::Data { seq, index, count, payload }) = Datagram::decode(datagram) else {
        return None;
    };
    if seq.wrapping_sub(start) != u32::from(index) || count > MAX_PARTS {
        return None;
    }
    // The record header (type, version, length), then the type of the
    // first handshake message.
    let opens = payload.len() > 5 && payload[0] == HANDSHAKE && payload[1] == 0x03 && payload[5] == CLIENT_HELLO;
    (index != 0 || opens).then_some(Part { index, count })
}

/// What the whole first record shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hello {
    /// A ClientHello that offers the ALPN of the link.
    Offers,
    /// No ClientHello, or one with no offer of the ALPN of the link.
    Refused,
    /// The ClientHello goes on past the first record: TLS checks it.
    Open,
}

/// The parts of the first record of one session, as they come.
#[derive(Debug)]
pub(super) struct Parts {
    slots: Vec<Option<Vec<u8>>>,
    missing: usize,
}

impl Parts {
    /// The parts of a record of `count` parts, none come yet.
    pub(super) fn new(count: u16) -> Parts {
        Parts { slots: vec![None; usize::from(count)], missing: usize::from(count) }
    }

    /// Keep the datagram `datagram` of the part `part`. A part of another
    /// count is no part of this record: false.
    pub(super) fn add(&mut self, part: Part, datagram: Vec<u8>) -> bool {
        if usize::from(part.count) != self.slots.len() {
            return false;
        }
        let slot = &mut self.slots[usize::from(part.index)];
        if slot.is_none() {
            self.missing -= 1;
        }
        *slot = Some(datagram);
        true
    }

    /// Each part came.
    pub(super) fn complete(&self) -> bool {
        self.missing == 0
    }

    /// The datagrams in order, and what the record shows for `alpn`. Only
    /// for a complete record.
    pub(super) fn take(self, alpn: &[u8]) -> (Vec<Vec<u8>>, Hello) {
        let datagrams: Vec<Vec<u8>> = self.slots.into_iter().flatten().collect();
        let record: Vec<u8> = datagrams.iter().flat_map(|d| d[DATA_HEADER..].iter().copied()).collect();
        let hello = read(&record, alpn);
        (datagrams, hello)
    }
}

/// What the bytes `record` show: the parser of rustls reads the ClientHello.
fn read(record: &[u8], alpn: &[u8]) -> Hello {
    let mut acceptor = rustls::server::Acceptor::default();
    let mut rest = record;
    while !rest.is_empty() {
        match acceptor.read_tls(&mut rest) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => return Hello::Refused,
        }
    }
    match acceptor.accept() {
        Ok(Some(accepted)) => {
            let offered = accepted.client_hello().alpn().is_some_and(|mut each| each.any(|p| p == alpn));
            if offered { Hello::Offers } else { Hello::Refused }
        }
        Ok(None) => Hello::Open,
        Err(_) => Hello::Refused,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// The first TLS bytes of a client that offers `alpn`.
    fn client_hello(alpn: &[u8]) -> Vec<u8> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
        config.alpn_protocols = vec![alpn.to_vec()];
        let name = rustls::pki_types::ServerName::try_from("l.test").unwrap();
        let mut conn = rustls::ClientConnection::new(Arc::new(config), name).unwrap();
        let mut out = Vec::new();
        conn.write_tls(&mut out).unwrap();
        out
    }

    /// The `DATA` frames of `record` in parts of `size` bytes from the
    /// sequence `start`.
    fn frames(record: &[u8], size: usize, start: u32) -> Vec<(Part, Vec<u8>)> {
        let count = record.len().div_ceil(size) as u16;
        let each = record.chunks(size).enumerate().map(|(i, chunk)| {
            let index = i as u16;
            let frame = Datagram::Data { seq: start + u32::from(index), index, count, payload: chunk.to_vec() };
            (Part { index, count }, frame.to_vec())
        });
        each.collect()
    }

    #[test]
    fn a_first_part_opens_a_client_hello_and_each_part_is_of_the_first_record() {
        let record = client_hello(b"podlink/1");
        for (want, frame) in frames(&record, 100, 7) {
            assert_eq!(part(&frame, 7), Some(want), "{want:?}");
            assert_eq!(part(&frame, 0), None, "another start: {want:?}");
        }
        let junk = Datagram::Data { seq: 0, index: 0, count: 1, payload: b"GET / HTTP/1.1".to_vec() };
        assert_eq!(part(&junk.to_vec(), 0), None, "a part 0 that opens no ClientHello");
        let many = Datagram::Data { seq: 1, index: 1, count: MAX_PARTS + 1, payload: vec![0; 40] };
        assert_eq!(part(&many.to_vec(), 0), None, "more parts than a session holds");
        for other in [&[][..], &[4u8][..], &[3u8; 39][..], &b"\x16\x03\x01\x00\x10\x01"[..]] {
            assert_eq!(part(other, 0), None, "{other:?}");
        }
    }

    #[test]
    fn a_whole_record_in_any_order_shows_the_offer_of_the_alpn() {
        let record = client_hello(b"podlink/1");
        let mut parts = frames(&record, 64, 0);
        assert!(parts.len() > 2, "a ClientHello of {} bytes", record.len());
        parts.reverse();
        let mut kept = Parts::new(parts[0].0.count);
        for (part, frame) in parts.clone() {
            assert!(!kept.complete());
            assert!(kept.add(part, frame));
        }
        assert!(kept.complete());
        let (datagrams, hello) = kept.take(b"podlink/1");
        assert_eq!(hello, Hello::Offers);
        let ordered: Vec<Vec<u8>> = parts.iter().rev().map(|(_, f)| f.clone()).collect();
        assert_eq!(datagrams, ordered, "the datagrams in the order of the record");

        let fill = |parts: &[(Part, Vec<u8>)]| {
            let mut kept = Parts::new(parts[0].0.count);
            parts.iter().for_each(|(p, f)| assert!(kept.add(*p, f.clone())));
            kept
        };
        assert_eq!(fill(&parts).take(b"other/1").1, Hello::Refused, "another ALPN");
        let first = frames(&record[..record.len() - 10], 64, 0);
        assert_eq!(fill(&first).take(b"podlink/1").1, Hello::Open, "a ClientHello past the record");
        let mut bad = record.clone();
        bad[0] = 0x17;
        assert_eq!(fill(&frames(&bad, 64, 0)).take(b"podlink/1").1, Hello::Refused, "a record of application data");
        let mut other = Parts::new(3);
        assert!(!other.add(Part { index: 0, count: 2 }, Vec::new()), "a part of another count");
    }
}

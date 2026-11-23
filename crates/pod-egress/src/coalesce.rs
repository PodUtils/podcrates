//! One first write of 3,333 bytes: the opening request of a VLESS session
//! and its first bytes of application data, together, padded when the data
//! is short. A session that names its target in a short header and then
//! waits for the first bytes of a slow link spends a whole round trip on a
//! write of a few dozen bytes; at 572 bit/s the 3,333-byte write fills the
//! link for 47 s, so the padding is a property of the pacing rule and not of
//! the protocol ([`TARGET`]).
//!
//! The padding is an `x-padding` field of the request's head, a field that
//! the server ignores: a WebSocket server ignores a header of the upgrade,
//! and Xray-core's XHTTP server reads its own `x_padding` query and no other.
//! The bytes of the field are therefore never delivered to the peer as
//! application data, and the first write that carries data ends at the
//! target exactly.
//!
//! [`First`] holds the one-time rule: a session coalesces once, and a second
//! call returns the bytes as they came. Two coalesced writes are a stall.

/// The bytes of the first write that carries data: the opening request and
/// the first application data, padding among them.
pub const TARGET: usize = 3_333;

/// The name of the field that carries the padding. The server ignores it.
pub const PAD_HEADER: &str = "x-padding";

/// The bytes that [`PAD_HEADER`] costs in a head: its name, `": "`, and its
/// line's CRLF, with no filler byte.
pub const FIELD_OVERHEAD: usize = PAD_HEADER.len() + 4;

/// The filler bytes of the padding, and whether the head and the data alone
/// fall short of [`TARGET`]: a short first write is padded and said.
///
/// `head` and `data` are counts of bytes: the head with its request line and
/// each field, and the application data that follows it. A room smaller than
/// [`FIELD_OVERHEAD`] takes no field, and the write stays short.
pub fn padding(head: usize, data: usize) -> (usize, bool) {
    let room = TARGET.saturating_sub(head + data);
    let short = room > 0;
    let filler = room.saturating_sub(FIELD_OVERHEAD);
    (if room >= FIELD_OVERHEAD { filler } else { 0 }, short)
}

/// The first write of a session whose head is an HTTP request's bytes: the
/// head with the padding's field before its last CRLF, then `data`, in one
/// buffer of [`TARGET`] bytes where the head and the data leave room. The
/// boolean is `short`, as [`padding`] gives it.
///
/// A head that does not end with the empty line of an HTTP request takes no
/// field: the bytes come back as they are, with `data` after them.
pub fn first_write(head: &[u8], data: &[u8]) -> (Vec<u8>, bool) {
    let (filler, short) = padding(head.len(), data.len());
    let mut out = Vec::with_capacity(TARGET.max(head.len() + data.len()));
    if filler == 0 || !head.ends_with(b"\r\n\r\n") {
        out.extend_from_slice(head);
        out.extend_from_slice(data);
        return (out, short);
    }
    // Before the head's empty line: its last two bytes are that line.
    let at = head.len() - 2;
    out.extend_from_slice(&head[..at]);
    out.extend_from_slice(PAD_HEADER.as_bytes());
    out.extend_from_slice(b": ");
    out.extend(std::iter::repeat_n(b'X', filler));
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&head[at..]);
    out.extend_from_slice(data);
    (out, short)
}

/// The frame of a first write that a caller hands to a stream: the bytes,
/// and what they were.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Report {
    /// The bytes of the first write that carried data.
    pub bytes: usize,
    /// The head and the data were short of [`TARGET`], and padding filled
    /// the rest where the room allowed the field.
    pub short: bool,
    /// A session's first write was coalesced.
    pub coalesced: bool,
}

/// The one coalesced write of a session. The first call returns the head and
/// the data as one frame; each call after it returns its data unchanged, so
/// that a stall cannot follow a stall.
#[derive(Debug, Default)]
pub struct First {
    report: Report,
}

impl First {
    pub const fn new() -> First {
        First { report: Report { bytes: 0, short: false, coalesced: false } }
    }

    /// The one write: the head and `data` in one frame on the first call,
    /// with the boolean saying that this call coalesced. A later call
    /// returns `data` alone, with `false`.
    pub fn write(&mut self, head: &[u8], data: &[u8]) -> (Vec<u8>, bool) {
        if self.report.coalesced {
            return (data.to_vec(), false);
        }
        let (frame, short) = first_write(head, data);
        self.report = Report { bytes: frame.len(), short, coalesced: true };
        (frame, true)
    }

    /// What the first write was.
    pub fn report(&self) -> Report {
        self.report
    }

    /// Whether this session coalesced its first write.
    pub fn coalesced(&self) -> bool {
        self.report.coalesced
    }

    /// Whether the head and the data alone fell short of the target.
    pub fn short(&self) -> bool {
        self.report.short
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};
    use tokio::io::{AsyncWrite, AsyncWriteExt};

    /// A stream that records each write whole, as a peer reads it.
    #[derive(Clone, Default)]
    struct Recording {
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl Recording {
        fn writes(&self) -> Vec<Vec<u8>> {
            self.writes.lock().unwrap_or_else(|p| p.into_inner()).clone()
        }
    }

    impl AsyncWrite for Recording {
        fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, data: &[u8]) -> Poll<std::io::Result<usize>> {
            self.writes.lock().unwrap_or_else(|p| p.into_inner()).push(data.to_vec());
            Poll::Ready(Ok(data.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    fn head() -> Vec<u8> {
        b"POST /xh/ HTTP/1.1\r\nHost: v.example\r\nContent-Type: application/grpc\r\n\r\n".to_vec()
    }

    /// The request as the peer reads it: the head's fields, and the bytes
    /// after the empty line.
    fn split(frame: &[u8]) -> (Vec<(String, String)>, Vec<u8>) {
        let end = frame.windows(4).position(|w| w == b"\r\n\r\n").expect("an empty line") + 4;
        let text = String::from_utf8(frame[..end].to_vec()).unwrap();
        let fields = text
            .split("\r\n")
            .skip(1)
            .filter(|line| !line.is_empty())
            .map(|line| {
                let (name, value) = line.split_once(':').expect("a field");
                (name.trim().to_string(), value.trim().to_string())
            })
            .collect();
        (fields, frame[end..].to_vec())
    }

    /// COALESCE-1: the write that carries the opening request and the first
    /// data is one write of the target, and the data is what follows it.
    #[tokio::test]
    async fn the_first_write_is_one_write_of_the_target_with_the_data_after_the_head() {
        let mut stream = Recording::default();
        let mut first = First::new();
        let request = b"\x00the VLESS request";
        let (frame, coalesced) = first.write(&head(), request);
        assert!(coalesced);
        stream.write_all(&frame).await.unwrap();
        let writes = stream.writes();
        assert_eq!(writes.len(), 1, "one write");
        assert_eq!(writes[0].len(), TARGET, "the first write is exactly the target");
        assert_eq!(first.report(), Report { bytes: TARGET, short: true, coalesced: true });
        let (fields, data) = split(&writes[0]);
        assert_eq!(data, request, "the padding is no application data");
        let padding = fields.iter().find(|(name, _)| name == PAD_HEADER).expect("the padding's field");
        assert!(padding.1.bytes().all(|b| b == b'X') && !padding.1.is_empty(), "{padding:?}");
        assert_eq!(fields[0].0, "Host", "the fields of the head come first, in their order");
    }

    /// COALESCE-2: a short write is padded to the target and reported, and a
    /// write whose data fills the target is not.
    #[test]
    fn a_short_first_write_is_padded_and_said() {
        let (frame, short) = first_write(&head(), b"");
        assert!(short && frame.len() == TARGET, "a short write is padded: {}", frame.len());
        let (whole, short) = first_write(&head(), &vec![7u8; TARGET - head().len()]);
        assert!(!short && whole.len() == TARGET && !whole.windows(9).any(|w| w == b"x-padding"));
        // The room is smaller than the field: the write stays short and the
        // head takes no field.
        let (nearly, short) = first_write(&head(), &vec![7u8; TARGET - head().len() - 1]);
        assert!(short && nearly.len() == TARGET - 1, "{}", nearly.len());
        assert!(!nearly.windows(9).any(|w| w == b"x-padding"));
    }

    /// COALESCE-3: the second write is the data alone, and the session says
    /// that its first write was the coalesced one.
    #[tokio::test]
    async fn a_second_write_is_not_coalesced_again() {
        let mut stream = Recording::default();
        let mut first = First::new();
        let (frame, coalesced) = first.write(&head(), b"one");
        assert!(coalesced);
        let (again, coalesced) = first.write(&head(), b"two");
        assert!(!coalesced, "a second call does not coalesce");
        assert_eq!(again, b"two".to_vec(), "the data as it came");
        stream.write_all(&frame).await.unwrap();
        stream.write_all(&again).await.unwrap();
        let writes = stream.writes();
        assert_eq!((writes[0].len(), writes[1].len()), (TARGET, 3));
        assert!(first.coalesced() && first.short());
        assert_eq!(first.report().bytes, TARGET);
    }

    /// A head that is no HTTP request takes no field: the coalescer adds no
    /// byte that a protocol would not read as its own.
    #[test]
    fn a_head_with_no_empty_line_is_untouched() {
        let (frame, short) = first_write(b"\x00\x01\x02", b"data");
        assert_eq!(frame.as_slice(), &b"\x00\x01\x02data"[..]);
        assert!(short, "the write is short of the target, and no field went in");
    }
}

//! Split the TLS ClientHello into records.
//!
//! A ClientHello is one handshake message. TLS permits a handshake message
//! in more than one record (RFC 8446, section 5.1), and a server joins them.
//! A middlebox that reads the server name from the first record only does
//! not find it when the name is cut. The split changes the records, not the
//! bytes of the handshake: the server sees the same ClientHello.
//!
//! The split applies to the first record that a connection writes, when it
//! is a ClientHello. Any other first record, or one larger than 16 KiB, goes
//! out unchanged. The split is a value that the caller holds and gives to
//! [`wrap`] for each connection that it wants split (podnet: its MASQUE
//! connection); with no split, nothing changes.

use std::pin::Pin;
use std::task::{Context, Poll};

use pod_core::BoxedIo;
use pod_core::entropy::{Entropy, OsEntropy};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::error::Error;

/// Where the records end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Records of a random size in the range.
    Record,
    /// The first record ends in the middle of the server name; then records
    /// of a random size.
    Sni,
}

/// The split of the first ClientHello.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Split {
    pub mode: Mode,
    /// The fewest bytes of handshake data in a record.
    pub min: usize,
    /// The most bytes of handshake data in a record.
    pub max: usize,
}

/// The size range when none is given.
pub const DEFAULT_SIZE: (usize, usize) = (64, 160);

/// The part of a split that [`Split::parse`] refused, in
/// [`Error::Split`]. The words name no option of a consumer: the consumer
/// adds the name of its flag or its variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The mode: `off`, `record` or `sni`.
    Mode,
    /// The size of a record: `N` or `MIN-MAX`.
    Size,
}

impl Part {
    /// The values that this part accepts, in words.
    pub fn accepted(self) -> &'static str {
        match self {
            Part::Mode => "off, record or sni",
            Part::Size => "16 to 4096 bytes, MIN-MAX or one number",
        }
    }

    /// What this part is, in words.
    pub fn what(self) -> &'static str {
        match self {
            Part::Mode => "split mode of the ClientHello",
            Part::Size => "record size of the split ClientHello",
        }
    }
}

impl Split {
    /// `mode` is `off`, `record` or `sni` (`off` and an empty text give
    /// `None`); `size` is `N` or `MIN-MAX`, each 16 to 4096, or [`DEFAULT_SIZE`]
    /// when it is not given. A bad size is an error also with `off`: a value
    /// that the user gave is never ignored. The error is [`Error::Split`],
    /// with the part and the value.
    pub fn parse(mode: &str, size: Option<&str>) -> Result<Option<Self>, Error> {
        let mode = match mode.trim().to_ascii_lowercase().as_str() {
            "" | "off" => None,
            "record" => Some(Mode::Record),
            "sni" => Some(Mode::Sni),
            other => return Err(Error::Split { part: Part::Mode, value: other.to_string() }),
        };
        let (min, max) = match size.map(str::trim).filter(|s| !s.is_empty()) {
            None => DEFAULT_SIZE,
            Some(text) => {
                let bad = || Error::Split { part: Part::Size, value: text.to_string() };
                let (a, b) = text.split_once('-').unwrap_or((text, text));
                let a: usize = a.trim().parse().map_err(|_| bad())?;
                let b: usize = b.trim().parse().map_err(|_| bad())?;
                if !(16..=4096).contains(&a) || !(16..=4096).contains(&b) || a > b {
                    return Err(bad());
                }
                (a, b)
            }
        };
        Ok(mode.map(|mode| Split { mode, min, max }))
    }
}

/// `io` with its first ClientHello split by `split`, when there is one; else
/// `io` as it is.
pub fn wrap(io: BoxedIo, split: Option<Split>) -> BoxedIo {
    match split {
        Some(split) => Box::new(Fragmenting::new(io, split)),
        None => io,
    }
}

/// A random number in `min..=max`, from the random source of the system.
fn random_in(min: usize, max: usize) -> usize {
    if max <= min {
        return min;
    }
    let mut b = [0u8; 4];
    // With no random bytes the size is `min`: the records stay valid.
    let _ = OsEntropy.fill(&mut b);
    min + (u32::from_le_bytes(b) as usize) % (max - min + 1)
}

/// The offset in the handshake data of the bytes of the server name, and
/// their length, when the data is a ClientHello with one.
fn server_name_at(hs: &[u8]) -> Option<(usize, usize)> {
    if *hs.first()? != 1 {
        return None;
    }
    let u16_at = |i: usize| -> Option<usize> { Some(u16::from_be_bytes([*hs.get(i)?, *hs.get(i + 1)?]) as usize) };
    // type (1), length (3), version (2), random (32).
    let mut i = 4 + 2 + 32;
    i += 1 + *hs.get(i)? as usize; // session id
    i += 2 + u16_at(i)?; // cipher suites
    i += 1 + *hs.get(i)? as usize; // compression methods
    let end = (i + 2 + u16_at(i)?).min(hs.len());
    i += 2;
    while i + 4 <= end {
        let kind = u16_at(i)?;
        let len = u16_at(i + 2)?;
        let data = i + 4;
        if kind == 0 {
            // server_name: list length (2), name type (1), name length (2).
            let name_len = u16_at(data + 3)?;
            if *hs.get(data + 2)? != 0 || data + 5 + name_len > hs.len() {
                return None;
            }
            return Some((data + 5, name_len));
        }
        i = data + len;
    }
    None
}

/// The records of a first record `record` (header and data), when it is a
/// ClientHello of at most 16 KiB. `random(min, max)` gives the sizes.
pub fn fragment_with(record: &[u8], split: &Split, mut random: impl FnMut(usize, usize) -> usize) -> Option<Vec<u8>> {
    if record.len() < 6 || record[0] != 0x16 || record[1] != 3 {
        return None;
    }
    let len = u16::from_be_bytes([record[3], record[4]]) as usize;
    let hs = &record[5..];
    if hs.len() != len || len > 16 * 1024 || hs[0] != 1 {
        return None;
    }
    let mut cuts = Vec::new();
    let mut at = 0;
    if split.mode == Mode::Sni
        && let Some((name, name_len)) = server_name_at(hs)
    {
        at = name + (name_len / 2).max(1);
        cuts.push(at);
    }
    while at < len {
        at = (at + random(split.min, split.max)).min(len);
        cuts.push(at);
    }
    let mut out = Vec::with_capacity(len + 5 * cuts.len());
    let mut start = 0;
    for end in cuts {
        if end <= start {
            continue;
        }
        out.extend_from_slice(&[0x16, record[1], record[2]]);
        out.extend_from_slice(&((end - start) as u16).to_be_bytes());
        out.extend_from_slice(&hs[start..end]);
        start = end;
    }
    Some(out)
}

/// A stream whose first record is split by [`fragment_with`].
pub struct Fragmenting<S> {
    inner: S,
    split: Split,
    /// The bytes of the first record, until it is whole.
    head: Vec<u8>,
    /// The bytes to write before any other.
    out: Vec<u8>,
    sent: usize,
    done: bool,
}

impl<S> Fragmenting<S> {
    pub fn new(inner: S, split: Split) -> Self {
        Fragmenting { inner, split, head: Vec::new(), out: Vec::new(), sent: 0, done: false }
    }
}

impl<S: AsyncWrite + Unpin> Fragmenting<S> {
    /// Write the bytes of `out`.
    fn drain(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        while self.sent < self.out.len() {
            let n = std::task::ready!(Pin::new(&mut self.inner).poll_write(cx, &self.out[self.sent..]))?;
            if n == 0 {
                return Poll::Ready(Err(std::io::ErrorKind::WriteZero.into()));
            }
            self.sent += n;
        }
        self.out.clear();
        self.sent = 0;
        Poll::Ready(Ok(()))
    }

    /// The first record is whole, or cannot be: put it in `out`.
    fn finish_head(&mut self, whole: usize) {
        let rest = self.head.split_off(whole.min(self.head.len()));
        let head = std::mem::take(&mut self.head);
        let split = self.split;
        match fragment_with(&head, &split, random_in) {
            Some(records) => {
                tracing::debug!(
                    bytes = head.len() - 5,
                    records = records.len() - (head.len() - 5),
                    "the ClientHello is split"
                );
                self.out.extend_from_slice(&records);
            }
            None => {
                tracing::debug!("the first record is not a ClientHello; it goes out whole");
                self.out.extend_from_slice(&head);
            }
        }
        self.out.extend_from_slice(&rest);
        self.done = true;
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Fragmenting<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        let this = &mut *self;
        std::task::ready!(this.drain(cx))?;
        if this.done {
            return Pin::new(&mut this.inner).poll_write(cx, buf);
        }
        this.head.extend_from_slice(buf);
        if this.head.len() >= 5 {
            let whole = 5 + u16::from_be_bytes([this.head[3], this.head[4]]) as usize;
            if this.head[0] != 0x16 || this.head.len() >= whole {
                this.finish_head(whole);
            }
        }
        // The bytes are taken; a write that waits ends in a later call.
        if this.done {
            let _ = this.drain(cx)?;
        }
        Poll::Ready(Ok(buf.len()))
    }

    // rustls writes its records with one vectored write: after the first
    // record, each buffer goes to the stream under it in one call.
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        if !self.done {
            let first = bufs.iter().find(|b| !b.is_empty()).map_or(&[][..], |b| &**b);
            return self.poll_write(cx, first);
        }
        let this = &mut *self;
        std::task::ready!(this.drain(cx))?;
        Pin::new(&mut this.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = &mut *self;
        if !this.done && !this.head.is_empty() {
            // A flush before the record is whole: it goes out as it is.
            this.out.extend_from_slice(&std::mem::take(&mut this.head));
            this.done = true;
        }
        std::task::ready!(this.drain(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        std::task::ready!(self.as_mut().poll_flush(cx))?;
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Fragmenting<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A ClientHello of rustls for `name`, as the first record.
    fn client_hello(name: &str) -> Vec<u8> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
        let name = rustls_pki_types::ServerName::try_from(name.to_string()).unwrap();
        let mut conn = rustls::ClientConnection::new(Arc::new(config), name).unwrap();
        let mut out = Vec::new();
        conn.write_tls(&mut out).unwrap();
        out
    }

    /// The handshake data of a run of records, and the size of each.
    fn join(records: &[u8]) -> (Vec<u8>, Vec<usize>) {
        let (mut data, mut sizes, mut i) = (Vec::new(), Vec::new(), 0);
        while i < records.len() {
            assert_eq!(records[i], 0x16);
            let len = u16::from_be_bytes([records[i + 3], records[i + 4]]) as usize;
            data.extend_from_slice(&records[i + 5..i + 5 + len]);
            sizes.push(len);
            i += 5 + len;
        }
        (data, sizes)
    }

    #[test]
    fn the_records_carry_the_same_client_hello() {
        let hello = client_hello("consumer-masque.cloudflareclient.com");
        let split = Split::parse("record", Some("16-40")).unwrap().unwrap();
        let records = fragment_with(&hello, &split, random_in).unwrap();
        let (data, sizes) = join(&records);
        assert_eq!(data, hello[5..]);
        assert!(sizes.len() > 3);
        assert!(sizes[..sizes.len() - 1].iter().all(|s| (16..=40).contains(s)));
    }

    #[test]
    fn the_sni_mode_cuts_the_name() {
        let name = "consumer-masque.cloudflareclient.com";
        let hello = client_hello(name);
        let (at, len) = server_name_at(&hello[5..]).unwrap();
        assert_eq!(&hello[5 + at..5 + at + len], name.as_bytes());
        let split = Split::parse("sni", Some("4096")).unwrap().unwrap();
        let records = fragment_with(&hello, &split, random_in).unwrap();
        let (data, sizes) = join(&records);
        assert_eq!(data, hello[5..]);
        assert_eq!(sizes[0], at + len / 2);
        let first = &records[5..5 + sizes[0]];
        assert!(!first.windows(name.len()).any(|w| w == name.as_bytes()));
    }

    /// A bad size is an error also with `off`.
    #[test]
    fn other_records_and_bad_values() {
        let split = Split::parse("record", None).unwrap().unwrap();
        assert_eq!((split.min, split.max), DEFAULT_SIZE);
        assert!(fragment_with(&[0x17, 3, 3, 0, 1, 0], &split, random_in).is_none());
        assert!(fragment_with(&[0x16, 3, 1, 0, 2, 2, 0], &split, random_in).is_none());
        assert_eq!(Split::parse("off", Some("64")).unwrap(), None);
        assert!(Split::parse("off", Some("abc")).is_err());
        assert!(Split::parse("", Some("15")).is_err());
        for (mode, size) in
            [("fast", None), ("record", Some("4-9000")), ("record", Some("200-100")), ("sni", Some("x"))]
        {
            assert!(Split::parse(mode, size).is_err(), "{mode} {size:?}");
        }
    }

    /// The error of a bad value names the part, the value and the accepted
    /// values, and no option of a consumer; the consumer can make its own
    /// words from the part and the value.
    #[test]
    fn a_bad_value_names_its_part_and_the_accepted_values() {
        let mode = Split::parse(" Fast ", None).unwrap_err();
        assert_eq!(mode, Error::Split { part: Part::Mode, value: "fast".into() });
        assert_eq!(mode.to_string(), "\"fast\" is no split mode of the ClientHello: give off, record or sni");
        let size = Split::parse("off", Some(" 4-9000 ")).unwrap_err();
        assert_eq!(size, Error::Split { part: Part::Size, value: "4-9000".into() });
        assert_eq!(
            size.to_string(),
            "\"4-9000\" is no record size of the split ClientHello: give 16 to 4096 bytes, MIN-MAX or one number"
        );
        for e in [mode, size] {
            assert!(!e.to_string().contains("--"), "{e}");
        }
        let failure = pod_core::Failure::from(Split::parse("sni", Some("x")).unwrap_err());
        assert_eq!(failure.class, pod_core::Class::Config);
    }

    #[tokio::test]
    async fn the_stream_splits_the_first_record_only() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let hello = client_hello("example.com");
        let split = Split::parse("record", Some("16-32")).unwrap().unwrap();
        let (a, mut b) = tokio::io::duplex(64 * 1024);
        let mut w = Fragmenting::new(a, split);
        // The record comes in two writes, then other bytes.
        w.write_all(&hello[..7]).await.unwrap();
        w.write_all(&hello[7..]).await.unwrap();
        w.write_all(&[0x17, 3, 3, 0, 1, 9]).await.unwrap();
        w.shutdown().await.unwrap();
        drop(w);
        let mut got = Vec::new();
        b.read_to_end(&mut got).await.unwrap();
        assert!(got.ends_with(&[0x17, 3, 3, 0, 1, 9]));
        let (data, sizes) = join(&got[..got.len() - 6]);
        assert_eq!(data, hello[5..]);
        assert!(sizes.len() > 1);
    }

    /// After the first record, one vectored write takes each buffer: rustls
    /// writes its records with one vectored write.
    #[tokio::test]
    async fn after_the_first_record_a_vectored_write_takes_each_buffer() {
        use tokio::io::{AsyncReadExt as _, AsyncWrite as _, AsyncWriteExt as _};
        let hello = client_hello("example.com");
        let split = Split::parse("record", Some("16-32")).unwrap().unwrap();
        let (a, mut b) = tokio::io::duplex(64 * 1024);
        let mut w = Fragmenting::new(a, split);
        w.write_all(&hello).await.unwrap();
        let parts: [&[u8]; 2] = [&[0x17, 3, 3, 0, 1, 9], &[0x17, 3, 3, 0, 2, 7, 8]];
        assert!(w.is_write_vectored());
        assert_eq!(w.write_vectored(&parts.map(std::io::IoSlice::new)).await.unwrap(), 13);
        w.shutdown().await.unwrap();
        drop(w);
        let mut got = Vec::new();
        b.read_to_end(&mut got).await.unwrap();
        assert!(got.ends_with(&parts.concat()));
    }

    /// [`wrap`] with no split gives the stream as it is; with a split, the
    /// first ClientHello goes out in more than one record.
    #[tokio::test]
    async fn wrap_splits_only_when_the_caller_gives_a_split() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let hello = client_hello("example.com");
        for split in [None, Split::parse("record", Some("16")).unwrap()] {
            let (a, mut b) = tokio::io::duplex(64 * 1024);
            let mut io = wrap(Box::new(a), split);
            io.write_all(&hello).await.unwrap();
            io.shutdown().await.unwrap();
            drop(io);
            let mut got = Vec::new();
            b.read_to_end(&mut got).await.unwrap();
            let (data, sizes) = join(&got);
            assert_eq!(data, hello[5..]);
            match split {
                None => assert_eq!(got, hello),
                Some(_) => assert!(sizes.iter().all(|s| *s <= 16) && sizes.len() > 1, "{sizes:?}"),
            }
        }
    }
}

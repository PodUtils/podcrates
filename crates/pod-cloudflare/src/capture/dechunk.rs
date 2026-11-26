//! A decoder of a chunked body that takes the bytes in parts, for the
//! capture only: the peer gets the bytes as they are.

/// The longest size line, as `copy_chunked_to_futures` reads it.
const MAX_SIZE_LINE: usize = 4096;

/// The longest trailer line.
const MAX_TRAILER_LINE: usize = crate::proxy::MAX_TRAILER_LINE;

#[derive(Debug, Default, PartialEq, Eq)]
enum State {
    /// In a size line.
    #[default]
    Size,
    /// In the data of a chunk: the bytes that remain.
    Data(u64),
    /// In the line end after the data of a chunk.
    DataEnd,
    /// In the trailer, after the last chunk.
    Trailer,
    /// After the trailer: the bytes that follow are not of the body.
    Done,
}

/// The framing of a chunked body is not valid.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BadFraming;

/// The state of the decoder between two parts.
#[derive(Debug, Default)]
pub(crate) struct Dechunk {
    state: State,
    line: Vec<u8>,
}

impl Dechunk {
    /// Decode `input` and give each part of the payload to `out`. When
    /// `out` returns false, the decoder stops. A bad framing is an error.
    pub(crate) fn feed(&mut self, mut input: &[u8], out: &mut dyn FnMut(&[u8]) -> bool) -> Result<(), BadFraming> {
        while !input.is_empty() {
            match self.state {
                State::Done => return Ok(()),
                State::Data(remaining) => {
                    let n = input.len().min(usize::try_from(remaining).unwrap_or(usize::MAX));
                    let (payload, rest) = input.split_at(n);
                    input = rest;
                    let remaining = remaining - n as u64;
                    self.state = if remaining == 0 { State::DataEnd } else { State::Data(remaining) };
                    if !out(payload) {
                        return Ok(());
                    }
                }
                State::Size | State::DataEnd | State::Trailer => {
                    let limit = match self.state {
                        State::Size => MAX_SIZE_LINE,
                        State::DataEnd => 2,
                        _ => MAX_TRAILER_LINE,
                    };
                    let (taken, ended) = match input.iter().position(|b| *b == b'\n') {
                        Some(at) => (at + 1, true),
                        None => (input.len(), false),
                    };
                    self.line.extend_from_slice(&input[..taken]);
                    input = &input[taken..];
                    if self.line.len() > limit {
                        return Err(BadFraming);
                    }
                    if ended {
                        self.end_line()?;
                    }
                }
            }
        }
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), BadFraming> {
        let line = std::mem::take(&mut self.line);
        let empty = line == b"\r\n" || line == b"\n";
        self.state = match self.state {
            State::Size => {
                let text = std::str::from_utf8(&line).map_err(|_| BadFraming)?;
                let hex = text.split(';').next().unwrap_or("").trim();
                match u64::from_str_radix(hex, 16).map_err(|_| BadFraming)? {
                    0 => State::Trailer,
                    size => State::Data(size),
                }
            }
            State::DataEnd if empty => State::Size,
            State::DataEnd => return Err(BadFraming),
            State::Trailer if empty => State::Done,
            State::Trailer => State::Trailer,
            State::Data(_) | State::Done => unreachable!("a line ends only in a line state"),
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(parts: &[&[u8]]) -> Result<Vec<u8>, BadFraming> {
        let mut decoder = Dechunk::default();
        let mut payload = Vec::new();
        for part in parts {
            decoder.feed(part, &mut |bytes| {
                payload.extend_from_slice(bytes);
                true
            })?;
        }
        Ok(payload)
    }

    /// The payload is the same for each split of the wire, and the size
    /// lines, the extensions and the trailer stay out.
    #[test]
    fn each_split_gives_the_payload() {
        let wire = b"3;ext=1\r\nabc\r\n4\r\ndefg\r\n0\r\nX-T: 1\r\n\r\nafter";
        assert_eq!(decode(&[wire]).unwrap(), b"abcdefg");
        for at in 1..wire.len() {
            let (a, b) = wire.split_at(at);
            assert_eq!(decode(&[a, b]).unwrap(), b"abcdefg", "split at {at}");
        }
        let bytes: Vec<&[u8]> = wire.chunks(1).collect();
        assert_eq!(decode(&bytes).unwrap(), b"abcdefg");
    }

    /// A bad size, data with no line end and a size line with no end are
    /// errors.
    #[test]
    fn a_bad_framing_is_an_error() {
        for bad in [&b"zz\r\nab\r\n"[..], b"2\r\nabXX0\r\n\r\n"] {
            assert_eq!(decode(&[bad]), Err(BadFraming), "{:?}", String::from_utf8_lossy(bad));
        }
        assert_eq!(decode(&[&[b'1'; MAX_SIZE_LINE + 1]]), Err(BadFraming));
    }

    /// The decoder stops when the sink takes no more.
    #[test]
    fn the_decoder_stops_when_the_sink_stops() {
        let mut decoder = Dechunk::default();
        let mut calls = 0;
        decoder
            .feed(b"1\r\na\r\n1\r\nb\r\n0\r\n\r\n", &mut |_| {
                calls += 1;
                false
            })
            .unwrap();
        assert_eq!(calls, 1);
    }
}

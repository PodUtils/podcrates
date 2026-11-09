//! The control messages of the relay and the reader of its answers.
//!
//! The sizes and the offsets come from `include/gsocket/gsocket.h`: `LISTEN`
//! and `CONNECT` are 128 bytes with the address at byte 32, and each answer,
//! `ACCEPT` and `PING` is 32 bytes. A read asks for at most the bytes of one
//! message, so the first byte of the TLS handshake never enters this reader.
//!
//! The builders and [`parse`] need no runtime; the reader of a stream needs
//! the feature `rt`. The module is public only for the fuzz target
//! `gsr_message` (feature `fuzzing`, or `--cfg fuzzing`).

// With no runtime, only the tests read the builders.
#![cfg_attr(not(feature = "rt"), allow(dead_code))]

use pod_core::{Class, Failure, Stage};
#[cfg(feature = "rt")]
use tokio::io::{AsyncRead, AsyncReadExt};

pub use crate::identity::ADDR_LEN;

/// The length of a `LISTEN` message.
pub const LISTEN_LEN: usize = 128;
/// The length of a `CONNECT` message.
pub const CONNECT_LEN: usize = 128;
/// The length of each message that the relay sends, and of `ACCEPT` and `PING`.
pub const MESSAGE_LEN: usize = 32;
/// The length of the token of a listener.
pub const TOKEN_LEN: usize = 16;

const ADDR_OFFSET: usize = 32;
const TOKEN_OFFSET: usize = 16;

const TYPE_LISTEN: u8 = 0x01;
const TYPE_CONNECT: u8 = 0x02;
const TYPE_PING: u8 = 0x03;
const TYPE_PONG: u8 = 0x04;
const TYPE_START: u8 = 0x05;
const TYPE_ACCEPT: u8 = 0x06;
const TYPE_STATUS: u8 = 0x07;

const FLAG_START_SERVER: u8 = 0x01;
const FLAG_START_CLIENT: u8 = 0x02;

const STATUS_FATAL: u8 = 0x02;

/// The version of the control protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    /// The major version.
    pub major: u8,
    /// The minor version.
    pub minor: u8,
}

/// A message from the relay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    /// The answer to a `PING`.
    Pong,
    /// The relay paired this peer. `server` names the side of the TLS
    /// handshake.
    Start { server: bool },
    /// A status. A fatal one ends the wait.
    Status { fatal: bool, code: u8 },
}

/// The bytes of a `LISTEN` message: register the address, and wait.
pub fn listen(addr: &[u8; ADDR_LEN], token: &[u8; TOKEN_LEN], version: Version) -> [u8; LISTEN_LEN] {
    let mut message = [0u8; LISTEN_LEN];
    message[0] = TYPE_LISTEN;
    message[1] = version.major;
    message[2] = version.minor;
    message[TOKEN_OFFSET..TOKEN_OFFSET + TOKEN_LEN].copy_from_slice(token);
    message[ADDR_OFFSET..ADDR_OFFSET + ADDR_LEN].copy_from_slice(addr);
    message
}

/// The bytes of a `CONNECT` message with no flag: the relay answers `START`
/// or `STATUS` at once.
pub fn connect(addr: &[u8; ADDR_LEN], version: Version) -> [u8; CONNECT_LEN] {
    let mut message = [0u8; CONNECT_LEN];
    message[0] = TYPE_CONNECT;
    message[1] = version.major;
    message[2] = version.minor;
    message[ADDR_OFFSET..ADDR_OFFSET + ADDR_LEN].copy_from_slice(addr);
    message
}

/// The bytes of an `ACCEPT` message: this peer is ready for the payload.
pub fn accept() -> [u8; MESSAGE_LEN] {
    let mut message = [0u8; MESSAGE_LEN];
    message[0] = TYPE_ACCEPT;
    message
}

/// The bytes of a `PING` message.
pub fn ping() -> [u8; MESSAGE_LEN] {
    let mut message = [0u8; MESSAGE_LEN];
    message[0] = TYPE_PING;
    message
}

/// One message of the relay, read from `io`.
///
/// A call that a timeout cancels keeps the bytes that it read. The next call
/// continues the same message, because the position is a field.
#[cfg(feature = "rt")]
#[derive(Debug, Default)]
pub struct Reader {
    buffer: [u8; MESSAGE_LEN],
    filled: usize,
}

#[cfg(feature = "rt")]
impl Reader {
    pub fn new() -> Reader {
        Reader { buffer: [0u8; MESSAGE_LEN], filled: 0 }
    }

    /// The next message, or `None` when the relay closed the stream before a
    /// message started.
    pub async fn next(&mut self, io: &mut (impl AsyncRead + Unpin)) -> Result<Option<Message>, Failure> {
        while self.filled < MESSAGE_LEN {
            // Ask for the bytes of this message alone: a byte of the payload
            // that follows must stay in the socket for the TLS layer.
            let read = io.read(&mut self.buffer[self.filled..]).await.map_err(|e| Failure::io(Stage::Relay, e))?;
            if read == 0 {
                return if self.filled == 0 {
                    Ok(None)
                } else {
                    Err(Failure::new(
                        Stage::Relay,
                        Class::Protocol,
                        "the relay closed the connection in the middle of a message",
                    ))
                };
            }
            self.filled += read;
        }
        let message = parse(&self.buffer)?;
        self.filled = 0;
        Ok(Some(message))
    }
}

/// The message in the 32 bytes of `buffer`.
pub fn parse(buffer: &[u8; MESSAGE_LEN]) -> Result<Message, Failure> {
    match buffer[0] {
        TYPE_PONG => Ok(Message::Pong),
        TYPE_START => {
            let flags = buffer[1];
            if flags & FLAG_START_SERVER != 0 {
                Ok(Message::Start { server: true })
            } else if flags & FLAG_START_CLIENT != 0 {
                Ok(Message::Start { server: false })
            } else {
                Err(protocol(format!("the relay sent START with flags 0x{flags:02x}, which name no side")))
            }
        }
        TYPE_STATUS => Ok(Message::Status { fatal: buffer[1] == STATUS_FATAL, code: buffer[2] }),
        other => Err(protocol(format!("the relay sent message type 0x{other:02x}"))),
    }
}

fn protocol(why: String) -> Failure {
    Failure::new(Stage::Relay, Class::Protocol, why)
}

/// The fuzz target `gsr_message`: [`Reader::next`] on the bytes of `data`
/// after its first two, in fragments of 1 to 32 bytes (the first byte), with
/// a read that stays pending and is cancelled at each 2nd or 3rd call (the
/// second byte modulo 4; 0 and 1 for no cancel). It panics when a read takes a byte past the
/// message that it gives, when a cancelled read loses a byte, or when the
/// fragments give other messages than whole reads. It gives the messages
/// that it read.
#[cfg(any(fuzzing, feature = "fuzzing"))]
pub fn fuzz_messages(data: &[u8]) -> Vec<Message> {
    let (step, every, stream) = match data {
        [a, b, rest @ ..] => (1 + usize::from(*a) % MESSAGE_LEN, usize::from(*b) % 4, rest),
        _ => return Vec::new(),
    };
    let (cut, cut_end) = read_all(stream, step, every);
    let (whole, whole_end) = read_all(stream, MESSAGE_LEN, 0);
    assert_eq!(cut, whole, "fragments and whole reads give the same messages");
    assert_eq!(cut_end, whole_end, "fragments and whole reads end the same way");
    cut
}

/// The messages of `stream`, and how the reads ended: `None` at the end of
/// the stream, else the class of the failure.
#[cfg(any(fuzzing, feature = "fuzzing"))]
fn read_all(stream: &[u8], step: usize, every: usize) -> (Vec<Message>, Option<Class>) {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    let mut io = Fragments { data: stream, at: 0, step, every, calls: 0 };
    let mut reader = Reader::new();
    let mut messages = Vec::new();
    let mut cx = Context::from_waker(Waker::noop());
    // Each call gives a byte, the end, or one pending answer: the loop ends.
    for _ in 0..(4 * stream.len() + 8) {
        let polled = pin!(reader.next(&mut io)).poll(&mut cx);
        // On a pending answer the future dropped above: a cancel, and its
        // bytes stay in `reader`.
        match polled {
            Poll::Pending => continue,
            Poll::Ready(Ok(Some(message))) => {
                messages.push(message);
                assert_eq!(io.at, messages.len() * MESSAGE_LEN, "a read took a byte past its message");
            }
            Poll::Ready(Ok(None)) => return (messages, None),
            Poll::Ready(Err(failure)) => {
                assert!(io.at <= (messages.len() + 1) * MESSAGE_LEN, "a read took a byte past its message");
                return (messages, Some(failure.class));
            }
        }
    }
    panic!("the reader made no progress on {} bytes", stream.len());
}

/// A stream that gives `step` bytes at most for each read, stays pending at
/// each `every`-th call, and then ends.
#[cfg(any(fuzzing, feature = "fuzzing"))]
struct Fragments<'a> {
    data: &'a [u8],
    at: usize,
    step: usize,
    every: usize,
    calls: usize,
}

#[cfg(any(fuzzing, feature = "fuzzing"))]
impl AsyncRead for Fragments<'_> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.calls += 1;
        if self.every > 1 && self.calls.is_multiple_of(self.every) {
            return std::task::Poll::Pending;
        }
        let n = self.step.min(buf.remaining()).min(self.data.len() - self.at);
        let (start, end) = (self.at, self.at + n);
        buf.put_slice(&self.data[start..end]);
        self.at = end;
        std::task::Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "rt")]
    use std::io;
    #[cfg(feature = "rt")]
    use std::pin::Pin;
    #[cfg(feature = "rt")]
    use std::task::{Context, Poll};
    #[cfg(feature = "rt")]
    use std::time::Duration;

    #[cfg(feature = "rt")]
    use tokio::io::ReadBuf;

    use super::*;

    /// A stream of bytes that gives out at most `up_to` of them, and then
    /// stays pending, or ends when `eof` is true.
    #[cfg(feature = "rt")]
    struct Wire {
        data: Vec<u8>,
        at: usize,
        up_to: usize,
        eof: bool,
    }

    #[cfg(feature = "rt")]
    impl Wire {
        fn new(data: Vec<u8>) -> Wire {
            let up_to = data.len();
            Wire { data, at: 0, up_to, eof: false }
        }

        fn slowly(data: Vec<u8>, up_to: usize) -> Wire {
            Wire { data, at: 0, up_to, eof: false }
        }
    }

    #[cfg(feature = "rt")]
    impl AsyncRead for Wire {
        fn poll_read(mut self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
            if self.at >= self.up_to {
                return if self.eof { Poll::Ready(Ok(())) } else { Poll::Pending };
            }
            let n = (self.up_to - self.at).min(buf.remaining());
            let (start, end) = (self.at, self.at + n);
            buf.put_slice(&self.data[start..end]);
            self.at = end;
            Poll::Ready(Ok(()))
        }
    }

    fn addr() -> [u8; ADDR_LEN] {
        [0xaa; ADDR_LEN]
    }

    fn start(flags: u8) -> [u8; MESSAGE_LEN] {
        let mut message = [0u8; MESSAGE_LEN];
        message[0] = TYPE_START;
        message[1] = flags;
        message
    }

    #[test]
    fn a_listen_message_has_its_fields_where_the_relay_reads_them() {
        let token = [0x11; TOKEN_LEN];
        let message = listen(&addr(), &token, Version { major: 1, minor: 3 });
        assert_eq!(message.len(), LISTEN_LEN);
        assert_eq!(message[0], TYPE_LISTEN);
        assert_eq!((message[1], message[2]), (1, 3));
        assert_eq!(&message[TOKEN_OFFSET..TOKEN_OFFSET + TOKEN_LEN], &token);
        assert_eq!(&message[ADDR_OFFSET..ADDR_OFFSET + ADDR_LEN], &addr());
        assert!(message[..TOKEN_OFFSET].iter().enumerate().all(|(i, b)| i < 3 || *b == 0));
    }

    #[test]
    fn a_connect_message_names_the_address_and_no_flag() {
        let message = connect(&addr(), Version { major: 1, minor: 3 });
        assert_eq!(message.len(), CONNECT_LEN);
        assert_eq!(message[0], TYPE_CONNECT);
        assert_eq!((message[1], message[2], message[3]), (1, 3, 0));
        assert_eq!(&message[ADDR_OFFSET..ADDR_OFFSET + ADDR_LEN], &addr());
    }

    #[test]
    fn the_answers_have_the_length_that_the_relay_sends() {
        assert_eq!(accept().len(), MESSAGE_LEN);
        assert_eq!(accept()[0], TYPE_ACCEPT);
        assert_eq!(ping().len(), MESSAGE_LEN);
        assert_eq!(ping()[0], TYPE_PING);
    }

    #[test]
    fn a_start_names_the_side_of_the_handshake() {
        assert_eq!(parse(&start(FLAG_START_SERVER)).unwrap(), Message::Start { server: true });
        assert_eq!(parse(&start(FLAG_START_CLIENT)).unwrap(), Message::Start { server: false });
        let unknown = parse(&start(0)).expect_err("no side");
        assert_eq!((unknown.stage, unknown.class), (Stage::Relay, Class::Protocol));
    }

    #[test]
    fn a_status_names_its_code_and_its_class() {
        let mut message = [0u8; MESSAGE_LEN];
        message[0] = TYPE_STATUS;
        message[1] = STATUS_FATAL;
        message[2] = 0x02;
        assert_eq!(parse(&message).unwrap(), Message::Status { fatal: true, code: 2 });
        message[1] = 0;
        assert_eq!(parse(&message).unwrap(), Message::Status { fatal: false, code: 2 });
    }

    #[cfg(feature = "rt")]
    #[tokio::test]
    async fn the_reader_stops_at_the_end_of_a_message() {
        let mut data = Vec::new();
        data.extend_from_slice(&start(FLAG_START_SERVER));
        data.extend_from_slice(b"a record of the payload");
        let mut io = Wire::new(data);
        let mut reader = Reader::new();
        let message = reader.next(&mut io).await.unwrap().unwrap();
        assert_eq!(message, Message::Start { server: true });
        assert_eq!(io.at, MESSAGE_LEN, "the payload stays in the socket");
    }

    #[cfg(feature = "rt")]
    #[tokio::test]
    async fn the_reader_discards_a_pong_before_a_start() {
        let mut data = Vec::new();
        data.extend_from_slice(&ping_answer());
        data.extend_from_slice(&start(FLAG_START_CLIENT));
        let mut io = Wire::new(data);
        let mut reader = Reader::new();
        assert_eq!(reader.next(&mut io).await.unwrap(), Some(Message::Pong));
        assert_eq!(reader.next(&mut io).await.unwrap(), Some(Message::Start { server: false }));
    }

    #[cfg(feature = "rt")]
    #[tokio::test]
    async fn an_end_before_a_message_is_none_and_an_end_inside_one_is_an_error() {
        let mut empty = Wire::new(Vec::new());
        empty.eof = true;
        assert_eq!(Reader::new().next(&mut empty).await.unwrap(), None);

        let mut short = Wire::new(vec![TYPE_START, FLAG_START_SERVER]);
        short.eof = true;
        let failure = Reader::new().next(&mut short).await.expect_err("a cut message");
        assert_eq!(failure.class, Class::Protocol);
    }

    #[cfg(feature = "rt")]
    #[tokio::test(start_paused = true)]
    async fn a_cancelled_read_keeps_the_bytes_of_the_message() {
        let mut wire = Wire::slowly(start(FLAG_START_SERVER).to_vec(), 10);
        let mut reader = Reader::new();
        let cancelled = tokio::time::timeout(Duration::from_millis(1), reader.next(&mut wire)).await;
        assert!(cancelled.is_err(), "the stream stays pending");
        assert_eq!(wire.at, 10, "the first 10 bytes arrived");

        wire.up_to = MESSAGE_LEN;
        let message = reader.next(&mut wire).await.unwrap().unwrap();
        assert_eq!(message, Message::Start { server: true });
    }

    #[cfg(feature = "rt")]
    fn ping_answer() -> [u8; MESSAGE_LEN] {
        let mut message = [0u8; MESSAGE_LEN];
        message[0] = TYPE_PONG;
        message
    }
}

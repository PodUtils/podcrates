//! A stand-in TPM on one end of a stream: it reads each command in one
//! read, as swtpm does (`SWTPM_IO_Read` of `src/swtpm/swtpm_io.c`), and
//! writes the answer of the test. With [`Frame::Mssim`] it checks the
//! prefix of `TPM_SEND_COMMAND` and writes the size of the response before
//! it and the acknowledgement after it (`mainLoop` of
//! `src/swtpm/mainloop.c`). It is no model of a TPM.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pod_tpm::wire::{MAX_COMMAND, MAX_RESPONSE};

/// The framing that the stand-in reads and writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// The bytes of the command and of the response, as they are.
    Raw,
    /// The prefix of `TPM_SEND_COMMAND` (8), the locality and the size; the
    /// size of the response, the response, and an acknowledgement of 0.
    Mssim,
}

/// What the stand-in does with one command.
pub enum Answer {
    /// This response, in the framing of the stand-in.
    Whole(Vec<u8>),
    /// These bytes, as they are, then the end of the connection.
    Bytes(Vec<u8>),
    /// No answer: the stand-in reads until the other end closes.
    Silent,
    /// The end of the connection.
    Close,
}

/// A response of `size` bytes that succeeded: the header, then zeros.
pub fn response(size: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; size];
    bytes[..2].copy_from_slice(&0x8001u16.to_be_bytes());
    bytes[2..6].copy_from_slice(&(size as u32).to_be_bytes());
    bytes
}

/// The header of a response that says `size` bytes, and `body` bytes more.
pub fn header_saying(size: u32, body: usize) -> Vec<u8> {
    let mut bytes = vec![0x80, 0x01];
    bytes.extend_from_slice(&size.to_be_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.resize(10 + body, 0);
    bytes
}

/// The response in the framing `frame`.
pub fn framed(frame: Frame, response: &[u8]) -> Vec<u8> {
    match frame {
        Frame::Raw => response.to_vec(),
        Frame::Mssim => {
            let mut out = (response.len() as u32).to_be_bytes().to_vec();
            out.extend_from_slice(response);
            out.extend_from_slice(&[0; 4]);
            out
        }
    }
}

/// The end of a stand-in: each command that came, and whether the other end
/// closed the stream.
pub struct Seen {
    pub commands: Vec<Vec<u8>>,
    pub closed: bool,
}

/// Serve `answers` in order on `stream`, in a thread. The receiver gives
/// what the stand-in saw when it ends.
pub fn serve<S: Read + Write + Send + 'static>(stream: S, frame: Frame, answers: Vec<Answer>) -> mpsc::Receiver<Seen> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let seen = run(stream, frame, answers);
        let _ = tx.send(seen);
    });
    rx
}

/// What the stand-in saw, within `limit`; a panic when it does not end.
pub fn ended(rx: &mpsc::Receiver<Seen>, limit: Duration) -> Seen {
    rx.recv_timeout(limit).expect("the stand-in did not end")
}

fn run<S: Read + Write>(mut stream: S, frame: Frame, answers: Vec<Answer>) -> Seen {
    let mut seen = Seen { commands: Vec::new(), closed: false };
    let mut buffer = vec![0u8; MAX_COMMAND + 9];
    for answer in answers {
        let n = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => {
                seen.closed = true;
                return seen;
            }
            Ok(n) => n,
        };
        seen.commands.push(unframe(frame, &buffer[..n]));
        let out = match answer {
            Answer::Whole(response) => framed(frame, &response),
            Answer::Bytes(bytes) => {
                let _ = stream.write_all(&bytes);
                return seen;
            }
            Answer::Silent => {
                seen.closed = until_closed(&mut stream);
                return seen;
            }
            Answer::Close => return seen,
        };
        if stream.write_all(&out).is_err() {
            return seen;
        }
    }
    seen.closed = until_closed(&mut stream);
    seen
}

/// The command of one read, with the prefix checked and taken away.
fn unframe(frame: Frame, bytes: &[u8]) -> Vec<u8> {
    match frame {
        Frame::Raw => bytes.to_vec(),
        Frame::Mssim => {
            assert!(bytes.len() >= 9, "a command of {} bytes has no prefix", bytes.len());
            assert_eq!(&bytes[..4], &8u32.to_be_bytes(), "the prefix is not TPM_SEND_COMMAND");
            assert_eq!(bytes[4], 0, "the locality is not 0");
            let size = u32::from_be_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
            assert_eq!(size, bytes.len() - 9, "the size of the prefix is not the size of the command");
            bytes[9..].to_vec()
        }
    }
}

/// True when the other end closed: a read gives 0 or fails.
fn until_closed<S: Read>(stream: &mut S) -> bool {
    let mut sink = [0u8; 64];
    loop {
        match stream.read(&mut sink) {
            Ok(0) | Err(_) => return true,
            Ok(_) => {}
        }
    }
}

/// The largest response that a transport takes.
pub const BOUND: usize = MAX_RESPONSE;

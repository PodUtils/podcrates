//! The command channel of a TPM in software, on TCP or on a Unix socket.
//!
//! swtpm (`swtpm socket --tpm2 --server type=tcp,port=PORT` or
//! `--server type=unixio,path=PATH`) reads each command in one read
//! (`SWTPM_IO_Read`) and takes two forms of it (`mainLoop` and
//! `tpmlib_handle_tcg_tpm2_cmd_header` of swtpm 0.10.2):
//!
//! * its own, [`Framing::Raw`]: the command as it is, and the response as
//!   it is;
//! * the form of the `mssim` interface, [`Framing::Mssim`], which the
//!   simulator of the reference (`ms-tpm-20-ref`) takes too: the prefix
//!   `TPM_SEND_COMMAND` (8, a `u32`), the locality (a byte) and the size of
//!   the command (a `u32`); then the size of the response (a `u32`), the
//!   response and an acknowledgement of 0 (a `u32`).
//!
//! The control channel of swtpm (`--ctrl`: init, power, reset, shutdown)
//! and `TPM2_Startup` are the work of the harness that starts it, not of
//! this transport, unless swtpm has `--flags not-need-init,startup-clear`.
//! swtpm keeps the connection for each next command unless its server has
//! the option `disconnect`.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::Path;
use std::time::{Duration, Instant};

use pod_core::sockopt::shut_needs_no_limit;
use pod_core::{Class, Failure};

use super::bound::{Limits, broken, host, late, left, response_size, size_field, system};
use super::check_command;
use crate::transport::Transport;
use crate::wire::HEADER;

/// The command of the `mssim` interface that carries a TPM command.
const TPM_SEND_COMMAND: u32 = 8;

/// The form of a command and of a response on the channel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Framing {
    /// swtpm's own: the command and the response as they are.
    #[default]
    Raw,
    /// The `mssim` interface: the prefix `TPM_SEND_COMMAND`, locality 0;
    /// the size of the response before it and an acknowledgement after it.
    Mssim,
}

enum Stream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}

impl Stream {
    /// The read and the write limit. A socket that the peer shut needs none
    /// ([`shut_needs_no_limit`]: macOS refuses the set on it): its next
    /// read gives the bytes that remain, then the end, so a short response
    /// is a short response and not a failure of the set.
    fn timeouts(&self, wait: Duration) -> io::Result<()> {
        let wait = Some(wait);
        match self {
            Stream::Tcp(s) => shut_needs_no_limit(s.set_read_timeout(wait))
                .and_then(|()| shut_needs_no_limit(s.set_write_timeout(wait))),
            #[cfg(unix)]
            Stream::Unix(s) => shut_needs_no_limit(s.set_read_timeout(wait))
                .and_then(|()| shut_needs_no_limit(s.set_write_timeout(wait))),
        }
    }

    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Tcp(s) => s.read(buffer),
            #[cfg(unix)]
            Stream::Unix(s) => s.read(buffer),
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Stream::Tcp(s) => s.write_all(bytes),
            #[cfg(unix)]
            Stream::Unix(s) => s.write_all(bytes),
        }
    }
}

/// A connection to the command channel of a TPM in software. One command at
/// a time; each one is bounded by [`Limits`] and its response by
/// [`crate::wire::MAX_RESPONSE`]. After a failure on the way (a time limit,
/// a response out of its bounds, a closed connection) the connection is
/// closed, as the bytes that remain on it are of no command, and each next
/// call fails with the class `Unavailable`.
pub struct Socket {
    stream: Option<Stream>,
    name: String,
    limits: Limits,
    framing: Framing,
}

impl Socket {
    /// Connect to the TCP channel at `addr` (swtpm `--server type=tcp`), in
    /// the time limit of a command.
    pub fn connect_tcp(addr: SocketAddr, limits: Limits) -> Result<Socket, Failure> {
        let name = format!("the TPM socket {addr}");
        let stream = TcpStream::connect_timeout(&addr, limits.command)
            .map_err(|e| system(format!("no TPM: {name} does not answer"), e))?;
        Socket::from_tcp(stream, limits).map(|s| s.named(name))
    }

    /// Connect to the Unix socket at `path` (swtpm `--server type=unixio`).
    #[cfg(unix)]
    pub fn connect_unix(path: &Path, limits: Limits) -> Result<Socket, Failure> {
        let name = format!("the TPM socket {}", path.display());
        let stream = UnixStream::connect(path).map_err(|e| system(format!("no TPM: {name} does not answer"), e))?;
        Socket::from_unix(stream, limits).map(|s| s.named(name))
    }

    /// A TCP stream that is connected already.
    pub fn from_tcp(stream: TcpStream, limits: Limits) -> Result<Socket, Failure> {
        // A command and its response are one exchange: no wait for more.
        let _ = stream.set_nodelay(true);
        Ok(Socket::of(Stream::Tcp(stream), "the TPM socket", limits))
    }

    /// A Unix stream that is connected already: one end of a socket pair,
    /// or a socket that a parent passed.
    #[cfg(unix)]
    pub fn from_unix(stream: UnixStream, limits: Limits) -> Result<Socket, Failure> {
        Ok(Socket::of(Stream::Unix(stream), "the TPM socket", limits))
    }

    /// The same socket with `framing` ([`Framing::Raw`] by default).
    pub fn with_framing(mut self, framing: Framing) -> Socket {
        self.framing = framing;
        self
    }

    fn of(stream: Stream, name: &str, limits: Limits) -> Socket {
        Socket { stream: Some(stream), name: name.to_string(), limits, framing: Framing::Raw }
    }

    fn named(mut self, name: String) -> Socket {
        self.name = name;
        self
    }
}

impl Transport for Socket {
    fn submit(&mut self, command: &[u8]) -> Result<Vec<u8>, Failure> {
        check_command(command)?;
        let Some(stream) = self.stream.as_mut() else {
            return Err(broken(&self.name));
        };
        let mut way =
            Way { stream, name: &self.name, limits: &self.limits, until: Instant::now() + self.limits.command };
        let answer = way.exchange(self.framing, command);
        if answer.is_err() {
            // The drop closes the connection.
            self.stream = None;
        }
        answer
    }
}

/// One exchange on a stream, in its time limit.
struct Way<'a> {
    stream: &'a mut Stream,
    name: &'a str,
    limits: &'a Limits,
    until: Instant,
}

impl Way<'_> {
    fn exchange(&mut self, framing: Framing, command: &[u8]) -> Result<Vec<u8>, Failure> {
        let mut out = Vec::with_capacity(command.len() + 9);
        if framing == Framing::Mssim {
            out.extend_from_slice(&TPM_SEND_COMMAND.to_be_bytes());
            out.push(0);
            out.extend_from_slice(&u32::try_from(command.len()).unwrap_or(u32::MAX).to_be_bytes());
        }
        out.extend_from_slice(command);
        // One write: swtpm reads a command in one read.
        self.wait()?;
        self.stream.write_all(&out).map_err(|e| self.fault(e, "the command did not go to"))?;
        match framing {
            Framing::Raw => {
                let mut header = [0u8; HEADER];
                self.fill(&mut header, true)?;
                let size = response_size(size_field(&header).unwrap_or(0), self.name)?;
                let mut answer = vec![0u8; size];
                answer[..HEADER].copy_from_slice(&header);
                self.fill(&mut answer[HEADER..], false)?;
                Ok(answer)
            }
            Framing::Mssim => {
                let mut prefix = [0u8; 4];
                self.fill(&mut prefix, true)?;
                let size = response_size(u32::from_be_bytes(prefix), self.name)?;
                let mut answer = vec![0u8; size];
                self.fill(&mut answer, false)?;
                let mut ack = [0u8; 4];
                self.fill(&mut ack, false)?;
                match u32::from_be_bytes(ack) {
                    0 => Ok(answer),
                    n => Err(host(Class::Protocol, format!("the acknowledgement of {} is {n}, not 0", self.name))),
                }
            }
        }
    }

    /// Set the wait of the next call of the stream to the time that remains.
    fn wait(&mut self) -> Result<(), Failure> {
        let wait = left(self.until, self.name, self.limits)?;
        self.stream.timeouts(wait).map_err(|e| self.fault(e, "the time limit cannot be set on"))
    }

    /// Fill `buffer`. `first` is the start of a response: an end there is a
    /// connection that closed, not a short response.
    fn fill(&mut self, buffer: &mut [u8], first: bool) -> Result<(), Failure> {
        let mut at = 0;
        while at < buffer.len() {
            self.wait()?;
            match self.stream.read(&mut buffer[at..]) {
                Ok(0) if first && at == 0 => {
                    return Err(host(Class::Unavailable, format!("{} closed the connection", self.name)));
                }
                Ok(0) => {
                    let why = format!("the response of {} ends after {at} of {} bytes", self.name, buffer.len());
                    return Err(host(Class::Protocol, why));
                }
                Ok(n) => at += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(self.fault(e, "the response cannot be read from")),
            }
        }
        Ok(())
    }

    fn fault(&self, e: io::Error, what: &str) -> Failure {
        match e.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => late(self.name, self.limits),
            _ => system(format!("{what} {}", self.name), e),
        }
    }
}

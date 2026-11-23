//! The NTPv4 carrier (feature `ntp`, PodUtils/podnet#484): the bytes of a
//! link travel in the extension fields of NTP packets over UDP port 123,
//! one poll for each request (NTP-1 to NTP-4).
//!
//! * The extension field carries a 16-bit type, a 16-bit length and a
//!   value ([`wire`]). Each packet takes another type from the unassigned
//!   range 0x1000 to 0xEFFF ([`wire::Types`]), so a constant type is not a
//!   signature (NTP-1).
//! * The timestamps are those of a disciplined clock
//!   ([`session::Discipline`]): the carrier steers its own timestamps, not
//!   the system clock, and holds the measured offset inside the tolerance
//!   of the caller (1 ms by default) so that a server sees a plausible
//!   clock (NTP-2).
//! * A poll waits [`Ntp::poll`] seconds between one request and the next,
//!   32 s by default and never under 16 s or over 64 s: this is a control
//!   channel (NTP-3).
//! * A reply that the server cut short is a carrier limit, not a protocol
//!   error: the session reports it, reduces the payload and goes on
//!   (NTP-4).
//! * The dial address is `HOST[:PORT]` (port 123 by default); the listen
//!   address is `IP:PORT`.
//!
//! UDP loses a request or a reply, and the session sends a request that got
//! no reply again, so a byte can arrive twice after a lost reply. The facts
//! say `reliable: false`: put the ARQ of [`crate::arq`] above this carrier,
//! as for any carrier that loses datagrams.
//!
//! The session ([`session::Session`]) does no I/O, so a test drives it
//! against a server of its own; the poll loop of the dial and the server of
//! the listener are `client` and `server`.

mod client;
mod server;
pub mod session;
pub mod wire;

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;

pub use server::NtpListener;
pub use session::{Discipline, MAX_PAYLOAD, Session};
pub use wire::{Field, Packet, Timestamp, Types};

use crate::carrier::{
    BoxIo, Carrier, CarrierError, CarrierErrorKind, Cost, Facts, Limit, Limits, Listener, Pending, Rate,
};
use crate::pipes::{Ends, pipes};

/// The name of the carrier.
pub const NAME: &str = "ntp";

/// The port of NTP.
pub const PORT: u16 = 123;

/// The payload bytes of one request by default: the gate of the carrier
/// (PodUtils/podnet#484).
pub const PAYLOAD: usize = 300;

/// The poll interval by default.
pub const POLL: Duration = Duration::from_secs(32);

/// The shortest poll interval (NTP-3).
pub const MIN_POLL: Duration = Duration::from_secs(16);

/// The longest poll interval (NTP-3).
pub const MAX_POLL: Duration = Duration::from_secs(64);

/// The tolerance of the clock discipline by default (NTP-2).
pub const TOLERANCE: Duration = Duration::from_millis(1);

/// The time limit of a reply to one request.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(4);

/// The ring of the byte stream between the link and the task of the
/// carrier.
const PIPE: usize = client::PIPE;

/// The NTPv4 carrier: the payload, the poll interval, the tolerance of the
/// discipline and the limits of its sessions. A clone shares the limits.
#[derive(Clone, Debug)]
pub struct Ntp {
    /// The payload bytes of one request, at most [`MAX_PAYLOAD`].
    pub payload: usize,
    /// The time between one request and the next, 16 s to 64 s.
    pub poll: Duration,
    /// How far the clock of the carrier may stand from the server's.
    pub tolerance: Duration,
    /// The time limit of the reply to one request.
    pub reply_timeout: Duration,
    limits: Limits,
}

impl Default for Ntp {
    fn default() -> Ntp {
        Ntp { payload: PAYLOAD, poll: POLL, tolerance: TOLERANCE, reply_timeout: REPLY_TIMEOUT, limits: Limits::new() }
    }
}

fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

impl Ntp {
    pub fn new() -> Ntp {
        Ntp::default()
    }

    /// The last limit that a session of this carrier met.
    pub fn limit(&self) -> Option<Limit> {
        self.limits.get()
    }

    /// The payload bytes of one request.
    pub fn with_payload(mut self, payload: usize) -> Ntp {
        self.payload = payload;
        self
    }

    /// The time between one request and the next.
    pub fn with_poll(mut self, poll: Duration) -> Ntp {
        self.poll = poll;
        self
    }

    /// How far the clock of the carrier may stand from the server's.
    pub fn with_tolerance(mut self, tolerance: Duration) -> Ntp {
        self.tolerance = tolerance;
        self
    }

    /// The time limit of the reply to one request.
    pub fn with_reply_timeout(mut self, bound: Duration) -> Ntp {
        self.reply_timeout = bound;
        self
    }

    /// The settings are in their ranges.
    pub fn check(&self) -> Result<(), CarrierError> {
        if !(1..=MAX_PAYLOAD).contains(&self.payload) {
            return Err(err(
                CarrierErrorKind::Usage,
                format!("a payload of {} bytes: give 1 to {MAX_PAYLOAD}", self.payload),
            ));
        }
        if !(MIN_POLL..=MAX_POLL).contains(&self.poll) {
            return Err(err(
                CarrierErrorKind::Usage,
                format!("a poll of {:?}: give {MIN_POLL:?} to {MAX_POLL:?} (NTP-3)", self.poll),
            ));
        }
        if self.reply_timeout.is_zero() || self.tolerance.is_zero() {
            return Err(err(CarrierErrorKind::Usage, "a time of the carrier is 0"));
        }
        Ok(())
    }

    /// The rate of the carrier: one request for each poll, and the payload
    /// of each.
    pub fn rate(&self) -> Rate {
        let secs = self.poll.as_secs_f64().max(1.0);
        Rate {
            bytes_per_second: Some((self.payload as f64 / secs) as u64),
            poll_interval: Some(self.poll),
            cost: Cost::Highest,
        }
    }

    /// The host and the port of a dial address `HOST[:PORT]`.
    pub fn address(text: &str) -> Result<(String, u16), CarrierError> {
        let bad = || err(CarrierErrorKind::Usage, format!("{text:?} is no HOST[:PORT]"));
        let (host, port) = match text.rsplit_once(':') {
            Some((host, port)) => (host, port.parse::<u16>().map_err(|_| bad())?),
            None => (text, PORT),
        };
        let host = host.trim_start_matches('[').trim_end_matches(']');
        if host.is_empty() {
            return Err(bad());
        }
        Ok((host.to_string(), port))
    }

    /// A session with these settings.
    fn session(&self) -> Session {
        use pod_core::entropy::{Entropy, OsEntropy};
        let mut seed = [0u8; 2];
        let seed = OsEntropy.fill(&mut seed).map(|()| u16::from_be_bytes(seed)).unwrap_or(0x1234);
        Session::new(self.payload, self.poll, self.tolerance, seed)
    }
}

impl Carrier for Ntp {
    fn name(&self) -> &'static str {
        NAME
    }

    /// Ordered, not reliable (a lost reply sends a request again, and the
    /// ARQ above takes a duplicate), duplex, with the payload as its unit
    /// and the poll as its latency floor.
    fn facts(&self) -> Facts {
        Facts {
            ordered: true,
            reliable: false,
            duplex: true,
            mtu: Some(self.payload),
            cost: Cost::Highest,
            rate: self.rate(),
        }
    }

    /// A UDP socket binds on the loopback: the host allows the family.
    fn available(&self) -> Result<(), CarrierError> {
        std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .or_else(|_| std::net::UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)))
            .map(drop)
            .map_err(|e| err(CarrierErrorKind::Unavailable, format!("UDP sockets on the loopback: {e}")))
    }

    /// Poll the server of `HOST[:PORT]`; the stream carries the bytes in
    /// both directions.
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            self.check()?;
            let (host, port) = Ntp::address(address)?;
            let mut targets = tokio::net::lookup_host((host.as_str(), port))
                .await
                .map_err(|e| CarrierError::io(NAME, format!("the resolve of {host}:{port}"), e))?;
            let target =
                targets.next().ok_or_else(|| err(CarrierErrorKind::NotFound, format!("no address for {host}")))?;
            let any: SocketAddr =
                if target.is_ipv6() { (Ipv6Addr::UNSPECIFIED, 0).into() } else { (Ipv4Addr::UNSPECIFIED, 0).into() };
            let socket = UdpSocket::bind(any)
                .await
                .map_err(|e| CarrierError::io(NAME, format!("a UDP socket toward {target}"), e))?;
            socket.connect(target).await.map_err(|e| CarrierError::io(NAME, format!("connect to {target}"), e))?;
            let ends = Ends::new();
            let (stream, far) = pipes(PIPE, ends.clone());
            let session = self.session();
            let (poll, reply_timeout, limits) = (self.poll, self.reply_timeout, self.limits.clone());
            let task = client::run(socket, session, far, poll, reply_timeout, limits);
            tokio::spawn(async move {
                if let Err(e) = task.await {
                    ends.fail_with(Arc::new(move || io::Error::new(io::ErrorKind::InvalidData, e.to_string())));
                }
            });
            Ok(Box::new(stream) as BoxIo)
        })
    }

    /// Listen at `IP:PORT` as an NTP server; each source address that sends
    /// a request is a peer.
    fn listen<'a>(&'a self, address: &'a str) -> Pending<'a, Box<dyn Listener>> {
        Box::pin(async move {
            self.check()?;
            let addr: SocketAddr = address
                .parse()
                .map_err(|_| err(CarrierErrorKind::Usage, format!("{address:?} is no IP:PORT (127.0.0.1:123)")))?;
            let socket = UdpSocket::bind(addr).await.map_err(|e| CarrierError::io(NAME, format!("bind {addr}"), e))?;
            let local_addr = socket.local_addr().map_err(|e| CarrierError::io(NAME, "the local address", e))?;
            let socket = Arc::new(socket);
            let (accept, accepted) = tokio::sync::mpsc::channel(64);
            tokio::spawn(server::run(socket, accept, PIPE));
            Ok(Box::new(NtpListener::new(local_addr, accepted)) as Box<dyn Listener>)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_poll_of_the_carrier_stays_between_sixteen_and_sixty_four_seconds() {
        let ntp = Ntp::new();
        assert!(ntp.check().is_ok());
        assert!(Ntp { poll: Duration::from_secs(15), ..ntp.clone() }.check().is_err(), "a poll under 16 s");
        assert!(Ntp { poll: Duration::from_secs(65), ..ntp.clone() }.check().is_err(), "a poll over 64 s");
        assert!(Ntp { payload: 0, ..ntp.clone() }.check().is_err());
        let facts = ntp.facts();
        assert_eq!((facts.ordered, facts.reliable, facts.duplex, facts.mtu), (true, false, true, Some(300)));
        assert_eq!(facts.rate.poll_interval, Some(POLL));
        assert_eq!(facts.rate.cost, facts.cost);
        assert_eq!(facts.rate.bytes_per_second, Some(9), "300 bytes each 32 s");
    }

    #[test]
    fn a_dial_address_names_a_port_or_takes_123() {
        assert_eq!(Ntp::address("time.example").expect("a name"), ("time.example".into(), 123));
        assert_eq!(Ntp::address("127.0.0.1:1123").expect("a port"), ("127.0.0.1".into(), 1123));
        assert_eq!(Ntp::address("[::1]:1123").expect("a v6 address"), ("::1".into(), 1123));
        assert!(Ntp::address(":123").is_err());
    }
}

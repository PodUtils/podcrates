//! pod-link: many streams between two peers over one carrier byte stream.
//!
//! A [`Link`] runs over any byte stream ([`tokio::io::AsyncRead`] and
//! [`tokio::io::AsyncWrite`]) that a [`Carrier`] gives. It multiplexes
//! streams with the session rules of podnet's link (PodUtils/podnet#466,
//! row E0-2):
//!
//! - the dialer opens streams with odd ids, the listener with even ids;
//! - each stream and the link have a credit window: 16 KiB at the start,
//!   up to the settings (at most 8 MiB). The link's window bounds the bytes
//!   that one end holds unread, so a stream that stops reading holds only
//!   its own window, and the other streams keep their credit;
//! - 256 streams by default, 4096 at most;
//! - a frame (the record that the carrier carries) has at most 1 MiB; the
//!   link splits a write into frames of [`Settings::frame_size`];
//! - a keepalive: a `PING` after a silence, and the end of a link that is
//!   silent too long ([`Error::Dead`]);
//! - `OPEN`, `DATA`, `WINDOW`, `CLOSE` (a half close), `RESET` and
//!   `GOAWAY` ([`frame`]).
//!
//! The carriers: [`unix::Unix`] (a stream over `AF_UNIX`),
//! [`memory::Memory`] (two ends in one process), with the feature `tcp`,
//! `tcp::Tcp` (TLS 1.3 over TCP through the caller's way out, each side
//! pinning the Ed25519 key of the other), and with the feature `ws`,
//! `ws::Ws` (the same TLS under a WebSocket, its listener on the accept
//! loop of `pod-ws`), with the feature `h2`, `h2::H2` (the same TLS
//! under one POST of HTTP/2), with the feature `poll`, `poll::Poll` (the
//! same TLS under requests of HTTP/1.1: the long poll, SSE, a chunked
//! stream and the simplex poll), with the feature `quic`, `quic::Quic` (the
//! same TLS under one stream of QUIC, over UDP or the caller's datagrams),
//! with the feature `udp`, `udp::Udp` (the same TLS over KCP over UDP), with
//! the feature `dns`, `dns::Dns` (the same TLS over the ARQ over DNS queries
//! and TXT answers), with the feature `icmp`, `icmp::Icmp` (the same TLS
//! over the ARQ over ICMP echo messages), and with the feature `s3`,
//! `s3::S3` (the same TLS over objects of a bucket of the S3 API; the
//! feature `s3-standin` adds `s3::standin`, a stand-in of the S3 API on
//! the loopback for the tests of a consumer), with the feature `ntp`,
//! `ntp::Ntp` (the bytes in the extension fields of NTPv4 packets over UDP
//! 123), with the feature `smtp`, `smtp::Smtp` (one message for each batch
//! over submission, 587 with STARTTLS or 465 with TLS), and with the
//! feature `imap`, `imap::Imap` (the bytes up by `APPEND` and down by `UID
//! FETCH` over 143 with STARTTLS or 993 with TLS; [`mail_pair`] joins a
//! submission carrier and a retrieval carrier into one stream). Each
//! carrier says with [`Carrier::available`] why the host cannot use it,
//! and a listener gives the key that its peer proved with
//! [`Listener::accept_peer`]. [`Carrier::facts`] gives the cost class and
//! the rate of each carrier ([`Rate`]); a limit that its service sets is a
//! [`Limit`], not an error. The registry of the carriers, their order and
//! their flags are the consumer's (call C117).
//!
//! [`split::Split`] runs one link over two carriers: the bytes of the
//! dialer up one, the bytes of the listener down the other, and the loss of
//! either half ends the session with its name.
//!
//! A carrier that loses datagrams ([`datagram::Datagrams`]) carries a link
//! through the selective-repeat ARQ: [`arq::ArqStream`] makes one ordered
//! byte stream of it. [`datagram::memory::pair`] and
//! [`datagram::lossy::Lossy`] are a datagram pair in one process and a
//! shim that loses, reorders and duplicates from a seed.
//!
//! The resume of a link after the loss of its carrier is `pod-session`'s:
//! a consumer runs the link over the application side of a resumable
//! session, which is a byte stream (call C120).
//!
//! The crate reads no environment variable, keeps no global state and
//! prints nothing: each error converts to a [`pod_core::Failure`] (the
//! stage `Session` for the link, `Connect` for a carrier).

pub mod arq;
mod carrier;
pub mod datagram;
#[cfg(any(feature = "dns", feature = "icmp", feature = "s3"))]
mod dgram_tls;
#[cfg(feature = "dns")]
pub mod dns;
mod error;
pub mod frame;
#[cfg(feature = "h2")]
pub mod h2;
#[cfg(feature = "icmp")]
pub mod icmp;
#[cfg(feature = "imap")]
pub mod imap;
mod inbound;
mod link;
#[cfg(any(feature = "imap", feature = "smtp"))]
mod mail;
pub mod memory;
#[cfg(feature = "ntp")]
pub mod ntp;
#[cfg(feature = "tcp")]
mod pin;
mod pipes;
#[cfg(feature = "poll")]
pub mod poll;
#[cfg(any(feature = "h2", feature = "poll"))]
mod post_url;
#[cfg(feature = "quic")]
pub mod quic;
#[cfg(feature = "s3")]
pub mod s3;
mod settings;
#[cfg(feature = "smtp")]
pub mod smtp;
pub mod split;
mod state;
mod stream;
#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "udp")]
pub mod udp;
#[cfg(unix)]
pub mod unix;
#[cfg(feature = "ws")]
pub mod ws;

pub use carrier::{
    Accepted, BoxIo, Carrier, CarrierError, CarrierErrorKind, Cost, Facts, Io, Limit, LimitKind, Limits, Listener,
    Pending, Rate,
};
pub use error::{Error, GoAwayCode, ResetCode};
pub use frame::{Decoder, Frame, FrameError, MAX_RECORD};
pub use link::Link;
pub use settings::{
    CONTROL_LIMIT, INITIAL_WINDOW, MAX_CREDIT, MAX_STREAMS, MAX_WINDOW, MIN_FRAME, MIN_QUEUE, MIN_WINDOW, Settings,
    SettingsError,
};
pub use state::Role;
pub use stream::Stream;

/// One byte stream of the two halves of a mail tunnel (features `smtp` and
/// `imap`): the bytes up go to the submission carrier, and the bytes down
/// come from the retrieval carrier. Use it where a link needs one carrier
/// and the network permits submission alone, or retrieval alone.
#[cfg(any(feature = "imap", feature = "smtp"))]
pub fn mail_pair(up: BoxIo, down: BoxIo) -> BoxIo {
    mail::join(up, down)
}

/// The bytes of one message of the mail carriers (features `smtp` and
/// `imap`): the body, decoded from base64 when the message carries the
/// headers of [`smtp`], and the body as it is for another tool's message.
#[cfg(any(feature = "imap", feature = "smtp"))]
pub fn mail_body(message: &[u8]) -> Vec<u8> {
    mail::body(message)
}

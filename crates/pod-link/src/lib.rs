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
//! [`memory::Memory`] (two ends in one process) and, with the feature
//! `tcp`, `tcp::Tcp` (TLS 1.3 over TCP through the caller's way out, each
//! side pinning the Ed25519 key of the other). Each carrier says with
//! [`Carrier::available`] why the host cannot use it, and a listener gives
//! the key that its peer proved with [`Listener::accept_peer`]. The
//! registry of the carriers, their order and their flags are the
//! consumer's (call C117).
//!
//! The resume of a link after the loss of its carrier is `pod-session`'s:
//! a consumer runs the link over the application side of a resumable
//! session, which is a byte stream (call C120).
//!
//! The crate reads no environment variable, keeps no global state and
//! prints nothing: each error converts to a [`pod_core::Failure`] (the
//! stage `Session` for the link, `Connect` for a carrier).

mod carrier;
mod error;
pub mod frame;
mod inbound;
mod link;
pub mod memory;
mod settings;
mod state;
mod stream;
#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(unix)]
pub mod unix;

pub use carrier::{Accepted, BoxIo, Carrier, CarrierError, CarrierErrorKind, Facts, Io, Listener, Pending};
pub use error::{Error, GoAwayCode, ResetCode};
pub use frame::{Decoder, Frame, FrameError, MAX_RECORD};
pub use link::Link;
pub use settings::{INITIAL_WINDOW, MAX_STREAMS, MAX_WINDOW, MIN_FRAME, MIN_WINDOW, Settings, SettingsError};
pub use state::Role;
pub use stream::Stream;

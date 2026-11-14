//! A client of the QEMU Guest Agent (`qemu-ga`, and the agent of the
//! virtio-win drivers on Windows) over the Unix socket of its
//! virtio-serial channel. It gives a host `exec`, the copy of files and a
//! clean stop of a full guest.
//!
//! * [`Client::connect`] with its [`Settings`]: the socket, the bound of
//!   each call, the size limits. The connect syncs: `guest-sync-delimited`
//!   after the byte 0xFF, so that no old answer is read as the answer of a
//!   call.
//! * [`Client::ping`], [`Client::exec`] and [`Client::exec_status`] (and
//!   [`Client::wait_exec`]), [`Client::file_open`], [`Client::file_read`],
//!   [`Client::file_write`], [`Client::file_close`] (and
//!   [`Client::read_file`], [`Client::write_file`]), [`Client::shutdown`].
//! * Each error names its call and its kind ([`ErrorKind`]): no agent, a
//!   refusal of the agent in its own words, the bound, a closed
//!   connection, a broken answer, a limit, a bad value.
//! * [`wire`]: the requests and the answers as pure functions, on each
//!   target; the client is on Unix only.
//!
//! **No environment and no global.** The caller gives the path of the
//! socket; the crate reads no variable, keeps no process state and prints
//! nothing. The calls block: one call at a time on one connection.

pub mod error;
pub mod wire;

#[cfg(unix)]
mod client;

#[cfg(unix)]
pub use client::{Client, Settings};
pub use error::{Error, ErrorKind, Result};
pub use wire::{Answer, Chunk, Exec, ExecStatus, OpenMode, Shutdown};

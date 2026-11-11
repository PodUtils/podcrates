//! A client of the REST API of incus on its Unix socket, with no `incus`
//! or `lxc` binary and no TLS. It speaks API 1.0 of incus 6.0 and newer
//! (the 6.0 LTS is the floor).
//!
//! * [`Client`] with its [`Settings`]: the path of the socket, the project,
//!   the limits. Each call opens its own connection (HTTP/1.1, `Host:
//!   unix.socket`).
//! * The answer of each call is the envelope of incus: `sync` gives its
//!   metadata, `async` a background [`Operation`] that the crate waits for
//!   (`GET /1.0/operations/ID/wait`), `error` an [`Error`] with the words of
//!   incus and the call.
//! * [`Client::server`]: the facts of the server, and [`Server::check`]
//!   against the API extensions that the calls need ([`NEEDED`]).
//! * [`Client::import_image`]: an image from a stream: a unified tarball,
//!   or the metadata and the root file system apart (multipart).
//! * [`Client::create_instance`], [`Client::set_state`] (start, stop with
//!   `force` and `timeout`), [`Client::instances`] (`recursion=1`),
//!   [`Client::instance`], [`Client::delete_instance`],
//!   [`Client::console_log`].
//! * [`Client::exec_recorded`]: a command with `record-output`: stdout and
//!   stderr apart, and the exit code. [`Client::exec_attached`]: a command
//!   with its websockets (stdin, stdout, stderr, control) on the same
//!   socket, with the frames of `pod-ws`.
//! * [`Client::get_file`], [`Client::put_file`]: the files of an instance.
//!
//! **No environment and no global.** The caller gives the path of the
//! socket; the crate reads no variable, keeps no process state, prints
//! nothing, and writes its diagnostics to `tracing`. A call to a socket
//! where no incus answers is refused by name ([`ErrorKind::NoServer`]).

pub mod attach;
mod client;
mod envelope;
pub mod error;
pub mod exec;
pub mod files;
pub mod images;
pub mod instances;
pub mod operation;
pub mod server;
mod upload;
pub mod wire;

pub use attach::{Attached, Control, Output, Stdin};
pub use client::{Client, Settings};
pub use error::{Error, ErrorKind, Result};
pub use exec::{Exec, ExecOutput};
pub use files::{File, FileContent, FileKind, FileMeta, FileWrite};
pub use images::{ImageAlias, ImageData, ImageImport, ImportedImage};
pub use instances::{Action, Devices, Instance, Kind, NewInstance, Source, StateChange};
pub use operation::{Operation, status};
pub use server::{Environment, NEEDED, Server};
pub use upload::Payload;

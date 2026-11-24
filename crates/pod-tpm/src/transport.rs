//! The way to a TPM: one whole command out, its whole response back. The
//! codec does no I/O; a device, a socket of a TPM in software, the TBS of
//! Windows or the script of a test is a [`Transport`].
//!
//! The transports of this crate, each behind its feature:
//!
//! * `Device` (feature `device`, Unix): the device of the kernel, such as
//!   `/dev/tpmrm0`, by the path of the caller;
//! * `Socket` (feature `device`): the command channel of a TPM in
//!   software, swtpm or the simulator of the reference, on TCP or on a
//!   Unix socket;
//! * `tbs::Tbs` (feature `tbs`, Windows): the TPM Base Services; on another
//!   system the feature holds the map of its codes, `tbs::failure`.
//!
//! Each one bounds a command to [`MAX_COMMAND`] before it sends it and reads
//! at most [`MAX_RESPONSE`] bytes of a response. Each failure is a
//! [`Failure`] at `Stage::Host` with a class: `Unavailable` for no TPM,
//! `Permission` for a refusal of the system, `Timeout` for a late answer,
//! `Protocol` for a response out of its bounds, `Usage` for a command out
//! of its bounds. A transport holds its device or its connection open as
//! long as the caller holds the transport, and closes it on drop. On a
//! system with none of them (macOS has no TPM), a caller handles
//! `Unavailable` from its own transport.
//!
//! [`MAX_COMMAND`]: crate::wire::MAX_COMMAND
//! [`MAX_RESPONSE`]: crate::wire::MAX_RESPONSE

use pod_core::Failure;

use crate::cmd;
use crate::error::Error;
use crate::wire::{In, response};

/// A way to a TPM. A transport runs one command at a time, and `submit` may
/// block until the TPM answers. A failure of the way (no TPM, no device, a
/// time limit) is a [`Failure`] at `Stage::Host`: the class `Unavailable`
/// for no TPM or no device.
///
/// A closure `FnMut(&[u8]) -> Result<Vec<u8>, Failure>` is a transport, so
/// a test needs no type of its own.
pub trait Transport {
    /// Send `command` and return the whole response.
    fn submit(&mut self, command: &[u8]) -> Result<Vec<u8>, Failure>;
}

impl<F> Transport for F
where
    F: FnMut(&[u8]) -> Result<Vec<u8>, Failure>,
{
    fn submit(&mut self, command: &[u8]) -> Result<Vec<u8>, Failure> {
        self(command)
    }
}

/// A response that succeeded: its handles and its parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub handles: Vec<u32>,
    pub params: Vec<u8>,
}

impl Reply {
    /// Read the response `bytes` of a command of code `code`: the frame
    /// with its bounds, then the handles that the command gives.
    pub fn read(code: u32, bytes: &[u8]) -> Result<Reply, Error> {
        let answer = response(bytes, cmd::response_handles(code))?;
        Ok(Reply { handles: answer.handles, params: answer.params.rest().to_vec() })
    }

    /// A reader of the parameters.
    pub fn params(&self) -> In<'_> {
        In::new(&self.params)
    }

    /// The first handle.
    pub fn handle(&self) -> Result<u32, Error> {
        self.handles.first().copied().ok_or_else(|| Error::Malformed("no handle came".into()))
    }

    /// The end of a response that gives no parameter: a parameter is an
    /// error.
    pub fn empty(&self) -> Result<(), Error> {
        self.params().finish("a response with no parameter")
    }
}

/// Send `command` and read its response. The code of the command comes
/// from its header, and it tells how many handles the response holds.
pub fn transact<T: Transport + ?Sized>(transport: &mut T, command: &[u8]) -> Result<Reply, Error> {
    let Some(&[a, b, c, d]) = command.get(6..10) else {
        return Err(Error::Usage("a command is shorter than its header".into()));
    };
    let code = u32::from_be_bytes([a, b, c, d]);
    let bytes = transport.submit(command).map_err(Error::Transport)?;
    Reply::read(code, &bytes)
}

/// The bounds of a command that a transport sends: at least its header, at
/// most [`crate::wire::MAX_COMMAND`]. A command out of them is refused
/// before it is sent, with the class `Usage`.
#[cfg(any(feature = "device", all(feature = "tbs", windows)))]
pub(crate) fn check_command(command: &[u8]) -> Result<(), Failure> {
    use crate::wire::{HEADER, MAX_COMMAND};
    if (HEADER..=MAX_COMMAND).contains(&command.len()) {
        return Ok(());
    }
    let why = format!("a command of {} bytes: a TPM takes {HEADER} to {MAX_COMMAND} bytes", command.len());
    Err(Failure::new(pod_core::Stage::Host, pod_core::Class::Usage, why))
}

#[cfg(feature = "device")]
mod bound;
#[cfg(feature = "device")]
pub use bound::Limits;
#[cfg(all(feature = "device", unix))]
mod device;
#[cfg(all(feature = "device", unix))]
pub use device::Device;
#[cfg(feature = "device")]
mod socket;
#[cfg(feature = "device")]
pub use socket::{Framing, Socket};
#[cfg(feature = "tbs")]
pub mod tbs;

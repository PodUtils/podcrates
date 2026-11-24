//! The way to a TPM: one whole command out, its whole response back. The
//! codec does no I/O; a device, a socket of a TPM in software, the TBS of
//! Windows or the script of a test is a [`Transport`].

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

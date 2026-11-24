//! The marshalling of TPM 2.0 (the Library specification, part 1, clause 18;
//! part 2, clauses 5 and 10): integers big-endian, sized buffers (`TPM2B`)
//! with a length of two bytes, and the frame of a command and of its
//! response. Bytes in, bytes out: nothing here talks to a device, so each
//! rule is tested on byte vectors.
//!
//! A response is untrusted input. Each read checks the bytes that remain
//! before it takes them, and each count is checked against the bytes that
//! remain before an allocation.

use crate::error::Error;
use crate::rc::ResponseCode;

/// The tag of a command or a response with no authorization area.
pub const ST_NO_SESSIONS: u16 = 0x8001;
/// The tag of a command or a response with an authorization area.
pub const ST_SESSIONS: u16 = 0x8002;
/// The tag of a response of a TPM 1.2 (`TPM_TAG_RSP_COMMAND`).
pub const ST_TPM12_RESPONSE: u16 = 0x00C4;
/// The password session (`TPM_RS_PW`).
pub const RS_PW: u32 = 0x4000_0009;
/// The owner hierarchy (`TPM_RH_OWNER`).
pub const RH_OWNER: u32 = 0x4000_0001;
/// The handle of nothing (`TPM_RH_NULL`).
pub const RH_NULL: u32 = 0x4000_0007;
/// The endorsement hierarchy (`TPM_RH_ENDORSEMENT`).
pub const RH_ENDORSEMENT: u32 = 0x4000_000B;
/// The platform hierarchy (`TPM_RH_PLATFORM`).
pub const RH_PLATFORM: u32 = 0x4000_000C;
/// The header of each command and response: a tag, a size, a code.
pub const HEADER: usize = 10;
/// The most bytes of a command. The Linux driver refuses a write of more
/// than `TPM_BUFSIZE` (4096) with `E2BIG`.
pub const MAX_COMMAND: usize = 4096;
/// The most bytes of a response: `TPM_BUFSIZE` (4096), the buffer of the
/// Linux driver for one response. A longer answer is no TPM's.
pub const MAX_RESPONSE: usize = 4096;
/// The most bytes of a `TPM2B_DIGEST` or a `TPM2B_NONCE`: the largest digest
/// (`sizeof(TPMU_HA)`, SHA-512).
pub const MAX_DIGEST: usize = 64;
/// The most bytes of a `TPM2B_NAME`: the algorithm and the largest digest.
pub const MAX_NAME: usize = 2 + MAX_DIGEST;

/// A buffer that the fields of a command go into. A sized buffer longer
/// than its length field can hold marks the whole as unusable: a cut length
/// sends another command than the one meant.
#[derive(Default)]
pub struct Out {
    bytes: Vec<u8>,
    too_long: bool,
}

impl Out {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.bytes.push(v);
        self
    }

    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn raw(&mut self, b: &[u8]) -> &mut Self {
        self.bytes.extend_from_slice(b);
        self
    }

    /// A `TPM2B`: the length in two bytes, then the bytes.
    pub fn sized(&mut self, b: &[u8]) -> &mut Self {
        match u16::try_from(b.len()) {
            Ok(n) => {
                self.u16(n);
                self.raw(b)
            }
            Err(_) => {
                self.too_long = true;
                self
            }
        }
    }

    /// The bytes, or why there are none.
    pub fn finish(self) -> Result<Vec<u8>, Error> {
        if self.too_long {
            return Err(Error::Usage("a field of the command is longer than a TPM takes".into()));
        }
        Ok(self.bytes)
    }
}

/// One entry of the authorization area of a command (`TPMS_AUTH_COMMAND`).
#[derive(Debug, Clone, Copy)]
pub struct Auth<'a> {
    /// The session handle, or [`RS_PW`].
    pub handle: u32,
    pub nonce: &'a [u8],
    /// `TPMA_SESSION`: `continueSession` is bit 0.
    pub attributes: u8,
    /// The HMAC, or the password of a password session.
    pub hmac: &'a [u8],
}

impl Auth<'static> {
    /// The password session with the empty password: the owner hierarchy and
    /// a PCR take it when nobody set a password.
    pub const EMPTY_PASSWORD: Auth<'static> = Auth { handle: RS_PW, nonce: &[], attributes: 0, hmac: &[] };
}

impl<'a> Auth<'a> {
    /// A policy session with a fresh nonce of the caller and no HMAC: the
    /// policy of the object asks for no value. The session ends with the
    /// command (no `continueSession`).
    pub fn policy(session: u32, nonce: &'a [u8]) -> Auth<'a> {
        Auth { handle: session, nonce, attributes: 0, hmac: &[] }
    }
}

/// A command: its header, its handles, its authorization area when it has
/// sessions, and its parameters. A command over [`MAX_COMMAND`] is refused
/// before it is sent.
pub fn command(code: u32, handles: &[u32], sessions: &[Auth], params: &[u8]) -> Result<Vec<u8>, Error> {
    let mut auth = Out::default();
    for s in sessions {
        auth.u32(s.handle).sized(s.nonce).u8(s.attributes).sized(s.hmac);
    }
    let auth = auth.finish()?;
    let mut body = Out::default();
    for h in handles {
        body.u32(*h);
    }
    if !sessions.is_empty() {
        let size = u32::try_from(auth.len()).map_err(|_| Error::Usage("the authorization area is too long".into()))?;
        body.u32(size).raw(&auth);
    }
    body.raw(params);
    let body = body.finish()?;
    let size = HEADER + body.len();
    if size > MAX_COMMAND {
        return Err(Error::Usage(format!(
            "a command of {size} bytes is longer than the {MAX_COMMAND} that a TPM takes"
        )));
    }
    let tag = if sessions.is_empty() { ST_NO_SESSIONS } else { ST_SESSIONS };
    let mut out = Out::default();
    // The size is at most MAX_COMMAND, so it fits in a u32.
    out.u16(tag).u32(size as u32).u32(code).raw(&body);
    out.finish()
}

/// A response that succeeded: its tag, its handles, then its parameters.
pub struct Response<'a> {
    pub tag: u16,
    pub handles: Vec<u32>,
    pub params: In<'a>,
}

/// Read a response that carries `handles` handles. Its size field must be
/// the count of bytes that came, from [`HEADER`] to [`MAX_RESPONSE`]; a
/// code other than success is [`Error::Tpm`].
pub fn response(bytes: &[u8], handles: usize) -> Result<Response<'_>, Error> {
    if bytes.len() > MAX_RESPONSE {
        return Err(Error::Malformed(format!(
            "{} bytes came, more than the {MAX_RESPONSE} of a response",
            bytes.len()
        )));
    }
    let mut input = In::new(bytes);
    let tag = input.u16()?;
    let size = input.u32()?;
    let code = input.u32()?;
    if tag == ST_TPM12_RESPONSE {
        return Err(Error::Malformed(format!("its tag is {tag:#06x}: the answer of a TPM 1.2, not of a TPM 2.0")));
    }
    if tag != ST_NO_SESSIONS && tag != ST_SESSIONS {
        return Err(Error::Malformed(format!("its tag is {tag:#06x}, which no TPM 2.0 gives")));
    }
    if usize::try_from(size).ok() != Some(bytes.len()) {
        return Err(Error::Malformed(format!("it says {size} bytes, and {} came", bytes.len())));
    }
    if code != 0 {
        return Err(Error::Tpm(ResponseCode::new(code)));
    }
    let mut out = Vec::with_capacity(handles.min(input.remaining() / 4));
    for _ in 0..handles {
        out.push(input.u32()?);
    }
    if tag == ST_SESSIONS {
        // The parameters have their own size; the authorization area of the
        // TPM follows them and is not read.
        let length = input.u32()?;
        let length = usize::try_from(length).unwrap_or(usize::MAX);
        let params = input.take(length)?;
        return Ok(Response { tag, handles: out, params: In::new(params) });
    }
    Ok(Response { tag, handles: out, params: input })
}

/// A reader of marshalled fields: each read past the end is an error.
#[derive(Clone, Debug)]
pub struct In<'a> {
    rest: &'a [u8],
}

impl<'a> In<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        In { rest: bytes }
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.rest.len() < n {
            return Err(Error::Malformed(format!("{n} bytes were due and {} are left", self.rest.len())));
        }
        let (head, tail) = self.rest.split_at(n);
        self.rest = tail;
        Ok(head)
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        let b = self.take(1)?;
        Ok(b.first().copied().unwrap_or_default())
    }

    pub fn u16(&mut self) -> Result<u16, Error> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// The bytes of a `TPM2B`.
    pub fn sized(&mut self) -> Result<&'a [u8], Error> {
        let n = self.u16()?;
        self.take(usize::from(n))
    }

    /// The bytes of a `TPM2B` of at most `max` bytes, the bound of part 2
    /// for `what`.
    pub fn sized_max(&mut self, max: usize, what: &str) -> Result<&'a [u8], Error> {
        let n = usize::from(self.u16()?);
        if n > max {
            return Err(Error::Malformed(format!("{what} of {n} bytes, where part 2 allows {max}")));
        }
        self.take(n)
    }

    /// A `TPM2B` whole, its length included: what a key file keeps.
    pub fn sized_whole(&mut self) -> Result<&'a [u8], Error> {
        let start = self.rest;
        let n = usize::from(self.u16()?);
        self.take(n)?;
        Ok(&start[..2 + n])
    }

    /// The count of a list (`TPML_*`) whose items have at least `item` bytes
    /// each, and at most `max` items. A count that passes the bytes that
    /// remain is refused before anything is allocated for it.
    pub fn count(&mut self, item: usize, max: usize, what: &str) -> Result<usize, Error> {
        let n = self.u32()?;
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        if n > max {
            return Err(Error::Malformed(format!("a list of {n} {what}, where part 2 allows {max}")));
        }
        if n.saturating_mul(item) > self.rest.len() {
            return Err(Error::Malformed(format!(
                "a list of {n} {what} in {} bytes, which cannot hold them",
                self.rest.len()
            )));
        }
        Ok(n)
    }

    pub fn rest(&self) -> &'a [u8] {
        self.rest
    }

    pub fn remaining(&self) -> usize {
        self.rest.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rest.is_empty()
    }

    /// The end of a structure: a byte after it is an error.
    pub fn finish(&self, what: &str) -> Result<(), Error> {
        if self.rest.is_empty() {
            return Ok(());
        }
        Err(Error::Malformed(format!("{} bytes after {what}", self.rest.len())))
    }
}

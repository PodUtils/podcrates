//! A TPM in software (feature `soft`), for the tests of this crate and of a
//! consumer: a [`Transport`] that reads the fourteen commands of [`crate::cmd`]
//! as a TPM reads them (the Library specification, part 3) and answers with
//! each field of each response, so the strict readers of the crate take
//! them. **It is a model, not a TPM:** it protects no secret (each key comes
//! from its seed, and a test reads it), it has one PCR bank (SHA-256), no
//! NV memory, no HMAC session, no salt and no lockout. The proof against a
//! TPM is the lane of CI against swtpm and the simulator of the reference
//! (`scripts/ci/tpm-tests.sh`).
//!
//! What it holds is what a key under a policy rests on:
//!
//! * a key loads only under the parent that made it, in a TPM of the same
//!   seed: its private part carries a check that the parent's secret keys
//!   (`TPM_RC_INTEGRITY`);
//! * a key with no `userWithAuth` refuses the password session
//!   (`TPM_RC_AUTH_UNAVAILABLE`), and a policy session signs only when its
//!   digest is the key's `authPolicy` (`TPM_RC_POLICY_FAIL`);
//! * `TPM2_PolicyPCR` with a digest that the PCRs do not hold is refused
//!   outside a trial session (`TPM_RC_VALUE`);
//! * `TPM2_VerifySignature` checks ECDSA P-256 (`TPM_RC_SIGNATURE`), and
//!   `TPM2_PolicyAuthorize` outside a trial takes only the session's digest
//!   and a ticket that this TPM made (`TPM_RC_VALUE`);
//! * three transient objects and three sessions at most, as the PC Client
//!   profile asks (`TPM_RC_OBJECT_MEMORY`, `TPM_RC_SESSION_MEMORY`), so a
//!   handle that a sequence does not flush shows.
//!
//! A command or a value that the model does not take is refused with the
//! response code that a TPM gives for it. Its digests of a policy follow
//! part 3 from the bytes of the command; they do not call
//! [`crate::policy`], so the model checks the crate's digests.

mod object;
mod session;

use std::collections::BTreeMap;

use p256::ecdsa::SigningKey;
use pod_core::Failure;
use sha2::{Digest, Sha256};

use crate::cmd::*;
use crate::rc::Code;
use crate::transport::Transport;
use crate::wire::{HEADER, In, Out, RS_PW, ST_NO_SESSIONS, ST_SESSIONS};

/// A response code, as the TPM gives it.
type Rc = u32;

/// The most transient objects and the most loaded sessions: the least that
/// the PC Client profile asks (`TPM_PT_HR_TRANSIENT_MIN`,
/// `TPM_PT_HR_LOADED_MIN`).
pub const SLOTS: usize = 3;

/// The count of PCRs of the SHA-256 bank.
pub const PCRS: usize = 24;

/// What a handle of the model holds.
enum Object {
    /// A storage key: a primary, or a persistent key. Its secret keys the
    /// private part of each key under it.
    Storage { name: Vec<u8>, secret: [u8; 32], hierarchy: u32 },
    /// A signing key that `TPM2_Load` loaded.
    Key { area: crate::EccPublic, key: SigningKey, name: Vec<u8> },
    /// A public key that `TPM2_LoadExternal` loaded.
    External { area: crate::EccPublic, name: Vec<u8>, hierarchy: u32 },
}

/// A policy session (or a trial one) of SHA-256.
struct Session {
    trial: bool,
    digest: Vec<u8>,
}

/// One entry of the authorization area of a command.
struct AuthIn {
    handle: u32,
    attributes: u8,
    hmac: Vec<u8>,
}

/// The TPM in software. Two models of the same seed are the same TPM: a key
/// file of one loads in the other.
pub struct SoftTpm {
    seed: [u8; 32],
    pcrs: [[u8; 32]; PCRS],
    counter: u32,
    objects: BTreeMap<u32, Object>,
    persistent: BTreeMap<u32, Object>,
    sessions: BTreeMap<u32, Session>,
    next: u32,
    sent: Vec<Vec<u8>>,
}

impl SoftTpm {
    /// A TPM whose keys, nonces and tickets come from `seed`, with each PCR
    /// at zero and no persistent key.
    pub fn new(seed: [u8; 32]) -> SoftTpm {
        SoftTpm {
            seed,
            pcrs: [[0; 32]; PCRS],
            counter: 0,
            objects: BTreeMap::new(),
            persistent: BTreeMap::new(),
            sessions: BTreeMap::new(),
            next: 0,
            sent: Vec::new(),
        }
    }

    /// Extend PCR `pcr` with the SHA-256 of `data`, as a boot measures an
    /// event. A PCR past the bank is ignored.
    pub fn extend(&mut self, pcr: usize, data: &[u8]) {
        let event = sha256(&[data]);
        if let Some(value) = self.pcrs.get_mut(pcr) {
            *value = sha256(&[&value[..], &event]);
            self.counter = self.counter.wrapping_add(1);
        }
    }

    /// A storage key (the template H-2) at the persistent handle `handle`,
    /// as `tpm2_evictcontrol` keeps one at `0x81000001`.
    pub fn persist(&mut self, handle: u32) {
        let secret = self.derive(b"persistent", &[&handle.to_be_bytes()]);
        let name = handle.to_be_bytes().to_vec();
        self.persistent.insert(handle, Object::Storage { name, secret, hierarchy: crate::wire::RH_OWNER });
    }

    /// The value of PCR `pcr` of the SHA-256 bank.
    pub fn pcr(&self, pcr: usize) -> [u8; 32] {
        self.pcrs.get(pcr).copied().unwrap_or_default()
    }

    /// The count of transient objects and sessions loaded now: none after
    /// each sequence that ended.
    pub fn loaded(&self) -> usize {
        self.objects.len() + self.sessions.len()
    }

    /// Each command that came, as it came.
    pub fn commands(&self) -> &[Vec<u8>] {
        &self.sent
    }

    /// 32 bytes from the seed, `label` and `parts`: each key, nonce, secret
    /// and ticket of the model.
    fn derive(&self, label: &[u8], parts: &[&[u8]]) -> [u8; 32] {
        let mut all: Vec<&[u8]> = vec![&self.seed, label];
        all.extend_from_slice(parts);
        sha256(&all)
    }

    /// A fresh number for a handle or a nonce.
    fn fresh(&mut self) -> u32 {
        self.next = self.next.wrapping_add(1);
        self.next
    }

    /// Run one command: the tag, the handles and the parameters of its
    /// response, and the area of its sessions; or a response code.
    fn run(&mut self, bytes: &[u8]) -> Result<Done, Rc> {
        let mut input = In::new(bytes);
        let header = (input.u16(), input.u32(), input.u32());
        let (Ok(tag), Ok(size), Ok(code)) = header else {
            return Err(Code::CommandSize.value());
        };
        if usize::try_from(size).ok() != Some(bytes.len()) {
            return Err(Code::CommandSize.value());
        }
        if tag != ST_NO_SESSIONS && tag != ST_SESSIONS {
            return Err(BAD_TAG);
        }
        let (handle_count, auth_count) = shape(code).ok_or(Code::CommandCode.value())?;
        let mut handles = Vec::with_capacity(handle_count);
        for _ in 0..handle_count {
            handles.push(input.u32().map_err(|_| Code::CommandSize.value())?);
        }
        let auths = if tag == ST_SESSIONS { read_auths(&mut input)? } else { Vec::new() };
        if auth_count == 0 && !auths.is_empty() {
            return Err(Code::AuthContext.value());
        }
        if auths.len() < auth_count {
            return Err(Code::AuthMissing.value());
        }
        for (n, handle) in handles.iter().enumerate() {
            self.check_handle(code, *handle, n as u32 + 1)?;
        }
        for (n, auth) in auths.iter().enumerate() {
            self.authorize(handles[n], auth, n as u32 + 1)?;
        }
        let (out_handles, params) = self.dispatch(code, &handles, &mut input)?;
        let mut done = Done { tag, handles: out_handles, params, sessions: Vec::new() };
        for auth in &auths {
            if auth.handle == RS_PW {
                done.sessions.push((Vec::new(), CONTINUE));
                continue;
            }
            let number = self.fresh();
            let nonce = self.derive(b"nonce", &[&number.to_be_bytes()]).to_vec();
            done.sessions.push((nonce, auth.attributes & CONTINUE));
            if auth.attributes & CONTINUE == 0 {
                self.sessions.remove(&auth.handle);
            }
        }
        Ok(done)
    }

    /// A handle of the handle area must name what the command takes: a
    /// loaded object or session, a persistent key, a hierarchy, a PCR.
    fn check_handle(&self, code: u32, handle: u32, n: u32) -> Result<(), Rc> {
        let present = match handle >> 24 {
            0x80 => self.objects.contains_key(&handle),
            0x02 | 0x03 => self.sessions.contains_key(&handle),
            0x81 => {
                if !self.persistent.contains_key(&handle) {
                    return Err(on_handle(Code::Handle, n));
                }
                true
            }
            _ => true,
        };
        if !present {
            return Err(Code::ReferenceH0.value() + n - 1);
        }
        let session = code == CC_POLICY_PCR || code == CC_POLICY_AUTHORIZE || code == CC_POLICY_GET_DIGEST;
        if session && handle >> 24 != 0x03 {
            return Err(on_handle(Code::Handle, n));
        }
        Ok(())
    }

    /// The authorization of the entity `entity` by `auth`, the session `n`.
    /// Each object, hierarchy and PCR of the model has the empty password;
    /// a signing key has no `userWithAuth` and takes its policy alone.
    fn authorize(&self, entity: u32, auth: &AuthIn, n: u32) -> Result<(), Rc> {
        let key = match self.objects.get(&entity) {
            Some(Object::Key { area, .. }) => Some(area),
            _ => None,
        };
        if auth.handle == RS_PW {
            if !auth.hmac.is_empty() {
                return Err(on_session(Code::AuthFail, n));
            }
            return match key {
                Some(area) if area.attributes & crate::public::USER_WITH_AUTH == 0 => {
                    Err(Code::AuthUnavailable.value())
                }
                _ => Ok(()),
            };
        }
        let Some(session) = self.sessions.get(&auth.handle) else {
            return Err(Code::ReferenceS0.value() + n - 1);
        };
        match key {
            Some(area) if !session.trial && session.digest == area.auth_policy => Ok(()),
            _ => Err(on_session(Code::PolicyFail, n)),
        }
    }

    fn dispatch(&mut self, code: u32, handles: &[u32], p: &mut In) -> Answer {
        match code {
            CC_CREATE_PRIMARY => self.create_primary(handles[0], p),
            CC_CREATE => self.create(handles[0], p),
            CC_LOAD => self.load(handles[0], p),
            CC_LOAD_EXTERNAL => self.load_external(p),
            CC_SIGN => self.sign(handles[0], p),
            CC_VERIFY_SIGNATURE => self.verify_signature(handles[0], p),
            CC_FLUSH_CONTEXT => self.flush(p),
            CC_START_AUTH_SESSION => self.start_auth_session(handles, p),
            CC_POLICY_PCR => self.policy_pcr(handles[0], p),
            CC_POLICY_AUTHORIZE => self.policy_authorize(handles[0], p),
            CC_POLICY_GET_DIGEST => self.policy_get_digest(handles[0], p),
            CC_PCR_READ => self.pcr_read(p),
            CC_PCR_EXTEND => self.pcr_extend(handles[0], p),
            CC_GET_CAPABILITY => self.get_capability(p),
            _ => Err(Code::CommandCode.value()),
        }
    }
}

/// The handles and the parameters of a response, or a response code.
type Answer = Result<(Vec<u32>, Vec<u8>), Rc>;

/// A command that succeeded.
struct Done {
    tag: u16,
    handles: Vec<u32>,
    params: Vec<u8>,
    /// For each session of the command: the nonce of the TPM and the
    /// attributes of the response.
    sessions: Vec<(Vec<u8>, u8)>,
}

/// `continueSession`, bit 0 of `TPMA_SESSION`.
const CONTINUE: u8 = 0x01;

/// `TPM_RC_BAD_TAG`: a code of the format of a TPM 1.2 (part 2, 6.6.3).
const BAD_TAG: Rc = 0x01E;

/// The count of handles and of authorizations of the command `code`, or
/// `None` for a command that the model does not have.
fn shape(code: u32) -> Option<(usize, usize)> {
    Some(match code {
        CC_CREATE_PRIMARY | CC_CREATE | CC_LOAD | CC_SIGN | CC_PCR_EXTEND => (1, 1),
        CC_VERIFY_SIGNATURE | CC_POLICY_PCR | CC_POLICY_AUTHORIZE | CC_POLICY_GET_DIGEST => (1, 0),
        CC_START_AUTH_SESSION => (2, 0),
        CC_LOAD_EXTERNAL | CC_FLUSH_CONTEXT | CC_PCR_READ | CC_GET_CAPABILITY => (0, 0),
        _ => return None,
    })
}

/// The authorization area (`TPMS_AUTH_COMMAND` each, three at most).
fn read_auths(input: &mut In) -> Result<Vec<AuthIn>, Rc> {
    let bad = |_| Code::AuthSize.value();
    let size = input.u32().map_err(bad)?;
    let mut area = In::new(input.take(usize::try_from(size).unwrap_or(usize::MAX)).map_err(bad)?);
    let mut auths = Vec::new();
    while !area.is_empty() {
        if auths.len() == 3 {
            return Err(Code::AuthSize.value());
        }
        let handle = area.u32().map_err(bad)?;
        area.sized().map_err(bad)?;
        let attributes = area.u8().map_err(bad)?;
        let hmac = area.sized().map_err(bad)?.to_vec();
        auths.push(AuthIn { handle, attributes, hmac });
    }
    Ok(auths)
}

/// A format-one code of parameter `n`.
fn on_param(code: Code, n: u32) -> Rc {
    code.value() | 0x040 | (n << 8)
}

/// A format-one code of handle `n`.
fn on_handle(code: Code, n: u32) -> Rc {
    code.value() | (n << 8)
}

/// A format-one code of session `n`.
fn on_session(code: Code, n: u32) -> Rc {
    code.value() | 0x800 | (n << 8)
}

/// A parameter that does not unmarshal: `TPM_RC_INSUFFICIENT` of
/// parameter `n`.
fn short(n: u32) -> impl Fn(crate::Error) -> Rc {
    move |_| on_param(Code::Insufficient, n)
}

/// The end of the parameters: a byte after them is `TPM_RC_SIZE`.
fn end(p: &In) -> Result<(), Rc> {
    if p.is_empty() { Ok(()) } else { Err(Code::Size.value()) }
}

/// The SHA-256 of the concatenation of `parts`.
fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part);
    }
    hash.finalize().into()
}

/// The bytes of `out`, which hold no field longer than its length.
fn bytes(out: Out) -> Vec<u8> {
    out.finish().unwrap_or_default()
}

impl Transport for SoftTpm {
    fn submit(&mut self, command: &[u8]) -> Result<Vec<u8>, Failure> {
        self.sent.push(command.to_vec());
        let mut out = Out::default();
        match self.run(command) {
            Ok(done) => {
                let mut body = Out::default();
                for handle in &done.handles {
                    body.u32(*handle);
                }
                if done.tag == ST_SESSIONS {
                    // The parameters have a size of their own; one answer
                    // of each session comes after them.
                    body.u32(done.params.len() as u32).raw(&done.params);
                    for (nonce, attributes) in &done.sessions {
                        body.sized(nonce).u8(*attributes).sized(&[]);
                    }
                } else {
                    body.raw(&done.params);
                }
                let body = bytes(body);
                out.u16(done.tag).u32((HEADER + body.len()) as u32).u32(0).raw(&body);
            }
            Err(code) => {
                out.u16(ST_NO_SESSIONS).u32(HEADER as u32).u32(code);
            }
        }
        Ok(bytes(out))
    }
}

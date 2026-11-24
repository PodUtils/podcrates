//! A fake TPM for the tests of the sequences: it answers the fourteen
//! commands with whole responses, keeps the handles that it gave, keeps
//! each command that came, and fails at one command of the caller's
//! choice. It computes the policy digest of a session with the crate's own
//! functions and takes a "signature" of a policy whose `r` is the digest
//! it signs. It is no model of a TPM: the proof against a TPM is T-1222's.

use pod_core::entropy::{Entropy, NoRandom};
use pod_core::{Class, Failure, Stage};
use pod_tpm::policy::{self, key_name, policy_authorize_digest, policy_pcr_update};
use pod_tpm::wire::{In, Out, RH_NULL};
use pod_tpm::{EccPublic, HashAlg, PcrSelection};

/// How the command of the index `fail_at` fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fail {
    /// The TPM refuses it with this response code.
    Tpm(u32),
    /// The transport gives no response; the TPM does not see the command.
    Transport,
    /// The TPM runs it, and the response holds its handles and no
    /// parameter.
    Short,
}

#[derive(Debug, Clone)]
enum Obj {
    Primary,
    Key { auth_policy: Vec<u8> },
    External,
    Session { trial: bool, digest: Vec<u8> },
}

/// The fake TPM.
pub struct FakeTpm {
    pub pcrs: [[u8; 32]; 24],
    /// The private part of each key carries it: another seed is another TPM.
    pub seed: u8,
    pub persistent: Vec<u32>,
    pub sent: Vec<Vec<u8>>,
    pub fail_at: Option<(usize, Fail)>,
    /// A flush of a handle that is not loaded.
    pub bad_flushes: usize,
    /// The count of handles that the TPM gave.
    pub given: usize,
    /// The digest of a trial session comes back changed.
    pub other_trial: bool,
    live: Vec<(u32, Obj)>,
    next: u32,
    counter: u32,
}

/// The code of a refusal with the number of a parameter or a handle.
const P1: u32 = 0x040 | 0x100;
pub const RC_VALUE_P1: u32 = 0x084 | P1;
pub const RC_HANDLE_H1: u32 = 0x08B | 0x100;
pub const RC_SIGNATURE_P2: u32 = 0x09B | 0x040 | 0x200;
pub const RC_POLICY_FAIL_S1: u32 = 0x09D | 0x800 | 0x100;
pub const RC_INTEGRITY_P1: u32 = 0x09F | P1;
pub const RC_FAILURE: u32 = 0x101;

impl FakeTpm {
    pub fn new() -> FakeTpm {
        FakeTpm {
            pcrs: [[0; 32]; 24],
            seed: 1,
            persistent: vec![0x8100_0001],
            sent: Vec::new(),
            fail_at: None,
            bad_flushes: 0,
            given: 0,
            other_trial: false,
            live: Vec::new(),
            next: 0,
            counter: 0,
        }
    }

    /// A fake TPM with the PCRs `pcrs` that fails as `fail_at` says.
    pub fn with(pcrs: [[u8; 32]; 24], fail_at: Option<(usize, Fail)>) -> FakeTpm {
        FakeTpm { pcrs, fail_at, ..FakeTpm::new() }
    }

    /// Extend PCR `n` of the SHA-256 bank with the hash of `data`.
    pub fn extend(&mut self, n: usize, data: &[u8]) {
        let digest = policy::hash(HashAlg::Sha256, &[data]).unwrap();
        let new = policy::hash(HashAlg::Sha256, &[&self.pcrs[n], &digest]).unwrap();
        self.pcrs[n].copy_from_slice(&new);
        self.counter += 1;
    }

    /// The count of transient handles and sessions loaded now.
    pub fn loaded(&self) -> usize {
        self.live.len()
    }

    /// The count of commands of `code` that came.
    pub fn count(&self, code: u32) -> usize {
        self.sent.iter().filter(|c| c[6..10] == code.to_be_bytes()).count()
    }

    fn give(&mut self, obj: Obj) -> u32 {
        self.next += 1;
        self.given += 1;
        let base = if matches!(obj, Obj::Session { .. }) { 0x0300_0000 } else { 0x8000_0000 };
        let handle = base + self.next;
        self.live.push((handle, obj));
        handle
    }

    fn get(&mut self, handle: u32) -> Option<&mut Obj> {
        self.live.iter_mut().find(|(h, _)| *h == handle).map(|(_, o)| o)
    }

    fn parent_ok(&mut self, handle: u32) -> bool {
        self.persistent.contains(&handle) || matches!(self.get(handle), Some(Obj::Primary))
    }

    fn values_digest(&self, selection: &PcrSelection) -> Vec<u8> {
        let values: Vec<&[u8]> = selection.pcrs(HashAlg::Sha256).iter().map(|n| &self.pcrs[*n as usize][..]).collect();
        policy::pcr_values_digest(HashAlg::Sha256, &values).unwrap()
    }

    /// Run one command: the handles of the response and its parameters, or
    /// a response code.
    fn run(&mut self, code: u32, handles: &[u32], auth: Option<u32>, p: &mut In) -> Result<(Vec<u32>, Vec<u8>), u32> {
        let mut out = Out::default();
        let digest32 = [0u8; 32];
        let name = |out: &mut Out| {
            out.sized(&[[0, 0x0B].as_slice(), &[0; 32]].concat());
        };
        let creation = |out: &mut Out| {
            out.sized(&[]).sized(&digest32).u16(0x8021).u32(0x4000_0001).sized(&[]);
        };
        match code {
            0x131 => {
                p.sized().unwrap();
                let public = p.sized_whole().unwrap().to_vec();
                let h = self.give(Obj::Primary);
                out.raw(&public);
                creation(&mut out);
                name(&mut out);
                Ok((vec![h], out.finish().unwrap()))
            }
            0x153 => {
                if !self.parent_ok(handles[0]) {
                    return Err(RC_HANDLE_H1);
                }
                p.sized().unwrap();
                let public = p.sized_whole().unwrap().to_vec();
                out.sized(&[self.seed, 0xAA, 0xBB]).raw(&public);
                creation(&mut out);
                Ok((vec![], out.finish().unwrap()))
            }
            0x157 => {
                if !self.parent_ok(handles[0]) {
                    return Err(RC_HANDLE_H1);
                }
                let private = p.sized().unwrap().to_vec();
                if private.first() != Some(&self.seed) {
                    return Err(RC_INTEGRITY_P1);
                }
                let area = EccPublic::read(p.sized_whole().unwrap()).unwrap();
                let h = self.give(Obj::Key { auth_policy: area.auth_policy });
                name(&mut out);
                Ok((vec![h], out.finish().unwrap()))
            }
            0x167 => {
                p.u16().unwrap();
                let public = p.sized_whole().unwrap().to_vec();
                let h = self.give(Obj::External);
                out.sized(&key_name(&public).unwrap());
                Ok((vec![h], out.finish().unwrap()))
            }
            0x176 => {
                p.sized().unwrap();
                p.sized().unwrap();
                let trial = p.u8().unwrap() == 3;
                let h = self.give(Obj::Session { trial, digest: vec![0; 32] });
                out.sized(&[7; 32]);
                Ok((vec![h], out.finish().unwrap()))
            }
            0x17F => {
                let given = p.sized().unwrap().to_vec();
                let selection = PcrSelection::read(p).unwrap();
                let now = self.values_digest(&selection);
                let Some(Obj::Session { trial, digest }) = self.get(handles[0]) else { return Err(RC_HANDLE_H1) };
                if !*trial && !given.is_empty() && given != now {
                    return Err(RC_VALUE_P1);
                }
                let used = if given.is_empty() { now } else { given };
                *digest = policy_pcr_update(HashAlg::Sha256, digest, &selection, &used).unwrap();
                Ok((vec![], vec![]))
            }
            0x16A => {
                let approved = p.sized().unwrap().to_vec();
                let reference = p.sized().unwrap().to_vec();
                let key = p.sized().unwrap().to_vec();
                p.u16().unwrap();
                let hierarchy = p.u32().unwrap();
                let Some(Obj::Session { trial, digest }) = self.get(handles[0]) else { return Err(RC_HANDLE_H1) };
                if !*trial && (hierarchy == RH_NULL || approved != *digest) {
                    return Err(RC_VALUE_P1);
                }
                *digest = policy_authorize_digest(HashAlg::Sha256, &key, &reference).unwrap();
                Ok((vec![], vec![]))
            }
            0x177 => {
                let signed = p.sized().unwrap().to_vec();
                p.u16().unwrap();
                p.u16().unwrap();
                let r = p.sized().unwrap().to_vec();
                if !matches!(self.get(handles[0]), Some(Obj::External)) {
                    return Err(RC_HANDLE_H1);
                }
                if r != signed {
                    return Err(RC_SIGNATURE_P2);
                }
                out.u16(0x8022).u32(0x4000_0001).sized(&digest32);
                Ok((vec![], out.finish().unwrap()))
            }
            0x189 => {
                let other = self.other_trial;
                let Some(Obj::Session { trial, digest }) = self.get(handles[0]) else { return Err(RC_HANDLE_H1) };
                let mut d = digest.clone();
                if *trial && other {
                    d[31] ^= 1;
                }
                out.sized(&d);
                Ok((vec![], out.finish().unwrap()))
            }
            0x15D => {
                let session = auth.unwrap_or_default();
                let Some(Obj::Key { auth_policy }) = self.get(handles[0]).cloned() else { return Err(RC_HANDLE_H1) };
                match self.get(session) {
                    Some(Obj::Session { trial: false, digest }) if *digest == auth_policy => {}
                    _ => return Err(RC_POLICY_FAIL_S1),
                }
                self.live.retain(|(h, _)| *h != session);
                out.u16(0x18).u16(0x0B).sized(&[0x11; 32]).sized(&[0x22; 32]);
                Ok((vec![], out.finish().unwrap()))
            }
            0x165 => {
                let h = p.u32().unwrap();
                let before = self.live.len();
                self.live.retain(|(x, _)| *x != h);
                if self.live.len() == before {
                    self.bad_flushes += 1;
                    return Err(RC_HANDLE_H1);
                }
                Ok((vec![], vec![]))
            }
            0x17E => {
                let asked = PcrSelection::read(p).unwrap();
                let pcrs: Vec<u32> = asked.pcrs(HashAlg::Sha256).into_iter().take(8).collect();
                let read = PcrSelection::of(HashAlg::Sha256, &pcrs).unwrap();
                out.u32(self.counter).raw(&read.marshal()).u32(pcrs.len() as u32);
                for n in &pcrs {
                    out.sized(&self.pcrs[*n as usize]);
                }
                Ok((vec![], out.finish().unwrap()))
            }
            0x182 => {
                p.u32().unwrap();
                p.u16().unwrap();
                let digest = p.take(32).unwrap().to_vec();
                let n = handles[0] as usize;
                let new = policy::hash(HashAlg::Sha256, &[&self.pcrs[n], &digest]).unwrap();
                self.pcrs[n].copy_from_slice(&new);
                self.counter += 1;
                Ok((vec![], vec![]))
            }
            0x17A => {
                let capability = p.u32().unwrap();
                let property = p.u32().unwrap();
                out.u8(0).u32(capability);
                if capability == 0 {
                    out.u32(3).u16(0x0B).u32(0).u16(0x18).u32(0).u16(0x23).u32(0);
                } else {
                    let value = if property == 0x105 { 0x4642_4B00 } else { 3 };
                    out.u32(1).u32(property).u32(value);
                }
                Ok((vec![], out.finish().unwrap()))
            }
            _ => Err(0x143),
        }
    }
}

impl Default for FakeTpm {
    fn default() -> Self {
        FakeTpm::new()
    }
}

/// The count of handles before the parameters of each command.
fn command_handles(code: u32) -> usize {
    match code {
        0x176 => 2,
        0x131 | 0x153 | 0x157 | 0x15D | 0x177 | 0x17F | 0x16A | 0x189 | 0x182 => 1,
        _ => 0,
    }
}

impl pod_tpm::Transport for FakeTpm {
    fn submit(&mut self, command: &[u8]) -> Result<Vec<u8>, Failure> {
        let index = self.sent.len();
        self.sent.push(command.to_vec());
        let fail = self.fail_at.filter(|(at, _)| *at == index).map(|(_, f)| f);
        if fail == Some(Fail::Transport) {
            return Err(Failure::new(Stage::Host, Class::Unavailable, "the planted failure of the transport"));
        }
        let mut input = In::new(command);
        let tag = input.u16().unwrap();
        input.u32().unwrap();
        let code = input.u32().unwrap();
        let handles: Vec<u32> = (0..command_handles(code)).map(|_| input.u32().unwrap()).collect();
        let mut auth = None;
        if tag == 0x8002 {
            let size = input.u32().unwrap() as usize;
            let area = input.take(size).unwrap();
            auth = Some(u32::from_be_bytes([area[0], area[1], area[2], area[3]]));
        }
        if let Some(Fail::Tpm(rc)) = fail {
            return Ok(frame(0x8001, rc, &[]));
        }
        let (handles, params) = match self.run(code, &handles, auth, &mut input) {
            Ok(answer) => answer,
            Err(rc) => return Ok(frame(0x8001, rc, &[])),
        };
        let params = if fail == Some(Fail::Short) { Vec::new() } else { params };
        let mut body = Out::default();
        for h in &handles {
            body.u32(*h);
        }
        if tag == 0x8002 {
            body.u32(params.len() as u32).raw(&params).raw(&[0, 0, 0, 0, 0]);
        } else {
            body.raw(&params);
        }
        Ok(frame(tag, 0, &body.finish().unwrap()))
    }
}

fn frame(tag: u16, rc: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Out::default();
    out.u16(tag).u32((10 + body.len()) as u32).u32(rc).raw(body);
    out.finish().unwrap()
}

/// An entropy that gives one byte, again and again.
pub struct Bytes(pub u8);

impl Entropy for Bytes {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), NoRandom> {
        buf.fill(self.0);
        Ok(())
    }
}

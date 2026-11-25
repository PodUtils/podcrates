//! The commands of sessions, of the PCRs and of the capabilities in the
//! model: `TPM2_StartAuthSession`, `TPM2_PolicyPCR`,
//! `TPM2_PolicyAuthorize`, `TPM2_PolicyGetDigest` (part 3, clauses 11.1,
//! 23.7, 23.16 and 23.19), `TPM2_PCR_Read`, `TPM2_PCR_Extend` (22.4, 22.2)
//! and `TPM2_GetCapability` (30.2). The digest of a policy follows the
//! equations of part 3 from the bytes of the command.

use super::{Answer, PCRS, Rc, SLOTS, Session, SoftTpm, bytes, end, on_handle, on_param, sha256, short};
use crate::alg::{ALG_AES, ALG_CFB, ALG_ECC, ALG_ECDSA, ALG_NULL, ALG_SHA1, ALG_SHA256, ALG_SHA384, ALG_SHA512};
use crate::cmd::capability::{PT_HR_TRANSIENT_AVAIL, PT_MANUFACTURER};
use crate::cmd::object::ST_VERIFIED;
use crate::cmd::{CAP_ALGS, CAP_HANDLES, CAP_TPM_PROPERTIES, CC_POLICY_AUTHORIZE, CC_POLICY_PCR};
use crate::rc::Code;
use crate::wire::{In, Out, RH_NULL};

/// The algorithms of the model and their `TPMA_ALGORITHM`, in the order
/// of their ids.
const ALGORITHMS: [(u16, u32); 6] = [
    (ALG_AES, 0x0002),
    (ALG_SHA256, 0x0004),
    (ALG_NULL, 0x0000),
    (ALG_ECDSA, 0x0501),
    (ALG_ECC, 0x0009),
    (ALG_CFB, 0x0202),
];

/// `TPM_PT_HR_TRANSIENT_MIN`, `TPM_PT_HR_LOADED_MIN`, `TPM_PT_HR_LOADED`
/// and `TPM_PT_HR_LOADED_AVAIL` (part 2, 6.13).
const PT_HR_TRANSIENT_MIN: u32 = 0x10E;
const PT_HR_LOADED_MIN: u32 = 0x110;
const PT_HR_LOADED: u32 = 0x203;
const PT_HR_LOADED_AVAIL: u32 = 0x204;

/// The most items of one answer of `TPM2_GetCapability`.
const MAX_ITEMS: usize = 64;

impl SoftTpm {
    pub(super) fn start_auth_session(&mut self, handles: &[u32], p: &mut In) -> Answer {
        let nonce = p.sized().map_err(short(1))?;
        let salt = p.sized().map_err(short(2))?;
        let kind = p.u8().map_err(short(3))?;
        let symmetric = p.u16().map_err(short(4))?;
        let hash = p.u16().map_err(short(5))?;
        end(p)?;
        // The model has no salted and no bound session.
        for (n, handle) in handles.iter().enumerate() {
            if *handle != RH_NULL {
                return Err(on_handle(Code::Handle, n as u32 + 1));
            }
        }
        if !(16..=32).contains(&nonce.len()) {
            return Err(on_param(Code::Size, 1));
        }
        if !salt.is_empty() {
            return Err(on_param(Code::Value, 2));
        }
        let trial = match kind {
            0x01 => false,
            0x03 => true,
            _ => return Err(on_param(Code::Value, 3)),
        };
        if symmetric != ALG_NULL {
            return Err(on_param(Code::Symmetric, 4));
        }
        if hash != ALG_SHA256 {
            return Err(on_param(Code::Hash, 5));
        }
        if self.sessions.len() >= SLOTS {
            return Err(Code::SessionMemory.value());
        }
        let number = self.fresh();
        let handle = 0x0300_0000 | number;
        self.sessions.insert(handle, Session { trial, digest: vec![0; 32] });
        let mut out = Out::default();
        out.sized(&self.derive(b"nonce", &[&number.to_be_bytes()]));
        Ok((vec![handle], bytes(out)))
    }

    /// The session of `handle`, which the check of the handles found.
    fn session(&mut self, handle: u32) -> Result<&mut Session, Rc> {
        self.sessions.get_mut(&handle).ok_or(Code::ReferenceH0.value())
    }

    pub(super) fn policy_pcr(&mut self, handle: u32, p: &mut In) -> Answer {
        let given = p.sized().map_err(short(1))?.to_vec();
        let start = p.rest();
        let pcrs = read_selection(p)?;
        let selection = start[..start.len() - p.remaining()].to_vec();
        end(p)?;
        let values: Vec<&[u8]> = pcrs.iter().map(|n| &self.pcrs[*n][..]).collect();
        let now = sha256(&values).to_vec();
        let session = self.session(handle)?;
        if !session.trial && !given.is_empty() && given != now {
            return Err(on_param(Code::Value, 1));
        }
        let used = if session.trial && !given.is_empty() { given } else { now };
        session.digest = sha256(&[&session.digest, &CC_POLICY_PCR.to_be_bytes(), &selection, &used]).to_vec();
        Ok((vec![], vec![]))
    }

    pub(super) fn policy_authorize(&mut self, handle: u32, p: &mut In) -> Answer {
        let approved = p.sized().map_err(short(1))?.to_vec();
        let policy_ref = p.sized().map_err(short(2))?.to_vec();
        let key_sign = p.sized().map_err(short(3))?.to_vec();
        let tag = p.u16().map_err(short(4))?;
        let hierarchy = p.u32().map_err(short(4))?;
        let ticket = p.sized().map_err(short(4))?.to_vec();
        end(p)?;
        let expected = self.ticket(hierarchy, &sha256(&[&approved, &policy_ref]), &key_sign);
        let session = self.session(handle)?;
        if !session.trial {
            if session.digest != approved {
                return Err(on_param(Code::Value, 1));
            }
            if key_sign.len() != 34 || key_sign[..2] != ALG_SHA256.to_be_bytes() {
                return Err(on_param(Code::Hash, 3));
            }
            if tag != ST_VERIFIED {
                return Err(on_param(Code::Tag, 4));
            }
            // A ticket of the NULL hierarchy is empty, and never equal.
            if ticket != expected {
                return Err(on_param(Code::Value, 4));
            }
        }
        let first = sha256(&[&[0; 32], &CC_POLICY_AUTHORIZE.to_be_bytes(), &key_sign]);
        session.digest = sha256(&[&first, &policy_ref]).to_vec();
        Ok((vec![], vec![]))
    }

    pub(super) fn policy_get_digest(&mut self, handle: u32, p: &mut In) -> Answer {
        end(p)?;
        let mut out = Out::default();
        out.sized(&self.session(handle)?.digest);
        Ok((vec![], bytes(out)))
    }

    pub(super) fn pcr_read(&mut self, p: &mut In) -> Answer {
        let asked = read_banks(p)?;
        end(p)?;
        // A TPM reads 8 PCRs at most at once (TPML_DIGEST), and names the
        // PCRs that it read in each bank of the question.
        let mut values = Vec::new();
        let mut out_banks = Out::default();
        for (alg, size, bits) in &asked {
            let mut read = vec![0u8; usize::from(*size)];
            if *alg == ALG_SHA256 {
                for n in bits.iter().copied().take(8usize.saturating_sub(values.len())) {
                    read[n / 8] |= 1 << (n % 8);
                    values.push(self.pcrs[n]);
                }
            }
            out_banks.u16(*alg).u8(*size).raw(&read);
        }
        let mut out = Out::default();
        out.u32(self.counter).u32(asked.len() as u32).raw(&bytes(out_banks)).u32(values.len() as u32);
        for value in &values {
            out.sized(value);
        }
        Ok((vec![], bytes(out)))
    }

    pub(super) fn pcr_extend(&mut self, pcr: u32, p: &mut In) -> Answer {
        let count = p.count(2, 16, "digests").map_err(short(1))?;
        let mut extend = None;
        for _ in 0..count {
            let alg = p.u16().map_err(short(1))?;
            let size = match alg {
                ALG_SHA1 => 20,
                ALG_SHA256 => 32,
                ALG_SHA384 => 48,
                ALG_SHA512 => 64,
                _ => return Err(on_param(Code::Hash, 1)),
            };
            let digest = p.take(size).map_err(short(1))?;
            // The model has the SHA-256 bank alone; a digest of a bank that
            // it does not have changes nothing.
            if alg == ALG_SHA256 {
                extend = Some(digest.to_vec());
            }
        }
        end(p)?;
        let index = usize::try_from(pcr).ok().filter(|n| *n < PCRS).ok_or(on_handle(Code::Value, 1))?;
        if let Some(digest) = extend {
            self.pcrs[index] = sha256(&[&self.pcrs[index], &digest]);
        }
        self.counter = self.counter.wrapping_add(1);
        Ok((vec![], vec![]))
    }

    pub(super) fn get_capability(&mut self, p: &mut In) -> Answer {
        let capability = p.u32().map_err(short(1))?;
        let property = p.u32().map_err(short(2))?;
        let count = usize::try_from(p.u32().map_err(short(3))?).unwrap_or(MAX_ITEMS).min(MAX_ITEMS);
        end(p)?;
        let mut out = Out::default();
        match capability {
            CAP_ALGS => {
                let all: Vec<_> = ALGORITHMS.iter().filter(|(id, _)| u32::from(*id) >= property).collect();
                out.u8(u8::from(all.len() > count)).u32(capability).u32(all.len().min(count) as u32);
                for (id, attributes) in all.into_iter().take(count) {
                    out.u16(*id).u32(*attributes);
                }
            }
            CAP_HANDLES => {
                let all: Vec<u32> = match property >> 24 {
                    0x80 => self.objects.keys().copied().collect(),
                    0x81 => self.persistent.keys().copied().collect(),
                    0x02 | 0x03 => self.sessions.keys().copied().collect(),
                    _ => Vec::new(),
                };
                let all: Vec<u32> = all.into_iter().filter(|h| *h >= property).collect();
                out.u8(u8::from(all.len() > count)).u32(capability).u32(all.len().min(count) as u32);
                for handle in all.into_iter().take(count) {
                    out.u32(handle);
                }
            }
            CAP_TPM_PROPERTIES => {
                let transient = (SLOTS - self.objects.len()) as u32;
                let loaded = self.sessions.len() as u32;
                let table = [
                    (PT_MANUFACTURER, u32::from_be_bytes(*b"SOFT")),
                    (PT_HR_TRANSIENT_MIN, SLOTS as u32),
                    (PT_HR_LOADED_MIN, SLOTS as u32),
                    (PT_HR_LOADED, loaded),
                    (PT_HR_LOADED_AVAIL, SLOTS as u32 - loaded),
                    (PT_HR_TRANSIENT_AVAIL, transient),
                ];
                let all: Vec<_> = table.iter().filter(|(id, _)| *id >= property).collect();
                out.u8(u8::from(all.len() > count)).u32(capability).u32(all.len().min(count) as u32);
                for (id, value) in all.into_iter().take(count) {
                    out.u32(*id).u32(*value);
                }
            }
            _ => return Err(on_param(Code::Value, 1)),
        }
        Ok((vec![], bytes(out)))
    }
}

/// The banks of a `TPML_PCR_SELECTION`: each algorithm, the size of its
/// bit set, and the PCRs that it selects.
fn read_banks(p: &mut In) -> Result<Vec<(u16, u8, Vec<usize>)>, Rc> {
    let count = p.count(4, 16, "banks").map_err(|_| on_param(Code::Value, 2))?;
    let mut banks = Vec::with_capacity(count);
    for _ in 0..count {
        let alg = p.u16().map_err(short(2))?;
        let size = p.u8().map_err(short(2))?;
        if !(3..=4).contains(&size) {
            return Err(on_param(Code::Value, 2));
        }
        let bits = p.take(usize::from(size)).map_err(short(2))?;
        let pcrs: Vec<usize> = (0..usize::from(size) * 8).filter(|n| bits[n / 8] & (1 << (n % 8)) != 0).collect();
        if pcrs.iter().any(|n| *n >= PCRS) {
            return Err(on_param(Code::Value, 2));
        }
        banks.push((alg, size, pcrs));
    }
    Ok(banks)
}

/// The PCRs of a selection of `TPM2_PolicyPCR`, of the SHA-256 bank alone:
/// the model has no other bank.
fn read_selection(p: &mut In) -> Result<Vec<usize>, Rc> {
    let mut pcrs = Vec::new();
    for (alg, _, bits) in read_banks(p)? {
        if !bits.is_empty() && alg != ALG_SHA256 {
            return Err(on_param(Code::Value, 2));
        }
        pcrs.extend(bits);
    }
    Ok(pcrs)
}

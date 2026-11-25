//! The commands of objects in the model: `TPM2_CreatePrimary`,
//! `TPM2_Create`, `TPM2_Load`, `TPM2_LoadExternal`, `TPM2_Sign`,
//! `TPM2_VerifySignature` and `TPM2_FlushContext` (part 3, clauses 24.1,
//! 12.1 to 12.3, 20.1, 20.2 and 28.4).

use p256::ecdsa::signature::hazmat::{PrehashSigner, PrehashVerifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};

use super::{Answer, Object, Rc, SLOTS, SoftTpm, bytes, end, on_handle, on_param, sha256, short};
use crate::EccPublic;
use crate::alg::{ALG_ECDSA, ALG_NULL, ALG_SHA256};
use crate::cmd::object::ST_VERIFIED;
use crate::public::{DECRYPT, RESTRICTED, SIGN};
use crate::rc::Code;
use crate::wire::{In, Out, RH_ENDORSEMENT, RH_NULL, RH_OWNER, RH_PLATFORM};

/// `TPM_ST_CREATION`, the tag of a creation ticket.
const ST_CREATION: u16 = 0x8021;
/// The bytes of the private part of a key of the model: its check, then
/// its scalar under a key stream of the parent.
const PRIVATE: usize = 64;

/// The name of a public area (`TPMT_PUBLIC`): `TPM_ALG_SHA256` and its hash.
fn name_of(area: &[u8]) -> Vec<u8> {
    [&ALG_SHA256.to_be_bytes()[..], &sha256(&[area])].concat()
}

/// The public area of the template `area` with the point of `key` in its
/// unique field, which the template leaves empty.
fn with_point(area: &[u8], key: &SigningKey) -> Option<Vec<u8>> {
    let point = key.verifying_key().to_sec1_point(false);
    let xy = point.as_bytes().get(1..)?;
    let head = area.strip_suffix(&[0, 0, 0, 0])?;
    let mut out = Out::default();
    out.raw(head).sized(&xy[..32]).sized(&xy[32..]);
    out.finish().ok()
}

/// A scalar of P-256 from 32 bytes of the seed: the bytes, hashed again in
/// the rare case that they are no scalar.
fn key_from(mut bytes: [u8; 32]) -> SigningKey {
    loop {
        if let Ok(key) = SigningKey::from_slice(&bytes) {
            return key;
        }
        bytes = sha256(&[&bytes]);
    }
}

/// The parameters of `TPM2_CreatePrimary` and `TPM2_Create`: the public
/// area of the template (`TPMT_PUBLIC`, read as an ECC P-256 area), the
/// outside data and the selection of the creation data.
struct Template {
    area: Vec<u8>,
    public: EccPublic,
    outside: Vec<u8>,
    selection: Vec<u8>,
}

impl SoftTpm {
    /// Read the parameters of a creation. The model keeps no
    /// authorization value and no data in a key.
    fn template(&self, p: &mut In) -> Result<Template, Rc> {
        let mut sensitive = In::new(p.sized().map_err(short(1))?);
        let user_auth = sensitive.sized().map_err(short(1))?;
        let data = sensitive.sized().map_err(short(1))?;
        if !user_auth.is_empty() || !data.is_empty() || !sensitive.is_empty() {
            return Err(on_param(Code::Value, 1));
        }
        let whole = p.sized_whole().map_err(short(2))?;
        let public = EccPublic::read(whole).map_err(|_| on_param(Code::Type, 2))?;
        if public.name_alg != ALG_SHA256 {
            return Err(on_param(Code::Hash, 2));
        }
        let outside = p.sized().map_err(short(3))?.to_vec();
        let start = p.rest();
        crate::PcrSelection::read(p).map_err(|_| on_param(Code::Value, 4))?;
        let selection = start[..start.len() - p.remaining()].to_vec();
        end(p)?;
        Ok(Template { area: whole[2..].to_vec(), public, outside, selection })
    }

    /// The creation data (`TPMS_CREATION_DATA`), its hash and its ticket.
    fn creation(
        &self,
        out: &mut Out,
        t: &Template,
        parent: (u16, &[u8]),
        hierarchy: u32,
        name: &[u8],
    ) -> Result<(), Rc> {
        let selection = crate::PcrSelection::from_bytes(&t.selection).map_err(|_| on_param(Code::Value, 4))?;
        let values: Vec<&[u8]> =
            selection.pcrs(crate::HashAlg::Sha256).iter().map(|n| &self.pcrs[*n as usize][..]).collect();
        let mut data = Out::default();
        data.raw(&t.selection).sized(&sha256(&values)).u8(1).u16(parent.0);
        data.sized(parent.1).sized(parent.1).sized(&t.outside);
        let data = bytes(data);
        let hash = sha256(&[&data]);
        let ticket = self.derive(b"creation", &[&hierarchy.to_be_bytes(), &hash, name]);
        out.sized(&data).sized(&hash).u16(ST_CREATION).u32(hierarchy).sized(&ticket);
        Ok(())
    }

    /// A slot for a transient object, or `TPM_RC_OBJECT_MEMORY`.
    fn place(&mut self, object: Object) -> Result<u32, Rc> {
        if self.objects.len() >= SLOTS {
            return Err(Code::ObjectMemory.value());
        }
        let handle = 0x8000_0000 | self.fresh();
        self.objects.insert(handle, object);
        Ok(handle)
    }

    /// The storage key of the handle `parent`: its name, its secret and its
    /// hierarchy.
    fn storage(&self, parent: u32) -> Result<(Vec<u8>, [u8; 32], u32), Rc> {
        match self.objects.get(&parent).or_else(|| self.persistent.get(&parent)) {
            Some(Object::Storage { name, secret, hierarchy }) => Ok((name.clone(), *secret, *hierarchy)),
            _ => Err(on_handle(Code::Type, 1)),
        }
    }

    pub(super) fn create_primary(&mut self, hierarchy: u32, p: &mut In) -> Answer {
        if ![RH_OWNER, RH_ENDORSEMENT, RH_PLATFORM, RH_NULL].contains(&hierarchy) {
            return Err(on_handle(Code::Value, 1));
        }
        let t = self.template(p)?;
        if t.public.attributes & (RESTRICTED | DECRYPT) != RESTRICTED | DECRYPT || t.public.attributes & SIGN != 0 {
            return Err(on_param(Code::Attributes, 2));
        }
        // A TPM makes the same primary from the same template: the key
        // comes from the seed of the hierarchy and the template.
        let label = [&hierarchy.to_be_bytes()[..], &t.area].concat();
        let key = key_from(self.derive(b"primary", &[&label]));
        let area = with_point(&t.area, &key).ok_or(on_param(Code::Value, 2))?;
        let name = name_of(&area);
        let secret = self.derive(b"storage", &[&label]);
        let mut out = Out::default();
        out.sized(&area);
        self.creation(&mut out, &t, (ALG_NULL, &hierarchy.to_be_bytes()), hierarchy, &name)?;
        out.sized(&name);
        let handle = self.place(Object::Storage { name, secret, hierarchy })?;
        Ok((vec![handle], bytes(out)))
    }

    pub(super) fn create(&mut self, parent: u32, p: &mut In) -> Answer {
        let (parent_name, secret, hierarchy) = self.storage(parent)?;
        let t = self.template(p)?;
        if t.public.attributes & SIGN == 0 || t.public.attributes & (RESTRICTED | DECRYPT) != 0 {
            return Err(on_param(Code::Attributes, 2));
        }
        let number = self.fresh();
        let key = key_from(self.derive(b"key", &[&number.to_be_bytes(), &t.area]));
        let area = with_point(&t.area, &key).ok_or(on_param(Code::Value, 2))?;
        let name = name_of(&area);
        let private = wrap(&secret, &name, &key);
        let mut out = Out::default();
        out.sized(&private).sized(&area);
        self.creation(&mut out, &t, (ALG_SHA256, &parent_name), hierarchy, &name)?;
        Ok((vec![], bytes(out)))
    }

    pub(super) fn load(&mut self, parent: u32, p: &mut In) -> Answer {
        let (_, secret, _) = self.storage(parent)?;
        let private = p.sized().map_err(short(1))?.to_vec();
        let whole = p.sized_whole().map_err(short(2))?;
        end(p)?;
        let area = EccPublic::read(whole).map_err(|_| on_param(Code::Type, 2))?;
        let name = name_of(&whole[2..]);
        let key = unwrap(&secret, &name, &private).ok_or(on_param(Code::Integrity, 1))?;
        if key.verifying_key().to_sec1_point(false).as_bytes() != area.point {
            return Err(on_param(Code::Binding, 2));
        }
        let mut out = Out::default();
        out.sized(&name);
        let handle = self.place(Object::Key { area, key, name })?;
        Ok((vec![handle], bytes(out)))
    }

    pub(super) fn load_external(&mut self, p: &mut In) -> Answer {
        if !p.sized().map_err(short(1))?.is_empty() {
            return Err(on_param(Code::Size, 1));
        }
        let whole = p.sized_whole().map_err(short(2))?;
        let hierarchy = p.u32().map_err(short(3))?;
        end(p)?;
        if ![RH_OWNER, RH_ENDORSEMENT, RH_PLATFORM, RH_NULL].contains(&hierarchy) {
            return Err(on_param(Code::Hierarchy, 3));
        }
        let area = EccPublic::read(whole).map_err(|_| on_param(Code::Type, 2))?;
        if area.name_alg != ALG_SHA256 {
            return Err(on_param(Code::Hash, 2));
        }
        VerifyingKey::from_sec1_bytes(&area.point).map_err(|_| on_param(Code::EccPoint, 2))?;
        let name = name_of(&whole[2..]);
        let mut out = Out::default();
        out.sized(&name);
        let handle = self.place(Object::External { area, name, hierarchy })?;
        Ok((vec![handle], bytes(out)))
    }

    pub(super) fn sign(&mut self, key: u32, p: &mut In) -> Answer {
        let digest = p.sized().map_err(short(1))?;
        let scheme = p.u16().map_err(short(2))?;
        if scheme != ALG_NULL && (scheme != ALG_ECDSA || p.u16().map_err(short(2))? != ALG_SHA256) {
            return Err(on_param(Code::Scheme, 2));
        }
        p.u16().map_err(short(3))?;
        p.u32().map_err(short(3))?;
        p.sized().map_err(short(3))?;
        end(p)?;
        let Some(Object::Key { key, area, .. }) = self.objects.get(&key) else {
            return Err(on_handle(Code::Key, 1));
        };
        if area.attributes & SIGN == 0 {
            return Err(on_handle(Code::Key, 1));
        }
        if digest.len() != 32 {
            return Err(on_param(Code::Size, 1));
        }
        let signature: Signature = key.sign_prehash(digest).map_err(|_| on_param(Code::Value, 1))?;
        let rs = signature.to_bytes();
        let mut out = Out::default();
        out.u16(ALG_ECDSA).u16(ALG_SHA256).sized(&rs[..32]).sized(&rs[32..]);
        Ok((vec![], bytes(out)))
    }

    pub(super) fn verify_signature(&mut self, key: u32, p: &mut In) -> Answer {
        let digest = p.sized().map_err(short(1))?.to_vec();
        let alg = p.u16().map_err(short(2))?;
        if alg != ALG_ECDSA {
            return Err(on_param(Code::Scheme, 2));
        }
        let hash = p.u16().map_err(short(2))?;
        let (r, s) = (p.sized().map_err(short(2))?, p.sized().map_err(short(2))?);
        end(p)?;
        let (point, name, hierarchy) = match self.objects.get(&key) {
            Some(Object::External { area, name, hierarchy }) if area.attributes & SIGN != 0 => {
                (area.point, name.clone(), *hierarchy)
            }
            Some(Object::Key { area, name, .. }) => (area.point, name.clone(), RH_OWNER),
            _ => return Err(on_handle(Code::Key, 1)),
        };
        if hash != ALG_SHA256 {
            return Err(on_param(Code::Scheme, 2));
        }
        let fixed = [pad(r).ok_or(on_param(Code::Signature, 2))?, pad(s).ok_or(on_param(Code::Signature, 2))?].concat();
        let checked = VerifyingKey::from_sec1_bytes(&point)
            .ok()
            .zip(Signature::from_slice(&fixed).ok())
            .is_some_and(|(k, sig)| k.verify_prehash(&digest, &sig).is_ok());
        if !checked {
            return Err(on_param(Code::Signature, 2));
        }
        // Part 3 (20.1): a key of the NULL hierarchy gives a NULL ticket.
        let ticket = if hierarchy == RH_NULL { Vec::new() } else { self.ticket(hierarchy, &digest, &name).to_vec() };
        let mut out = Out::default();
        out.u16(ST_VERIFIED).u32(hierarchy).sized(&ticket);
        Ok((vec![], bytes(out)))
    }

    /// The digest of a ticket of a checked signature: a hash keyed by the
    /// seed, as a TPM's HMAC with the proof of its hierarchy.
    pub(super) fn ticket(&self, hierarchy: u32, digest: &[u8], name: &[u8]) -> [u8; 32] {
        self.derive(b"verified", &[&ST_VERIFIED.to_be_bytes(), &hierarchy.to_be_bytes(), digest, name])
    }

    pub(super) fn flush(&mut self, p: &mut In) -> Answer {
        let handle = p.u32().map_err(short(1))?;
        end(p)?;
        let gone = match handle >> 24 {
            0x80 => self.objects.remove(&handle).is_some(),
            0x02 | 0x03 => self.sessions.remove(&handle).is_some(),
            _ => false,
        };
        if !gone {
            return Err(on_param(Code::Handle, 1));
        }
        Ok((vec![], vec![]))
    }
}

/// A scalar of a `TPM2B_ECC_PARAMETER`, padded to 32 bytes.
fn pad(value: &[u8]) -> Option<[u8; 32]> {
    let zeros = value.iter().take_while(|b| **b == 0).count();
    let value = &value[zeros..];
    let mut out = [0u8; 32];
    out.get_mut(32usize.checked_sub(value.len())?..)?.copy_from_slice(value);
    Some(out)
}

/// The private part of `key` under the parent of `secret`: a check over
/// the name and the wrapped scalar, then the scalar under a key stream of
/// the parent and the name.
fn wrap(secret: &[u8; 32], name: &[u8], key: &SigningKey) -> Vec<u8> {
    let stream = sha256(&[secret, b"stream", name]);
    let scalar = key.to_bytes();
    let wrapped: Vec<u8> = scalar.iter().zip(stream).map(|(a, b)| a ^ b).collect();
    let check = sha256(&[secret, b"integrity", name, &wrapped]);
    [&check[..], &wrapped].concat()
}

/// The key of a private part, when the check holds: the same parent (the
/// same seed of the TPM) and the same public area.
fn unwrap(secret: &[u8; 32], name: &[u8], private: &[u8]) -> Option<SigningKey> {
    if private.len() != PRIVATE {
        return None;
    }
    let (check, wrapped) = private.split_at(32);
    if sha256(&[secret, b"integrity", name, wrapped]) != check {
        return None;
    }
    let stream = sha256(&[secret, b"stream", name]);
    let scalar: Vec<u8> = wrapped.iter().zip(stream).map(|(a, b)| a ^ b).collect();
    SigningKey::from_slice(&scalar).ok()
}

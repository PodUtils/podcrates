//! The public area of a TPM key (`TPMT_PUBLIC`; the Library specification,
//! part 2, clause 12.2): the two templates that the crate asks the TPM for,
//! the public area of an outside key of a policy, the reader of an ECC
//! P-256 area, and the ECDSA signature of `TPM2_Sign` (`TPMT_SIGNATURE`,
//! clause 11.3.4). The wire forms of a consumer (SSH, X.509) stay with the
//! consumer.

use crate::alg::{ALG_AES, ALG_CFB, ALG_ECC, ALG_ECDAA, ALG_ECDSA, ALG_NULL, ALG_SHA256, CURVE_P256, describe};
use crate::error::Error;
use crate::wire::{In, MAX_DIGEST, Out};

/// The bits of `TPMA_OBJECT` (part 2, clause 8.3).
pub const FIXED_TPM: u32 = 1 << 1;
pub const FIXED_PARENT: u32 = 1 << 4;
pub const SENSITIVE_DATA_ORIGIN: u32 = 1 << 5;
pub const USER_WITH_AUTH: u32 = 1 << 6;
pub const ADMIN_WITH_POLICY: u32 = 1 << 7;
pub const NO_DA: u32 = 1 << 10;
pub const RESTRICTED: u32 = 1 << 16;
pub const DECRYPT: u32 = 1 << 17;
pub const SIGN: u32 = 1 << 18;

/// The bytes of a coordinate and of `r` and `s` on P-256.
const P256_BYTES: usize = 32;

/// The attributes of the storage primary of the template H-2.
pub const STORAGE: u32 =
    FIXED_TPM | FIXED_PARENT | SENSITIVE_DATA_ORIGIN | USER_WITH_AUTH | NO_DA | RESTRICTED | DECRYPT;

/// The attributes of a signing key under a policy: made in this TPM and
/// bound to it and to its parent; it signs and does nothing else. With no
/// `userWithAuth`, the TPM takes no password for it, only its policy.
pub const SIGNING: u32 = FIXED_TPM | FIXED_PARENT | SENSITIVE_DATA_ORIGIN | ADMIN_WITH_POLICY | NO_DA | SIGN;

/// The attributes of the public area of an outside ECC key: those that
/// openssl_tpm2_engine gives it, so that both compute the same name.
pub const EXTERNAL: u32 = SIGN | DECRYPT | USER_WITH_AUTH;

/// The storage primary under the owner hierarchy that the key files name:
/// the template H-2 of the TCG (EK Credential Profile B.4.5, as the
/// Provisioning Guidance 7.5.1 changes it for storage), NIST P-256 with
/// unique fields of length zero. Each reader of a key file makes the same
/// key again from it, so the seed of the TPM alone decides it. The result
/// is a `TPM2B_PUBLIC`, whole.
pub fn storage_primary() -> Result<Vec<u8>, Error> {
    let mut area = Out::default();
    area.u16(ALG_ECC).u16(ALG_SHA256).u32(STORAGE).sized(&[]);
    area.u16(ALG_AES).u16(128).u16(ALG_CFB);
    area.u16(ALG_NULL).u16(CURVE_P256).u16(ALG_NULL);
    area.sized(&[]).sized(&[]);
    sized(area)
}

/// The template of a signing key: ECDSA with SHA-256 on P-256, whose
/// authorization is `auth_policy` alone (a digest of at most 64 bytes).
pub fn signing_key(auth_policy: &[u8]) -> Result<Vec<u8>, Error> {
    if auth_policy.len() > MAX_DIGEST {
        return Err(Error::Usage(format!("an authPolicy of {} bytes is longer than a digest", auth_policy.len())));
    }
    let mut area = Out::default();
    area.u16(ALG_ECC).u16(ALG_SHA256).u32(SIGNING).sized(auth_policy);
    area.u16(ALG_NULL).u16(ALG_ECDSA).u16(ALG_SHA256).u16(CURVE_P256).u16(ALG_NULL);
    area.sized(&[]).sized(&[]);
    sized(area)
}

/// The public area (`TPM2B_PUBLIC`, whole) of an outside ECDSA P-256 key
/// from its uncompressed point (0x04, X, Y): no scheme, no symmetric part,
/// named with SHA-256. `TPM2_LoadExternal` takes it for the check of the
/// signature of a policy key.
pub fn external_p256(point: &[u8]) -> Result<Vec<u8>, Error> {
    let (x, y) = match point {
        [4, coordinates @ ..] if coordinates.len() == 2 * P256_BYTES => coordinates.split_at(P256_BYTES),
        _ => return Err(Error::Usage("the point is not an uncompressed point of P-256".into())),
    };
    let mut area = Out::default();
    area.u16(ALG_ECC).u16(ALG_SHA256).u32(EXTERNAL).sized(&[]);
    area.u16(ALG_NULL).u16(ALG_NULL).u16(CURVE_P256).u16(ALG_NULL);
    area.sized(x).sized(y);
    sized(area)
}

fn sized(area: Out) -> Result<Vec<u8>, Error> {
    let area = area.finish()?;
    let mut out = Out::default();
    out.sized(&area);
    out.finish()
}

/// What the crate reads of the public area of an ECC P-256 key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EccPublic {
    pub name_alg: u16,
    pub attributes: u32,
    pub auth_policy: Vec<u8>,
    /// The point, uncompressed: 0x04, X, Y, each coordinate of 32 bytes.
    pub point: [u8; 65],
}

impl EccPublic {
    /// Read a `TPM2B_PUBLIC` whole, its length included. A key of another
    /// type or on another curve is refused by its name.
    pub fn read(whole: &[u8]) -> Result<EccPublic, Error> {
        let mut outer = In::new(whole);
        let area = outer.sized()?;
        outer.finish("the public area")?;
        let mut input = In::new(area);
        let kind = input.u16()?;
        if kind != ALG_ECC {
            return Err(Error::Unsupported(format!(
                "the key is of type {}, where the crate takes ECC P-256 keys",
                describe(kind)
            )));
        }
        let name_alg = input.u16()?;
        let attributes = input.u32()?;
        let auth_policy = input.sized_max(MAX_DIGEST, "an authPolicy")?.to_vec();
        // TPMS_ECC_PARMS: symmetric, scheme, curve, kdf; each a selector,
        // with its details unless it is NULL.
        if input.u16()? != ALG_NULL {
            input.take(4)?;
        }
        match input.u16()? {
            ALG_NULL => {}
            ALG_ECDAA => {
                input.take(4)?;
            }
            _ => {
                input.u16()?;
            }
        }
        let curve = input.u16()?;
        if curve != CURVE_P256 {
            return Err(Error::Unsupported(format!(
                "the key is on curve {curve:#06x}, where the crate takes NIST P-256 (0x0003)"
            )));
        }
        if input.u16()? != ALG_NULL {
            input.u16()?;
        }
        let mut point = [0u8; 65];
        point[0] = 4;
        let x = input.sized_max(P256_BYTES, "a coordinate of P-256")?;
        let y = input.sized_max(P256_BYTES, "a coordinate of P-256")?;
        input.finish("the point")?;
        // A shorter coordinate has its leading zeros cut: pad it again.
        point[1 + P256_BYTES - x.len()..1 + P256_BYTES].copy_from_slice(x);
        point[65 - y.len()..].copy_from_slice(y);
        Ok(EccPublic { name_alg, attributes, auth_policy, point })
    }
}

/// An ECDSA signature on P-256 with SHA-256: `r` and `s`, each of 32 bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcdsaSignature {
    pub r: [u8; 32],
    pub s: [u8; 32],
}

/// `TPMT_SIGNATURE` of `TPM_ALG_NULL`: the signature of the policy of a key
/// itself, in a key file.
pub const NULL_SIGNATURE: &[u8] = &[0x00, 0x10];

impl EcdsaSignature {
    /// Read a `TPMT_SIGNATURE` of ECDSA with SHA-256 from `input`. Another
    /// algorithm or hash, and an `r` or an `s` of zero or of more than 32
    /// bytes, are refused.
    pub fn read_from(input: &mut In) -> Result<EcdsaSignature, Error> {
        let alg = input.u16()?;
        if alg != ALG_ECDSA {
            return Err(Error::Unsupported(format!("the TPM signed with {}, not ECDSA", describe(alg))));
        }
        let hash = input.u16()?;
        if hash != ALG_SHA256 {
            return Err(Error::Unsupported(format!("the TPM signed with hash {}, not SHA-256", describe(hash))));
        }
        let r = scalar(input.sized()?)?;
        let s = scalar(input.sized()?)?;
        Ok(EcdsaSignature { r, s })
    }

    /// The `TPMT_SIGNATURE`, as `TPM2_VerifySignature` takes it.
    pub fn to_tpmt(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + 2 * (2 + P256_BYTES));
        for v in [ALG_ECDSA, ALG_SHA256, P256_BYTES as u16] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&self.r);
        out.extend_from_slice(&(P256_BYTES as u16).to_be_bytes());
        out.extend_from_slice(&self.s);
        out
    }
}

/// A scalar of P-256 from a `TPM2B_ECC_PARAMETER`: its leading zeros cut,
/// then padded to 32 bytes. Zero, and a value of more than 32 bytes, are
/// out of range.
fn scalar(bytes: &[u8]) -> Result<[u8; 32], Error> {
    let zeros = bytes.iter().take_while(|b| **b == 0).count();
    let value = &bytes[zeros..];
    if value.is_empty() || value.len() > P256_BYTES {
        return Err(Error::Malformed("the signature of the TPM has an r or an s out of range".into()));
    }
    let mut out = [0u8; 32];
    out[P256_BYTES - value.len()..].copy_from_slice(value);
    Ok(out)
}

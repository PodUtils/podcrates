//! The algorithm ids of TPM 2.0 that the crate uses (`TPM_ALG_ID`; the
//! Library specification, part 2, clause 6.3, and the TCG Algorithm
//! Registry), and the hash algorithms of a PCR bank, a name and a policy
//! session.

use std::fmt;

pub const ALG_RSA: u16 = 0x0001;
pub const ALG_SHA1: u16 = 0x0004;
pub const ALG_AES: u16 = 0x0006;
pub const ALG_SHA256: u16 = 0x000B;
pub const ALG_SHA384: u16 = 0x000C;
pub const ALG_SHA512: u16 = 0x000D;
pub const ALG_NULL: u16 = 0x0010;
pub const ALG_SM3_256: u16 = 0x0012;
pub const ALG_RSASSA: u16 = 0x0014;
pub const ALG_ECDSA: u16 = 0x0018;
pub const ALG_ECDAA: u16 = 0x001A;
pub const ALG_KEYEDHASH: u16 = 0x0008;
pub const ALG_ECC: u16 = 0x0023;
pub const ALG_SYMCIPHER: u16 = 0x0025;
pub const ALG_CFB: u16 = 0x0043;
/// `TPM_ECC_NIST_P256`.
pub const CURVE_P256: u16 = 0x0003;

/// A hash algorithm of a PCR bank, a name or a policy session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HashAlg {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlg {
    /// Each hash algorithm, in the order of its id.
    pub const ALL: [HashAlg; 4] = [HashAlg::Sha1, HashAlg::Sha256, HashAlg::Sha384, HashAlg::Sha512];

    /// The `TPM_ALG_ID`.
    pub fn id(self) -> u16 {
        match self {
            HashAlg::Sha1 => ALG_SHA1,
            HashAlg::Sha256 => ALG_SHA256,
            HashAlg::Sha384 => ALG_SHA384,
            HashAlg::Sha512 => ALG_SHA512,
        }
    }

    /// The algorithm of `id`, or `None` for an id that is not a hash that the
    /// crate knows.
    pub fn from_id(id: u16) -> Option<HashAlg> {
        HashAlg::ALL.into_iter().find(|a| a.id() == id)
    }

    /// The bytes of a digest.
    pub fn size(self) -> usize {
        match self {
            HashAlg::Sha1 => 20,
            HashAlg::Sha256 => 32,
            HashAlg::Sha384 => 48,
            HashAlg::Sha512 => 64,
        }
    }

    /// The name of the bank, as tpm2-tools writes it: `sha256`.
    pub fn name(self) -> &'static str {
        match self {
            HashAlg::Sha1 => "sha1",
            HashAlg::Sha256 => "sha256",
            HashAlg::Sha384 => "sha384",
            HashAlg::Sha512 => "sha512",
        }
    }
}

impl fmt::Display for HashAlg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The name of an algorithm id for a line of an error: `RSA (0x0001)`.
pub fn describe(id: u16) -> String {
    let name = match id {
        ALG_RSA => "RSA",
        ALG_SHA1 => "SHA-1",
        ALG_AES => "AES",
        ALG_KEYEDHASH => "a keyed hash",
        ALG_SHA256 => "SHA-256",
        ALG_SHA384 => "SHA-384",
        ALG_SHA512 => "SHA-512",
        ALG_NULL => "NULL",
        ALG_SM3_256 => "SM3",
        ALG_RSASSA => "RSASSA",
        ALG_ECDSA => "ECDSA",
        ALG_ECDAA => "ECDAA",
        ALG_ECC => "ECC",
        ALG_SYMCIPHER => "a symmetric cipher",
        _ => return format!("algorithm {id:#06x}"),
    };
    format!("{name} ({id:#06x})")
}

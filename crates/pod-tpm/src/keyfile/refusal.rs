//! Why a key file was refused: each rule of the reader by its name, so a
//! caller and a test tell a sealed key from a broken file.

use std::fmt;

use pod_core::Class;

/// Why the crate does not read or write a key file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The type is id-sealedkey (2.23.133.10.1.5): the crate has no
    /// `TPM2_Unseal` (call C931).
    Sealed,
    /// The type is id-importablekey (2.23.133.10.1.4): the crate has no
    /// `TPM2_Import`.
    Importable,
    /// Another type, by its OID.
    UnknownType(String),
    /// An optional field `[n]` that draft-bottomley-tpm-keys-00 does not
    /// define: it can be necessary to load the key.
    UnknownField(u8),
    /// The parent is a volatile handle (MSO 0x80), whose number changes
    /// with the order of the loads (the draft, 3.1.8).
    VolatileParent(u32),
    /// The parent is not a permanent (0x40) or a persistent (0x81) handle.
    BadParent(u32),
    /// A field is larger than its bound.
    TooLarge { field: &'static str, size: usize, max: usize },
    /// A list has more items than its bound.
    TooMany { field: &'static str, max: usize },
    /// Bytes come after the end of a structure.
    Trailing { after: &'static str, count: usize },
    /// The DER, the PEM or a rule of the draft is broken; the words say how.
    Malformed(String),
}

impl Refusal {
    /// `Unsupported` for a type or a field that the crate does not use,
    /// else `Config`: the file cannot be used.
    pub fn class(&self) -> Class {
        match self {
            Refusal::Sealed | Refusal::Importable | Refusal::UnknownType(_) | Refusal::UnknownField(_) => {
                Class::Unsupported
            }
            _ => Class::Config,
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Sealed => {
                f.write_str("it holds sealed data (id-sealedkey), and the crate has no TPM2_Unseal: it signs only")
            }
            Refusal::Importable => {
                f.write_str("it holds an importable key (id-importablekey), and the crate has no TPM2_Import")
            }
            Refusal::UnknownType(oid) => write!(f, "its type {oid} is not a key type of TPM 2.0"),
            Refusal::UnknownField(n) => {
                write!(f, "it has the field [{n}], which draft-bottomley-tpm-keys-00 does not define")
            }
            Refusal::VolatileParent(h) => {
                write!(f, "its parent {h:#010x} is a volatile handle, whose number changes with the order of loads")
            }
            Refusal::BadParent(h) => {
                write!(f, "its parent {h:#010x} is neither a permanent (0x40) nor a persistent (0x81) handle")
            }
            Refusal::TooLarge { field, size, max } => write!(f, "{field} has {size} bytes, more than {max}"),
            Refusal::TooMany { field, max } => write!(f, "{field}: more than {max}"),
            Refusal::Trailing { after, count } => {
                write!(f, "{count} byte{} after {after}", if *count == 1 { "" } else { "s" })
            }
            Refusal::Malformed(why) => f.write_str(why),
        }
    }
}

impl From<Refusal> for crate::Error {
    fn from(refusal: Refusal) -> crate::Error {
        crate::Error::KeyFile(refusal)
    }
}

//! A TPM 2.0 codec in pure Rust, for an ECDSA P-256 key that a TPM makes
//! and keeps, and that signs only while its policy holds: the PCRs hold
//! the values of a known boot state (`TPM2_PolicyPCR`), or a policy key
//! signed the boot state (`TPM2_PolicyAuthorize`).
//!
//! * [`wire`]: the frame of a command and of a response, with the bounds of
//!   an untrusted response: the size field equals the bytes that came, from
//!   10 to [`wire::MAX_RESPONSE`]; each sized buffer and each count is
//!   checked against the bytes that remain.
//! * [`rc`]: each response code of part 2 as one line, the format-zero and
//!   the format-one codes read apart; a code that the table does not know
//!   keeps its number.
//! * [`cmd`]: a builder and a reader for each of the fourteen commands;
//!   [`public`]: the two templates (the storage primary H-2 and a signing
//!   key), the public area of a policy key, the reader of an ECC P-256
//!   area and of an ECDSA signature.
//! * [`selection`]: the PCR selection of each bank; [`policy`]: the digest
//!   of `TPM2_PolicyPCR` and of `TPM2_PolicyAuthorize`, the approved policy
//!   hash, the name of a key, and a [`Policy`] of statements as a key file
//!   keeps them.
//! * [`keyfile`]: the key file `TSS2 PRIVATE KEY` of a loadable key, in DER
//!   and in PEM (draft-bottomley-tpm-keys-00), with its policy and its
//!   signed policies as statements; each size and each count of the
//!   untrusted file has a bound.
//! * [`flow`]: the sequences (the storage primary, a new signing key and
//!   its key file, a load, a signature under a policy of PCRs or a policy
//!   key, the PCRs, the capabilities), each handle in a guard that flushes
//!   it on each path.
//! * [`Transport`]: one whole command out, its whole response back;
//!   [`transact`] sends a command and reads its [`Reply`]. The transports
//!   of [`transport`]: a device of a Unix kernel and the socket of a TPM in
//!   software (feature `device`), the TBS of Windows (feature `tbs`).
//! * `soft` (feature `soft`): a TPM in software for the tests of this crate
//!   and of a consumer, a model that gives each field of each response.
//!
//! **No I/O, no environment and no global in the codec.** The codec reads
//! no random source (the caller gives each nonce), no variable and no file;
//! a transport opens the path or the address of the caller alone. The
//! default build has no C library and no tokio.

pub mod alg;
pub mod cmd;
pub mod error;
pub mod flow;
pub mod keyfile;
pub mod policy;
pub mod public;
pub mod rc;
pub mod selection;
#[cfg(feature = "soft")]
pub mod soft;
pub mod transport;
pub mod wire;

pub use alg::HashAlg;
pub use error::Error;
pub use keyfile::{AuthPolicy, KeyFile, Kind, Refusal};
pub use policy::{Authorize, Policy, Statement};
pub use public::{EccPublic, EcdsaSignature};
pub use rc::{Code, ResponseCode, Subject};
pub use selection::PcrSelection;
pub use transport::{Reply, Transport, transact};

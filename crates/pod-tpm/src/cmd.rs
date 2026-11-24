//! The commands of TPM 2.0 that a signing key under a policy needs (the
//! Library specification, part 3), each as a builder of the bytes of the
//! command and a reader of its [`Reply`](crate::Reply). No session here
//! encrypts a parameter: none of them is secret (a digest to sign, a public
//! signature, the values of the PCRs), and the private part of a key never
//! leaves the TPM unwrapped.
//!
//! The fourteen commands: `CreatePrimary`, `Create`, `Load`, `LoadExternal`,
//! `Sign`, `VerifySignature` and `FlushContext` ([`object`]);
//! `StartAuthSession`, `PolicyPCR`, `PolicyAuthorize` and `PolicyGetDigest`
//! ([`session`]); `PCR_Read` and `PCR_Extend` ([`pcr`]); `GetCapability`
//! ([`capability`]). There is no `Unseal` and no template of a data object
//! or of an Ed25519 key.

pub mod capability;
pub mod object;
pub mod pcr;
pub mod session;

pub use capability::{CAP_ALGS, CAP_HANDLES, CAP_TPM_PROPERTIES, Capability, CapabilityData, PT_MANUFACTURER};
pub use capability::{get_capability, get_manufacturer};
pub use object::{Created, Loaded, Primary, Ticket};
pub use object::{create, create_primary, flush_context, load, load_external, sign, verify_signature};
pub use pcr::{PcrValues, pcr_extend, pcr_read};
pub use session::start_auth_session;
pub use session::{SessionKind, Started, policy_authorize, policy_digest, policy_get_digest, policy_pcr};

/// The command codes (`TPM_CC`; part 2, clause 6.5.2).
pub const CC_CREATE_PRIMARY: u32 = 0x0000_0131;
pub const CC_CREATE: u32 = 0x0000_0153;
pub const CC_LOAD: u32 = 0x0000_0157;
pub const CC_SIGN: u32 = 0x0000_015D;
pub const CC_FLUSH_CONTEXT: u32 = 0x0000_0165;
pub const CC_LOAD_EXTERNAL: u32 = 0x0000_0167;
pub const CC_POLICY_AUTHORIZE: u32 = 0x0000_016A;
pub const CC_POLICY_AUTH_VALUE: u32 = 0x0000_016B;
pub const CC_START_AUTH_SESSION: u32 = 0x0000_0176;
pub const CC_VERIFY_SIGNATURE: u32 = 0x0000_0177;
pub const CC_GET_CAPABILITY: u32 = 0x0000_017A;
pub const CC_PCR_READ: u32 = 0x0000_017E;
pub const CC_POLICY_PCR: u32 = 0x0000_017F;
pub const CC_PCR_EXTEND: u32 = 0x0000_0182;
pub const CC_POLICY_GET_DIGEST: u32 = 0x0000_0189;
pub const CC_POLICY_PASSWORD: u32 = 0x0000_018C;

/// The count of handles in the response of the command `code`: one for the
/// commands that make an object or a session, else none.
pub fn response_handles(code: u32) -> usize {
    match code {
        CC_CREATE_PRIMARY | CC_LOAD | CC_LOAD_EXTERNAL | CC_START_AUTH_SESSION => 1,
        _ => 0,
    }
}

/// The name of a command code for a line of an error: `TPM2_Sign`.
pub fn name(code: u32) -> String {
    let name = match code {
        CC_CREATE_PRIMARY => "TPM2_CreatePrimary",
        CC_CREATE => "TPM2_Create",
        CC_LOAD => "TPM2_Load",
        CC_SIGN => "TPM2_Sign",
        CC_FLUSH_CONTEXT => "TPM2_FlushContext",
        CC_LOAD_EXTERNAL => "TPM2_LoadExternal",
        CC_POLICY_AUTHORIZE => "TPM2_PolicyAuthorize",
        CC_POLICY_AUTH_VALUE => "TPM2_PolicyAuthValue",
        CC_START_AUTH_SESSION => "TPM2_StartAuthSession",
        CC_VERIFY_SIGNATURE => "TPM2_VerifySignature",
        CC_GET_CAPABILITY => "TPM2_GetCapability",
        CC_PCR_READ => "TPM2_PCR_Read",
        CC_POLICY_PCR => "TPM2_PolicyPCR",
        CC_PCR_EXTEND => "TPM2_PCR_Extend",
        CC_POLICY_GET_DIGEST => "TPM2_PolicyGetDigest",
        CC_POLICY_PASSWORD => "TPM2_PolicyPassword",
        _ => return format!("the command of code {code:#010x}"),
    };
    name.to_string()
}

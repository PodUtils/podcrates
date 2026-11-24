//! The response codes of TPM 2.0 (`TPM_RC`; the Library specification,
//! part 2, clause 6.6), each as one line: the name of the code, its
//! meaning, and the handle, session or parameter that it is of. The names
//! are inside their sentence, as `tpm2_rc_decode` of tpm2-tools prints
//! them, so that a search finds them.
//!
//! A format-zero code (bit 7 clear) is a code of the TPM with no modifier;
//! with bit 8 clear it is a code of a TPM 1.2. A format-one code (bit 7 set)
//! has its number in bits 0 to 5 and the parameter, handle or session that
//! it is of in bits 6 and 8 to 11. A code with bits 16 and up set is not of
//! the TPM: a resource manager or a driver in front of it gave it.

use std::fmt;

use pod_core::Class;

/// Bit 7: the format-one codes.
const RC_FMT1: u32 = 0x080;
/// Bit 8: the format-zero codes of TPM 2.0.
const RC_VER1: u32 = 0x100;
/// Bit 6 of a format-one code: the code is of a parameter.
const RC_P: u32 = 0x040;
/// Bit 10 of a format-zero code: the vendor of the TPM defines the code.
const RC_VENDOR: u32 = 0x400;
/// Bit 11 of a format-zero code: a warning.
const RC_WARN: u32 = 0x800;

macro_rules! codes {
    ($($variant:ident = $value:expr, $name:literal, $words:literal;)*) => {
        /// A response code of part 2 without its modifier, or a code that
        /// the table does not know, with its number.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum Code {
            $(#[doc = $words] $variant,)*
            /// A code that is not in the table: of a TPM 1.2, of a vendor,
            /// of a layer in front of the TPM, or a number that part 2 does
            /// not give. It keeps the whole response code.
            Unknown(u32),
        }

        const TABLE: &[(Code, u32, &str, &str)] = &[$((Code::$variant, $value, $name, $words),)*];
    };
}

codes! {
    Initialize = 0x100, "TPM_RC_INITIALIZE", "the TPM was not started";
    Failure = 0x101, "TPM_RC_FAILURE", "the TPM is in its failure mode";
    Sequence = 0x103, "TPM_RC_SEQUENCE", "the sequence of commands is not correct";
    Private = 0x10B, "TPM_RC_PRIVATE", "a code that part 2 does not use";
    Hmac = 0x119, "TPM_RC_HMAC", "a code that part 2 does not use";
    Disabled = 0x120, "TPM_RC_DISABLED", "the command is disabled";
    Exclusive = 0x121, "TPM_RC_EXCLUSIVE", "the command needs the exclusive audit sequence";
    AuthType = 0x124, "TPM_RC_AUTH_TYPE", "the command needs an authorization of another type";
    AuthMissing = 0x125, "TPM_RC_AUTH_MISSING", "the command needs an authorization session";
    Policy = 0x126, "TPM_RC_POLICY", "the policy session is not correct for the command";
    Pcr = 0x127, "TPM_RC_PCR", "the check of the PCRs failed";
    PcrChanged = 0x128, "TPM_RC_PCR_CHANGED", "the PCRs changed after the policy checked them";
    Upgrade = 0x12D, "TPM_RC_UPGRADE", "the TPM is in the field upgrade mode";
    TooManyContexts = 0x12E, "TPM_RC_TOO_MANY_CONTEXTS", "the counter of context ids is at its maximum";
    AuthUnavailable = 0x12F, "TPM_RC_AUTH_UNAVAILABLE", "the object takes no authorization of this kind";
    Reboot = 0x130, "TPM_RC_REBOOT", "the TPM must start again";
    Unbalanced = 0x131, "TPM_RC_UNBALANCED", "the hash and the symmetric algorithm are not balanced";
    CommandSize = 0x142, "TPM_RC_COMMAND_SIZE", "the size of the command is not correct";
    CommandCode = 0x143, "TPM_RC_COMMAND_CODE", "the TPM does not know the command";
    AuthSize = 0x144, "TPM_RC_AUTHSIZE", "the size of the authorization area is not correct";
    AuthContext = 0x145, "TPM_RC_AUTH_CONTEXT", "this command takes no authorization session";
    NvRange = 0x146, "TPM_RC_NV_RANGE", "the offset and the size are out of the NV range";
    NvSize = 0x147, "TPM_RC_NV_SIZE", "the size of the NV allocation is too large";
    NvLocked = 0x148, "TPM_RC_NV_LOCKED", "the NV access is locked";
    NvAuthorization = 0x149, "TPM_RC_NV_AUTHORIZATION", "the authorization of the NV access failed";
    NvUninitialized = 0x14A, "TPM_RC_NV_UNINITIALIZED", "the NV index was not written";
    NvSpace = 0x14B, "TPM_RC_NV_SPACE", "the TPM has no NV space for the allocation";
    NvDefined = 0x14C, "TPM_RC_NV_DEFINED", "the NV index or the persistent object is defined";
    BadContext = 0x150, "TPM_RC_BAD_CONTEXT", "the context is not valid";
    CpHash = 0x151, "TPM_RC_CPHASH", "the cpHash is set or not correct";
    Parent = 0x152, "TPM_RC_PARENT", "the handle is not a valid parent";
    NeedsTest = 0x153, "TPM_RC_NEEDS_TEST", "a function of the TPM needs a self-test";
    NoResult = 0x154, "TPM_RC_NO_RESULT", "an internal function of the TPM gave no result";
    Sensitive = 0x155, "TPM_RC_SENSITIVE", "the sensitive area did not unmarshal";
    Asymmetric = 0x081, "TPM_RC_ASYMMETRIC", "the asymmetric algorithm is not supported or not correct";
    Attributes = 0x082, "TPM_RC_ATTRIBUTES", "the attributes are not consistent";
    Hash = 0x083, "TPM_RC_HASH", "the hash algorithm is not supported or not correct";
    Value = 0x084, "TPM_RC_VALUE", "a value is out of range or not correct for the context";
    Hierarchy = 0x085, "TPM_RC_HIERARCHY", "the hierarchy is not enabled or not correct";
    KeySize = 0x087, "TPM_RC_KEY_SIZE", "the key size is not supported";
    Mgf = 0x088, "TPM_RC_MGF", "the mask generation function is not supported";
    Mode = 0x089, "TPM_RC_MODE", "the mode of operation is not supported";
    Type = 0x08A, "TPM_RC_TYPE", "the type of the value is not correct";
    Handle = 0x08B, "TPM_RC_HANDLE", "the handle is not correct";
    Kdf = 0x08C, "TPM_RC_KDF", "the key derivation function is not supported";
    Range = 0x08D, "TPM_RC_RANGE", "a value is out of range";
    AuthFail = 0x08E, "TPM_RC_AUTH_FAIL", "the authorization failed and the lockout counter increased";
    Nonce = 0x08F, "TPM_RC_NONCE", "the nonce is not correct";
    Pp = 0x090, "TPM_RC_PP", "the authorization needs the physical presence";
    Scheme = 0x092, "TPM_RC_SCHEME", "the scheme is not supported";
    Size = 0x095, "TPM_RC_SIZE", "a structure has the wrong size";
    Symmetric = 0x096, "TPM_RC_SYMMETRIC", "the symmetric algorithm is not supported";
    Tag = 0x097, "TPM_RC_TAG", "a tag is not correct";
    Selector = 0x098, "TPM_RC_SELECTOR", "a union selector is not correct";
    Insufficient = 0x09A, "TPM_RC_INSUFFICIENT", "the input is too short";
    Signature = 0x09B, "TPM_RC_SIGNATURE", "the signature is not valid";
    Key = 0x09C, "TPM_RC_KEY", "the key is not correct for the command";
    PolicyFail = 0x09D, "TPM_RC_POLICY_FAIL", "the policy of the object was not met";
    Integrity = 0x09F, "TPM_RC_INTEGRITY", "the integrity check failed: the object is of another TPM or changed";
    Ticket = 0x0A0, "TPM_RC_TICKET", "the ticket is not valid";
    ReservedBits = 0x0A1, "TPM_RC_RESERVED_BITS", "a reserved bit is set";
    BadAuth = 0x0A2, "TPM_RC_BAD_AUTH", "the authorization value is not correct";
    Expired = 0x0A3, "TPM_RC_EXPIRED", "the policy expired";
    PolicyCc = 0x0A4, "TPM_RC_POLICY_CC", "the policy is for another command";
    Binding = 0x0A5, "TPM_RC_BINDING", "the public and the sensitive areas are not bound";
    Curve = 0x0A6, "TPM_RC_CURVE", "the curve is not supported";
    EccPoint = 0x0A7, "TPM_RC_ECC_POINT", "the point is not on the curve";
    FwLimited = 0x0A8, "TPM_RC_FW_LIMITED", "the secret of the firmware of the hierarchy is not available";
    SvnLimited = 0x0A9, "TPM_RC_SVN_LIMITED", "the secret of the firmware SVN of the hierarchy is not available";
    ContextGap = 0x901, "TPM_RC_CONTEXT_GAP", "the gap of the context ids is too large";
    ObjectMemory = 0x902, "TPM_RC_OBJECT_MEMORY", "the TPM has no space for another object";
    SessionMemory = 0x903, "TPM_RC_SESSION_MEMORY", "the TPM has no space for another session";
    Memory = 0x904, "TPM_RC_MEMORY", "the TPM has no shared memory left";
    SessionHandles = 0x905, "TPM_RC_SESSION_HANDLES", "the TPM has no session handle left: flush a session";
    ObjectHandles = 0x906, "TPM_RC_OBJECT_HANDLES", "the TPM has no object handle left";
    Locality = 0x907, "TPM_RC_LOCALITY", "the locality is not correct";
    Yielded = 0x908, "TPM_RC_YIELDED", "the TPM stopped the command for a time; send it again";
    Canceled = 0x909, "TPM_RC_CANCELED", "the command was canceled";
    Testing = 0x90A, "TPM_RC_TESTING", "the TPM does its self-tests; send the command again";
    ReferenceH0 = 0x910, "TPM_RC_REFERENCE_H0", "handle 1 refers to an object or a session that is not loaded";
    ReferenceH1 = 0x911, "TPM_RC_REFERENCE_H1", "handle 2 refers to an object or a session that is not loaded";
    ReferenceH2 = 0x912, "TPM_RC_REFERENCE_H2", "handle 3 refers to an object or a session that is not loaded";
    ReferenceH3 = 0x913, "TPM_RC_REFERENCE_H3", "handle 4 refers to an object or a session that is not loaded";
    ReferenceH4 = 0x914, "TPM_RC_REFERENCE_H4", "handle 5 refers to an object or a session that is not loaded";
    ReferenceH5 = 0x915, "TPM_RC_REFERENCE_H5", "handle 6 refers to an object or a session that is not loaded";
    ReferenceH6 = 0x916, "TPM_RC_REFERENCE_H6", "handle 7 refers to an object or a session that is not loaded";
    ReferenceS0 = 0x918, "TPM_RC_REFERENCE_S0", "session 1 is not loaded";
    ReferenceS1 = 0x919, "TPM_RC_REFERENCE_S1", "session 2 is not loaded";
    ReferenceS2 = 0x91A, "TPM_RC_REFERENCE_S2", "session 3 is not loaded";
    ReferenceS3 = 0x91B, "TPM_RC_REFERENCE_S3", "session 4 is not loaded";
    ReferenceS4 = 0x91C, "TPM_RC_REFERENCE_S4", "session 5 is not loaded";
    ReferenceS5 = 0x91D, "TPM_RC_REFERENCE_S5", "session 6 is not loaded";
    ReferenceS6 = 0x91E, "TPM_RC_REFERENCE_S6", "session 7 is not loaded";
    NvRate = 0x920, "TPM_RC_NV_RATE", "the TPM limits the NV writes now; try again later";
    Lockout = 0x921, "TPM_RC_LOCKOUT", "the TPM is in the lockout mode after failed authorizations";
    Retry = 0x922, "TPM_RC_RETRY", "the TPM did not start the command; send it again";
    NvUnavailable = 0x923, "TPM_RC_NV_UNAVAILABLE", "the NV memory is not available now";
}

impl Code {
    /// Each code of the table, in the order of part 2.
    pub fn all() -> impl Iterator<Item = Code> {
        TABLE.iter().map(|(c, ..)| *c)
    }

    /// The value of the code in part 2, without a modifier; the whole code
    /// for [`Code::Unknown`].
    pub fn value(self) -> u32 {
        match self {
            Code::Unknown(raw) => raw,
            known => TABLE.iter().find(|(c, ..)| *c == known).map_or(0, |(_, v, ..)| *v),
        }
    }

    /// The name of part 2 (`TPM_RC_VALUE`), or `None` for [`Code::Unknown`].
    pub fn name(self) -> Option<&'static str> {
        TABLE.iter().find(|(c, ..)| *c == self).map(|(_, _, name, _)| *name)
    }

    /// What the code means.
    pub fn words(self) -> &'static str {
        TABLE.iter().find(|(c, ..)| *c == self).map_or("an error of the TPM", |(_, _, _, words)| *words)
    }

    fn of_value(value: u32, raw: u32) -> Code {
        TABLE.iter().find(|(_, v, ..)| *v == value).map_or(Code::Unknown(raw), |(c, ..)| *c)
    }
}

/// What a format-one code is of: each number counts from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    Handle(u8),
    Session(u8),
    Parameter(u8),
}

/// A response code of a TPM, as it came.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResponseCode(u32);

impl ResponseCode {
    pub fn new(raw: u32) -> ResponseCode {
        ResponseCode(raw)
    }

    /// The code as it came, with its modifier.
    pub fn raw(self) -> u32 {
        self.0
    }

    /// The layer that gave the code: 0 for the TPM, else a resource manager
    /// or a driver in front of it (bits 16 to 31).
    pub fn layer(self) -> u32 {
        self.0 >> 16
    }

    /// Whether the code is of the format one (bit 7 set).
    pub fn is_format_one(self) -> bool {
        self.layer() == 0 && self.0 & RC_FMT1 != 0
    }

    /// Whether the code is of a TPM 1.2: a format-zero code with bit 8
    /// clear.
    pub fn is_tpm12(self) -> bool {
        self.layer() == 0 && self.0 & (RC_FMT1 | RC_VER1) == 0
    }

    /// The code without its modifier.
    pub fn code(self) -> Code {
        let raw = self.0;
        if self.layer() != 0 || self.is_tpm12() {
            return Code::Unknown(raw);
        }
        if self.is_format_one() {
            return Code::of_value(RC_FMT1 | (raw & 0x03F), raw);
        }
        if raw & RC_VENDOR != 0 {
            return Code::Unknown(raw);
        }
        Code::of_value(raw & 0xFFF, raw)
    }

    /// The parameter, handle or session that a format-one code is of; `None`
    /// for a format-zero code, or when the TPM did not name one.
    pub fn subject(self) -> Option<Subject> {
        if !self.is_format_one() {
            return None;
        }
        // N is four bits, so each number fits in a u8.
        let n = ((self.0 >> 8) & 0xF) as u8;
        if self.0 & RC_P != 0 {
            return (n != 0).then_some(Subject::Parameter(n));
        }
        if n & 0x8 != 0 {
            return (n & 0x7 != 0).then_some(Subject::Session(n & 0x7));
        }
        (n != 0).then_some(Subject::Handle(n))
    }

    /// Whether the code is a warning: the TPM was busy or short of a
    /// resource, and the command was not wrong.
    pub fn is_warning(self) -> bool {
        self.layer() == 0 && !self.is_format_one() && self.0 & (RC_VER1 | RC_WARN) == RC_VER1 | RC_WARN
    }

    /// The class of `pod_core` for a caller: `Policy` for a refusal by the
    /// policy, the lockout or the authorization; `TempFail` for a warning;
    /// `Unsupported` for an algorithm or a command that the TPM does not
    /// have; `Unavailable` for a TPM that does not run, or a layer in front
    /// of it; `Usage` for a handle that is not loaded; else `Protocol`.
    pub fn class(self) -> Class {
        use Code::*;
        if self.layer() != 0 {
            return Class::Unavailable;
        }
        if self.is_tpm12() {
            return Class::Unsupported;
        }
        match self.code() {
            PolicyFail | Policy | Pcr | PcrChanged | BadAuth | AuthFail | Lockout | AuthUnavailable | AuthMissing
            | AuthType | Expired | PolicyCc | Integrity | Ticket | Signature | NvAuthorization | Pp => Class::Policy,
            Initialize | Failure | Reboot | Upgrade | Disabled | NeedsTest | Hierarchy | FwLimited | SvnLimited => {
                Class::Unavailable
            }
            CommandCode | Asymmetric | Hash | KeySize | Mgf | Mode | Kdf | Scheme | Symmetric | Curve => {
                Class::Unsupported
            }
            ReferenceH0 | ReferenceH1 | ReferenceH2 | ReferenceH3 | ReferenceH4 | ReferenceH5 | ReferenceH6
            | ReferenceS0 | ReferenceS1 | ReferenceS2 | ReferenceS3 | ReferenceS4 | ReferenceS5 | ReferenceS6 => {
                Class::Usage
            }
            _ if self.is_warning() => Class::TempFail,
            _ => Class::Protocol,
        }
    }
}

impl fmt::Display for ResponseCode {
    /// One line: the name and the meaning, what the code is of, and its
    /// number.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let raw = self.0;
        if self.layer() != 0 {
            return write!(
                f,
                "the resource manager or the driver of the TPM (layer {}) refused the command, code {raw:#010x}",
                self.layer()
            );
        }
        if self.is_tpm12() {
            return write!(f, "an answer of a TPM 1.2, code {raw:#06x}: this crate speaks TPM 2.0");
        }
        let code = self.code();
        match code.name() {
            Some(name) => write!(f, "{name}: {}", code.words())?,
            None if !self.is_format_one() && raw & RC_VENDOR != 0 => {
                f.write_str("an error that the vendor of the TPM defines")?
            }
            None => f.write_str("an error of the TPM that part 2 does not name")?,
        }
        match self.subject() {
            Some(Subject::Parameter(n)) => write!(f, ", of parameter {n}")?,
            Some(Subject::Session(n)) => write!(f, ", of session {n}")?,
            Some(Subject::Handle(n)) => write!(f, ", of handle {n}")?,
            None => {}
        }
        write!(f, " (code {raw:#06x})")
    }
}

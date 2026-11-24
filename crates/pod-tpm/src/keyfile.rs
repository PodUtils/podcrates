//! The key file of a TPM key, `TSS2 PRIVATE KEY`, in DER and in PEM, as
//! draft-bottomley-tpm-keys-00 gives it ("ASN.1 Specification for TPM 2.0
//! Key Files"), and as openssl_tpm2_engine, tpm2-openssl and
//! go-tpm-keyfiles write it:
//!
//! ```text
//! TPMKey ::= SEQUENCE {
//!   type        OBJECT IDENTIFIER,          -- id-loadablekey 2.23.133.10.1.3
//!   emptyAuth   [0] EXPLICIT BOOLEAN OPTIONAL,
//!   policy      [1] EXPLICIT SEQUENCE OF TPMPolicy OPTIONAL,
//!   secret      [2] EXPLICIT OCTET STRING OPTIONAL,
//!   authPolicy  [3] EXPLICIT SEQUENCE OF TPMAuthPolicy OPTIONAL,
//!   description [4] EXPLICIT UTF8String OPTIONAL,
//!   rsaParent   [5] EXPLICIT BOOLEAN OPTIONAL,
//!   parent      INTEGER,
//!   pubkey      OCTET STRING,               -- TPM2B_PUBLIC
//!   privkey     OCTET STRING }              -- TPM2B_PRIVATE
//! TPMPolicy ::= SEQUENCE { commandCode [0] EXPLICIT INTEGER,
//!                          commandPolicy [1] EXPLICIT OCTET STRING }
//! TPMAuthPolicy ::= SEQUENCE { name [0] EXPLICIT UTF8String OPTIONAL,
//!                              policy [1] EXPLICIT SEQUENCE OF TPMPolicy }
//! ```
//!
//! The crate reads and writes the loadable key alone (call C931): a sealed
//! and an importable key are read and refused by their names. The file is
//! untrusted input, so the reader has a bound on each size and each count,
//! and refuses each byte that DER or the draft does not allow. The writer
//! refuses what the reader refuses, so each file that it writes reads back.

mod der;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use self::der::{BOOLEAN, INTEGER, OCTET_STRING, OID, Reader, SEQUENCE, UTF8_STRING, explicit};
use crate::cmd::CC_POLICY_AUTHORIZE;
use crate::error::Error;
use crate::policy::{Policy, Statement};
use crate::wire::In;

mod refusal;
pub use refusal::Refusal;

/// The most bytes of the DER of a key file: a key file of a TPM has some
/// hundred bytes.
pub const MAX_DER: usize = 16 * 1024;
/// The most bytes of the PEM text of a key file.
pub const MAX_PEM: usize = 24 * 1024;
/// The most bytes of `pubkey` and of `privkey`: the bound of a frame.
pub const MAX_PART: usize = 4096;
/// The most statements of a policy, and the most signed policies.
pub const MAX_STATEMENTS: usize = 8;
pub const MAX_AUTH_POLICIES: usize = 8;
/// The most bytes of the body of one statement.
pub const MAX_BODY: usize = 1024;
/// The most bytes of `description` and of the name of a signed policy.
pub const MAX_TEXT: usize = 256;

/// The most significant octet of a handle: a permanent handle (a
/// hierarchy), a persistent object and a volatile (transient) object.
pub const PERMANENT: u8 = 0x40;
pub const PERSISTENT: u8 = 0x81;
pub const VOLATILE: u8 = 0x80;

/// id-loadablekey, id-importablekey and id-sealedkey (2.23.133.10.1.3 to
/// .5), and the OID of a loadable key in the old files of
/// openssl_tpm2_engine (2.23.133.10.2).
const OID_LOADABLE: &[u8] = &[0x67, 0x81, 0x05, 0x0A, 0x01, 0x03];
const OID_IMPORTABLE: &[u8] = &[0x67, 0x81, 0x05, 0x0A, 0x01, 0x04];
const OID_SEALED: &[u8] = &[0x67, 0x81, 0x05, 0x0A, 0x01, 0x05];
const OID_LOADABLE_OLD: &[u8] = &[0x67, 0x81, 0x05, 0x0A, 0x02];

/// The guard lines of the PEM text.
pub const BEGIN: &str = "-----BEGIN TSS2 PRIVATE KEY-----";
pub const END: &str = "-----END TSS2 PRIVATE KEY-----";

/// The type of a key file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    /// A key for `TPM2_Load` (id-loadablekey).
    Loadable,
}

/// One signed policy (`TPMAuthPolicy`, the draft, 5.1): a name to show, and
/// statements that end with a signed `TPM2_PolicyAuthorize`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthPolicy {
    pub name: Option<String>,
    pub policy: Policy,
}

/// A key file of a loadable key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyFile {
    pub kind: Kind,
    /// The key needs no authorization value. The writer leaves the field out
    /// when it is false, as the draft advises.
    pub empty_auth: bool,
    /// The statements of the key's policy, in their order. With signed
    /// policies, the first is the `TPM2_PolicyAuthorize` of the policy key,
    /// with a NULL signature.
    pub policy: Policy,
    /// The signed policies, the most likely first.
    pub auth_policy: Vec<AuthPolicy>,
    pub description: Option<String>,
    /// The parent is the RSA 2048 primary (template H-1), not P-256 (H-2).
    pub rsa_parent: bool,
    /// The handle of the parent: a permanent handle (the reader makes the
    /// primary of that hierarchy again) or a persistent one.
    pub parent: u32,
    /// `TPM2B_PUBLIC`, whole.
    pub public: Vec<u8>,
    /// `TPM2B_PRIVATE`, whole.
    pub private: Vec<u8>,
}

impl KeyFile {
    /// A loadable key of `parent` with no policy, which needs no
    /// authorization value.
    pub fn loadable(parent: u32, public: Vec<u8>, private: Vec<u8>) -> KeyFile {
        KeyFile {
            kind: Kind::Loadable,
            empty_auth: true,
            policy: Policy::default(),
            auth_policy: Vec::new(),
            description: None,
            rsa_parent: false,
            parent,
            public,
            private,
        }
    }

    /// The DER of the file.
    pub fn to_der(&self) -> Result<Vec<u8>, Error> {
        self.check()?;
        let mut body = der::element(OID, OID_LOADABLE);
        if self.empty_auth {
            body.extend(der::explicit_true(0));
        }
        if !self.policy.statements.is_empty() {
            der::put(&mut body, explicit(1), &write_policy(&self.policy));
        }
        if !self.auth_policy.is_empty() {
            let mut list = Vec::new();
            for signed in &self.auth_policy {
                let mut one = Vec::new();
                if let Some(name) = &signed.name {
                    der::put(&mut one, explicit(0), &der::element(UTF8_STRING, name.as_bytes()));
                }
                der::put(&mut one, explicit(1), &write_policy(&signed.policy));
                der::put(&mut list, SEQUENCE, &one);
            }
            der::put(&mut body, explicit(3), &der::element(SEQUENCE, &list));
        }
        if let Some(text) = &self.description {
            der::put(&mut body, explicit(4), &der::element(UTF8_STRING, text.as_bytes()));
        }
        if self.rsa_parent {
            body.extend(der::explicit_true(5));
        }
        body.extend(der::integer(self.parent));
        der::put(&mut body, OCTET_STRING, &self.public);
        der::put(&mut body, OCTET_STRING, &self.private);
        let out = der::element(SEQUENCE, &body);
        if out.len() > MAX_DER {
            return Err(Refusal::TooLarge { field: "the key file", size: out.len(), max: MAX_DER }.into());
        }
        Ok(out)
    }

    /// The PEM text of the file: the guard lines, and the Base64 of the DER
    /// in lines of 64 characters.
    pub fn to_pem(&self) -> Result<String, Error> {
        let text = STANDARD.encode(self.to_der()?);
        let mut out = String::with_capacity(text.len() + text.len() / 64 + 2 * BEGIN.len());
        out.push_str(BEGIN);
        out.push('\n');
        let mut rest = text.as_str();
        while !rest.is_empty() {
            let (line, after) = rest.split_at(rest.len().min(64));
            out.push_str(line);
            out.push('\n');
            rest = after;
        }
        out.push_str(END);
        out.push('\n');
        Ok(out)
    }

    /// Read the file of `bytes`: PEM when it begins with the guard line
    /// (after white space), else DER.
    pub fn read(bytes: &[u8]) -> Result<KeyFile, Error> {
        if !is_pem(bytes) {
            return KeyFile::from_der(bytes);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| Refusal::Malformed("the PEM text is not UTF-8".into()))?;
        KeyFile::from_pem(text)
    }

    /// Read a PEM text: the first block between the guard lines; text
    /// outside the block does not count (RFC 7468, section 2).
    pub fn from_pem(text: &str) -> Result<KeyFile, Error> {
        if text.len() > MAX_PEM {
            return Err(Refusal::TooLarge { field: "the PEM text", size: text.len(), max: MAX_PEM }.into());
        }
        let bad = |why: &str| Error::from(Refusal::Malformed(why.into()));
        let (_, after) = text.split_once(BEGIN).ok_or_else(|| bad("no line -----BEGIN TSS2 PRIVATE KEY-----"))?;
        let (body, _) = after.split_once(END).ok_or_else(|| bad("no line -----END TSS2 PRIVATE KEY-----"))?;
        let body: String = body.chars().filter(|c| !matches!(c, ' ' | '\t' | '\r' | '\n')).collect();
        let der = STANDARD.decode(body).map_err(|e| bad(&format!("the body of the PEM text is not Base64: {e}")))?;
        KeyFile::from_der(&der)
    }

    /// Read the DER of a key file.
    pub fn from_der(bytes: &[u8]) -> Result<KeyFile, Error> {
        if bytes.len() > MAX_DER {
            return Err(Refusal::TooLarge { field: "the key file", size: bytes.len(), max: MAX_DER }.into());
        }
        let body = Reader::only(bytes, SEQUENCE, "the TPMKey SEQUENCE")?;
        let mut fields = Reader::new(body);
        let oid = fields.element(OID, "the type")?;
        let kind = match oid {
            OID_LOADABLE | OID_LOADABLE_OLD => Kind::Loadable,
            OID_SEALED => return Err(Refusal::Sealed.into()),
            OID_IMPORTABLE => return Err(Refusal::Importable.into()),
            other => return Err(Refusal::UnknownType(der::dotted(other)).into()),
        };
        let mut file = KeyFile::loadable(0, Vec::new(), Vec::new());
        file.kind = kind;
        file.empty_auth = false;
        let mut last = None;
        while let Some(n) = fields.peek().and_then(der::explicit_number) {
            if last.is_some_and(|before| n <= before) {
                return Err(Refusal::Malformed(format!("the field [{n}] comes twice or out of order")).into());
            }
            last = Some(n);
            let inner = fields.element(explicit(n), "an optional field")?;
            match n {
                0 => file.empty_auth = der::boolean(Reader::only(inner, BOOLEAN, "emptyAuth")?, "emptyAuth")?,
                1 => file.policy = read_policy(inner, "policy")?,
                2 => {
                    let why = "a loadable key with a secret ([2]), which the draft allows in an importable key alone";
                    return Err(Refusal::Malformed(why.into()).into());
                }
                3 => file.auth_policy = read_auth_policies(inner)?,
                4 => file.description = Some(read_text(inner, "description")?),
                5 => file.rsa_parent = der::boolean(Reader::only(inner, BOOLEAN, "rsaParent")?, "rsaParent")?,
                other => return Err(Refusal::UnknownField(other).into()),
            }
        }
        file.parent = der::unsigned(fields.element(INTEGER, "parent")?, "parent")?;
        file.public = fields.element(OCTET_STRING, "pubkey")?.to_vec();
        file.private = fields.element(OCTET_STRING, "privkey")?.to_vec();
        fields.finish("privkey")?;
        file.check()?;
        Ok(file)
    }

    /// The rules that the reader and the writer share: the bounds, the
    /// parent, the parts as whole `TPM2B`s, and the place of each
    /// `TPM2_PolicyAuthorize` (the draft, 3.1.5, 3.1.7, 3.1.8, 4.3, 5.1.2).
    fn check(&self) -> Result<(), Refusal> {
        match self.parent.to_be_bytes()[0] {
            PERMANENT | PERSISTENT => {}
            VOLATILE => return Err(Refusal::VolatileParent(self.parent)),
            _ => return Err(Refusal::BadParent(self.parent)),
        }
        if self.rsa_parent && self.parent.to_be_bytes()[0] != PERMANENT {
            return Err(Refusal::Malformed("rsaParent with a parent that is not a permanent handle".into()));
        }
        for (field, part) in [("pubkey", &self.public), ("privkey", &self.private)] {
            if part.len() > MAX_PART {
                return Err(Refusal::TooLarge { field, size: part.len(), max: MAX_PART });
            }
            let mut input = In::new(part);
            if input.sized().is_err() || !input.is_empty() {
                return Err(Refusal::Malformed(format!("{field} is not one whole TPM2B")));
            }
        }
        if let Some(text) = &self.description {
            check_text(text, "description")?;
        }
        check_policy(&self.policy, "policy")?;
        if self.policy.statements.iter().skip(1).any(|s| s.code == CC_POLICY_AUTHORIZE) {
            return Err(Refusal::Malformed("a policy with a TPM2_PolicyAuthorize after its first statement".into()));
        }
        if self.auth_policy.len() > MAX_AUTH_POLICIES {
            return Err(Refusal::TooMany { field: "signed policies", max: MAX_AUTH_POLICIES });
        }
        if self.auth_policy.is_empty() {
            return Ok(());
        }
        match self.policy.statements.first() {
            None => return Err(Refusal::Malformed("an authPolicy in a key with no policy".into())),
            Some(first) if first.code != CC_POLICY_AUTHORIZE => {
                let why = "an authPolicy in a key whose first policy statement is not TPM2_PolicyAuthorize";
                return Err(Refusal::Malformed(why.into()));
            }
            Some(_) => {}
        }
        for signed in &self.auth_policy {
            if let Some(name) = &signed.name {
                check_text(name, "the name of a signed policy")?;
            }
            check_policy(&signed.policy, "a signed policy")?;
            let steps = &signed.policy.statements;
            let Some((last, before)) = steps.split_last() else {
                return Err(Refusal::Malformed("a signed policy of the authPolicy with no statement".into()));
            };
            if last.code != CC_POLICY_AUTHORIZE {
                return Err(Refusal::Malformed("a signed policy that does not end with TPM2_PolicyAuthorize".into()));
            }
            if before.iter().skip(1).any(|s| s.code == CC_POLICY_AUTHORIZE) {
                let why = "a signed policy with a TPM2_PolicyAuthorize between its first and its last statement";
                return Err(Refusal::Malformed(why.into()));
            }
        }
        Ok(())
    }
}

/// Whether `bytes` begin as the PEM text of a key file, after white space.
pub fn is_pem(bytes: &[u8]) -> bool {
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(bytes.len());
    bytes.get(start..).is_some_and(|rest| rest.starts_with(BEGIN.as_bytes()))
}

fn check_text(text: &str, field: &'static str) -> Result<(), Refusal> {
    if text.len() > MAX_TEXT {
        return Err(Refusal::TooLarge { field, size: text.len(), max: MAX_TEXT });
    }
    Ok(())
}

fn check_policy(policy: &Policy, field: &'static str) -> Result<(), Refusal> {
    if policy.statements.len() > MAX_STATEMENTS {
        return Err(Refusal::TooMany { field, max: MAX_STATEMENTS });
    }
    for statement in &policy.statements {
        if statement.body.len() > MAX_BODY {
            let field = "the body of a policy statement";
            return Err(Refusal::TooLarge { field, size: statement.body.len(), max: MAX_BODY });
        }
    }
    Ok(())
}

/// A `SEQUENCE OF TPMPolicy`.
fn write_policy(policy: &Policy) -> Vec<u8> {
    let mut list = Vec::new();
    for statement in &policy.statements {
        let mut one = der::element(explicit(0), &der::integer(statement.code));
        der::put(&mut one, explicit(1), &der::element(OCTET_STRING, &statement.body));
        der::put(&mut list, SEQUENCE, &one);
    }
    der::element(SEQUENCE, &list)
}

/// The contents of an explicit field that holds a `SEQUENCE OF TPMPolicy`.
/// The count is checked as each statement comes, before the next is read.
fn read_policy(inner: &[u8], field: &'static str) -> Result<Policy, Refusal> {
    let mut list = Reader::new(Reader::only(inner, SEQUENCE, field)?);
    let mut statements = Vec::new();
    while !list.is_empty() {
        if statements.len() == MAX_STATEMENTS {
            return Err(Refusal::TooMany { field, max: MAX_STATEMENTS });
        }
        let mut one = Reader::new(list.element(SEQUENCE, "a TPMPolicy")?);
        let code = Reader::only(one.element(explicit(0), "commandCode")?, INTEGER, "commandCode")?;
        let code = der::unsigned(code, "commandCode")?;
        let body = Reader::only(one.element(explicit(1), "commandPolicy")?, OCTET_STRING, "commandPolicy")?;
        one.finish("a TPMPolicy")?;
        if body.len() > MAX_BODY {
            let field = "the body of a policy statement";
            return Err(Refusal::TooLarge { field, size: body.len(), max: MAX_BODY });
        }
        statements.push(Statement { code, body: body.to_vec() });
    }
    Ok(Policy::new(statements))
}

/// The contents of `[3]`: the signed policies.
fn read_auth_policies(inner: &[u8]) -> Result<Vec<AuthPolicy>, Refusal> {
    let mut list = Reader::new(Reader::only(inner, SEQUENCE, "authPolicy")?);
    let mut out = Vec::new();
    while !list.is_empty() {
        if out.len() == MAX_AUTH_POLICIES {
            return Err(Refusal::TooMany { field: "signed policies", max: MAX_AUTH_POLICIES });
        }
        let mut one = Reader::new(list.element(SEQUENCE, "a TPMAuthPolicy")?);
        let mut name = None;
        if one.peek() == Some(explicit(0)) {
            name = Some(read_text(
                one.element(explicit(0), "the name of a signed policy")?,
                "the name of a signed policy",
            )?);
        }
        let policy = read_policy(one.element(explicit(1), "the policy of a TPMAuthPolicy")?, "a signed policy")?;
        one.finish("a TPMAuthPolicy")?;
        out.push(AuthPolicy { name, policy });
    }
    Ok(out)
}

/// The contents of an explicit field that holds a UTF8String.
fn read_text(inner: &[u8], field: &'static str) -> Result<String, Refusal> {
    let bytes = Reader::only(inner, UTF8_STRING, field)?;
    if bytes.len() > MAX_TEXT {
        return Err(Refusal::TooLarge { field, size: bytes.len(), max: MAX_TEXT });
    }
    der::text(bytes, field)
}
